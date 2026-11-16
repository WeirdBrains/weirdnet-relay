//! The relay's info document: `/.well-known/weirdnet-relay.json`, the `Info` answer on
//! `/weirdnet/relay/info/1`, and what the HTML page shows.
use crate::config::Config;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use weirdnet_net::{Multiaddr, PeerId, RelayLimits, Status, WEIRDNET};

/// The document's format version.
pub const INFO_VERSION: u32 = 1;
/// Where the document is served.
pub const WELL_KNOWN_PATH: &str = "/.well-known/weirdnet-relay.json";

/// The protocols a relay speaks.
pub const PROTOCOLS: &[&str] = &[
    crate::wire::INFO_PROTOCOL,
    "/weirdnet/kad/1",
    "/weirdnet/id/1",
    "/weirdnet/names/1",
    "/libp2p/circuit/relay/0.2.0/hop",
    "/libp2p/circuit/relay/0.2.0/stop",
    "/libp2p/dcutr",
    "/libp2p/autonat/1.0.0",
    "/ipfs/ping/1.0.0",
];

/// What a relay says about itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelayInfo {
    /// [`INFO_VERSION`].
    pub v: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub operator: String,
    /// Abuse and contact address.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub contact: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// `weirdnet-relay/VERSION`.
    pub software: String,
    pub peer_id: String,
    /// Full addresses to dial or add as a relay, each ending in `/p2p/<peer_id>`.
    pub addrs: Vec<String>,
    /// `tcp`, `quic-v1`, …
    pub transports: Vec<String>,
    pub protocols: Vec<String>,
    /// `relay`, `bootstrap`, `kad-server`, `autonat`.
    pub roles: Vec<String>,
    /// The network identifiers the relay runs under.
    pub network: NetworkIds,
    pub limits: LimitsInfo,
    pub status: RelayStatus,
    /// Whether the relay forwards public HTTPS for registered sites (SNI passthrough).
    #[serde(default)]
    pub forwarding: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkIds {
    pub profile: String,
    pub kad: String,
    pub identify: String,
    pub relay_key: String,
    pub info_protocol: String,
    pub default_port: u16,
}

/// Relay limits; `null` means no limit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LimitsInfo {
    pub max_reservations: Option<u64>,
    pub max_reservations_per_peer: Option<u64>,
    pub reservation_duration_secs: u64,
    pub max_circuits: Option<u64>,
    pub max_circuits_per_peer: Option<u64>,
    pub max_circuit_duration_secs: Option<u64>,
    pub max_circuit_bytes: Option<u64>,
    pub rate_limits: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RelayStatus {
    /// AutoNAT: `public`, `private` or `unknown`.
    pub nat: String,
    pub connected_peers: usize,
    pub started_at: i64,
    pub generated_at: i64,
}

fn count(v: usize) -> Option<u64> {
    (v != usize::MAX).then_some(v as u64)
}

impl LimitsInfo {
    pub fn from_limits(l: &RelayLimits) -> Self {
        LimitsInfo {
            max_reservations: count(l.max_reservations),
            max_reservations_per_peer: count(l.max_reservations_per_peer),
            reservation_duration_secs: l.reservation_duration.as_secs(),
            max_circuits: count(l.max_circuits),
            max_circuits_per_peer: count(l.max_circuits_per_peer),
            max_circuit_duration_secs: (l.max_circuit_duration < RelayLimits::MAX_CIRCUIT_DURATION).then_some(l.max_circuit_duration.as_secs()),
            max_circuit_bytes: (l.max_circuit_bytes != 0).then_some(l.max_circuit_bytes),
            rate_limits: l.rate_limits,
        }
    }
}

