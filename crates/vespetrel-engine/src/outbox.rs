//! Undo Send & Scheduled Send Outbox Queue §7 Phase 7
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use vespetrel_core::ComposedMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledMessage {
    pub id: String,
    pub message: ComposedMessage,
    pub scheduled_at: DateTime<Utc>,
    pub send_at: DateTime<Utc>,
    pub is_cancelled: bool,
}

pub struct UndoSendBuffer {
    /// Grace period in seconds (5–30s)
    pub grace_period_secs: i64,
    pub pending: HashMap<String, ScheduledMessage>,
}

impl UndoSendBuffer {
    pub fn new(grace_period_secs: i64) -> Self {
        Self {
            grace_period_secs: grace_period_secs.clamp(5, 30),
            pending: HashMap::new(),
        }
    }

    /// Enqueue a message with undo grace period
    pub fn enqueue(&mut self, message: ComposedMessage) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let send_at = now + Duration::seconds(self.grace_period_secs);

        self.pending.insert(
            id.clone(),
            ScheduledMessage {
                id: id.clone(),
                message,
                scheduled_at: now,
                send_at,
                is_cancelled: false,
            },
        );

        id
    }

    /// Cancel a pending send before the grace period expires
    pub fn cancel(&mut self, id: &str) -> Option<ComposedMessage> {
        self.pending.remove(id).map(|s| s.message)
    }

    /// Drain all messages that have passed their grace period and are ready to send
    pub fn drain_ready(&mut self, now: DateTime<Utc>) -> Vec<ComposedMessage> {
        let mut ready_ids = Vec::new();
        for (id, scheduled) in &self.pending {
            if !scheduled.is_cancelled && scheduled.send_at <= now {
                ready_ids.push(id.clone());
            }
        }

        let mut ready_messages = Vec::new();
        for id in ready_ids {
            if let Some(s) = self.pending.remove(&id) {
                ready_messages.push(s.message);
            }
        }
        ready_messages
    }
}

pub struct OutboxQueue {
    pub scheduled: HashMap<String, ScheduledMessage>,
}

impl OutboxQueue {
    pub fn new() -> Self {
        Self {
            scheduled: HashMap::new(),
        }
    }

    /// Schedule a message for future automated transmission
    pub fn schedule_send(&mut self, message: ComposedMessage, send_at: DateTime<Utc>) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        self.scheduled.insert(
            id.clone(),
            ScheduledMessage {
                id: id.clone(),
                message,
                scheduled_at: now,
                send_at,
                is_cancelled: false,
            },
        );

        id
    }

    /// Cancel a scheduled send
    pub fn cancel_scheduled(&mut self, id: &str) -> bool {
        self.scheduled.remove(id).is_some()
    }

    /// Drain all scheduled messages due for delivery
    pub fn drain_due(&mut self, now: DateTime<Utc>) -> Vec<ComposedMessage> {
        let mut due_ids = Vec::new();
        for (id, item) in &self.scheduled {
            if !item.is_cancelled && item.send_at <= now {
                due_ids.push(id.clone());
            }
        }

        let mut msgs = Vec::new();
        for id in due_ids {
            if let Some(item) = self.scheduled.remove(&id) {
                msgs.push(item.message);
            }
        }
        msgs
    }

    /// Enqueue a scheduled message into both memory and persistent SQLite outbox table
    pub fn schedule_send_persistent(
        &mut self,
        conn: &rusqlite::Connection,
        account_id: &str,
        message: ComposedMessage,
        send_at: DateTime<Utc>,
    ) -> Result<String, vespetrel_storage::StorageError> {
        let id = self.schedule_send(message.clone(), send_at);
        let entry = vespetrel_storage::repo::OutboxEntry {
            id: id.clone(),
            account_id: account_id.to_string(),
            composed_message: message,
            scheduled_at: Utc::now().timestamp(),
            send_at: send_at.timestamp(),
            is_cancelled: false,
            attempts: 0,
            last_error: None,
        };
        vespetrel_storage::repo::enqueue_outbox(conn, &entry)?;
        Ok(id)
    }

    /// Cancel a scheduled send in memory and persist cancellation to SQLite outbox
    pub fn cancel_scheduled_persistent(
        &mut self,
        conn: &rusqlite::Connection,
        id: &str,
    ) -> Result<bool, vespetrel_storage::StorageError> {
        self.cancel_scheduled(id);
        vespetrel_storage::repo::cancel_outbox(conn, id)
    }

    /// Restore pending scheduled messages from SQLite outbox table on application startup
    pub fn load_from_db(
        &mut self,
        conn: &rusqlite::Connection,
    ) -> Result<usize, vespetrel_storage::StorageError> {
        let now_ts = Utc::now().timestamp();
        // Load all uncancelled messages due in the future or ready to process
        let entries = vespetrel_storage::repo::list_due_outbox(conn, now_ts + 86400 * 365)?;
        let count = entries.len();
        for entry in entries {
            let send_at = DateTime::from_timestamp(entry.send_at, 0).unwrap_or_else(Utc::now);
            let scheduled_at =
                DateTime::from_timestamp(entry.scheduled_at, 0).unwrap_or_else(Utc::now);
            self.scheduled.insert(
                entry.id.clone(),
                ScheduledMessage {
                    id: entry.id,
                    message: entry.composed_message,
                    scheduled_at,
                    send_at,
                    is_cancelled: entry.is_cancelled,
                },
            );
        }
        Ok(count)
    }
}

