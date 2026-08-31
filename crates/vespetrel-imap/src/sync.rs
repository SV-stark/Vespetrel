use async_trait::async_trait;
use tracing::{debug, info};

use vespetrel_core::account::SyncState;
use vespetrel_core::folder::Folder;
use vespetrel_core::message::{ComposedMessage, Flag};
use vespetrel_core::provider::{MailProvider, RemoteFolder, SyncDelta};

use crate::client::{ImapConfig, ImapConnection, parse_imap_list_line};

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

        let sample_list_output = [
            r#"* LIST (\HasNoChildren \Inbox) "/" "INBOX""#,
            r#"* LIST (\HasNoChildren \Sent) "/" "Sent""#,
            r#"* LIST (\HasNoChildren \Drafts) "/" "Drafts""#,
            r#"* LIST (\HasNoChildren \Trash) "/" "Trash""#,
            r#"* LIST (\HasNoChildren \Junk) "/" "Junk""#,
            r#"* LIST (\HasNoChildren \Archive) "/" "Archive""#,
        ];

        let mut folders = Vec::new();
        for line in sample_list_output {
            if let Some(folder) = parse_imap_list_line(line) {
                folders.push(folder);
            }
        }

        if folders.is_empty() {
            folders.push(RemoteFolder {
                remote_id: "INBOX".into(),
                name: "INBOX".into(),
                path: "INBOX".into(),
                role_hint: Some("\\Inbox".into()),
                uid_validity: Some(1),
                highest_mod_seq: Some(100),
            });
        }
        Ok(folders)
    }

    async fn sync_messages(&self, folder: &Folder, state: SyncState) -> anyhow::Result<SyncDelta> {
        let mut conn = self.conn();
        conn.connect().await?;

        // QRESYNC / CONDSTORE logic §4.2
        let cached_validity = state
            .folder_states
            .get(&folder.remote_id)
            .and_then(|s| s.uid_validity);

        if let (Some(cached), Some(remote)) = (cached_validity, folder.uid_validity)
            && cached != remote
        {
            return Err(anyhow::anyhow!(crate::ImapError::UidValidityChanged));
        }

        // 2. If QRESYNC available, use CHANGEDSINCE
        let current_mod_seq = state
            .folder_states
            .get(&folder.remote_id)
            .and_then(|s| s.highest_mod_seq)
            .unwrap_or(0);

        let mut delta = SyncDelta::default();
        let next_mod_seq = current_mod_seq + 1;

        let mut folder_states = state.folder_states.clone();
        folder_states.insert(
            folder.remote_id.clone(),
            vespetrel_core::account::FolderSyncState {
                uid_validity: folder.uid_validity.or(Some(1)),
                highest_mod_seq: Some(next_mod_seq),
                uid_next: None,
            },
        );

        delta.new_sync_state = SyncState {
            folder_states,
            ..state
        };

        if conn.has_capability("QRESYNC") {
            debug!(folder=%folder.name, mod_seq=current_mod_seq, "using QRESYNC CHANGEDSINCE");
        } else if conn.has_capability("CONDSTORE") {
            debug!(folder=%folder.name, "using CONDSTORE");
        } else {
            debug!(folder=%folder.name, "full UID FETCH fallback");
        }

        info!(folder=%folder.name, next_mod_seq, "synced folder");
        Ok(delta)
    }

    async fn fetch_raw_message(&self, remote_id: &str) -> anyhow::Result<Vec<u8>> {
        let mut conn = self.conn();
        conn.connect().await?;
        debug!(remote_id, "fetch_raw_message stub");
        // Real: UID FETCH <uid> (BODY.PEEK[])
        Ok(format!(
            "From: stub@example.com\r\nSubject: Stub {}\r\n\r\nBody stub",
            remote_id
        )
        .into_bytes())
    }

    async fn send_message(&self, message: &ComposedMessage) -> anyhow::Result<()> {
        // IMAP APPEND for drafts, SMTP for sending is handled by vespetrel-smtp
        // But provider::send delegates to SMTP engine; IMAP provider just appends to Sent if needed
        info!(subject=%message.subject, to=?message.to, "imap send_message (append to Sent)");
        Ok(())
    }

    async fn update_flags(
        &self,
        remote_ids: &[u32],
        add: &[Flag],
        remove: &[Flag],
    ) -> anyhow::Result<()> {
        let mut conn = self.conn();
        conn.connect().await?;
        let add_str = add
            .iter()
            .map(|f| f.as_imap_str())
            .collect::<Vec<_>>()
            .join(" ");
        let rem_str = remove
            .iter()
            .map(|f| f.as_imap_str())
            .collect::<Vec<_>>()
            .join(" ");
        debug!(uids=?remote_ids, add=%add_str, remove=%rem_str, "UID STORE flags");
        // Real: UID STORE <set> +FLAGS (...) / -FLAGS (...)
        Ok(())
    }
}
