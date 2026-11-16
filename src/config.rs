//! Configuration: a TOML file, then `WEIRDNET_RELAY_*` environment variables and flags
//! (applied as [`Overrides`]) on top. Flags win over the environment, which wins over the
//! file, which wins over the defaults.
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use weirdnet_net::{Multiaddr, NodeConfig, RelayLimits};

/// The default port (TCP and QUIC), Weirdnet's.
pub const DEFAULT_PORT: u16 = 4747;
/// Where the info page listens by default.
pub const DEFAULT_HTTP: &str = "127.0.0.1:4780";
/// The state directory when neither the config nor systemd (`$STATE_DIRECTORY`) names one.
pub const DEFAULT_STATE_DIR: &str = "/var/lib/weirdnet-relay";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The relay's name on its info page and in identify.
    pub name: String,
    /// Who runs it.
    pub operator: String,
    /// Where to send abuse reports and questions.
    pub contact: String,
    /// A line about the relay.
    pub description: String,
    /// Where `relay.db` lives (default: `$STATE_DIRECTORY`, else `/var/lib/weirdnet-relay`).
    pub state_dir: Option<PathBuf>,
    /// The database key file (default: the systemd credential `db-key`).
    pub db_key_file: Option<PathBuf>,
    pub network: Network,
    pub limits: Limits,
    pub http: Http,
    pub wss: Wss,
    pub forwarding: Forwarding,
    pub log: Log,
}

/// WebSocket over TLS, for clients on networks that only let HTTPS out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Wss {
    pub enabled: bool,
    /// The relay's DNS name, the one the certificate is for.
    pub name: String,
    /// The certificate chain and key (PEM), e.g. certbot's `fullchain.pem` and `privkey.pem`.
    /// Without them the relay takes plain WebSocket on `127.0.0.1:<local_port>` from a TLS
    /// reverse proxy (Caddy, nginx) that holds the certificate.
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
    /// The public port.
    pub port: u16,
    /// The loopback port behind a proxy, or behind forwarding (443 shared by server name).
    pub local_port: u16,
}

/// Public HTTPS forwarding by SNI passthrough for hosts without a public address.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Forwarding {
    pub enabled: bool,
    /// Where HTTPS comes in.
    pub https: String,
    /// Where plain HTTP comes in (ACME HTTP-01 for registered names, redirects for the rest);
    /// empty: not at all.
    pub http: String,
    /// Names served on this machine itself, passed untouched to a local address: the info
    /// page's TLS proxy, say (`"relay.example.com" = "127.0.0.1:8443"`).
    pub local: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Network {
    /// TCP and QUIC port when `listen` is empty.
    pub port: u16,
    /// Listen addresses (default: TCP and QUIC on every interface, IPv4 and IPv6).
    pub listen: Vec<String>,
    /// Addresses others reach this relay at (DNS names or public IPs), without `/p2p/`.
    pub external: Vec<String>,
    /// Other relays or bootstrap nodes, with `/p2p/`.
    pub bootstrap: Vec<String>,
    /// mDNS on the local network.
    pub mdns: bool,
}

/// Relay limits. Nothing is capped unless set (`preset = "unlimited"`); `preset = "libp2p"`
/// starts from libp2p's defaults. Any field set here overrides the preset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub preset: Preset,
    pub max_reservations: Option<usize>,
    pub max_reservations_per_peer: Option<usize>,
    pub reservation_duration_secs: Option<u64>,
    pub max_circuits: Option<usize>,
    pub max_circuits_per_peer: Option<usize>,
    pub max_circuit_duration_secs: Option<u64>,
    /// Bytes per circuit and direction; 0: no limit.
    pub max_circuit_bytes: Option<u64>,
    pub rate_limits: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    #[default]
    Unlimited,
    Libp2p,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Http {
    pub enabled: bool,
    pub listen: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Log {
    /// `pretty` or `json`.
    pub format: String,
    /// A `tracing` filter (`info`, `weirdnet_relay=debug,info`, …).
    pub level: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            name: "Weirdnet relay".into(),
            operator: String::new(),
            contact: String::new(),
            description: String::new(),
            state_dir: None,
            db_key_file: None,
            network: Network::default(),
            limits: Limits::default(),
            http: Http::default(),
            wss: Wss::default(),
            forwarding: Forwarding::default(),
            log: Log::default(),
        }
    }
}

