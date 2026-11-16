//! The relay's database: one SQLCipher file (`relay.db`) holding the node identity and the
//! relay's state, opened with the operator's raw key through `weirdnet-storage`.
use crate::error::{Error, Result};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use weirdnet_net::Sighting;
use weirdnet_net::identity::{IdentityStore, Stored};
use weirdnet_storage::RawKey;
use weirdnet_storage::rusqlite::{Connection, OptionalExtension, params};
use zeroize::Zeroizing;

/// Schema migrations, in order. Never edit one that has shipped; add a new one.
pub const MIGRATIONS: &[&str] = &[r#"
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
"#];

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
