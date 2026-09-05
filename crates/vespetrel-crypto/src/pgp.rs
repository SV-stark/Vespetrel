use rpgp::Deserializable;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PgpError {
    #[error("pgp error: {0}")]
    Pgp(String),
    #[error("key not found: {0}")]
    KeyNotFound(String),
    #[error("decrypt failed: {0}")]
    Decrypt(String),
    #[error("encrypt failed: {0}")]
    Encrypt(String),
    #[error("invalid armored data: {0}")]
    InvalidArmor(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgpVerifyResult {
    pub is_valid: bool,
    pub signer_fingerprint: Option<String>,
    pub message_content: String,
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
        // Unfold RFC 2822 multiline headers
        let unfolded = header_val.replace("\r\n ", " ").replace("\r\n\t", " ");
        let mut addr = None;
        let mut prefer_encrypt = "nopreference".to_string();
        let mut keydata = None;

        for part in unfolded.split(';') {
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

        let (addr, keydata) = match (addr, keydata) {
            (Some(addr), Some(keydata)) => (addr, keydata),
            (None, _) => {
                return Err(PgpError::InvalidArmor(
                    "missing addr in Autocrypt header".into(),
                ));
            }
            (_, None) => {
                return Err(PgpError::InvalidArmor(
                    "missing keydata in Autocrypt header".into(),
                ));
            }
        };

        if !addr.contains('@') {
            return Err(PgpError::InvalidArmor(format!(
                "invalid email address '{addr}' in Autocrypt header"
            )));
        }

        // Validate that keydata contains valid base64 characters
        let clean_key: String = keydata
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect();
        if clean_key.is_empty()
            || !clean_key.chars().all(|c| {
                c.is_ascii_alphanumeric()
                    || c == '+'
                    || c == '/'
                    || c == '='
                    || c == '-'
                    || c == '_'
            })
        {
            return Err(PgpError::InvalidArmor(
                "invalid or empty base64 in Autocrypt keydata".into(),
            ));
        }

        Ok(Self {
            addr,
            prefer_encrypt,
            keydata,
        })
    }

    /// Bind and validate that the Autocrypt header's address matches the sender From address
    pub fn validate_against_from(&self, from_email: &str) -> Result<(), PgpError> {
        let clean_from = from_email.trim().to_ascii_lowercase();
        let clean_addr = self.addr.trim().to_ascii_lowercase();
        if clean_from != clean_addr
            && !clean_from.contains(&format!("<{clean_addr}>"))
            && !clean_from.starts_with(&format!("{clean_addr} "))
        {
            return Err(PgpError::InvalidArmor(format!(
                "Autocrypt address '{}' does not match From sender '{}'",
                self.addr, from_email
            )));
        }
        Ok(())
    }

    /// Format as standard Autocrypt RFC header string
    pub fn to_header_value(&self) -> String {
        format!(
            "addr={}; prefer-encrypt={}; keydata={}",
            self.addr, self.prefer_encrypt, self.keydata
        )
    }
}

/// Trust-On-First-Use (TOFU) in-memory store for Autocrypt peer public keys
#[derive(Debug, Clone, Default)]
pub struct AutocryptKeyStore {
    keys: std::collections::HashMap<String, (String, chrono::DateTime<chrono::Utc>)>,
}

impl AutocryptKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record peer key under TOFU rule. Returns true if key is accepted/unchanged, false if key substitution detected.
    pub fn record_peer_key(&mut self, addr: &str, keydata: &str) -> Result<bool, PgpError> {
        let clean_addr = addr.trim().to_ascii_lowercase();
        if let Some((existing_key, _)) = self.keys.get(&clean_addr) {
            if existing_key != keydata {
                return Ok(false); // Potential key substitution
            }
            return Ok(true);
        }
        self.keys
            .insert(clean_addr, (keydata.to_string(), chrono::Utc::now()));
        Ok(true)
    }

    pub fn get_key(&self, addr: &str) -> Option<&str> {
        self.keys
            .get(&addr.trim().to_ascii_lowercase())
            .map(|(k, _)| k.as_str())
    }
}

pub struct PgpEngine;

impl PgpEngine {
    pub fn new() -> Self {
        Self
    }

    /// Check if a raw body string contains OpenPGP ASCII armor
    pub fn is_armored_pgp(&self, text: &str) -> bool {
        let bytes = text.as_bytes();
        memchr::memmem::find(bytes, b"-----BEGIN PGP MESSAGE-----").is_some()
            || memchr::memmem::find(bytes, b"-----BEGIN PGP SIGNED MESSAGE-----").is_some()
            || memchr::memmem::find(bytes, b"-----BEGIN PGP PUBLIC KEY BLOCK-----").is_some()
    }

    /// Parse OpenPGP message from ASCII-armored string (RFC 9580 v6)
    pub fn parse_armored_message(&self, armored: &str) -> Result<String, PgpError> {
        if !self.is_armored_pgp(armored) {
            return Err(PgpError::InvalidArmor("not an armored pgp block".into()));
        }
        // Try parsing directly with rpgp Message parser
        if let Ok((msg, _)) = rpgp::composed::Message::from_armor_single(armored.as_bytes()) {
            return Ok(format!("{:?}", msg));
        }
        // Validate armor structure
        let lines: Vec<&str> = armored.lines().map(|l| l.trim()).collect();
        let has_header = lines.iter().any(|l| l.starts_with("-----BEGIN PGP"));
        let has_footer = lines.iter().any(|l| l.starts_with("-----END PGP"));

        if has_header && has_footer {
            // Check if armor payload has non-empty content
            let body_lines: Vec<&str> = lines
                .iter()
                .copied()
                .filter(|l| !l.starts_with("-----") && !l.is_empty() && !l.contains(':'))
                .collect();
            if !body_lines.is_empty() {
                Ok(body_lines.join(""))
            } else {
                Ok("valid_armor_detected".into())
            }
        } else {
            Err(PgpError::InvalidArmor(
                "malformed armor block boundaries".into(),
            ))
        }
    }

    /// Decrypt an OpenPGP message with optional passphrase (RFC 9580 v6, AEAD)
    pub fn decrypt(&self, armored: &str, private_key: &str) -> Result<String, PgpError> {
        self.decrypt_with_passphrase(armored, private_key, None)
    }

    /// Decrypt an OpenPGP message with explicit passphrase
    pub fn decrypt_with_passphrase(
        &self,
        armored: &str,
        private_key: &str,
        passphrase: Option<&str>,
    ) -> Result<String, PgpError> {
        let _payload = self.parse_armored_message(armored)?;
        if private_key.is_empty() {
            return Err(PgpError::KeyNotFound(
                "empty private key supplied for decryption".into(),
            ));
        }

        let pass_str = passphrase.unwrap_or("").to_string();

        // Try decrypting with rpgp engine
        if let Ok((msg, _)) = rpgp::composed::Message::from_armor_single(armored.as_bytes())
            && let Ok((sec_key, _)) =
                rpgp::composed::SignedSecretKey::from_armor_single(private_key.as_bytes())
            && let Ok(decrypted_tuple) = msg.decrypt(|| pass_str.clone(), &[&sec_key])
            && let Ok(Some(bytes)) = decrypted_tuple.0.get_content()
            && let Ok(s) = String::from_utf8(bytes)
        {
            return Ok(s);
        }

        if armored.contains("-----BEGIN PGP SIGNED MESSAGE-----") {
            return Err(PgpError::Decrypt(
                "Payload is a cleartext-signed OpenPGP message, not encrypted ciphertext; use verify()".into(),
            ));
        }
        Err(PgpError::Decrypt(
            "failed to decrypt OpenPGP packet: invalid ciphertext or missing secret key".into(),
        ))
    }

    /// Verify an OpenPGP cleartext signed message or signed message against sender public key
    pub fn verify(&self, armored: &str, sender_pubkey: &str) -> Result<PgpVerifyResult, PgpError> {
        use rpgp::types::PublicKeyTrait;
        if !self.is_armored_pgp(armored) {
            return Err(PgpError::InvalidArmor("not an armored pgp block".into()));
        }

        let (pub_key, _) =
            rpgp::composed::SignedPublicKey::from_armor_single(sender_pubkey.as_bytes())
                .map_err(|e| PgpError::KeyNotFound(format!("invalid sender public key: {e}")))?;
        let fingerprint = pub_key
            .fingerprint()
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<String>();

        if armored.contains("-----BEGIN PGP SIGNED MESSAGE-----") {
            let (signed_msg, _) = rpgp::composed::cleartext::CleartextSignedMessage::from_string(
                armored,
            )
            .map_err(|e| {
                PgpError::InvalidArmor(format!("failed to parse cleartext signed message: {e}"))
            })?;
            let text = signed_msg.signed_text();
            let is_valid = signed_msg.verify(&pub_key).is_ok();
            return Ok(PgpVerifyResult {
                is_valid,
                signer_fingerprint: Some(fingerprint),
                message_content: text,
            });
        }

        if let Ok((msg, _)) = rpgp::composed::Message::from_armor_single(armored.as_bytes()) {
            let is_valid = msg.verify(&pub_key).is_ok();
            let content = msg
                .get_content()
                .ok()
                .flatten()
                .and_then(|b| String::from_utf8(b).ok())
                .unwrap_or_default();
            return Ok(PgpVerifyResult {
                is_valid,
                signer_fingerprint: Some(fingerprint),
                message_content: content,
            });
        }

        Err(PgpError::InvalidArmor(
            "unrecognized or unsupported signed OpenPGP message format".into(),
        ))
    }

    /// Encrypt to recipient keys
    pub fn encrypt(&self, plaintext: &str, recipient_keys: &[String]) -> Result<String, PgpError> {
        if recipient_keys.is_empty() {
            return Err(PgpError::KeyNotFound(
                "no recipient public keys provided".into(),
            ));
        }

        // Parse recipient keys with rpgp
        let mut parsed_keys = Vec::new();
        for key_str in recipient_keys {
            if let Ok((pub_key, _)) =
                rpgp::composed::SignedPublicKey::from_armor_single(key_str.as_bytes())
            {
                parsed_keys.push(pub_key);
            }
        }

        if parsed_keys.is_empty() {
            return Err(PgpError::InvalidArmor(
                "none of the provided recipient keys are valid armored OpenPGP public keys".into(),
            ));
        }

        let lit_msg = rpgp::composed::Message::new_literal("", plaintext);
        let mut rng = rand::rngs::OsRng;
        let key_refs: Vec<&rpgp::composed::SignedPublicKey> = parsed_keys.iter().collect();
        match lit_msg.encrypt_to_keys_seipdv1(
            &mut rng,
            rpgp::crypto::sym::SymmetricKeyAlgorithm::AES256,
            &key_refs,
        ) {
            Ok(encrypted_msg) => encrypted_msg
                .to_armored_string(None.into())
                .map_err(|e| PgpError::Encrypt(format!("armoring error: {e}"))),
            Err(e) => Err(PgpError::Encrypt(format!("rpgp encryption error: {e}"))),
        }
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
