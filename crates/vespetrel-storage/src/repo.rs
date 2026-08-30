use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use vespetrel_core::{Account, Folder, Message};

// Simple synchronous repository helpers - callers use deadpool-sqlite threadpool or blocking

pub fn upsert_account(conn: &Connection, acct: &Account) -> anyhow::Result<()> {
    conn.execute(
        r#"INSERT INTO accounts (id, name, email, provider_type, auth_config, sync_state, is_active, created_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
           ON CONFLICT(id) DO UPDATE SET
             name=excluded.name, email=excluded.email, provider_type=excluded.provider_type,
             auth_config=excluded.auth_config, sync_state=excluded.sync_state, is_active=excluded.is_active"#,
        params![
            acct.id,
            acct.name,
            acct.email,
            acct.provider_type.to_string(),
            serde_json::to_string(&acct.auth_config)?,
            serde_json::to_string(&acct.sync_state)?,
            if acct.is_active { 1 } else { 0 },
            acct.created_at.timestamp(),
        ],
    )?;
    Ok(())
}

pub fn list_accounts(conn: &Connection) -> anyhow::Result<Vec<Account>> {
    let mut stmt = conn.prepare("SELECT id, name, email, provider_type, auth_config, sync_state, is_active, created_at FROM accounts")?;
    let rows = stmt.query_map([], |row| {
        let pt_str: String = row.get(3)?;
        let pt = pt_str.parse().unwrap_or(vespetrel_core::ProviderType::Imap);
        let auth_json: String = row.get(4)?;
        let sync_json: String = row.get(5)?;
        Ok(Account {
            id: row.get(0)?,
            name: row.get(1)?,
            email: row.get(2)?,
            provider_type: pt,
            auth_config: serde_json::from_str(&auth_json).unwrap_or_default(),
            sync_state: serde_json::from_str(&sync_json).unwrap_or_default(),
            is_active: row.get::<_, i64>(6)? != 0,
            created_at: DateTime::from_timestamp(row.get::<_, i64>(7)?, 0).unwrap_or_else(Utc::now),
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn upsert_folder(conn: &Connection, folder: &Folder) -> anyhow::Result<()> {
    conn.execute(
        r#"INSERT INTO folders (id, account_id, remote_id, name, path, role, uid_validity, highest_mod_seq, total_count, unread_count)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
           ON CONFLICT(account_id, remote_id) DO UPDATE SET
             name=excluded.name, path=excluded.path, role=excluded.role,
             uid_validity=excluded.uid_validity, highest_mod_seq=excluded.highest_mod_seq,
             total_count=excluded.total_count, unread_count=excluded.unread_count"#,
        params![
            folder.id,
            folder.account_id,
            folder.remote_id,
            folder.name,
            folder.path,
            folder.role.to_string(),
            folder.uid_validity.map(|v| v as i64),
            folder.highest_mod_seq.map(|v| v as i64),
            folder.total_count,
            folder.unread_count,
        ],
    )?;
    Ok(())
}

pub fn list_folders(conn: &Connection, account_id: &str) -> anyhow::Result<Vec<Folder>> {
    let mut stmt = conn.prepare("SELECT id, account_id, remote_id, name, path, role, uid_validity, highest_mod_seq, total_count, unread_count FROM folders WHERE account_id = ?1")?;
    let rows = stmt.query_map(params![account_id], |row| {
        let role_str: String = row.get(5)?;
        Ok(Folder {
            id: row.get(0)?,
            account_id: row.get(1)?,
            remote_id: row.get(2)?,
            name: row.get(3)?,
            path: row.get(4)?,
            role: role_str.parse().unwrap_or(vespetrel_core::FolderRole::Custom),
            uid_validity: row.get::<_, Option<i64>>(6)?.map(|v| v as u32),
            highest_mod_seq: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
            total_count: row.get(8)?,
            unread_count: row.get(9)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_folder(conn: &Connection, account_id: &str, remote_id: &str) -> anyhow::Result<Option<Folder>> {
    let mut stmt = conn.prepare("SELECT id, account_id, remote_id, name, path, role, uid_validity, highest_mod_seq, total_count, unread_count FROM folders WHERE account_id = ?1 AND remote_id = ?2")?;
    stmt.query_row(params![account_id, remote_id], |row| {
        let role_str: String = row.get(5)?;
        Ok(Folder {
            id: row.get(0)?,
            account_id: row.get(1)?,
            remote_id: row.get(2)?,
            name: row.get(3)?,
            path: row.get(4)?,
            role: role_str.parse().unwrap_or(vespetrel_core::FolderRole::Custom),
            uid_validity: row.get::<_, Option<i64>>(6)?.map(|v| v as u32),
            highest_mod_seq: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
            total_count: row.get(8)?,
            unread_count: row.get(9)?,
        })
    })
    .optional()
    .map_err(Into::into)
}

pub fn insert_message(conn: &Connection, msg: &Message) -> anyhow::Result<()> {
    conn.execute(
        r#"INSERT INTO messages (id, account_id, folder_id, thread_id, remote_uid, message_id_header, in_reply_to, subject, from_address, from_name, to_addresses, cc_addresses, bcc_addresses, reply_to, sent_at, received_at, is_read, is_flagged, is_draft, has_attachments, body_snippet, body_text_preview, blob_path, size_bytes)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24)
           ON CONFLICT(folder_id, remote_uid) DO UPDATE SET
             thread_id=excluded.thread_id, subject=excluded.subject, is_read=excluded.is_read,
             is_flagged=excluded.is_flagged, body_snippet=excluded.body_snippet"#,
        params![
            msg.id,
            msg.account_id,
            msg.folder_id,
            msg.thread_id,
            msg.remote_uid as i64,
            msg.message_id_header,
            msg.in_reply_to,
            msg.subject,
            msg.from_address,
            msg.from_name,
            serde_json::to_string(&msg.to_addresses)?,
            serde_json::to_string(&msg.cc_addresses)?,
            serde_json::to_string(&msg.bcc_addresses)?,
            msg.reply_to.as_ref().map(|v| serde_json::to_string(v).unwrap()).unwrap_or("null".to_string()),
            msg.sent_at.timestamp(),
            msg.received_at.timestamp(),
            if msg.is_read { 1 } else { 0 },
            if msg.is_flagged { 1 } else { 0 },
            if msg.is_draft { 1 } else { 0 },
            if msg.has_attachments { 1 } else { 0 },
            msg.body_snippet,
            msg.body_text_preview,
            msg.blob_path,
            msg.size_bytes,
        ],
    )?;
    Ok(())
}

pub fn list_messages_in_folder(conn: &Connection, folder_id: &str, limit: usize, offset: usize) -> anyhow::Result<Vec<Message>> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, folder_id, thread_id, remote_uid, message_id_header, in_reply_to, subject, from_address, from_name, to_addresses, cc_addresses, bcc_addresses, reply_to, sent_at, received_at, is_read, is_flagged, is_draft, has_attachments, body_snippet, body_text_preview, blob_path, size_bytes FROM messages WHERE folder_id = ?1 ORDER BY sent_at DESC LIMIT ?2 OFFSET ?3"
    )?;
    let rows = stmt.query_map(params![folder_id, limit as i64, offset as i64], |row| {
        Ok(Message {
            id: row.get(0)?,
            account_id: row.get(1)?,
            folder_id: row.get(2)?,
            thread_id: row.get(3)?,
            remote_uid: row.get::<_, i64>(4)? as u32,
            message_id_header: row.get(5)?,
            in_reply_to: row.get(6)?,
            subject: row.get(7)?,
            from_address: row.get(8)?,
            from_name: row.get(9)?,
            to_addresses: serde_json::from_str(&row.get::<_, String>(10)?).unwrap_or_default(),
            cc_addresses: serde_json::from_str(&row.get::<_, String>(11)?).unwrap_or_default(),
            bcc_addresses: serde_json::from_str(&row.get::<_, String>(12)?).unwrap_or_default(),
            reply_to: row.get::<_, Option<String>>(13)?.and_then(|s| serde_json::from_str(&s).ok()),
            sent_at: DateTime::from_timestamp(row.get::<_, i64>(14)?, 0).unwrap_or_else(Utc::now),
            received_at: DateTime::from_timestamp(row.get::<_, i64>(15)?, 0).unwrap_or_else(Utc::now),
            is_read: row.get::<_, i64>(16)? != 0,
            is_flagged: row.get::<_, i64>(17)? != 0,
            is_draft: row.get::<_, i64>(18)? != 0,
            has_attachments: row.get::<_, i64>(19)? != 0,
            body_snippet: row.get(20)?,
            body_text_preview: row.get(21)?,
            blob_path: row.get(22)?,
            size_bytes: row.get(23)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}
