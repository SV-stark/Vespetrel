use tracing::{debug, info};

#[derive(Clone)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
    pub username: String,
    /// Password or OAuth2 access token (XOAUTH2)
    pub auth_token: String,
    pub use_xoauth2: bool,
    /// Mailbox to SELECT before starting IDLE.
    ///
    /// `IDLE` is only valid in the SELECTED state (RFC 2177 §3), so a live-push connection
    /// must name the mailbox it is watching.
    pub mailbox: String,
    /// Cached UIDVALIDITY for `mailbox`, used for the QRESYNC select parameters.
    pub uid_validity: Option<u32>,
    /// Cached HIGHESTMODSEQ for `mailbox`, used for the QRESYNC select parameters.
    pub highest_mod_seq: u64,
}

impl std::fmt::Debug for ImapConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImapConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("use_tls", &self.use_tls)
            .field("username", &self.username)
            .field("auth_token", &"[REDACTED]")
            .field("use_xoauth2", &self.use_xoauth2)
            .field("mailbox", &self.mailbox)
            .field("uid_validity", &self.uid_validity)
            .field("highest_mod_seq", &self.highest_mod_seq)
            .finish()
    }
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
            mailbox: "INBOX".into(),
            uid_validity: None,
            highest_mod_seq: 0,
        }
    }

    pub fn with_xoauth2(mut self) -> Self {
        self.use_xoauth2 = true;
        self
    }

    /// Set the mailbox to watch for live pushes.
    pub fn with_mailbox(mut self, mailbox: impl Into<String>) -> Self {
        self.mailbox = mailbox.into();
        self
    }

    /// Set the cached sync cursor used to build a QRESYNC select.
    pub fn with_cursor(mut self, uid_validity: Option<u32>, highest_mod_seq: u64) -> Self {
        self.uid_validity = uid_validity;
        self.highest_mod_seq = highest_mod_seq;
        self
    }
}
/// Supported transport streams for IMAP client (plain TCP or TLS)
#[allow(clippy::large_enum_variant)]
pub enum ImapStream {
    Plain(tokio::net::TcpStream),
    Tls(tokio_rustls::client::TlsStream<tokio::net::TcpStream>),
    /// In-memory duplex pipe, used to drive the protocol state machine in tests without a
    /// real server. Never constructed in production paths.
    #[cfg(test)]
    Test(tokio::io::DuplexStream),
}

impl ImapStream {
    pub async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use tokio::io::AsyncReadExt;
        match self {
            Self::Plain(s) => s.read(buf).await,
            Self::Tls(s) => s.read(buf).await,
            #[cfg(test)]
            Self::Test(s) => s.read(buf).await,
        }
    }

    pub async fn write_all(&mut self, src: &[u8]) -> std::io::Result<()> {
        use tokio::io::AsyncWriteExt;
        match self {
            Self::Plain(s) => s.write_all(src).await,
            Self::Tls(s) => s.write_all(src).await,
            #[cfg(test)]
            Self::Test(s) => s.write_all(src).await,
        }
    }

    pub async fn flush(&mut self) -> std::io::Result<()> {
        use tokio::io::AsyncWriteExt;
        match self {
            Self::Plain(s) => s.flush().await,
            Self::Tls(s) => s.flush().await,
            #[cfg(test)]
            Self::Test(s) => s.flush().await,
        }
    }
}

/// Async IMAP connection wrapper - negotiates capabilities, performs TLS/AUTH, and handles commands
pub struct ImapConnection {
    config: ImapConfig,
    pub capabilities: Vec<String>,
    pub tag_counter: u32,
    pub stream: Option<ImapStream>,
    /// Tag used by the most recent `execute_cmd`.
    ///
    /// Needed because the tagged completion line can only be matched by prefix, and
    /// `execute_cmd` allocates the tag internally.
    pub last_tag: String,
}

impl Drop for ImapConnection {
    fn drop(&mut self) {
        if self.stream.is_some() {
            debug!("closing IMAP connection stream via RAII drop");
            self.stream = None;
        }
    }
}

/// Byte offset just past a complete `{<digits>[+][~]}\r\n` literal marker, plus the declared
/// length.
///
/// Scans for the **last** such marker whose declared body is not yet fully buffered, i.e. the
/// literal currently being received. Returning the first marker (as the previous code did)
/// is wrong for any response carrying more than one literal — `UID FETCH 1:*` routinely does
/// — and the reader then waited on an already-satisfied literal and returned early.
///
/// Works on raw bytes only. The old code computed the offset from a
/// `String::from_utf8_lossy` copy and then subtracted it from the *raw* buffer length;
/// `from_utf8_lossy` expands each invalid byte to a 3-byte U+FFFD, so any 8-bit data before
/// the marker made `lit_start` overshoot and the reader consumed more bytes than the server
/// declared, swallowing the start of the next response.
fn literal_marker(buf: &[u8]) -> Option<(usize, usize)> {
    let mut found = None;
    let mut search = 0usize;
    while let Some(rel) = memchr::memchr(b'{', &buf[search..]) {
        let open = search + rel;
        search = open + 1;
        let Some(close_rel) = memchr::memchr(b'}', &buf[open + 1..]) else {
            continue;
        };
        let close = open + 1 + close_rel;
        // Must be followed by CRLF for this to be a literal marker.
        if buf.get(close + 1..close + 3) != Some(b"\r\n") {
            continue;
        }
        // The body must be digits only, optionally followed by the `+` (literal+) and `~`
        // (non-synchronizing) markers.
        let body = &buf[open + 1..close];
        let digits: &[u8] = match body.iter().position(|b| !b.is_ascii_digit()) {
            Some(i) => {
                let rest = &body[i..];
                // Only `+` / `~` markers may trail the digits.
                if !rest.iter().all(|b| matches!(b, b'+' | b'~')) {
                    continue;
                }
                &body[..i]
            }
            None => body,
        };
        if digits.is_empty() || digits.len() > 19 {
            continue;
        }
        let Ok(n) = std::str::from_utf8(digits).unwrap_or("").parse::<usize>() else {
            continue;
        };
        let start = close + 3;
        // Only the trailing, still-incomplete literal is pending.
        if buf.len().saturating_sub(start) < n {
            found = Some((start, n));
        }
    }
    found
}

/// Byte offset of the literal body currently being read, if a marker is present.
fn literal_start(buf: &[u8]) -> Option<usize> {
    literal_marker(buf).map(|(start, _)| start)
}

/// Declared length of a literal that has not yet been fully read.
fn pending_literal(buf: &[u8]) -> Option<usize> {
    let (_, n) = literal_marker(buf)?;
    if n > ImapConnection::MAX_IMAP_RESPONSE {
        return None;
    }
    Some(n)
}

