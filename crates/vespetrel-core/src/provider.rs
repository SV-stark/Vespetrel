use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::account::SyncState;
use crate::folder::Folder;
use crate::message::{ComposedMessage, Flag};

/// How a provider delivers changes.
///
/// The distinction is not cosmetic: it decides whether the sync engine calls
/// [`MailProvider::sync_messages`] once per folder or calls
/// [`MailProvider::sync_mailbox`] exactly once.
///
/// * [`SyncScope::Folder`] — IMAP, and any provider whose cursor is per-folder
///   (`HIGHESTMODSEQ`, `UIDVALIDITY`). The engine loops over folders.
/// * [`SyncScope::Mailbox`] — providers with one cursor covering the whole mailbox, such as
///   Gmail's `users.history.list`. The engine calls the mailbox once and fans the result out.
///
/// Getting this wrong drops mail. Looping a mailbox-scoped provider per folder would either
/// re-walk the same history N times or, worse, attach every changed message to whichever folder
/// happened to be iterated first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncScope {
    /// Cursor is per-folder; call `sync_messages` once per folder.
    Folder,
    /// Cursor covers the whole mailbox; call `sync_mailbox` once.
    Mailbox,
}

/// A mailbox-scoped delta, already partitioned by destination folder.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FolderDelta {
    /// `RemoteFolder::remote_id` this delta applies to.
    pub folder_remote_id: String,
    pub delta: SyncDelta,
}

/// Unified async provider trait - §4.1 of spec
#[async_trait]
pub trait MailProvider: Send + Sync {
    /// Which delivery model this provider uses. Defaults to per-folder.
    fn sync_scope(&self) -> SyncScope {
        SyncScope::Folder
    }

    async fn sync_folder_list(&self) -> Result<Vec<RemoteFolder>, ProviderError>;

    /// Sync one folder. Called only when [`Self::sync_scope`] is [`SyncScope::Folder`].
    async fn sync_messages(
        &self,
        folder: &Folder,
        state: SyncState,
    ) -> Result<SyncDelta, ProviderError>;

    /// Sync the whole mailbox, partitioned by destination folder.
    ///
    /// Called exactly once per cycle when [`Self::sync_scope`] is [`SyncScope::Mailbox`]. The
    /// default implementation refuses, so a provider that declares mailbox scope without
    /// implementing this fails loudly instead of silently syncing nothing.
    async fn sync_mailbox(&self, _state: SyncState) -> Result<Vec<FolderDelta>, ProviderError> {
        Err(ProviderError::Protocol(
            "provider declares mailbox sync scope but implements no mailbox sync".into(),
        ))
    }

    /// Cheap check for whether the mailbox is already mirrored locally.
    ///
    /// Lets a provider decide between a full sync and a cursor walk without hard-coding that
    /// knowledge into the engine. `None` means "unknown", and the engine falls back to a full
    /// sync, which is always safe.
    async fn is_mirror_complete(&self, _state: &SyncState) -> Option<bool> {
        None
    }

    /// Supply the set of provider-native message ids already mirrored locally.
    ///
    /// Called once per cycle, before [`Self::sync_mailbox`] or [`Self::sync_messages`]. A
    /// provider needs it to distinguish a genuinely new message from one that merely changed
    /// state: without it, every flag change costs a full refetch, which exhausts Gmail's
    /// per-project quota on an active mailbox.
    ///
    /// The default is a no-op, so providers that do not need it are unaffected.
    fn set_stored_message_ids(&self, _ids: std::collections::BTreeSet<String>) {}
    async fn fetch_raw_message(&self, remote_id: &str) -> Result<Vec<u8>, ProviderError>;
    async fn send_message(&self, message: &ComposedMessage) -> Result<(), ProviderError>;
    async fn update_flags(
        &self,
        remote_ids: &[u32],
        add: &[Flag],
        remove: &[Flag],
    ) -> Result<(), ProviderError>;

    async fn update_flags_by_remote_id(
        &self,
        remote_ids: &[String],
        add: &[Flag],
        remove: &[Flag],
    ) -> Result<(), ProviderError> {
        let numeric_uids: Vec<u32> = remote_ids.iter().filter_map(|s| s.parse().ok()).collect();
        self.update_flags(&numeric_uids, add, remove).await
    }
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
    /// Deleted messages by IMAP-style numeric UID.
    ///
    /// IMAP-addressable only. Providers whose message identifiers are opaque strings (Gmail's
    /// `messages/{id}`) must use [`Self::deleted_remote_ids`] instead; `u32` cannot represent
    /// them without inventing a lossy mapping.
    pub deleted_uids: Vec<u32>,
    /// Deleted messages by provider-native string id.
    ///
    /// Added for providers that do not expose numeric UIDs. IMAP leaves this empty and uses
    /// `deleted_uids`. Consumers should treat a deletion as present in either list.
    #[serde(default)]
    pub deleted_remote_ids: Vec<String>,
    pub new_sync_state: SyncState,
}

impl SyncDelta {
    /// Does this delta carry no work at all?
    ///
    /// Distinguishes "the provider found no changes" from "the provider found no changes *and*
    /// advanced the cursor". A cursor advance alone still has to be persisted, or the same
    /// empty range is re-read on every cycle forever.
    pub fn is_empty(&self) -> bool {
        self.inserted.is_empty()
            && self.updated.is_empty()
            && self.deleted_uids.is_empty()
            && self.deleted_remote_ids.is_empty()
            && self.new_sync_state == SyncState::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMessage {
    pub remote_uid: u32,
    #[serde(default)]
    pub remote_id: Option<String>,
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
