use deadpool_sqlite::{Config as PoolConfig, Hook, HookError, Pool, Runtime};
use rusqlite::Connection;

use crate::migrations::run_migrations;

use zeroize::Zeroizing;

/// PRAGMA configuration from §3.1
pub const PRAGMAS: &[&str] = &[
    "PRAGMA journal_mode = WAL",
    "PRAGMA synchronous = NORMAL",
    "PRAGMA foreign_keys = ON",
    "PRAGMA busy_timeout = 5000",
    "PRAGMA cache_size = -64000",
    "PRAGMA temp_store = MEMORY",
    "PRAGMA journal_size_limit = 67108864",
    "PRAGMA wal_autocheckpoint = 1000",
    "PRAGMA secure_delete = FAST",
];

pub type StoragePool = Pool;

pub fn create_pool(db_path: &str) -> crate::StorageResult<StoragePool> {
    let key = get_keyring_encryption_key("vespetrel", "db_key");
    create_pool_with_key(db_path, key.as_ref().map(|z| z.as_str()))
}

pub fn create_pool_with_key(
    db_path: &str,
    encryption_key: Option<&str>,
) -> crate::StorageResult<StoragePool> {
    let pool_size = if db_path == ":memory:" {
        1
    } else {
        std::env::var("VESPETREL_POOL_SIZE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(8)
            .clamp(1, 64)
    };
    let mut cfg = PoolConfig::new(db_path);
    cfg.pool = Some(deadpool_sqlite::PoolConfig::new(pool_size));
    // For file-backed databases, run migrations once on a dedicated connection before starting pool
    if db_path != ":memory:" {
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut conn = Connection::open(db_path)?;
        init_connection_with_key(&mut conn, encryption_key)?;
    }

    let is_memory = db_path == ":memory:";
    let key_owned = encryption_key.map(|s| Zeroizing::new(s.to_string()));
    let pool = cfg
        .builder(Runtime::Tokio1)
        .map_err(|e| crate::StorageError::Pool(e.to_string()))?
        .post_create(Hook::async_fn(move |conn, _metrics| {
            let key = key_owned.clone();
            Box::pin(async move {
                conn.interact(move |c| {
                    setup_connection_pragmas_with_key(c, key.as_deref().map(|z| z.as_str()))?;
                    if is_memory {
                        run_migrations(c)?;
                    }
                    Ok::<(), crate::StorageError>(())
                })
                .await
                .map_err(|e| HookError::message(e.to_string()))?
                .map_err(|e| HookError::message(e.to_string()))
            })
        }))
        .build()
        .map_err(|e| crate::StorageError::Pool(e.to_string()))?;
    Ok(pool)
}

/// Initialize a single connection with optional encryption key, PRAGMAs, and migrations
pub fn init_connection(conn: &mut Connection) -> crate::StorageResult<()> {
    init_connection_with_key(conn, None)
}

/// Configure connection encryption and SQLite PRAGMAs
pub fn setup_connection_pragmas_with_key(
    conn: &mut Connection,
    encryption_key: Option<&str>,
) -> crate::StorageResult<()> {
    if let Some(key) = encryption_key {
        let zero_key = Zeroizing::new(key.to_string());
        let mut hex_key = String::with_capacity(zero_key.len() * 2);
        for b in zero_key.as_bytes() {
            use std::fmt::Write;
            let _ = write!(&mut hex_key, "{:02x}", b);
        }
        let zero_hex = Zeroizing::new(hex_key);
        // Safe hex-encoded SQLCipher key literal: PRAGMA key = "x'...'";
        conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", zero_hex.as_str()))?;
    }
    for pragma in PRAGMAS {
        conn.execute_batch(pragma)?;
    }
    // Attempt 256MB mmap, fallback to 64MB if unavailable (e.g. 32-bit platforms)
    if conn.execute_batch("PRAGMA mmap_size = 268435456").is_err() {
        let _ = conn.execute_batch("PRAGMA mmap_size = 67108864");
    }
    // Verify foreign keys are enabled
    let fk_enabled: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    if fk_enabled != 1 {
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
    }
    Ok(())
}

/// Initialize connection with optional SQLCipher encryption key and run migrations
pub fn init_connection_with_key(
    conn: &mut Connection,
    encryption_key: Option<&str>,
) -> crate::StorageResult<()> {
    setup_connection_pragmas_with_key(conn, encryption_key)?;
    run_migrations(conn)?;
    Ok(())
}

/// Sourced database encryption key from OS keyring or environment
pub fn get_keyring_encryption_key(service: &str, user: &str) -> Option<Zeroizing<String>> {
    if let Ok(key) = std::env::var("VESPETREL_DB_KEY")
        && !key.is_empty()
    {
        return Some(Zeroizing::new(key));
    }
    if let Ok(entry) = keyring::Entry::new(service, user)
        && let Ok(secret) = entry.get_password()
        && !secret.is_empty()
    {
        return Some(Zeroizing::new(secret));
    }
    None
}

/// Helper to open an in-memory DB for tests with full schema
pub fn open_in_memory() -> crate::StorageResult<Connection> {
    let mut conn = Connection::open_in_memory()?;
    init_connection(&mut conn)?;
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
        let mut conn = Connection::open_in_memory().unwrap();
        init_connection_with_key(&mut conn, Some("test_secret_key_123")).unwrap();
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }
}
