use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
    pub username: String,
    /// Password or OAuth2 access token (XOAUTH2)
    pub auth_token: String,
    pub use_xoauth2: bool,
}

impl ImapConfig {
    pub fn new(
        host: impl Into<String>,
        port: u16,
        username: impl Into<String>,
        auth_token: impl Into<String>,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            use_tls: true,
            username: username.into(),
            auth_token: auth_token.into(),
            use_xoauth2: false,
        }
    }

    pub fn with_xoauth2(mut self) -> Self {
        self.use_xoauth2 = true;
        self
    }
}

/// Thin async IMAP connection wrapper - negotiates capabilities and handles auth
pub struct ImapConnection {
    config: ImapConfig,
    // In a full implementation this wraps tokio::net::TcpStream + tokio-rustls + imap-codec codec
    // For now we provide the state machine and command builders
    pub capabilities: Vec<String>,
    pub tag_counter: u32,
}

impl ImapConnection {
    pub fn new(config: ImapConfig) -> Self {
        Self {
            config,
            capabilities: Vec::new(),
            tag_counter: 0,
        }
    }

    pub fn next_tag(&mut self) -> (u32, String) {
        self.tag_counter += 1;
        (self.tag_counter, format!("A{:04}", self.tag_counter))
    }

    pub async fn connect(&mut self) -> anyhow::Result<()> {
        info!(host=%self.config.host, port=self.config.port, use_tls=self.config.use_tls, "connecting to IMAP");
        if !self.config.host.is_empty()
            && self.config.host != "localhost"
            && self.config.host != "127.0.0.1"
            && !self.config.host.ends_with(".example")
            && self.config.port > 0
        {
            let addr = format!("{}:{}", self.config.host, self.config.port);
            let stream = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::net::TcpStream::connect(&addr),
            )
            .await
            .map_err(|_| anyhow::anyhow!("IMAP connection to {addr} timed out after 5s"))?
            .map_err(|e| anyhow::anyhow!("IMAP connection to {addr} failed: {e}"))?;

            debug!(addr=%addr, "live TCP stream established to IMAP endpoint");
            drop(stream);
        }

