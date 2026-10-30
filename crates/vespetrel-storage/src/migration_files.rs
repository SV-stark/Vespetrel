//! Migration runner over numbered `.sql` files.
//!
//! Why files rather than inline `execute_batch` calls: each migration is then independently
//! reviewable in a diff, applied in numeric order, and applied exactly once with a recorded
//! name. The previous inline form buried five migrations in one 478-line function where a
//! change to the last one required reading the first four to confirm it was safe.
//!
//! Adding a migration:
//!
//! 1. Create `sql/NNN_name.sql`. Numbers must be contiguous and ascending; `006` follows `005`.
//! 2. Append an entry to [`MIGRATIONS`] with its version, name and an optional
//!    [`MigrationGuard`] for anything SQLite cannot express in plain SQL (conditional DDL,
//!    data backfills).
//!
//! Migrations may contain data migrations (`UPDATE`, `INSERT`) as well as DDL. That matters:
//! the previous set was DDL-only, so an upgraded row kept optimistic capability flags that a
//! newly discovered provider session would have contradicted.

use rusqlite::Connection;
use tracing::info;

use crate::StorageResult;

/// A migration whose SQL cannot run unconditionally.
///
/// `pragma_table_info` lookups and similar introspection need Rust, so the runner yields to a
/// callback with an open transaction and the caller decides what to run.
///
/// `Sync` so the static table can be shared across threads; migrations run single-threaded at
/// startup, but the type must still be safe to reference from the static.
pub type MigrationGuard = dyn Fn(&mut rusqlite::Transaction) -> StorageResult<()> + Sync;

/// One versioned schema step.
pub struct Migration {
    pub version: i64,
    /// Stable identifier recorded in `_schema_migrations`. Never renamed once shipped: the
    /// recorded name is how a support conversation identifies what a database has run.
    pub name: &'static str,
    /// The DDL/DML to execute inside a single exclusive transaction.
    pub sql: &'static str,
    /// Optional hook for conditional steps. Runs inside the same transaction as `sql`.
    pub guard: Option<&'static MigrationGuard>,
}

/// Every migration, in ascending version order.
///
/// Version 1 *is* applied by this runner, not baked into `init_connection`: keeping the
/// baseline in the same sequence as everything after it means a fresh database and an upgraded
/// one converge through identical code, so the two cannot drift.
pub static MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema_fts5",
        sql: include_str!("../sql/001_initial_schema.sql"),
        guard: None,
    },
    Migration {
        version: 2,
        name: "fts5_unicode61_diacritics_rebuild",
        sql: include_str!("../sql/002_fts5_unicode61_diacritics.sql"),
        guard: None,
    },
    Migration {
        version: 3,
        name: "messages_remote_id_col",
        sql: include_str!("../sql/003_messages_remote_id.sql"),
        // `ALTER TABLE ... ADD COLUMN` has no IF NOT EXISTS in SQLite, so the column itself is
        // added conditionally here.
        guard: Some(&ensure_remote_id_column),
    },
    Migration {
        version: 4,
        name: "outbox_table",
        sql: include_str!("../sql/004_outbox.sql"),
        guard: None,
    },
    Migration {
        version: 5,
        name: "fts5_update_trigger_account_id",
        sql: include_str!("../sql/005_fts5_update_trigger_account_id.sql"),
        guard: None,
    },
    Migration {
        version: 6,
        name: "account_aliases",
        sql: include_str!("../sql/006_account_aliases.sql"),
        guard: None,
    },
];

/// Add `messages.remote_id` only when an older database predates the column.
///
/// `ALTER TABLE ... ADD COLUMN` has no `IF NOT EXISTS` in SQLite, so a database that already
/// has the column would fail the whole migration. Introspecting first keeps the runner
/// idempotent regardless of which path created the schema.
fn ensure_remote_id_column(tx: &mut rusqlite::Transaction) -> StorageResult<()> {
    let has_remote_id: bool = tx
        .query_row(
            "SELECT count(*) FROM pragma_table_info('messages') WHERE name = 'remote_id'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .unwrap_or(false);

    if !has_remote_id {
        tx.execute_batch("ALTER TABLE messages ADD COLUMN remote_id TEXT")?;
    }
    Ok(())
}

/// Apply every migration that has not been recorded yet.
///
/// Idempotent: a second call is a no-op. Each migration runs in its own exclusive
/// transaction, so a failure part-way leaves earlier versions applied and the failing one
/// rolled back rather than leaving a half-migrated schema.
pub fn run(conn: &mut Connection) -> StorageResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS _schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at INTEGER NOT NULL
        );
        "#,
    )?;

    // Deliberately *not* short-circuiting on `PRAGMA user_version`. It is a denormalised copy of
    // `_schema_migrations` and the two can disagree — a restored backup, a hand-edited database,
    // or a version bumped outside the transaction. Trusting it there silently skips migrations
    // the recorded history says were never applied, leaving a database that opens fine and then
    // fails at the first query touching a missing column. The per-migration check below costs
    // one indexed SELECT per version.
    for migration in MIGRATIONS {
        let already_applied = conn
            .query_row(
                "SELECT count(*) FROM _schema_migrations WHERE version = ?1",
                [migration.version],
                |r| r.get::<_, i64>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(false);
        if already_applied {
            continue;
        }

        let mut tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Exclusive)?;
        // The guard runs first. It exists for conditional preparatory work — version 3 adds
        // `messages.remote_id` before the file indexes it — so SQL that depends on a guard's
        // output would fail if the order were reversed.
        if let Some(guard) = migration.guard {
            guard(&mut tx)?;
        }
        tx.execute_batch(migration.sql)?;
        tx.execute(
            "INSERT OR IGNORE INTO _schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                migration.version,
                migration.name,
                chrono::Utc::now().timestamp()
            ],
        )?;
        tx.commit()?;

        // `PRAGMA user_version` does not accept a bound parameter.
        conn.execute_batch(&format!("PRAGMA user_version = {}", migration.version))?;
        info!(
            version = migration.version,
            name = migration.name,
            "applied schema migration"
        );
    }

    Ok(())
}

