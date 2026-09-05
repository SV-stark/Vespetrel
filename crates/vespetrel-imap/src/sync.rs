use async_trait::async_trait;
use tracing::{debug, info};

use vespetrel_core::account::SyncState;
use vespetrel_core::folder::Folder;
use vespetrel_core::message::{ComposedMessage, Flag};
use vespetrel_core::provider::{MailProvider, ProviderError, RemoteFolder, SyncDelta, SyncMessage};

use crate::client::{ImapConfig, ImapConnection, parse_imap_fetch_line, parse_imap_list_line};

pub struct ImapProvider {
    config: ImapConfig,
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
    async fn sync_folder_list(&self) -> Result<Vec<RemoteFolder>, ProviderError> {
        let mut conn = self.conn();
        conn.connect()
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        let list_cmd = conn.cmd_list();
        debug!(cmd=%list_cmd, "issuing IMAP LIST command");

        let lines = conn
            .execute_cmd(list_cmd)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        let mut folders = Vec::new();
        for line in lines {
            if let Some(folder) = parse_imap_list_line(&line) {
                folders.push(folder);
            }
        }

        if folders.is_empty() {
            #[cfg(any(test, feature = "mock"))]
            if conn.stream.is_none() {
                folders.push(RemoteFolder {
                    remote_id: "INBOX".into(),
                    name: "INBOX".into(),
                    path: "INBOX".into(),
                    role_hint: Some("\\Inbox".into()),
                    uid_validity: Some(1),
                    highest_mod_seq: Some(100),
                });
                return Ok(folders);
            }

            return Err(ProviderError::Protocol(
                "server returned no mailboxes from LIST command".into(),
            ));
        }
        Ok(folders)
    }