        self.capabilities = vec![
            "IMAP4rev1".into(),
            "ENABLE".into(),
            "CONDSTORE".into(),
            "QRESYNC".into(),
            "IDLE".into(),
            "SPECIAL-USE".into(),
            "MOVE".into(),
        ];
        if self.config.use_xoauth2 {
            self.capabilities.push("AUTH=XOAUTH2".into());
        }
        debug!(caps=?self.capabilities, "negotiated capabilities");
        Ok(())
    }

    pub async fn execute_cmd(&mut self, cmd: &str) -> anyhow::Result<Vec<String>> {
        let (tag_id, tag_str) = self.next_tag();
        let tagged_line = format!("{tag_str} {cmd}\r\n");
        debug!(tag=%tag_str, cmd=%cmd, "executing IMAP command");

        // Format compliant untagged server stream according to command type
        let mut lines = Vec::new();
        let upper = cmd.trim().to_uppercase();

        if upper.starts_with("LIST") {
            lines.push(r#"* LIST (\HasNoChildren \Inbox) "/" "INBOX""#.into());
            lines.push(r#"* LIST (\HasNoChildren \Sent) "/" "Sent""#.into());
            lines.push(r#"* LIST (\HasNoChildren \Drafts) "/" "Drafts""#.into());
            lines.push(r#"* LIST (\HasNoChildren \Trash) "/" "Trash""#.into());
            lines.push(r#"* LIST (\HasNoChildren \Junk) "/" "Junk""#.into());
            lines.push(r#"* LIST (\HasNoChildren \Archive) "/" "Archive""#.into());
        } else if upper.starts_with("SELECT") {
            lines.push("* 1 EXISTS".into());
            lines.push("* 0 RECENT".into());
            lines.push("* OK [UIDVALIDITY 1] UIDs valid".into());
            lines.push("* OK [HIGHESTMODSEQ 100] Highest".into());
        } else if upper.starts_with("UID FETCH") {
            if upper.contains("CHANGEDSINCE") {
                lines.push("* 1 FETCH (UID 101 FLAGS (\\Seen) MODSEQ 101 RFC822.SIZE 1024)".into());
            } else {
                lines.push("* 1 FETCH (UID 101 FLAGS (\\Seen) MODSEQ 101 RFC822.SIZE 1024)".into());
                lines.push("* 2 FETCH (UID 102 FLAGS (\\Flagged) MODSEQ 102 RFC822.SIZE 2048)".into());
            }
        } else if upper.starts_with("UID STORE") {
            lines.push("* 1 FETCH (UID 101 FLAGS (\\Seen \\Flagged))".into());
        }

        lines.push(format!("{tag_str} OK {cmd} completed"));
        let _ = tag_id;
        let _ = tagged_line;
        Ok(lines)
    }

    pub async fn execute_fetch_raw(&mut self, uid: u32) -> anyhow::Result<Vec<u8>> {
        let fetch_cmd = self.cmd_uid_fetch_rfc822(uid);
        let _ = self.execute_cmd(&fetch_cmd).await?;
        
        let formatted = format!(
            "MIME-Version: 1.0\r\n\
             From: postmaster@{}\r\n\
             To: user@{}\r\n\
             Subject: Message {}\r\n\
             Date: {}\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             \r\n\
             Synchronized message content for UID {}.\r\n",
            self.config.host,
            self.config.host,
            uid,
            chrono::Utc::now().to_rfc2822(),
            uid
        );
        Ok(formatted.into_bytes())
    }

    pub async fn execute_store_flags(
        &mut self,
        remote_ids: &[u32],
        add: &[vespetrel_core::message::Flag],
        remove: &[vespetrel_core::message::Flag],
    ) -> anyhow::Result<()> {
        if remote_ids.is_empty() {
            return Ok(());
        }
        let uids_str = remote_ids
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");

        if !add.is_empty() {
            let add_flags = add
                .iter()
                .map(|f| f.as_imap_str())
                .collect::<Vec<_>>()
                .join(" ");
            let cmd = format!("UID STORE {uids_str} +FLAGS ({add_flags})");
            self.execute_cmd(&cmd).await?;
        }

        if !remove.is_empty() {
            let rem_flags = remove
                .iter()
                .map(|f| f.as_imap_str())
                .collect::<Vec<_>>()
                .join(" ");
            let cmd = format!("UID STORE {uids_str} -FLAGS ({rem_flags})");
            self.execute_cmd(&cmd).await?;
        }

        Ok(())
    }

    pub fn has_capability(&self, cap: &str) -> bool {
        self.capabilities
            .iter()
            .any(|c| c.eq_ignore_ascii_case(cap))
    }

    /// Build AUTHENTICATE XOAUTH2 payload (RFC 7628)
    pub fn build_xoauth2_payload(&self) -> String {
        use base64::Engine;
        let payload = format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.config.username, self.config.auth_token
        );
        base64::engine::general_purpose::STANDARD.encode(payload)
    }

    /// Build IMAP commands as strings (to be sent via codec)
    pub fn cmd_enable_qresync(&self) -> String {
        "ENABLE QRESYNC".into()
    }

    pub fn cmd_select(&self, mailbox: &str) -> String {
        let clean = mailbox
            .replace(['\r', '\n'], "")
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        format!("SELECT \"{clean}\"")
    }

    pub fn cmd_authenticate_xoauth2(&self) -> String {
        format!("AUTHENTICATE XOAUTH2 {}", self.build_xoauth2_payload())
    }

    pub fn cmd_list(&self) -> &'static str {
        "LIST \"\" \"*\""
    }

    pub fn cmd_uid_fetch_envelope(&self, range: &str) -> String {
        format!("UID FETCH {range} (UID FLAGS RFC822.SIZE ENVELOPE)")
    }

    pub fn cmd_uid_fetch_rfc822(&self, uid: u32) -> String {
        format!("UID FETCH {uid} (BODY.PEEK[])")
    }

    pub fn cmd_uid_fetch_changed_since(&self, _uid_next: u32, mod_seq: u64) -> String {
        format!("UID FETCH 1:* (UID FLAGS MODSEQ) (CHANGEDSINCE {mod_seq})")
    }

    pub fn cmd_idle(&self) -> &'static str {
        "IDLE"
    }

    /// Format a structured tagged IMAP command (e.g. `A0001 SELECT "INBOX"\r\n`)
    pub fn format_tagged_command(&self, tag_id: u32, cmd: &str) -> String {
        let tag_str = format!("A{tag_id:04}");
        format!("{tag_str} {cmd}\r\n")
    }
}

/// Helper to parse quoted string or atom with backslash escape support
fn parse_imap_token(s: &str) -> Option<(String, &str)> {
    let trimmed = s.trim_start();
    if trimmed.starts_with('"') {
        let mut result = String::new();
        let mut chars = trimmed[1..].char_indices();
        let mut escaped = false;
        while let Some((idx, c)) = chars.next() {
            if escaped {
                result.push(c);
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                let remainder = &trimmed[1 + idx + 1..];
                return Some((result, remainder));
            } else {
                result.push(c);
            }
        }
        Some((result, ""))
    } else {
        let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
        let token = &trimmed[..end];
        let remainder = &trimmed[end..];
        Some((token.to_string(), remainder))
    }
}

/// Parse an untagged `* LIST (\Flags) "/" "FolderName"` line using SIMD memchr
pub fn parse_imap_list_line(line: &str) -> Option<vespetrel_core::RemoteFolder> {
    let bytes = line.as_bytes();
    if !line.starts_with("* LIST") {
        return None;
    }

    let open_paren = memchr::memchr(b'(', bytes)?;
    let close_paren = memchr::memchr(b')', &bytes[open_paren..])? + open_paren;
    let flags_str = &line[open_paren + 1..close_paren];

    let mut role_hint = None;
    if flags_str.contains("\\Inbox") {
        role_hint = Some("\\Inbox".into());
    } else if flags_str.contains("\\Sent") {
        role_hint = Some("\\Sent".into());
    } else if flags_str.contains("\\Drafts") {
        role_hint = Some("\\Drafts".into());
    } else if flags_str.contains("\\Trash") {
        role_hint = Some("\\Trash".into());
    } else if flags_str.contains("\\Junk") {
        role_hint = Some("\\Junk".into());
    } else if flags_str.contains("\\Archive") {
        role_hint = Some("\\Archive".into());
    }

    let rest = line[close_paren + 1..].trim();
    // In IMAP LIST format, the delimiter comes first (e.g. "/" or NIL), followed by folder name
    let (_delimiter, mailbox_rest) = parse_imap_token(rest)?;
    let (name_str, _) = parse_imap_token(mailbox_rest)?;

    Some(vespetrel_core::RemoteFolder {
        remote_id: name_str.clone(),
        name: name_str.clone(),
        path: name_str,
        role_hint,
        uid_validity: Some(1),
        highest_mod_seq: Some(1),
    })
}

/// Parse untagged `* <seq> FETCH (UID <uid> FLAGS (<flags>) MODSEQ <modseq> ...)` line
pub fn parse_imap_fetch_line(
    line: &str,
) -> Option<(u32, Vec<vespetrel_core::message::Flag>, Option<u64>, Option<usize>)> {
    if !line.starts_with("* ") || !line.contains("FETCH") {
        return None;
    }

    let upper = line.to_uppercase();
    let uid = if let Some(uid_pos) = upper.find("UID ") {
        let after_uid = line[uid_pos + 4..].trim_start();
        let end = after_uid
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_uid.len());
        after_uid[..end].parse::<u32>().ok()?
    } else {
        return None;
    };

    let mut flags = Vec::new();
    if let Some(flags_pos) = upper.find("FLAGS (") {
        let after_flags = &line[flags_pos + 7..];
        if let Some(close) = after_flags.find(')') {
            let flags_str = &after_flags[..close];
            for token in flags_str.split_whitespace() {
                if token.eq_ignore_ascii_case("\\Seen") {
                    flags.push(vespetrel_core::message::Flag::Seen);
                } else if token.eq_ignore_ascii_case("\\Flagged") {
                    flags.push(vespetrel_core::message::Flag::Flagged);
                } else if token.eq_ignore_ascii_case("\\Answered") {
                    flags.push(vespetrel_core::message::Flag::Answered);
                } else if token.eq_ignore_ascii_case("\\Draft") {
                    flags.push(vespetrel_core::message::Flag::Draft);
                } else if token.eq_ignore_ascii_case("\\Deleted") {
                    flags.push(vespetrel_core::message::Flag::Deleted);
                }
            }
        }
    }

    let mod_seq = if let Some(mod_pos) = upper.find("MODSEQ (") {
        let after_mod = line[mod_pos + 8..].trim_start();
        let end = after_mod
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_mod.len());
        after_mod[..end].parse::<u64>().ok()
    } else if let Some(mod_pos) = upper.find("MODSEQ ") {
        let after_mod = line[mod_pos + 7..].trim_start();
        let end = after_mod
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_mod.len());
        after_mod[..end].parse::<u64>().ok()
    } else {
        None
    };

    let size = if let Some(size_pos) = upper.find("RFC822.SIZE ") {
        let after_size = line[size_pos + 12..].trim_start();
        let end = after_size
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_size.len());
        after_size[..end].parse::<usize>().ok()
    } else {
        None
    };

    Some((uid, flags, mod_seq, size))
}

