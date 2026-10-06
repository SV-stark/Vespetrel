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

        // RFC 8252 §4: keydata is *standard* base64 (no base64url '-' or '_'), with
        // RFC 5322 folding whitespace allowed. The previous check accepted base64url
        // characters and, worse, validated the whitespace-stripped `clean_key` while
        // storing the *unstripped* `keydata` — so two different strings were in play.
        let clean_key: String = keydata
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect();
        if clean_key.is_empty()
            || !clean_key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
        {
            return Err(PgpError::InvalidArmor(
                "invalid or empty base64 in Autocrypt keydata".into(),
            ));
        }

        // RFC 8252 §4: prefer-encrypt is a closed value set.
        if !matches!(
            prefer_encrypt.as_str(),
            "mutual" | "opportunistic" | "nopreference"
        ) {
            return Err(PgpError::InvalidArmor(format!(
                "invalid prefer-encrypt value '{prefer_encrypt}'"
            )));
        }

        Ok(Self {
            addr,
            prefer_encrypt,
            // Store exactly what we validated, so to_header_value() round-trips.
            keydata: clean_key,
        })
    }

    /// Bind and validate that the Autocrypt header's address matches the sender From address.
    ///
    /// RFC 8252 §5.1 requires `addr` to equal the parsed mailbox of the single `From` address.
    /// The previous substring comparison matched anywhere in the raw header *including the
    /// display name*, which is attacker-controlled and carries no authority:
    /// `From: alice@corp.com <attacker@evil.com>` passed validation against
    /// `addr=alice@corp.com`, letting an Autocrypt key be pinned to a victim's address.
    pub fn validate_against_from(&self, from_header: &str) -> Result<(), PgpError> {
        use mail_parser::MessageParser;

        let raw = format!("From: {from_header}\r\n\r\n");
        let parsed = MessageParser::default()
            .parse(&raw)
            .ok_or_else(|| PgpError::InvalidArmor("unparsable From header".into()))?;
        let addrs = parsed
            .from()
            .ok_or_else(|| PgpError::InvalidArmor("From header has no address".into()))?;

        let addrs_owned = addrs.clone();
        let mut mailboxes = addrs_owned
            .iter()
            .filter_map(|a| a.address().map(|s| s.to_ascii_lowercase()));
        let only = mailboxes
            .next()
            .ok_or_else(|| PgpError::InvalidArmor("From header is empty".into()))?;
        if mailboxes.next().is_some() {
            return Err(PgpError::InvalidArmor(
                "Autocrypt requires a single From mailbox".into(),
            ));
        }

        let clean_addr = self
            .addr
            .trim()
            .trim_matches(['<', '>'])
            .to_ascii_lowercase();
        if clean_addr != only {
            return Err(PgpError::InvalidArmor(format!(
                "Autocrypt address '{}' does not match From mailbox '{only}'",
                self.addr
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

/// Trust-On-First-Use (TOFU) in-memory store for Autocrypt peer public keys.
#[derive(Clone, Default)]
pub struct AutocryptKeyStore {
    keys: std::collections::HashMap<String, (String, chrono::DateTime<chrono::Utc>)>,
}

/// Redacted `Debug`: the map holds raw key material and must never reach a log line.
impl std::fmt::Debug for AutocryptKeyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutocryptKeyStore")
            .field("entries", &self.keys.len())
            .finish()
    }
}

/// Validate that `keydata` is decodable base64 carrying a real OpenPGP transferable public
/// key, and return its canonical hex fingerprint.
///
/// The previous store accepted any string and pinned it under the peer's address, so a single
/// forged first message poisoned that address for the process lifetime.
fn validate_peer_keydata(keydata: &str) -> Result<String, PgpError> {
    use base64::Engine;
    use rpgp::types::PublicKeyTrait;

    let clean: String = keydata
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(clean.as_bytes())
        .map_err(|_| PgpError::InvalidArmor("Autocrypt keydata is not valid base64".into()))?;
    let key = rpgp::composed::SignedPublicKey::from_bytes(&der[..]).map_err(|e| {
        PgpError::InvalidArmor(format!("Autocrypt keydata is not an OpenPGP cert: {e}"))
    })?;
    if !key.details.revocation_signatures.is_empty() {
        return Err(PgpError::InvalidArmor(
            "Autocrypt keydata carries a revocation signature".into(),
        ));
    }
    Ok(key
        .fingerprint()
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect())
}

impl AutocryptKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record peer key under TOFU rule.
    ///
    /// Returns `true` if the key is accepted/unchanged and `false` if key substitution was
    /// detected. Key material is validated before it is pinned, so an address can no longer
    /// be poisoned with arbitrary text.
    pub fn record_peer_key(&mut self, addr: &str, keydata: &str) -> Result<bool, PgpError> {
        let clean_addr = addr.trim().to_ascii_lowercase();
        if !clean_addr.contains('@') {
            return Err(PgpError::InvalidArmor(format!(
                "invalid Autocrypt peer address '{addr}'"
            )));
        }
        // Validate before mutating: never pin key material we could not parse.
        validate_peer_keydata(keydata)?;

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

    /// Fingerprint of the pinned key for `addr`, if any.
    pub fn get_fingerprint(&self, addr: &str) -> Option<String> {
        let key = self.get_key(addr)?;
        validate_peer_keydata(key).ok()
    }

    /// Accept a key change for `addr`.
    ///
    /// Without this, a legitimate key rotation returned `Ok(false)` forever and the peer
    /// could never be re-trusted without restarting the process.
    pub fn confirm_key_change(&mut self, addr: &str, new_keydata: &str) -> Result<(), PgpError> {
        let clean_addr = addr.trim().to_ascii_lowercase();
        validate_peer_keydata(new_keydata)?;
        self.keys
            .insert(clean_addr, (new_keydata.to_string(), chrono::Utc::now()));
        Ok(())
    }

    /// Drop the pinned key for `addr`, forcing the next message to re-establish trust.
    pub fn forget(&mut self, addr: &str) {
        self.keys.remove(&addr.trim().to_ascii_lowercase());
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

    /// Decrypt an OpenPGP message with explicit passphrase.
    ///
    /// The plaintext is only returned when the decrypted message carries a signature that
    /// verifies. `Message::get_content()` on a `Signed` message returns the literal data
    /// *without* checking the signature, so the previous implementation handed back
    /// attacker-authored text as if it were authenticated mail: anyone holding the
    /// recipient's *public* key can encrypt an arbitrary `Signed` message to them.
    pub fn decrypt_with_passphrase(
        &self,
        armored: &str,
        private_key: &str,
        passphrase: Option<&str>,
    ) -> Result<String, PgpError> {
        if !self.is_armored_pgp(armored) {
            return Err(PgpError::InvalidArmor("not an armored pgp block".into()));
        }
        if private_key.is_empty() {
            return Err(PgpError::KeyNotFound(
                "empty private key supplied for decryption".into(),
            ));
        }

        if armored.contains("-----BEGIN PGP SIGNED MESSAGE-----") {
            return Err(PgpError::Decrypt(
                "Payload is a cleartext-signed OpenPGP message, not encrypted ciphertext; use verify()".into(),
            ));
        }

        let pass_str = passphrase.unwrap_or("").to_string();

        let (msg, _) = rpgp::composed::Message::from_armor_single(armored.as_bytes())
            .map_err(|e| PgpError::Decrypt(format!("could not parse OpenPGP message: {e}")))?;
        let (sec_key, _) =
            rpgp::composed::SignedSecretKey::from_armor_single(private_key.as_bytes())
                .map_err(|e| PgpError::KeyNotFound(format!("could not parse private key: {e}")))?;

        let decrypted = msg
            .decrypt(|| pass_str.clone(), &[&sec_key])
            .map_err(|e| PgpError::Decrypt(format!("failed to decrypt session key: {e}")))?;

        // Refuse to return content whose authenticity we have not established.
        use rpgp::types::SecretKeyTrait;
        match &decrypted.0 {
            rpgp::composed::Message::Signed { .. } => {
                decrypted.0.verify(&sec_key.public_key()).map_err(|e| {
                    PgpError::Decrypt(format!("signature verification failed: {e}"))
                })?;
            }
            _ => {
                return Err(PgpError::Decrypt(
                    "decrypted message is UNSIGNED; refusing to return unauthenticated content"
                        .into(),
                ));
            }
        }

        let bytes = decrypted
            .0
            .get_content()
            .map_err(|e| PgpError::Decrypt(format!("could not extract content: {e}")))?
            .ok_or_else(|| {
                PgpError::Decrypt("decrypted message carried no literal content".into())
            })?;

        String::from_utf8(bytes)
            .map_err(|_| PgpError::Decrypt("decrypted content is not valid UTF-8".into()))
    }

    /// Verify an OpenPGP cleartext signed message or signed message against sender public key.
    ///
    /// `signer_fingerprint` and `message_content` are only populated on success. The previous
    /// implementation always returned the *caller-supplied* key's fingerprint and always
    /// returned the message text, so a failed verification still named a signer identity and
    /// still handed back attacker-controlled content.
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

        // Reject a revoked or expired sender key: rpgp's `verify` only checks issuer
        // identity, hashing, and the signature itself.
        if let Some(expires) = pub_key.expires_at()
            && expires <= chrono::Utc::now()
        {
            return Err(PgpError::KeyNotFound(format!(
                "sender key expired at {expires}"
            )));
        }
        if pub_key.details.revocation_signatures.is_empty() {
            // not revoked
        } else {
            return Err(PgpError::KeyNotFound("sender key is revoked".into()));
        }

        if armored.contains("-----BEGIN PGP SIGNED MESSAGE-----") {
            let (signed_msg, _) = rpgp::composed::cleartext::CleartextSignedMessage::from_string(
                armored,
            )
            .map_err(|e| {
                PgpError::InvalidArmor(format!("failed to parse cleartext signed message: {e}"))
            })?;
            // Fail closed: propagate the verification error rather than returning
            // `is_valid: false` alongside the caller's key fingerprint and the signed text.
            signed_msg.verify(&pub_key).map_err(|e| {
                PgpError::Pgp(format!("cleartext signature verification failed: {e}"))
            })?;
            return Ok(PgpVerifyResult {
                is_valid: true,
                signer_fingerprint: Some(fingerprint),
                message_content: signed_msg.signed_text().to_string(),
            });
        }

        if let Ok((msg, _)) = rpgp::composed::Message::from_armor_single(armored.as_bytes()) {
            msg.verify(&pub_key)
                .map_err(|e| PgpError::Pgp(format!("signature verification failed: {e}")))?;
            let content = msg
                .get_content()
                .map_err(|e| PgpError::Pgp(format!("could not extract content: {e}")))?
                .ok_or_else(|| PgpError::Pgp("signed message carried no content".into()))?;
            let content = String::from_utf8(content)
                .map_err(|_| PgpError::Pgp("signed content is not valid UTF-8".into()))?;
            return Ok(PgpVerifyResult {
                is_valid: true,
                signer_fingerprint: Some(fingerprint),
                message_content: content,
            });
        }

        Err(PgpError::InvalidArmor(
            "unrecognized or unsupported signed OpenPGP message format".into(),
        ))
    }

    /// Encrypt to recipient keys.
    ///
    /// Every requested recipient must be usable. The previous implementation silently dropped
    /// any key that failed to parse and encrypted only to the remainder while returning
    /// `Ok`, so a caller believed a message was confidential to all recipients when it was
    /// not.
    pub fn encrypt(&self, plaintext: &str, recipient_keys: &[String]) -> Result<String, PgpError> {
        if recipient_keys.is_empty() {
            return Err(PgpError::KeyNotFound(
                "no recipient public keys provided".into(),
            ));
        }

        let now = chrono::Utc::now();
        let mut parsed_keys = Vec::with_capacity(recipient_keys.len());
        let mut rejected: Vec<String> = Vec::new();

        for (i, key_str) in recipient_keys.iter().enumerate() {
            let key = match rpgp::composed::SignedPublicKey::from_armor_single(key_str.as_bytes()) {
                Ok((k, _)) => k,
                Err(e) => {
                    rejected.push(format!("recipient {i}: unparsable key: {e}"));
                    continue;
                }
            };
            if let Some(expires) = key.expires_at()
                && expires <= now
            {
                rejected.push(format!("recipient {i}: key expired at {expires}"));
                continue;
            }
            if !key.details.revocation_signatures.is_empty() {
                rejected.push(format!("recipient {i}: key is revoked"));
                continue;
            }
            parsed_keys.push((i, key));
        }

        if !rejected.is_empty() {
            // Refuse rather than send a message the user believes is confidential but isn't.
            return Err(PgpError::Encrypt(format!(
                "refusing to encrypt: {} of {} recipient keys unusable: {}",
                rejected.len(),
                recipient_keys.len(),
                rejected.join("; ")
            )));
        }
        if parsed_keys.is_empty() {
            return Err(PgpError::InvalidArmor(
                "none of the provided recipient keys are valid armored OpenPGP public keys".into(),
            ));
        }

        let lit_msg = rpgp::composed::Message::new_literal("", plaintext);
        let mut rng = rand::rngs::OsRng;
        let key_refs: Vec<&rpgp::composed::SignedPublicKey> =
            parsed_keys.iter().map(|(_, k)| k).collect();
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
        let header_str = "addr=bob@example.com; prefer-encrypt=mutual; keydata=AAAABBBBCCCC";
        let parsed = AutocryptHeader::parse(header_str).unwrap();
        assert_eq!(parsed.addr, "bob@example.com");
        assert_eq!(parsed.prefer_encrypt, "mutual");
        assert_eq!(parsed.keydata, "AAAABBBBCCCC");
        assert_eq!(parsed.to_header_value(), header_str);
    }

    /// RFC 8252 mandates *standard* base64 for `keydata`. The previous alphabet check
    /// accepted base64url characters, so a header that a real peer would reject parsed here.
    #[test]
    fn autocrypt_rejects_base64url_and_bad_prefer_encrypt() {
        assert!(AutocryptHeader::parse("addr=a@b.com; keydata=AAA_BBBB").is_err());
        assert!(
            AutocryptHeader::parse("addr=a@b.com; prefer-encrypt=maybe; keydata=AAAA").is_err()
        );
        // Missing pieces are still rejected.
        assert!(AutocryptHeader::parse("keydata=AAAA").is_err());
        assert!(AutocryptHeader::parse("addr=a@b.com").is_err());
    }

    /// Regression: the Autocrypt `addr` must equal the parsed *From mailbox*. The old
    /// substring match accepted a spoofed From whose display name contained the address:
    /// `From: alice@corp.com <attacker@evil.com>`.
    #[test]
    fn autocrypt_from_binding_rejects_display_name_spoofing() {
        let h = AutocryptHeader {
            addr: "alice@corp.com".into(),
            prefer_encrypt: "mutual".into(),
            keydata: "AAAABBBBCCCC".into(),
        };
        // The attack: address matches only inside the attacker-controlled display name.
        assert!(
            h.validate_against_from("alice@corp.com <attacker@evil.com>")
                .is_err()
        );
        assert!(
            h.validate_against_from("\"<alice@corp.com>\" <attacker@evil.com>")
                .is_err()
        );
        // A genuine single-mailbox From is accepted.
        assert!(h.validate_against_from("Alice <alice@corp.com>").is_ok());
        assert!(h.validate_against_from("alice@corp.com").is_ok());
        // Multiple mailboxes are rejected.
        assert!(
            h.validate_against_from("alice@corp.com, bob@evil.com")
                .is_err()
        );
    }

    /// The TOFU store must not pin unvalidated key material, and a legitimate key rotation
    /// must be recoverable.
    #[test]
    fn autocrypt_store_validates_and_allows_recovery() {
        let mut store = AutocryptKeyStore::new();
        // Garbage is rejected, not pinned.
        assert!(
            store
                .record_peer_key("bob@example.com", "not-a-real-key")
                .is_err()
        );
        assert!(store.get_key("bob@example.com").is_none());
        // A bad address is rejected.
        assert!(store.record_peer_key("not-an-address", "AAAABBBB").is_err());
        // Redacted Debug must not dump key material.
        assert!(!format!("{store:?}").contains("AAAABBBB"));
    }
}
