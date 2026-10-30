//! Schema migration entry point.
//!
//! The migration bodies live in numbered `.sql` files under `sql/`, driven by
//! [`crate::migration_files`]. They used to be inline `execute_batch` calls in this module:
//! five migrations in one 478-line function, where reviewing a change to the last one meant
//! reading the first four to confirm it was safe. As files, each version is independently
//! reviewable, applied in numeric order, and applied exactly once.
//!
//! Add a migration by creating `sql/NNN_name.sql` and appending a [`Migration`] entry in
//! [`crate::migration_files::MIGRATIONS`]. Keep the numbers contiguous — the runner's
//! `PRAGMA user_version` fast path assumes no gaps.

use crate::StorageResult;
use rusqlite::Connection;

/// Run every migration that has not been recorded yet.
///
/// Idempotent: safe to call on every open, which is what `db::init_connection` does. Each
/// migration runs in its own exclusive transaction, so a failure leaves earlier versions
/// applied and the failing one rolled back rather than leaving a half-migrated schema.
pub fn run_migrations(conn: &mut Connection) -> StorageResult<()> {
    crate::migration_files::run(conn)
}

/// Explicit job to rebuild the FTS5 index from the `messages` table.
///
/// `messages_fts` is a *standalone* FTS5 table (not `content='messages'`), so
/// `INSERT INTO messages_fts(messages_fts) VALUES('rebuild')` is a no-op here: `'rebuild'`
/// only re-tokenizes an external-content table from its content table, and on a standalone
/// table it merely re-indexes rows that are already self-consistent. It would silently
/// succeed while leaving any index drift in place. Re-populate from `messages` instead.
pub fn rebuild_fts(conn: &Connection) -> StorageResult<()> {
    conn.execute_batch(
        r#"
        DELETE FROM messages_fts;
        INSERT INTO messages_fts(message_id, account_id, subject, from_address, from_name, to_addresses, body_content)
          SELECT id, account_id, subject, from_address, from_name, to_addresses,
                 COALESCE(body_text_preview, body_snippet, '')
          FROM messages;
        "#,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration_files::{MIGRATIONS, applied};

    #[test]
    fn migrations_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        run_migrations(&mut conn).unwrap(); // second run should not fail
        // verify tables exist
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('messages','accounts','folders','outbox')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 4, "core tables must all exist after migration");
    }

    #[test]
    fn every_migration_is_recorded_with_its_declared_name() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        let recorded = applied(&conn).unwrap();
        let expected: Vec<(i64, String)> = MIGRATIONS
            .iter()
            .map(|m| (m.version, m.name.to_string()))
            .collect();
        assert_eq!(
            recorded, expected,
            "recorded migration history must match the declared sequence exactly"
        );
    }

    #[test]
    fn remote_id_column_and_index_exist_after_migration() {
        // Version 3 adds the column conditionally; verify both halves landed.
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        let has_column: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('messages') WHERE name='remote_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_column, 1, "messages.remote_id must exist");

        let has_index: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='idx_messages_remote_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_index, 1, "idx_messages_remote_id must exist");
    }

    #[test]
    fn fts_update_trigger_watches_account_id() {
        // Regression guard for migration 5: version 2 recreated `messages_au` and dropped the
        // `account_id` arm, so a message moved between accounts kept its old account tag —
        // invisible to its new owner and still returned to the old one.
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='trigger' AND name='messages_au'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            sql.contains("old.account_id IS NOT new.account_id"),
            "messages_au must re-index on account_id change; trigger was:\n{sql}"
        );
    }

    #[test]
    fn rebuild_fts_restores_every_message() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        conn.execute_batch(
            "INSERT INTO accounts VALUES ('a','A','a@example.com','imap','{}','{}',1,NULL,0);
             INSERT INTO folders VALUES ('f','a','INBOX','INBOX','INBOX','inbox',NULL,0,1,1,NULL);
             INSERT INTO messages (id, account_id, folder_id, remote_uid, subject, from_address,
                to_addresses, cc_addresses, bcc_addresses, sent_at, received_at, blob_path,
                size_bytes, body_text_preview)
             VALUES ('m1','a','f',1,'Rechnung für März','s@example.com','a@example.com','','',1,1,'b',5,'Café');",
        )
        .unwrap();

        // Wipe the index to simulate drift, then rebuild from `messages`.
        conn.execute_batch("DELETE FROM messages_fts").unwrap();
        let before: i64 = conn
            .query_row("SELECT count(*) FROM messages_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, 0);

        rebuild_fts(&conn).unwrap();
        let after: i64 = conn
            .query_row("SELECT count(*) FROM messages_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            after, 1,
            "rebuild must restore every message into the index"
        );

        // The rebuilt index must still fold accents, or the migration's tokenizer was lost.
        let hits: i64 = conn
            .query_row(
                "SELECT count(*) FROM messages_fts WHERE messages_fts MATCH 'cafe'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1, "accent folding must survive the rebuild");
    }
}
