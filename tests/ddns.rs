//! Dynamic DNS: the records a relay wants, rounds against an in-memory zone (create, no-op,
//! address change, Grindvakt's refusal to guess), the config checks, and the DigitalOcean
//! provider built from the config against a mock API.
use grindvakt_core::ip::Discovered;
use grindvakt_core::provider::DnsProvider;
use grindvakt_core::record::{ExistingRecord, RecordKind, RecordValue};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use weirdnet_net::{Keypair, PeerId};
use weirdnet_relay::Config;
use weirdnet_relay::config::{Ddns, Wss};
use weirdnet_relay::ddns::{self, Status};

#[derive(Default)]
struct Zone {
    records: Mutex<Vec<ExistingRecord>>,
    calls: AtomicUsize,
}

impl DnsProvider for Zone {
    fn name(&self) -> &str {
        "memory"
    }
    async fn list(&self, _z: &str, name: &str, kind: RecordKind) -> grindvakt_core::Result<Vec<ExistingRecord>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.records.lock().unwrap().iter().filter(|r| r.name == name && r.value.kind() == kind).cloned().collect())
    }
    async fn create(&self, _z: &str, name: &str, value: &RecordValue, ttl: u32) -> grindvakt_core::Result<()> {
        let mut z = self.records.lock().unwrap();
        let id = z.len() as u64 + 1;
        z.push(ExistingRecord { id, name: name.into(), value: value.clone(), ttl: Some(ttl) });
        Ok(())
    }
    async fn update(&self, _z: &str, id: u64, value: &RecordValue, ttl: u32) -> grindvakt_core::Result<()> {
        for r in self.records.lock().unwrap().iter_mut().filter(|r| r.id == id) {
            r.value = value.clone();
            r.ttl = Some(ttl);
        }
        Ok(())
    }
}

impl Zone {
    fn values(&self, name: &str) -> Vec<String> {
        let mut v: Vec<String> = self.records.lock().unwrap().iter().filter(|r| r.name == name).map(|r| r.value.content()).collect();
        v.sort();
        v
    }
}

fn cfg(name: &str) -> Config {
    Config {
        ddns: Ddns { enabled: true, provider: "digitalocean".into(), zone: "example.com".into(), name: name.into(), ..Default::default() },
        ..Default::default()
    }
}

fn found(v4: &str, v6: Option<&str>) -> Discovered {
    Discovered { v4: Some(v4.parse().unwrap()), v6: v6.map(|s| s.parse().unwrap()) }
}

fn peer() -> PeerId {
    Keypair::generate_ed25519().public().to_peer_id()
}

#[test]
fn the_records_a_relay_wants() {
    let p = peer();
    let c = cfg("relay");
    let want = ddns::wanted(&c, &p, &found("203.0.114.7", Some("2001:db8::7"))).unwrap();
    let shown: Vec<String> = want.iter().map(|r| format!("{} {} {}", r.kind(), r.name, r.value.content())).collect();
    assert_eq!(
        shown,
        vec![
            "A relay.example.com 203.0.114.7".to_string(),
            "AAAA relay.example.com 2001:db8::7".to_string(),
            format!("TXT _dnsaddr.relay.example.com dnsaddr=/dns/relay.example.com/tcp/4747/p2p/{p}"),
            format!("TXT _dnsaddr.relay.example.com dnsaddr=/dns/relay.example.com/udp/4747/quic-v1/p2p/{p}"),
        ]
    );
    assert!(want.iter().all(|r| r.ttl == 300));
    // WSS on the same name, IPv4 only, no dnsaddr, the zone apex.
    let mut c = cfg("@");
    c.ddns.ipv6 = false;
    c.wss = Wss { enabled: true, name: "example.com".into(), ..Default::default() };
    let want = ddns::wanted(&c, &p, &found("203.0.114.7", Some("2001:db8::7"))).unwrap();
    assert_eq!(want.iter().filter(|r| r.kind() == RecordKind::Aaaa).count(), 0);
    assert!(want.iter().any(|r| r.value.content() == format!("dnsaddr=/dns/example.com/tcp/443/tls/ws/p2p/{p}")));
    assert_eq!(want[0].name, "example.com");
    c.ddns.dnsaddr = false;
    assert_eq!(ddns::wanted(&c, &p, &found("203.0.114.7", None)).unwrap().len(), 1);
    // Nothing to publish: no IPv4 found and IPv6 not kept.
    assert!(ddns::wanted(&c, &p, &Discovered { v4: None, v6: Some("2001:db8::7".parse().unwrap()) }).is_err());
    // Names given in full, or with a trailing dot.
    assert_eq!(Ddns { zone: "Example.com.".into(), name: "relay.example.com".into(), ..Default::default() }.fqdn(), "relay.example.com");
}

