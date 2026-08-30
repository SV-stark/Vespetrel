//! Vespetrel Render - HTML sanitization + tracker stripping pipeline §5.2

pub mod html;
pub mod mime;

pub use html::{sanitize, RewriteOptions, SanitizeOptions};
pub use mime::ParsedMail;
