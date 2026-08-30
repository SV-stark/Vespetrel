use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    pub name: Option<String>,
    pub email: String,
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(name) = &self.name {
            write!(f, "{name} <{}>", self.email)
        } else {
            write!(f, "{}", self.email)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flag {
    Seen,
    Flagged,
    Answered,
    Deleted,
    Draft,
}

impl Flag {
    pub fn as_imap_str(&self) -> &'static str {
        match self {
            Self::Seen => "\\Seen",
            Self::Flagged => "\\Flagged",
            Self::Answered => "\\Answered",
            Self::Deleted => "\\Deleted",
            Self::Draft => "\\Draft",
        }
    }
}

/// Full message envelope stored in `messages` table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub account_id: String,
    pub folder_id: String,
    pub thread_id: Option<String>,
    pub remote_uid: u32,
    pub message_id_header: Option<String>,
    pub in_reply_to: Option<String>,
    pub subject: Option<String>,
    pub from_address: String,
    pub from_name: Option<String>,
    pub to_addresses: Vec<Address>,
    pub cc_addresses: Vec<Address>,
    pub bcc_addresses: Vec<Address>,
    pub reply_to: Option<Vec<Address>>,
    pub sent_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub is_read: bool,
    pub is_flagged: bool,
    pub is_draft: bool,
    pub has_attachments: bool,
    pub body_snippet: Option<String>,
    pub body_text_preview: Option<String>,
    pub blob_path: String,
    pub size_bytes: i64,
}

/// Lightweight projection for virtual list rendering (sub-1ms frame)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSummary {
    pub id: String,
    pub thread_id: Option<String>,
    pub subject: Option<String>,
    pub from_address: String,
    pub from_name: Option<String>,
    pub snippet: Option<String>,
    pub sent_at: DateTime<Utc>,
    pub is_read: bool,
    pub is_flagged: bool,
    pub has_attachments: bool,
}

impl From<Message> for MessageSummary {
    fn from(m: Message) -> Self {
        Self {
            id: m.id,
            thread_id: m.thread_id,
            subject: m.subject,
            from_address: m.from_address,
            from_name: m.from_name,
            snippet: m.body_snippet,
            sent_at: m.sent_at,
            is_read: m.is_read,
            is_flagged: m.is_flagged,
            has_attachments: m.has_attachments,
        }
    }
}

/// Composed outbound message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposedMessage {
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<ComposedAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposedAttachment {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}
