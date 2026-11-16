use std::time::Duration;
use weirdnet_net::RelayLimits;
use weirdnet_relay::Config;
use weirdnet_relay::config::{Overrides, Preset};

#[test]
fn defaults_are_valid_and_unlimited() {
    let c = Config::default();
    c.validate().unwrap();
    assert_eq!(c.network.port, 4747);
    assert!(c.relay_limits().is_unlimited());
    assert_eq!(c.listen_addrs().unwrap().len(), 4);
    let n = c.node_config().unwrap();
    assert!(n.relay_server && !n.memory);
}

#[test]
fn the_example_config_parses() {
    let c = Config::from_file(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/relay.toml"))).unwrap();
    c.validate().unwrap();
    assert!(!c.contact.is_empty());
}

#[test]
fn toml_limits_override_the_preset() {
    let c = Config::from_toml(
        r#"
name = "r"
[limits]
preset = "libp2p"
max_circuit_bytes = 0
max_circuit_duration_secs = 600
"#,
    )
    .unwrap();
    let l = c.relay_limits();
    assert_eq!(l.max_circuit_bytes, 0);
    assert_eq!(l.max_circuit_duration, Duration::from_secs(600));
    assert_eq!(l.max_circuits, RelayLimits::default().max_circuits);
    assert!(l.rate_limits);
}

#[test]
fn flags_win_over_the_file() {
    let mut c = Config::from_toml("name = \"file\"\n[network]\nport = 1000\n[http]\nlisten = \"127.0.0.1:1\"\n").unwrap();
    c.apply(&Overrides {
        name: Some("flag".into()),
        port: Some(2000),
        no_http: true,
        limits: Some(Preset::Libp2p),
        external: vec!["/dns4/relay.example.com/tcp/2000".into()],
        ..Default::default()
    });
    assert_eq!((c.name.as_str(), c.network.port, c.http.enabled), ("flag", 2000, false));
    assert_eq!(c.relay_limits(), RelayLimits::default());
    assert_eq!(c.network.external, vec!["/dns4/relay.example.com/tcp/2000".to_string()]);
    c.validate().unwrap();
}

#[test]
fn bad_configs_are_refused() {
    for (toml, why) in [
        ("[network]\nexternal = [\"not an addr\"]", "multiaddr"),
        ("[network]\nexternal = [\"/ip4/1.2.3.4/tcp/4747/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN\"]", "/p2p/"),
        ("[network]\nbootstrap = [\"/ip4/1.2.3.4/tcp/4747\"]", "/p2p/"),
        ("[limits]\nmax_circuits = 0", "at least 1"),
        ("[http]\nlisten = \"nope\"", "host:port"),
        ("[log]\nformat = \"xml\"", "pretty"),
        ("name = \"\"", "name"),
    ] {
        let err = Config::from_toml(toml).unwrap().validate().unwrap_err().to_string();
        assert!(err.contains(why), "{toml}: {err}");
    }
    assert!(Config::from_toml("nonsense = 1").is_err());
}

#[test]
fn the_state_dir_falls_back_to_the_default() {
    let c = Config { state_dir: Some("/tmp/x".into()), ..Default::default() };
    assert_eq!(c.db_path(), std::path::PathBuf::from("/tmp/x/relay.db"));
}