#[tokio::test]
async fn rounds_create_skip_and_follow_the_address() {
    let (zone, c, p) = (Zone::default(), cfg("relay"), peer());
    let status = Mutex::new(Status::default());
    let a = found("203.0.114.7", None);
    ddns::round(&zone, &c, &p, &status, true, || async { Ok(a) }).await;
    assert_eq!(zone.values("relay"), vec!["203.0.114.7"]);
    assert_eq!(zone.values("_dnsaddr.relay").len(), 2);
    let s = status.lock().unwrap().clone();
    assert_eq!((s.published, s.changes, s.last_error), (Some(a), 3, None));
    // Same address, no recheck due: the zone isn't asked at all.
    let calls = zone.calls.load(Ordering::SeqCst);
    ddns::round(&zone, &c, &p, &status, false, || async { Ok(a) }).await;
    assert_eq!(zone.calls.load(Ordering::SeqCst), calls);
    // A recheck plans again and finds nothing to change.
    ddns::round(&zone, &c, &p, &status, true, || async { Ok(a) }).await;
    assert_eq!(status.lock().unwrap().changes, 3);
    // A new address updates the A record in place; the dnsaddr records name the host, so
    // they stay.
    let b = found("203.0.114.9", None);
    ddns::round(&zone, &c, &p, &status, false, || async { Ok(b) }).await;
    assert_eq!(zone.values("relay"), vec!["203.0.114.9"]);
    assert_eq!(zone.records.lock().unwrap().len(), 3);
    assert_eq!(status.lock().unwrap().changes, 4);
    // No public address (CGNAT): nothing is written and the status says why.
    ddns::round(&zone, &c, &p, &status, true, || async { Err(grindvakt_core::Error::NoGlobalAddress("CGNAT".into())) }).await;
    assert!(status.lock().unwrap().last_error.as_deref().unwrap().contains("no public address"));
    assert_eq!(zone.values("relay"), vec!["203.0.114.9"]);
}

#[tokio::test]
async fn an_ambiguous_zone_is_left_alone() {
    let zone = Zone::default();
    for (id, ip) in [(1, "198.51.100.1"), (2, "198.51.100.2")] {
        zone.records.lock().unwrap().push(ExistingRecord { id, name: "relay".into(), value: RecordValue::A(ip.parse().unwrap()), ttl: Some(300) });
    }
    let status = Mutex::new(Status::default());
    ddns::round(&zone, &cfg("relay"), &peer(), &status, true, || async { Ok(found("203.0.114.7", None)) }).await;
    let s = status.lock().unwrap().clone();
    assert!(s.last_error.as_deref().unwrap().contains("refusing"), "{s:?}");
    assert_eq!(s.published, None);
    assert_eq!(zone.values("relay"), vec!["198.51.100.1", "198.51.100.2"], "nothing written");
    assert!(zone.values("_dnsaddr.relay").is_empty(), "the whole plan is refused");
}

#[test]
fn ddns_config_is_checked() {
    let ok = |f: fn(&mut Ddns)| {
        let mut c = cfg("relay");
        f(&mut c.ddns);
        c.validate()
    };
    assert!(ok(|_| {}).is_ok());
    assert!(Config::default().validate().is_ok(), "off by default");
    assert!(ok(|d| d.provider = "route53".into()).is_err());
    assert!(ok(|d| d.zone = String::new()).is_err());
    assert!(ok(|d| d.zone = "localhost".into()).is_err());
    assert!(ok(|d| d.name = "bad name".into()).is_err());
    assert!(ok(|d| d.name = "@".into()).is_ok());
    assert!(
        ok(|d| {
            d.ipv4 = false;
            d.ipv6 = false;
        })
        .is_err()
    );
    assert!(ok(|d| d.ttl = 10).is_err());
    assert!(ok(|d| d.interval_secs = 5).is_err());
    assert!(ok(|d| d.api = "http://dns.example.com".into()).is_err());
    assert!(ok(|d| d.api = "http://127.0.0.1:9".into()).is_ok());
}

