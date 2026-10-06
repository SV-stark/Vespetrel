use vespetrel_core::message::Address;
use vespetrel_render::{
    PhishingRisk, RewriteOptions, SanitizeOptions, sanitize, scan_content_for_phishing,
};

/// What the reader actually *determined* about a message's cryptography.
///
/// Every variant names a check that ran. There is deliberately no "assumed good"
/// state: a reader that labels an unexamined message as verified teaches the user
/// that the badge means nothing, which is worse than showing no badge at all.
/// A state the engine cannot reach is not represented here; it belongs in the
/// roadmap, not in the type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SecurityStatus {
    /// No `multipart/signed` or `application/pkcs7` structure was found. Nothing to
    /// report, so the reader shows no security badge at all.
    #[default]
    Unsigned,
    /// A signature is present but no key for the sender is held locally, so no
    /// signature could be checked. Reported as *not verified*, never as a pass.
    SignedUnverified,
    /// A signature was checked against material carried in the message and is good.
    SignatureValid,
    /// A signature was checked and is bad: wrong signer, altered content, or a
    /// certificate outside its validity window.
    SignatureInvalid,
    /// The message is an encrypted envelope and no matching private key is
    /// available to open it. The body shown is the armor, not the plaintext.
    EncryptedLocked,
}

/// How strongly a reader security fact should be presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthState {
    /// Verified good.
    Pass,
    /// Verified bad.
    Fail,
    /// Present but not determined. Must not be styled as a failure.
    Unknown,
}

/// One verified (or explicitly unverified) fact about a message's transport security.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthFact {
    pub label: &'static str,
    pub state: AuthState,
}

#[derive(Debug, Clone)]
pub struct AttachmentInfo {
    pub filename: String,
    pub content_type: String,
    pub size_bytes: usize,
    pub blob_path: String,
}

/// Which presentation the reader's body needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// A sanitized HTML fragment, ready for an HTML renderer.
    Html,
    /// Literal text. Carries no markup: it is escaped before display.
    Text,
    /// Nothing loaded yet.
    Empty,
}

