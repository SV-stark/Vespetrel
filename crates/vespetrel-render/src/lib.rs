//! Vespetrel Render - HTML sanitization + tracker stripping pipeline §5.2

pub mod html;
pub mod mime;

pub use html::{RewriteOptions, SanitizeOptions, render_sandboxed_document, sanitize};
pub use mime::ParsedMail;
