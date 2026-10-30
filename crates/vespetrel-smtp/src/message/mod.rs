//! Thunderbird-grade RFC 5322 and MIME message building with rich headers and multipart structures.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::SmtpResult;
use vespetrel_core::message::{Address, ComposedAttachment, ComposedMessage};

/// Thunderbird message priority levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Priority {
    Highest,
    High,
    #[default]
    Normal,
    Low,
    Lowest,
}

impl Priority {
    /// Formats as standard `X-Priority` header value (1-5).
    pub fn as_x_priority_str(&self) -> &'static str {
        match self {
            Self::Highest => "1 (Highest)",
            Self::High => "2 (High)",
            Self::Normal => "3 (Normal)",
            Self::Low => "4 (Low)",
            Self::Lowest => "5 (Lowest)",
        }
    }

    /// Formats as standard `Importance` header value (high/normal/low).
    pub fn as_importance_str(&self) -> &'static str {
        match self {
            Self::Highest | Self::High => "high",
            Self::Normal => "normal",
            Self::Low | Self::Lowest => "low",
        }
    }
}

/// Thunderbird message sensitivity (RFC 2156 / RFC 4021).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sensitivity {
    Personal,
    Private,
    CompanyConfidential,
}

impl Sensitivity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Personal => "Personal",
            Self::Private => "Private",
            Self::CompanyConfidential => "Company-Confidential",
        }
    }
}

/// An inline attachment (e.g. embedded image in HTML with `cid:...`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineAttachment {
    pub content_id: String,
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

/// Rich Thunderbird-style message structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmtpMessage {
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    /// Bcc recipients: sent in SMTP envelope, stripped from RFC 5322 headers for privacy.
    pub bcc: Vec<Address>,
    pub reply_to: Option<Address>,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<ComposedAttachment>,
    pub inline_attachments: Vec<InlineAttachment>,

    // Thunderbird rich headers:
    pub priority: Option<Priority>,
    pub sensitivity: Option<Sensitivity>,
    pub organization: Option<String>,
    pub read_receipt_to: Option<String>,
    pub delivery_receipt_to: Option<String>,
    pub autocrypt_header: Option<String>,
    pub custom_headers: Vec<(String, String)>,
}

impl Default for SmtpMessage {
    fn default() -> Self {
        Self {
            from: Address {
                name: None,
                email: String::new(),
            },
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: None,
            subject: String::new(),
            body_text: String::new(),
            body_html: None,
            in_reply_to: None,
            references: Vec::new(),
            attachments: Vec::new(),
            inline_attachments: Vec::new(),
            priority: None,
            sensitivity: None,
            organization: None,
            read_receipt_to: None,
            delivery_receipt_to: None,
            autocrypt_header: None,
            custom_headers: Vec::new(),
        }
    }
}

impl From<ComposedMessage> for SmtpMessage {
    fn from(msg: ComposedMessage) -> Self {
        Self {
            from: msg.from,
            to: msg.to,
            cc: msg.cc,
            bcc: msg.bcc,
            reply_to: None,
            subject: msg.subject,
            body_text: msg.body_text,
            body_html: msg.body_html,
            in_reply_to: msg.in_reply_to,
            references: msg.references,
            attachments: msg.attachments,
            inline_attachments: Vec::new(),
            priority: None,
            sensitivity: None,
            organization: None,
            read_receipt_to: None,
            delivery_receipt_to: None,
            autocrypt_header: None,
            custom_headers: Vec::new(),
        }
    }
}

impl SmtpMessage {
    pub fn new(from: Address, subject: impl Into<String>) -> Self {
        Self {
            from,
            subject: subject.into(),
            ..Default::default()
        }
    }

    pub fn to(mut self, addr: Address) -> Self {
        self.to.push(addr);
        self
    }

    pub fn cc(mut self, addr: Address) -> Self {
        self.cc.push(addr);
        self
    }

    pub fn bcc(mut self, addr: Address) -> Self {
        self.bcc.push(addr);
        self
    }

