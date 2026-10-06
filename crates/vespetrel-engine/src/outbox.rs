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

    /// Drain all messages that have passed their grace period and are ready to send.
    ///
    /// Ordered by `send_at`. `HashMap` iteration order is unspecified and seed-dependent, so
    /// the previous implementation returned messages in an arbitrary order that differed
    /// between runs — two messages scheduled for 09:00 and 09:01 both due at 09:05 were sent
    /// in random order.
    pub fn drain_ready(&mut self, now: DateTime<Utc>) -> Vec<ComposedMessage> {
        let mut due: Vec<(DateTime<Utc>, String)> = self
            .pending
            .iter()
            .filter(|(_, s)| !s.is_cancelled && s.send_at <= now)
            .map(|(id, s)| (s.send_at, id.clone()))
            .collect();
        due.sort(); // earliest first, ties broken by id for determinism
        due.into_iter()
            .filter_map(|(_, id)| self.pending.remove(&id).map(|s| s.message))
            .collect()
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

    /// Drain all scheduled messages due for delivery, in `send_at` order.
    ///
    /// `HashMap` iteration order is unspecified, so the previous implementation returned
    /// scheduled sends in an arbitrary order that varied between runs.
    pub fn drain_due(&mut self, now: DateTime<Utc>) -> Vec<ComposedMessage> {
        let mut due: Vec<(DateTime<Utc>, String)> = self
            .scheduled
            .iter()
            .filter(|(_, it)| !it.is_cancelled && it.send_at <= now)
            .map(|(id, it)| (it.send_at, id.clone()))
            .collect();
        due.sort();
        due.into_iter()
            .filter_map(|(_, id)| self.scheduled.remove(&id).map(|s| s.message))
            .collect()
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

    /// Restore pending scheduled messages from SQLite outbox table on application startup.
    ///
    /// Every uncancelled row is loaded, regardless of `send_at`. The previous code widened
    /// a `send_at <= now` filter by a year, which only moved the cutoff: a message scheduled
    /// beyond a year was never restored, so `cancel_scheduled` returned false (the user could
    /// not cancel it) and `drain_due` would never return it.
    pub fn load_from_db(
        &mut self,
        conn: &rusqlite::Connection,
    ) -> Result<usize, vespetrel_storage::StorageError> {
        let entries = vespetrel_storage::repo::list_all_uncancelled_outbox(conn)?;
        let count = entries.len();
        for entry in entries {
            let send_at = DateTime::from_timestamp(entry.send_at, 0).ok_or_else(|| {
                vespetrel_storage::StorageError::CorruptData(format!(
                    "outbox entry {} has an out-of-range send_at ({})",
                    entry.id, entry.send_at
                ))
            })?;
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
    /// Enqueue into the undo buffer and persist to the SQLite outbox table.
    ///
    /// The database row is written **first**. The previous order mutated the in-memory map
    /// and then wrote SQLite, so a failed `enqueue_outbox` left an in-memory entry with no
    /// durable row — the message would be sent in this process but silently vanish on
    /// restart. It also `unwrap()`ed a lookup that could only be reached by a bug.
    pub fn enqueue_persistent(
        &mut self,
        conn: &rusqlite::Connection,
        account_id: &str,
        message: ComposedMessage,
    ) -> Result<String, vespetrel_storage::StorageError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let send_at = now + Duration::seconds(self.grace_period_secs);

        let entry = vespetrel_storage::repo::OutboxEntry {
            id: id.clone(),
            account_id: account_id.to_string(),
            composed_message: message.clone(),
            scheduled_at: now.timestamp(),
            send_at: send_at.timestamp(),
            is_cancelled: false,
            attempts: 0,
            last_error: None,
        };
        // Fails fast: nothing has been mutated yet.
        vespetrel_storage::repo::enqueue_outbox(conn, &entry)?;

        self.pending.insert(
            id.clone(),
            ScheduledMessage {
                id,
                message,
                scheduled_at: now,
                send_at,
                is_cancelled: false,
            },
        );
        Ok(entry.id)
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

/// Maximum delivery attempts before a message is given up on.
///
/// Without a ceiling, a permanently undeliverable message (bad recipient, `550 5.1.1`,
/// full recipient quota) incremented `attempts` on every outbox tick forever, hammering the
/// server and burying the log in the same error.
pub const MAX_DELIVERY_ATTEMPTS: i64 = 5;

/// Claim one due outbox row for delivery.
///
/// Returns `Some(entry)` only for the caller that atomically flipped the attempt counter.
/// Without this, two concurrent `process_due_outbox` calls both read the same due rows and
/// both send them, delivering duplicates.
fn claim_due_entry(
    conn: &rusqlite::Connection,
    now_ts: i64,
) -> Result<Option<vespetrel_storage::repo::OutboxEntry>, vespetrel_storage::StorageError> {
    use rusqlite::{OptionalExtension, params};

    // Find the earliest due row, then claim it with a conditional update. The
    // `attempts < MAX` predicate is what stops an exhausted message from being retried.
    let candidate: Option<(String, i64)> = conn
        .query_row(
            "SELECT id, attempts FROM outbox
              WHERE is_cancelled = 0 AND send_at <= ?1
              ORDER BY send_at ASC
              LIMIT 1",
            params![now_ts],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;

    let Some((id, attempts)) = candidate else {
        return Ok(None);
    };

    if attempts >= MAX_DELIVERY_ATTEMPTS {
        // Exhausted. Cancel it so the user sees it stop rather than retrying forever, and
        // record why.
        let msg: String = conn
            .query_row(
                "SELECT COALESCE(last_error, 'delivery failed') FROM outbox WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_else(|| "delivery failed".to_string());
        conn.execute(
            "UPDATE outbox SET is_cancelled = 1, last_error = ?2 WHERE id = ?1",
            params![
                id,
                format!("gave up after {MAX_DELIVERY_ATTEMPTS} attempts: {msg}")
            ],
        )?;
        tracing::error!(id=%id, attempts, "outbox message permanently failed; not retrying");
        return Ok(None);
    }

    // Atomic claim: only the caller whose UPDATE affects a row may send.
    let claimed = conn.execute(
        "UPDATE outbox SET attempts = attempts + 1
          WHERE id = ?1 AND is_cancelled = 0 AND attempts = ?2",
        params![id, attempts],
    )?;
    if claimed != 1 {
        // Another worker won the race.
        return Ok(None);
    }

    let entry = vespetrel_storage::repo::get_outbox_entry(conn, &id)?;
    Ok(entry)
}

/// Drains due messages from the SQLite outbox table and dispatches them via MailProvider.
///
/// Each row is claimed atomically before sending, so concurrent drains cannot deliver the
/// same message twice, and delivery stops after [`MAX_DELIVERY_ATTEMPTS`].
pub async fn process_due_outbox(
    conn: &rusqlite::Connection,
    provider: &dyn vespetrel_core::provider::MailProvider,
    now: DateTime<Utc>,
) -> Vec<(String, Result<(), String>)> {
    let mut results = Vec::new();
    let now_ts = now.timestamp();

    loop {
        let entry = match claim_due_entry(conn, now_ts) {
            Ok(Some(e)) => e,
            Ok(None) => break,
            Err(e) => {
                tracing::error!(error=%e, "failed to claim a due outbox message");
                break;
            }
        };

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

        // Bound the work per pass so a large backlog cannot monopolise the caller.
        if results.len() >= vespetrel_storage::repo::OUTBOX_PAGE_LIMIT as usize {
            break;
        }
    }
    results
}

impl Default for OutboxQueue {
    fn default() -> Self {
        Self::new()
    }
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

    /// Regression: `HashMap` iteration order made scheduled sends go out in a random order
    /// that differed between runs.
    #[test]
    fn test_drain_is_ordered_by_send_at() {
        let mk = |subject: &str| ComposedMessage {
            from: Address {
                name: None,
                email: "u@example.com".into(),
            },
            to: vec![Address {
                name: None,
                email: "d@example.com".into(),
            }],
            cc: vec![],
            bcc: vec![],
            subject: subject.into(),
            body_text: "x".into(),
            body_html: None,
            in_reply_to: None,
            references: vec![],
            attachments: vec![],
        };

        let now = Utc::now();
        let mut outbox = OutboxQueue::new();
        // Insert out of chronological order on purpose.
        outbox.schedule_send(mk("third"), now + Duration::hours(3));
        outbox.schedule_send(mk("first"), now + Duration::hours(1));
        outbox.schedule_send(mk("second"), now + Duration::hours(2));

        let ready = outbox.drain_due(now + Duration::hours(5));
        let subjects: Vec<&str> = ready.iter().map(|m| m.subject.as_str()).collect();
        assert_eq!(subjects, vec!["first", "second", "third"]);

        // The same must hold for the undo buffer.
        let mut undo = UndoSendBuffer::new(10);
        undo.enqueue(mk("c"));
        undo.enqueue(mk("a"));
        undo.enqueue(mk("b"));
        let drained: Vec<String> = undo
            .drain_ready(now + Duration::hours(1))
            .into_iter()
            .map(|m| m.subject)
            .collect();
        // Grace periods are identical, so only require a deterministic, complete result.
        assert_eq!(drained.len(), 3);
    }

    /// Regression: `load_from_db` used a `send_at <= now + 1 year` window, so a message
    /// scheduled further out was never restored and could not be cancelled.
    #[test]
    fn test_load_from_db_restores_far_future_schedules() {
        let conn = vespetrel_storage::open_in_memory().unwrap();
        let acct = vespetrel_core::Account::new(
            "far_acct",
            "far@test.com",
            vespetrel_core::ProviderType::Imap,
        );
        vespetrel_storage::repo::upsert_account(&conn, &acct).unwrap();

        let mk = |subject: &str| ComposedMessage {
            from: Address {
                name: None,
                email: "u@example.com".into(),
            },
            to: vec![Address {
                name: None,
                email: "d@example.com".into(),
            }],
            cc: vec![],
            bcc: vec![],
            subject: subject.into(),
            body_text: "x".into(),
            body_html: None,
            in_reply_to: None,
            references: vec![],
            attachments: vec![],
        };

        let far = Utc::now() + chrono::Duration::days(900);
        let mut q = OutboxQueue::new();
        let id = q
            .schedule_send_persistent(&conn, &acct.id, mk("far"), far)
            .unwrap();

        let mut restored = OutboxQueue::new();
        assert_eq!(restored.load_from_db(&conn).unwrap(), 1);
        assert!(restored.scheduled.contains_key(&id));
        // And it is cancellable, which was impossible before.
        assert!(restored.cancel_scheduled(&id));
    }

    /// Regression: `enqueue_persistent` mutated memory before the DB write, so a failed write
    /// left an in-memory entry that vanished on restart.
    #[test]
    fn test_enqueue_persistent_does_not_mutate_on_db_failure() {
        let conn = vespetrel_storage::open_in_memory().unwrap();
        let mut undo = UndoSendBuffer::new(10);
        let msg = ComposedMessage {
            from: Address {
                name: None,
                email: "u@example.com".into(),
            },
            to: vec![Address {
                name: None,
                email: "d@example.com".into(),
            }],
            cc: vec![],
            bcc: vec![],
            subject: "s".into(),
            body_text: "b".into(),
            body_html: None,
            in_reply_to: None,
            references: vec![],
            attachments: vec![],
        };
        // No account row exists, so the outbox FK fails.
        assert!(
            undo.enqueue_persistent(&conn, "no-such-account", msg)
                .is_err()
        );
        assert!(
            undo.pending.is_empty(),
            "an in-memory entry survived a failed DB write"
        );
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
