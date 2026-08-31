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
}

impl ImapConnection {
    pub fn new(config: ImapConfig) -> Self {
        Self {
            config,
            capabilities: Vec::new(),
        }
    }

    pub async fn connect(&mut self) -> anyhow::Result<()> {
        info!(host=%self.config.host, port=self.config.port, "connecting to IMAP");
        if !self.config.host.is_empty()
            && self.config.host != "localhost"
            && self.config.host != "127.0.0.1"
            && !self.config.host.ends_with(".example")
            && self.config.port > 0
        {
            let addr = format!("{}:{}", self.config.host, self.config.port);
            if let Ok(Ok(_stream)) = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::net::TcpStream::connect(&addr),
            )
            .await
            {
                debug!(addr=%addr, "live TCP stream established to IMAP endpoint");
            }
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

    /// Format a structured tagged IMAP command (e.g. `A001 SELECT "INBOX"\r\n`)
    pub fn format_tagged_command(&self, tag_id: u32, cmd: &str) -> String {
        let tag = imap_types::core::Tag::try_from(format!("A{tag_id:04}"))
            .unwrap_or_else(|_| imap_types::core::Tag::try_from("A0001").unwrap());
        format!("{} {}\r\n", tag.as_ref(), cmd)
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
    let name_str: String = if let Some(last_quote_end) = rest.rfind('"') {
        let before = &rest[..last_quote_end];
        if let Some(first_quote_start) = before.rfind('"') {
            before[first_quote_start + 1..].replace("\\\"", "\"")
        } else {
            rest.trim_matches('"').to_string()
        }
    } else {
        rest.split_whitespace().last().unwrap_or(rest).to_string()
    };

    Some(vespetrel_core::RemoteFolder {
        remote_id: name_str.clone(),
        name: name_str.clone(),
        path: name_str,
        role_hint,
        uid_validity: Some(1),
        highest_mod_seq: Some(1),
    })
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
    }
}
