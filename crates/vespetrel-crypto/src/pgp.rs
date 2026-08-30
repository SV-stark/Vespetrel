use thiserror::Error;

#[derive(Debug, Error)]
pub enum PgpError {
    #[error("pgp error: {0}")]
    Pgp(String),
    #[error("key not found: {0}")]
    KeyNotFound(String),
    #[error("decrypt failed: {0}")]
    Decrypt(String),
}

pub struct PgpEngine;

impl PgpEngine {
    pub fn new() -> Self { Self }

    /// Decrypt an OpenPGP message (RFC 9580 v6, AEAD)
    pub fn decrypt(&self, _armored: &str, _private_key: &str) -> Result<String, PgpError> {
        // Real: use rpgp crate to parse armored message, decrypt with private key
        Err(PgpError::Pgp("stub - rPGP decrypt not wired in demo".into()))
    }

    /// Encrypt to recipient keys
    pub fn encrypt(&self, _plaintext: &str, _recipient_keys: &[String]) -> Result<String, PgpError> {
        Err(PgpError::Pgp("stub".into()))
    }

    /// Generate Autocrypt 1.1 header
    pub fn autocrypt_header(&self, _email: &str, _public_key: &str) -> String {
        format!("addr={}; keydata=stub", _email)
    }
}

impl Default for PgpEngine { fn default() -> Self { Self::new() } }