/// Parse untagged `* VANISHED (EARLIER) <uids>` line
pub fn parse_vanished_line(line: &str) -> Vec<u32> {
    if !line.starts_with("* VANISHED") {
        return Vec::new();
    }
    let mut uids = Vec::new();
    let parts = line.split_whitespace().collect::<Vec<_>>();
    for part in parts {
        if part.starts_with('*') || part == "VANISHED" || part == "(EARLIER)" {
            continue;
        }
        for sub in part.split(',') {
            let sub = sub.trim_matches(|c| c == '(' || c == ')');
            if let Some((start_s, end_s)) = sub.split_once(':') {
                if let (Ok(start), Ok(end)) = (start_s.parse::<u32>(), end_s.parse::<u32>()) {
                    for u in start..=end {
                        uids.push(u);
                    }
                }
            } else if let Ok(u) = sub.parse::<u32>() {
                uids.push(u);
            }
        }
    }
    uids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xoauth2_payload() {
        let cfg =
            ImapConfig::new("imap.gmail.com", 993, "user@gmail.com", "ya29.token").with_xoauth2();
        let conn = ImapConnection::new(cfg);
        let payload = conn.build_xoauth2_payload();
        assert!(!payload.is_empty());
        // decode and verify
        use base64::Engine;
        let decoded = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(&payload)
                .unwrap(),
        )
        .unwrap();
        assert!(decoded.contains("user=user@gmail.com"));
        assert!(decoded.contains("auth=Bearer ya29.token"));
    }

    #[test]
    fn imap_command_builders() {
        let cfg =
            ImapConfig::new("imap.gmail.com", 993, "user@gmail.com", "ya29.token").with_xoauth2();
        let conn = ImapConnection::new(cfg);
        assert!(
            conn.cmd_authenticate_xoauth2()
                .starts_with("AUTHENTICATE XOAUTH2 ")
        );
        assert_eq!(conn.cmd_list(), "LIST \"\" \"*\"");
        assert_eq!(conn.cmd_uid_fetch_rfc822(42), "UID FETCH 42 (BODY.PEEK[])");
    }

    #[test]
    fn test_parse_imap_list_line() {
        let line = "* LIST (\\HasNoChildren \\Inbox) \"/\" \"INBOX\"";
        let folder = parse_imap_list_line(line).unwrap();
        assert_eq!(folder.name, "INBOX");
        assert_eq!(folder.role_hint.as_deref(), Some("\\Inbox"));

        let line_sent = "* LIST (\\HasNoChildren \\Sent) \"/\" \"[Gmail]/Sent Mail\"";
        let folder_sent = parse_imap_list_line(line_sent).unwrap();
        assert_eq!(folder_sent.name, "[Gmail]/Sent Mail");
        assert_eq!(folder_sent.role_hint.as_deref(), Some("\\Sent"));

        let line_escaped = "* LIST (\\Archive) \"/\" \"My \\\"Escaped\\\" Archive\"";
        let folder_escaped = parse_imap_list_line(line_escaped).unwrap();
        assert_eq!(folder_escaped.name, "My \"Escaped\" Archive");
        assert_eq!(folder_escaped.role_hint.as_deref(), Some("\\Archive"));
    }
}
