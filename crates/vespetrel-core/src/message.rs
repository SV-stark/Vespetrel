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
            if name.contains([',', '<', '>', '"', '@', ';', ':']) {
                let escaped = name.replace('"', "\\\"");
                write!(f, "\"{escaped}\" <{}>", self.email)
            } else {
                write!(f, "{name} <{}>", self.email)
            }
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
    pub references: Option<String>,
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

impl Message {
    pub fn new(
        account_id: impl Into<String>,
        folder_id: impl Into<String>,
        remote_uid: u32,
        subject: impl Into<String>,
        from_address: impl Into<String>,
        to_addresses: Vec<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            account_id: account_id.into(),
            folder_id: folder_id.into(),
            thread_id: None,
            remote_uid,
            message_id_header: None,
            in_reply_to: None,
            references: None,
            subject: Some(subject.into()),
            from_address: from_address.into(),
            from_name: None,
            to_addresses: to_addresses
                .into_iter()
                .map(|email| Address { name: None, email })
                .collect(),
            cc_addresses: Vec::new(),
            bcc_addresses: Vec::new(),
            reply_to: None,
            sent_at: now,
            received_at: now,
            is_read: false,
            is_flagged: false,
            is_draft: false,
            has_attachments: false,
            body_snippet: None,
            body_text_preview: None,
            blob_path: String::new(),
            size_bytes: 0,
        }
    }

    pub fn summary(&self) -> MessageSummary {
        MessageSummary {
            id: self.id.clone(),
            thread_id: self.thread_id.clone(),
            subject: self.subject.clone(),
            from_address: self.from_address.clone(),
            from_name: self.from_name.clone(),
            snippet: self.body_snippet.clone(),
            sent_at: self.sent_at,
            is_read: self.is_read,
            is_flagged: self.is_flagged,
            has_attachments: self.has_attachments,
        }
    }
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
