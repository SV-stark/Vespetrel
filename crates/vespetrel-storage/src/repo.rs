use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};

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
            role: role_str
                .parse()
                .unwrap_or(vespetrel_core::FolderRole::Custom),
            uid_validity: row.get::<_, Option<i64>>(6)?.map(|v| v as u32),
            highest_mod_seq: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
            total_count: row.get(8)?,
            unread_count: row.get(9)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_folder(
    conn: &Connection,
    account_id: &str,
    remote_id: &str,
) -> anyhow::Result<Option<Folder>> {
    let mut stmt = conn.prepare("SELECT id, account_id, remote_id, name, path, role, uid_validity, highest_mod_seq, total_count, unread_count FROM folders WHERE account_id = ?1 AND remote_id = ?2")?;
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

pub fn list_messages_in_folder(
    conn: &Connection,
    folder_id: &str,
    limit: usize,
    offset: usize,
) -> anyhow::Result<Vec<Message>> {
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
            reply_to: row
                .get::<_, Option<String>>(13)?
                .and_then(|s| serde_json::from_str(&s).ok()),
            sent_at: DateTime::from_timestamp(row.get::<_, i64>(14)?, 0).unwrap_or_else(Utc::now),
            received_at: DateTime::from_timestamp(row.get::<_, i64>(15)?, 0)
                .unwrap_or_else(Utc::now),
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

pub fn update_message_flags(
    conn: &Connection,
    message_id: &str,
    is_read: Option<bool>,
    is_flagged: Option<bool>,
) -> anyhow::Result<()> {
    if let Some(read) = is_read {
        conn.execute(
            "UPDATE messages SET is_read = ?1 WHERE id = ?2",
            params![if read { 1 } else { 0 }, message_id],
        )?;
    }
    if let Some(flagged) = is_flagged {
        conn.execute(
            "UPDATE messages SET is_flagged = ?1 WHERE id = ?2",
            params![if flagged { 1 } else { 0 }, message_id],
        )?;
    }
    Ok(())
}

pub fn delete_message(conn: &Connection, message_id: &str) -> anyhow::Result<()> {
    conn.execute("DELETE FROM messages WHERE id = ?1", params![message_id])?;
    Ok(())
}

pub fn upsert_thread(conn: &Connection, thread: &vespetrel_core::Thread) -> anyhow::Result<()> {
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
) -> anyhow::Result<Vec<vespetrel_core::Thread>> {
    let mut stmt = conn.prepare("SELECT id, account_id, subject, last_message_at, message_count, unread_count, snippet FROM threads WHERE account_id = ?1 ORDER BY last_message_at DESC")?;
    let rows = stmt.query_map(params![account_id], |row| {
        Ok(vespetrel_core::Thread {
            id: row.get(0)?,
            account_id: row.get(1)?,
            subject: row.get(2)?,
            last_message_at: DateTime::from_timestamp(row.get::<_, i64>(3)?, 0)
                .unwrap_or_else(Utc::now),
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
) -> anyhow::Result<()> {
    conn.execute(
        r#"INSERT INTO contacts (id, account_id, remote_id, display_name, email, vcard_data)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6)
           ON CONFLICT(id) DO UPDATE SET
             display_name=excluded.display_name, email=excluded.email, vcard_data=excluded.vcard_data"#,
        params![
            contact.id,
            account_id,
            contact.id,
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
) -> anyhow::Result<Vec<vespetrel_core::Contact>> {
    let mut stmt = conn.prepare("SELECT id, display_name, email, vcard_data FROM contacts WHERE account_id = ?1 ORDER BY display_name ASC")?;
    let rows = stmt.query_map(params![account_id], |row| {
        Ok(vespetrel_core::Contact {
            id: row.get(0)?,
            display_name: row.get(1)?,
            email: row.get(2)?,
            vcard_data: row.get(3)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn upsert_calendar_event(
    conn: &Connection,
    event: &vespetrel_core::CalendarEvent,
) -> anyhow::Result<()> {
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
            "",
        ],
    )?;
    Ok(())
}

pub fn list_calendar_events(
    conn: &Connection,
    calendar_id: &str,
) -> anyhow::Result<Vec<vespetrel_core::CalendarEvent>> {
    let mut stmt = conn.prepare("SELECT id, calendar_id, ical_uid, title, description, start_at, end_at, location FROM calendar_events WHERE calendar_id = ?1 ORDER BY start_at ASC")?;
    let rows = stmt.query_map(params![calendar_id], |row| {
        Ok(vespetrel_core::CalendarEvent {
            id: row.get(0)?,
            calendar_id: row.get(1)?,
            ical_uid: row.get(2)?,
            title: row.get(3)?,
            description: row.get(4)?,
            start: DateTime::from_timestamp(row.get::<_, i64>(5)?, 0).unwrap_or_else(Utc::now),
            end: DateTime::from_timestamp(row.get::<_, i64>(6)?, 0).unwrap_or_else(Utc::now),
            location: row.get(7)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn upsert_task(conn: &Connection, task: &vespetrel_core::TaskItem) -> anyhow::Result<()> {
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
) -> anyhow::Result<Vec<vespetrel_core::TaskItem>> {
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
            priority: row.get::<_, u8>(8)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn toggle_task_completion(
    conn: &Connection,
    id: &str,
    is_completed: bool,
) -> anyhow::Result<()> {
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

pub fn delete_task(conn: &Connection, id: &str) -> anyhow::Result<()> {
    conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
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
        );
        upsert_account(&conn, &acct).unwrap();
        let accts = list_accounts(&conn).unwrap();
        assert_eq!(accts.len(), 1);
        assert_eq!(accts[0].email, "alice@example.com");

        // 2. Folder
        let folder = Folder::new(&acct.id, "INBOX", "Inbox", "INBOX");
        upsert_folder(&conn, &folder).unwrap();
        let folders = list_folders(&conn, &acct.id).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Inbox");

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
            display_name: Some("Bob".into()),
            email: "bob@example.com".into(),
            vcard_data: None,
        };
        upsert_contact(&conn, &acct.id, &contact).unwrap();
        let contacts = list_contacts(&conn, &acct.id).unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].email, "bob@example.com");

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
    }
}
