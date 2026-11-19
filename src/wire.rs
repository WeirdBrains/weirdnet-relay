//! The relay's own request protocol, `/weirdnet/relay/info/2`.
//!
//! [`RELAY_PROFILE`] is [`WEIRDNET`] with only the request-response protocol, the agent name
//! and the thread name changed: Kademlia, identify, the relay provider key and every
//! signature domain stay Weirdnet's, so the relay is in the same DHT as every Weirdnet node,
//! but no app ever receives the relay's requests on its own protocol id.
use crate::info::RelayInfo;
use serde::{Deserialize, Serialize};
use weirdnet_net::{Profile, WEIRDNET};

/// The relay's request-response protocol id.
pub const INFO_PROTOCOL: &str = "/weirdnet/relay/info/2";

/// The profile the relay's node runs.
pub const RELAY_PROFILE: Profile = Profile { name: "weirdnet-relay", protocol: INFO_PROTOCOL, thread: "weirdnet-relay", ..WEIRDNET };

/// What a peer can ask the relay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Request {
    /// The info document (the same as `/.well-known/weirdnet-relay.json`).
    Info,
    Ping,
}

/// The relay's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    Info(Box<RelayInfo>),
    Pong { now_ms: i64 },
    Error(String),
}
