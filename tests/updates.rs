//! Update announcements: the board's trust rules and persistence, then a publisher, a device
//! and a following relay against real relays on loopback.
use std::sync::Arc;
use std::time::{Duration, Instant};
use weirdnet_net::{Keypair, Limits, Multiaddr, Node, NodeConfig, PeerId, WEIRDNET};
use weirdnet_relay::config::{Network, UpdateRoot, Updates};
use weirdnet_relay::store::Store;
use weirdnet_relay::updates::Board;
use weirdnet_relay::{Config, Relay};
use weirdnet_update::announce::wire::{self, PROTOCOL, Subscription};
use weirdnet_update::announce::{Offered, Topic};
use weirdnet_update::manifest::Labels;
use weirdnet_update::{Announcement, Delegation, Domain, FileEntry, Root, RootPin, Signed, SigningKey, Timestamp, UpdateKind, UpdateManifest};

const DAY: u64 = 86_400;
const APP: &str = "org.weirdbrains.morph";

fn key(n: u8) -> SigningKey {
    SigningKey::from_seed(&[n; 32])
}

struct Org {
    release: SigningKey,
    env: Signed,
    pin: RootPin,
}

fn org(seed: u8) -> Org {
    let roots = [key(seed), key(seed + 1)];
    let release = key(seed + 10);
    let root = Root {
        version: 1,
        org: "weirdbrains".into(),
        keys: roots.iter().map(|k| k.public()).collect(),
        threshold: 2,
        release: vec![Delegation { key: release.public(), apps: vec![APP.into()], channels: vec![], expires: None }],
        release_threshold: 1,
        timestamp: vec![],
        timestamp_threshold: 0,
        expires: None,
    };
    let env = Signed::sign(Domain::Root, &root, &[&roots[0], &roots[1]]).unwrap();
    Org { release, env, pin: root.pin() }
}

fn announcement(o: &Org, seq: u64) -> Signed {
    let now = Timestamp::now();
    let m = UpdateManifest {
        app: APP.into(),
        channel: "stable".into(),
        update_id: format!("u{seq}"),
        seq,
        kind: UpdateKind::Assets,
        version: semver::Version::new(1, 0, seq),
        base_releases: vec![],
        files: vec![FileEntry::describe("ui/main.rfw", b"hello")],
        min_client: None,
        max_client: None,
        capabilities: vec![],
        labels: Labels::default(),
        targeting: None,
        rollout: None,
        rollback_of: None,
        notes: String::new(),
        created: now,
        expires: now.plus(7 * DAY),
    };
    let m = Signed::sign(Domain::Manifest, &m, &[&o.release]).unwrap();
    let a = Announcement::for_manifest("weirdbrains", o.pin, &m, now, DAY, vec!["https://updates.weirdbrains.com/morph".into()]).unwrap();
    a.sign(&[&o.release]).unwrap()
}

/// `[updates]` trusting `o`, with its root written to `dir`.
fn updates_for(o: &Org, dir: &std::path::Path) -> Updates {
    let f = dir.join(format!("{}.root", o.pin));
    std::fs::write(&f, o.env.to_bytes().unwrap()).unwrap();
    Updates { roots: vec![UpdateRoot { pin: o.pin.to_string(), chain: vec![f] }], ..Default::default() }
}

#[test]
fn the_board_keeps_the_latest_for_trusted_orgs_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let o = org(1);
    let cfg = updates_for(&o, dir.path());
    let store = Arc::new(Store::open_in_memory().unwrap());
    let board = Board::new(&cfg, store.clone()).unwrap();
    assert!(board.carries());
    let a1 = announcement(&o, 1);
    assert_eq!(board.offer(&a1).unwrap(), Offered::New);
    assert_eq!(board.offer(&a1).unwrap(), Offered::Same);
    let a2 = announcement(&o, 2);
    assert_eq!(board.offer(&a2).unwrap(), Offered::New);
    assert_eq!(board.offer(&a1).unwrap(), Offered::Stale, "an older seq never replaces a newer one");
    // Another org isn't carried, and a forgery for a carried one is refused.
    assert!(board.offer(&announcement(&org(30), 1)).is_err(), "untrusted org");
    let a: Announcement = a2.decode().unwrap();
    assert!(board.offer(&a.sign(&[&key(99)]).unwrap()).is_err(), "forged");
    assert_eq!(board.len(), 1);
    // A restart refills the board from the database.
    let again = Board::new(&cfg, store).unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again.offer(&a2).unwrap(), Offered::Same);
    // A relay with no roots carries nothing.
    let none = Board::new(&Updates::default(), Arc::new(Store::open_in_memory().unwrap())).unwrap();
    assert!(!none.carries());
    assert!(none.offer(&a2).is_err());
}

#[test]
fn a_bad_root_config_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let o = org(1);
    let mut cfg = updates_for(&o, dir.path());
    cfg.roots[0].pin = org(30).pin.to_string();
    assert!(Board::new(&cfg, Arc::new(Store::open_in_memory().unwrap())).is_err(), "the chain doesn't match the pin");
    let mut cfg = updates_for(&o, dir.path());
    cfg.roots[0].chain = vec![dir.path().join("missing")];
    assert!(Board::new(&cfg, Arc::new(Store::open_in_memory().unwrap())).is_err());
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

