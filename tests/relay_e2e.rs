//! A real relay over TCP on loopback: an app reserves a slot, another reaches it through
//! the circuit with a large answer (no caps by default), and the info protocol answers.
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use weirdnet_net::{Handler, Keypair, Multiaddr, Node, NodeConfig, PeerId, WEIRDNET};
use weirdnet_relay::config::Network;
use weirdnet_relay::store::Store;
use weirdnet_relay::wire::{RELAY_PROFILE, Request, Response};
use weirdnet_relay::{Config, Relay, RelayInfo};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Get(usize);
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Blob(Vec<u8>);

struct Blobs;
impl Handler<Get, Blob> for Blobs {
    fn request(&self, _: &PeerId, Get(n): Get) -> Blob {
        Blob(vec![9; n])
    }
}

struct Nothing;
impl Handler<Request, Response> for Nothing {
    fn request(&self, _: &PeerId, _: Request) -> Response {
        Response::Error("a client".into())
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn wait(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !f() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn client() -> NodeConfig {
    NodeConfig { listen: vec![], mdns: false, ..Default::default() }
}

#[test]
fn apps_reach_each_other_through_the_relay_and_it_describes_itself() {
    let port = free_port();
    let base = format!("/ip4/127.0.0.1/tcp/{port}");
    let cfg = Config {
        name: "Loopback relay".into(),
        contact: "abuse@example.com".into(),
        network: Network { listen: vec![base.clone()], external: vec![base.clone()], ..Default::default() },
        ..Default::default()
    };
    let relay = Relay::start(cfg, Store::open_in_memory().unwrap()).unwrap();
    let info = relay.refresh();
    assert_eq!(info.addrs, vec![format!("{base}/p2p/{}", relay.peer_id())]);
    let raddr: Multiaddr = info.addrs[0].parse().unwrap();

    // B: no listener of its own, reachable only through its reservation
    let b = Node::start(&WEIRDNET, Keypair::generate_ed25519(), NodeConfig { relays: vec![raddr.clone()], ..client() }, Arc::new(Blobs)).unwrap();
    wait("reservation", || b.status().reservations.contains(&relay.peer_id().to_string()));

    let a = Node::start(&WEIRDNET, Keypair::generate_ed25519(), client(), Arc::new(Blobs)).unwrap();
    let circuit: Multiaddr = format!("{raddr}/p2p-circuit/p2p/{}", b.peer_id()).parse().unwrap();
    let Blob(v) = a.request(b.peer_id(), std::slice::from_ref(&circuit), Get(1024 * 1024)).unwrap();
    assert_eq!(v.len(), 1024 * 1024);

    // the relay saw both and wrote them down
    wait("sightings", || relay.store().peer(&b.peer_id().to_string()).unwrap().is_some());

    // the info protocol (the running binary refreshes the document every few seconds)
    relay.refresh();
    let c = Node::start(&RELAY_PROFILE, Keypair::generate_ed25519(), client(), Arc::new(Nothing)).unwrap();
    match c.request(relay.peer_id(), std::slice::from_ref(&raddr), Request::Info).unwrap() {
        Response::Info(i) => {
            assert_eq!(i.name, "Loopback relay");
            assert_eq!(i.peer_id, relay.peer_id().to_string());
            assert!(i.status.connected_peers >= 2);
            assert_eq!(RelayInfo::parse(&i.to_json()).unwrap(), *i);
        }
        other => panic!("{other:?}"),
    }
    match c.request(relay.peer_id(), std::slice::from_ref(&raddr), Request::Ping).unwrap() {
        Response::Pong { now_ms } => assert!(now_ms > 0),
        other => panic!("{other:?}"),
    }
    c.stop();
    for n in [a, b] {
        n.stop();
    }
    relay.stop();
}

/// Muninn's protocol ids, as in Muninn's `profile.rs` (`/muninn/...`; Muninn keeps them).
const MUNINN_IDS: weirdnet_net::Profile = weirdnet_net::Profile {
    name: "muninn",
    protocol: "/muninn/net/2",
    kad: "/muninn/kad/1",
    relay_key: "/muninn/relay/1",
    identify: "/muninn/id/1",
    default_port: 4741,
    ..WEIRDNET
};

#[test]
fn apps_on_their_own_protocol_ids_use_the_relay_too() {
    let port = free_port();
    let base = format!("/ip4/127.0.0.1/tcp/{port}");
    let cfg = Config { network: Network { listen: vec![base.clone()], external: vec![base.clone()], ..Default::default() }, ..Default::default() };
    let relay = Relay::start(cfg, Store::open_in_memory().unwrap()).unwrap();
    let raddr: Multiaddr = relay.refresh().addrs[0].parse().unwrap();

    let b = Node::start(&MUNINN_IDS, Keypair::generate_ed25519(), NodeConfig { relays: vec![raddr.clone()], ..client() }, Arc::new(Blobs)).unwrap();
    wait("reservation", || b.status().reservations.contains(&relay.peer_id().to_string()));
    let a = Node::start(&MUNINN_IDS, Keypair::generate_ed25519(), client(), Arc::new(Blobs)).unwrap();
    let circuit: Multiaddr = format!("{raddr}/p2p-circuit/p2p/{}", b.peer_id()).parse().unwrap();
    let Blob(v) = a.request(b.peer_id(), std::slice::from_ref(&circuit), Get(256 * 1024)).unwrap();
    assert_eq!(v.len(), 256 * 1024);
    a.stop();
    b.stop();
    relay.stop();
}

#[test]
fn the_relay_keeps_signed_peer_records_for_lookups() {
    use weirdnet_net::names::{Entry, Lookup};
    let port = free_port();
    let base = format!("/ip4/127.0.0.1/tcp/{port}");
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("relay.db");
    let key = [5u8; 32];
    let cfg = Config { network: Network { listen: vec![base.clone()], external: vec![base.clone()], ..Default::default() }, ..Default::default() };
    let relay = Relay::start(cfg.clone(), Store::open(&db, &key).unwrap()).unwrap();
    let raddr: Multiaddr = relay.refresh().addrs[0].parse().unwrap();

    let user = Keypair::generate_ed25519();
    let uid = weirdnet_net::user::user_id(&user.public());
    let bkey = Keypair::generate_ed25519();
    let b = Node::start(&WEIRDNET, bkey.clone(), NodeConfig { relays: vec![raddr.clone()], ..client() }, Arc::new(Blobs)).unwrap();
    wait("reservation", || b.status().reservations.contains(&relay.peer_id().to_string()));
    let now = weirdnet_net::now_ms();
    let cert = WEIRDNET.certify(&user, &uid, &b.peer_id().to_string(), "phone", now).unwrap();
    let circuit: Multiaddr = format!("{raddr}/p2p-circuit/p2p/{}", b.peer_id()).parse().unwrap();
    let rec = WEIRDNET.peer_record(&bkey, &user.public(), &cert, &[circuit], Some(&raddr), now as u64, now, 3_600_000).unwrap();
    assert_eq!(b.register_everywhere(&Entry::Record(rec.clone())), 1);
    assert_eq!(relay.store().names_count().unwrap(), 1);

    let a =
        Node::start(&WEIRDNET, Keypair::generate_ed25519(), NodeConfig { names_servers: vec![raddr.clone()], ..client() }, Arc::new(Blobs)).unwrap();
    assert_eq!(a.lookup(&uid), Lookup::Found(rec.clone()));
    let (peer, Blob(v)) = a.request_user(&uid, Get(4096)).unwrap();
    assert_eq!((peer, v.len()), (b.peer_id(), 4096));

    // the entry survives a restart of the relay
    a.stop();
    b.stop();
    relay.stop();
    let relay = Relay::start(cfg, Store::open(&db, &key).unwrap()).unwrap();
    assert_eq!(weirdnet_net::names::NameStore::get(relay.store(), &uid), Some(Entry::Record(rec)));
    relay.stop();
}
