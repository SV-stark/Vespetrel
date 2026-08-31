use rusqlite::Connection;

/// Run all DDL migrations - §3.2 with version tracking
pub fn run_migrations(conn: &Connection) -> anyhow::Result<()> {
    // 1. Ensure migration tracking table exists
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS _schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at INTEGER NOT NULL
        );
        "#,
    )?;

    // Check if migration version 1 has been applied
    let is_v1_applied: bool = conn
        .query_row(
            "SELECT count(*) FROM _schema_migrations WHERE version = 1",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|count| count > 0)?;

    if !is_v1_applied {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(
            r#"
            -- Accounts

            CREATE TABLE IF NOT EXISTS accounts (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                email TEXT NOT NULL,
                provider_type TEXT NOT NULL,
                auth_config TEXT NOT NULL,
                sync_state TEXT NOT NULL DEFAULT '{}',
                is_active INTEGER NOT NULL DEFAULT 1,
                color TEXT,
                created_at INTEGER NOT NULL
            );

            -- Folders
            CREATE TABLE IF NOT EXISTS folders (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                remote_id TEXT NOT NULL,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                role TEXT NOT NULL DEFAULT 'custom',
                uid_validity INTEGER,
                highest_mod_seq INTEGER DEFAULT 0,
                total_count INTEGER DEFAULT 0,
                unread_count INTEGER DEFAULT 0,
                color TEXT,
                UNIQUE(account_id, remote_id)
            );

            CREATE INDEX IF NOT EXISTS idx_folders_account ON folders(account_id);

            -- Threads
            CREATE TABLE IF NOT EXISTS threads (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                subject TEXT,
                last_message_at INTEGER NOT NULL,
                message_count INTEGER NOT NULL DEFAULT 1,
                unread_count INTEGER NOT NULL DEFAULT 0,
                snippet TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_threads_account ON threads(account_id);

            -- Messages
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                folder_id TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
                thread_id TEXT REFERENCES threads(id) ON DELETE SET NULL,
                remote_uid INTEGER NOT NULL,
                message_id_header TEXT,
                in_reply_to TEXT,
                references_header TEXT,
                subject TEXT,

                from_address TEXT NOT NULL,
                from_name TEXT,
                to_addresses TEXT NOT NULL,
                cc_addresses TEXT NOT NULL,
                bcc_addresses TEXT NOT NULL,
                reply_to TEXT,
                sent_at INTEGER NOT NULL,
                received_at INTEGER NOT NULL,
                is_read INTEGER NOT NULL DEFAULT 0,
                is_flagged INTEGER NOT NULL DEFAULT 0,
                is_draft INTEGER NOT NULL DEFAULT 0,
                has_attachments INTEGER NOT NULL DEFAULT 0,
                body_snippet TEXT,
                body_text_preview TEXT,
                blob_path TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                UNIQUE(folder_id, remote_uid)
            );
            CREATE INDEX IF NOT EXISTS idx_messages_folder ON messages(folder_id);
            CREATE INDEX IF NOT EXISTS idx_messages_folder_sent ON messages(folder_id, sent_at DESC);
            CREATE INDEX IF NOT EXISTS idx_messages_thread ON messages(thread_id);
            CREATE INDEX IF NOT EXISTS idx_messages_account ON messages(account_id);
            CREATE INDEX IF NOT EXISTS idx_messages_sent ON messages(sent_at DESC);

            -- Labels
            CREATE TABLE IF NOT EXISTS message_labels (
                message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
                label TEXT NOT NULL,
                PRIMARY KEY(message_id, label)
            );

            -- Attachments
            CREATE TABLE IF NOT EXISTS attachments (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
                content_id TEXT,
                filename TEXT NOT NULL,
                content_type TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                blob_path TEXT NOT NULL,
                is_inline INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_attachments_message ON attachments(message_id);

            -- FTS5 virtual table
            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                message_id UNINDEXED,
                account_id UNINDEXED,
                subject,
                from_address,
                from_name,
                to_addresses,
                body_content,
                tokenize = 'unicode61'
            );

            -- Triggers for FTS5 sync
            CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
                INSERT INTO messages_fts(message_id, account_id, subject, from_address, from_name, to_addresses, body_content)
                VALUES (new.id, new.account_id, new.subject, new.from_address, new.from_name, new.to_addresses, new.body_text_preview);
            END;

            CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
                DELETE FROM messages_fts WHERE message_id = old.id;
            END;

            CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages
            WHEN old.subject IS NOT new.subject
              OR old.account_id IS NOT new.account_id
              OR old.body_text_preview IS NOT new.body_text_preview
              OR old.from_address IS NOT new.from_address
              OR old.from_name IS NOT new.from_name
              OR old.to_addresses IS NOT new.to_addresses
            BEGIN
                DELETE FROM messages_fts WHERE message_id = old.id;
                INSERT INTO messages_fts(message_id, account_id, subject, from_address, from_name, to_addresses, body_content)
                VALUES (new.id, new.account_id, new.subject, new.from_address, new.from_name, new.to_addresses, new.body_text_preview);
            END;


            -- Calendar & Contacts (PIM)
            CREATE TABLE IF NOT EXISTS calendars (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                remote_id TEXT NOT NULL,
                name TEXT NOT NULL,
                color TEXT,
                UNIQUE(account_id, remote_id)
            );

            CREATE TABLE IF NOT EXISTS calendar_events (
                id TEXT PRIMARY KEY,
                calendar_id TEXT NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
                ical_uid TEXT,
                title TEXT NOT NULL,
                description TEXT,
                start_at INTEGER NOT NULL,
                end_at INTEGER NOT NULL,
                location TEXT,
                raw_ical TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_calendar_events_calendar ON calendar_events(calendar_id);

            CREATE TABLE IF NOT EXISTS contacts (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                remote_id TEXT,
                display_name TEXT,
                email TEXT NOT NULL,
                vcard_data TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_contacts_account ON contacts(account_id);

            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                calendar_id TEXT NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
                ical_uid TEXT,
                title TEXT NOT NULL,
                description TEXT,
                due_at INTEGER,
                is_completed INTEGER NOT NULL DEFAULT 0,
                completed_at INTEGER,
                priority INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_tasks_calendar ON tasks(calendar_id);

            -- Signatures
            CREATE TABLE IF NOT EXISTS signatures (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL,
                name TEXT NOT NULL,
                raw_html TEXT NOT NULL,
                plain_text TEXT,
                is_default INTEGER NOT NULL DEFAULT 0,
                include_in_replies INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_signatures_account ON signatures(account_id);

            -- User Settings Key-Value JSON store
            CREATE TABLE IF NOT EXISTS user_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            "#,
        )?;

        // Record migration version 1
        let now = chrono::Utc::now().timestamp();
        tx.execute(
            "INSERT OR IGNORE INTO _schema_migrations (version, name, applied_at) VALUES (1, 'initial_schema_fts5', ?1)",
            [now],
        )?;
        tx.commit()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn migrations_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap(); // second run should not fail
        // verify tables exist
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='messages'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let v1_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM _schema_migrations WHERE version = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v1_count, 1);
    }
}
