//! The relay's database: one SQLCipher file (`relay.db`) holding the node identity and the
//! relay's state, opened with the operator's raw key through `weirdnet-storage`.
use crate::error::{Error, Result};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use weirdnet_net::Sighting;
use weirdnet_net::identity::{IdentityStore, Stored};
use weirdnet_net::names::{Entry, NameStore};
use weirdnet_storage::RawKey;
use weirdnet_storage::rusqlite::{Connection, OptionalExtension, params};
use zeroize::Zeroizing;

/// Schema migrations, in order. Never edit one that has shipped; add a new one.
pub const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE node_identity (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  keypair BLOB NOT NULL,
  peer_id TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE peers (
  peer_id TEXT PRIMARY KEY,
  addrs TEXT NOT NULL,
  caps TEXT,
  agent TEXT,
  source TEXT NOT NULL,
  first_seen INTEGER NOT NULL,
  last_seen INTEGER NOT NULL
);
CREATE INDEX peers_last_seen ON peers(last_seen);
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#,
    r#"
CREATE TABLE names (
  user_id TEXT PRIMARY KEY,
  entry BLOB NOT NULL,
  seq INTEGER NOT NULL,
  expires INTEGER NOT NULL
);
CREATE INDEX names_expires ON names(expires);
"#,
    r#"
CREATE TABLE revoked_sites (
  name TEXT PRIMARY KEY,
  reason TEXT NOT NULL,
  revoked_at INTEGER NOT NULL
);
"#,
    r#"
CREATE TABLE update_envelopes (
  id TEXT PRIMARY KEY,
  envelope BLOB NOT NULL,
  expires INTEGER NOT NULL
);
"#,
];

/// A site this relay doesn't forward.
#[derive(Debug, Clone, PartialEq)]
pub struct RevokedSite {
    pub name: String,
    pub reason: String,
    pub revoked_at: i64,
}

/// A peer the relay has seen.
#[derive(Debug, Clone, PartialEq)]
pub struct Peer {
    pub peer_id: String,
    pub addrs: Vec<String>,
    pub caps: Option<Vec<String>>,
    pub agent: Option<String>,
    pub source: String,
    pub first_seen: i64,
    pub last_seen: i64,
}

pub struct Store {
    conn: Mutex<Connection>,
}

fn json_list(v: &[String]) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn parse_list(s: &str) -> Vec<String> {
    serde_json::from_str(s).unwrap_or_default()
}

impl Store {
    /// Open (or create) the database at `path` with `key` and bring its schema up to date.
    pub fn open(path: &Path, key: &RawKey) -> Result<Store> {
        let conn = weirdnet_storage::open(path, Some(key), "relay")?;
        Store::init(conn)
    }

