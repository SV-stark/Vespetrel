use deadpool_sqlite::{Config as PoolConfig, Pool, Runtime};
use rusqlite::Connection;

use crate::migrations::run_migrations;

/// PRAGMA configuration from §3.1
pub const PRAGMAS: &[&str] = &[
    "PRAGMA journal_mode = WAL",
    "PRAGMA synchronous = NORMAL",
    "PRAGMA foreign_keys = ON",
    "PRAGMA busy_timeout = 5000",
    "PRAGMA cache_size = -64000",
    "PRAGMA mmap_size = 268435456",
    "PRAGMA temp_store = MEMORY",
];

pub type StoragePool = Pool;

pub fn create_pool(db_path: &str) -> anyhow::Result<StoragePool> {
    let cfg = PoolConfig::new(db_path);
    let pool = cfg.create_pool(Runtime::Tokio1)?;
    Ok(pool)
}

/// Initialize a single connection with PRAGMAs and migrations
pub fn init_connection(conn: &Connection) -> anyhow::Result<()> {
    for pragma in PRAGMAS {
        conn.execute_batch(pragma)?;
    }
    run_migrations(conn)?;
    Ok(())
}

/// Helper to open an in-memory DB for tests with full schema
pub fn open_in_memory() -> anyhow::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    init_connection(&conn)?;
    Ok(conn)
}
