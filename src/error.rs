//! Errors.
use weirdnet_net::NetError;
use weirdnet_storage::StorageError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config: {0}")]
    Config(String),
    #[error("database key: {0}")]
    Key(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Net(#[from] NetError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("wake: {0}")]
    Wake(String),
    #[error("updates: {0}")]
    Updates(String),
    #[error("ddns: {0}")]
    Ddns(String),
    #[error("info document: {0}")]
    Info(String),
}

impl From<weirdnet_storage::rusqlite::Error> for Error {
    fn from(e: weirdnet_storage::rusqlite::Error) -> Self {
        Error::Storage(StorageError::Sqlite(e))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
