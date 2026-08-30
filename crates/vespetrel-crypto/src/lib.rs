//! Vespetrel Crypto - OpenPGP (rPGP RFC9580), S/MIME, OAuth2 PKCE, keyring §4.5 + §5.3

pub mod keyring;
pub mod oauth;
pub mod pgp;
pub mod smime;

pub use keyring::{Keyring, KeyringError};
pub use oauth::{OAuth2Config, OAuth2Engine};
pub use pgp::{AutocryptHeader, PgpEngine, PgpError};
pub use smime::SmimeEngine;