/// Every `(version, name)` recorded in a database, ascending.
///
/// Used by tests and diagnostics to report what a database has actually run.
pub fn applied(conn: &Connection) -> StorageResult<Vec<(i64, String)>> {
    let has_table: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='_schema_migrations'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);
    if !has_table {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT version, name FROM _schema_migrations ORDER BY version")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    /// Version numbers must be contiguous and ascending.
    ///
    /// A gap means a file was renumbered or skipped. Since `PRAGMA user_version` gates the fast
    /// path, a gap could silently skip a migration for an existing database.
    #[test]
    fn migration_versions_are_contiguous_and_ascending() {
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(
                migration.version,
                index as i64 + 1,
                "migration {} ({}) has a version gap",
                migration.version,
                migration.name
            );
        }
    }

    /// Every migration needs a distinct, non-empty, snake_case name.
    ///
    /// The name is persisted in `_schema_migrations`, so a duplicate would make two
    /// migrations indistinguishable in support output.
    #[test]
    fn migration_names_are_unique_and_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for migration in MIGRATIONS {
            assert!(!migration.name.is_empty(), "migration has no name");
            assert!(
                seen.insert(migration.name),
                "duplicate migration name {}",
                migration.name
            );
            assert!(
                migration
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "migration name {} should be snake_case",
                migration.name
            );
        }
    }

    /// No migration file may be empty.
    ///
    /// An empty file compiles happily and would record a version as applied without changing
    /// anything, hiding the mistake until the missing column was needed at runtime.
    #[test]
    fn migration_files_are_not_empty() {
        for migration in MIGRATIONS {
            assert!(
                !migration.sql.trim().is_empty(),
                "migration {} ({}) has an empty SQL file",
                migration.version,
                migration.name
            );
        }
    }

    #[test]
    fn running_migrations_is_idempotent() {
        let mut conn = open_in_memory().expect("open db");
        run(&mut conn).expect("first run");
        let after_first = applied(&conn).expect("read applied");
        assert!(!after_first.is_empty());

        run(&mut conn).expect("second run");
        assert_eq!(
            applied(&conn).expect("read applied"),
            after_first,
            "a second run must not re-apply or duplicate migrations"
        );
    }

    #[test]
    fn every_migration_is_recorded_after_a_run() {
        let mut conn = open_in_memory().expect("open db");
        run(&mut conn).expect("run");
        let recorded: Vec<i64> = applied(&conn)
            .expect("read applied")
            .into_iter()
            .map(|(v, _)| v)
            .collect();
        let expected: Vec<i64> = MIGRATIONS.iter().map(|m| m.version).collect();
        assert_eq!(
            recorded, expected,
            "every declared migration must be recorded"
        );
    }

    #[test]
    fn user_version_tracks_the_newest_migration() {
        let mut conn = open_in_memory().expect("open db");
        run(&mut conn).expect("run");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("read user_version");
        assert_eq!(version, MIGRATIONS.iter().map(|m| m.version).max().unwrap());
    }

    #[test]
    fn recorded_history_is_authoritative_over_pragma_user_version() {
        // `PRAGMA user_version` is a denormalised copy of the recorded history and the two can
        // disagree. Trusting the pragma would skip every migration below the recorded version,
        // leaving a database that opens cleanly and then fails on a missing column.
        let mut conn = open_in_memory().expect("open db");
        run(&mut conn).expect("run");

        // Simulate a restored backup: history says "never applied", pragma says "current".
        conn.execute_batch("DELETE FROM _schema_migrations")
            .expect("clear history");
        let newest = MIGRATIONS.iter().map(|m| m.version).max().unwrap();
        conn.execute_batch(&format!("PRAGMA user_version = {newest}"))
            .expect("bump pragma");

        run(&mut conn).expect("re-run");
        assert_eq!(
            applied(&conn).expect("read applied").len(),
            MIGRATIONS.len(),
            "the recorded history must win; a stale pragma must not skip migrations"
        );
    }

    #[test]
    fn applied_reports_nothing_for_an_unmigrated_database() {
        let conn = Connection::open_in_memory().expect("open db");
        assert!(applied(&conn).expect("read applied").is_empty());
    }
}
