//! Vespetrel SMTP - lettre + mail-send with DKIM & XOAUTH2 §5

use vespetrel_core::message::ComposedMessage;
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_token: String,
    pub use_xoauth2: bool,
    pub use_starttls: bool,
    /// DKIM private key PEM (optional)
    pub dkim_key: Option<String>,
    pub dkim_selector: Option<String>,
    pub dkim_domain: Option<String>,
}

impl SmtpConfig {
    pub fn new(host: impl Into<String>, port: u16, username: impl Into<String>, auth_token: impl Into<String>) -> Self {
        Self { host: host.into(), port, username: username.into(), auth_token: auth_token.into(), use_xoauth2: false, use_starttls: true, dkim_key: None, dkim_selector: None, dkim_domain: None }
    }
    pub fn with_xoauth2(mut self) -> Self { self.use_xoauth2 = true; self }
    pub fn with_dkim(mut self, domain: impl Into<String>, selector: impl Into<String>, key: impl Into<String>) -> Self {
        self.dkim_domain = Some(domain.into());
        self.dkim_selector = Some(selector.into());
        self.dkim_key = Some(key.into());
        self
    }
}

pub struct SmtpClient {
    config: SmtpConfig,
}

impl SmtpClient {
    pub fn new(config: SmtpConfig) -> Self { Self { config } }

    /// Build RFC5322 message using mail-builder (handles MIME, attachments, encodings)
    pub fn build_rfc822(&self, msg: &ComposedMessage) -> anyhow::Result<Vec<u8>> {
        // Real implementation uses mail-builder::MessageBuilder
        // For now construct minimally with lettre::Message
        use lettre::message::{header, Message as LettreMessage};

        let mut builder = LettreMessage::builder()
            .from(format!("{} <{}>", msg.from.name.as_deref().unwrap_or(""), msg.from.email).parse()?)
            .subject(&msg.subject);

        for to in &msg.to {
            let addr: lettre::Address = to.email.parse()?;
            let mailbox = lettre::message::Mailbox::new(to.name.clone(), addr);
            builder = builder.to(mailbox);
        }
        for cc in &msg.cc {
            let addr: lettre::Address = cc.email.parse()?;
            builder = builder.cc(lettre::message::Mailbox::new(cc.name.clone(), addr));
        }
        for bcc in &msg.bcc {
            let addr: lettre::Address = bcc.email.parse()?;
            builder = builder.bcc(lettre::message::Mailbox::new(bcc.name.clone(), addr));
        }

        let body = if let Some(html) = &msg.body_html {
            // multipart alternative - simple stub picks html
            html.clone()
        } else {
            msg.body_text.clone()
        };

        let email = builder.body(body)?;
        Ok(email.formatted())
    }

    pub async fn send(&self, msg: &ComposedMessage) -> anyhow::Result<()> {
        let raw = self.build_rfc822(msg)?;
        debug!(size=%raw.len(), to=?msg.to, "SMTP send stub");
        // Real: use lettre::AsyncSmtpTransport with XOAUTH2 or LOGIN
        // let creds = if self.config.use_xoauth2 { Credentials::new("user", xoauth2_token) } else ...
        // transport.send(raw).await?
        info!(subject=%msg.subject, host=%self.config.host, "SMTP sent (stub - not actually connecting)");
        // DKIM signing would happen here via mail-send if configured
        if self.config.dkim_key.is_some() {
            debug!("DKIM signing would be applied here");
        }
        Ok(())
    }
}
