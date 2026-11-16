//! The relay wake path (`/weirdnet/wake/1`, protocol in [`weirdnet_push::relaywake`]).
//!
//! Devices that opted in keep a stream open and subscribe their wake handles. Trusted peers
//! deliver sealed payloads over their own streams, and the relay can also follow a push
//! service's relay announcements, so wakes originating from WeirdBrains reach devices behind
//! NAT. Payloads pass through unchanged; the relay can't read them.
//!
//! The relay keeps nothing on disk. Wakes for a device that was connected within
//! `hold_secs` wait for its reconnect (at most [`HELD_PER_HANDLE`] each). A handle gets at
//! most `max_per_minute` wakes, which protects the phone's battery from a sender that
//! learned its handle.
use crate::config::Wake as WakeConfig;
use crate::error::{Error, Result};
use futures::{AsyncReadExt, AsyncWriteExt};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use weirdnet_net::{Limits, Message, Node};
use weirdnet_push::relaywake::{self, Delivery, FromRelay, MAX_HANDLES, PROTOCOL, ToRelay};
use weirdnet_push::{Payload96, WakeHandle};

/// Wakes held per handle for a reconnecting device.
pub const HELD_PER_HANDLE: usize = 4;
const QUEUE: usize = 16;
const FIRST_FRAME: Duration = Duration::from_secs(15);

type Tx = mpsc::Sender<(WakeHandle, Payload96)>;

#[derive(Default)]
struct Inner {
    subs: HashMap<String, Vec<(u64, Tx)>>,
    subscribers: usize,
    /// When a handle's last subscriber left.
    left: HashMap<String, Instant>,
    held: HashMap<String, VecDeque<(Instant, WakeHandle, Payload96)>>,
    rate: HashMap<String, (Instant, u32)>,
    next: u64,
}

/// Counters for the info page and logs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WakeStats {
    /// Streams subscribed now.
    pub subscribers: usize,
    /// Handles subscribed now.
    pub handles: usize,
    /// Wakes waiting for a reconnect.
    pub held: usize,
}

/// Who is subscribed, and what waits for them.
pub struct Hub {
    cfg: WakeConfig,
    inner: Mutex<Inner>,
}

/// A subscription; dropping it unsubscribes.
pub struct Sub {
    hub: Arc<Hub>,
    id: u64,
    digests: Vec<String>,
    /// Wakes for the subscribed handles.
    pub rx: mpsc::Receiver<(WakeHandle, Payload96)>,
}

impl Drop for Sub {
    fn drop(&mut self) {
        self.hub.unsubscribe(self.id, &self.digests);
    }
}

