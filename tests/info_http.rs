use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use weirdnet_net::{Keypair, Status};
use weirdnet_relay::relay::InfoHandle;
use weirdnet_relay::{Config, RelayInfo, http};

fn sample() -> RelayInfo {
    let peer = Keypair::generate_ed25519().public().to_peer_id();
    let cfg = Config {
        name: "Test <relay>".into(),
        contact: "abuse@example.com".into(),
        network: weirdnet_relay::config::Network {
            external: vec!["/dns4/relay.example.com/tcp/4747".into(), "/dns4/relay.example.com/udp/4747/quic-v1".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    let status = Status { nat: "public".into(), connected: vec!["a".into(), "b".into()], ..Default::default() };
    RelayInfo::build(&cfg, &peer, &status, &["/ip4/10.0.0.1/tcp/4747".into()], 5)
}

#[test]
fn the_document_announces_external_addresses_with_the_peer_id() {
    let i = sample();
    assert_eq!(i.v, 1);
    assert_eq!(i.addrs.len(), 2);
    assert!(i.addrs.iter().all(|a| a.ends_with(&format!("/p2p/{}", i.peer_id))));
    assert_eq!(i.transports, vec!["tcp".to_string(), "quic-v1".to_string()]);
    assert_eq!(i.network.kad, "/weirdnet/kad/1");
    assert_eq!(i.network.relay_key, "/weirdnet/relay/1");
    // weirdnet-net 0.8: names and the relay's own requests moved to /2 with the new codec
    assert_eq!(i.network.info_protocol, "/weirdnet/relay/info/2");
    assert!(i.protocols.iter().any(|p| p == "/weirdnet/names/2"));
    assert!(!i.protocols.iter().any(|p| p.ends_with("names/1") || p.ends_with("info/1")));
    assert_eq!((i.limits.max_circuit_bytes, i.limits.max_circuit_duration_secs, i.limits.max_circuits), (None, None, None));
    assert!(!i.limits.rate_limits);
    assert_eq!(i.status.connected_peers, 2);
    let back = RelayInfo::parse(&i.to_json()).unwrap();
    assert_eq!(back, i);
    assert_eq!(back.relay_addrs().unwrap().len(), 2);
}

#[test]
fn documents_that_lie_are_refused() {
    let mut i = sample();
    i.v = 2;
    assert!(RelayInfo::parse(&i.to_json()).is_err());
    let mut i = sample();
    i.addrs.push("/ip4/1.2.3.4/tcp/1".into());
    assert!(RelayInfo::parse(&i.to_json()).is_err());
    let mut i = sample();
    i.peer_id = "nope".into();
    assert!(RelayInfo::parse(&i.to_json()).is_err());
}

async fn get(h: &InfoHandle, path: &str) -> (StatusCode, Option<String>, Option<String>, String) {
    let app = http::router(h.clone());
    let r = app.oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
    let ct = r.headers().get("content-type").map(|v| v.to_str().unwrap().to_string());
    let cors = r.headers().get("access-control-allow-origin").map(|v| v.to_str().unwrap().to_string());
    let status = r.status();
    let body = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
    (status, ct, cors, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn the_info_page_serves_html_json_and_health() {
    let h = InfoHandle::new(sample());
    let (s, ct, cors, body) = get(&h, "/.well-known/weirdnet-relay.json").await;
    assert_eq!((s, ct.as_deref(), cors.as_deref()), (StatusCode::OK, Some("application/json"), Some("*")));
    let info = RelayInfo::parse(&body).unwrap();
    assert_eq!(info.name, "Test <relay>");

    let (s, ct, _, body) = get(&h, "/").await;
    assert_eq!(s, StatusCode::OK);
    assert!(ct.unwrap().starts_with("text/html"));
    assert!(body.contains("Test &lt;relay&gt;") && !body.contains("Test <relay>"));
    assert!(body.contains(&info.peer_id));
    assert!(body.contains("mailto:abuse@example.com"));
    assert!(body.contains("no limit"));

    let (s, _, _, body) = get(&h, "/healthz").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("\"ok\":true"));

    let (s, _, _, _) = get(&h, "/nope").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
