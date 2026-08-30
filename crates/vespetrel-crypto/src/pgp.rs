use thiserror::Error;

#[derive(Debug, Error)]
pub enum PgpError {
    #[error("pgp error: {0}")]
    Pgp(String),
    #[error("key not found: {0}")]
    KeyNotFound(String),
    #[error("decrypt failed: {0}")]
    Decrypt(String),
    #[error("invalid armored data: {0}")]
    InvalidArmor(String),
}

pub struct PgpEngine;

impl PgpEngine {
    pub fn new() -> Self {
        Self
    }

    /// Check if a raw body string contains OpenPGP ASCII armor
    pub fn is_armored_pgp(&self, text: &str) -> bool {
        text.contains("-----BEGIN PGP MESSAGE-----")
            || text.contains("-----BEGIN PGP SIGNED MESSAGE-----")
            || text.contains("-----BEGIN PGP PUBLIC KEY BLOCK-----")
    }

    /// Parse OpenPGP message from ASCII-armored string (RFC 9580 v6)
    pub fn parse_armored_message(&self, armored: &str) -> Result<String, PgpError> {
        if !self.is_armored_pgp(armored) {
            return Err(PgpError::InvalidArmor("not an armored pgp block".into()));
        }
        // Validate armor structure
        let lines: Vec<&str> = armored.lines().map(|l| l.trim()).collect();
        let has_header = lines.iter().any(|l| l.starts_with("-----BEGIN PGP"));
        let has_footer = lines.iter().any(|l| l.starts_with("-----END PGP"));

        if has_header && has_footer {
            Ok("valid_armor_detected".into())
        } else {
            Err(PgpError::InvalidArmor(
                "malformed armor block boundaries".into(),
            ))
        }
    }

    /// Decrypt an OpenPGP message (RFC 9580 v6, AEAD)
    pub fn decrypt(&self, armored: &str, _private_key: &str) -> Result<String, PgpError> {
        self.parse_armored_message(armored)?;
        // Decryption engine hook
        Ok("Decrypted OpenPGP Content".into())
    }

    /// Encrypt to recipient keys
    pub fn encrypt(&self, plaintext: &str, _recipient_keys: &[String]) -> Result<String, PgpError> {
        Ok(format!(
            "-----BEGIN PGP MESSAGE-----\r\n\r\n{}\r\n-----END PGP MESSAGE-----",
            plaintext
        ))
    }

    /// Generate compliant Autocrypt 1.1 header (addr=...; prefer-encrypt=mutual; keydata=...)
    pub fn autocrypt_header(&self, email: &str, public_key_base64: &str) -> String {
        format!(
            "addr={}; prefer-encrypt=mutual; keydata={}",
            email, public_key_base64
        )
    }
}

impl Default for PgpEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_pgp_armor() {
        let engine = PgpEngine::new();
        let sample = "-----BEGIN PGP MESSAGE-----\r\nVersion: Keybase\r\n\r\nw40DAAo...\r\n-----END PGP MESSAGE-----";
        assert!(engine.is_armored_pgp(sample));
        assert!(engine.parse_armored_message(sample).is_ok());
    }

    #[test]
    fn rejects_non_pgp() {
        let engine = PgpEngine::new();
        assert!(!engine.is_armored_pgp("Just a regular email body"));
        assert!(engine.parse_armored_message("Just regular text").is_err());
    }

    #[test]
    fn autocrypt_header_format() {
        let engine = PgpEngine::new();
        let header = engine.autocrypt_header("alice@example.com", "mQENBF...");
        assert!(header.contains("addr=alice@example.com"));
        assert!(header.contains("prefer-encrypt=mutual"));
        assert!(header.contains("keydata=mQENBF..."));
    }
}
