//! WSS and site forwarding on a real relay over loopback: a client reaches the relay over
//! WebSocket-over-TLS, a host registers a site, and a revoked site stops being forwarded.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use weirdnet_net::ingress::{IngressGrant, Registration, StaticBinding};
use weirdnet_net::user::user_id;
use weirdnet_net::{Handler, Keypair, Multiaddr, Node, NodeConfig, PeerId, WEIRDNET, WssTls};
use weirdnet_relay::config::{Forwarding, Network, Wss};
use weirdnet_relay::relay::start_forwarding;
use weirdnet_relay::store::Store;
use weirdnet_relay::wire::{RELAY_PROFILE, Request, Response};
use weirdnet_relay::{Config, Relay};

struct Nothing;
impl Handler<Request, Response> for Nothing {
    fn request(&self, _: &PeerId, _: Request) -> Response {
        Response::Error("a client".into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Q;
#[derive(Debug, Clone, Serialize, Deserialize)]
struct R;
struct H;
impl Handler<Q, R> for H {
    fn request(&self, _: &PeerId, _: Q) -> R {
        R
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn base(port: u16) -> Config {
    let a = format!("/ip4/127.0.0.1/tcp/{port}");
    Config {
        name: "Loopback relay".into(),
        network: Network { listen: vec![a.clone()], external: vec![a], ..Default::default() },
        ..Default::default()
    }
}

#[test]
fn clients_reach_the_relay_over_websocket_tls() {
    let dir = tempfile::tempdir().unwrap();
    let c = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    std::fs::write(dir.path().join("fullchain.pem"), c.cert.pem()).unwrap();
    std::fs::write(dir.path().join("privkey.pem"), c.key_pair.serialize_pem()).unwrap();
    let wss_port = free_port();
    let cfg = Config {
        wss: Wss {
            enabled: true,
            name: "localhost".into(),
            cert: Some(dir.path().join("fullchain.pem")),
            key: Some(dir.path().join("privkey.pem")),
            port: wss_port,
            ..Default::default()
        },
        ..base(free_port())
    };
    assert!(cfg.node_config().unwrap().external.iter().any(|a| a.to_string() == format!("/dns/localhost/tcp/{wss_port}/tls/ws")));
    let relay = Relay::start(cfg, Store::open_in_memory().unwrap()).unwrap();
    let info = relay.refresh();
    assert!(info.transports.contains(&"websocket".to_string()), "{info:?}");
    let wss: Multiaddr = format!("/dns4/localhost/tcp/{wss_port}/tls/ws/p2p/{}", relay.peer_id()).parse().unwrap();
    let trust = WssTls { trust: vec![c.cert.der().to_vec()], ..Default::default() };
    let client =
        Node::start(&RELAY_PROFILE, Keypair::generate_ed25519(), NodeConfig { wss: Some(trust), ..Default::default() }, Arc::new(Nothing)).unwrap();
    match client.request(relay.peer_id(), &[wss], Request::Ping).unwrap() {
        Response::Pong { .. } => {}
        r => panic!("{r:?}"),
    }
}

#[test]
fn a_bad_wss_config_is_refused() {
    let mut cfg = Config { wss: Wss { enabled: true, ..Default::default() }, ..base(1) };
    assert!(cfg.validate().unwrap_err().to_string().contains("wss.name"));
    cfg.wss.name = "relay.example.com".into();
    cfg.wss.cert = Some("/nonexistent".into());
    assert!(cfg.validate().unwrap_err().to_string().contains("cert"));
    cfg.wss.key = Some("/nonexistent".into());
    cfg.network.external.clear();
    assert!(cfg.validate().unwrap_err().to_string().contains("external"));
    let mut cfg = base(1);
    cfg.forwarding = Forwarding { enabled: true, https: "nope".into(), ..Default::default() };
    assert!(cfg.validate().is_err());
    cfg.forwarding = Forwarding { enabled: true, local: [("bad name!".to_string(), "127.0.0.1:1".to_string())].into(), ..Default::default() };
    assert!(cfg.validate().unwrap_err().to_string().contains("forwarding.local"));
}

#[test]
fn a_site_registers_and_a_revoked_one_is_dropped() {
    let mut cfg = base(free_port());
    cfg.forwarding = Forwarding { enabled: true, https: "127.0.0.1:0".into(), http: String::new(), ..Default::default() };
    // the relay's own ingress needs DNS for bindings; here one with a fixed table
    let store = Store::open_in_memory().unwrap();
    let relay = Relay::start_opts(cfg.clone(), store, false).unwrap();
    let user = Keypair::generate_ed25519();
    let uid = user_id(&user.public());
    let binding = StaticBinding(HashMap::from([("site.example.com".to_string(), uid.clone()), ("bad.example.com".to_string(), uid.clone())]));
    let ingress = start_forwarding(&cfg, relay.node(), relay.store(), Arc::new(binding)).unwrap();
    assert!(relay.refresh().forwarding);

    let host = Node::start(&WEIRDNET, Keypair::generate_ed25519(), NodeConfig::default(), Arc::new(H)).unwrap();
    let now = weirdnet_net::now_ms();
    let cert = WEIRDNET.certify(&user, &uid, &host.peer_id().to_string(), "host", now).unwrap();
    let grants =
        ["site.example.com", "bad.example.com"].iter().map(|n| IngressGrant::sign(&user, n, &relay.peer_id(), now, 3_600_000).unwrap()).collect();
    let raddr: Multiaddr = relay.refresh().addrs[0].parse().unwrap();
    let r = host.register_ingress(relay.peer_id(), &[raddr], &Registration { grants, cert, addrs: vec![] }).unwrap();
    assert_eq!(r.accepted.len(), 2, "{r:?}");
    assert_eq!(ingress.sites().len(), 2);

    // `weirdnet-relay revoke-site bad.example.com` writes the store; the relay syncs
    relay.store().revoke_site("bad.example.com", "abuse report", now).unwrap();
    for n in relay.store().revoked_sites().unwrap() {
        ingress.revoke(&n.name);
    }
    let names: Vec<String> = ingress.sites().into_iter().map(|s| s.name).collect();
    assert_eq!(names, vec!["site.example.com".to_string()]);
    assert!(relay.store().unrevoke_site("bad.example.com").unwrap());
    assert!(relay.store().revoked_sites().unwrap().is_empty());
    std::thread::sleep(Duration::from_millis(10));
}

#[test]
fn behind_a_proxy_the_relay_takes_plain_websocket_on_loopback() {
    let port = free_port();
    let mut cfg = base(free_port());
    cfg.wss = Wss { enabled: true, name: "localhost".into(), local_port: port, port: 443, ..Default::default() };
    cfg.validate().unwrap();
    assert_eq!(cfg.wss_listen()[0].to_string(), format!("/ip4/127.0.0.1/tcp/{port}/ws"));
    let relay = Relay::start(cfg, Store::open_in_memory().unwrap()).unwrap();
    // announced as WSS on 443 (the proxy's), reachable here as plain WebSocket
    assert!(relay.refresh().addrs.iter().any(|a| a.starts_with("/dns/localhost/tcp/443/tls/ws/p2p/")));
    let ws: Multiaddr = format!("/ip4/127.0.0.1/tcp/{port}/ws/p2p/{}", relay.peer_id()).parse().unwrap();
    let client = Node::start(&RELAY_PROFILE, Keypair::generate_ed25519(), NodeConfig::default(), Arc::new(Nothing)).unwrap();
    assert!(matches!(client.request(relay.peer_id(), &[ws], Request::Ping).unwrap(), Response::Pong { .. }));
}
