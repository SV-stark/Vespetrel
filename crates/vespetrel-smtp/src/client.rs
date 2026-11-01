//! High-level SMTP Client and Sender with Geary outbox and Thunderbird feature set.

use std::sync::Arc;
use std::time::Duration;
#[allow(unused_imports)]
use tracing::debug;
use tracing::{info, warn};

use crate::auth::Credentials;
use crate::dkim::DkimSigner;
use crate::dsn::DsnOptions;
use crate::error::SmtpResult;
use crate::message::SmtpMessage;
use crate::outbox::OutboxQueue;
use crate::transport::{SmtpConnection, TlsMode};
use vespetrel_core::message::ComposedMessage;

/// Consolidated SMTP configuration.
#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_token: String,
    pub use_xoauth2: bool,
    pub use_starttls: bool,
    pub tls_mode: TlsMode,
    pub client_hostname: String,
    pub timeout: Duration,

    /// DKIM private key PEM (optional)
    pub dkim_key: Option<String>,
    pub dkim_selector: Option<String>,
    pub dkim_domain: Option<String>,
    /// Autocrypt 1.1 Header value (optional)
    pub autocrypt_header: Option<String>,
    /// Delivery Status Notification options (optional)
    pub dsn: Option<DsnOptions>,
}

impl std::fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("auth_token", &"[REDACTED]")
            .field("use_xoauth2", &self.use_xoauth2)
            .field("use_starttls", &self.use_starttls)
            .field("tls_mode", &self.tls_mode)
            .field("client_hostname", &self.client_hostname)
            .field("timeout", &self.timeout)
            .field("dkim_key", &self.dkim_key.as_ref().map(|_| "[REDACTED]"))
            .field("dkim_selector", &self.dkim_selector)
            .field("dkim_domain", &self.dkim_domain)
            .field("autocrypt_header", &self.autocrypt_header)
            .field("dsn", &self.dsn)
            .finish()
    }
}

impl SmtpConfig {
    pub fn new(
        host: impl Into<String>,
        port: u16,
        username: impl Into<String>,
        auth_token: impl Into<String>,
    ) -> Self {
        let p = port;
        let default_tls = if p == 465 {
            TlsMode::DirectTls
        } else {
            TlsMode::StrictStartTls
        };

        Self {
            host: host.into(),
            port,
            username: username.into(),
            auth_token: auth_token.into(),
            use_xoauth2: false,
            use_starttls: true,
            tls_mode: default_tls,
            client_hostname: "localhost".into(),
            timeout: Duration::from_secs(30),
            dkim_key: None,
            dkim_selector: None,
            dkim_domain: None,
            autocrypt_header: None,
            dsn: None,
        }
    }

    pub fn with_xoauth2(mut self) -> Self {
        self.use_xoauth2 = true;
        self
    }

    pub fn with_starttls(mut self, enabled: bool) -> Self {
        self.use_starttls = enabled;
        if !enabled {
            self.tls_mode = TlsMode::PlaintextOnly;
        } else if self.port == 465 {
            self.tls_mode = TlsMode::DirectTls;
        } else {
            self.tls_mode = TlsMode::StrictStartTls;
        }
        self
    }

    pub fn with_tls_mode(mut self, mode: TlsMode) -> Self {
        self.tls_mode = mode;
        self.use_starttls = matches!(
            mode,
            TlsMode::StrictStartTls | TlsMode::OpportunisticStartTls
        );
        self
    }

    pub fn with_hostname(mut self, hostname: impl Into<String>) -> Self {
        self.client_hostname = hostname.into();
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_dkim(
        mut self,
        domain: impl Into<String>,
        selector: impl Into<String>,
        key: impl Into<String>,
    ) -> Self {
        self.dkim_domain = Some(domain.into());
        self.dkim_selector = Some(selector.into());
        self.dkim_key = Some(key.into());
        self
    }

    pub fn with_autocrypt(mut self, header_val: impl Into<String>) -> Self {
        self.autocrypt_header = Some(header_val.into());
        self
    }

    pub fn with_dsn(mut self, dsn: DsnOptions) -> Self {
        self.dsn = Some(dsn);
        self
    }

    pub fn credentials(&self) -> Credentials {
        let mut creds = Credentials::new(&self.username).with_server(&self.host, self.port);

        if self.use_xoauth2 {
            creds = creds.with_oauth_token(&self.auth_token);
        } else {
            creds = creds.with_password(&self.auth_token);
        }
        creds
    }
}

/// Comprehensive, lightweight SMTP client supporting both direct pure-Rust transmission
/// and Geary-style asynchronous offline outbox queueing.
pub struct SmtpClient {
    config: SmtpConfig,
    outbox: Arc<OutboxQueue>,
}

impl SmtpClient {
    pub fn new(config: SmtpConfig) -> Self {
        Self {
            config,
            outbox: Arc::new(OutboxQueue::new()),
        }
    }

