use std::sync::Arc;

use async_trait::async_trait;
use tracing::{debug, info, warn};

use vespetrel_core::account::SyncState;
use vespetrel_core::folder::Folder;
use vespetrel_core::message::{ComposedMessage, Flag};
use vespetrel_core::provider::{MailProvider, RemoteFolder, SyncDelta, SyncMessage};

use crate::client::{ImapConfig, ImapConnection};

pub struct ImapProvider {
    config: ImapConfig,
    // In production: deadpool of connections or single multiplexed connection
}

impl ImapProvider {
    pub fn new(config: ImapConfig) -> Self {
        Self { config }
    }

    fn conn(&self) -> ImapConnection {
        ImapConnection::new(self.config.clone())
    }
}

#[async_trait]
impl MailProvider for ImapProvider {
    async fn sync_folder_list(&self) -> anyhow::Result<Vec<RemoteFolder>> {
        let mut conn = self.conn();
        conn.connect().await?;
        // Real: LIST "" "*" + SPECIAL-USE detection + XLIST fallback
        // Stub returns common folders
        debug!("sync_folder_list stub");
        Ok(vec![
            RemoteFolder { remote_id: "INBOX".into(), name: "INBOX".into(), path: "INBOX".into(), role_hint: Some("\\Inbox".into()), uid_validity: Some(1), highest_mod_seq: Some(100) },
            RemoteFolder { remote_id: "Sent".into(), name: "Sent".into(), path: "Sent".into(), role_hint: Some("\\Sent".into()), uid_validity: Some(1), highest_mod_seq: Some(50) },
            RemoteFolder { remote_id: "Drafts".into(), name: "Drafts".into(), path: "Drafts".into(), role_hint: Some("\\Drafts".into()), uid_validity: Some(1), highest_mod_seq: Some(10) },
        ])
    }

    async fn sync_messages(&self, folder: &Folder, state: SyncState) -> anyhow::Result<SyncDelta> {
        let mut conn = self.conn();
        conn.connect().await?;

        // QRESYNC / CONDSTORE logic §4.2
        // 1. Check UIDVALIDITY
        if let Some(cached_validity) = state.folder_states.get(&folder.remote_id).and_then(|s| s.uid_validity) {
            if let Some(remote_validity) = folder.uid_validity {
                if cached_validity != remote_validity {
                    return Err(anyhow::anyhow!(crate::ImapError::UidValidityChanged));
                }
            }
        }

        // 2. If QRESYNC available, use CHANGEDSINCE
        let delta = if conn.has_capability("QRESYNC") {
            let mod_seq = state.folder_states.get(&folder.remote_id).and_then(|s| s.highest_mod_seq).unwrap_or(0);
            debug!(folder=%folder.name, mod_seq, "using QRESYNC CHANGEDSINCE");
            // Real: UID FETCH ... (CHANGEDSINCE mod_seq)
            SyncDelta::default()
        } else if conn.has_capability("CONDSTORE") {
            debug!(folder=%folder.name, "using CONDSTORE");
            SyncDelta::default()
        } else {
            debug!(folder=%folder.name, "full UID FETCH fallback");
            SyncDelta::default()
        };

        info!(folder=%folder.name, inserted=%delta.inserted.len(), "synced folder");
        Ok(delta)
    }

    async fn fetch_raw_message(&self, remote_id: &str) -> anyhow::Result<Vec<u8>> {
        let mut conn = self.conn();
        conn.connect().await?;
        debug!(remote_id, "fetch_raw_message stub");
        // Real: UID FETCH <uid> (BODY.PEEK[])
        Ok(format!("From: stub@example.com\r\nSubject: Stub {}\r\n\r\nBody stub", remote_id).into_bytes())
    }

    async fn send_message(&self, message: &ComposedMessage) -> anyhow::Result<()> {
        // IMAP APPEND for drafts, SMTP for sending is handled by vespetrel-smtp
        // But provider::send delegates to SMTP engine; IMAP provider just appends to Sent if needed
        info!(subject=%message.subject, to=?message.to, "imap send_message (append to Sent)");
        Ok(())
    }

    async fn update_flags(&self, remote_ids: &[u32], add: &[Flag], remove: &[Flag]) -> anyhow::Result<()> {
        let mut conn = self.conn();
        conn.connect().await?;
        let add_str = add.iter().map(|f| f.as_imap_str()).collect::<Vec<_>>().join(" ");
        let rem_str = remove.iter().map(|f| f.as_imap_str()).collect::<Vec<_>>().join(" ");
        debug!(uids=?remote_ids, add=%add_str, remove=%rem_str, "UID STORE flags");
        // Real: UID STORE <set> +FLAGS (...) / -FLAGS (...)
        Ok(())
    }
}
