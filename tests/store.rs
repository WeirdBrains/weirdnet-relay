use weirdnet_net::identity::{IdentityStore, load_or_create};
use weirdnet_net::{Keypair, Sighting};
use weirdnet_relay::dbkey;
use weirdnet_relay::store::Store;

#[test]
fn the_node_key_survives_a_restart_and_needs_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state/relay.db");
    let key = [7u8; 32];
    let first = {
        let s = Store::open(&db, &key).unwrap();
        load_or_create(&s).unwrap().public().to_peer_id()
    };
    let s = Store::open(&db, &key).unwrap();
    assert_eq!(load_or_create(&s).unwrap().public().to_peer_id(), first);
    assert_eq!(s.node_identity().unwrap().unwrap().1, first.to_string());
    drop(s);
    // encrypted at rest: not a plaintext SQLite file, and another key does not open it
    let head = std::fs::read(&db).unwrap();
    assert_ne!(&head[..16], b"SQLite format 3\0");
    assert!(Store::open(&db, &[8u8; 32]).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&db).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[test]
fn identities_are_stored_once() {
    let s = Store::open_in_memory().unwrap();
    s.init_node_identity(b"one", "p1").unwrap();
    s.init_node_identity(b"two", "p2").unwrap();
    assert_eq!(s.node_identity().unwrap().unwrap().1, "p1");
    assert_eq!(s.schema_version().unwrap(), 1);
}

#[test]
fn peers_are_upserted_and_pruned() {
    let s = Store::open_in_memory().unwrap();
    let peer = Keypair::generate_ed25519().public().to_peer_id();
    let a = Sighting { peer, addrs: vec!["/ip4/1.2.3.4/tcp/4747".into()], caps: None, agent: None, source: "mdns" };
    s.saw(&a, 100).unwrap();
    let b = Sighting { peer, addrs: vec![], caps: Some(vec!["relay".into()]), agent: Some("morph/0.2".into()), source: "identify" };
    s.saw(&b, 200).unwrap();
    let p = s.peer(&peer.to_string()).unwrap().unwrap();
    assert_eq!(p.addrs, vec!["/ip4/1.2.3.4/tcp/4747".to_string()]);
    assert_eq!(p.caps, Some(vec!["relay".to_string()]));
    assert_eq!((p.agent.as_deref(), p.source.as_str(), p.first_seen, p.last_seen), (Some("morph/0.2"), "identify", 100, 200));
    assert_eq!(s.peers_seen_since(150).unwrap(), 1);
    assert_eq!(s.prune_peers(150).unwrap(), 0);
    assert_eq!(s.prune_peers(250).unwrap(), 1);
    assert!(s.peer(&peer.to_string()).unwrap().is_none());
    s.set_meta("k", "v").unwrap();
    assert_eq!(s.meta("k").unwrap().as_deref(), Some("v"));
}

#[test]
fn key_files_are_hex_or_raw() {
    let hex = dbkey::generate_hex().unwrap();
    assert_eq!(hex.len(), 64);
    let k = dbkey::parse(format!("  {}\n", hex.as_str()).as_bytes()).unwrap();
    assert_eq!(hex::encode(&k[..]), hex.as_str());
    assert_eq!(*dbkey::parse(&[3u8; 32]).unwrap(), [3u8; 32]);
    assert!(dbkey::parse(b"short").is_err());
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("db-key");
    std::fs::write(&f, hex.as_bytes()).unwrap();
    assert_eq!(dbkey::locate(Some(&f)).unwrap(), f);
    assert_eq!(*dbkey::read(&f).unwrap(), *k);
}
