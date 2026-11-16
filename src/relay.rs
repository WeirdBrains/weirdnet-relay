//! A running relay: the node, its database and the info document.
use crate::config::Config;
use crate::error::Result;
use crate::info::RelayInfo;
use crate::store::Store;
use crate::wire::{RELAY_PROFILE, Request, Response};
use std::sync::{Arc, RwLock};
use weirdnet_net::ingress::{Binding, DnsBinding, Ingress, IngressConfig};
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
    ingress: Option<Ingress>,
}

impl Relay {
    /// Start a relay on an open store: load or create the node key, start the node and
    /// build the first info document.
    pub fn start(cfg: Config, store: Store) -> Result<Relay> {
        Self::start_opts(cfg, store, true)
    }

    /// [`Relay::start`], optionally without taking ports 443 and 80 for forwarding (to print
    /// the info document beside a running relay).
    pub fn start_opts(cfg: Config, store: Store, forward: bool) -> Result<Relay> {
        cfg.validate()?;
        let store = Arc::new(store);
        let key = weirdnet_net::identity::load_or_create(&*store)?;
        let peer = key.public().to_peer_id();
        let mut node_cfg = cfg.node_config()?;
        // the names (rendezvous) server keeps its entries in the relay's database
        node_cfg.names_server = Some(weirdnet_net::names::Names(store.clone()));
        let info = InfoHandle::new(RelayInfo::build(&cfg, &peer, &Default::default(), &[], now_ms()));
        let handler = Arc::new(RelayHandler { store: store.clone(), info: info.clone() });
        let node = Node::start(&RELAY_PROFILE, key, node_cfg, handler)?;
        let ingress = if forward && cfg.forwarding.enabled {
            let binding: Arc<dyn Binding> = Arc::new(DnsBinding::system()?);
            Some(start_forwarding(&cfg, &node, &store, binding)?)
        } else {
            None
        };
        let relay = Relay { cfg, node, store, info, ingress };
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

    /// The forwarding ingress, when on.
    pub fn ingress(&self) -> Option<&Ingress> {
        self.ingress.as_ref()
    }

    /// Bring the ingress's revoked names in line with the database (`revoke-site` and
    /// `unrevoke-site` write there while the relay runs).
    pub fn sync_revoked(&self) -> Result<()> {
        let Some(i) = &self.ingress else { return Ok(()) };
        let want: std::collections::HashSet<String> = self.store.revoked_sites()?.into_iter().map(|r| r.name).collect();
        for n in i.revoked() {
            if !want.contains(&n) {
                i.unrevoke(&n);
            }
        }
        for n in &want {
            i.revoke(n);
        }
        Ok(())
    }

    pub fn running(&self) -> bool {
        self.node.running()
    }

    pub fn stop(self) {
        self.node.stop();
    }
}

/// Start forwarding on `node`: the revoked names from the store, and the relay's own WSS name
/// routed to its loopback listener.
pub fn start_forwarding(cfg: &Config, node: &Node<Request, Response>, store: &Store, binding: Arc<dyn Binding>) -> Result<Ingress> {
    let (https, http) = cfg.forwarding_addrs()?;
    let local = cfg.forwarding_local()?;
    let blocked = store.revoked_sites()?.into_iter().map(|r| r.name).collect();
    let icfg = IngressConfig { https: Some(https), http, enabled: true, blocked, local, ..Default::default() };
    let i = Ingress::start(node, icfg, binding)?;
    tracing::info!(https = ?i.https_addr(), http = ?i.http_addr(), "forwarding sites by server name");
    Ok(i)
}