impl Default for Network {
    fn default() -> Self {
        Network { port: DEFAULT_PORT, listen: vec![], external: vec![], bootstrap: vec![], mdns: false }
    }
}

impl Default for Http {
    fn default() -> Self {
        Http { enabled: true, listen: DEFAULT_HTTP.into() }
    }
}

impl Default for Wss {
    fn default() -> Self {
        Wss { enabled: false, name: String::new(), cert: None, key: None, port: 443, local_port: 4743 }
    }
}

impl Default for Forwarding {
    fn default() -> Self {
        Forwarding { enabled: false, https: "[::]:443".into(), http: "[::]:80".into(), local: Default::default() }
    }
}

impl Default for Log {
    fn default() -> Self {
        Log { format: "pretty".into(), level: "info".into() }
    }
}

/// Values from flags or the environment; `None` keeps the file's.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub name: Option<String>,
    pub operator: Option<String>,
    pub contact: Option<String>,
    pub state_dir: Option<PathBuf>,
    pub db_key_file: Option<PathBuf>,
    pub port: Option<u16>,
    pub listen: Vec<String>,
    pub external: Vec<String>,
    pub bootstrap: Vec<String>,
    pub mdns: Option<bool>,
    pub http_listen: Option<String>,
    pub no_http: bool,
    pub limits: Option<Preset>,
    pub log_format: Option<String>,
    pub log_level: Option<String>,
}

fn multiaddrs(what: &str, v: &[String]) -> Result<Vec<Multiaddr>> {
    v.iter().map(|s| s.trim().parse::<Multiaddr>().map_err(|e| Error::Config(format!("{what}: `{s}` is not a multiaddr ({e})")))).collect()
}

fn has_peer(a: &Multiaddr) -> bool {
    a.iter().any(|p| p.tag() == "p2p")
}

