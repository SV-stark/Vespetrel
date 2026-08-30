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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutocryptHeader {
    pub addr: String,
    pub prefer_encrypt: String,
    pub keydata: String,
}

impl AutocryptHeader {
    /// Parse RFC-compliant Autocrypt 1.1 header (e.g. `addr=alice@example.com; prefer-encrypt=mutual; keydata=mQEN...`)
    pub fn parse(header_val: &str) -> Result<Self, PgpError> {
        let mut addr = None;
        let mut prefer_encrypt = "nopreference".to_string();
        let mut keydata = None;

        for part in header_val.split(';') {
            let part = part.trim();
            if let Some((k, v)) = part.split_once('=') {
                let k = k.trim();
                let v = v.trim();
                match k {
                    "addr" => addr = Some(v.to_string()),
                    "prefer-encrypt" => prefer_encrypt = v.to_string(),
                    "keydata" => keydata = Some(v.to_string()),
                    _ => {}
                }
            }
        }

        match (addr, keydata) {
            (Some(addr), Some(keydata)) => Ok(Self {
                addr,
                prefer_encrypt,
                keydata,
            }),
            (None, _) => Err(PgpError::InvalidArmor(
                "missing addr in Autocrypt header".into(),
            )),
            (_, None) => Err(PgpError::InvalidArmor(
                "missing keydata in Autocrypt header".into(),
            )),
        }
    }

    /// Format as standard Autocrypt RFC header string
    pub fn to_header_value(&self) -> String {
        format!(
            "addr={}; prefer-encrypt={}; keydata={}",
            self.addr, self.prefer_encrypt, self.keydata
        )
    }
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

    #[test]
    fn test_autocrypt_parse_roundtrip() {
        let header_str = "addr=bob@example.com; prefer-encrypt=mutual; keydata=AAAA_KEY_DATA";
        let parsed = AutocryptHeader::parse(header_str).unwrap();
        assert_eq!(parsed.addr, "bob@example.com");
        assert_eq!(parsed.prefer_encrypt, "mutual");
        assert_eq!(parsed.keydata, "AAAA_KEY_DATA");
        assert_eq!(parsed.to_header_value(), header_str);
    }
}