/// Has the server finished the response for `tag`?
///
/// The tagged response must be the **last** complete line. A substring search returned as
/// soon as a message body happened to contain `<tag> OK`, dropping every remaining untagged
/// response and permanently desynchronizing the stream.
fn response_is_complete(buf: &[u8], tag: &str) -> bool {
    let text = String::from_utf8_lossy(buf);
    // Ignore a trailing partial line.
    let body = match text.rfind('\n') {
        Some(i) => &text[..i + 1],
        None => return false,
    };
    let last = match body.lines().rfind(|l| !l.trim().is_empty()) {
        Some(l) => l,
        None => return false,
    };
    // `<tag> OKAY` is not `<tag> OK`: require a delimiter after the status word.
    let is_tagged = |status: &str| {
        last.strip_prefix(tag)
            .and_then(|rest| rest.strip_prefix(' '))
            .is_some_and(|rest| {
                rest.strip_prefix(status)
                    .is_some_and(|tail| tail.is_empty() || tail.starts_with(' '))
            })
    };
    is_tagged("OK") || is_tagged("NO") || is_tagged("BAD") || last.starts_with('+')
}

/// Extract the first fully-buffered FETCH literal payload from a raw response.
fn extract_first_literal(buf: &[u8]) -> Option<Vec<u8>> {
    let mut search = 0usize;
    while let Some(rel) = memchr::memchr(b'{', &buf[search..]) {
        let open = search + rel;
        search = open + 1;
        let Some(close_rel) = memchr::memchr(b'}', &buf[open + 1..]) else {
            continue;
        };
        let close = open + 1 + close_rel;
        if buf.get(close + 1..close + 3) != Some(b"\r\n") {
            continue;
        }
        let body = &buf[open + 1..close];
        let digits: &[u8] = match body.iter().position(|b| !b.is_ascii_digit()) {
            Some(i) if body[i..].iter().all(|b| matches!(b, b'+' | b'~')) => &body[..i],
            Some(_) => continue,
            None => body,
        };
        if digits.is_empty() || digits.len() > 19 {
            continue;
        }
        let Ok(n) = std::str::from_utf8(digits).unwrap_or("").parse::<usize>() else {
            continue;
        };
        let start = close + 3;
        let end = start.checked_add(n)?;
        if end <= buf.len() {
            return Some(buf[start..end].to_vec());
        }
    }
    None
}

/// How an XOAUTH2 failure should be handled by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Xoauth2Failure {
    /// The credential is bad; the user must re-authorize.
    NeedsReauth,
    /// A backend hiccup; a reconnect is likely to succeed.
    Transient,
    /// Anything else, including malformed challenges.
    Rejected,
}

/// An `AUTHENTICATE XOAUTH2` failure, carrying enough classification for the sync engine to
/// choose between backoff and re-consent.
#[derive(Debug, thiserror::Error)]
pub enum Xoauth2Error {
    #[error("{0}")]
    NeedsReauth(String),
    #[error("{0}")]
    Transient(String),
    #[error("{0}")]
    Rejected(String),
    #[error("XOAUTH2 transport failure: {0}")]
    Transport(String),
}

impl Xoauth2Error {
    /// Does this failure mean the user must re-authorize?
    pub fn needs_reauth(&self) -> bool {
        matches!(self, Self::NeedsReauth(_))
    }

    /// Is this failure worth retrying without user action?
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Transient(_))
    }
}

/// Read the next SASL line that advances an `AUTHENTICATE` exchange: a continuation
/// request (`+`) or a tagged response.
///
/// [`read_imap_line`] waits for a single expected prefix, but the SASL state machine must react
/// to whichever of the two arrives first.
async fn read_sasl_line(
    stream: &mut ImapStream,
    tag: &str,
    timeout_secs: u64,
) -> anyhow::Result<String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let mut acc: Vec<u8> = Vec::with_capacity(512);
    let mut buf = vec![0u8; 4096];

    loop {
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("timed out after {timeout_secs}s waiting for SASL response");
        }
        let n = match tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut buf))
            .await
        {
            Ok(Ok(0)) => anyhow::bail!("IMAP server closed the connection during AUTHENTICATE"),
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => continue,
        };
        acc.extend_from_slice(&buf[..n]);

        let text = String::from_utf8_lossy(&acc).to_string();
        if let Some(line) = text
            .lines()
            .find(|l| l.starts_with('+') || l.starts_with(tag))
        {
            return Ok(line.trim_end().to_string());
        }
        if acc.len() > ImapConnection::MAX_IMAP_RESPONSE {
            anyhow::bail!("IMAP response exceeded 16MB limit: potential DoS");
        }
    }
}

/// Read IMAP lines until one starts with `expect` (a continuation `+` or a tag).
///
/// Bounded by `timeout_secs` and treats a closed connection as an error.
async fn read_imap_line(
    stream: &mut ImapStream,
    expect: &str,
    timeout_secs: u64,
) -> anyhow::Result<String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let mut acc: Vec<u8> = Vec::with_capacity(512);
    let mut buf = vec![0u8; 4096];

    loop {
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("timed out after {timeout_secs}s waiting for IMAP response");
        }
        let n = match tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut buf))
            .await
        {
            Ok(Ok(0)) => anyhow::bail!("IMAP server closed the connection unexpectedly"),
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => continue,
        };
        acc.extend_from_slice(&buf[..n]);

        let text = String::from_utf8_lossy(&acc).to_string();
        if let Some(line) = text.lines().find(|l| l.starts_with(expect)) {
            return Ok(line.trim_end().to_string());
        }
        if acc.len() > ImapConnection::MAX_IMAP_RESPONSE {
            anyhow::bail!("IMAP response exceeded 16MB limit: potential DoS");
        }
    }
}

impl ImapConnection {
    pub fn new(config: ImapConfig) -> Self {
        Self {
            config,
            capabilities: Vec::new(),
            tag_counter: 0,
            stream: None,
            last_tag: String::new(),
        }
    }

    /// Is this connection a simulated one?
    ///
    /// Decided by the configured host **only**. The previous implementation also returned
    /// `true` whenever the `mock`/`test` feature was compiled in, so a mock-enabled build
    /// never opened a socket to a real server and silently ingested fabricated folders and
    /// messages (UIDs 101/102, no bodies) into the user's real database.
    pub fn is_mock(&self) -> bool {
        self.config.host.is_empty()
            || self.config.host == "localhost"
            || self.config.host == "127.0.0.1"
            || self.config.host == "::1"
            || self.config.host == "example.com"
            || self.config.host == "imap.example.com"
            || self.config.host.ends_with(".example")
            || self.config.host.ends_with(".example.com")
            || self.config.host.ends_with(".invalid")
            || self.config.host.ends_with(".test")
            || self.config.host.ends_with(".localhost")
    }

    pub fn next_tag(&mut self) -> (u32, String) {
        self.tag_counter += 1;
        (self.tag_counter, format!("A{:04}", self.tag_counter))
    }

