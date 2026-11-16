//! The database key: 32 bytes the operator supplies, never stored by the relay.
//!
//! Where it is read from, in order: `--db-key-file` / `db_key_file` in the config, else the
//! systemd credential `db-key` (`$CREDENTIALS_DIRECTORY/db-key`, from `LoadCredential=` or
//! `LoadCredentialEncrypted=`). The file holds 64 hex characters (whitespace around them is
//! ignored) or exactly 32 raw bytes. The key is never logged or printed except by
//! [`generate_hex`] for `weirdnet-relay gen-key`.
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use weirdnet_storage::RawKey;
use zeroize::Zeroizing;

/// The systemd credential name.
pub const CREDENTIAL: &str = "db-key";

/// The key file to read: the configured one, else the systemd credential.
pub fn locate(configured: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = configured {
        return Ok(p.to_path_buf());
    }
    if let Some(dir) = std::env::var_os("CREDENTIALS_DIRECTORY") {
        let p = Path::new(&dir).join(CREDENTIAL);
        if p.exists() {
            return Ok(p);
        }
    }
    Err(Error::Key(
        "no key: pass --db-key-file (or db_key_file in the config), or load the systemd credential `db-key` (LoadCredentialEncrypted=db-key:…)"
            .into(),
    ))
}

/// Read a key file: 64 hex characters, or 32 raw bytes.
pub fn read(path: &Path) -> Result<Zeroizing<RawKey>> {
    let bytes = Zeroizing::new(std::fs::read(path).map_err(|e| Error::Key(format!("cannot read {}: {e}", path.display())))?);
    warn_if_shared(path);
    parse(&bytes)
}

/// Parse key bytes: 64 hex characters (surrounding whitespace ignored), or 32 raw bytes.
pub fn parse(bytes: &[u8]) -> Result<Zeroizing<RawKey>> {
    let mut key = Zeroizing::new([0u8; 32]);
    let trimmed = bytes.trim_ascii();
    if trimmed.len() == 64 && trimmed.iter().all(u8::is_ascii_hexdigit) {
        hex::decode_to_slice(trimmed, &mut key[..]).map_err(|_| Error::Key("not hex".into()))?;
        return Ok(key);
    }
    if bytes.len() == 32 {
        key.copy_from_slice(bytes);
        return Ok(key);
    }
    Err(Error::Key("a key file holds 64 hex characters or 32 raw bytes".into()))
}

/// A fresh random key as 64 hex characters.
pub fn generate_hex() -> Result<Zeroizing<String>> {
    let mut key = Zeroizing::new([0u8; 32]);
    getrandom::fill(&mut key[..]).map_err(|e| Error::Key(format!("no randomness: {e}")))?;
    Ok(Zeroizing::new(hex::encode(&key[..])))
}

#[cfg(unix)]
fn warn_if_shared(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(m) = std::fs::metadata(path)
        && m.permissions().mode() & 0o077 != 0
    {
        tracing::warn!("the database key file {} is readable by others; chmod 0600 it", path.display());
    }
}

#[cfg(not(unix))]
fn warn_if_shared(_: &Path) {}
