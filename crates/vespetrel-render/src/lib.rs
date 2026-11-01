//! Vespetrel Render - HTML sanitization + tracker stripping pipeline §5.2

pub mod auth_badge;
pub mod cleaner;
pub mod html;
pub mod mdn;
pub mod mime;
pub mod preview;
pub mod remote_image_proxy;
pub mod unsubscribe;

pub use auth_badge::{AuthBadgeParser, AuthStatus, EmailSecuritySummary};
pub use cleaner::{
    PhishingRisk, analyze_phishing_risk, clean_tracking_url, scan_content_for_phishing,
};
pub use html::{RewriteOptions, SanitizeOptions, render_sandboxed_document, sanitize};
pub use mdn::{DispositionType, MdnEngine, MdnRequest};
pub use mime::ParsedMail;
pub use preview::{
    AttachmentKind, AttachmentPreviewData, detect_attachment_kind, format_file_size,
    generate_preview,
};
pub use remote_image_proxy::{ProxyError, RemoteImageProxy};
pub use unsubscribe::{ListUnsubscribe, UnsubscribeAction};
