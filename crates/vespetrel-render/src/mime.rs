use mail_parser::{MessageParser, MimeHeaders};

/// Parsed mail view model for rendering §5.1
#[derive(Debug, Clone)]
pub struct ParsedMail {
    pub subject: Option<String>,
    pub from: Option<String>,
    pub to: Vec<String>,
    pub date: Option<String>,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub attachments: Vec<AttachmentInfo>,
}

#[derive(Debug, Clone)]
pub struct AttachmentInfo {
    pub filename: String,
    pub content_type: String,
    pub size: usize,
}

impl ParsedMail {
    pub fn parse(raw: &[u8]) -> Option<Self> {
        let msg = MessageParser::default().parse(raw)?;
        let subject = msg.subject().map(|s| s.to_string());
        let from = msg.from().and_then(|a| a.first()).map(|addr| {
            let name = addr.name.as_deref().unwrap_or("");
            let email = addr.address.as_deref().unwrap_or("");
            if name.is_empty() {
                email.to_string()
            } else {
                format!("{name} <{email}>")
            }
        });
        let to = msg
            .to()
            .map(|addrs| {
                addrs
                    .iter()
                    .map(|a| a.address.as_deref().unwrap_or("").to_string())
                    .collect()
            })
            .unwrap_or_default();
        let date = msg.date().map(|d| d.to_rfc3339());
        let text_body = msg.body_text(0).map(|cow| cow.into_owned());
        let html_body = msg.body_html(0).map(|cow| cow.into_owned());

        let attachments = msg
            .attachments()
            .map(|a| AttachmentInfo {
                filename: a.attachment_name().unwrap_or("untitled").to_string(),
                content_type: a
                    .content_type()
                    .map(|ct| {
                        format!(
                            "{}/{}",
                            ct.c_type,
                            ct.c_subtype.as_deref().unwrap_or("octet-stream")
                        )
                    })
                    .unwrap_or_else(|| "application/octet-stream".into()),
                size: a.contents().len(),
            })
            .collect::<Vec<_>>();

        Some(Self {
            subject,
            from,
            to,
            date,
            text_body,
            html_body,
            attachments,
        })
    }

    /// Best body for rendering: sanitized HTML if present, else text
    pub fn render_body(&self) -> String {
        if let Some(html) = &self.html_body {
            // Caller should use crate::sanitize
            html.clone()
        } else if let Some(text) = &self.text_body {
            // Escape and wrap in <pre>
            format!("<pre>{}</pre>", html_escape(text))
        } else {
            "<p><em>(no content)</em></p>".into()
        }
    }
}

/// High-throughput SIMD Quoted-Printable byte decoder using memchr
pub fn decode_quoted_printable_simd(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut cursor = 0;

    while cursor < input.len() {
        if let Some(pos) = memchr::memchr(b'=', &input[cursor..]) {
            let eq_idx = cursor + pos;
            out.extend_from_slice(&input[cursor..eq_idx]);
            cursor = eq_idx;

            if cursor + 1 < input.len()
                && (input[cursor + 1] == b'\r' || input[cursor + 1] == b'\n')
            {
                // Soft line break =\r\n or =\n
                cursor += 2;
                if cursor <= input.len()
                    && input[cursor - 1] == b'\r'
                    && cursor < input.len()
                    && input[cursor] == b'\n'
                {
                    cursor += 1;
                }
            } else if cursor + 2 < input.len() {
                let h1 = hex_val(input[cursor + 1]);
                let h2 = hex_val(input[cursor + 2]);
                if let (Some(v1), Some(v2)) = (h1, h2) {
                    out.push((v1 << 4) | v2);
                    cursor += 3;
                } else {
                    out.push(b'=');
                    cursor += 1;
                }
            } else {
                out.push(b'=');
                cursor += 1;
            }
        } else {
            out.extend_from_slice(&input[cursor..]);
            break;
        }
    }
    out
}

#[inline(always)]
fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn html_escape(s: &str) -> String {
    let bytes = s.as_bytes();
    if memchr::memchr3(b'&', b'<', b'>', bytes).is_none() {
        return s.to_string();
    }

    let mut out = String::with_capacity(s.len() + 16);
    let mut cursor = 0;
    while cursor < bytes.len() {
        if let Some(pos) = memchr::memchr3(b'&', b'<', b'>', &bytes[cursor..]) {
            let special_idx = cursor + pos;
            if let Ok(clean) = simdutf8::basic::from_utf8(&bytes[cursor..special_idx]) {
                out.push_str(clean);
            }
            match bytes[special_idx] {
                b'&' => out.push_str("&amp;"),
                b'<' => out.push_str("&lt;"),
                b'>' => out.push_str("&gt;"),
                _ => {}
            }
            cursor = special_idx + 1;
        } else {
            if let Ok(clean) = simdutf8::basic::from_utf8(&bytes[cursor..]) {
                out.push_str(clean);
            }
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsed_mail_rendering_and_extraction() {
        let raw = b"From: Alice <alice@example.com>\r\nTo: Bob <bob@example.com>\r\nSubject: Test Email\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<h1>Hello Bob</h1>";
        let parsed = ParsedMail::parse(raw).unwrap();
        assert_eq!(parsed.subject.as_deref(), Some("Test Email"));
        assert_eq!(parsed.from.as_deref(), Some("Alice <alice@example.com>"));
        assert_eq!(parsed.to, vec!["bob@example.com"]);
        assert!(parsed.html_body.is_some());
        assert_eq!(parsed.render_body(), "<h1>Hello Bob</h1>");

        let raw_text = b"From: Alice <alice@example.com>\r\nTo: Bob <bob@example.com>\r\nSubject: Text Email\r\nContent-Type: text/plain\r\n\r\nPlain text message <test>";
        let parsed_text = ParsedMail::parse(raw_text).unwrap();
        assert_eq!(
            parsed_text.text_body.as_deref(),
            Some("Plain text message <test>")
        );
        assert!(parsed_text.render_body().contains("Plain text message"));
    }

    #[test]
    fn test_decode_quoted_printable_simd() {
        let input = b"Hello=20World=21=0D=0AThis is a soft line break=\r\ncontinuation.";
        let decoded = decode_quoted_printable_simd(input);
        assert_eq!(
            String::from_utf8_lossy(&decoded),
            "Hello World!\r\nThis is a soft line breakcontinuation."
        );
    }

    #[test]
    fn test_html_escape_simd() {
        let raw = "<script>alert('hello & welcome');</script>";
        let escaped = html_escape(raw);
        assert_eq!(
            escaped,
            "&lt;script&gt;alert('hello &amp; welcome');&lt;/script&gt;"
        );
    }
}
