//! Vespetrel Render - HTML sanitization + tracker stripping pipeline §5.2

pub mod auth_badge;
pub mod cleaner;
pub mod html;
pub mod mdn;
pub mod mime;
pub mod unsubscribe;

pub use auth_badge::{AuthBadgeParser, AuthStatus, EmailSecuritySummary};
pub use cleaner::{
    PhishingRisk, analyze_phishing_risk, clean_tracking_url, scan_content_for_phishing,
};
pub use html::{RewriteOptions, SanitizeOptions, render_sandboxed_document, sanitize};
pub use mdn::{DispositionType, MdnEngine, MdnRequest};
pub use mime::ParsedMail;
pub use unsubscribe::{ListUnsubscribe, UnsubscribeAction};
