//! Geary-style lightweight asynchronous Outbox & Offline Queue for resilient message transmission.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, broadcast};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::error::SmtpError;
use crate::message::SmtpMessage;

/// Status of an outbox message through its delivery lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryStatus {
    /// Queued and waiting for background worker to dispatch.
    Queued,
    /// Currently actively transmitting to remote SMTP server.
    Sending {
        started_at: DateTime<Utc>,
        attempt: u32,
    },
    /// Acknowledged and accepted by remote SMTP server.
    Sent {
        delivered_at: DateTime<Utc>,
        response: String,
    },
    /// Temporary failure (e.g. 4xx status, offline network); will retry automatically.
    FailedTransient {
        attempt: u32,
        next_retry: DateTime<Utc>,
        reason: String,
    },
    /// Fatal permanent failure (e.g. 5xx status, invalid recipient); will not retry.
    FailedPermanent {
        failed_at: DateTime<Utc>,
        code: Option<u16>,
        reason: String,
    },
    /// Cancelled by user before completion.
    Cancelled,
}

impl DeliveryStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Sent { .. } | Self::FailedPermanent { .. } | Self::Cancelled
        )
    }

    pub fn is_ready_to_send(&self, now: DateTime<Utc>) -> bool {
        match self {
            Self::Queued => true,
            Self::FailedTransient { next_retry, .. } => *next_retry <= now,
            _ => false,
        }
    }
}

/// Exponential backoff retry configuration for transient network or 4xx failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_multiplier: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_secs(5),
            max_delay: Duration::from_secs(1800), // 30 minutes
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryPolicy {
    pub fn next_delay(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }
        let factor = self.backoff_multiplier.powi((attempt - 1) as i32);
        let secs = (self.initial_delay.as_secs_f64() * factor).min(self.max_delay.as_secs_f64());
        Duration::from_secs_f64(secs)
    }
}

/// An entry in the asynchronous outbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxItem {
    pub id: String,
    pub message: SmtpMessage,
    pub status: DeliveryStatus,
    pub created_at: DateTime<Utc>,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub attempts: u32,
    pub error_history: Vec<String>,
}

impl OutboxItem {
    pub fn new(message: SmtpMessage) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            message,
            status: DeliveryStatus::Queued,
            created_at: Utc::now(),
            last_attempt_at: None,
            attempts: 0,
            error_history: Vec::new(),
        }
    }
}

/// Outbox events for GUI / UI progress bars and toasts.
#[derive(Debug, Clone)]
pub enum OutboxEvent {
    Enqueued {
        id: String,
        subject: String,
    },
    Sending {
        id: String,
        attempt: u32,
    },
    Sent {
        id: String,
        response: String,
    },
    TransientFailure {
        id: String,
        next_retry: DateTime<Utc>,
        reason: String,
    },
    PermanentFailure {
        id: String,
        reason: String,
    },
    Cancelled {
        id: String,
    },
}

/// Thread-safe in-memory or persisted Outbox Queue.
#[derive(Clone)]
pub struct OutboxQueue {
    items: Arc<RwLock<HashMap<String, OutboxItem>>>,
    event_tx: broadcast::Sender<OutboxEvent>,
    retry_policy: RetryPolicy,
}