    pub fn body_text(mut self, text: impl Into<String>) -> Self {
        self.body_text = text.into();
        self
    }

    pub fn body_html(mut self, html: impl Into<String>) -> Self {
        self.body_html = Some(html.into());
        self
    }

    pub fn priority(mut self, priority: Priority) -> Self {
        self.priority = Some(priority);
        self
    }

    pub fn request_read_receipt(mut self, email: impl Into<String>) -> Self {
        self.read_receipt_to = Some(email.into());
        self
    }

    pub fn request_delivery_receipt(mut self, email: impl Into<String>) -> Self {
        self.delivery_receipt_to = Some(email.into());
        self
    }

    pub fn organization(mut self, org: impl Into<String>) -> Self {
        self.organization = Some(org.into());
        self
    }

    pub fn autocrypt(mut self, header_val: impl Into<String>) -> Self {
        self.autocrypt_header = Some(header_val.into());
        self
    }

    pub fn attach(
        mut self,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        data: Vec<u8>,
    ) -> Self {
        self.attachments.push(ComposedAttachment {
            filename: filename.into(),
            content_type: content_type.into(),
            data,
        });
        self
    }

    pub fn attach_inline(
        mut self,
        cid: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        data: Vec<u8>,
    ) -> Self {
        self.inline_attachments.push(InlineAttachment {
            content_id: cid.into(),
            filename: filename.into(),
            content_type: content_type.into(),
            data,
        });
        self
    }

    /// Extract all envelope recipients (To + Cc + Bcc).
    pub fn envelope_recipients(&self) -> Vec<String> {
        let mut recipients = Vec::with_capacity(self.to.len() + self.cc.len() + self.bcc.len());
        for addr in self.to.iter().chain(&self.cc).chain(&self.bcc) {
            recipients.push(addr.email.trim().to_string());
        }
        recipients
    }

    /// Build raw RFC 5322 MIME wire bytes for transmission.
    /// Notice: Bcc recipients are completely stripped from headers!
    pub fn build_rfc5322_bytes(&self, fqdn: &str) -> SmtpResult<Vec<u8>> {
        let mut headers = Vec::new();

        // 1. Message-ID
        let domain = if let Some(pos) = self.from.email.find('@') {
            &self.from.email[pos + 1..]
        } else {
            fqdn
        };
        let msg_id = format!("<{}@{}>", Uuid::new_v4(), domain);
        headers.push(format!("Message-ID: {msg_id}"));

        // 2. Date
        let date_str = Utc::now().to_rfc2822();
        headers.push(format!("Date: {date_str}"));

        // 3. From
        headers.push(format!("From: {}", format_address_rfc2047(&self.from)));

        // 4. Reply-To
        if let Some(reply_to) = &self.reply_to {
            headers.push(format!("Reply-To: {}", format_address_rfc2047(reply_to)));
        }

        // 5. To
        if !self.to.is_empty() {
            let to_list: Vec<String> = self.to.iter().map(format_address_rfc2047).collect();
            headers.push(format!("To: {}", to_list.join(", ")));
        }

        // 6. Cc
        if !self.cc.is_empty() {
            let cc_list: Vec<String> = self.cc.iter().map(format_address_rfc2047).collect();
            headers.push(format!("Cc: {}", cc_list.join(", ")));
        }

        // NOTE: Bcc is explicitly NOT serialized into RFC 5322 headers to protect recipient privacy!

        // 7. Subject
        let clean_subject = self.subject.replace(['\r', '\n'], " ");
        headers.push(format!(
            "Subject: {}",
            encode_rfc2047_if_needed(&clean_subject)
        ));

        // 8. Threading headers
        if let Some(in_reply_to) = &self.in_reply_to {
            let clean = in_reply_to.replace(['\r', '\n'], " ");
            headers.push(format!("In-Reply-To: {clean}"));
        }
        if !self.references.is_empty() {
            let clean = self.references.join(" ").replace(['\r', '\n'], " ");
            headers.push(format!("References: {clean}"));
        }

        // 9. Thunderbird headers
        if let Some(priority) = self.priority {
            headers.push(format!("X-Priority: {}", priority.as_x_priority_str()));
            headers.push(format!("Importance: {}", priority.as_importance_str()));
        }
        if let Some(sensitivity) = self.sensitivity {
            headers.push(format!("Sensitivity: {}", sensitivity.as_str()));
        }
        if let Some(org) = &self.organization {
            let clean_org = org.replace(['\r', '\n'], " ");
            headers.push(format!(
                "Organization: {}",
                encode_rfc2047_if_needed(&clean_org)
            ));
        }
        if let Some(read_receipt) = &self.read_receipt_to {
            let clean = read_receipt.replace(['\r', '\n'], "");
            headers.push(format!("Disposition-Notification-To: <{clean}>"));
        }
        if let Some(del_receipt) = &self.delivery_receipt_to {
            let clean = del_receipt.replace(['\r', '\n'], "");
            headers.push(format!("Return-Receipt-To: <{clean}>"));
        }
        if let Some(autocrypt) = &self.autocrypt_header {
            let clean = autocrypt.replace(['\r', '\n'], " ");
            headers.push(format!("Autocrypt: {clean}"));
        }

        // 10. User Agent
        headers.push("User-Agent: Vespetrel/0.1.0".to_string());
        headers.push("MIME-Version: 1.0".to_string());

        // 11. Custom headers
        for (k, v) in &self.custom_headers {
            let clean_k = k.replace(['\r', '\n', ':'], "");
            let clean_v = v.replace(['\r', '\n'], " ");
            headers.push(format!("{clean_k}: {clean_v}"));
        }

        // Build MIME hierarchy
        let (content_type, body_bytes) = self.build_mime_tree()?;
        headers.push(format!("Content-Type: {content_type}"));

        let mut output = Vec::new();
        for h in headers {
            output.extend_from_slice(h.as_bytes());
            output.extend_from_slice(b"\r\n");
        }
        output.extend_from_slice(b"\r\n");
        output.extend_from_slice(&body_bytes);

        Ok(output)
    }

