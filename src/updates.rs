//! Update announcements (`/weirdnet/updates/1`, protocol in
//! [`weirdnet_update::announce::wire`]).
//!
//! The relay is a carrier: publishers (and other relays) publish signed announcements and
//! root rotations, devices subscribe to the apps they run, and following relays subscribe to
//! everything. The relay keeps only announcements signed for the orgs in `[updates] roots`,
//! the latest per org, app and channel, in its database. Devices verify each announcement
//! against their own root and then fetch and verify the update itself; the relay can't make
//! an update acceptable, only withhold news of one.
use crate::config::Updates as UpdatesConfig;
use crate::error::{Error, Result};
use crate::store::Store;
use futures::{AsyncReadExt, AsyncWriteExt};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use weirdnet_net::{Limits, Message, Multiaddr, Node, PeerId};
use weirdnet_update::announce::wire::{self, FromCarrier, MAX_PUBLISH, MAX_TOPICS, PROTOCOL, Subscription, ToCarrier};
use weirdnet_update::announce::{Announcement, Carrier, Offered, Topic};
use weirdnet_update::{Domain, RootPin, Signed, Timestamp};

const FIRST_FRAME: Duration = Duration::from_secs(15);

/// The carrier and the store behind it.
pub struct Board {
    carrier: Mutex<Carrier>,
    store: Arc<Store>,
    tx: broadcast::Sender<Signed>,
}

fn now() -> Timestamp {
    Timestamp::now()
}

