use async_trait::async_trait;
use tracing::{debug, info};

use vespetrel_core::account::SyncState;
use vespetrel_core::folder::Folder;
use vespetrel_core::message::{ComposedMessage, Flag};
use vespetrel_core::provider::{MailProvider, ProviderError, RemoteFolder, SyncDelta, SyncMessage};

use crate::client::{ImapConfig, ImapConnection, parse_imap_fetch_line, parse_imap_list_line};

/// Collect `* VANISHED` UIDs from a response line.
///
/// Returns `true` when a range was too large to expand, meaning the caller must discard the
/// folder's local UID state and re-fetch from scratch.
fn collect_vanished(line: &str, delta: &mut SyncDelta) -> bool {
    let uids = crate::client::parse_vanished_line(line);
    if uids.is_empty() {
        return false;
    }
    if uids.contains(&crate::client::VANISHED_RANGE_TOO_LARGE) {
        return true;
    }
    delta.deleted_uids.extend(uids);
    false
}

/// Extract the UID from a `* <seq> FETCH (UID <uid> ...)` response line.
fn parse_fetch_uid(line: &str) -> Option<u32> {
    let upper = line.to_ascii_uppercase();
    let pos = upper.find("UID ")?;
    line[pos + 4..]
        .split(|c: char| !c.is_ascii_digit())
        .find(|s| !s.is_empty())?
        .parse()
        .ok()
}

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
            if conn.stream.is_none() || conn.is_mock() {
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
        let select_tag = conn.last_tag.clone();
        let select_lines = conn
            .execute_cmd(&select_cmd)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        // A rejected SELECT must abort the folder. Previously the NO/BAD reply was ignored,
        // the FETCH also failed, and `highest_mod_seq_found` stayed `None` so the code
        // invented `current + 1` and persisted it — moving the cursor past messages that
        // were never received. If the failure was transient, they were lost permanently.
        if let Some(err) = select_lines.iter().find(|l| {
            l.starts_with(&format!("{select_tag} NO"))
                || l.starts_with(&format!("{select_tag} BAD"))
        }) {
            return Err(ProviderError::Protocol(format!(
                "SELECT {} rejected: {}",
                folder.remote_id,
                err.trim()
            )));
        }

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
            collect_vanished(sl, &mut delta);
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
                .map_err(|e| ProviderError::Protocol(e.to_string()))?
        };
        debug!(folder=%folder.name, "sent UID FETCH");

        let lines = conn
            .execute_cmd(&fetch_cmd)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        // Map message *sequence* numbers to UIDs as the FETCH responses reveal them, so a
        // bare `* n EXPUNGE` can be resolved. The previous code used the sequence number
        // directly as a UID, which deletes the wrong local message whenever sequence
        // numbers and UIDs differ (always true after a server-side move).
        let mut seq_to_uid: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        // Any VANISHED range too large to expand forces a full resync of this folder.
        let mut force_full_resync = false;

        for line in &lines {
            if line.contains(" FETCH")
                && let Some(seq) = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse::<u32>().ok())
                && let Some(uid) = parse_fetch_uid(line)
            {
                seq_to_uid.insert(seq, uid);
            }
        }

        for line in &lines {
            if let Some((uid, flags, mod_seq, _size)) = parse_imap_fetch_line(line) {
                if let Some(m) = mod_seq {
                    highest_mod_seq_found =
                        Some(highest_mod_seq_found.map_or(m, |curr| curr.max(m)));
                }
                // Do not record a message as synced until its body is actually retrieved.
                // `.ok()` here used to store `raw_rfc822: None` for a transient fetch
                // failure; since the next sync uses CHANGEDSINCE, the server would not
                // resend it and the message stayed bodyless forever.
                let raw_rfc822 = if conn.stream.is_some() {
                    Some(conn.execute_fetch_raw(uid).await.map_err(|e| {
                        ProviderError::Protocol(format!(
                            "failed to fetch body for UID {uid} in {}: {e}",
                            folder.remote_id
                        ))
                    })?)
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
            force_full_resync |= collect_vanished(line, &mut delta);
            if line.contains(" EXPUNGE")
                && let Some(seq) = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse::<u32>().ok())
            {
                // Resolve the sequence number to a UID. Without a mapping we cannot say
                // which message vanished, and guessing deleted an unrelated one.
                if let Some(uid) = seq_to_uid.get(&seq) {
                    delta.deleted_uids.push(*uid);
                } else {
                    debug!(
                        folder=%folder.name, seq,
                        "EXPUNGE sequence number could not be mapped to a UID; forcing resync"
                    );
                    force_full_resync = true;
                }
            }
        }

        if force_full_resync {
            return Err(ProviderError::Protocol(format!(
                "{}: too many or unmappable expunges; a full resync is required",
                folder.remote_id
            )));
        }

        // Never invent a sequence number. `unwrap_or(current + 1)` moved the cursor forward
        // even when the server told us nothing.
        let next_mod_seq = highest_mod_seq_found.unwrap_or(current_mod_seq);

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

        // Every interpolated value goes through `hdr_safe` (inside the builder). A subject
        // or display name containing CRLF otherwise injects arbitrary headers into the
        // stored copy, and since it is APPENDed to the user's own Sent folder those headers
        // render in the thread view and re-enter the client on the next sync.
        let raw_mime = build_sent_copy_rfc822(message);

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

/// Make a value safe to interpolate into a single RFC 5322 header field.
///
/// CR and LF become a space so a value cannot start a new header line, and NUL is removed
/// (it truncates the value in several IMAP/MIME parsers, hiding the rest of the field).
pub fn hdr_safe(value: &str) -> String {
    value
        .chars()
        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
        .filter(|c| *c != '\0')
        .collect()
}

/// Build the RFC 5322 message APPENDed to the account's Sent folder.
///
/// Split out from the provider so the header-construction rules (in particular, that no
/// untrusted value can inject a header) are testable without a live IMAP server.
pub fn build_sent_copy_rfc822(message: &ComposedMessage) -> Vec<u8> {
    let to_list = hdr_safe(
        &message
            .to
            .iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(", "),
    );
    let cc_list = hdr_safe(
        &message
            .cc
            .iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(", "),
    );
    let from_hdr = hdr_safe(&message.from.to_string());
    let subject_hdr = hdr_safe(&message.subject);
    let date_hdr = hdr_safe(&chrono::Utc::now().to_rfc2822());

    let mut raw = String::with_capacity(message.body_text.len() + 512);
    raw.push_str(&format!("From: {from_hdr}\r\nTo: {to_list}\r\n"));
    if !cc_list.is_empty() {
        raw.push_str(&format!("Cc: {cc_list}\r\n"));
    }
    raw.push_str(&format!("Subject: {subject_hdr}\r\nDate: {date_hdr}\r\n"));
    match message.body_html.as_ref() {
        Some(html) => {
            raw.push_str("MIME-Version: 1.0\r\nContent-Type: text/html; charset=utf-8\r\n\r\n");
            raw.push_str(html);
        }
        None => {
            raw.push_str("MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n");
            raw.push_str(&message.body_text);
        }
    }
    raw.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vespetrel_core::message::Address;

    fn msg(subject: &str) -> ComposedMessage {
        ComposedMessage {
            from: Address {
                name: Some("Alice".into()),
                email: "alice@example.com".into(),
            },
            to: vec![Address {
                name: Some("Bob".into()),
                email: "bob@example.com".into(),
            }],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: subject.into(),
            body_text: "hi".into(),
            body_html: None,
            in_reply_to: None,
            references: Vec::new(),
            attachments: Vec::new(),
        }
    }

    /// A subject is fully attacker-controlled whenever a message is *forwarded* or a reply
    /// is auto-generated. Interpolating it raw let a CRLF inject headers into the copy that
    /// is APPENDed to the user's own Sent folder, which then re-enter the client on the
    /// next sync as if the user's mail server had produced them.
    #[test]
    fn sent_copy_rejects_header_injection() {
        let raw = String::from_utf8(build_sent_copy_rfc822(&msg(
            "hi\r\nX-Injected: yes\r\n\r\n<html>spoof</html>",
        )))
        .unwrap();

        let header_block = raw.split("\r\n\r\n").next().unwrap();
        let injected: Vec<&str> = header_block
            .lines()
            .filter(|l| l.to_ascii_lowercase().starts_with("x-"))
            .collect();
        assert!(injected.is_empty(), "header injected:\n{raw}");
        // The folded text stays inside the Subject value, on one line.
        let subject = header_block
            .lines()
            .find_map(|l| l.strip_prefix("Subject: "))
            .expect("Subject header");
        assert_eq!(subject, "hi  X-Injected: yes    <html>spoof</html>");
        // The body separator still occurs exactly once.
        assert_eq!(raw.matches("\r\n\r\n").count(), 1);
    }

    #[test]
    fn sent_copy_reports_cc_and_preserves_body() {
        let mut m = msg("Hello");
        m.cc = vec![Address {
            name: Some("Carol".into()),
            email: "carol@example.com".into(),
        }];
        m.body_html = Some("<p>hi</p>".into());
        let raw = String::from_utf8(build_sent_copy_rfc822(&m)).unwrap();
        assert!(raw.contains("Cc: Carol <carol@example.com>"), "{raw}");
        assert!(
            raw.contains("Content-Type: text/html; charset=utf-8"),
            "{raw}"
        );
        assert!(raw.ends_with("\r\n\r\n<p>hi</p>"), "{raw}");
    }
}
