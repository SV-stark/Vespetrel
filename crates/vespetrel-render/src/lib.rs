//! Vespetrel Render - HTML sanitization + tracker stripping pipeline §5.2

pub mod cleaner;
pub mod html;
pub mod mime;

pub use cleaner::{PhishingRisk, analyze_phishing_risk, clean_tracking_url};
pub use html::{RewriteOptions, SanitizeOptions, render_sandboxed_document, sanitize};
pub use mime::ParsedMail;