impl Board {
    /// A board trusting `cfg.roots`, refilled from the store.
    pub fn new(cfg: &UpdatesConfig, store: Arc<Store>) -> Result<Arc<Self>> {
        let mut carrier = Carrier::new(cfg.max_announcements);
        for r in &cfg.roots {
            let pin = RootPin::from_hex(&r.pin).map_err(|e| Error::Config(format!("updates.roots pin: {e}")))?;
            let mut chain = Vec::new();
            for f in &r.chain {
                let b = std::fs::read(f).map_err(|e| Error::Config(format!("updates.roots chain {}: {e}", f.display())))?;
                chain.push(Signed::from_bytes(&b).map_err(|e| Error::Config(format!("updates.roots chain {}: {e}", f.display())))?);
            }
            carrier.trust(pin, &chain, now()).map_err(|e| Error::Config(format!("updates.roots {}: {e}", r.pin)))?;
        }
        let board = Arc::new(Self { carrier: Mutex::new(carrier), store, tx: broadcast::channel(1024).0 });
        for b in board.store.update_envelopes(now().0 as i64)? {
            if let Ok(env) = Signed::from_bytes(&b) {
                let _ = board.lock().offer(&env, now());
            }
        }
        Ok(board)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Carrier> {
        self.carrier.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Offers an envelope: kept, stored and passed on if it is new.
    pub fn offer(&self, env: &Signed) -> std::result::Result<Offered, String> {
        let t = now();
        let r = self.lock().offer(env, t).map_err(|e| e.to_string())?;
        if matches!(r, Offered::New | Offered::Root) {
            let (id, expires) = match env.domain {
                Domain::Root => (format!("root:{}", env.payload_hash().to_hex()), i64::MAX),
                _ => {
                    let a: Announcement = env.decode().map_err(|e| e.to_string())?;
                    (format!("a:{}/{}/{}", a.pin, a.app, a.channel), a.expires.0 as i64)
                }
            };
            match env.to_bytes() {
                Ok(b) => {
                    if let Err(e) = self.store.put_update_envelope(&id, &b, expires) {
                        tracing::warn!("keeping an update announcement: {e}");
                    }
                }
                Err(e) => tracing::warn!("encoding an update announcement: {e}"),
            }
            let _ = self.tx.send(env.clone());
        }
        Ok(r)
    }

    /// How many announcements are held.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether none are.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// Whether any org is trusted.
    pub fn carries(&self) -> bool {
        !self.lock().pins().is_empty()
    }

    fn snapshot(&self, topics: &[Topic], all: bool) -> Vec<Signed> {
        let c = self.lock();
        let t = now();
        let mut v: Vec<Signed> = Vec::new();
        // Rotations first, so a lagging device or relay can check what follows.
        for pin in c.pins() {
            if all || topics.iter().any(|tp| tp.pin == pin) {
                v.extend(c.chain(&pin).unwrap_or_default().iter().skip(1).cloned());
            }
        }
        v.extend(if all { c.all(t) } else { c.matching(topics, t) });
        v
    }
}

/// The running service: the stream acceptor, upstream followers and their runtime.
pub struct UpdatesService {
    board: Arc<Board>,
    rt: Option<tokio::runtime::Runtime>,
}

impl UpdatesService {
    /// Serves `/weirdnet/updates/1` on `node` and follows `cfg.upstream`.
    pub fn start<Q: Message, R: Message>(cfg: &UpdatesConfig, node: Arc<Node<Q, R>>, store: Arc<Store>) -> Result<Self> {
        let board = Board::new(cfg, store)?;
        let limits =
            Limits { window: 128 << 10, max_frame: 64 << 10, max_streams: cfg.max_subscribers + 256, backlog: 256, idle: None, ..Default::default() };
        let mut incoming = node.accept_streams(PROTOCOL, limits)?;
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().thread_name("weirdnet-relay-updates").build()?;
        let keepalive = Duration::from_secs(cfg.keepalive_secs);
        let b = board.clone();
        rt.spawn(async move {
            while let Some(s) = incoming.next().await {
                let b = b.clone();
                tokio::spawn(async move {
                    let peer = s.peer();
                    if let Err(e) = session(b, s, keepalive).await {
                        tracing::debug!(%peer, "updates stream: {e}");
                    }
                });
            }
        });
        for a in cfg.upstream_addrs()? {
            let peer = peer_of(&a).ok_or_else(|| Error::Config(format!("updates.upstream `{a}` needs /p2p/")))?;
            let b = board.clone();
            let n = node.clone();
            rt.spawn(async move { follow(n, peer, a, b).await });
        }
        tracing::info!(orgs = cfg.roots.len(), held = board.len(), "update announcements on {PROTOCOL}");
        Ok(Self { board, rt: Some(rt) })
    }

    /// The board.
    pub fn board(&self) -> &Arc<Board> {
        &self.board
    }
}

impl Drop for UpdatesService {
    fn drop(&mut self) {
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

fn peer_of(a: &Multiaddr) -> Option<PeerId> {
    a.to_string().rsplit_once("/p2p/").and_then(|(_, id)| id.split('/').next()?.parse().ok())
}

async fn follow<Q: Message, R: Message>(node: Arc<Node<Q, R>>, peer: PeerId, addr: Multiaddr, board: Arc<Board>) {
    let mut backoff = Duration::from_secs(1);
    loop {
        let started = Instant::now();
        let r = async {
            let s = node.open_stream_async(peer, std::slice::from_ref(&addr), PROTOCOL, Limits::default()).await.map_err(|e| e.to_string())?;
            let mut sub = Subscription::start(s, vec![], true).await.map_err(|e| e.to_string())?;
            let idle = Duration::from_secs(u64::from(sub.keepalive_s.max(10)) * 3);
            loop {
                match tokio::time::timeout(idle, sub.next()).await {
                    Err(_) => return Err("upstream went quiet".to_string()),
                    Ok(Ok(Some(env))) => {
                        if let Err(e) = board.offer(&env) {
                            tracing::debug!(%peer, "upstream envelope refused: {e}");
                        }
                    }
                    Ok(Ok(None)) => return Ok(()),
                    Ok(Err(e)) => return Err(e.to_string()),
                }
            }
        }
        .await;
        match r {
            Ok(()) => tracing::info!(%peer, "upstream announcements ended; reconnecting"),
            Err(e) => tracing::warn!(%peer, "upstream announcements: {e}"),
        }
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(300));
    }
}

async fn session(board: Arc<Board>, s: weirdnet_net::Stream, keepalive: Duration) -> Result<()> {
    let (mut r, mut w) = s.split();
    let first: Option<ToCarrier> =
        tokio::time::timeout(FIRST_FRAME, wire::read(&mut r)).await.map_err(|_| Error::Updates("no first frame".into()))?.map_err(upd)?;
    match first {
        None => Ok(()),
        Some(ToCarrier::Publish { envelopes }) => {
            if envelopes.is_empty() || envelopes.len() > MAX_PUBLISH {
                wire::write(&mut w, &FromCarrier::Error(format!("publish 1 to {MAX_PUBLISH} envelopes"))).await.map_err(upd)?;
            } else {
                let res: Vec<_> = envelopes.iter().map(|e| board.offer(e)).collect();
                wire::write(&mut w, &FromCarrier::Published(res)).await.map_err(upd)?;
            }
            let _ = w.close().await;
            Ok(())
        }
        Some(ToCarrier::Subscribe { topics, all }) => {
            if topics.len() > MAX_TOPICS || (topics.is_empty() && !all) {
                wire::write(&mut w, &FromCarrier::Error(format!("subscribe to 1 to {MAX_TOPICS} topics, or all"))).await.map_err(upd)?;
                let _ = w.close().await;
                return Ok(());
            }
            let mut rx = board.tx.subscribe();
            wire::write(&mut w, &FromCarrier::Subscribed { keepalive_s: keepalive.as_secs() as u32 }).await.map_err(upd)?;
            for env in board.snapshot(&topics, all) {
                wire::write(&mut w, &FromCarrier::Envelope(env)).await.map_err(upd)?;
            }
            let mut gone = tokio::spawn(async move {
                let mut b = [0u8; 64];
                while matches!(r.read(&mut b).await, Ok(n) if n > 0) {}
            });
            let mut tick = tokio::time::interval(keepalive);
            tick.tick().await;
            loop {
                tokio::select! {
                    _ = &mut gone => break,
                    m = rx.recv() => match m {
                        Ok(env) => {
                            let wanted = all || match env.domain {
                                Domain::Root => true,
                                _ => Carrier::matches(&env, &topics),
                            };
                            if wanted {
                                wire::write(&mut w, &FromCarrier::Envelope(env)).await.map_err(upd)?;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => tracing::debug!("an updates subscriber missed {n}"),
                        Err(broadcast::error::RecvError::Closed) => break,
                    },
                    _ = tick.tick() => wire::write(&mut w, &FromCarrier::Keepalive).await.map_err(upd)?,
                }
            }
            gone.abort();
            Ok(())
        }
    }
}

fn upd(e: weirdnet_update::Error) -> Error {
    Error::Updates(e.to_string())
}
