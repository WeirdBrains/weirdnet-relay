//! weirdnet-relay: a relay and bootstrap node for the Weirdnet network.
//!
//! The node is a [`weirdnet_net::Node`] with the relay server on (libp2p circuit relay v2,
//! with [`weirdnet_net::RelayLimits::unlimited`] by default), Kademlia in server mode and
//! the relay provider record, AutoNAT and DCUtR coordination. Its node key and its state
//! live in one SQLCipher database ([`store`]) opened with a key the operator supplies
//! ([`dbkey`]). It answers its own small protocol, `/weirdnet/relay/info/1` ([`wire`]),
//! and serves an info page ([`http`]): HTML at `/` and [`RelayInfo`] at
//! `/.well-known/weirdnet-relay.json`.
//!
//! Apps can use [`RelayInfo::parse`] and [`RelayInfo::relay_addrs`] to add a relay from its
//! info document instead of re-declaring the format.
#![forbid(unsafe_code)]

pub mod config;
pub mod dbkey;
mod error;
pub mod http;
pub mod info;
pub mod relay;
pub mod store;
pub mod wire;

pub use config::Config;
pub use error::{Error, Result};
pub use info::RelayInfo;
pub use relay::Relay;

/// This crate's version, as the relay reports it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `weirdnet-relay/VERSION`.
pub fn software() -> String {
    format!("weirdnet-relay/{VERSION}")
}
