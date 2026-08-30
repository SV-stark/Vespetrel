//! Vespetrel Storage - SQLite WAL + FTS5 + blob store

pub mod blob;
pub mod db;
pub mod fts;
pub mod migrations;
pub mod repo;

pub use blob::BlobStore;
pub use db::{create_pool, StoragePool, PRAGMAS};
pub use fts::{search_messages, SearchResult};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("rusqlite error: {0}")]
    Rusqlite(#[from] rusqlite::Error),
    #[error("pool error: {0}")]
    Pool(String),
    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("not found: {0}")]
    NotFound(String),
}

pub type StorageResult<T> = Result<T, StorageError>;