impl Hub {
    /// A hub with `cfg`'s limits.
    pub fn new(cfg: WakeConfig) -> Arc<Self> {
        Arc::new(Self { cfg, inner: Mutex::new(Inner::default()) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Subscribes a stream to `handles`, handing it any wakes held for them.
    pub fn subscribe(self: &Arc<Self>, handles: &[WakeHandle]) -> Result<Sub> {
        if handles.is_empty() || handles.len() > MAX_HANDLES {
            return Err(Error::Wake(format!("subscribe 1 to {MAX_HANDLES} handles")));
        }
        let (tx, rx) = mpsc::channel(QUEUE);
        let mut g = self.lock();
        if g.subscribers >= self.cfg.max_subscribers {
            return Err(Error::Wake("the relay has as many wake subscribers as it takes".into()));
        }
        g.next += 1;
        let id = g.next;
        g.subscribers += 1;
        let mut digests: Vec<String> = handles.iter().map(|h| h.digest()).collect();
        digests.sort();
        digests.dedup();
        let hold = Duration::from_secs(self.cfg.hold_secs);
        for d in &digests {
            g.subs.entry(d.clone()).or_default().push((id, tx.clone()));
            g.left.remove(d);
            if let Some(q) = g.held.remove(d) {
                for (at, h, p) in q {
                    if at.elapsed() < hold {
                        let _ = tx.try_send((h, p));
                    }
                }
            }
        }
        drop(g);
        Ok(Sub { hub: self.clone(), id, digests, rx })
    }

    fn unsubscribe(&self, id: u64, digests: &[String]) {
        let mut g = self.lock();
        g.subscribers = g.subscribers.saturating_sub(1);
        for d in digests {
            if let Some(v) = g.subs.get_mut(d) {
                v.retain(|(i, _)| *i != id);
                if v.is_empty() {
                    g.subs.remove(d);
                    g.left.insert(d.clone(), Instant::now());
                }
            }
        }
    }

    /// Passes a wake on to the device subscribed with `handle`.
    pub fn deliver(&self, handle: &WakeHandle, payload: Payload96) -> Delivery {
        let d = handle.digest();
        let now = Instant::now();
        let hold = Duration::from_secs(self.cfg.hold_secs);
        let mut g = self.lock();
        let live = g.subs.contains_key(&d);
        let recent = g.left.get(&d).is_some_and(|t| now.duration_since(*t) < hold);
        if !live && !recent {
            return Delivery::Offline;
        }
        let r = g.rate.entry(d.clone()).or_insert((now, 0));
        if now.duration_since(r.0) >= Duration::from_secs(60) {
            *r = (now, 0);
        }
        if r.1 >= self.cfg.max_per_minute {
            return Delivery::TooMany;
        }
        r.1 += 1;
        if live {
            let mut sent = false;
            for (_, tx) in g.subs.get(&d).into_iter().flatten() {
                sent |= tx.try_send((handle.clone(), payload)).is_ok();
            }
            if sent {
                return Delivery::Delivered;
            }
        }
        let q = g.held.entry(d).or_default();
        if q.len() >= HELD_PER_HANDLE {
            q.pop_front();
        }
        q.push_back((now, handle.clone(), payload));
        Delivery::Held
    }

    /// Forgets expired holds and counters.
    pub fn prune(&self) {
        let hold = Duration::from_secs(self.cfg.hold_secs);
        let mut g = self.lock();
        g.left.retain(|_, t| t.elapsed() < hold);
        g.held.retain(|_, q| {
            q.retain(|(t, _, _)| t.elapsed() < hold);
            !q.is_empty()
        });
        g.rate.retain(|_, (t, _)| t.elapsed() < Duration::from_secs(60));
    }

    /// Counters.
    pub fn stats(&self) -> WakeStats {
        let g = self.lock();
        WakeStats { subscribers: g.subscribers, handles: g.subs.len(), held: g.held.values().map(VecDeque::len).sum() }
    }
}

/// The running wake path: the stream acceptor, the push follower and their runtime.
pub struct WakeService {
    hub: Arc<Hub>,
    rt: Option<tokio::runtime::Runtime>,
}

impl WakeService {
    /// Accepts wake streams on `node`, and follows the push service if configured.
    pub fn start<Q: Message, R: Message>(cfg: &WakeConfig, node: &Node<Q, R>) -> Result<Self> {
        let token = match &cfg.push_token_file {
            Some(f) if !cfg.push.is_empty() => {
                let t = std::fs::read_to_string(f).map_err(|e| Error::Config(format!("wake push_token_file {}: {e}", f.display())))?;
                Some(t.trim().to_string())
            }
            _ => None,
        };
        if !cfg.push.is_empty() && token.as_deref().is_none_or(str::is_empty) {
            return Err(Error::Config("wake.push needs push_token_file (a relay token from `weirdnet-push relay-token`)".into()));
        }
        let limits =
            Limits { window: 16 << 10, max_frame: 4096, max_streams: cfg.max_subscribers + 256, backlog: 256, idle: None, ..Default::default() };
        let mut incoming = node.accept_streams(PROTOCOL, limits)?;
        let hub = Hub::new(cfg.clone());
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().thread_name("weirdnet-relay-wake").build()?;
        let keepalive = Duration::from_secs(cfg.keepalive_secs);
        let h = hub.clone();
        rt.spawn(async move {
            while let Some(s) = incoming.next().await {
                let h = h.clone();
                tokio::spawn(async move {
                    let peer = s.peer();
                    if let Err(e) = session(h, s, keepalive).await {
                        tracing::debug!(%peer, "wake stream: {e}");
                    }
                });
            }
        });
        let h = hub.clone();
        rt.spawn(async move {
            let mut t = tokio::time::interval(Duration::from_secs(15));
            loop {
                t.tick().await;
                h.prune();
            }
        });
        if let Some(token) = token {
            let url = cfg.push.clone();
            let h = hub.clone();
            rt.spawn(follow(url, token, h));
        }
        tracing::info!(push = %cfg.push, "wake path on {PROTOCOL}");
        Ok(Self { hub, rt: Some(rt) })
    }

    /// The hub (for counters and tests).
    pub fn hub(&self) -> &Arc<Hub> {
        &self.hub
    }
}

impl Drop for WakeService {
    fn drop(&mut self) {
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

async fn follow(url: String, token: String, hub: Arc<Hub>) {
    let client = weirdnet_push::PushClient::new(url.clone());
    let mut backoff = Duration::from_secs(1);
    loop {
        let started = Instant::now();
        let r = client
            .announcements(&token, |a| {
                let st = hub.deliver(&a.handle, a.payload);
                tracing::trace!(?st, "announcement");
                true
            })
            .await;
        match r {
            Ok(()) => tracing::info!(%url, "push announcements ended; reconnecting"),
            Err(e) => tracing::warn!(%url, "push announcements: {e}"),
        }
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(60));
    }
}

async fn session(hub: Arc<Hub>, s: weirdnet_net::Stream, keepalive: Duration) -> Result<()> {
    let (mut r, mut w) = s.split();
    let first: Option<ToRelay> =
        tokio::time::timeout(FIRST_FRAME, relaywake::read(&mut r)).await.map_err(|_| Error::Wake("no first frame".into()))?.map_err(push_err)?;
    match first {
        None => Ok(()),
        Some(ToRelay::Deliver { handle, payload }) => {
            let status = hub.deliver(&handle, payload);
            relaywake::write(&mut w, &FromRelay::Delivered { status }).await.map_err(push_err)?;
            let _ = w.close().await;
            Ok(())
        }
        Some(ToRelay::Subscribe { handles }) => {
            let mut sub = match hub.subscribe(&handles) {
                Ok(s) => s,
                Err(e) => {
                    relaywake::write(&mut w, &FromRelay::Error { error: e.to_string() }).await.map_err(push_err)?;
                    let _ = w.close().await;
                    return Ok(());
                }
            };
            let n = sub.digests.len();
            relaywake::write(&mut w, &FromRelay::Subscribed { handles: n, keepalive_s: keepalive.as_secs() as u32 }).await.map_err(push_err)?;
            // The device sends nothing more; a read ending means it went away.
            let mut gone = tokio::spawn(async move {
                let mut b = [0u8; 64];
                while matches!(r.read(&mut b).await, Ok(n) if n > 0) {}
            });
            let mut tick = tokio::time::interval(keepalive);
            tick.tick().await;
            loop {
                tokio::select! {
                    _ = &mut gone => break,
                    m = sub.rx.recv() => {
                        let Some((handle, payload)) = m else { break };
                        relaywake::write(&mut w, &FromRelay::Wake { handle, payload }).await.map_err(push_err)?;
                    }
                    _ = tick.tick() => relaywake::write(&mut w, &FromRelay::Keepalive).await.map_err(push_err)?,
                }
            }
            gone.abort();
            Ok(())
        }
    }
}

fn push_err(e: weirdnet_push::Error) -> Error {
    Error::Wake(e.to_string())
}
