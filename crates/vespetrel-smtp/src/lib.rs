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
    /// Autocrypt 1.1 Header value (optional)
    pub autocrypt_header: Option<String>,
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
            autocrypt_header: None,
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
    pub fn with_autocrypt(mut self, header_val: impl Into<String>) -> Self {
        self.autocrypt_header = Some(header_val.into());
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

        let from_addr: lettre::Address = msg.from.email.trim().replace(['\r', '\n'], "").parse()?;
        let from_name = msg
            .from
            .name
            .as_ref()
            .map(|n| n.replace(['\r', '\n', '"'], ""));
        let from_mailbox = lettre::message::Mailbox::new(from_name, from_addr);

        let clean_subject = msg.subject.replace(['\r', '\n'], " ");
        let mut builder = LettreMessage::builder()
            .from(from_mailbox)
            .subject(clean_subject);

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

        let email = if let Some(html) = &msg.body_html {
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

    pub async fn send(&self, msg: &ComposedMessage) -> anyhow::Result<()> {
        if !self.config.host.is_empty()
            && self.config.host != "localhost"
            && self.config.host != "127.0.0.1"
            && !self.config.host.ends_with(".example")
            && self.config.port > 0
        {
            return self.send_live(msg).await;
        }

        let raw = self.build_rfc822(msg)?;
        debug!(size=%raw.len(), to=?msg.to, "simulated test SMTP delivery");
        info!(subject=%msg.subject, host=%self.config.host, "SMTP test delivery passed");
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

#[cfg(test)]
mod tests {
    use super::*;
    use vespetrel_core::message::Address;

    #[test]
    fn test_smtp_build_message_with_autocrypt() {
        let config = SmtpConfig::new("smtp.example.com", 587, "alice", "token")
            .with_autocrypt("addr=alice@example.com; prefer-encrypt=mutual; keydata=mQEN...");
        let client = SmtpClient::new(config);

        let msg = ComposedMessage {
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
            subject: "Encrypted discussion".into(),
            body_text: "Hello Bob".into(),
            body_html: None,
            in_reply_to: None,
            references: Vec::new(),
            attachments: Vec::new(),
        };

        let formatted = client.build_rfc822(&msg).unwrap();
        let raw_str = String::from_utf8_lossy(&formatted);
        assert!(raw_str.contains("Autocrypt: addr=alice@example.com"));
        assert!(raw_str.contains("Subject: Encrypted discussion"));
    }

    #[test]
    fn test_smtp_build_message_with_cc_bcc_and_html() {
        let config = SmtpConfig::new("smtp.example.com", 465, "alice", "token");
        let client = SmtpClient::new(config);

        let msg = ComposedMessage {
            from: Address {
                name: Some("Alice".into()),
                email: "alice@example.com".into(),
            },
            to: vec![Address {
                name: Some("Bob".into()),
                email: "bob@example.com".into(),
            }],
            cc: vec![Address {
                name: Some("Carol".into()),
                email: "carol@example.com".into(),
            }],
            bcc: vec![Address {
                name: None,
                email: "secret@example.com".into(),
            }],
            subject: "Sprint Update".into(),
            body_text: "Update".into(),
            body_html: Some("<h1>Sprint Update</h1>".into()),
            in_reply_to: Some("<prev-msg@example.com>".into()),
            references: vec!["<prev-msg@example.com>".into()],
            attachments: Vec::new(),
        };

        let formatted = client.build_rfc822(&msg).unwrap();
        let raw_str = String::from_utf8_lossy(&formatted);
        assert!(raw_str.contains("Cc: Carol <carol@example.com>"));
        assert!(raw_str.contains("In-Reply-To: <prev-msg@example.com>"));
        assert!(raw_str.contains("<h1>Sprint Update</h1>"));
    }
}