impl Config {
    /// Read a TOML file.
    pub fn from_file(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::Config(format!("cannot read {}: {e}", path.display())))?;
        Config::from_toml(&text)
    }

    pub fn from_toml(text: &str) -> Result<Config> {
        toml::from_str(text).map_err(|e| Error::Config(e.to_string()))
    }

    /// Apply flags and environment values.
    pub fn apply(&mut self, o: &Overrides) {
        let set = |dst: &mut String, v: &Option<String>| {
            if let Some(v) = v {
                *dst = v.clone();
            }
        };
        set(&mut self.name, &o.name);
        set(&mut self.operator, &o.operator);
        set(&mut self.contact, &o.contact);
        if o.state_dir.is_some() {
            self.state_dir = o.state_dir.clone();
        }
        if o.db_key_file.is_some() {
            self.db_key_file = o.db_key_file.clone();
        }
        if let Some(p) = o.port {
            self.network.port = p;
        }
        for (dst, src) in
            [(&mut self.network.listen, &o.listen), (&mut self.network.external, &o.external), (&mut self.network.bootstrap, &o.bootstrap)]
        {
            if !src.is_empty() {
                *dst = src.clone();
            }
        }
        if let Some(m) = o.mdns {
            self.network.mdns = m;
        }
        set(&mut self.http.listen, &o.http_listen);
        if o.no_http {
            self.http.enabled = false;
        }
        if let Some(p) = o.limits {
            self.limits.preset = p;
        }
        set(&mut self.log.format, &o.log_format);
        set(&mut self.log.level, &o.log_level);
    }

    /// Check everything a start would trip over.
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() || self.name.len() > 80 {
            return Err(Error::Config("name: 1 to 80 characters".into()));
        }
        self.listen_addrs()?;
        for a in self.external_addrs()? {
            if has_peer(&a) {
                return Err(Error::Config(format!("external: `{a}` must not carry /p2p/ (the relay adds its own)")));
            }
        }
        for a in self.bootstrap_addrs()? {
            if !has_peer(&a) {
                return Err(Error::Config(format!("bootstrap: `{a}` needs /p2p/<peer id>")));
            }
        }
        self.relay_limits().check()?;
        if self.http.enabled {
            self.http_addr()?;
        }
        if self.wss.enabled {
            if weirdnet_net::ingress::sni::host_name(self.wss.name.as_bytes()).is_none() {
                return Err(Error::Config("wss.name: the relay's DNS name (the certificate's)".into()));
            }
            if self.wss.cert.is_some() != self.wss.key.is_some() {
                return Err(Error::Config("wss: `cert` and `key` go together (or neither, behind a TLS proxy)".into()));
            }
            if self.network.external.is_empty() {
                return Err(Error::Config("wss: set network.external too (the relay then announces exactly those and the WSS address)".into()));
            }
        }
        if self.forwarding.enabled {
            self.forwarding_addrs()?;
            self.forwarding_local()?;
        }
        if !matches!(self.log.format.as_str(), "pretty" | "json") {
            return Err(Error::Config("log.format: `pretty` or `json`".into()));
        }
        Ok(())
    }

    pub fn listen_addrs(&self) -> Result<Vec<Multiaddr>> {
        if self.network.listen.is_empty() { Ok(NodeConfig::default_listen(self.network.port)) } else { multiaddrs("listen", &self.network.listen) }
    }

    pub fn external_addrs(&self) -> Result<Vec<Multiaddr>> {
        multiaddrs("external", &self.network.external)
    }

    pub fn bootstrap_addrs(&self) -> Result<Vec<Multiaddr>> {
        multiaddrs("bootstrap", &self.network.bootstrap)
    }

    /// Where forwarding takes HTTPS and HTTP.
    pub fn forwarding_addrs(&self) -> Result<(SocketAddr, Option<SocketAddr>)> {
        let f = &self.forwarding;
        let https = f.https.parse().map_err(|_| Error::Config(format!("forwarding.https: `{}` is not host:port", f.https)))?;
        let http = if f.http.trim().is_empty() {
            None
        } else {
            Some(f.http.parse().map_err(|_| Error::Config(format!("forwarding.http: `{}` is not host:port", f.http)))?)
        };
        Ok((https, http))
    }

    /// The names forwarding passes to local addresses (the relay's own WSS among them).
    pub fn forwarding_local(&self) -> Result<Vec<(String, SocketAddr)>> {
        let mut v = vec![];
        for (n, a) in &self.forwarding.local {
            let addr = a.parse().map_err(|_| Error::Config(format!("forwarding.local: `{a}` is not host:port")))?;
            let name = weirdnet_net::ingress::sni::host_name(n.as_bytes())
                .ok_or_else(|| Error::Config(format!("forwarding.local: `{n}` is not a host name")))?;
            v.push((name, addr));
        }
        if self.wss.enabled && self.wss.cert.is_some() && !v.iter().any(|(n, _)| n.eq_ignore_ascii_case(&self.wss.name)) {
            v.push((self.wss.name.to_ascii_lowercase(), SocketAddr::from(([127, 0, 0, 1], self.wss.local_port))));
        }
        Ok(v)
    }

    /// The WSS address others dial: `/dns/<name>/tcp/<port>/tls/ws`.
    pub fn wss_external(&self) -> Option<Multiaddr> {
        if !self.wss.enabled {
            return None;
        }
        format!("/dns/{}/tcp/{}/tls/ws", self.wss.name.trim_end_matches('.'), self.wss.port).parse().ok()
    }

    /// Where WSS listens: plain WebSocket on loopback behind a proxy, TLS on loopback behind
    /// forwarding, else TLS on the public port on every interface.
    pub fn wss_listen(&self) -> Vec<Multiaddr> {
        if !self.wss.enabled {
            return vec![];
        }
        let v = if self.wss.cert.is_none() {
            vec![format!("/ip4/127.0.0.1/tcp/{}/ws", self.wss.local_port)]
        } else if self.forwarding.enabled {
            vec![format!("/ip4/127.0.0.1/tcp/{}/tls/ws", self.wss.local_port)]
        } else {
            vec![format!("/ip4/0.0.0.0/tcp/{}/tls/ws", self.wss.port), format!("/ip6/::/tcp/{}/tls/ws", self.wss.port)]
        };
        v.iter().filter_map(|a| a.parse().ok()).collect()
    }

    pub fn http_addr(&self) -> Result<SocketAddr> {
        self.http.listen.parse().map_err(|_| Error::Config(format!("http.listen: `{}` is not host:port", self.http.listen)))
    }

    /// The state directory: configured, else `$STATE_DIRECTORY` (systemd), else the default.
    pub fn state_dir(&self) -> PathBuf {
        if let Some(d) = &self.state_dir {
            return d.clone();
        }
        if let Some(d) = std::env::var_os("STATE_DIRECTORY") {
            // systemd may list several, separated by ':'
            if let Some(first) = d.to_string_lossy().split(':').next() {
                return PathBuf::from(first);
            }
        }
        PathBuf::from(DEFAULT_STATE_DIR)
    }

    /// The database file.
    pub fn db_path(&self) -> PathBuf {
        self.state_dir().join("relay.db")
    }

    /// The relay limits: the preset, with any field set here on top.
    pub fn relay_limits(&self) -> RelayLimits {
        let l = &self.limits;
        let mut r = match l.preset {
            Preset::Unlimited => RelayLimits::unlimited(),
            Preset::Libp2p => RelayLimits::default(),
        };
        if let Some(v) = l.max_reservations {
            r.max_reservations = v;
        }
        if let Some(v) = l.max_reservations_per_peer {
            r.max_reservations_per_peer = v;
        }
        if let Some(v) = l.reservation_duration_secs {
            r.reservation_duration = Duration::from_secs(v);
        }
        if let Some(v) = l.max_circuits {
            r.max_circuits = v;
        }
        if let Some(v) = l.max_circuits_per_peer {
            r.max_circuits_per_peer = v;
        }
        if let Some(v) = l.max_circuit_duration_secs {
            r.max_circuit_duration = Duration::from_secs(v);
        }
        if let Some(v) = l.max_circuit_bytes {
            r.max_circuit_bytes = v;
        }
        if let Some(v) = l.rate_limits {
            r.rate_limits = v;
        }
        r
    }

    /// The node configuration for weirdnet-net.
    pub fn node_config(&self) -> Result<NodeConfig> {
        let mut listen = self.listen_addrs()?;
        let mut external = self.external_addrs()?;
        let mut wss = None;
        if self.wss.enabled {
            listen.extend(self.wss_listen());
            external.extend(self.wss_external());
            if let (Some(c), Some(k)) = (&self.wss.cert, &self.wss.key) {
                wss = Some(weirdnet_net::WssTls::from_pem_files(c, k).map_err(|e| Error::Config(format!("wss: {e}")))?);
            }
        }
        Ok(NodeConfig {
            listen,
            wss,
            mdns: self.network.mdns,
            relay_server: true,
            bootstrap: self.bootstrap_addrs()?,
            relays: vec![],
            memory: false,
            label: self.name.clone(),
            external,
            version: crate::VERSION.into(),
            relay_limits: self.relay_limits(),
            ..Default::default()
        })
    }
}
