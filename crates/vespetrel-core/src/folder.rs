use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FolderRole {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Archive,
    Junk,
    Custom,
}

impl std::fmt::Display for FolderRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Inbox => "inbox",
            Self::Sent => "sent",
            Self::Drafts => "drafts",
            Self::Trash => "trash",
            Self::Archive => "archive",
            Self::Junk => "junk",
            Self::Custom => "custom",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for FolderRole {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "inbox" => Self::Inbox,
            "sent" => Self::Sent,
            "drafts" => Self::Drafts,
            "trash" => Self::Trash,
            "archive" => Self::Archive,
            "junk" | "spam" => Self::Junk,
            _ => Self::Custom,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub account_id: String,
    /// Remote identifier (IMAP mailbox path, JMAP mailboxId, Graph folderId)
    pub remote_id: String,
    pub name: String,
    /// Full path like "INBOX/Work" or "/mail/Foo"
    pub path: String,
    pub role: FolderRole,
    pub uid_validity: Option<u32>,
    pub highest_mod_seq: Option<u64>,
    pub total_count: i64,
    pub unread_count: i64,
}

impl Folder {
    pub fn new(account_id: impl Into<String>, remote_id: impl Into<String>, name: impl Into<String>, path: impl Into<String>) -> Self {
        let name_s = name.into();
        let path_s = path.into();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            account_id: account_id.into(),
            remote_id: remote_id.into(),
            name: name_s,
            path: path_s,
            role: FolderRole::Custom,
            uid_validity: None,
            highest_mod_seq: Some(0),
            total_count: 0,
            unread_count: 0,
        }
    }

    /// Infer role from IMAP SPECIAL-USE or well-known names
    pub fn infer_role(name: &str, special_use: Option<&str>) -> FolderRole {
        if let Some(s) = special_use {
            match s.to_lowercase().as_str() {
                "\\sent" => return FolderRole::Sent,
                "\\drafts" => return FolderRole::Drafts,
                "\\trash" => return FolderRole::Trash,
                "\\archive" => return FolderRole::Archive,
                "\\junk" | "\\spam" => return FolderRole::Junk,
                _ => {}
            }
        }
        match name.to_lowercase().as_str() {
            "inbox" | "inbox/" => FolderRole::Inbox,
            "sent" | "sent items" | "sent mail" => FolderRole::Sent,
            "drafts" => FolderRole::Drafts,
            "trash" | "deleted items" | "deleted" => FolderRole::Trash,
            "archive" => FolderRole::Archive,
            "junk" | "spam" => FolderRole::Junk,
            _ => FolderRole::Custom,
        }
    }
}
