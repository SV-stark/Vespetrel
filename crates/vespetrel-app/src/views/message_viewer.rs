use vespetrel_core::message::Address;
use vespetrel_render::{RewriteOptions, SanitizeOptions, sanitize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityStatus {
    Unencrypted,
    PgpSignedValid,
    PgpEncryptedAndSigned,
    SmimeValid,
    DkimPass,
    DkimFail,
}

#[derive(Debug, Clone)]
pub struct AttachmentInfo {
    pub filename: String,
    pub content_type: String,
    pub size_bytes: usize,
    pub blob_path: String,
}

pub struct MessageViewer {
    pub raw_html: Option<String>,
    pub plain_text: Option<String>,
    pub sanitized_html: Option<String>,
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub subject: Option<String>,
    pub sent_at: Option<chrono::DateTime<chrono::Utc>>,
    pub security_status: SecurityStatus,
    pub block_remote_images: bool,
    pub attachments: Vec<AttachmentInfo>,
}

impl MessageViewer {
    pub fn new() -> Self {
        Self {
            raw_html: None,
            plain_text: None,
            sanitized_html: None,
            from: None,
            to: Vec::new(),
            cc: Vec::new(),
            subject: None,
            sent_at: None,
            security_status: SecurityStatus::Unencrypted,
            block_remote_images: true,
            attachments: Vec::new(),
        }
    }

    pub fn load_html(&mut self, html: impl Into<String>, block_remote: bool) {
        let html = html.into();
        self.raw_html = Some(html.clone());
        self.block_remote_images = block_remote;
        self.re_sanitize();
    }

    pub fn toggle_remote_images(&mut self) {
        self.block_remote_images = !self.block_remote_images;
        self.re_sanitize();
    }

    fn re_sanitize(&mut self) {
        if let Some(html) = &self.raw_html {
            let opts = SanitizeOptions {
                rewrite: RewriteOptions {
                    block_remote_images: self.block_remote_images,
                    rewrite_cid: true,
                },
            };
            self.sanitized_html = sanitize(html, &opts).ok();
        }
    }

    pub fn load_text(&mut self, text: impl Into<String>) {
        self.plain_text = Some(text.into());
        // Plaintext rendered as native GPUI Markdown, not via wry
        self.raw_html = None;
        self.sanitized_html = None;
    }

    pub fn rendered(&self) -> String {
        if let Some(sanitized) = &self.sanitized_html {
            sanitized.clone()
        } else if let Some(text) = &self.plain_text {
            format!("<pre>{}</pre>", html_escape(text))
        } else {
            "<p><em>Select a message</em></p>".into()
        }
    }

    /// Helper to generate Reply template
    pub fn generate_reply_text(&self) -> String {
        let quote = self.plain_text.as_deref().unwrap_or("");
        let sender = self
            .from
            .as_ref()
            .map(|a| a.to_string())
            .unwrap_or_default();
        format!(
            "\n\nOn {}, {} wrote:\n> {}",
            self.sent_at.map(|d| d.to_rfc2822()).unwrap_or_default(),
            sender,
            quote.replace('\n', "\n> ")
        )
    }
}

impl Default for MessageViewer {
    fn default() -> Self {
        Self::new()
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_viewer_security_and_render() {
        let mut viewer = MessageViewer::new();
        viewer.from = Some(Address {
            name: Some("Alice".into()),
            email: "alice@example.com".into(),
        });
        viewer.load_html(
            "<p>Hello <img src=\"https://tracker.example/pixel.gif\"></p>",
            true,
        );
        assert!(
            viewer.rendered().contains("data-blocked-src")
                || !viewer.rendered().contains("https://tracker.example")
        );

        viewer.toggle_remote_images();
        assert!(!viewer.block_remote_images);

        viewer.load_text("Simple plain text reply test");
        let reply = viewer.generate_reply_text();
        assert!(reply.contains("wrote:\n> Simple plain text reply test"));
    }
}
