//! A running relay: the node, its database and the info document.
use crate::config::Config;
use crate::error::Result;
use crate::info::RelayInfo;
use crate::store::Store;
use crate::wire::{RELAY_PROFILE, Request, Response};
use std::sync::{Arc, RwLock};
use weirdnet_net::{Handler, Node, PeerId, Sighting, now_ms};

/// The latest info document, shared with the HTTP server and the request handler.
#[derive(Clone)]
pub struct InfoHandle(Arc<RwLock<RelayInfo>>);

impl InfoHandle {
    pub fn new(info: RelayInfo) -> Self {
        InfoHandle(Arc::new(RwLock::new(info)))
    }

    pub fn get(&self) -> RelayInfo {
        self.0.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn set(&self, info: RelayInfo) {
        *self.0.write().unwrap_or_else(|p| p.into_inner()) = info;
    }
}

struct RelayHandler {
    store: Arc<Store>,
    info: InfoHandle,
}

impl Handler<Request, Response> for RelayHandler {
    fn request(&self, from: &PeerId, req: Request) -> Response {
        tracing::debug!(peer = %from, ?req, "request");
        match req {
            Request::Info => Response::Info(Box::new(self.info.get())),
            Request::Ping => Response::Pong { now_ms: now_ms() },
        }
    }

    fn seen(&self, s: Sighting) {
        tracing::debug!(peer = %s.peer, source = s.source, agent = ?s.agent, "peer seen");
        if let Err(e) = self.store.saw(&s, now_ms()) {
            tracing::warn!("cannot record peer {}: {e}", s.peer);
        }
    }
}

pub struct Relay {
    cfg: Config,
    node: Node<Request, Response>,
    store: Arc<Store>,
    info: InfoHandle,
}

impl Relay {
    /// Start a relay on an open store: load or create the node key, start the node and
    /// build the first info document.
    pub fn start(cfg: Config, store: Store) -> Result<Relay> {
        cfg.validate()?;
        let store = Arc::new(store);
        let key = weirdnet_net::identity::load_or_create(&*store)?;
        let peer = key.public().to_peer_id();
        let node_cfg = cfg.node_config()?;
        let info = InfoHandle::new(RelayInfo::build(&cfg, &peer, &Default::default(), &[], now_ms()));
        let handler = Arc::new(RelayHandler { store: store.clone(), info: info.clone() });
        let node = Node::start(&RELAY_PROFILE, key, node_cfg, handler)?;
        let relay = Relay { cfg, node, store, info };
        relay.refresh();
        let i = relay.info.get();
        tracing::info!(peer_id = %i.peer_id, addrs = ?i.addrs, "relay started");
        Ok(relay)
    }

    pub fn peer_id(&self) -> PeerId {
        self.node.peer_id()
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn node(&self) -> &Node<Request, Response> {
        &self.node
    }

    /// The info document, shared with the HTTP server.
    pub fn info_handle(&self) -> InfoHandle {
        self.info.clone()
    }

    /// Rebuild the info document from the node's current status.
    pub fn refresh(&self) -> RelayInfo {
        let info = RelayInfo::build(&self.cfg, &self.node.peer_id(), &self.node.status(), &self.node.reachable_addrs(), now_ms());
        self.info.set(info.clone());
        info
    }

    pub fn running(&self) -> bool {
        self.node.running()
    }

    pub fn stop(self) {
        self.node.stop();
    }
}