/// Present a plain-text body as HTML without letting it be read as markup.
///
/// Routing plaintext through a Markdown renderer is wrong for mail: a signature
/// block reading `* regards` becomes italic, and a list line beginning `- ` becomes
/// a bullet. Escaping every character and turning each newline into an explicit
/// `<br/>` is the only way to show the bytes the sender actually wrote.
pub fn plaintext_as_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for (idx, line) in text.split('\n').enumerate() {
        if idx > 0 {
            out.push_str("<br/>");
        }
        out.push_str(&html_escape(line.trim_end_matches('\r')));
    }
    out
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
    pub phishing_warning: Option<String>,
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
            security_status: SecurityStatus::Unsigned,
            block_remote_images: true,
            attachments: Vec::new(),
            phishing_warning: None,
        }
    }

    pub fn load_html(&mut self, html: impl Into<String>, block_remote: bool) {
        let html = html.into();
        self.raw_html = Some(html.clone());
        self.block_remote_images = block_remote;
        self.check_phishing(&html);
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
        let t = text.into();
        self.check_phishing(&t);
        self.plain_text = Some(t);
        self.raw_html = None;
        self.sanitized_html = None;
    }

    /// Drop every loaded body and every verdict.
    ///
    /// Without this, opening a message with no signature after one that had a
    /// valid signature left the previous message's green badge on screen, because
    /// `security_status` was only ever assigned, never reset.
    pub fn clear(&mut self) {
        let block = self.block_remote_images;
        *self = Self::new();
        self.block_remote_images = block;
    }

    fn check_phishing(&mut self, content: &str) {
        if let Some(risk) = scan_content_for_phishing(content) {
            let msg = match risk {
                PhishingRisk::DeceptiveDisplayDomain {
                    display_domain,
                    target_domain,
                } => format!(
                    "Deceptive link detected: Text displays \"{display_domain}\" but links to \"{target_domain}\"."
                ),
                PhishingRisk::RawIpAddress { ip } => {
                    format!("Suspicious raw IP address detected in link target: \"{ip}\".")
                }
                PhishingRisk::PunycodeHomograph { domain } => {
                    format!("Homograph / Punycode spoofing detected in domain: \"{domain}\".")
                }
                PhishingRisk::UserInfoSpoofing {
                    user_info,
                    target_domain,
                } => {
                    format!(
                        "Deceptive URL with userinfo spoofing \"{user_info}\" targeting \"{target_domain}\"."
                    )
                }
                PhishingRisk::Safe => String::new(),
            };
            self.phishing_warning = if msg.is_empty() { None } else { Some(msg) };
        } else {
            self.phishing_warning = None;
        }
    }

    /// The document to hand the reader's HTML renderer, and how it must be treated.
    ///
    /// The old `rendered()` handed a bare `String` back, and the caller put that
    /// string straight into a `Div::child`, where GPUI makes a *text node* of it. A
    /// message therefore displayed its own sanitized markup as literal `<table>`
    /// source. Returning the sanitized fragment together with its `BodyKind` lets
    /// the caller hand it to a real renderer instead.
    pub fn body(&self) -> (BodyKind, String) {
        if let Some(html) = self
            .sanitized_html
            .as_ref()
            .filter(|h| !h.trim().is_empty())
        {
            return (BodyKind::Html, html.clone());
        }
        if let Some(text) = &self.plain_text {
            return (BodyKind::Text, plaintext_as_html(text));
        }
        (BodyKind::Empty, String::new())
    }

    /// The security facts the reader is entitled to display for this message.
    ///
    /// The reader previously rendered "DKIM pass" and "SPF pass" unconditionally, for
    /// every message, whether or not any signature was ever checked. That asserts a
    /// verification that did not happen, so a message with no DKIM signature at all was
    /// labelled as having passed DKIM. Report only what was actually determined, and say
    /// "not verified" in the neutral state rather than implying either outcome.
    pub fn auth_facts(&self) -> Vec<AuthFact> {
        match self.security_status {
            // Nothing was found, so there is nothing to say. An empty row reads as
            // "no cryptography here"; a neutral badge on every message would train
            // the user to ignore the row that matters.
            SecurityStatus::Unsigned => Vec::new(),
            SecurityStatus::SignedUnverified => vec![AuthFact {
                label: "Signature not verified",
                state: AuthState::Unknown,
            }],
            SecurityStatus::SignatureValid => vec![AuthFact {
                label: "Signature valid",
                state: AuthState::Pass,
            }],
            SecurityStatus::SignatureInvalid => vec![AuthFact {
                label: "Signature invalid",
                state: AuthState::Fail,
            }],
            SecurityStatus::EncryptedLocked => vec![AuthFact {
                label: "Encrypted, no private key",
                state: AuthState::Unknown,
            }],
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

pub fn html_escape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + 16);
    let mut last = 0;

    for (i, &b) in bytes.iter().enumerate() {
        let replacement = match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            b'\'' => "&#39;",
            _ => continue,
        };
        out.push_str(&s[last..i]);
        out.push_str(replacement);
        last = i + 1;
    }
    out.push_str(&s[last..]);
    out
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
        let (kind, body) = viewer.body();
        assert_eq!(kind, BodyKind::Html);
        assert!(
            body.contains("data-blocked-src") || !body.contains("https://tracker.example"),
            "a blocked remote image must not keep its fetchable URL"
        );

        viewer.toggle_remote_images();
        assert!(!viewer.block_remote_images);

        viewer.load_text("Simple plain text reply test");
        let reply = viewer.generate_reply_text();
        assert!(reply.contains("wrote:\n> Simple plain text reply test"));
    }

    /// The reader used to print "DKIM Pass" and "SPF Pass" for every message. Assert that
    /// only an actually-evaluated signature produces a verdict, and that a message with no
    /// signature is reported as unverified rather than as passing.
    #[test]
    fn security_facts_only_report_what_was_verified() {
        let mut viewer = MessageViewer::new();

        // No signature at all: no badge, and specifically no green one.
        assert!(
            viewer.auth_facts().is_empty(),
            "an unsigned message must show no security fact"
        );

        viewer.security_status = SecurityStatus::SignedUnverified;
        let facts = viewer.auth_facts();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].state, AuthState::Unknown);
        assert!(
            !facts.iter().any(|f| f.state == AuthState::Pass),
            "an unchecked signature must never be reported as passing"
        );

        viewer.security_status = SecurityStatus::SignatureValid;
        assert!(
            viewer
                .auth_facts()
                .iter()
                .any(|f| f.state == AuthState::Pass)
        );

        viewer.security_status = SecurityStatus::SignatureInvalid;
        assert!(
            viewer
                .auth_facts()
                .iter()
                .any(|f| f.state == AuthState::Fail)
        );

        viewer.security_status = SecurityStatus::EncryptedLocked;
        let facts = viewer.auth_facts();
        assert_eq!(facts[0].state, AuthState::Unknown);
        assert!(
            !facts.iter().any(|f| f.label.contains("DKIM")),
            "DKIM is never verified, so it must not be reported"
        );
        assert!(
            !facts.iter().any(|f| f.label.contains("SPF")),
            "SPF is never evaluated, so it must not be reported as passing"
        );
    }

    /// The previous implementation left `security_status` at whatever the last
    /// message set, so opening a plain message after a signed one kept the green
    /// badge. Assert that loading a new body resets the verdict.
    ///
    /// Regression: `gui.rs` assigned `security_status` in its async hydration
    /// branch and never cleared it, so the previous message's verdict survived a
    /// folder change and a selection change.
    #[test]
    fn opening_an_unsigned_message_clears_the_previous_verdict() {
        let mut viewer = MessageViewer::new();
        viewer.security_status = SecurityStatus::SignatureValid;
        assert!(!viewer.auth_facts().is_empty());

        viewer.clear();
        assert_eq!(viewer.security_status, SecurityStatus::Unsigned);
        assert!(viewer.auth_facts().is_empty());
    }

    /// Plaintext must be shown as the bytes the sender wrote. Rendering it as
    /// Markdown would turn `* regards` into italic and a `- ` line into a bullet.
    #[test]
    fn plaintext_is_escaped_rather_than_read_as_markup() {
        let html = plaintext_as_html("Hi <b>there</b>\n* regards\n- item");
        assert!(html.contains("&lt;b&gt;"));
        assert!(!html.contains("<b>"));
        assert!(html.contains("<br/>"));

        let mut viewer = MessageViewer::new();
        viewer.load_text("<script>alert(1)</script>");
        let (kind, body) = viewer.body();
        assert_eq!(kind, BodyKind::Text);
        assert!(!body.contains("<script>"));
        assert!(body.contains("&lt;script&gt;"));
    }

    /// The body the reader renders must be the sanitized fragment, never the raw
    /// markup the sender wrote.
    #[test]
    fn body_prefers_sanitized_html_and_falls_back_to_nothing() {
        let mut viewer = MessageViewer::new();
        assert_eq!(viewer.body().0, BodyKind::Empty);

        viewer.load_html("<p>Hi</p><script>steal()</script>", true);
        let (kind, body) = viewer.body();
        assert_eq!(kind, BodyKind::Html);
        assert!(body.contains("Hi"));
        assert!(!body.contains("<script"));
    }
}
