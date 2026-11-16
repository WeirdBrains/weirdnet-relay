//! The wake path: the hub's rules, then a device, a sender and a push service against a real
//! relay on loopback.
use std::sync::Arc;
use std::time::{Duration, Instant};
use weirdnet_net::{Keypair, Limits, Multiaddr, Node, NodeConfig, PeerId, WEIRDNET};
use weirdnet_push::api::Transport;
use weirdnet_push::relaywake::{self, Delivery, PROTOCOL, Subscription};
use weirdnet_push::server::{Config as PushConfig, RelayConfig, Service, router, token_digest};
use weirdnet_push::{Payload96, PushClient, PushKey, Reason, WakeCard, WakeHandle};
use weirdnet_relay::config::{Network, Wake};
use weirdnet_relay::store::Store;
use weirdnet_relay::wake::Hub;
use weirdnet_relay::{Config, Relay};

fn payload(b: u8) -> Payload96 {
    let mut a = [b; 96];
    a[0] = 1;
    Payload96(a)
}

#[test]
fn hub_delivers_holds_and_caps() {
    let hub = Hub::new(Wake { max_per_minute: 5, max_subscribers: 2, hold_secs: 60, ..Default::default() });
    let h = WakeHandle::generate();
    assert_eq!(hub.deliver(&h, payload(1)), Delivery::Offline, "never seen: nothing kept");
    let mut s = hub.subscribe(std::slice::from_ref(&h)).unwrap();
    assert_eq!(hub.deliver(&h, payload(2)), Delivery::Delivered);
    assert_eq!(s.rx.try_recv().unwrap(), (h.clone(), payload(2)));
    drop(s);
    assert_eq!(hub.stats().subscribers, 0);
    // Just dropped off: held for the reconnect, and handed over then.
    assert_eq!(hub.deliver(&h, payload(3)), Delivery::Held);
    let mut s = hub.subscribe(std::slice::from_ref(&h)).unwrap();
    assert_eq!(s.rx.try_recv().unwrap(), (h.clone(), payload(3)));
    assert_eq!(hub.stats().held, 0);
    // Battery protection: at most 5 a minute per handle (2 used; Offline doesn't count).
    for i in 4..7 {
        assert_eq!(hub.deliver(&h, payload(i)), Delivery::Delivered);
    }
    assert_eq!(hub.deliver(&h, payload(7)), Delivery::TooMany);
    // Limits on subscriptions.
    let _b = hub.subscribe(&[WakeHandle::generate()]).unwrap();
    assert!(hub.subscribe(&[WakeHandle::generate()]).is_err(), "max_subscribers");
    drop(s.rx.try_recv());
    assert!(hub.subscribe(&[]).is_err());
    let many: Vec<_> = (0..9).map(|_| WakeHandle::generate()).collect();
    drop(_b);
    assert!(hub.subscribe(&many).is_err(), "at most 8 handles");
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

fn relay(wake: Wake) -> (Relay, Multiaddr) {
    let base = format!("/ip4/127.0.0.1/tcp/{}", free_port());
    let cfg =
        Config { network: Network { listen: vec![base.clone()], external: vec![base.clone()], ..Default::default() }, wake, ..Default::default() };
    let relay = Relay::start(cfg, Store::open_in_memory().unwrap()).unwrap();
    let addr = format!("{base}/p2p/{}", relay.peer_id()).parse().unwrap();
    (relay, addr)
}

async fn subscribe(n: &Node<(), ()>, relay: PeerId, addr: &Multiaddr, h: &WakeHandle) -> Subscription<weirdnet_net::Stream> {
    let s = n.open_stream_async(relay, std::slice::from_ref(addr), PROTOCOL, Limits::default()).await.unwrap();
    Subscription::start(s, std::slice::from_ref(h)).await.unwrap()
}

async fn wait_subscribed(relay: &Relay, n: usize) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while relay.wake().unwrap().hub().stats().subscribers < n {
        assert!(Instant::now() < deadline, "no subscriber");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_peer_wakes_a_subscribed_phone_through_its_home_relay() {
    let (relay, addr) = relay(Wake { keepalive_secs: 10, ..Default::default() });
    assert!(relay.refresh().protocols.iter().any(|p| p == PROTOCOL));
    let phone = node();
    let key = PushKey::generate();
    let card = WakeCard { service: String::new(), handle: WakeHandle::generate(), key: hex::encode(key.public()) };
    let mut sub = subscribe(&phone, relay.peer_id(), &addr, &card.handle).await;
    assert_eq!(sub.keepalive_s, 10);

    let peer = node();
    let s = peer.open_stream_async(relay.peer_id(), std::slice::from_ref(&addr), PROTOCOL, Limits::default()).await.unwrap();
    let st = relaywake::deliver(s, &card.handle, card.seal(Reason::MessageWaiting).unwrap()).await.unwrap();
    assert_eq!(st, Delivery::Delivered);
    let (h, p) = tokio::time::timeout(Duration::from_secs(20), sub.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(h, card.handle);
    assert_eq!(key.open(&h, &p).unwrap().reason, Reason::MessageWaiting);

    // A stranger's handle is unknown here.
    let s = peer.open_stream_async(relay.peer_id(), std::slice::from_ref(&addr), PROTOCOL, Limits::default()).await.unwrap();
    assert_eq!(relaywake::deliver(s, &WakeHandle::generate(), payload(9)).await.unwrap(), Delivery::Offline);

    // The phone goes away; a wake waits for its reconnect.
    drop(sub);
    let deadline = Instant::now() + Duration::from_secs(20);
    while relay.wake().unwrap().hub().stats().subscribers > 0 {
        assert!(Instant::now() < deadline, "the relay never noticed the phone leave");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let s = peer.open_stream_async(relay.peer_id(), std::slice::from_ref(&addr), PROTOCOL, Limits::default()).await.unwrap();
    assert_eq!(relaywake::deliver(s, &card.handle, card.seal(Reason::ConnectionRequested).unwrap()).await.unwrap(), Delivery::Held);
    let mut sub = subscribe(&phone, relay.peer_id(), &addr, &card.handle).await;
    let (h, p) = tokio::time::timeout(Duration::from_secs(20), sub.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(key.open(&h, &p).unwrap().reason, Reason::ConnectionRequested);
    relay.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn wakes_from_the_push_service_reach_a_phone_on_the_relay_path() {
    let dir = tempfile::tempdir().unwrap();
    let token = WakeHandle::generate().as_str().to_string();
    let pcfg = PushConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        store: dir.path().join("push.json"),
        max_registrations: 100,
        apns: None,
        fcm: None,
        relay: Some(RelayConfig { token_digests: vec![token_digest(&token)] }),
    };
    let service = Service::new(&pcfg).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, router(service)).await.unwrap() });
    std::fs::write(dir.path().join("token"), format!("{token}\n")).unwrap();

    let (relay, addr) = relay(Wake { push: url.clone(), push_token_file: Some(dir.path().join("token")), ..Default::default() });
    let push = PushClient::new(url);
    let reg = push.register(Transport::Relay).await.unwrap();
    let key = PushKey::generate();
    let card = WakeCard { service: String::new(), handle: reg.handle.clone(), key: hex::encode(key.public()) };

    let phone = node();
    let mut sub = subscribe(&phone, relay.peer_id(), &addr, &card.handle).await;
    wait_subscribed(&relay, 1).await;
    // The relay follows the push service; wait until a wake gets through.
    let deadline = Instant::now() + Duration::from_secs(30);
    let got = loop {
        push.wake_card(&card, Reason::MessageWaiting).await.unwrap();
        if let Ok(r) = tokio::time::timeout(Duration::from_millis(500), sub.next()).await {
            break r.unwrap().unwrap();
        }
        assert!(Instant::now() < deadline, "no wake through the push service");
    };
    assert_eq!(key.open(&got.0, &got.1).unwrap().reason, Reason::MessageWaiting);
    relay.stop();
}

#[test]
fn bad_wake_configs_are_refused() {
    for w in [
        Wake { keepalive_secs: 1, ..Default::default() },
        Wake { push: "http://push.example.com".into(), push_token_file: Some("/x".into()), ..Default::default() },
        Wake { push: "https://push.example.com".into(), ..Default::default() },
        Wake { max_per_minute: 0, ..Default::default() },
    ] {
        assert!(Config { wake: w, ..Default::default() }.validate().is_err());
    }
    assert!(
        Config {
            wake: Wake { push: "https://push.example.com".into(), push_token_file: Some("/x".into()), ..Default::default() },
            ..Default::default()
        }
        .validate()
        .is_ok()
    );
}
