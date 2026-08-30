use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub id: String,
    pub account_id: String,
    pub subject: Option<String>,
    pub last_message_at: DateTime<Utc>,
    pub message_count: i64,
    pub unread_count: i64,
    pub snippet: Option<String>,
}

impl Thread {
    pub fn new(account_id: impl Into<String>, subject: Option<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            account_id: account_id.into(),
            subject,
            last_message_at: Utc::now(),
            message_count: 1,
            unread_count: 1,
            snippet: None,
        }
    }
}