    /// A private in-memory database (tests).
    pub fn open_in_memory() -> Result<Store> {
        Store::init(weirdnet_storage::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Store> {
        weirdnet_storage::migrate::<Error>(&mut conn, MIGRATIONS, MIGRATIONS.len(), |_, _| Ok(()))?;
        Ok(Store { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(weirdnet_storage::schema_version(&self.conn())?)
    }

    /// Remember a peer: addresses, capabilities and agent replace the old ones when given.
    pub fn saw(&self, s: &Sighting, now: i64) -> Result<()> {
        let caps = s.caps.as_ref().map(|c| json_list(c));
        self.conn().execute(
            "INSERT INTO peers (peer_id, addrs, caps, agent, source, first_seen, last_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(peer_id) DO UPDATE SET
               addrs = CASE WHEN ?2 = '[]' THEN addrs ELSE ?2 END,
               caps = COALESCE(?3, caps), agent = COALESCE(?4, agent), source = ?5, last_seen = ?6",
            params![s.peer.to_string(), json_list(&s.addrs), caps, s.agent, s.source, now],
        )?;
        Ok(())
    }

    pub fn peer(&self, peer_id: &str) -> Result<Option<Peer>> {
        Ok(self
            .conn()
            .query_row("SELECT peer_id, addrs, caps, agent, source, first_seen, last_seen FROM peers WHERE peer_id = ?1", [peer_id], |r| {
                Ok(Peer {
                    peer_id: r.get(0)?,
                    addrs: parse_list(&r.get::<_, String>(1)?),
                    caps: r.get::<_, Option<String>>(2)?.map(|c| parse_list(&c)),
                    agent: r.get(3)?,
                    source: r.get(4)?,
                    first_seen: r.get(5)?,
                    last_seen: r.get(6)?,
                })
            })
            .optional()?)
    }

    /// How many peers were seen at or after `since` (ms).
    pub fn peers_seen_since(&self, since: i64) -> Result<u64> {
        Ok(self.conn().query_row("SELECT count(*) FROM peers WHERE last_seen >= ?1", [since], |r| r.get::<_, i64>(0))? as u64)
    }

    /// Forget peers not seen since `before` (ms); returns how many.
    pub fn prune_peers(&self, before: i64) -> Result<usize> {
        Ok(self.conn().execute("DELETE FROM peers WHERE last_seen < ?1", [before])?)
    }

    /// Stop forwarding `name` (lowercase, no trailing dot).
    pub fn revoke_site(&self, name: &str, reason: &str, now: i64) -> Result<()> {
        self.conn().execute(
            "INSERT INTO revoked_sites (name, reason, revoked_at) VALUES (?1, ?2, ?3) ON CONFLICT(name) DO UPDATE SET reason = ?2, revoked_at = ?3",
            params![name, reason, now],
        )?;
        Ok(())
    }

    /// Forward `name` again; whether it was revoked.
    pub fn unrevoke_site(&self, name: &str) -> Result<bool> {
        Ok(self.conn().execute("DELETE FROM revoked_sites WHERE name = ?1", [name])? > 0)
    }

    pub fn revoked_sites(&self) -> Result<Vec<RevokedSite>> {
        let c = self.conn();
        let mut st = c.prepare("SELECT name, reason, revoked_at FROM revoked_sites ORDER BY name")?;
        let rows = st.query_map([], |r| Ok(RevokedSite { name: r.get(0)?, reason: r.get(1)?, revoked_at: r.get(2)? }))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Keeps an update announcement or root rotation (CBOR envelope) under `id`, replacing
    /// what was there.
    pub fn put_update_envelope(&self, id: &str, envelope: &[u8], expires: i64) -> Result<()> {
        self.conn().execute(
            "INSERT INTO update_envelopes (id, envelope, expires) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET envelope = ?2, expires = ?3",
            params![id, envelope, expires],
        )?;
        Ok(())
    }

    /// The kept envelopes that haven't expired at `now` (unix seconds), root rotations first.
    pub fn update_envelopes(&self, now: i64) -> Result<Vec<Vec<u8>>> {
        let c = self.conn();
        c.execute("DELETE FROM update_envelopes WHERE expires <= ?1", [now])?;
        let mut st = c.prepare("SELECT envelope FROM update_envelopes ORDER BY id NOT LIKE 'root:%', id")?;
        let rows = st.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn().query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn().execute("INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2", [key, value])?;
        Ok(())
    }
}

impl IdentityStore for Store {
    type Error = Error;

    fn node_identity(&self) -> Result<Option<Stored>> {
        Ok(self
            .conn()
            .query_row("SELECT keypair, peer_id FROM node_identity WHERE id = 1", [], |r| {
                Ok((Zeroizing::new(r.get::<_, Vec<u8>>(0)?), r.get::<_, String>(1)?))
            })
            .optional()?)
    }

    fn init_node_identity(&self, keypair: &[u8], peer: &str) -> Result<()> {
        self.conn().execute(
            "INSERT OR IGNORE INTO node_identity (id, keypair, peer_id, created_at) VALUES (1, ?1, ?2, ?3)",
            params![keypair, peer, weirdnet_net::now_ms()],
        )?;
        Ok(())
    }
}

/// The names (rendezvous) server's entries: signed peer records and withdrawals, checked by
/// weirdnet-net before they get here, kept across restarts until they expire.
impl NameStore for Store {
    fn get(&self, user: &str) -> Option<Entry> {
        let bytes: Option<Vec<u8>> =
            self.conn().query_row("SELECT entry FROM names WHERE user_id = ?1", [user], |r| r.get(0)).optional().ok().flatten();
        bytes.and_then(|b| Entry::decode(&b).ok())
    }

    fn put(&self, e: &Entry) {
        let r = self.conn().execute(
            "INSERT INTO names (user_id, entry, seq, expires) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(user_id) DO UPDATE SET entry = ?2, seq = ?3, expires = ?4",
            params![e.user_id(), e.encode(), e.seq() as i64, e.expires()],
        );
        if let Err(err) = r {
            tracing::warn!("cannot store a names entry: {err}");
        }
    }

    fn prune(&self, now: i64) {
        let _ = self.conn().execute("DELETE FROM names WHERE expires <= ?1", [now]);
    }
}

impl Store {
    /// How many names entries are held.
    pub fn names_count(&self) -> Result<u64> {
        Ok(self.conn().query_row("SELECT COUNT(*) FROM names", [], |r| r.get::<_, i64>(0))? as u64)
    }
}