    /// Construct MIME body hierarchy based on presence of text, HTML, inline attachments, and regular attachments.
    fn build_mime_tree(&self) -> SmtpResult<(String, Vec<u8>)> {
        // Base content: text/plain, text/html, or multipart/alternative
        let (base_ct, base_bytes) = match (&self.body_html, self.body_text.trim().is_empty()) {
            (Some(html), false) => {
                let alt_boundary = format!("alt-{}", Uuid::new_v4().simple());
                let mut alt_buf = Vec::new();

                // text part
                alt_buf.extend_from_slice(format!("--{alt_boundary}\r\n").as_bytes());
                alt_buf.extend_from_slice(b"Content-Type: text/plain; charset=utf-8\r\n");
                alt_buf.extend_from_slice(b"Content-Transfer-Encoding: quoted-printable\r\n\r\n");
                alt_buf.extend_from_slice(encode_quoted_printable(&self.body_text).as_bytes());
                alt_buf.extend_from_slice(b"\r\n");

                // html part
                alt_buf.extend_from_slice(format!("--{alt_boundary}\r\n").as_bytes());
                alt_buf.extend_from_slice(b"Content-Type: text/html; charset=utf-8\r\n");
                alt_buf.extend_from_slice(b"Content-Transfer-Encoding: quoted-printable\r\n\r\n");
                alt_buf.extend_from_slice(encode_quoted_printable(html).as_bytes());
                alt_buf.extend_from_slice(b"\r\n");

                alt_buf.extend_from_slice(format!("--{alt_boundary}--\r\n").as_bytes());
                (
                    format!("multipart/alternative; boundary=\"{alt_boundary}\""),
                    alt_buf,
                )
            }
            (Some(html), true) => (
                "text/html; charset=utf-8".to_string(),
                encode_quoted_printable(html).into_bytes(),
            ),
            (None, _) => (
                "text/plain; charset=utf-8".to_string(),
                encode_quoted_printable(&self.body_text).into_bytes(),
            ),
        };

        // If inline attachments exist, wrap base content in multipart/related
        let (rel_ct, rel_bytes) = if !self.inline_attachments.is_empty() {
            let rel_boundary = format!("rel-{}", Uuid::new_v4().simple());
            let mut rel_buf = Vec::new();

            // base content part
            rel_buf.extend_from_slice(format!("--{rel_boundary}\r\n").as_bytes());
            rel_buf.extend_from_slice(format!("Content-Type: {base_ct}\r\n\r\n").as_bytes());
            rel_buf.extend_from_slice(&base_bytes);
            rel_buf.extend_from_slice(b"\r\n");

            // inline parts
            for inline in &self.inline_attachments {
                rel_buf.extend_from_slice(format!("--{rel_boundary}\r\n").as_bytes());
                rel_buf.extend_from_slice(
                    format!(
                        "Content-Type: {}\r\nContent-Transfer-Encoding: base64\r\nContent-ID: <{}>\r\nContent-Disposition: inline; filename=\"{}\"\r\n\r\n",
                        inline.content_type, inline.content_id, inline.filename
                    )
                    .as_bytes(),
                );
                rel_buf.extend_from_slice(encode_base64_lines(&inline.data).as_bytes());
                rel_buf.extend_from_slice(b"\r\n");
            }
            rel_buf.extend_from_slice(format!("--{rel_boundary}--\r\n").as_bytes());
            (
                format!("multipart/related; boundary=\"{rel_boundary}\""),
                rel_buf,
            )
        } else {
            (base_ct, base_bytes)
        };

        // If regular attachments exist, wrap in multipart/mixed
        if !self.attachments.is_empty() {
            let mixed_boundary = format!("mixed-{}", Uuid::new_v4().simple());
            let mut mixed_buf = Vec::new();

            // related/alternative/text part
            mixed_buf.extend_from_slice(format!("--{mixed_boundary}\r\n").as_bytes());
            mixed_buf.extend_from_slice(format!("Content-Type: {rel_ct}\r\n\r\n").as_bytes());
            mixed_buf.extend_from_slice(&rel_bytes);
            mixed_buf.extend_from_slice(b"\r\n");

            // attachment parts
            for att in &self.attachments {
                mixed_buf.extend_from_slice(format!("--{mixed_boundary}\r\n").as_bytes());
                mixed_buf.extend_from_slice(
                    format!(
                        "Content-Type: {}\r\nContent-Transfer-Encoding: base64\r\nContent-Disposition: attachment; filename=\"{}\"\r\n\r\n",
                        att.content_type, att.filename
                    )
                    .as_bytes(),
                );
                mixed_buf.extend_from_slice(encode_base64_lines(&att.data).as_bytes());
                mixed_buf.extend_from_slice(b"\r\n");
            }
            mixed_buf.extend_from_slice(format!("--{mixed_boundary}--\r\n").as_bytes());
            Ok((
                format!("multipart/mixed; boundary=\"{mixed_boundary}\""),
                mixed_buf,
            ))
        } else {
            Ok((rel_ct, rel_bytes))
        }
    }
}

