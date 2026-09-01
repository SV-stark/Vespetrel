use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::account::SyncState;
use crate::folder::Folder;
use crate::message::{ComposedMessage, Flag};

/// Unified async provider trait - §4.1 of spec
#[async_trait]
pub trait MailProvider: Send + Sync {
    async fn sync_folder_list(&self) -> Result<Vec<RemoteFolder>, ProviderError>;
    async fn sync_messages(&self, folder: &Folder, state: SyncState) -> Result<SyncDelta, ProviderError>;
    async fn fetch_raw_message(&self, remote_id: &str) -> Result<Vec<u8>, ProviderError>;
    async fn send_message(&self, message: &ComposedMessage) -> Result<(), ProviderError>;
    async fn update_flags(
        &self,
        remote_ids: &[u32],
        add: &[Flag],
        remove: &[Flag],
    ) -> Result<(), ProviderError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteFolder {
    pub remote_id: String,
    pub name: String,
    pub path: String,
    pub role_hint: Option<String>,
    pub uid_validity: Option<u32>,
    pub highest_mod_seq: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncDelta {
    pub inserted: Vec<SyncMessage>,
    pub updated: Vec<SyncMessage>,
    pub deleted_uids: Vec<u32>,
    pub new_sync_state: SyncState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMessage {
    pub remote_uid: u32,
    pub flags: Vec<Flag>,
    pub raw_rfc822: Option<Vec<u8>>,
    pub mod_seq: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Uid(pub u32);

impl From<u32> for Uid {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

impl std::fmt::Display for Uid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("Authentication failed: {0}")]
    AuthError(String),
    #[error("UIDVALIDITY changed from {expected} to {actual} - local cache invalidated")]
    UidValidityChanged { expected: u32, actual: u32 },
    #[error("Protocol error: {0}")]
    Protocol(String),
    #[error("Connection timed out: {0}")]
    Timeout(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Other provider error: {0}")]
    Other(String),
}

#[derive(Debug, Clone)]
pub enum SyncEvent {
    FolderListUpdated(Vec<RemoteFolder>),
    MessagesInserted(Vec<crate::message::MessageSummary>),
    MessageFlagsUpdated {
        id: String,
        is_read: bool,
        is_flagged: bool,
    },
    MessagesDeleted(Vec<String>),
    SyncError {
        folder: String,
        error: String,
    },
    SyncFinished {
        account_id: String,
    },
}
