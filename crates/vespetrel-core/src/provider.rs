use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::account::SyncState;
use crate::folder::Folder;
use crate::message::{ComposedMessage, Flag};

/// Unified async provider trait - §4.1 of spec
#[async_trait]
pub trait MailProvider: Send + Sync {
    async fn sync_folder_list(&self) -> anyhow::Result<Vec<RemoteFolder>>;
    async fn sync_messages(&self, folder: &Folder, state: SyncState) -> anyhow::Result<SyncDelta>;
    async fn fetch_raw_message(&self, remote_id: &str) -> anyhow::Result<Vec<u8>>;
    async fn send_message(&self, message: &ComposedMessage) -> anyhow::Result<()>;
    async fn update_flags(
        &self,
        remote_ids: &[u32],
        add: &[Flag],
        remove: &[Flag],
    ) -> anyhow::Result<()>;
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
