//! Vespetrel Core - Domain models

pub mod account;
pub mod attachment;
pub mod contact;
pub mod error;
pub mod folder;
pub mod invite;
pub mod mailing_list;
pub mod message;
pub mod provider;
pub mod richtext;
pub mod settings;
pub mod signature;

pub mod split_inbox;
pub mod tag;
pub mod template;
pub mod thread;
pub mod virtual_folder;

pub use account::{Account, AuthConfig, ProviderType, SyncState};
pub use attachment::Attachment;
pub use contact::{CalendarEvent, Contact, TaskItem};
pub use error::{CoreError, CoreResult};
pub use folder::{Folder, FolderRole};
pub use invite::{MeetingInvitation, RsvpStatus};
pub use mailing_list::{MailingList, MailingListExpander};
pub use message::{
    Address, ComposedAttachment, ComposedMessage, Flag, Message, MessageSummary, stable_uid_from_id,
};
pub use provider::{MailProvider, RemoteFolder, SyncDelta, SyncEvent};
pub use richtext::{BlockKind, InlineStyle, RichTextDocument, TextSpan};
pub use settings::*;
pub use signature::{Signature, SignatureProfile, SignatureStore, SignatureTemplate};
pub use split_inbox::{InboxCategory, classify_inbox_category};
pub use tag::{MessageTag, TagStore};
pub use template::{EmailTemplate, TemplateStore};
pub use thread::{Thread, ThreadNode, ThreadTree, normalize_subject};
pub use virtual_folder::{VirtualFolder, VirtualFolderType};
