use mail_parser::MessageParser;

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
            if name.is_empty() { email.to_string() } else { format!("{name} <{email}>") }
        });
        let to = msg.to().map(|addrs| addrs.iter().map(|a| a.address.as_deref().unwrap_or("").to_string()).collect()).unwrap_or_default();
        let date = msg.date().map(|d| d.to_rfc3339());
        let text_body = msg.body_text(0).map(|cow| cow.into_owned());
        let html_body = msg.body_html(0).map(|cow| cow.into_owned());

        let attachments = msg.attachments().map(|iter| {
            iter.map(|a| AttachmentInfo {
                filename: a.attachment_name().unwrap_or("untitled").to_string(),
                content_type: a.content_type().map(|ct| format!("{}/{}", ct.c_type, ct.c_subtype)).unwrap_or("application/octet-stream".into()),
                size: a.contents().len(),
            }).collect()
        }).unwrap_or_default();

        Some(Self { subject, from, to, date, text_body, html_body, attachments })
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

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