    pub fn config(&self) -> &SmtpConfig {
        &self.config
    }

    pub fn outbox(&self) -> &Arc<OutboxQueue> {
        &self.outbox
    }

    /// Enqueue a message into the asynchronous outbox for immediate background delivery.
    pub async fn send_via_outbox(&self, msg: SmtpMessage) -> String {
        self.outbox.enqueue(msg).await
    }

    /// Pure-Rust direct async delivery using native Tokio and Rustls transport.
    pub async fn send_pure_rust(&self, msg: &SmtpMessage) -> SmtpResult<String> {
        let fqdn = if !self.config.client_hostname.is_empty()
            && self.config.client_hostname != "localhost"
        {
            &self.config.client_hostname
        } else {
            &self.config.host
        };

        // 1. Generate RFC 5322 payload (strips Bcc, adds Thunderbird headers)
        let mut raw_bytes = msg.build_rfc5322_bytes(fqdn)?;

        // 2. Sign with DKIM if configured
        if let (Some(domain), Some(selector), Some(key)) = (
            &self.config.dkim_domain,
            &self.config.dkim_selector,
            &self.config.dkim_key,
        ) {
            let signer = DkimSigner::new(domain, selector, key)?;
            raw_bytes = signer.sign_message(&raw_bytes)?;
        }

        let recipients = msg.envelope_recipients();
        let sender = &msg.from.email;

        // 3. Connect via pure-Rust Tokio + Rustls
        let mut conn = SmtpConnection::new(&self.config.host, self.config.port)
            .with_timeout(self.config.timeout);

        conn.connect(self.config.tls_mode, fqdn).await?;

        // 4. Authenticate if credentials provided
        if !self.config.auth_token.is_empty() {
            let creds = self.config.credentials();
            conn.authenticate(&creds).await?;
        }

        // 5. Transmit message
        let server_id = conn
            .transmit_message(sender, &recipients, &raw_bytes)
            .await?;
        let _ = conn.quit().await;

        Ok(server_id)
    }