    pub async fn connect(&mut self) -> anyhow::Result<()> {
        info!(host=%self.config.host, port=self.config.port, use_tls=self.config.use_tls, "connecting to IMAP");

        let is_mock = self.is_mock();
        let try_live =
            !is_mock || self.config.host == "localhost" || self.config.host == "127.0.0.1";

        if try_live && !self.config.host.is_empty() && self.config.port > 0 {
            let addr = format!("{}:{}", self.config.host, self.config.port);
            let timeout_dur = if is_mock {
                std::time::Duration::from_millis(500)
            } else {
                std::time::Duration::from_secs(10)
            };

            match tokio::time::timeout(timeout_dur, tokio::net::TcpStream::connect(&addr)).await {
                Ok(Ok(tcp_stream)) => {
                    let mut stream = if self.config.use_tls || self.config.port == 993 {
                        debug!(addr=%addr, "performing TLS handshake for IMAP connection");
                        let mut root_store = rustls::RootCertStore::empty();
                        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                        let client_config = rustls::ClientConfig::builder()
                            .with_root_certificates(root_store)
                            .with_no_client_auth();
                        let connector =
                            tokio_rustls::TlsConnector::from(std::sync::Arc::new(client_config));
                        let server_name =
                            rustls::pki_types::ServerName::try_from(self.config.host.clone())
                                .map_err(|e| anyhow::anyhow!("invalid TLS server name: {e}"))?;
                        let tls_stream =
                            connector
                                .connect(server_name, tcp_stream)
                                .await
                                .map_err(|e| {
                                    anyhow::anyhow!("TLS handshake failed with {addr}: {e}")
                                })?;
                        ImapStream::Tls(tls_stream)
                    } else {
                        ImapStream::Plain(tcp_stream)
                    };

                    let mut banner_buf = vec![0u8; 4096];
                    if let Ok(Ok(n)) = tokio::time::timeout(
                        std::time::Duration::from_secs(3),
                        stream.read(&mut banner_buf),
                    )
                    .await
                    {
                        let banner = String::from_utf8_lossy(&banner_buf[..n]);
                        debug!(banner=%banner.trim(), "received IMAP server greeting");
                    }
                    self.stream = Some(stream);

                    // Query server capabilities dynamically
                    if let Ok(cap_lines) = self.execute_cmd("CAPABILITY").await {
                        for line in cap_lines {
                            if let Some(rest) = line.strip_prefix("* CAPABILITY") {
                                self.capabilities =
                                    rest.split_whitespace().map(|s| s.to_string()).collect();
                            }
                        }
                    }

                    // Perform authentication if credentials are provided and propagate error
                    if self.config.use_xoauth2 && !self.config.auth_token.is_empty() {
                        // Runs the RFC 7628 exchange explicitly. The previous single-shot
                        // `AUTHENTICATE XOAUTH2 <payload>` mis-read a legitimate continuation
                        // as rejection and discarded the server's stated reason.
                        self.authenticate_xoauth2().await.map_err(|e| match &e {
                            Xoauth2Error::Transport(m) => anyhow::anyhow!("{m}"),
                            other => anyhow::anyhow!("{other}"),
                        })?;
                    } else if !self.config.username.is_empty() && !self.config.auth_token.is_empty()
                    {
                        let login_cmd = self.cmd_login();
                        let login_resp = self.execute_cmd(&login_cmd).await?;
                        if let Some(err_line) = login_resp
                            .iter()
                            .find(|l| l.contains(" NO ") || l.contains(" BAD "))
                        {
                            anyhow::bail!(
                                "IMAP LOGIN authentication failed for user {}: {}",
                                self.config.username,
                                err_line.trim()
                            );
                        }
                    }

                    // Enable QRESYNC extension dynamically if server advertises support (RFC 5161 / RFC 7162)
                    if self.has_capability("ENABLE") && self.has_capability("QRESYNC") {
                        let enable_cmd = self.cmd_enable_qresync();
                        let _ = self.execute_cmd(&enable_cmd).await;
                    }
                }
                Ok(Err(e)) => {
                    if is_mock {
                        debug!(addr=%addr, error=%e, "failed to connect to local mock endpoint, using simulated IMAP");
                    } else {
                        anyhow::bail!("Failed to connect to IMAP server {addr}: {e}");
                    }
                }
                Err(_) => {
                    if is_mock {
                        debug!(addr=%addr, "connect to local mock endpoint timed out, using simulated IMAP");
                    } else {
                        anyhow::bail!(
                            "Connection to IMAP server {addr} timed out after 10 seconds"
                        );
                    }
                }
            }
        }

        if self.capabilities.is_empty() {
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
        }
        debug!(caps=?self.capabilities, "negotiated capabilities");
        Ok(())
    }

    pub const MAX_IMAP_RESPONSE: usize = 16 * 1024 * 1024;

    pub async fn execute_cmd(&mut self, cmd: &str) -> anyhow::Result<Vec<String>> {
        let raw = self.execute_cmd_raw(cmd).await?;
        Ok(String::from_utf8_lossy(&raw)
            .lines()
            .map(|l| l.trim_end_matches(['\r', '\n']).to_string())
            .filter(|l| !l.is_empty())
            .collect())
    }

