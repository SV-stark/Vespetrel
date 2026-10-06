#[allow(unused_imports)]
use tracing::{debug, info, warn};
use vespetrel_core::message::ComposedMessage;

#[derive(Clone)]
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

impl std::fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("auth_token", &"[REDACTED]")
            .field("use_xoauth2", &self.use_xoauth2)
            .field("use_starttls", &self.use_starttls)
            .field("dkim_key", &self.dkim_key.as_ref().map(|_| "[REDACTED]"))
            .field("dkim_selector", &self.dkim_selector)
            .field("dkim_domain", &self.dkim_domain)
            .field("autocrypt_header", &self.autocrypt_header)
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

    /// Build Lettre message (handles MIME headers, recipients, attachments, Message-ID, DKIM)
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

        // Build base content (plain text, alternative plain+html, or html only)
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

        // Combine with attachments into MultiPart::mixed() if attachments are present
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
            if !is_valid_dkim_domain(domain) {
                anyhow::bail!("invalid DKIM signing domain: syntax check failed");
            }
            if !is_valid_dkim_selector(selector) {
                anyhow::bail!("invalid DKIM selector: syntax check failed");
            }
            use base64::Engine;
            use sha2::Digest;

            let date_str = chrono::Utc::now().to_rfc2822();
            let date_hdr_name = lettre::message::header::HeaderName::new_from_ascii_str("Date");
            builder = builder.raw_header(lettre::message::header::HeaderValue::new(
                date_hdr_name,
                date_str.clone(),
            ));

            // Format preview message to compute exact canonicalized MIME body
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
            let body_bytes =
                if let Some(pos) = preview_raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    &preview_raw[pos + 4..]
                } else {
                    &preview_raw[..]
                };

            let canon_body = canonicalize_relaxed_body_bytes(body_bytes);
            let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
            hasher.update(&canon_body);
            let bh = base64::engine::general_purpose::STANDARD.encode(hasher.finalize());

            // Canonicalize the signed headers from the *serialized* message, not from the
            // in-memory model. The two differ whenever lettre rewrites a value: a non-ASCII
            // subject or display name comes out RFC 2047 encoded, long address lists are
            // folded, and the `From` display name is quoted differently. Signing the model
            // produced a signature over bytes the receiving agent never sees, so DKIM
            // verification failed for every message with a non-ASCII subject or sender name.
            let mut signed_names: Vec<&str> = vec!["from", "to", "subject", "date"];
            if !msg.cc.is_empty() {
                signed_names.insert(2, "cc");
            }
            let signed_headers = signed_names.join(":");

            let sig_input = format!(
                "v=1; a=rsa-sha256; d={domain}; s={selector}; c=relaxed/relaxed; q=dns/txt; h={signed_headers}; bh={bh}; b="
            );

            let headers_to_sign = canonicalize_signed_headers(&preview_raw, &signed_names)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "DKIM: the serialized message is missing one of the signed headers \
                         ({signed_headers}); refusing to emit an unverifiable signature"
                    )
                })?;

            // The `dkim-signature` field is not in the serialized bytes yet: it is the
            // signature being computed. RFC 6376 §3.5 signs it with the `b=` value removed,
            // which is exactly `sig_input`, and it is always the last field in `h=`.
            let mut signed_bytes = headers_to_sign;
            signed_bytes.extend_from_slice(
                canonicalize_relaxed_header("dkim-signature", &sig_input).as_bytes(),
            );

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
                    &signed_bytes,
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
            Ok(())
        }

        #[cfg(not(any(test, feature = "mock")))]
        {
            self.send_live(msg).await
        }
    }

    /// Live SMTP transport delivery connecting to Gmail/custom SMTP
    pub async fn send_live(&self, msg: &ComposedMessage) -> anyhow::Result<()> {
        use lettre::transport::smtp::authentication::Credentials;
        use lettre::transport::smtp::client::{Tls, TlsParameters};
        use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

        info!(subject=%msg.subject, host=%self.config.host, port=self.config.port, "connecting to live SMTP transport");

        let mut transport_builder = if self.config.port == 465 {
            // Port 465 is implicit TLS (SMTPS wrapper)
            let tls_params = TlsParameters::new(self.config.host.clone())?;
            AsyncSmtpTransport::<Tokio1Executor>::relay(&self.config.host)?
                .tls(Tls::Wrapper(tls_params))
        } else if self.config.use_starttls {
            // `starttls_relay` maps to lettre's `Tls::Required`, which is *opportunistic*:
            // if the server (or a MITM stripping the STARTTLS advertisement) does not offer
            // TLS, the connection silently continues in plaintext. Say so, because the user
            // asked for STARTTLS and will otherwise believe the mail was encrypted.
            if !self.config.auth_token.is_empty() {
                warn!(
                    host=%self.config.host,
                    port=self.config.port,
                    "STARTTLS is opportunistic: if the server does not advertise STARTTLS, \
                     credentials and the message are transmitted in cleartext"
                );
            }
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.config.host)?
        } else {
            // `use_starttls = false` means plaintext. The previous code called
            // `relay()`, which selects *implicit* TLS (`Tls::Wrapper`) and only works on
            // 465; pointing it at 587 attempted a TLS handshake on the submission port and
            // the connection failed for every server that speaks plaintext there.
            if !self.config.auth_token.is_empty() {
                warn!(
                    host=%self.config.host,
                    port=self.config.port,
                    "STARTTLS is disabled for a remote host: sending the message and the \
                     credentials unencrypted"
                );
            }
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&self.config.host)
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