    /// Compatibility method: Build Lettre message (handles MIME headers, recipients, attachments, Message-ID, DKIM).
    pub fn build_lettre_message(&self, msg: &ComposedMessage) -> anyhow::Result<lettre::Message> {
        use lettre::message::Message as LettreMessage;

        let from_addr: lettre::Address = msg.from.email.trim().replace(['\r', '\n'], "").parse()?;
        let from_name = msg
            .from
            .name
            .as_ref()
            .map(|n| n.replace(['\r', '\n', '"'], ""));
        let from_mailbox = lettre::message::Mailbox::new(from_name, from_addr);

        let clean_subject = msg.subject.replace(['\r', '\n'], " ");
        let domain = if let Some(pos) = msg.from.email.find('@') {
            &msg.from.email[pos + 1..]
        } else {
            &self.config.host
        };
        let msg_id = format!("<{}@{}>", uuid::Uuid::new_v4(), domain);

        let mut builder = LettreMessage::builder()
            .from(from_mailbox)
            .subject(clean_subject)
            .message_id(Some(msg_id));

        for to in &msg.to {
            let addr: lettre::Address = to.email.trim().replace(['\r', '\n'], "").parse()?;
            let name = to.name.as_ref().map(|n| n.replace(['\r', '\n', '"'], ""));
            let mailbox = lettre::message::Mailbox::new(name, addr);
            builder = builder.to(mailbox);
        }
        for cc in &msg.cc {
            let addr: lettre::Address = cc.email.trim().replace(['\r', '\n'], "").parse()?;
            let name = cc.name.as_ref().map(|n| n.replace(['\r', '\n', '"'], ""));
            builder = builder.cc(lettre::message::Mailbox::new(name, addr));
        }
        for bcc in &msg.bcc {
            let addr: lettre::Address = bcc.email.trim().replace(['\r', '\n'], "").parse()?;
            let name = bcc.name.as_ref().map(|n| n.replace(['\r', '\n', '"'], ""));
            builder = builder.bcc(lettre::message::Mailbox::new(name, addr));
        }

        if let Some(in_reply_to) = &msg.in_reply_to {
            let clean = in_reply_to.replace(['\r', '\n'], " ");
            let name = lettre::message::header::HeaderName::new_from_ascii_str("In-Reply-To");
            let val = lettre::message::header::HeaderValue::new(name, clean);
            builder = builder.raw_header(val);
        }

        if !msg.references.is_empty() {
            let clean = msg.references.join(" ").replace(['\r', '\n'], " ");
            let name = lettre::message::header::HeaderName::new_from_ascii_str("References");
            let val = lettre::message::header::HeaderValue::new(name, clean);
            builder = builder.raw_header(val);
        }

        if let Some(autocrypt) = &self.config.autocrypt_header {
            let sanitized_autocrypt = autocrypt.replace(['\r', '\n'], " ");
            let name = lettre::message::header::HeaderName::new_from_ascii_str("Autocrypt");
            let val = lettre::message::header::HeaderValue::new(name, sanitized_autocrypt);
            builder = builder.raw_header(val);
        }

        // Build base content
        let body_part = if let Some(html) = &msg.body_html {
            if !msg.body_text.trim().is_empty() {
                lettre::message::MultiPart::alternative_plain_html(
                    msg.body_text.clone(),
                    html.clone(),
                )
            } else {
                lettre::message::MultiPart::alternative()
                    .singlepart(lettre::message::SinglePart::html(html.clone()))
            }
        } else {
            lettre::message::MultiPart::alternative()
                .singlepart(lettre::message::SinglePart::plain(msg.body_text.clone()))
        };

        let multipart_body = if !msg.attachments.is_empty() {
            let mut mixed = lettre::message::MultiPart::mixed().multipart(body_part);
            for att in &msg.attachments {
                let ct = att
                    .content_type
                    .parse::<lettre::message::header::ContentType>()
                    .unwrap_or_else(|_| {
                        "application/octet-stream"
                            .parse::<lettre::message::header::ContentType>()
                            .unwrap()
                    });
                let attachment_part = lettre::message::Attachment::new(att.filename.clone())
                    .body(att.data.clone(), ct);
                mixed = mixed.singlepart(attachment_part);
            }
            Some(mixed)
        } else {
            None
        };

        // Handle DKIM signing if configured
        if let (Some(domain), Some(selector), Some(key)) = (
            &self.config.dkim_domain,
            &self.config.dkim_selector,
            &self.config.dkim_key,
        ) {
            let signer = DkimSigner::new(domain, selector, key)?;

            let date_str = chrono::Utc::now().to_rfc2822();
            let date_hdr_name = lettre::message::header::HeaderName::new_from_ascii_str("Date");
            builder = builder.raw_header(lettre::message::header::HeaderValue::new(
                date_hdr_name,
                date_str.clone(),
            ));

            let preview_builder = builder.clone();
            let preview_email = if let Some(mixed) = multipart_body.clone() {
                preview_builder.multipart(mixed)?
            } else if let Some(html) = &msg.body_html {
                if !msg.body_text.trim().is_empty() {
                    preview_builder.multipart(
                        lettre::message::MultiPart::alternative_plain_html(
                            msg.body_text.clone(),
                            html.clone(),
                        ),
                    )?
                } else {
                    preview_builder.body(html.clone())?
                }
            } else {
                preview_builder.body(msg.body_text.clone())?
            };

            let preview_raw = preview_email.formatted();
            let signed_bytes = signer.sign_message(&preview_raw)?;

            // Extract signed DKIM-Signature header line from signed_bytes
            let signed_str = String::from_utf8_lossy(&signed_bytes);
            if let Some(dkim_line) = signed_str
                .lines()
                .find(|l| l.starts_with("DKIM-Signature:"))
            {
                let val_str = dkim_line["DKIM-Signature:".len()..].trim().to_string();
                let name =
                    lettre::message::header::HeaderName::new_from_ascii_str("DKIM-Signature");
                let val = lettre::message::header::HeaderValue::new(name, val_str);
                builder = builder.raw_header(val);
            }
        }

        let email = if let Some(mixed) = multipart_body {
            builder.multipart(mixed)?
        } else if let Some(html) = &msg.body_html {
            if !msg.body_text.trim().is_empty() {
                builder.multipart(lettre::message::MultiPart::alternative_plain_html(
                    msg.body_text.clone(),
                    html.clone(),
                ))?
            } else {
                builder.body(html.clone())?
            }
        } else {
            builder.body(msg.body_text.clone())?
        };

        Ok(email)
    }