    /// Execute a command and return the **raw response bytes**.
    ///
    /// This is the primitive all other helpers must use. The previous `execute_cmd` split
    /// the response into trimmed, non-empty `String` lines, which destroys a FETCH literal in
    /// two ways: the blank line separating headers from the body is discarded (so every
    /// fetched message arrived with an empty body), and leading/trailing whitespace is
    /// stripped from every body line (breaking `format=flowed` space-stuffing and
    /// quoted-printable trailing whitespace).
    ///
    /// Completion is detected only when the *last* complete line is the tagged response. A
    /// substring search returned early whenever the message body happened to contain the
    /// bytes `<tag> OK`, desynchronizing the connection permanently.
    ///
    /// A timeout is an error, never a partial success. Returning a truncated response made
    /// `sync.rs` persist a `HIGHESTMODSEQ` cursor past messages it had never received, so
    /// they were never re-delivered.
    pub async fn execute_cmd_raw(&mut self, cmd: &str) -> anyhow::Result<Vec<u8>> {
        let (tag_id, tag_str) = self.next_tag();
        self.last_tag = tag_str.clone();
        let tagged_line = format!("{tag_str} {cmd}\r\n");
        // Log the verb only. `cmd` carries the LOGIN password or a base64 XOAUTH2 blob that
        // decodes straight back to the OAuth access token.
        let verb = cmd.split_whitespace().next().unwrap_or("?");
        debug!(tag=%tag_str, verb=%verb, "executing IMAP command");

        if let Some(stream) = self.stream.as_mut() {
            if stream.write_all(tagged_line.as_bytes()).await.is_err() {
                anyhow::bail!("failed to write IMAP command to stream");
            }
            let _ = stream.flush().await;
            let mut accumulated: Vec<u8> = Vec::new();
            let mut chunk = vec![0u8; 8192];
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);

            loop {
                let read_timeout = std::time::Duration::from_millis(500);
                let n = match tokio::time::timeout(read_timeout, stream.read(&mut chunk)).await {
                    Ok(Ok(0)) => {
                        anyhow::bail!("IMAP server closed the connection during {tag_str} {verb}");
                    }
                    Ok(Ok(n)) => n,
                    Ok(Err(e)) => return Err(e.into()),
                    Err(_) => {
                        if std::time::Instant::now() >= deadline {
                            break;
                        }
                        continue;
                    }
                };

                if accumulated.len() + n > Self::MAX_IMAP_RESPONSE {
                    anyhow::bail!("IMAP response exceeded 16MB limit: potential DoS");
                }
                accumulated.extend_from_slice(&chunk[..n]);

                // Drain any declared literals (RFC 3501 `{N}`, RFC 7888 non-synchronizing
                // `{N+}`, LITERAL+ `{N+}`). Counting every pending literal matters for
                // `UID FETCH 1:*`, where a single response declares many.
                while let Some(expected) = pending_literal(&accumulated) {
                    let lit_start = literal_start(&accumulated).expect("literal present");
                    let lit_read = accumulated.len().saturating_sub(lit_start);
                    if lit_read >= expected {
                        break;
                    }
                    let remaining = expected - lit_read;
                    let mut lit_buf = vec![0u8; remaining.min(65536)];
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        stream.read(&mut lit_buf),
                    )
                    .await
                    {
                        Ok(Ok(0)) => {
                            anyhow::bail!("IMAP server closed mid-literal during {tag_str} {verb}")
                        }
                        Ok(Ok(rn)) => accumulated.extend_from_slice(&lit_buf[..rn]),
                        Ok(Err(e)) => return Err(e.into()),
                        Err(_) => {
                            anyhow::bail!("timed out reading IMAP literal for {tag_str} {verb}")
                        }
                    }
                }

                if response_is_complete(&accumulated, &tag_str) {
                    return Ok(accumulated);
                }

                if std::time::Instant::now() >= deadline {
                    break;
                }
            }