impl OutboxQueue {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(128);
        Self {
            items: Arc::new(RwLock::new(HashMap::new())),
            event_tx: tx,
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Enqueue a message into the outbox. Returns the outbox task UUID.
    pub async fn enqueue(&self, message: SmtpMessage) -> String {
        let item = OutboxItem::new(message);
        let id = item.id.clone();
        let subject = item.message.subject.clone();

        {
            let mut items = self.items.write().await;
            items.insert(id.clone(), item);
        }

        let _ = self.event_tx.send(OutboxEvent::Enqueued {
            id: id.clone(),
            subject,
        });

        info!(id=%id, "message enqueued into asynchronous outbox");
        id
    }

    /// Retrieve an outbox item by ID.
    pub async fn get(&self, id: &str) -> Option<OutboxItem> {
        let items = self.items.read().await;
        items.get(id).cloned()
    }

    /// Cancel a pending or queued message.
    pub async fn cancel(&self, id: &str) -> bool {
        let mut items = self.items.write().await;
        if let Some(item) = items.get_mut(id).filter(|item| !item.status.is_terminal()) {
            item.status = DeliveryStatus::Cancelled;
            let _ = self
                .event_tx
                .send(OutboxEvent::Cancelled { id: id.to_string() });
            return true;
        }
        false
    }

    /// Force an immediate retry for a failed or delayed message.
    pub async fn retry_now(&self, id: &str) -> bool {
        let mut items = self.items.write().await;
        if let Some(item) = items
            .get_mut(id)
            .filter(|item| matches!(item.status, DeliveryStatus::FailedTransient { .. }))
        {
            item.status = DeliveryStatus::Queued;
            return true;
        }
        false
    }

    /// List all items currently in the outbox.
    pub async fn list(&self) -> Vec<OutboxItem> {
        let items = self.items.read().await;
        items.values().cloned().collect()
    }

    /// Find the next message ready for transmission.
    pub async fn next_ready(&self) -> Option<OutboxItem> {
        let now = Utc::now();
        let items = self.items.read().await;
        for item in items.values() {
            if item.status.is_ready_to_send(now) {
                return Some(item.clone());
            }
        }
        None
    }

    /// Mark an item as Sending.
    pub async fn mark_sending(&self, id: &str) -> Option<u32> {
        let mut items = self.items.write().await;
        if let Some(item) = items.get_mut(id) {
            item.attempts += 1;
            item.last_attempt_at = Some(Utc::now());
            item.status = DeliveryStatus::Sending {
                started_at: Utc::now(),
                attempt: item.attempts,
            };
            let attempt = item.attempts;
            let _ = self.event_tx.send(OutboxEvent::Sending {
                id: id.to_string(),
                attempt,
            });
            return Some(attempt);
        }
        None
    }

    /// Mark an item as Sent successfully.
    pub async fn mark_sent(&self, id: &str, response: String) {
        let mut items = self.items.write().await;
        if let Some(item) = items.get_mut(id) {
            item.status = DeliveryStatus::Sent {
                delivered_at: Utc::now(),
                response: response.clone(),
            };
            let _ = self.event_tx.send(OutboxEvent::Sent {
                id: id.to_string(),
                response,
            });
            info!(id=%id, "outbox message marked as successfully delivered");
        }
    }

    /// Record a delivery failure, classifying as transient or permanent according to SmtpError and retry policy.
    pub async fn mark_failed(&self, id: &str, err: &SmtpError) {
        let mut items = self.items.write().await;
        let Some(item) = items.get_mut(id) else {
            return;
        };

        let reason = err.to_string();
        item.error_history.push(reason.clone());

        let is_transient = err.is_transient() && item.attempts < self.retry_policy.max_attempts;

        if is_transient {
            let delay = self.retry_policy.next_delay(item.attempts);
            let next_retry = Utc::now()
                + chrono::Duration::from_std(delay).unwrap_or(chrono::Duration::seconds(60));
            item.status = DeliveryStatus::FailedTransient {
                attempt: item.attempts,
                next_retry,
                reason: reason.clone(),
            };
            let _ = self.event_tx.send(OutboxEvent::TransientFailure {
                id: id.to_string(),
                next_retry,
                reason,
            });
            warn!(id=%id, next_retry=%next_retry, "transient SMTP failure, scheduled retry");
        } else {
            item.status = DeliveryStatus::FailedPermanent {
                failed_at: Utc::now(),
                code: err.reply_code(),
                reason: reason.clone(),
            };
            let _ = self.event_tx.send(OutboxEvent::PermanentFailure {
                id: id.to_string(),
                reason,
            });
            error!(id=%id, "permanent SMTP failure; stopped retry attempts");
        }
    }

    /// Subscribe to real-time outbox lifecycle events.
    pub fn subscribe(&self) -> broadcast::Receiver<OutboxEvent> {
        self.event_tx.subscribe()
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
    use vespetrel_core::message::Address;

    #[tokio::test]
    async fn test_outbox_queue_lifecycle() {
        let queue = OutboxQueue::new();
        let msg = SmtpMessage::new(
            Address {
                name: Some("Alice".into()),
                email: "alice@example.com".into(),
            },
            "Outbox Test",
        )
        .to(Address {
            name: Some("Bob".into()),
            email: "bob@example.com".into(),
        });

        let id = queue.enqueue(msg).await;
        let item = queue.get(&id).await.unwrap();
        assert_eq!(item.status, DeliveryStatus::Queued);

        // Next ready should return it
        let ready = queue.next_ready().await.unwrap();
        assert_eq!(ready.id, id);

        // Mark sending
        let attempt = queue.mark_sending(&id).await.unwrap();
        assert_eq!(attempt, 1);

        // Mark transient failure
        let transient_err = SmtpError::Protocol {
            code: 451,
            enhanced: None,
            message: "Temporary server error".into(),
        };
        queue.mark_failed(&id, &transient_err).await;

        let failed_item = queue.get(&id).await.unwrap();
        assert!(matches!(
            failed_item.status,
            DeliveryStatus::FailedTransient { .. }
        ));

        // Mark sent
        queue.mark_sent(&id, "250 2.0.0 OK".into()).await;
        let sent_item = queue.get(&id).await.unwrap();
        assert!(matches!(sent_item.status, DeliveryStatus::Sent { .. }));
        assert!(sent_item.status.is_terminal());
    }
}