impl UndoSendBuffer {
    /// Enqueue into undo buffer and persist to SQLite outbox table
    pub fn enqueue_persistent(
        &mut self,
        conn: &rusqlite::Connection,
        account_id: &str,
        message: ComposedMessage,
    ) -> Result<String, vespetrel_storage::StorageError> {
        let id = self.enqueue(message.clone());
        let item = self.pending.get(&id).unwrap();
        let entry = vespetrel_storage::repo::OutboxEntry {
            id: id.clone(),
            account_id: account_id.to_string(),
            composed_message: message,
            scheduled_at: item.scheduled_at.timestamp(),
            send_at: item.send_at.timestamp(),
            is_cancelled: false,
            attempts: 0,
            last_error: None,
        };
        vespetrel_storage::repo::enqueue_outbox(conn, &entry)?;
        Ok(id)
    }

    /// Cancel undo send in memory and mark cancelled in SQLite outbox table
    pub fn cancel_persistent(
        &mut self,
        conn: &rusqlite::Connection,
        id: &str,
    ) -> Result<Option<ComposedMessage>, vespetrel_storage::StorageError> {
        let recovered = self.cancel(id);
        vespetrel_storage::repo::cancel_outbox(conn, id)?;
        Ok(recovered)
    }
}

/// Drains due messages from SQLite outbox table and dispatches them via MailProvider
pub async fn process_due_outbox(
    conn: &rusqlite::Connection,
    provider: &dyn vespetrel_core::provider::MailProvider,
    now: DateTime<Utc>,
) -> Vec<(String, Result<(), String>)> {
    let mut results = Vec::new();
    let entries = match vespetrel_storage::repo::list_due_outbox(conn, now.timestamp()) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(error=%e, "failed to query due outbox messages");
            return results;
        }
    };

    for entry in entries {
        match provider.send_message(&entry.composed_message).await {
            Ok(_) => {
                tracing::info!(id=%entry.id, to=?entry.composed_message.to, "outbox message delivered successfully");
                let _ = vespetrel_storage::repo::delete_outbox_entry(conn, &entry.id);
                results.push((entry.id, Ok(())));
            }
            Err(e) => {
                let err_str = e.to_string();
                tracing::error!(id=%entry.id, error=%err_str, "outbox delivery failed");
                let _ = vespetrel_storage::repo::mark_outbox_failed(conn, &entry.id, &err_str);
                results.push((entry.id, Err(err_str)));
            }
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use vespetrel_core::Address;

    #[test]
    fn test_undo_send_lifecycle() {
        let mut undo_buffer = UndoSendBuffer::new(10);
        let msg = ComposedMessage {
            from: Address {
                name: None,
                email: "user@example.com".into(),
            },
            to: vec![Address {
                name: None,
                email: "recipient@example.com".into(),
            }],
            cc: vec![],
            bcc: vec![],
            subject: "Test Undo".into(),
            body_text: "Hello".into(),
            body_html: None,
            in_reply_to: None,
            references: vec![],
            attachments: vec![],
        };

        let id = undo_buffer.enqueue(msg.clone());
        assert_eq!(undo_buffer.pending.len(), 1);

        // Cancel within grace period
        let recovered = undo_buffer.cancel(&id);
        assert!(recovered.is_some());
        assert_eq!(recovered.unwrap().subject, "Test Undo");
        assert_eq!(undo_buffer.pending.len(), 0);
    }

    #[test]
    fn test_scheduled_send_drain() {
        let mut outbox = OutboxQueue::new();
        let now = Utc::now();
        let future = now + Duration::hours(2);

        let msg = ComposedMessage {
            from: Address {
                name: None,
                email: "user@example.com".into(),
            },
            to: vec![Address {
                name: None,
                email: "recipient@example.com".into(),
            }],
            cc: vec![],
            bcc: vec![],
            subject: "Scheduled Update".into(),
            body_text: "Future".into(),
            body_html: None,
            in_reply_to: None,
            references: vec![],
            attachments: vec![],
        };

        let id = outbox.schedule_send(msg, future);
        assert_eq!(outbox.scheduled.len(), 1);

        // Before due time
        let ready = outbox.drain_due(now + Duration::hours(1));
        assert!(ready.is_empty());

        // After due time
        let ready = outbox.drain_due(now + Duration::hours(3));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].subject, "Scheduled Update");
        assert!(!outbox.cancel_scheduled(&id));
    }

    #[tokio::test]
    async fn test_persistent_outbox_flow() {
        let conn = vespetrel_storage::open_in_memory().unwrap();
        let acct = vespetrel_core::Account::new(
            "outbox_acct",
            "outbox@test.com",
            vespetrel_core::ProviderType::Imap,
        );
        vespetrel_storage::repo::upsert_account(&conn, &acct).unwrap();

        let mut queue = OutboxQueue::new();
        let msg = ComposedMessage {
            from: Address {
                name: None,
                email: "outbox@test.com".into(),
            },
            to: vec![Address {
                name: None,
                email: "dest@test.com".into(),
            }],
            cc: vec![],
            bcc: vec![],
            subject: "DB Outbox Test".into(),
            body_text: "Body".into(),
            body_html: None,
            in_reply_to: None,
            references: vec![],
            attachments: vec![],
        };

        let now = Utc::now();
        let id = queue
            .schedule_send_persistent(&conn, &acct.id, msg, now)
            .unwrap();
        assert!(!id.is_empty());

        // Restore in a fresh queue
        let mut queue2 = OutboxQueue::new();
        let loaded = queue2.load_from_db(&conn).unwrap();
        assert_eq!(loaded, 1);
        assert!(queue2.scheduled.contains_key(&id));
    }
}

impl Default for OutboxQueue {
    fn default() -> Self {
        Self::new()
    }
}