            // Truncation must be an error: a partial response would let the caller advance a
            // sync cursor over messages it never received.
            anyhow::bail!(
                "IMAP command {tag_str} {verb} timed out after 30s ({} bytes received, response incomplete)",
                accumulated.len()
            );
        }

        let _ = tag_id;
        if self.is_mock() {
            return Ok(self.mock_response(cmd).join("\r\n").into_bytes());
        }

        anyhow::bail!(
            "IMAP connection to {}:{} is not established",
            self.config.host,
            self.config.port
        );
    }

    /// Synthetic response used when no real connection is available.
    ///
    /// Gated on `is_mock()`, which keys off the configured host only. The previous code
    /// folded `cfg!(feature = "mock")` into `is_mock()`, so a mock-enabled build served
    /// fabricated folders and messages for *any* real account.
    fn mock_response(&self, cmd: &str) -> Vec<String> {
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
            lines.push("* 1 FETCH (UID 101 FLAGS (\\Seen) MODSEQ 101 RFC822.SIZE 1024)".into());
            if !upper.contains("CHANGEDSINCE") {
                lines.push(
                    "* 2 FETCH (UID 102 FLAGS (\\Flagged) MODSEQ 102 RFC822.SIZE 2048)".into(),
                );
            }
        } else if upper.starts_with("UID STORE") {
            lines.push("* 1 FETCH (UID 101 FLAGS (\\Seen \\Flagged))".into());
        }

        lines
    }

    pub async fn logout(&mut self) -> anyhow::Result<()> {
        if self.stream.is_some() {
            let logout_cmd = self.cmd_logout();
            let _ = self.execute_cmd(logout_cmd).await;
            self.stream = None;
        }
        Ok(())
    }

    pub async fn execute_append(
        &mut self,
        mailbox: &str,
        flags: &[vespetrel_core::Flag],
        raw_mime: &[u8],
    ) -> anyhow::Result<()> {
        let cmd = self.cmd_append(mailbox, flags, raw_mime.len());
        let (_tag_id, tag_str) = self.next_tag();
        let cmd_str = format!("{tag_str} {cmd}\r\n");

        if let Some(stream) = &mut self.stream {
            stream.write_all(cmd_str.as_bytes()).await?;

            // Every read is bounded. The previous version had no timeouts, so a server that
            // accepted APPEND and stopped answering wedged the send task forever.
            let cont = read_imap_line(stream, "+", 15).await?;
            if !cont.starts_with('+') {
                anyhow::bail!("server rejected IMAP APPEND literal continuation: {cont}");
            }

            stream.write_all(raw_mime).await?;
            stream.write_all(b"\r\n").await?;
            stream.flush().await?;

            // EOF is an error, not success. Treating a zero-byte read as "no NO/BAD seen"
            // reported a successful send and left the response in the socket, so the *next*
            // APPEND on the same connection consumed the stale reply and also "succeeded".
            let done = read_imap_line(stream, &tag_str, 15).await?;
            if done.contains(" NO ") || done.contains(" BAD ") {
                anyhow::bail!("IMAP APPEND failed: {done}");
            }
            return Ok(());
        }

        if self.is_mock() {
            let _ = _tag_id;
            return Ok(());
        }

        anyhow::bail!(
            "IMAP connection to {}:{} is not connected for APPEND",
            self.config.host,
            self.config.port
        );
    }

    pub async fn execute_fetch_raw(&mut self, uid: u32) -> anyhow::Result<Vec<u8>> {
        let fetch_cmd = self.cmd_uid_fetch_rfc822(uid);
        let raw = self.execute_cmd_raw(&fetch_cmd).await?;

        // Slice the literal by its declared byte count. Reassembling the message from
        // trimmed, blank-line-filtered text lost the header/body separator, so every
        // fetched message parsed as one long malformed header with no body at all.
        if let Some(payload) = extract_first_literal(&raw)
            && !payload.is_empty()
        {
            return Ok(payload);
        }

        #[cfg(test)]
        if self.is_mock() {
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
            return Ok(formatted.into_bytes());
        }

        anyhow::bail!("no literal MIME payload returned from IMAP server for UID {uid}");
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

    /// Decode a SASL challenge body into the server's JSON error, when it carries one.
    ///
    /// Gmail and Outlook answer a failed XOAUTH2 with `+ <base64 JSON>`, e.g.
    /// `{"status":"4.7.0","schemes":"Bearer","scope":"..."}`. Returning that text is the
    /// difference between "token expired" and a base64 blob the user cannot act on.
    fn decode_xoauth2_challenge(challenge_line: &str) -> String {
        use base64::Engine;
        let body = challenge_line.trim_start_matches('+').trim();
        if body.is_empty() {
            return String::new();
        }
        base64::engine::general_purpose::STANDARD
            .decode(body)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .filter(|decoded| decoded.contains('{'))
            .unwrap_or_else(|| body.to_string())
    }

    /// Classify an XOAUTH2 failure as a credential problem or a transient one.
    ///
    /// Exchange Online sometimes accepts the OAuth identity but fails to attach the resulting
    /// session to the mailbox, answering `"User is authenticated but not connected"`. That is a
    /// backend hiccup, not evidence the refresh token was revoked: reporting it as an auth
    /// failure sends the user to re-consent for a problem that a reconnect fixes. Callers use
    /// this to decide between backoff and re-authentication.
    pub fn classify_xoauth2_failure(detail: &str) -> Xoauth2Failure {
        let lower = detail.to_ascii_lowercase();

        // Checked first: Exchange Online's mailbox-attach hiccup is reported with the same
        // SASL status as a genuine rejection, so the status alone cannot be trusted.
        if lower.contains("user is authenticated but not connected") {
            return Xoauth2Failure::Transient;
        }

        // A `Bearer` scheme challenge is the server asking to re-authenticate over OAuth.
        if lower.contains("\"schemes\"") && lower.contains("bearer") {
            return Xoauth2Failure::NeedsReauth;
        }

        // SASL status 4.7.x is "authentication failure" (RFC 4422 §3.1), which for XOAUTH2
        // means the access token is no longer usable.
        if lower.contains("\"4.7.")
            || lower.contains("invalid_grant")
            || lower.contains("token expired")
        {
            return Xoauth2Failure::NeedsReauth;
        }

        Xoauth2Failure::Rejected
    }

    /// Run the RFC 7628 XOAUTH2 exchange to completion.
    ///
    /// The previous implementation sent `AUTHENTICATE XOAUTH2 <payload>` in one shot and
    /// treated *any* `+` continuation as failure. That is wrong in both directions:
    ///
    /// * A server that supports the SASL-IR initial response may reply with a bare `+`
    ///   continuation before its tagged verdict, which was read as a rejection.
    /// * When the token genuinely is rejected, the server sends `+ <base64 error>` and waits
    ///   for the client to answer. RFC 7628 §3.2 requires an **empty** response so the server
    ///   can then emit the tagged `NO` carrying the real reason. Bailing at the `+` threw that
    ///   reason away, leaving the user with a base64 blob.
    ///
    /// This drives the state machine explicitly: send the initial response, then reply `""`
    /// to any error challenge so the tagged response arrives.
    pub async fn authenticate_xoauth2(&mut self) -> Result<(), Xoauth2Error> {
        let (_tag_num, tag) = self.next_tag();
        self.last_tag = tag.clone();

        // The initial response rides along with the AUTHENTICATE command (SASL-IR). Servers
        // without SASL-IR ignore the extra argument and challenge instead, which the loop
        // below handles.
        let payload = self.build_xoauth2_payload();
        let command = format!("{tag} AUTHENTICATE XOAUTH2 {payload}\r\n");

        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| Xoauth2Error::Transport("no stream".into()))?;
        stream
            .write_all(command.as_bytes())
            .await
            .map_err(|e| Xoauth2Error::Transport(e.to_string()))?;
        let _ = stream.flush().await;

        // Bound the exchange. Without a deadline a server that never sends its tagged
        // response parks the sync worker forever.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut sent_initial = true;
        let mut last_detail = String::new();

        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err(Xoauth2Error::Transport(
                    "XOAUTH2 exchange timed out after 30s".into(),
                ));
            }

            // Read until the server says something that advances the exchange: a continuation
            // request or a tagged response. Untagged status lines are skipped.
            let line = {
                let stream = self
                    .stream
                    .as_mut()
                    .ok_or_else(|| Xoauth2Error::Transport("stream closed".into()))?;
                read_sasl_line(stream, &tag, 30)
                    .await
                    .map_err(|e| Xoauth2Error::Transport(e.to_string()))?
            };

            // Tagged success.
            if line.starts_with(&format!("{tag} OK")) {
                debug!("XOAUTH2 authentication succeeded");
                return Ok(());
            }
            // Tagged failure: the server finally told us why.
            if line.starts_with(&format!("{tag} NO")) || line.starts_with(&format!("{tag} BAD")) {
                let detail = if last_detail.is_empty() {
                    line.clone()
                } else {
                    format!("{last_detail} ({})", line.trim_start_matches(tag.as_str()))
                };
                return Err(Self::xoauth2_error(detail));
            }
            // Continuation request.
            if line.starts_with('+') {
                if !sent_initial {
                    // Server without SASL-IR: hand it the credential now.
                    let response = format!("{payload}\r\n");
                    let stream = self
                        .stream
                        .as_mut()
                        .ok_or_else(|| Xoauth2Error::Transport("stream closed".into()))?;
                    stream
                        .write_all(response.as_bytes())
                        .await
                        .map_err(|e| Xoauth2Error::Transport(e.to_string()))?;
                    let _ = stream.flush().await;
                    sent_initial = true;
                    continue;
                }
                last_detail = Self::decode_xoauth2_challenge(&line);
                // RFC 7628 §3.2: answer an error challenge with an empty line so the server
                // emits the tagged NO. Echoing the credential here instead makes Gmail and
                // Outlook re-issue error challenges until the socket times out.
                let stream = self
                    .stream
                    .as_mut()
                    .ok_or_else(|| Xoauth2Error::Transport("stream closed".into()))?;
                stream
                    .write_all(b"\r\n")
                    .await
                    .map_err(|e| Xoauth2Error::Transport(e.to_string()))?;
                let _ = stream.flush().await;
                continue;
            }
            // Anything else (untagged status) is not progress; keep reading.
            debug!(line = %line, "unexpected line during XOAUTH2 exchange");
        }
    }

    fn xoauth2_error(detail: String) -> Xoauth2Error {
        let message = format!("IMAP XOAUTH2 rejected: {detail}");
        match Self::classify_xoauth2_failure(&detail) {
            Xoauth2Failure::Transient => Xoauth2Error::Transient(message),
            Xoauth2Failure::NeedsReauth => Xoauth2Error::NeedsReauth(message),
            Xoauth2Failure::Rejected => Xoauth2Error::Rejected(message),
        }
    }

    /// Build IMAP commands as strings (to be sent via codec)
    pub fn cmd_enable_qresync(&self) -> String {
        "ENABLE QRESYNC".into()
    }

    pub fn cmd_select(&self, mailbox: &str) -> String {
        format!("SELECT \"{}\"", Self::quote_mailbox(mailbox))
    }

    /// Build RFC 7162 Section 3.2.5.2 SELECT mailbox (QRESYNC (uidvalidity modseq))
    pub fn cmd_select_qresync(&self, mailbox: &str, uid_validity: u32, mod_seq: u64) -> String {
        format!(
            "SELECT \"{}\" (QRESYNC ({uid_validity} {mod_seq}))",
            Self::quote_mailbox(mailbox)
        )
    }

    /// Escape a mailbox name for use inside an IMAP quoted string (RFC 3501 §4.3).
    ///
    /// Only CR, LF, `\` and `"` are special. The previous code also stripped `*` and `%`,
    /// which are ordinary data inside a quoted string, so a folder named `Q1% Complete` was
    /// selected as `Q1 Complete` — either failing with `NO` or, worse, syncing the wrong
    /// mailbox. CR/LF are removed because IMAP forbids them anywhere in a command.
    pub fn quote_mailbox(mailbox: &str) -> String {
        mailbox
            .chars()
            .filter(|c| *c != '\r' && *c != '\n')
            .flat_map(|c| match c {
                '\\' => vec!['\\', '\\'],
                '"' => vec!['\\', '"'],
                other => vec![other],
            })
            .collect()
    }

    /// Build RFC 3501 Section 6.3.11 APPEND command
    pub fn cmd_append(&self, mailbox: &str, flags: &[vespetrel_core::Flag], size: usize) -> String {
        let clean = mailbox.replace(['\r', '\n', '"'], "").replace('\\', "\\\\");
        let flags_str = if flags.is_empty() {
            "".to_string()
        } else {
            let f_list: Vec<&str> = flags
                .iter()
                .map(|f| match f {
                    vespetrel_core::Flag::Seen => "\\Seen",
                    vespetrel_core::Flag::Answered => "\\Answered",
                    vespetrel_core::Flag::Flagged => "\\Flagged",
                    vespetrel_core::Flag::Deleted => "\\Deleted",
                    vespetrel_core::Flag::Draft => "\\Draft",
                })
                .collect();
            format!(" ({})", f_list.join(" "))
        };
        format!("APPEND \"{clean}\"{flags_str} {{{size}}}")
    }

    pub fn cmd_authenticate_xoauth2(&self) -> String {
        format!("AUTHENTICATE XOAUTH2 {}", self.build_xoauth2_payload())
    }

    pub fn cmd_login(&self) -> String {
        let clean_user = self
            .config
            .username
            .replace(['\r', '\n'], "")
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let clean_pass = self
            .config
            .auth_token
            .replace(['\r', '\n'], "")
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        format!("LOGIN \"{clean_user}\" \"{clean_pass}\"")
    }

    pub fn cmd_logout(&self) -> &'static str {
        "LOGOUT"
    }

    pub fn cmd_list(&self) -> &'static str {
        "LIST \"\" \"*\""
    }

    /// `range` is interpolated straight into the command line.
    ///
    /// A UID sequence set is `[` `]`-delimited digits, commas, and a single `*`. Anything
    /// else (CR/LF, parens, spaces) would let a caller append a second IMAP command, so it is
    /// rejected rather than sanitized into something subtly different.
    pub fn cmd_uid_fetch_envelope(&self, range: &str) -> anyhow::Result<String> {
        let bytes = range.as_bytes();
        if bytes.is_empty()
            || !bytes
                .iter()
                .all(|b| b.is_ascii_digit() || matches!(b, b',' | b':' | b'*' | b'[' | b']'))
        {
            anyhow::bail!("invalid IMAP UID sequence set: {range:?}");
        }
        Ok(format!(
            "UID FETCH {range} (UID FLAGS RFC822.SIZE ENVELOPE)"
        ))
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
    if let Some(stripped) = trimmed.strip_prefix('"') {
        let mut result = String::new();
        let chars = stripped.char_indices();
        let mut escaped = false;
        for (idx, c) in chars {
            if escaped {
                result.push(c);
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                let remainder = &stripped[idx + 1..];
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

#[inline]
fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Parse untagged `* <seq> FETCH (UID <uid> FLAGS (<flags>) MODSEQ <modseq> ...)` line
#[allow(clippy::type_complexity)]
pub fn parse_imap_fetch_line(
    line: &str,
) -> Option<(
    u32,
    Vec<vespetrel_core::message::Flag>,
    Option<u64>,
    Option<usize>,
)> {
    if !line.starts_with("* ") || find_ascii_case_insensitive(line, "FETCH").is_none() {
        return None;
    }

    let uid_pos = find_ascii_case_insensitive(line, "UID ")?;
    let after_uid = line[uid_pos + 4..].trim_start();
    let end = after_uid
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after_uid.len());
    let uid = after_uid[..end].parse::<u32>().ok()?;

    let mut flags = Vec::new();
    if let Some(flags_pos) = find_ascii_case_insensitive(line, "FLAGS (") {
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

    let mod_seq = if let Some(mod_pos) = find_ascii_case_insensitive(line, "MODSEQ (") {
        let after_mod = line[mod_pos + 8..].trim_start();
        let end = after_mod
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_mod.len());
        after_mod[..end].parse::<u64>().ok()
    } else if let Some(mod_pos) = find_ascii_case_insensitive(line, "MODSEQ ") {
        let after_mod = line[mod_pos + 7..].trim_start();
        let end = after_mod
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_mod.len());
        after_mod[..end].parse::<u64>().ok()
    } else {
        None
    };

    let size = if let Some(size_pos) = find_ascii_case_insensitive(line, "RFC822.SIZE ") {
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

/// Sentinel returned by [`parse_vanished_line`] when a range is too large to expand.
///
/// A single 30-byte server line such as `* VANISHED (EARLIER) 1:4294967295` would
/// otherwise drive 4 294 967 295 `push`es — a remotely triggerable multi-GB allocation.
/// The caller must treat this as "too many, drop the folder's UID cache and re-fetch".
pub const VANISHED_RANGE_TOO_LARGE: u32 = 0;

/// Largest contiguous UID range [`parse_vanished_line`] will materialise.
const MAX_EXPANDED_VANISHED: u64 = 100_000;

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
                    if end < start {
                        uids.push(start);
                        uids.push(end);
                        continue;
                    }
                    let span = u64::from(end) - u64::from(start) + 1;
                    if span > MAX_EXPANDED_VANISHED {
                        // Cannot be represented as a UID list; signal a full resync instead
                        // of attempting a huge allocation.
                        return vec![VANISHED_RANGE_TOO_LARGE];
                    }
                    uids.extend(start..=end);
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
    fn xoauth2_failure_classification_distinguishes_reauth_from_backend_hiccup() {
        // Exchange Online accepts the identity but fails to attach the mailbox. Reporting this
        // as an auth failure sends the user to re-consent for a problem a reconnect fixes.
        assert_eq!(
            ImapConnection::classify_xoauth2_failure("User is authenticated but not connected"),
            Xoauth2Failure::Transient
        );
        assert_eq!(
            ImapConnection::classify_xoauth2_failure(
                "{\"status\":\"4.7.0\",\"schemes\":\"Bearer\",\"scope\":\"mail\"}"
            ),
            Xoauth2Failure::NeedsReauth,
            "a Bearer challenge means the token needs reissuing"
        );
        assert_eq!(
            ImapConnection::classify_xoauth2_failure("invalid_grant"),
            Xoauth2Failure::NeedsReauth
        );
        assert_eq!(
            ImapConnection::classify_xoauth2_failure("server internal error"),
            Xoauth2Failure::Rejected
        );
    }

    #[test]
    fn xoauth2_errors_expose_the_action_the_engine_should_take() {
        let needs = ImapConnection::xoauth2_error("invalid_grant".into());
        assert!(needs.needs_reauth());
        assert!(!needs.is_transient());

        let transient =
            ImapConnection::xoauth2_error("User is authenticated but not connected".into());
        assert!(transient.is_transient());
        assert!(!transient.needs_reauth());
    }

    #[test]
    fn xoauth2_challenge_decodes_server_json_and_leaves_plain_text_alone() {
        use base64::Engine;
        let json = r#"{"status":"4.7.0","schemes":"Bearer","scope":"https://mail.google.com/"}"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(json.as_bytes());
        assert_eq!(
            ImapConnection::decode_xoauth2_challenge(&format!("+ {encoded}")),
            json,
            "the user must see the server's reason, not a base64 blob"
        );

        // An empty continuation carries no detail.
        assert_eq!(ImapConnection::decode_xoauth2_challenge("+ "), "");
        // Undecodable bodies are passed through rather than swallowed.
        assert_eq!(
            ImapConnection::decode_xoauth2_challenge("+ not-base64!!"),
            "not-base64!!"
        );
    }

    #[test]
    fn xoauth2_payload_is_rfc7628_shaped() {
        let conn = ImapConnection::new(
            ImapConfig::new("imap.gmail.com", 993, "user@gmail.com", "ya29.token").with_xoauth2(),
        );
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(conn.build_xoauth2_payload())
            .unwrap();
        assert_eq!(
            String::from_utf8(decoded).unwrap(),
            "user=user@gmail.com\x01auth=Bearer ya29.token\x01\x01",
            "the payload must end with two NULs or the server rejects the exchange"
        );
        // The credential must never contain a literal newline, which would split the command.
        assert!(!conn.build_xoauth2_payload().contains('\n'));
    }

    #[tokio::test]
    async fn xoauth2_exchange_succeeds_after_a_continuation_request() {
        // A server supporting SASL-IR may still emit a bare `+` before its verdict. The old
        // code read any `+` as rejection and failed here.
        let (client, server) = tokio::io::duplex(4096);
        let mut server = tokio::io::BufReader::new(server);
        let mut conn = ImapConnection::new(
            ImapConfig::new("imap.gmail.com", 993, "user@gmail.com", "ya29.token").with_xoauth2(),
        );
        // Drive the state machine against a scripted server.
        let server_task = tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};
            let mut request = String::new();
            server.read_line(&mut request).await.unwrap();
            // Challenge once, then accept.
            server.write_all(b"+ \r\n").await.unwrap();
            let mut ack = String::new();
            server.read_line(&mut ack).await.unwrap();
            server
                .write_all(b"A0001 OK Authenticated\r\n")
                .await
                .unwrap();
            request
        });

        // Reuse the connection's own I/O by hand-driving the stream.
        conn.stream = Some(ImapStream::Test(client));
        // Run the exchange, then assert what the server saw.
        let result = conn.authenticate_xoauth2().await;
        let seen = server_task.await.unwrap();
        assert!(result.is_ok(), "exchange failed: {result:?}");
        assert!(
            seen.contains("AUTHENTICATE XOAUTH2"),
            "server never received the AUTHENTICATE: {seen:?}"
        );
    }

    #[tokio::test]
    async fn xoauth2_rejection_answers_error_challenge_with_an_empty_line() {
        // RFC 7628 §3.2: the client must reply empty so the server can emit the tagged NO.
        // Echoing the credential again makes Gmail/Outlook re-challenge until timeout.
        let (client, server) = tokio::io::duplex(4096);
        let mut server = tokio::io::BufReader::new(server);
        let mut conn = ImapConnection::new(
            ImapConfig::new("imap.gmail.com", 993, "user@gmail.com", "expired").with_xoauth2(),
        );

        let credential_echoed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = credential_echoed.clone();
        let server_task = tokio::spawn(async move {
            use std::sync::atomic::Ordering;
            use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};
            let mut request = String::new();
            server.read_line(&mut request).await.unwrap();
            // Error challenge carrying base64 JSON.
            server
                .write_all(b"+ eyJzdGF0dXMiOiI0LjcuMCJ9\r\n")
                .await
                .unwrap();
            let mut reply = String::new();
            server.read_line(&mut reply).await.unwrap();
            // A correct client sends an empty line, not the credential again.
            if reply.contains("auth=Bearer") {
                flag.store(true, Ordering::SeqCst);
            }
            server
                .write_all(b"A0001 NO Invalid credentials (Failure)\r\n")
                .await
                .unwrap();
            reply
        });

        conn.stream = Some(ImapStream::Test(client));
        let result = conn.authenticate_xoauth2().await;
        let reply = server_task.await.unwrap();

        let err = result.expect_err("an expired token must fail");
        assert!(err.needs_reauth(), "expected NeedsReauth, got {err}");
        assert!(
            !credential_echoed.load(std::sync::atomic::Ordering::SeqCst),
            "client echoed the credential after an error challenge: {reply:?}"
        );
        assert!(
            reply.trim().is_empty(),
            "expected an empty challenge response, got {reply:?}"
        );
    }

    #[test]
    fn xoauth2_marks_stream_loss_as_transport_not_auth() {
        // A dropped socket must not be reported as "your token expired", which would send the
        // user through an unnecessary re-consent.
        let err = Xoauth2Error::Transport("connection reset".into());
        assert!(!err.needs_reauth());
        assert!(!err.is_transient());
    }

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

    /// Regression: `execute_cmd` split the response into trimmed, blank-line-filtered lines.
    /// That discarded the header/body separator, so **every** fetched message parsed as one
    /// long malformed header with an empty body.
    #[test]
    fn fetch_raw_preserves_the_header_body_separator() {
        let body = "From: a@b.com\r\nSubject: T\r\n\r\nHello\r\nworld\r\n";
        let mut resp: Vec<u8> = Vec::new();
        resp.extend_from_slice(format!("* 1 FETCH (UID 7 BODY[] {{{}}}", body.len()).as_bytes());
        resp.extend_from_slice(b"\r\n");
        resp.extend_from_slice(body.as_bytes());
        resp.extend_from_slice(b")\r\nA0001 OK UID FETCH done\r\n");

        let got = extract_first_literal(&resp).expect("literal present");
        assert_eq!(got, body.as_bytes());
        // The separator must survive byte-for-byte.
        assert!(
            got.windows(4).any(|w| w == b"\r\n\r\n"),
            "header/body separator was lost"
        );
        // Trailing whitespace significant to QP/format=flowed must not be trimmed.
        let qp = "From: a@b.com\r\n\r\ntrailing space \r\n";
        let mut r2: Vec<u8> = format!("* 1 FETCH (BODY[] {{{}}}", qp.len()).into_bytes();
        r2.extend_from_slice(b"\r\n");
        r2.extend_from_slice(qp.as_bytes());
        r2.extend_from_slice(b")\r\nA0001 OK done\r\n");
        assert_eq!(extract_first_literal(&r2).unwrap(), qp.as_bytes());
    }

    /// A message body containing `<tag> OK` must not be mistaken for the tagged completion.
    #[test]
    fn response_completion_requires_tagged_last_line() {
        let tag = "A0007";
        // Body mentions the tag; the real tagged line comes later.
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(b"* 1 FETCH (BODY[] {12}\r\nA0007 OK hi\r\n)\r\n");
        assert!(
            !response_is_complete(&buf, tag),
            "matched the body text instead of the tagged line"
        );
        buf.extend_from_slice(b"A0007 OK FETCH completed\r\n");
        assert!(response_is_complete(&buf, tag));

        // A NO/BAD terminal is also completion.
        assert!(response_is_complete(b"A0007 NO failed\r\n", tag));
        assert!(response_is_complete(b"A0007 BAD bad\r\n", tag));
        // A continuation request is completion of the first phase.
        assert!(response_is_complete(b"+ ready\r\n", tag));
        // Incomplete.
        assert!(!response_is_complete(b"* 1 EXISTS\r\n", tag));
    }

    /// Literal offsets must be computed in the byte domain. Mixing `from_utf8_lossy`
    /// offsets with raw buffer lengths overshot whenever 8-bit data preceded the marker.
    #[test]
    fn literal_marker_uses_byte_offsets_with_8bit_data_before_it() {
        let mut buf: Vec<u8> = Vec::new();
        // A fully-received first literal whose body contains 0xFF (invalid UTF-8, so
        // from_utf8_lossy expands it to 3 bytes and shifts every later offset).
        buf.extend_from_slice(b"* 1 FETCH (SUBJECT {1}\r\n");
        buf.push(0xFF);
        buf.extend_from_slice(b")\r\n");
        // Now a second literal whose body has only partially arrived.
        buf.extend_from_slice(b"* 2 FETCH (BODY[] {5}\r\nhel");
        let (start, n) = literal_marker(&buf).expect("pending literal");
        assert_eq!(n, 5);
        assert_eq!(&buf[start..], b"hel");

        // And it resolves to the right bytes once complete.
        buf.extend_from_slice(b"lo)\r\nA0001 OK done\r\n");
        assert_eq!(extract_first_literal(&buf).unwrap(), b"\xFF".as_slice());
    }

    /// Regression: `%` and `*` are ordinary data inside an IMAP quoted string. The old code
    /// stripped them, so `Q1% Complete` was selected as `Q1 Complete`.
    #[test]
    fn cmd_select_preserves_legal_mailbox_characters() {
        let conn = ImapConnection::new(ImapConfig::new("h", 993, "u", "t"));
        assert_eq!(conn.cmd_select("Q1% Complete"), "SELECT \"Q1% Complete\"");
        assert_eq!(conn.cmd_select("a*b"), "SELECT \"a*b\"");
        // CR/LF are illegal anywhere in an IMAP command and must be removed.
        assert_eq!(conn.cmd_select("IN\r\nBOX"), "SELECT \"INBOX\"");
        // Quoting is escaped so a mailbox name cannot break out.
        assert_eq!(conn.cmd_select("a\"b"), "SELECT \"a\\\"b\"");
        assert_eq!(conn.cmd_select("a\\b"), "SELECT \"a\\\\b\"");
    }

    /// `range` is interpolated into the command line, so it must be a UID sequence set.
    #[test]
    fn cmd_uid_fetch_envelope_rejects_command_injection() {
        let conn = ImapConnection::new(ImapConfig::new("h", 993, "u", "t"));
        assert!(conn.cmd_uid_fetch_envelope("1:*").is_ok());
        assert!(conn.cmd_uid_fetch_envelope("1,2,3").is_ok());
        assert!(conn.cmd_uid_fetch_envelope("1:10").is_ok());
        assert!(conn.cmd_uid_fetch_envelope("").is_err());
        assert!(
            conn.cmd_uid_fetch_envelope("1:*)\r\nA0099 DELETE ")
                .is_err(),
            "command injection accepted"
        );
        assert!(conn.cmd_uid_fetch_envelope("1 * x").is_err());
    }

    /// A 30-byte VANISHED line must not drive a multi-GB allocation.
    #[test]
    fn vanished_range_is_bounded() {
        assert_eq!(
            parse_vanished_line("* VANISHED (EARLIER) 1:4294967295"),
            vec![VANISHED_RANGE_TOO_LARGE]
        );
        // A normal range still expands.
        assert_eq!(
            parse_vanished_line("* VANISHED (EARLIER) 10:12"),
            vec![10, 11, 12]
        );
        // A reversed range does not wrap or underflow.
        assert_eq!(parse_vanished_line("* VANISHED 12:10"), vec![12, 10]);
        // Discrete UIDs.
        assert_eq!(parse_vanished_line("* VANISHED 5,7,9"), vec![5, 7, 9]);
    }

    /// A mock build pointed at a real server must not silently serve fabricated data.
    #[test]
    fn is_mock_is_host_driven_only() {
        let real = ImapConnection::new(ImapConfig::new("imap.gmail.com", 993, "u", "t"));
        assert!(!real.is_mock());
        let local = ImapConnection::new(ImapConfig::new("localhost", 1143, "u", "t"));
        assert!(local.is_mock());
        let example = ImapConnection::new(ImapConfig::new("imap.example.com", 993, "u", "t"));
        assert!(example.is_mock());
    }

    /// A bare `<tag> OK` (no response text) is a legal success; the old check looked for the
    /// substring `" OK "` and rejected it.
    #[test]
    fn bare_tagged_ok_is_recognised() {
        assert!(response_is_complete(b"A0002 OK\r\n", "A0002"));
        assert!(response_is_complete(b"A0002 OK Success\r\n", "A0002"));
        assert!(!response_is_complete(b"A0002 OKAY then\r\n", "A0002"));
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