/// Transport names in `addrs`, in a stable order.
pub fn transports(addrs: &[String]) -> Vec<String> {
    let mut t = vec![];
    for a in addrs {
        let Ok(m) = a.parse::<Multiaddr>() else { continue };
        let tags: Vec<&str> = m.iter().map(|p| p.tag()).collect();
        let name = if tags.contains(&"quic-v1") {
            "quic-v1"
        } else if tags.contains(&"ws") || tags.contains(&"wss") || tags.contains(&"tls") {
            "websocket"
        } else if tags.contains(&"tcp") {
            "tcp"
        } else if tags.contains(&"memory") {
            "memory"
        } else {
            continue;
        };
        if !t.iter().any(|x| x == name) {
            t.push(name.to_string());
        }
    }
    t
}

/// The addresses to announce: the configured external ones (and the WSS one) with `/p2p/<id>`, else what the
/// node reports as reachable, without relay circuits.
pub fn announced(cfg: &Config, peer: &PeerId, reachable: &[String]) -> Vec<String> {
    let wss = cfg.wss_external().map(|a| a.to_string());
    let external: Vec<String> =
        cfg.network.external.iter().map(|a| a.trim().trim_end_matches('/').to_string()).chain(wss).map(|a| format!("{a}/p2p/{peer}")).collect();
    if !external.is_empty() {
        return external;
    }
    reachable.iter().filter(|a| !a.contains("/p2p-circuit")).cloned().collect()
}

impl RelayInfo {
    /// Build the document from the config and what the node reports.
    pub fn build(cfg: &Config, peer: &PeerId, status: &Status, reachable: &[String], now: i64) -> RelayInfo {
        let addrs = announced(cfg, peer, reachable);
        RelayInfo {
            v: INFO_VERSION,
            name: cfg.name.clone(),
            operator: cfg.operator.clone(),
            contact: cfg.contact.clone(),
            description: cfg.description.clone(),
            software: crate::software(),
            peer_id: peer.to_string(),
            transports: transports(&addrs),
            addrs,
            protocols: PROTOCOLS.iter().map(|p| p.to_string()).collect(),
            roles: ["relay", "bootstrap", "kad-server", "autonat", "names"].iter().map(|r| r.to_string()).collect(),
            network: NetworkIds {
                profile: WEIRDNET.name.into(),
                kad: WEIRDNET.kad.into(),
                identify: WEIRDNET.identify.into(),
                relay_key: WEIRDNET.relay_key.into(),
                info_protocol: crate::wire::INFO_PROTOCOL.into(),
                default_port: WEIRDNET.default_port,
            },
            limits: LimitsInfo::from_limits(&cfg.relay_limits()),
            status: RelayStatus {
                nat: status.nat.clone(),
                connected_peers: status.connected.len(),
                started_at: status.started_at,
                generated_at: now,
            },
            forwarding: cfg.forwarding.enabled,
        }
        .with_forwarding_protocol()
    }

    fn with_forwarding_protocol(mut self) -> Self {
        if self.forwarding {
            self.protocols.push(weirdnet_net::ingress::GRANT_PROTOCOL.into());
        }
        self
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }

    /// Read a document someone published, checking the version, the peer id and that every
    /// address dials that peer.
    pub fn parse(json: &str) -> Result<RelayInfo> {
        let info: RelayInfo = serde_json::from_str(json).map_err(|e| Error::Info(e.to_string()))?;
        if info.v != INFO_VERSION {
            return Err(Error::Info(format!("version {} is not {INFO_VERSION}", info.v)));
        }
        info.relay_addrs()?;
        Ok(info)
    }

    pub fn peer(&self) -> Result<PeerId> {
        self.peer_id.parse().map_err(|_| Error::Info(format!("`{}` is not a peer id", self.peer_id)))
    }

    /// The addresses to put in an app's `relays` and `bootstrap`.
    pub fn relay_addrs(&self) -> Result<Vec<Multiaddr>> {
        let peer = self.peer()?;
        let suffix = format!("/p2p/{peer}");
        self.addrs
            .iter()
            .map(|a| {
                let m: Multiaddr = a.parse().map_err(|_| Error::Info(format!("`{a}` is not a multiaddr")))?;
                if !a.ends_with(&suffix) {
                    return Err(Error::Info(format!("`{a}` does not end in {suffix}")));
                }
                Ok(m)
            })
            .collect()
    }
}