    /// Build RFC5322 raw bytes
    pub fn build_rfc822(&self, msg: &ComposedMessage) -> anyhow::Result<Vec<u8>> {
        let email = self.build_lettre_message(msg)?;
        Ok(email.formatted())
    }

    /// Backwards compatible send method
    pub async fn send(&self, msg: &ComposedMessage) -> anyhow::Result<()> {
        if !self.config.host.is_empty()
            && self.config.host != "localhost"
            && self.config.host != "127.0.0.1"
            && !self.config.host.ends_with(".example")
            && self.config.port > 0
        {
            return self.send_live(msg).await;
        }

        #[cfg(test)]
        {
            let raw = self.build_rfc822(msg)?;
            debug!(size=%raw.len(), to=?msg.to, "simulated test SMTP delivery");
            info!(subject=%msg.subject, host=%self.config.host, "SMTP test delivery passed");
            if self.config.dkim_key.is_some() {
                debug!("DKIM signing configured");
            }
            Ok(())
        }

        #[cfg(not(test))]
        {
            self.send_live(msg).await
        }
    }

    /// Live SMTP transport delivery
    pub async fn send_live(&self, msg: &ComposedMessage) -> anyhow::Result<()> {
        use lettre::transport::smtp::authentication::Credentials as LettreCreds;
        use lettre::transport::smtp::client::{Tls, TlsParameters};
        use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

        info!(subject=%msg.subject, host=%self.config.host, port=self.config.port, "connecting to live SMTP transport");

        let mut transport_builder = if self.config.port == 465 {
            let tls_params = TlsParameters::new(self.config.host.clone())?;
            AsyncSmtpTransport::<Tokio1Executor>::relay(&self.config.host)?
                .tls(Tls::Wrapper(tls_params))
        } else if self.config.use_starttls {
            if !self.config.auth_token.is_empty() {
                warn!(
                    host=%self.config.host,
                    port=self.config.port,
                    "STARTTLS is opportunistic: if the server does not advertise STARTTLS, credentials and the message are transmitted in cleartext"
                );
            }
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.config.host)?
        } else {
            if !self.config.auth_token.is_empty() {
                warn!(
                    host=%self.config.host,
                    port=self.config.port,
                    "STARTTLS is disabled for a remote host: sending message unencrypted"
                );
            }
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&self.config.host)
        };

        transport_builder = transport_builder.port(self.config.port);

        if !self.config.auth_token.is_empty() {
            let creds =
                LettreCreds::new(self.config.username.clone(), self.config.auth_token.clone());
            if self.config.use_xoauth2 {
                transport_builder = transport_builder.authentication(vec![
                    lettre::transport::smtp::authentication::Mechanism::Xoauth2,
                ]);
            }
            transport_builder = transport_builder.credentials(creds);
        }

        let transport = transport_builder.build();
        let email = self.build_lettre_message(msg)?;
        transport.send(email).await?;
        info!(subject=%msg.subject, "SMTP message delivered successfully");
        Ok(())
    }
}

/// Lightweight, one-line SMTP sender inspired by Geary's simplicity.
pub struct SmtpSender {
    client: SmtpClient,
}

impl SmtpSender {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        let config = SmtpConfig::new(host, port, "", "");
        Self {
            client: SmtpClient::new(config),
        }
    }

    pub fn with_auth(mut self, username: impl Into<String>, token: impl Into<String>) -> Self {
        self.client.config.username = username.into();
        self.client.config.auth_token = token.into();
        self
    }

    pub fn with_xoauth2(mut self) -> Self {
        self.client.config.use_xoauth2 = true;
        self
    }

    pub fn client(&self) -> &SmtpClient {
        &self.client
    }

    pub async fn send(&self, msg: &ComposedMessage) -> anyhow::Result<()> {
        self.client.send(msg).await
    }
}