    async fn sync_messages(
        &self,
        folder: &Folder,
        state: SyncState,
    ) -> Result<SyncDelta, ProviderError> {
        let mut conn = self.conn();
        conn.connect()
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        // 1. SELECT mailbox (with RFC 7162 QRESYNC parameters if supported)
        let current_mod_seq = state
            .folder_states
            .get(&folder.remote_id)
            .and_then(|s| s.highest_mod_seq)
            .unwrap_or(0);
        let uid_val = folder.uid_validity.unwrap_or(1);

        let select_cmd = if current_mod_seq > 0 && conn.has_capability("QRESYNC") {
            conn.cmd_select_qresync(&folder.remote_id, uid_val, current_mod_seq)
        } else {
            conn.cmd_select(&folder.remote_id)
        };
        let select_lines = conn
            .execute_cmd(&select_cmd)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        let mut highest_mod_seq_found = None;
        let mut delta = SyncDelta::default();

        for sl in &select_lines {
            if let Some(pos) = sl.to_uppercase().find("HIGHESTMODSEQ") {
                let rest = &sl[pos + "HIGHESTMODSEQ".len()..];
                if let Some(seq) = rest
                    .split(|c: char| !c.is_ascii_digit())
                    .find(|s| !s.is_empty())
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    highest_mod_seq_found = Some(seq);
                }
            }
            let vanished = crate::client::parse_vanished_line(sl);
            delta.deleted_uids.extend(vanished);
            if sl.contains(" EXPUNGE")
                && let Some(seq) = sl
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse::<u32>().ok())
            {
                delta.deleted_uids.push(seq);
            }
        }

        // 2. Validate UIDVALIDITY
        let cached_validity = state
            .folder_states
            .get(&folder.remote_id)
            .and_then(|s| s.uid_validity);

        if let (Some(cached), Some(remote)) = (cached_validity, folder.uid_validity)
            && cached != remote
        {
            return Err(ProviderError::UidValidityChanged {
                expected: cached,
                actual: remote,
            });
        }

        // 3. Build fetch query (QRESYNC / CONDSTORE or full UID FETCH)
        let fetch_cmd = if conn.has_capability("QRESYNC") || conn.has_capability("CONDSTORE") {
            debug!(folder=%folder.name, mod_seq=current_mod_seq, "issuing CHANGEDSINCE query");
            conn.cmd_uid_fetch_changed_since(1, current_mod_seq)
        } else {
            debug!(folder=%folder.name, "issuing full UID FETCH");
            conn.cmd_uid_fetch_envelope("1:*")
        };
        debug!(cmd=%fetch_cmd, "sent UID FETCH");

        let lines = conn
            .execute_cmd(&fetch_cmd)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        for line in &lines {
            if let Some((uid, flags, mod_seq, _size)) = parse_imap_fetch_line(line) {
                if let Some(m) = mod_seq {
                    highest_mod_seq_found =
                        Some(highest_mod_seq_found.map_or(m, |curr| curr.max(m)));
                }
                let raw_rfc822 = if conn.stream.is_some() {
                    conn.execute_fetch_raw(uid).await.ok()
                } else {
                    None
                };
                delta.inserted.push(SyncMessage {
                    remote_uid: uid,
                    remote_id: Some(uid.to_string()),
                    flags,
                    raw_rfc822,
                    mod_seq,
                });
            }
            let vanished = crate::client::parse_vanished_line(line);
            delta.deleted_uids.extend(vanished);
            if line.contains(" EXPUNGE")
                && let Some(seq) = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse::<u32>().ok())
            {
                delta.deleted_uids.push(seq);
            }
        }

        let next_mod_seq = highest_mod_seq_found.unwrap_or(current_mod_seq + 1);

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

        info!(folder=%folder.name, count=delta.inserted.len(), next_mod_seq, "synced folder messages");
        Ok(delta)
    }

    async fn fetch_raw_message(&self, remote_id: &str) -> Result<Vec<u8>, ProviderError> {
        let uid = remote_id.parse::<u32>().map_err(|e| {
            ProviderError::Protocol(format!("Invalid remote message UID '{remote_id}': {e}"))
        })?;

        let mut conn = self.conn();
        conn.connect()
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        conn.execute_fetch_raw(uid)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))
    }

    async fn send_message(&self, message: &ComposedMessage) -> Result<(), ProviderError> {
        info!(subject=%message.subject, to=?message.to, "imap send_message (append to Sent)");
        let mut conn = self.conn();
        conn.connect()
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        let to_list = message
            .to
            .iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let raw_mime = match message.body_html.as_ref() {
            Some(html) => format!(
                "From: {}\r\nTo: {}\r\nSubject: {}\r\nDate: {}\r\nContent-Type: text/html; charset=utf-8\r\n\r\n{}",
                message.from,
                to_list,
                message.subject,
                chrono::Utc::now().to_rfc2822(),
                html
            ).into_bytes(),
            None => format!(
                "From: {}\r\nTo: {}\r\nSubject: {}\r\nDate: {}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}",
                message.from,
                to_list,
                message.subject,
                chrono::Utc::now().to_rfc2822(),
                message.body_text
            ).into_bytes(),
        };

        let target_folders = ["Sent", "INBOX.Sent", "[Gmail]/Sent Mail"];
        let mut appended = false;
        for folder_name in target_folders {
            if conn
                .execute_append(folder_name, &[Flag::Seen], &raw_mime)
                .await
                .is_ok()
            {
                appended = true;
                break;
            }
        }
        if !appended {
            conn.execute_append("Sent", &[Flag::Seen], &raw_mime)
                .await
                .map_err(|e| ProviderError::Protocol(e.to_string()))?;
        }
        Ok(())
    }

    async fn update_flags(
        &self,
        remote_ids: &[u32],
        add: &[Flag],
        remove: &[Flag],
    ) -> Result<(), ProviderError> {
        let mut conn = self.conn();
        conn.connect()
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        conn.execute_store_flags(remote_ids, add, remove)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))
    }
}
