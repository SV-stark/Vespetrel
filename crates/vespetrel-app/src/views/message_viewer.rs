use vespetrel_render::{sanitize, SanitizeOptions, RewriteOptions};

pub struct MessageViewer {
    pub raw_html: Option<String>,
    pub plain_text: Option<String>,
    pub sanitized_html: Option<String>,
}

impl MessageViewer {
    pub fn new() -> Self { Self { raw_html: None, plain_text: None, sanitized_html: None } }

    pub fn load_html(&mut self, html: impl Into<String>, block_remote: bool) {
        let html = html.into();
        self.raw_html = Some(html.clone());
        let opts = SanitizeOptions { rewrite: RewriteOptions { block_remote_images: block_remote, rewrite_cid: true } };
        self.sanitized_html = sanitize(&html, &opts).ok();
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
}

impl Default for MessageViewer { fn default() -> Self { Self::new() } }

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

// #[cfg(feature = "gpui")]
// mod wry_bridge { /* wry WebView inside gpui element, blob:// protocol for cid: */ }
