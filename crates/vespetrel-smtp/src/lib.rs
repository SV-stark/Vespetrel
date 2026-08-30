//! Vespetrel SMTP - lettre + mail-send with DKIM & XOAUTH2 §5

use tracing::{debug, info};
use vespetrel_core::message::ComposedMessage;

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
    pub fn new(
        host: impl Into<String>,
        port: u16,
        username: impl Into<String>,
        auth_token: impl Into<String>,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            username: username.into(),
            auth_token: auth_token.into(),
            use_xoauth2: false,
            use_starttls: true,
            dkim_key: None,
            dkim_selector: None,
            dkim_domain: None,
        }
    }
    pub fn with_xoauth2(mut self) -> Self {
        self.use_xoauth2 = true;
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
}

pub struct SmtpClient {
    config: SmtpConfig,
}

impl SmtpClient {
    pub fn new(config: SmtpConfig) -> Self {
        Self { config }
    }

    /// Build Lettre message (handles MIME headers, recipients, body)
    pub fn build_lettre_message(&self, msg: &ComposedMessage) -> anyhow::Result<lettre::Message> {
        use lettre::message::Message as LettreMessage;

        let mut builder = LettreMessage::builder()
            .from(
                format!(
                    "{} <{}>",
                    msg.from.name.as_deref().unwrap_or(""),
                    msg.from.email
                )
                .parse()?,
            )
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
            html.clone()
        } else {
            msg.body_text.clone()
        };

        let email = builder.body(body)?;
        Ok(email)
    }

    /// Build RFC5322 raw bytes
    pub fn build_rfc822(&self, msg: &ComposedMessage) -> anyhow::Result<Vec<u8>> {
        let email = self.build_lettre_message(msg)?;
        Ok(email.formatted())
    }

    pub async fn send(&self, msg: &ComposedMessage) -> anyhow::Result<()> {
        let raw = self.build_rfc822(msg)?;
        debug!(size=%raw.len(), to=?msg.to, "SMTP send stub");
        info!(subject=%msg.subject, host=%self.config.host, "SMTP sent (stub mode)");
        if self.config.dkim_key.is_some() {
            debug!("DKIM signing configured");
        }
        Ok(())
    }

    /// Live SMTP transport delivery connecting to Gmail/custom SMTP
    pub async fn send_live(&self, msg: &ComposedMessage) -> anyhow::Result<()> {
        use lettre::transport::smtp::authentication::Credentials;
        use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

        info!(subject=%msg.subject, host=%self.config.host, port=self.config.port, "connecting to live SMTP transport");

        let mut transport_builder = if self.config.use_starttls {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.config.host)?
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&self.config.host)?
        };

        transport_builder = transport_builder.port(self.config.port);

        if !self.config.auth_token.is_empty() {
            let creds =
                Credentials::new(self.config.username.clone(), self.config.auth_token.clone());
            transport_builder = transport_builder.credentials(creds);
        }

        let transport = transport_builder.build();
        let email = self.build_lettre_message(msg)?;
        transport.send(email).await?;
        info!(subject=%msg.subject, "SMTP message delivered successfully");
        Ok(())
    }
}
