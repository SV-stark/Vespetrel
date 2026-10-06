//! Vespetrel Crypto - OpenPGP (rPGP RFC9580), S/MIME, OAuth2 PKCE, keyring §4.5 + §5.3

pub mod hardware;
pub mod keyring;
pub mod oauth;
pub mod pgp;
pub mod smime;

/// Constant-time byte-slice equality.
///
/// `ring::constant_time::verify_slices_are_equal` is deprecated and returns `Err` for a
/// length mismatch, so both facts are folded into a plain `bool` here. Used for comparing
/// PIN verifiers and OAuth CSRF state, where a short-circuiting `==` leaks the matching
/// prefix length.
#[allow(deprecated)] // ring exposes no non-deprecated constant-time compare yet.
pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    ring::constant_time::verify_slices_are_equal(a, b).is_ok()
}

pub use hardware::{HardwareSecurityKey, HardwareTokenType};
pub use keyring::{Keyring, KeyringError};
pub use oauth::{LoopbackListener, OAuth2Config, OAuth2Engine};
pub use pgp::{AutocryptHeader, PgpEngine, PgpError};
pub use smime::SmimeEngine;
