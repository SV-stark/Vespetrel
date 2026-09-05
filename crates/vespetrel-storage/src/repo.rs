use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use tracing::warn;

use crate::StorageResult;
use vespetrel_core::{Account, Folder, Message};

// Simple synchronous repository helpers - callers use deadpool-sqlite threadpool or blocking

pub fn upsert_account(conn: &Connection, acct: &Account) -> StorageResult<()> {
    conn.execute(
        r#"INSERT INTO accounts (id, name, email, provider_type, auth_config, sync_state, is_active, color, created_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
           ON CONFLICT(id) DO UPDATE SET
             name=excluded.name, email=excluded.email, provider_type=excluded.provider_type,
             auth_config=excluded.auth_config, sync_state=excluded.sync_state,
             is_active=excluded.is_active, color=excluded.color"#,
        params![
            acct.id,
            acct.name,
            acct.email,
            acct.provider_type.to_string(),
            serde_json::to_string(&acct.auth_config)?,
            serde_json::to_string(&acct.sync_state)?,
            if acct.is_active { 1 } else { 0 },
            acct.color,
            acct.created_at.timestamp(),
        ],
    )?;
    Ok(())
}

pub fn list_accounts(conn: &Connection) -> StorageResult<Vec<Account>> {
    let mut stmt = conn.prepare("SELECT id, name, email, provider_type, auth_config, sync_state, is_active, color, created_at FROM accounts")?;
    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let pt_str: String = row.get(3)?;
        let mut corrupted = false;
        let pt = match pt_str.parse() {
            Ok(p) => p,
            Err(e) => {
                warn!(account_id=%id, error=%e, raw=%pt_str, "corrupted provider_type in DB; marking inactive");
                corrupted = true;
                vespetrel_core::ProviderType::Imap
            }
        };
        let auth_json: String = row.get(4)?;
        let sync_json: String = row.get(5)?;
        let auth_config = match serde_json::from_str(&auth_json) {
            Ok(a) => a,
            Err(e) => {
                warn!(account_id=%id, error=%e, "corrupted auth_config JSON in DB; marking inactive");
                corrupted = true;
                Default::default()
            }
        };
        let sync_state = match serde_json::from_str(&sync_json) {
            Ok(s) => s,
            Err(e) => {
                warn!(account_id=%id, error=%e, "corrupted sync_state JSON in DB; marking inactive");
                corrupted = true;
                Default::default()
            }
        };
        let created_ts: i64 = row.get(8)?;
        let created_at = match DateTime::from_timestamp(created_ts, 0) {
            Some(dt) => dt,
            None => {
                warn!(account_id=%id, timestamp=created_ts, "corrupted created_at timestamp in DB; marking inactive");
                corrupted = true;
                DateTime::from_timestamp(0, 0).unwrap_or_default()
            }
        };
        let is_active = if corrupted {
            false
        } else {
            row.get::<_, i64>(6)? != 0
        };
        Ok(Account {
            id,
            name: row.get(1)?,
            email: row.get(2)?,
            provider_type: pt,
            auth_config,
            sync_state,
            is_active,
            color: row.get(7)?,
            created_at,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_account(conn: &Connection, id: &str) -> StorageResult<Option<Account>> {
    let mut stmt = conn.prepare("SELECT id, name, email, provider_type, auth_config, sync_state, is_active, color, created_at FROM accounts WHERE id = ?1")?;
    stmt.query_row(params![id], |row| {
        let id: String = row.get(0)?;
        let pt_str: String = row.get(3)?;
        let mut corrupted = false;
        let pt = match pt_str.parse() {
            Ok(p) => p,
            Err(e) => {
                warn!(account_id=%id, error=%e, raw=%pt_str, "corrupted provider_type in DB; marking inactive");
                corrupted = true;
                vespetrel_core::ProviderType::Imap
            }
        };
        let auth_json: String = row.get(4)?;
        let sync_json: String = row.get(5)?;
        let auth_config = match serde_json::from_str(&auth_json) {
            Ok(a) => a,
            Err(e) => {
                warn!(account_id=%id, error=%e, "corrupted auth_config JSON in DB; marking inactive");
                corrupted = true;
                Default::default()
            }
        };
        let sync_state = match serde_json::from_str(&sync_json) {
            Ok(s) => s,
            Err(e) => {
                warn!(account_id=%id, error=%e, "corrupted sync_state JSON in DB; marking inactive");
                corrupted = true;
                Default::default()
            }
        };
        let created_ts: i64 = row.get(8)?;
        let created_at = match DateTime::from_timestamp(created_ts, 0) {
            Some(dt) => dt,
            None => {
                warn!(account_id=%id, timestamp=created_ts, "corrupted created_at timestamp in DB; marking inactive");
                corrupted = true;
                DateTime::from_timestamp(0, 0).unwrap_or_default()
            }
        };
        let is_active = if corrupted {
            false
        } else {
            row.get::<_, i64>(6)? != 0
        };
        Ok(Account {
            id,
            name: row.get(1)?,
            email: row.get(2)?,
            provider_type: pt,
            auth_config,
            sync_state,
            is_active,
            color: row.get(7)?,
            created_at,
        })
    })
    .optional()
    .map_err(Into::into)
}

pub fn upsert_folder(conn: &Connection, folder: &Folder) -> StorageResult<()> {
    conn.execute(
        r#"INSERT INTO folders (id, account_id, remote_id, name, path, role, uid_validity, highest_mod_seq, total_count, unread_count, color)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
           ON CONFLICT(account_id, remote_id) DO UPDATE SET
             name=excluded.name, path=excluded.path, role=excluded.role,
             uid_validity=excluded.uid_validity, highest_mod_seq=excluded.highest_mod_seq,
             total_count=excluded.total_count, unread_count=excluded.unread_count, color=excluded.color"#,
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
            folder.color,
        ],
    )?;
    Ok(())
}

pub fn list_folders(conn: &Connection, account_id: &str) -> StorageResult<Vec<Folder>> {
    let mut stmt = conn.prepare("SELECT id, account_id, remote_id, name, path, role, uid_validity, highest_mod_seq, total_count, unread_count, color FROM folders WHERE account_id = ?1")?;
    let rows = stmt.query_map(params![account_id], |row| {
        let role_str: String = row.get(5)?;
        Ok(Folder {
            id: row.get(0)?,
            account_id: row.get(1)?,
            remote_id: row.get(2)?,
            name: row.get(3)?,
            path: row.get(4)?,
            role: role_str
                .parse()
                .unwrap_or(vespetrel_core::FolderRole::Custom),
            uid_validity: row.get::<_, Option<i64>>(6)?.map(|v| v as u32),
            highest_mod_seq: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
            total_count: row.get(8)?,
            unread_count: row.get(9)?,
            color: row.get(10)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_folder(
    conn: &Connection,
    account_id: &str,
    remote_id: &str,
) -> StorageResult<Option<Folder>> {
    let mut stmt = conn.prepare("SELECT id, account_id, remote_id, name, path, role, uid_validity, highest_mod_seq, total_count, unread_count, color FROM folders WHERE account_id = ?1 AND remote_id = ?2")?;
    stmt.query_row(params![account_id, remote_id], |row| {
        let role_str: String = row.get(5)?;
        Ok(Folder {
            id: row.get(0)?,
            account_id: row.get(1)?,
            remote_id: row.get(2)?,
            name: row.get(3)?,
            path: row.get(4)?,
            role: role_str
                .parse()
                .unwrap_or(vespetrel_core::FolderRole::Custom),
            uid_validity: row.get::<_, Option<i64>>(6)?.map(|v| v as u32),
            highest_mod_seq: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
            total_count: row.get(8)?,
            unread_count: row.get(9)?,
            color: row.get(10)?,
        })
    })
    .optional()
    .map_err(Into::into)
}

pub fn insert_message(conn: &Connection, msg: &Message) -> StorageResult<()> {
    let reply_to_json = msg
        .reply_to
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let to_addresses_json = serde_json::to_string(&msg.to_addresses)?;
    let cc_addresses_json = serde_json::to_string(&msg.cc_addresses)?;
    let bcc_addresses_json = serde_json::to_string(&msg.bcc_addresses)?;

    conn.execute(
        r#"INSERT INTO messages (id, account_id, folder_id, thread_id, remote_uid, message_id_header, in_reply_to, references_header, subject, from_address, from_name, to_addresses, cc_addresses, bcc_addresses, reply_to, sent_at, received_at, is_read, is_flagged, is_draft, has_attachments, body_snippet, body_text_preview, blob_path, size_bytes, remote_id)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)
           ON CONFLICT(folder_id, remote_uid) DO UPDATE SET
             thread_id=excluded.thread_id,
             message_id_header=excluded.message_id_header,
             in_reply_to=excluded.in_reply_to,
             references_header=excluded.references_header,
             subject=excluded.subject,
             from_address=excluded.from_address,
             from_name=excluded.from_name,
             to_addresses=excluded.to_addresses,
             cc_addresses=excluded.cc_addresses,
             bcc_addresses=excluded.bcc_addresses,
             reply_to=excluded.reply_to,
             sent_at=excluded.sent_at,
             received_at=excluded.received_at,
             is_read=excluded.is_read,
             is_flagged=excluded.is_flagged,
             is_draft=excluded.is_draft,
             has_attachments=excluded.has_attachments,
             body_snippet=excluded.body_snippet,
             body_text_preview=excluded.body_text_preview,
             blob_path=excluded.blob_path,
             size_bytes=excluded.size_bytes,
             remote_id=COALESCE(excluded.remote_id, messages.remote_id)"#,
        params![
            msg.id,
            msg.account_id,
            msg.folder_id,
            msg.thread_id,
            msg.remote_uid as i64,
            msg.message_id_header,
            msg.in_reply_to,
            msg.references,
            msg.subject,
            msg.from_address,
            msg.from_name,
            to_addresses_json,
            cc_addresses_json,
            bcc_addresses_json,
            reply_to_json,
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
            msg.remote_id,
        ],
    )?;

    Ok(())
}

pub fn list_messages_in_folder(
    conn: &Connection,
    folder_id: &str,
    limit: usize,
    offset: usize,
) -> StorageResult<Vec<Message>> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, folder_id, thread_id, remote_uid, message_id_header, in_reply_to, references_header, subject, from_address, from_name, to_addresses, cc_addresses, bcc_addresses, reply_to, sent_at, received_at, is_read, is_flagged, is_draft, has_attachments, body_snippet, body_text_preview, blob_path, size_bytes, remote_id FROM messages WHERE folder_id = ?1 ORDER BY sent_at DESC LIMIT ?2 OFFSET ?3"
    )?;
    let rows = stmt.query_map(params![folder_id, limit as i64, offset as i64], |row| {
        let to_json: String = row.get(11)?;
        let to_addresses = serde_json::from_str(&to_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                11,
                rusqlite::types::Type::Text,
                Box::new(crate::StorageError::CorruptData(format!(
                    "Corrupted to_addresses JSON: {e}"
                ))),
            )
        })?;
        let cc_json: String = row.get(12)?;
        let cc_addresses = serde_json::from_str(&cc_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                12,
                rusqlite::types::Type::Text,
                Box::new(crate::StorageError::CorruptData(format!(
                    "Corrupted cc_addresses JSON: {e}"
                ))),
            )
        })?;
        let bcc_json: String = row.get(13)?;
        let bcc_addresses = serde_json::from_str(&bcc_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                13,
                rusqlite::types::Type::Text,
                Box::new(crate::StorageError::CorruptData(format!(
                    "Corrupted bcc_addresses JSON: {e}"
                ))),
            )
        })?;
        let reply_to = row
            .get::<_, Option<String>>(14)?
            .and_then(|s| serde_json::from_str(&s).ok());
        let sent_ts: i64 = row.get(15)?;
        let sent_at = DateTime::from_timestamp(sent_ts, 0).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                15,
                rusqlite::types::Type::Integer,
                Box::new(crate::StorageError::CorruptData(format!(
                    "Invalid sent_at timestamp: {sent_ts}"
                ))),
            )
        })?;
        let recv_ts: i64 = row.get(16)?;
        let received_at = DateTime::from_timestamp(recv_ts, 0).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                16,
                rusqlite::types::Type::Integer,
                Box::new(crate::StorageError::CorruptData(format!(
                    "Invalid received_at timestamp: {recv_ts}"
                ))),
            )
        })?;
        Ok(Message {
            id: row.get(0)?,
            account_id: row.get(1)?,
            folder_id: row.get(2)?,
            thread_id: row.get(3)?,
            remote_uid: row.get::<_, i64>(4)? as u32,
            message_id_header: row.get(5)?,
            in_reply_to: row.get(6)?,
            references: row.get(7)?,
            subject: row.get(8)?,
            from_address: row.get(9)?,
            from_name: row.get(10)?,
            to_addresses,
            cc_addresses,
            bcc_addresses,
            reply_to,
            sent_at,
            received_at,
            is_read: row.get::<_, i64>(17)? != 0,
            is_flagged: row.get::<_, i64>(18)? != 0,
            is_draft: row.get::<_, i64>(19)? != 0,
            has_attachments: row.get::<_, i64>(20)? != 0,
            body_snippet: row.get(21)?,
            body_text_preview: row.get(22)?,
            blob_path: row.get(23)?,
            size_bytes: row.get(24)?,
            remote_id: row.get(25).ok(),
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn update_message_flags(
    conn: &Connection,
    message_id: &str,
    is_read: Option<bool>,
    is_flagged: Option<bool>,
) -> StorageResult<()> {
    let read_val = is_read.map(|r| if r { 1 } else { 0 });
    let flag_val = is_flagged.map(|f| if f { 1 } else { 0 });
    let rows = conn.execute(
        "UPDATE messages SET 
            is_read = COALESCE(?1, is_read),
            is_flagged = COALESCE(?2, is_flagged)
         WHERE id = ?3",
        params![read_val, flag_val, message_id],
    )?;
    if rows == 0 {
        return Err(crate::StorageError::NotFound(message_id.to_string()));
    }
    Ok(())
}

pub fn delete_message(conn: &Connection, message_id: &str) -> StorageResult<()> {
    let rows = conn.execute("DELETE FROM messages WHERE id = ?1", params![message_id])?;
    if rows == 0 {
        return Err(crate::StorageError::NotFound(message_id.to_string()));
    }
    Ok(())
}

pub fn upsert_thread(conn: &Connection, thread: &vespetrel_core::Thread) -> StorageResult<()> {
    conn.execute(
        r#"INSERT INTO threads (id, account_id, subject, last_message_at, message_count, unread_count, snippet)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
           ON CONFLICT(id) DO UPDATE SET
             subject=excluded.subject, last_message_at=excluded.last_message_at,
             message_count=excluded.message_count, unread_count=excluded.unread_count, snippet=excluded.snippet"#,
        params![
            thread.id,
            thread.account_id,
            thread.subject,
            thread.last_message_at.timestamp(),
            thread.message_count,
            thread.unread_count,
            thread.snippet,
        ],
    )?;
    Ok(())
}

pub fn list_threads(
    conn: &Connection,
    account_id: &str,
) -> StorageResult<Vec<vespetrel_core::Thread>> {
    let mut stmt = conn.prepare("SELECT id, account_id, subject, last_message_at, message_count, unread_count, snippet FROM threads WHERE account_id = ?1 ORDER BY last_message_at DESC")?;
    let rows = stmt.query_map(params![account_id], |row| {
        let last_ts: i64 = row.get(3)?;
        let last_message_at = DateTime::from_timestamp(last_ts, 0).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Integer,
                Box::new(crate::StorageError::CorruptData(format!(
                    "Invalid last_message_at timestamp: {last_ts}"
                ))),
            )
        })?;
        Ok(vespetrel_core::Thread {
            id: row.get(0)?,
            account_id: row.get(1)?,
            subject: row.get(2)?,
            last_message_at,
            message_count: row.get(4)?,
            unread_count: row.get(5)?,
            snippet: row.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn upsert_contact(
    conn: &Connection,
    account_id: &str,
    contact: &vespetrel_core::Contact,
) -> StorageResult<()> {
    conn.execute(
        r#"INSERT INTO contacts (id, account_id, remote_id, display_name, email, vcard_data)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6)
           ON CONFLICT(id) DO UPDATE SET
             remote_id=excluded.remote_id, display_name=excluded.display_name, email=excluded.email, vcard_data=excluded.vcard_data"#,
        params![
            contact.id,
            account_id,
            contact.remote_id,
            contact.display_name,
            contact.email,
            contact.vcard_data,
        ],
    )?;
    Ok(())
}

pub fn list_contacts(
    conn: &Connection,
    account_id: &str,
) -> StorageResult<Vec<vespetrel_core::Contact>> {
    let mut stmt = conn.prepare("SELECT id, remote_id, display_name, email, vcard_data FROM contacts WHERE account_id = ?1 ORDER BY display_name ASC")?;
    let rows = stmt.query_map(params![account_id], |row| {
        Ok(vespetrel_core::Contact {
            id: row.get(0)?,
            remote_id: row.get(1)?,
            display_name: row.get(2)?,
            email: row.get(3)?,
            vcard_data: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn upsert_calendar_event(
    conn: &Connection,
    event: &vespetrel_core::CalendarEvent,
) -> StorageResult<()> {
    conn.execute(
        r#"INSERT INTO calendar_events (id, calendar_id, ical_uid, title, description, start_at, end_at, location, raw_ical)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
           ON CONFLICT(id) DO UPDATE SET
             title=excluded.title, description=excluded.description, start_at=excluded.start_at,
             end_at=excluded.end_at, location=excluded.location, raw_ical=excluded.raw_ical"#,
        params![
            event.id,
            event.calendar_id,
            event.ical_uid,
            event.title,
            event.description,
            event.start.timestamp(),
            event.end.timestamp(),
            event.location,
            event.raw_ical.as_deref().unwrap_or(""),
        ],
    )?;
    Ok(())
}

pub fn list_calendar_events(
    conn: &Connection,
    calendar_id: &str,
) -> StorageResult<Vec<vespetrel_core::CalendarEvent>> {
    let mut stmt = conn.prepare("SELECT id, calendar_id, ical_uid, title, description, start_at, end_at, location, raw_ical FROM calendar_events WHERE calendar_id = ?1 ORDER BY start_at ASC")?;
    let rows = stmt.query_map(params![calendar_id], |row| {
        let start_ts: i64 = row.get(5)?;
        let start = DateTime::from_timestamp(start_ts, 0).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Integer,
                Box::new(crate::StorageError::CorruptData(format!(
                    "Invalid start_at timestamp: {start_ts}"
                ))),
            )
        })?;
        let end_ts: i64 = row.get(6)?;
        let end = DateTime::from_timestamp(end_ts, 0).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Integer,
                Box::new(crate::StorageError::CorruptData(format!(
                    "Invalid end_at timestamp: {end_ts}"
                ))),
            )
        })?;
        Ok(vespetrel_core::CalendarEvent {
            id: row.get(0)?,
            calendar_id: row.get(1)?,
            ical_uid: row.get(2)?,
            title: row.get(3)?,
            description: row.get(4)?,
            start,
            end,
            location: row.get(7)?,
            raw_ical: row.get(8)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn upsert_task(conn: &Connection, task: &vespetrel_core::TaskItem) -> StorageResult<()> {
    conn.execute(
        r#"INSERT INTO tasks (id, calendar_id, ical_uid, title, description, due_at, is_completed, completed_at, priority)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
           ON CONFLICT(id) DO UPDATE SET
             title=excluded.title, description=excluded.description, due_at=excluded.due_at,
             is_completed=excluded.is_completed, completed_at=excluded.completed_at, priority=excluded.priority"#,
        params![
            task.id,
            task.calendar_id,
            task.ical_uid,
            task.title,
            task.description,
            task.due_at.map(|d| d.timestamp()),
            if task.is_completed { 1 } else { 0 },
            task.completed_at.map(|d| d.timestamp()),
            task.priority,
        ],
    )?;
    Ok(())
}

pub fn list_tasks(
    conn: &Connection,
    calendar_id: &str,
) -> StorageResult<Vec<vespetrel_core::TaskItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, calendar_id, ical_uid, title, description, due_at, is_completed, completed_at, priority FROM tasks WHERE calendar_id = ?1 ORDER BY is_completed ASC, due_at ASC",
    )?;
    let rows = stmt.query_map(params![calendar_id], |row| {
        let is_completed_int: i64 = row.get(6)?;
        Ok(vespetrel_core::TaskItem {
            id: row.get(0)?,
            calendar_id: row.get(1)?,
            ical_uid: row.get(2)?,
            title: row.get(3)?,
            description: row.get(4)?,
            due_at: row
                .get::<_, Option<i64>>(5)?
                .and_then(|ts| DateTime::from_timestamp(ts, 0)),
            is_completed: is_completed_int != 0,
            completed_at: row
                .get::<_, Option<i64>>(7)?
                .and_then(|ts| DateTime::from_timestamp(ts, 0)),
            priority: (row.get::<_, i64>(8)?).clamp(0, 255) as u8,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn toggle_task_completion(
    conn: &Connection,
    id: &str,
    is_completed: bool,
) -> StorageResult<()> {
    let now = if is_completed {
        Some(Utc::now().timestamp())
    } else {
        None
    };
    conn.execute(
        "UPDATE tasks SET is_completed = ?1, completed_at = ?2 WHERE id = ?3",
        params![if is_completed { 1 } else { 0 }, now, id],
    )?;
    Ok(())
}

pub fn delete_task(conn: &Connection, id: &str) -> StorageResult<()> {
    conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn upsert_signature(conn: &Connection, sig: &vespetrel_core::Signature) -> StorageResult<()> {
    conn.execute_batch("BEGIN IMMEDIATE;")?;
    let res = (|| -> Result<(), rusqlite::Error> {
        if sig.is_default {
            conn.execute(
                "UPDATE signatures SET is_default = 0 WHERE account_id = ?1 OR account_id = '*'",
                params![sig.account_id],
            )?;
        }

        conn.execute(
            r#"INSERT INTO signatures (id, account_id, name, raw_html, plain_text, is_default, include_in_replies, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
               ON CONFLICT(id) DO UPDATE SET
                 account_id=excluded.account_id, name=excluded.name, raw_html=excluded.raw_html,
                 plain_text=excluded.plain_text, is_default=excluded.is_default,
                 include_in_replies=excluded.include_in_replies, updated_at=excluded.updated_at"#,
            params![
                sig.id,
                sig.account_id,
                sig.name,
                sig.raw_html,
                sig.plain_text,
                if sig.is_default { 1 } else { 0 },
                if sig.include_in_replies { 1 } else { 0 },
                sig.created_at.timestamp(),
                sig.updated_at.timestamp(),
            ],
        )?;
        Ok(())
    })();
    match res {
        Ok(()) => {
            conn.execute_batch("COMMIT;")?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK;");
            Err(e.into())
        }
    }
}

pub fn list_signatures_for_account(
    conn: &Connection,
    account_id: &str,
) -> StorageResult<Vec<vespetrel_core::Signature>> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, name, raw_html, plain_text, is_default, include_in_replies, created_at, updated_at
         FROM signatures
         WHERE account_id = ?1 OR account_id = '*'
         ORDER BY is_default DESC, name ASC",
    )?;

    let rows = stmt.query_map(params![account_id], |row| {
        let is_def: i64 = row.get(5)?;
        let inc_rep: i64 = row.get(6)?;
        let created_ts: i64 = row.get(7)?;
        let updated_ts: i64 = row.get(8)?;

        let created_at = DateTime::from_timestamp(created_ts, 0).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                7,
                rusqlite::types::Type::Integer,
                Box::new(crate::StorageError::CorruptData(format!(
                    "Invalid created_at timestamp: {created_ts}"
                ))),
            )
        })?;
        let updated_at = DateTime::from_timestamp(updated_ts, 0).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                8,
                rusqlite::types::Type::Integer,
                Box::new(crate::StorageError::CorruptData(format!(
                    "Invalid updated_at timestamp: {updated_ts}"
                ))),
            )
        })?;
        Ok(vespetrel_core::Signature {
            id: row.get(0)?,
            account_id: row.get(1)?,
            name: row.get(2)?,
            raw_html: row.get(3)?,
            plain_text: row.get(4)?,
            is_default: is_def != 0,
            include_in_replies: inc_rep != 0,
            created_at,
            updated_at,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_default_signature(
    conn: &Connection,
    account_id: &str,
) -> StorageResult<Option<vespetrel_core::Signature>> {
    let list = list_signatures_for_account(conn, account_id)?;
    Ok(list.into_iter().find(|s| s.is_default))
}

pub fn delete_signature(conn: &Connection, id: &str) -> StorageResult<()> {
    conn.execute("DELETE FROM signatures WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn save_user_settings(
    conn: &Connection,
    settings: &vespetrel_core::UserSettings,
) -> StorageResult<()> {
    let json = serde_json::to_string(settings)?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        r#"INSERT INTO user_settings (key, value, updated_at)
           VALUES ('global', ?1, ?2)
           ON CONFLICT(key) DO UPDATE SET
             value=excluded.value, updated_at=excluded.updated_at"#,
        params![json, now],
    )?;
    Ok(())
}

pub fn get_user_settings(conn: &Connection) -> StorageResult<vespetrel_core::UserSettings> {
    let mut stmt = conn.prepare("SELECT value FROM user_settings WHERE key = 'global'")?;
    let opt_json: Option<String> = stmt.query_row([], |row| row.get(0)).optional()?;
    match opt_json {
        Some(json) => match serde_json::from_str(&json) {
            Ok(settings) => Ok(settings),
            Err(e) => Err(crate::StorageError::CorruptData(format!(
                "corrupted user_settings JSON in DB: {e}"
            ))),
        },
        None => Ok(vespetrel_core::UserSettings::default()),
    }
}

// ---------------------------------------------------------------------------
// Outbox Persistence
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OutboxEntry {
    pub id: String,
    pub account_id: String,
    pub composed_message: vespetrel_core::ComposedMessage,
    pub scheduled_at: i64,
    pub send_at: i64,
    pub is_cancelled: bool,
    pub attempts: i64,
    pub last_error: Option<String>,
}

pub fn enqueue_outbox(conn: &Connection, entry: &OutboxEntry) -> StorageResult<()> {
    let msg_json = serde_json::to_string(&entry.composed_message)?;
    conn.execute(
        r#"INSERT INTO outbox (id, account_id, composed_message, scheduled_at, send_at, is_cancelled, attempts, last_error)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
           ON CONFLICT(id) DO UPDATE SET
             composed_message=excluded.composed_message,
             scheduled_at=excluded.scheduled_at,
             send_at=excluded.send_at,
             is_cancelled=excluded.is_cancelled,
             attempts=excluded.attempts,
             last_error=excluded.last_error"#,
        params![
            entry.id,
            entry.account_id,
            msg_json,
            entry.scheduled_at,
            entry.send_at,
            if entry.is_cancelled { 1 } else { 0 },
            entry.attempts,
            entry.last_error,
        ],
    )?;
    Ok(())
}

pub fn cancel_outbox(conn: &Connection, id: &str) -> StorageResult<bool> {
    let rows = conn.execute(
        "UPDATE outbox SET is_cancelled = 1 WHERE id = ?1 AND is_cancelled = 0",
        params![id],
    )?;
    Ok(rows > 0)
}

pub fn list_due_outbox(conn: &Connection, now: i64) -> StorageResult<Vec<OutboxEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, composed_message, scheduled_at, send_at, is_cancelled, attempts, last_error
         FROM outbox
         WHERE is_cancelled = 0 AND send_at <= ?1
         ORDER BY send_at ASC",
    )?;
    let rows = stmt.query_map(params![now], |row| {
        let id: String = row.get(0)?;
        let account_id: String = row.get(1)?;
        let msg_json: String = row.get(2)?;
        let scheduled_at: i64 = row.get(3)?;
        let send_at: i64 = row.get(4)?;
        let is_cancelled: bool = row.get::<_, i64>(5)? != 0;
        let attempts: i64 = row.get(6)?;
        let last_error: Option<String> = row.get(7)?;

        let composed_message = match serde_json::from_str(&msg_json) {
            Ok(m) => m,
            Err(e) => {
                return Err(rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                ));
            }
        };

        Ok(OutboxEntry {
            id,
            account_id,
            composed_message,
            scheduled_at,
            send_at,
            is_cancelled,
            attempts,
            last_error,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn delete_outbox_entry(conn: &Connection, id: &str) -> StorageResult<()> {
    conn.execute("DELETE FROM outbox WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn mark_outbox_failed(conn: &Connection, id: &str, error: &str) -> StorageResult<()> {
    conn.execute(
        "UPDATE outbox SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
        params![id, error],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::db::open_in_memory;
    use vespetrel_core::{Account, Contact, Folder, Message, TaskItem};

    #[test]
    fn test_repo_crud() {
        let conn = open_in_memory().unwrap();

        // 1. Account
        let acct = Account::new(
            "Alice",
            "alice@example.com",
            vespetrel_core::ProviderType::Imap,
        )
        .with_color("#3b82f6");
        upsert_account(&conn, &acct).unwrap();
        let accts = list_accounts(&conn).unwrap();
        assert_eq!(accts.len(), 1);
        assert_eq!(accts[0].email, "alice@example.com");
        assert_eq!(accts[0].color.as_deref(), Some("#3b82f6"));

        // 2. Folder
        let folder = Folder::new(&acct.id, "INBOX", "Inbox", "INBOX").with_color("#ef4444");
        upsert_folder(&conn, &folder).unwrap();
        let folders = list_folders(&conn, &acct.id).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Inbox");
        assert_eq!(folders[0].color.as_deref(), Some("#ef4444"));

        // 3. Message
        let msg = Message::new(
            &acct.id,
            &folder.id,
            1,
            "Hello",
            "bob@example.com",
            vec!["alice@example.com".into()],
        );
        insert_message(&conn, &msg).unwrap();
        let msgs = list_messages_in_folder(&conn, &folder.id, 10, 0).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].subject.as_deref(), Some("Hello"));

        // 4. Update flag
        update_message_flags(&conn, &msg.id, Some(true), Some(true)).unwrap();
        let updated = list_messages_in_folder(&conn, &folder.id, 10, 0).unwrap();
        assert!(updated[0].is_read);
        assert!(updated[0].is_flagged);

        // 5. Contact
        let contact = Contact {
            id: "c1".into(),
            remote_id: Some("rem_c1".into()),
            display_name: Some("Bob".into()),
            email: "bob@example.com".into(),
            vcard_data: None,
        };
        upsert_contact(&conn, &acct.id, &contact).unwrap();
        let contacts = list_contacts(&conn, &acct.id).unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].email, "bob@example.com");
        assert_eq!(contacts[0].remote_id.as_deref(), Some("rem_c1"));

        // 6. Delete message
        delete_message(&conn, &msg.id).unwrap();
        let empty_msgs = list_messages_in_folder(&conn, &folder.id, 10, 0).unwrap();
        assert_eq!(empty_msgs.len(), 0);

        // 7. Calendar and Tasks
        conn.execute(
            "INSERT INTO calendars (id, account_id, remote_id, name) VALUES ('cal1', ?1, 'r1', 'Personal')",
            params![acct.id],
        ).unwrap();

        let task = TaskItem::new("cal1", "Implement Vespetrel Tasks");
        upsert_task(&conn, &task).unwrap();
        let tasks = list_tasks(&conn, "cal1").unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Implement Vespetrel Tasks");
        assert!(!tasks[0].is_completed);

        toggle_task_completion(&conn, &task.id, true).unwrap();
        let completed = list_tasks(&conn, "cal1").unwrap();
        assert!(completed[0].is_completed);

        delete_task(&conn, &task.id).unwrap();
        assert_eq!(list_tasks(&conn, "cal1").unwrap().len(), 0);

        // 8. Signatures
        let sig = vespetrel_core::Signature::new(
            &acct.id,
            "Work Formal",
            "<div class=\"vespetrel-signature\">Alice &bull; Staff Engineer</div>",
            Some("-- \nAlice".into()),
            true,
            true,
        );
        upsert_signature(&conn, &sig).unwrap();

        let sigs = list_signatures_for_account(&conn, &acct.id).unwrap();
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "Work Formal");
        assert!(sigs[0].is_default);

        let def = get_default_signature(&conn, &acct.id).unwrap();
        assert!(def.is_some());
        assert_eq!(def.unwrap().name, "Work Formal");

        delete_signature(&conn, &sig.id).unwrap();
        assert_eq!(
            list_signatures_for_account(&conn, &acct.id).unwrap().len(),
            0
        );

        // 9. User Settings
        let default_settings = get_user_settings(&conn).unwrap();
        assert_eq!(
            default_settings.layout,
            vespetrel_core::PaneLayout::ThreePaneVertical
        );
        assert_eq!(default_settings.accent_color, "#3b82f6");

        let mut custom_settings = default_settings.clone();
        custom_settings.theme = vespetrel_core::ColorTheme::CatppuccinMocha;
        custom_settings.layout = vespetrel_core::PaneLayout::ClassicHorizontal;
        custom_settings.accent_color = "#cba6f7".into();
        custom_settings.undo_send_seconds = 15;
        save_user_settings(&conn, &custom_settings).unwrap();

        let loaded = get_user_settings(&conn).unwrap();
        assert_eq!(loaded.theme, vespetrel_core::ColorTheme::CatppuccinMocha);
        assert_eq!(loaded.layout, vespetrel_core::PaneLayout::ClassicHorizontal);
        assert_eq!(loaded.accent_color, "#cba6f7");
        assert_eq!(loaded.undo_send_seconds, 15);

        // 10. Outbox Persistence
        let outbox_msg = vespetrel_core::ComposedMessage {
            from: vespetrel_core::Address {
                name: Some("Alice".into()),
                email: "alice@example.com".into(),
            },
            to: vec![vespetrel_core::Address {
                name: None,
                email: "bob@example.com".into(),
            }],
            cc: vec![],
            bcc: vec![],
            subject: "Persistent Outbox Test".into(),
            body_text: "Checking durability".into(),
            body_html: None,
            in_reply_to: None,
            references: vec![],
            attachments: vec![],
        };
        let entry = OutboxEntry {
            id: "out-1".into(),
            account_id: acct.id.clone(),
            composed_message: outbox_msg,
            scheduled_at: 1000,
            send_at: 1010,
            is_cancelled: false,
            attempts: 0,
            last_error: None,
        };
        enqueue_outbox(&conn, &entry).unwrap();

        // Check list_due_outbox before send_at
        let due_early = list_due_outbox(&conn, 1005).unwrap();
        assert_eq!(due_early.len(), 0);

        // Check list_due_outbox at or after send_at
        let due = list_due_outbox(&conn, 1015).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, "out-1");
        assert_eq!(due[0].composed_message.subject, "Persistent Outbox Test");

        // Cancel outbox
        let cancelled = cancel_outbox(&conn, "out-1").unwrap();
        assert!(cancelled);
        let due_after_cancel = list_due_outbox(&conn, 1015).unwrap();
        assert_eq!(due_after_cancel.len(), 0);

        // Delete outbox entry
        delete_outbox_entry(&conn, "out-1").unwrap();
    }
}