struct Nothing;
impl weirdnet_net::Handler<(), ()> for Nothing {
    fn request(&self, _: &PeerId, _: ()) {}
}

fn node() -> Node<(), ()> {
    Node::start(&WEIRDNET, Keypair::generate_ed25519(), NodeConfig { listen: vec![], mdns: false, ..Default::default() }, Arc::new(Nothing)).unwrap()
}

fn relay(updates: Updates) -> (Relay, Multiaddr) {
    let base = format!("/ip4/127.0.0.1/tcp/{}", free_port());
    let cfg =
        Config { network: Network { listen: vec![base.clone()], external: vec![base.clone()], ..Default::default() }, updates, ..Default::default() };
    let relay = Relay::start(cfg, Store::open_in_memory().unwrap()).unwrap();
    let addr = format!("{base}/p2p/{}", relay.peer_id()).parse().unwrap();
    (relay, addr)
}

async fn subscribe(n: &Node<(), ()>, relay: PeerId, addr: &Multiaddr, topics: Vec<Topic>) -> Subscription<weirdnet_net::Stream> {
    let s = n.open_stream_async(relay, std::slice::from_ref(addr), PROTOCOL, Limits::default()).await.unwrap();
    Subscription::start(s, topics, false).await.unwrap()
}

async fn next_announcement(sub: &mut Subscription<weirdnet_net::Stream>) -> Signed {
    loop {
        let env = tokio::time::timeout(Duration::from_secs(20), sub.next()).await.expect("an announcement in time").unwrap().expect("open");
        if env.domain == Domain::Announcement {
            return env;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn announcements_reach_devices_here_and_through_a_following_relay() {
    let dir = tempfile::tempdir().unwrap();
    let o = org(1);
    let (a_relay, a_addr) = relay(updates_for(&o, dir.path()));
    let topic = Topic { pin: o.pin, app: APP.into(), channel: "stable".into() };

    // A device subscribes to Morph stable on relay A.
    let device = node();
    let mut sub = subscribe(&device, a_relay.peer_id(), &a_addr, vec![topic.clone()]).await;

    // A publisher publishes; an untrusted org's announcement is refused in the same batch.
    let publisher = node();
    let a1 = announcement(&o, 1);
    let s = publisher.open_stream_async(a_relay.peer_id(), std::slice::from_ref(&a_addr), PROTOCOL, Limits::default()).await.unwrap();
    let res = wire::publish(s, vec![a1.clone(), announcement(&org(30), 1)]).await.unwrap();
    assert_eq!(res[0], Ok(Offered::New));
    assert!(res[1].is_err());
    assert_eq!(next_announcement(&mut sub).await, a1);

    // Relay B follows A and hands the latest to a device that subscribes there.
    let mut b_cfg = updates_for(&o, dir.path());
    b_cfg.upstream = vec![a_addr.to_string()];
    let (b_relay, b_addr) = relay(b_cfg);
    let deadline = Instant::now() + Duration::from_secs(30);
    while b_relay.updates().unwrap().board().is_empty() {
        assert!(Instant::now() < deadline, "relay B never heard from A");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let phone = node();
    let mut far = subscribe(&phone, b_relay.peer_id(), &b_addr, vec![topic.clone()]).await;
    assert_eq!(next_announcement(&mut far).await, a1);

    // A newer one published at A reaches both devices; a device on another channel hears nothing.
    let quiet = node();
    let mut beta = subscribe(&quiet, a_relay.peer_id(), &a_addr, vec![Topic { channel: "beta".into(), ..topic }]).await;
    let a2 = announcement(&o, 2);
    let s = publisher.open_stream_async(a_relay.peer_id(), std::slice::from_ref(&a_addr), PROTOCOL, Limits::default()).await.unwrap();
    assert_eq!(wire::publish(s, vec![a2.clone()]).await.unwrap(), vec![Ok(Offered::New)]);
    assert_eq!(next_announcement(&mut sub).await, a2);
    assert_eq!(next_announcement(&mut far).await, a2);
    assert!(tokio::time::timeout(Duration::from_secs(2), next_announcement(&mut beta)).await.is_err(), "beta isn't told about stable");
    assert!(a_relay.info_handle().get().updates);
}

#[test]
fn updates_config_is_checked() {
    let ok = |u: Updates| Config { updates: u, ..Default::default() }.validate();
    assert!(ok(Updates::default()).is_ok());
    assert!(ok(Updates { upstream: vec!["/ip4/127.0.0.1/tcp/4001".into()], ..Default::default() }).is_err(), "needs /p2p/");
    assert!(ok(Updates { roots: vec![UpdateRoot { pin: "abc".into(), chain: vec!["r".into()] }], ..Default::default() }).is_err());
    assert!(ok(Updates { roots: vec![UpdateRoot { pin: "ab".repeat(32), chain: vec![] }], ..Default::default() }).is_err(), "no root file");
    assert!(ok(Updates { keepalive_secs: 1, ..Default::default() }).is_err());
}