/// Formats an Address with RFC 2047 encoded display name if non-ASCII.
fn format_address_rfc2047(addr: &Address) -> String {
    if let Some(name) = &addr.name {
        let encoded_name = encode_rfc2047_if_needed(name);
        format!("{encoded_name} <{}>", addr.email)
    } else {
        addr.email.clone()
    }
}

/// RFC 2047 encoded-word syntax `=?UTF-8?B?...?=` for non-ASCII strings.
pub fn encode_rfc2047_if_needed(val: &str) -> String {
    if val.is_ascii() && !val.contains(['"', '<', '>', '\r', '\n']) {
        return val.to_string();
    }
    let b64 = B64.encode(val.as_bytes());
    format!("=?UTF-8?B?{b64}?=")
}

/// Encodes binary data into 76-character wrapped base64 lines for MIME.
pub fn encode_base64_lines(data: &[u8]) -> String {
    let raw_b64 = B64.encode(data);
    let mut out = String::with_capacity(raw_b64.len() + (raw_b64.len() / 76) * 2);
    for chunk in raw_b64.as_bytes().chunks(76) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        out.push_str("\r\n");
    }
    out
}

/// Simple Quoted-Printable encoder (RFC 2045 §6.7).
pub fn encode_quoted_printable(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 128);
    let mut line_len = 0;

    for b in text.bytes() {
        if b == b'\r' || b == b'\n' {
            out.push(b as char);
            line_len = 0;
            continue;
        }

        if ((33..=126).contains(&b) && b != b'=') || b == b' ' || b == b'\t' {
            if line_len >= 75 {
                out.push_str("=\r\n");
                line_len = 0;
            }
            out.push(b as char);
            line_len += 1;
        } else {
            if line_len >= 73 {
                out.push_str("=\r\n");
                line_len = 0;
            }
            use std::fmt::Write;
            let _ = write!(out, "={:02X}", b);
            line_len += 3;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_builder_and_bcc_privacy() {
        let msg = SmtpMessage::new(
            Address {
                name: Some("Alice Smith".into()),
                email: "alice@example.com".into(),
            },
            "Important Meeting",
        )
        .to(Address {
            name: Some("Bob".into()),
            email: "bob@example.com".into(),
        })
        .cc(Address {
            name: None,
            email: "carol@example.com".into(),
        })
        .bcc(Address {
            name: None,
            email: "secret_auditor@example.com".into(),
        })
        .priority(Priority::Highest)
        .request_read_receipt("alice@example.com")
        .organization("Vespetrel Org")
        .body_text("Please be on time.")
        .body_html("<b>Please be on time.</b>");

        let recipients = msg.envelope_recipients();
        assert_eq!(
            recipients,
            vec![
                "bob@example.com",
                "carol@example.com",
                "secret_auditor@example.com"
            ]
        );

        let raw_bytes = msg.build_rfc5322_bytes("example.com").unwrap();
        let wire = String::from_utf8(raw_bytes).unwrap();

        // Must contain To and Cc
        assert!(wire.contains("To: Bob <bob@example.com>"));
        assert!(wire.contains("Cc: carol@example.com"));

        // MUST NOT contain Bcc header
        assert!(!wire.contains("secret_auditor@example.com"));
        assert!(!wire.contains("Bcc:"));

        // Must contain Thunderbird headers
        assert!(wire.contains("X-Priority: 1 (Highest)"));
        assert!(wire.contains("Importance: high"));
        assert!(wire.contains("Disposition-Notification-To: <alice@example.com>"));
        assert!(wire.contains("Organization: Vespetrel Org"));
        assert!(wire.contains("multipart/alternative"));
    }

    #[test]
    fn test_inline_attachments_and_mixed() {
        let msg = SmtpMessage::new(
            Address {
                name: None,
                email: "alice@example.com".into(),
            },
            "Report with Logo",
        )
        .to(Address {
            name: None,
            email: "bob@example.com".into(),
        })
        .body_text("Check attached report")
        .body_html("Check attached report <img src=\"cid:logo\">")
        .attach_inline("logo", "logo.png", "image/png", b"fake-png-data".to_vec())
        .attach("report.pdf", "application/pdf", b"fake-pdf-data".to_vec());

        let raw_bytes = msg.build_rfc5322_bytes("example.com").unwrap();
        let wire = String::from_utf8(raw_bytes).unwrap();

        assert!(wire.contains("multipart/mixed"));
        assert!(wire.contains("multipart/related"));
        assert!(wire.contains("Content-ID: <logo>"));
        assert!(wire.contains("filename=\"logo.png\""));
        assert!(wire.contains("filename=\"report.pdf\""));
    }

    #[test]
    fn test_rfc2047_encoding() {
        let utf8_name = "René Descartes";
        let encoded = encode_rfc2047_if_needed(utf8_name);
        assert!(encoded.starts_with("=?UTF-8?B?"));
        assert!(encoded.ends_with("?="));
    }
}
