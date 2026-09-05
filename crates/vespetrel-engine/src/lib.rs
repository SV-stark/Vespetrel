//! Vespetrel Engine - Tokio sync coordinator + account worker actors

pub mod coordinator;
pub mod event_bus;
pub mod feeds;
pub mod filter;
pub mod matrix;
pub mod migrator;
pub mod outbox;
pub mod plugin;
pub mod pop3;
pub mod sieve;
pub mod snooze;
pub mod spam;
pub mod worker;

pub use coordinator::{SyncCoordinator, make_provider};
pub use event_bus::{EventBus, EventReceiver, EventSender};
pub use feeds::{FeedItem, FeedSubscription, parse_feed_xml};
pub use filter::{
    ConditionCombinator, FilterAction, FilterCondition, FilterEngine, FilterField, FilterPredicate,
    FilterRule,
};
pub use matrix::{MatrixBridge, MatrixEvent, MatrixRoom};
pub use migrator::{
    MigratedAccount, ThunderbirdProfile, discover_thunderbird_profiles, parse_mbox_data,
    parse_thunderbird_prefs,
};
pub use outbox::{OutboxQueue, ScheduledMessage, UndoSendBuffer};
pub use plugin::{PluginAction, PluginEvent, PluginHost, PluginManifest, PluginPermission};
pub use pop3::{
    Pop3Command, Pop3MessageInfo, Pop3UidlCache, parse_pop3_status, parse_pop3_uidl_response,
};
pub use sieve::{ManageSieveCommand, SieveResponse, SieveScript, SieveValidator};
pub use snooze::{SnoozeManager, SnoozedThread};
pub use spam::{BayesClassifier, SpamScore};
pub use worker::{AccountWorker, WorkerCommand};

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use vespetrel_core::account::SyncState;
    use vespetrel_core::folder::Folder;
    use vespetrel_core::message::{ComposedMessage, Flag};
    use vespetrel_core::provider::{
        MailProvider, ProviderError, RemoteFolder, SyncDelta, SyncMessage,
    };

    struct MockProvider;

    #[async_trait]
    impl MailProvider for MockProvider {
        async fn sync_folder_list(&self) -> Result<Vec<RemoteFolder>, ProviderError> {
            Ok(vec![RemoteFolder {
                remote_id: "INBOX".into(),
                name: "INBOX".into(),
                path: "INBOX".into(),
                role_hint: Some("inbox".into()),
                uid_validity: Some(1),
                highest_mod_seq: Some(10),
            }])
        }

        async fn sync_messages(
            &self,
            _folder: &Folder,
            _state: SyncState,
        ) -> Result<SyncDelta, ProviderError> {
            Ok(SyncDelta {
                inserted: vec![SyncMessage {
                    remote_uid: 101,
                    remote_id: Some("101".into()),
                    flags: vec![Flag::Seen],
                    raw_rfc822: Some(b"From: sender@example.com\r\nTo: user@domain.com\r\nSubject: Message 101\r\n\r\nHello test".to_vec()),
                    mod_seq: Some(1),
                }],
                ..Default::default()
            })
        }

        async fn fetch_raw_message(&self, _remote_id: &str) -> Result<Vec<u8>, ProviderError> {
            Ok(vec![])
        }
        async fn send_message(&self, _msg: &ComposedMessage) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn update_flags(
            &self,
            _uids: &[u32],
            _add: &[Flag],
            _rem: &[Flag],
        ) -> Result<(), ProviderError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn coordinator_spawns_and_dispatches_events() {
        let (mut coord, mut rx) = SyncCoordinator::create();
        let provider = Arc::new(MockProvider);

        coord.spawn_worker("user@domain.com", provider);

        // Expect FolderListUpdated event
        let mut got_folders = false;
        let mut got_messages = false;

        while let Some(ev) = rx.recv().await {
            match ev {
                vespetrel_core::provider::SyncEvent::FolderListUpdated(f) => {
                    assert_eq!(f.len(), 1);
                    got_folders = true;
                }
                vespetrel_core::provider::SyncEvent::MessagesInserted(m) => {
                    assert_eq!(m.len(), 1);
                    assert_eq!(m[0].subject.as_deref(), Some("Message 101"));
                    got_messages = true;
                }
                vespetrel_core::provider::SyncEvent::SyncFinished { .. } => {
                    break;
                }
                _ => {}
            }
        }

        assert!(got_folders);
        assert!(got_messages);
        coord.stop_all();
    }

    #[tokio::test]
    async fn coordinator_bounded_flume_spawns_and_dispatches_events() {
        let (mut coord, rx) = SyncCoordinator::create_bounded(256);
        let provider = Arc::new(MockProvider);

        coord.spawn_worker("user-bounded@domain.com", provider);

        let mut got_folders = false;
        let mut got_messages = false;

        while let Ok(ev) = rx.recv_async().await {
            match ev {
                vespetrel_core::provider::SyncEvent::FolderListUpdated(f) => {
                    assert_eq!(f.len(), 1);
                    got_folders = true;
                }
                vespetrel_core::provider::SyncEvent::MessagesInserted(m) => {
                    assert_eq!(m.len(), 1);
                    assert_eq!(m[0].subject.as_deref(), Some("Message 101"));
                    got_messages = true;
                }
                vespetrel_core::provider::SyncEvent::SyncFinished { .. } => {
                    break;
                }
                _ => {}
            }
        }

        assert!(got_folders);
        assert!(got_messages);
        coord.stop_all();
    }
}
