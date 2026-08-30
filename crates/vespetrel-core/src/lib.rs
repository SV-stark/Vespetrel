//! Vespetrel Core - Domain models

pub mod account;
pub mod attachment;
pub mod contact;
pub mod error;
pub mod folder;
pub mod message;
pub mod provider;
pub mod thread;

pub use account::{Account, AuthConfig, ProviderType, SyncState};
pub use attachment::Attachment;
pub use contact::{CalendarEvent, Contact};
pub use error::{CoreError, CoreResult};
pub use folder::{Folder, FolderRole};
pub use message::{Address, ComposedAttachment, ComposedMessage, Flag, Message, MessageSummary};
pub use provider::{MailProvider, RemoteFolder, SyncDelta, SyncEvent};
pub use thread::{Thread, ThreadNode, ThreadTree, normalize_subject};