/// Canonicalizes an email body from raw bytes using RFC 6376 §3.4.4 relaxed body canonicalization.
pub fn canonicalize_relaxed_body_bytes(body: &[u8]) -> Vec<u8> {
    if body.is_empty() {
        return Vec::new();
    }
    let s = String::from_utf8_lossy(body);
    canonicalize_relaxed_body(&s)
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

/// Canonicalize the named headers, in `names` order, from an actual serialized RFC 5322
/// message, per RFC 6376 §3.4.2 relaxed header canonicalization.
///
/// Returns `None` when any named header is absent, so the caller fails closed rather than
/// emitting a signature whose `h=` list promises a header the message does not contain.
///
/// When a header appears more than once, the bottom-most instance is used: that is what a
/// verifier does, and any other choice produces a signature that can never validate.
pub fn canonicalize_signed_headers(raw: &[u8], names: &[&str]) -> Option<Vec<u8>> {
    let fields = split_header_fields(raw);
    let mut out = Vec::new();
    for name in names {
        let want = name.trim().to_ascii_lowercase();
        let value = fields
            .iter()
            .rev()
            .find(|(n, _)| *n == want)
            .map(|(_, v)| v.as_str())?;
        out.extend_from_slice(canonicalize_relaxed_header(name, value).as_bytes());
    }
    Some(out)
}

/// Split a serialized message into unfolded header fields as `(lowercase name, value)`.
///
/// A bare LF is accepted as a line terminator as well as CRLF: some servers and test
/// fixtures emit LF-only messages, and rejecting them would mean signing nothing.
fn split_header_fields(raw: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(raw);
    let header_end = text.find("\r\n\r\n").or_else(|| text.find("\n\n"));
    let block = match header_end {
        Some(i) => &text[..i],
        // No body separator: treat the whole input as the header block.
        None => &text[..],
    };

    let mut fields: Vec<(String, String)> = Vec::new();
    for line in block.split_inclusive(['\r', '\n']) {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation of the previous field. RFC 5322 unfolding removes the line
            // break but keeps the folding whitespace; relaxed canonicalization then
            // collapses the run, so a single space is appended.
            if let Some(last) = fields.last_mut() {
                last.1.push(' ');
                last.1.push_str(line);
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            fields.push((name.trim().to_ascii_lowercase(), value.to_string()));
        }
    }
    fields
}

/// A DKIM `d=` value must be a DNS domain name that can be looked up for a TXT record.
///
/// Rejecting anything that is not a plain ASCII LDH domain is what keeps a CR/LF, a space
/// or a Unicode character from ending up inside the `DKIM-Signature` header value.
pub fn is_valid_dkim_domain(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > 255 {
        return false;
    }
    // A trailing dot is legal in a presentation-format FQDN but not in a DKIM `d=`.
    if domain.ends_with('.') {
        return false;
    }
    domain.split('.').all(valid_ldh_label)
}

/// A DKIM `s=` value is a sub-domain: dot-separated LDH labels (RFC 6376 §3.1,
/// `selector = sub-domain ;`).
///
/// The previous check rejected the dot entirely, so any provider using a nested selector
/// (`mail.sel1.sel2`) was rejected as invalid; and it accepted non-ASCII letters, producing
/// a `s=` value that is not a legal DNS label.
pub fn is_valid_dkim_selector(selector: &str) -> bool {
    if selector.is_empty() || selector.len() > 255 {
        return false;
    }
    selector.split('.').all(valid_ldh_label)
}

fn valid_ldh_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && !label.starts_with('-')
        && !label.ends_with('-')
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

    /// The signed bytes must be derived from the headers the receiver actually parses.
    ///
    /// Signing the in-memory model broke verification as soon as lettre rewrote a value:
    /// a non-ASCII subject comes out RFC 2047 encoded, and the signature was computed over
    /// the raw UTF-8 instead. Reconstructing the canonicalized `h=` fields from the
    /// serialized bytes and checking they match is the invariant that was violated.
    #[test]
    fn dkim_signs_the_serialized_headers_not_the_model() {
        let raw = b"From: =?utf-8?B?w4FsaWNl?= <alice@example.com>\r\n\
To: Bob <bob@example.com>\r\n\
Subject: =?utf-8?Q?Caf=C3=A9_meeting?=\r\n\
Date: Mon, 01 Jan 2024 00:00:00 +0000\r\n\
\r\n\
body\r\n";
        let canon =
            canonicalize_signed_headers(raw, &["from", "to", "subject", "date"]).expect("headers");
        let canon = String::from_utf8(canon).unwrap();
        // RFC 6376 §3.4.2: `name:value` with no space after the colon.
        assert_eq!(
            canon,
            "from:=?utf-8?B?w4FsaWNl?= <alice@example.com>\r\n\
             to:Bob <bob@example.com>\r\n\
             subject:=?utf-8?Q?Caf=C3=A9_meeting?=\r\n\
             date:Mon, 01 Jan 2024 00:00:00 +0000\r\n"
                .replace("             ", "")
        );
    }

    /// A `bh=`/`b=` that is computed over headers the message does not contain can never
    /// verify. Fail closed instead of emitting one.
    #[test]
    fn dkim_canonicalization_fails_closed_on_missing_header() {
        let raw = b"From: a@b.com\r\nTo: c@d.com\r\n\r\nbody\r\n";
        assert!(canonicalize_signed_headers(raw, &["from", "to", "subject"]).is_none());
    }

    /// Verifiers use the bottom-most instance of a duplicated header. Signing any other
    /// instance yields a signature that can never validate, so a message carrying an
    /// attacker-supplied first `Subject:` must still sign the real one.
    #[test]
    fn dkim_uses_the_bottom_most_instance_of_a_duplicated_header() {
        let raw = b"Subject: attacker\r\n\
To: c@d.com\r\n\
Subject: real\r\n\
\r\nbody\r\n";
        let canon = canonicalize_signed_headers(raw, &["subject"]).expect("headers");
        assert_eq!(String::from_utf8(canon).unwrap(), "subject:real\r\n");
    }

    /// Folding whitespace must be undone before canonicalization, and the relaxation must
    /// collapse it to a single space.
    #[test]
    fn dkim_unfolds_continuation_lines() {
        let raw = b"To: Bob <bob@example.com>,\r\n  Carol <carol@example.com>\r\n\r\nbody\r\n";
        let canon = canonicalize_signed_headers(raw, &["to"]).expect("headers");
        assert_eq!(
            String::from_utf8(canon).unwrap(),
            "to:Bob <bob@example.com>, Carol <carol@example.com>\r\n"
        );
    }

    /// The old selector check rejected the dot, so providers with a nested selector could
    /// not sign at all, and it accepted non-ASCII letters, which are not legal in a DNS
    /// label and would corrupt the `DKIM-Signature` header value.
    #[test]
    fn dkim_selector_and_domain_syntax() {
        assert!(is_valid_dkim_selector("default"));
        assert!(is_valid_dkim_selector("s1"));
        assert!(is_valid_dkim_selector("mail.sel1.sel2"));
        assert!(!is_valid_dkim_selector(""));
        assert!(!is_valid_dkim_selector("has space"));
        assert!(!is_valid_dkim_selector("crlf\r\nInjected: x"));
        assert!(!is_valid_dkim_selector("selé"));
        assert!(!is_valid_dkim_selector(".leading"));
        assert!(!is_valid_dkim_selector("trailing."));
        assert!(!is_valid_dkim_selector("-lead"));

        assert!(is_valid_dkim_domain("example.com"));
        assert!(is_valid_dkim_domain("mx1.eu-west-1.amazonses.com"));
        assert!(!is_valid_dkim_domain(""));
        assert!(!is_valid_dkim_domain("example.com."));
        assert!(!is_valid_dkim_domain("bad domain.com"));
        assert!(!is_valid_dkim_domain("evil\r\nX-Injected: 1"));
        assert!(!is_valid_dkim_domain("exämple.com"));
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

    #[test]
    fn test_smtp_build_message_with_attachments() {
        use vespetrel_core::message::ComposedAttachment;

        let config = SmtpConfig::new("smtp.example.com", 587, "alice", "token");
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
            subject: "Quarterly Report".into(),
            body_text: "Please find report attached.".into(),
            body_html: None,
            in_reply_to: None,
            references: Vec::new(),
            attachments: vec![ComposedAttachment {
                filename: "report.pdf".into(),
                content_type: "application/pdf".into(),
                data: b"%PDF-1.4 dummy pdf content".to_vec(),
            }],
        };

        let formatted = client.build_rfc822(&msg).unwrap();
        let raw_str = String::from_utf8_lossy(&formatted);
        assert!(raw_str.contains("report.pdf"));
        assert!(raw_str.contains("application/pdf"));
        assert!(raw_str.contains("multipart/mixed"));
        assert!(raw_str.contains("Message-ID:"));
    }
}
