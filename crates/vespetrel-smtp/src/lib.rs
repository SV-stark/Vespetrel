#[allow(unused_imports)]
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

        if let (Some(domain), Some(selector), Some(key)) = (
            &self.config.dkim_domain,
            &self.config.dkim_selector,
            &self.config.dkim_key,
        ) {
            if domain.is_empty()
                || selector.is_empty()
                || selector
                    .chars()
                    .any(|c| !c.is_alphanumeric() && c != '-' && c != '_')
            {
                anyhow::bail!("invalid DKIM selector or domain: syntax check failed");
            }
            use base64::Engine;
            use sha2::Digest;

            let date_str = chrono::Utc::now().to_rfc2822();
            let date_hdr_name = lettre::message::header::HeaderName::new_from_ascii_str("Date");
            builder = builder.raw_header(lettre::message::header::HeaderValue::new(
                date_hdr_name,
                date_str.clone(),
            ));

            let body_content = msg.body_html.as_deref().unwrap_or(&msg.body_text);
            let canon_body = canonicalize_relaxed_body(body_content);
            let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
            hasher.update(&canon_body);
            let bh = base64::engine::general_purpose::STANDARD.encode(hasher.finalize());

            let signed_headers = if msg.cc.is_empty() {
                "from:to:subject:date"
            } else {
                "from:to:cc:subject:date"
            };

            let sig_input = format!(
                "v=1; a=rsa-sha256; d={domain}; s={selector}; c=relaxed/relaxed; q=dns/txt; h={signed_headers}; bh={bh}; b="
            );

            // Canonicalize headers in order of h= followed by DKIM-Signature up to b=
            let mut headers_to_sign = String::new();
            headers_to_sign.push_str(&canonicalize_relaxed_header("from", &msg.from.to_string()));
            let to_str = msg
                .to
                .iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            headers_to_sign.push_str(&canonicalize_relaxed_header("to", &to_str));
            if !msg.cc.is_empty() {
                let cc_str = msg
                    .cc
                    .iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                headers_to_sign.push_str(&canonicalize_relaxed_header("cc", &cc_str));
            }
            headers_to_sign.push_str(&canonicalize_relaxed_header("subject", &msg.subject));
            headers_to_sign.push_str(&canonicalize_relaxed_header("date", &date_str));
            headers_to_sign.push_str(&canonicalize_relaxed_header("dkim-signature", &sig_input));

            // RSA PKCS#1 v1.5 signing via aws-lc-rs
            let der_bytes = if key.contains("-----BEGIN") {
                let stripped = key
                    .lines()
                    .filter(|l| !l.starts_with("-----"))
                    .collect::<Vec<_>>()
                    .concat();
                base64::engine::general_purpose::STANDARD
                    .decode(stripped.trim().as_bytes())
                    .unwrap_or_else(|_| key.as_bytes().to_vec())
            } else if let Ok(decoded) =
                base64::engine::general_purpose::STANDARD.decode(key.trim().as_bytes())
            {
                decoded
            } else {
                key.as_bytes().to_vec()
            };

            let key_pair = aws_lc_rs::signature::RsaKeyPair::from_pkcs8(&der_bytes)
                .or_else(|_| aws_lc_rs::signature::RsaKeyPair::from_der(&der_bytes))
                .map_err(|_| {
                    anyhow::anyhow!("invalid RSA private key for DKIM: expected PKCS#8 or DER key")
                })?;

            use aws_lc_rs::signature::KeyPair;
            let rng = aws_lc_rs::rand::SystemRandom::new();
            let mut sig = vec![0; key_pair.public_key().modulus_len()];
            key_pair
                .sign(
                    &aws_lc_rs::signature::RSA_PKCS1_SHA256,
                    &rng,
                    headers_to_sign.as_bytes(),
                    &mut sig,
                )
                .map_err(|_| {
                    anyhow::anyhow!("failed to compute RSASSA-PKCS1-v1_5 DKIM signature")
                })?;

            let b = base64::engine::general_purpose::STANDARD.encode(&sig);
            let dkim_val = format!("{sig_input}{b}");
            let name = lettre::message::header::HeaderName::new_from_ascii_str("DKIM-Signature");
            let val = lettre::message::header::HeaderValue::new(name, dkim_val);
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

        #[cfg(any(test, feature = "mock"))]
        {
            let raw = self.build_rfc822(msg)?;
            debug!(size=%raw.len(), to=?msg.to, "simulated test SMTP delivery");
            info!(subject=%msg.subject, host=%self.config.host, "SMTP test delivery passed");
            if self.config.dkim_key.is_some() {
                debug!("DKIM signing configured");
            }
            return Ok(());
        }

        #[cfg(not(any(test, feature = "mock")))]
        {
            self.send_live(msg).await
        }
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

/// Canonicalizes an email body using the RFC 6376 §3.4.4 relaxed body canonicalization algorithm.
pub fn canonicalize_relaxed_body(body: &str) -> Vec<u8> {
    if body.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for line in body.lines() {
        let mut canon_line = String::with_capacity(line.len());
        let mut in_wsp = false;
        for c in line.trim_end().chars() {
            if c == ' ' || c == '\t' {
                if !in_wsp {
                    canon_line.push(' ');
                    in_wsp = true;
                }
            } else {
                canon_line.push(c);
                in_wsp = false;
            }
        }
        lines.push(canon_line);
    }
    while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    if lines.is_empty() {
        return Vec::new();
    }
    let mut result = lines.join("\r\n");
    result.push_str("\r\n");
    result.into_bytes()
}

/// Canonicalizes a single header field using RFC 6376 §3.4.2 relaxed header canonicalization.
pub fn canonicalize_relaxed_header(name: &str, value: &str) -> String {
    let lower_name = name.trim().to_ascii_lowercase();
    let mut canon_val = String::with_capacity(value.len());
    let mut in_wsp = false;
    for c in value.trim().chars() {
        if c == ' ' || c == '\t' || c == '\r' || c == '\n' {
            if !in_wsp {
                canon_val.push(' ');
                in_wsp = true;
            }
        } else {
            canon_val.push(c);
            in_wsp = false;
        }
    }
    format!("{lower_name}:{canon_val}\r\n")
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
    fn test_smtp_build_message_with_dkim() {
        let rsa_pkcs8 = "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCy+OYeW6EsC8TQ4z6RiAIxRM5kEz6gbYkyemoQXiZLDrJf4yVj5EfI4lwEbAAr6D2v+QBJofXQWdigjMooqk7RW5lr0LYsNN7UhfnWsOt74BGzuQ8k6zf2m1EJoD20I04UyWfil+IMvpDVqcd6UbZZCGlLjlPSEs1B6tncbHHSPtHhGJcCpeyO5/KFqcDc9oXwB4skNQ6eRA2KYLY9O4jqThjHueNlA4hic6r9CyF0hpGZtxe90F5hwJ8s3TLBWTn8n5tZD2B8euCTBmgMiH6fnOKOo22VEqzGh9bSioxm+EzFzB0nYoPbAri7dgch7X3r7dNGpkhjwzjhp2R4MYeBAgMBAAECggEAe1E/Fmniivvs+FWsVhCWGiaj45bTDy2KXEq27GJDFnKg+6sCp2qy/7rg1ncoQxi58Jes2A+N1asitbVs0kpPFrh75SshayJe66cI+CJdj7Rb3i9EPRcKL5TjaLON8KJm+bGxMBOhQVDJcT+T4DePYpeGHfaK0PP9lE7jIJtkbg2Nzvbw1XHmnTXdCD36q3gt2uLwnydQXXtoK6WfXmz1hgfiRkdhAo5bOwKpupgGzoBR3aZFtNhQqRVnz/dR4qqQHpvpatmnoxwrbRG0b0YcIheutUictZpbk2JNpEjEfKG8JvkFbsCFsZVEyYE4K2UzLJFof7mb9JCOlOd4B/HZ1QKBgQDe0uDpGC/6jVmHId4xEr3b7+pDxnkZAOy8xXRBl/hXxDAjNU+01EACRFVoWnHWHDeZVfILdh6Az8N+wxl4HY1CtVAhujXU/ANr6cRyPPKMm/P0YuJ85RsxmnOuGsYeZmiEwspJVASJ472KW0mOJnMieqGcGarn3ajAacGqu2gWpwKBgQDNnpUyuVKHssOrBmWBwZ9F6uhK2iSoxtLAQfUBK9KWNN2hURMXZW1QoKT3Kp/0CDEgIUz9v9zfylER2NsO7C1p2azj+W+jNKkOsITgIeAcrcp5wYS2K2q4x6U+JkARkKna0V4PwsHBgyZrIIycd6y6zF5hBMan+Hxy8DfggZLdlwKBgQCOtrP0t1grepLn2QpNlfpiPpRlml3/ZLc75J+kT2hxFifatQ96+yKQESI+twcIIoR9wi1Hp/y7ddZ5fw31/791BVnwcCqAYnTyjgQTQvP6mPwz/42efsLfD1SeI2nXGLJCrdwQAS7y/hls3zEKSZgecjrGFy5+WVr2+gVfi66MKwKBgAcH8jAe2Cydt0Uk3dm3Bjw80R6mIPTIf7JlTvxwRC4wtpdqj02QgVFtfNaa1YdhtFRV7y0KH4Jjh6wljzAOcWsaL2hIQkIBbfp7nL+RSPmSE8dgD6qvB2I0KXlbk3tGSBicaiv9y+RTGMA3B7fd+8ETdfK5WBWsUI0Zm7+Ijr4XAoGBANLc+H3c9h1faSb2YSN+QHtcyWJxhZONrE7pbBTMuVbgewtqjR2Et4AvmIuhwNfmZI6gnrbd1TWvGeGhZbdFQHpgms/92HjfAl61rttB+LgaKWoMod4XWqMxtChLNBpLqXEcY1xND9gAM66IGSPw7XKFiygzgDBucFcdJdwMo/Jk";
        let config = SmtpConfig::new("smtp.example.com", 465, "alice", "token").with_dkim(
            "example.com",
            "default",
            rsa_pkcs8,
        );
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
            subject: "DKIM Signed Message".into(),
            body_text: "Hello Bob, this email is signed with DKIM.".into(),
            body_html: None,
            in_reply_to: None,
            references: Vec::new(),
            attachments: Vec::new(),
        };

        let formatted = client.build_rfc822(&msg).unwrap();
        let raw_str = String::from_utf8_lossy(&formatted);
        assert!(raw_str.contains("DKIM-Signature:"));
        assert!(raw_str.contains("d=example.com"));
        assert!(raw_str.contains("s=default"));
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
