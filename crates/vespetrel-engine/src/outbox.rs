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
}
