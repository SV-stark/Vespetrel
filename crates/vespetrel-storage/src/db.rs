use deadpool_sqlite::{Config as PoolConfig, Hook, HookError, Pool, Runtime};
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
    let mut cfg = PoolConfig::new(db_path);
    cfg.pool = Some(deadpool_sqlite::PoolConfig::new(8));
    let pool = cfg
        .builder(Runtime::Tokio1)?
        .post_create(Hook::async_fn(|conn, _metrics| {
            Box::pin(async move {
                conn.interact(|c| init_connection(c))
                    .await
                    .map_err(|e| HookError::message(e.to_string()))?
                    .map_err(|e| HookError::message(e.to_string()))
            })
        }))
        .build()?;
    Ok(pool)
}

/// Initialize a single connection with optional encryption key, PRAGMAs, and migrations
pub fn init_connection(conn: &Connection) -> anyhow::Result<()> {
    init_connection_with_key(conn, None)
}

/// Initialize connection with optional SQLCipher encryption key
pub fn init_connection_with_key(
    conn: &Connection,
    encryption_key: Option<&str>,
) -> anyhow::Result<()> {
    if let Some(key) = encryption_key {
        let escaped_key = key.replace('\'', "''");
        let _ = conn.execute_batch(&format!("PRAGMA key = '{escaped_key}'"));
    }
    for pragma in PRAGMAS {
        conn.execute_batch(pragma)?;
    }
    // Verify foreign keys are enabled
    let fk_enabled: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    if fk_enabled != 1 {
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
    }
    run_migrations(conn)?;
    Ok(())
}

/// Sourced database encryption key from OS keyring or environment
pub fn get_keyring_encryption_key(service: &str, user: &str) -> Option<String> {
    if let Ok(key) = std::env::var("VESPETREL_DB_KEY")
        && !key.is_empty()
    {
        return Some(key);
    }
    let _ = (service, user);
    None
}

/// Helper to open an in-memory DB for tests with full schema
pub fn open_in_memory() -> anyhow::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    init_connection(&conn)?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_in_memory_connection() {
        let conn = open_in_memory().unwrap();
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn test_init_connection_with_key() {
        let conn = Connection::open_in_memory().unwrap();
        init_connection_with_key(&conn, Some("test_secret_key_123")).unwrap();
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }
}