#[test]
fn the_token_file_is_required_and_trimmed() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = cfg("relay");
    c.ddns.token_file = Some(dir.path().join("missing"));
    assert!(ddns::provider(&c.ddns).is_err());
    let f = dir.path().join("token");
    std::fs::write(&f, "\n").unwrap();
    c.ddns.token_file = Some(f.clone());
    assert!(ddns::provider(&c.ddns).is_err(), "empty");
    std::fs::write(&f, "do_secret\n").unwrap();
    let dns = ddns::provider(&c.ddns).unwrap();
    assert!(!format!("{dns:?}").contains("do_secret"), "the token never shows in debug output");
}

/// DigitalOcean's records API, as much as Grindvakt uses, checking the bearer token.
#[tokio::test]
async fn the_configured_provider_writes_through_the_api() {
    use axum::extract::{Path, State};
    use axum::http::{HeaderMap, StatusCode, Uri};
    use axum::routing::{get, put};
    use axum::{Json, Router};
    use serde_json::{Value, json};
    type Db = Arc<Mutex<Vec<Value>>>;
    fn authed(h: &HeaderMap) -> bool {
        h.get("authorization").and_then(|v| v.to_str().ok()) == Some("Bearer do_secret")
    }
    async fn list(State(db): State<Db>, h: HeaderMap, uri: Uri) -> Result<Json<Value>, StatusCode> {
        if !authed(&h) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let q: std::collections::HashMap<String, String> =
            uri.query().unwrap_or("").split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), v.replace("%2E", "."))).collect();
        let recs: Vec<Value> = db
            .lock()
            .unwrap()
            .iter()
            .filter(|r| {
                q.get("type").is_none_or(|t| r["type"] == *t)
                    && q.get("name").is_none_or(|n| r["name"] == *n || format!("{}.example.com", r["name"].as_str().unwrap()) == *n)
            })
            .cloned()
            .collect();
        Ok(Json(json!({ "domain_records": recs, "links": {} })))
    }
    async fn create(State(db): State<Db>, h: HeaderMap, Json(mut b): Json<Value>) -> Result<(StatusCode, Json<Value>), StatusCode> {
        if !authed(&h) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let mut d = db.lock().unwrap();
        b["id"] = json!(d.len() + 1);
        d.push(b.clone());
        Ok((StatusCode::CREATED, Json(json!({ "domain_record": b }))))
    }
    async fn update(State(db): State<Db>, h: HeaderMap, Path((_, id)): Path<(String, u64)>, Json(b): Json<Value>) -> Result<Json<Value>, StatusCode> {
        if !authed(&h) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let mut d = db.lock().unwrap();
        let r = d.iter_mut().find(|r| r["id"] == id).ok_or(StatusCode::NOT_FOUND)?;
        for (k, v) in b.as_object().unwrap() {
            r[k] = v.clone();
        }
        Ok(Json(json!({ "domain_record": r })))
    }
    let db: Db = Arc::default();
    let app = Router::new()
        .route("/v2/domains/{zone}/records", get(list).post(create))
        .route("/v2/domains/{zone}/records/{id}", put(update).patch(update))
        .with_state(db.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });

    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("token");
    std::fs::write(&f, "do_secret\n").unwrap();
    let mut c = cfg("relay");
    c.ddns.token_file = Some(f);
    c.ddns.api = api;
    c.validate().unwrap();
    let dns = ddns::provider(&c.ddns).unwrap();
    let p = peer();
    let want = ddns::wanted(&c, &p, &found("203.0.114.7", None)).unwrap();
    let dry = ddns::sync(&dns, "example.com", &want, true).await.unwrap();
    assert_eq!(dry.changes().count(), 3);
    assert!(db.lock().unwrap().is_empty(), "a dry run writes nothing");
    ddns::sync(&dns, "example.com", &want, false).await.unwrap();
    assert_eq!(db.lock().unwrap().len(), 3);
    assert!(ddns::sync(&dns, "example.com", &want, false).await.unwrap().is_noop(), "a second sync changes nothing");
    let moved = ddns::wanted(&c, &p, &found("203.0.114.9", None)).unwrap();
    ddns::sync(&dns, "example.com", &moved, false).await.unwrap();
    let d = db.lock().unwrap();
    assert_eq!(d.len(), 3);
    assert!(d.iter().any(|r| r["type"] == "A" && r["data"] == "203.0.114.9"), "{d:?}");
}
