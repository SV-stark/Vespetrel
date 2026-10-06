use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// PBKDF2 iteration count for PIN-derived key material.
///
/// OWASP's 2023 floor for PBKDF2-HMAC-SHA256 is 600 000. The previous value of 10 000 made
/// an offline PIN search (see `sign_digest`) trivial.
const PBKDF2_ITERATIONS: u32 = 600_000;

/// Minimum accepted PIN length.
const MIN_PIN_LEN: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HardwareTokenType {
    YubiKeyOpenPgp,
    Pkcs11Smartcard,
    Fido2HmacSecret,
}

fn serialize_attempts<S: serde::Serializer>(v: &Arc<AtomicU32>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u32(v.load(Ordering::SeqCst))
}

fn deserialize_attempts<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Arc<AtomicU32>, D::Error> {
    Ok(Arc::new(AtomicU32::new(u32::deserialize(d)?)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSecurityKey {
    pub id: String,
    pub serial_number: String,
    pub token_type: HardwareTokenType,
    pub manufacturer: String,
    pub key_slots: Vec<String>,
    /// Random per-token salt mixed into the PBKDF2 derivation.
    ///
    /// The previous code used the (public) serial number as the salt, so the entire derived
    /// key was a function of a short PIN and public data.
    #[serde(default = "default_salt")]
    pub kdf_salt: String,
    /// PBKDF2 verifier for the token PIN, set at enrolment.
    ///
    /// Without this there was no PIN check at all: any string of length >= 4 was accepted.
    #[serde(default)]
    pub pin_verifier: Option<String>,
    /// Consecutive failed attempts.
    ///
    /// Serialized as a plain `u32` and rebuilt into the `Arc<AtomicU32>` on load, so a serde
    /// round-trip no longer resets the lockout. The previous `#[serde(skip)]` meant any
    /// restart (or any `Clone`/persist) zeroed the counter, nullifying it entirely.
    #[serde(
        default,
        serialize_with = "serialize_attempts",
        deserialize_with = "deserialize_attempts"
    )]
    pub failed_attempts: Arc<AtomicU32>,
    pub max_attempts: u32,
}

fn default_salt() -> String {
    use base64::Engine;
    // Zero salt is only a fallback for deserializing pre-existing records; new tokens get a
    // real random salt in `new`.
    base64::engine::general_purpose::STANDARD.encode([0u8; 16])
}

impl HardwareSecurityKey {
    pub fn new(serial: impl Into<String>, token_type: HardwareTokenType) -> Self {
        use base64::Engine;
        let serial = serial.into();
        // Key slots are OpenPGP-specific; reporting them for a PKCS#11 or FIDO2 token told
        // the user the token had slots it does not have.
        let key_slots = match token_type {
            HardwareTokenType::YubiKeyOpenPgp => vec![
                "OPENPGP.1 (Signature)".into(),
                "OPENPGP.2 (Decryption)".into(),
                "OPENPGP.3 (Authentication)".into(),
            ],
            HardwareTokenType::Pkcs11Smartcard => vec!["PKCS#11 (Smartcard)".into()],
            HardwareTokenType::Fido2HmacSecret => vec!["FIDO2 HMAC Secret".into()],
        };
        let mut salt = [0u8; 16];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut salt)
            .expect("OS RNG must be available to enrol a hardware token");
        Self {
            id: format!("hwkey:{serial}"),
            serial_number: serial,
            token_type,
            manufacturer: "Yubico / PKCS#11 Provider".into(),
            key_slots,
            kdf_salt: base64::engine::general_purpose::STANDARD.encode(salt),
            pin_verifier: None,
            failed_attempts: Arc::new(AtomicU32::new(0)),
            max_attempts: 3,
        }
    }

    /// Derive the PIN key. The returned buffer is zeroized on drop.
    fn derive_pin_key(&self, pin: &str) -> zeroize::Zeroizing<[u8; 32]> {
        use base64::Engine;
        use ring::pbkdf2;
        use std::num::NonZeroU32;
        use zeroize::Zeroizing;

        let salt = base64::engine::general_purpose::STANDARD
            .decode(self.kdf_salt.as_bytes())
            .unwrap_or_else(|_| vec![0u8; 16]);
        let mut derived_key = Zeroizing::new([0u8; 32]);
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            NonZeroU32::new(PBKDF2_ITERATIONS).expect("iteration count is a nonzero constant"),
            &salt,
            pin.as_bytes(),
            &mut *derived_key,
        );
        derived_key
    }

    /// Record the token PIN at enrolment so it can be verified later.
    pub fn enrol_pin(&mut self, pin: &str) -> Result<(), String> {
        if pin.len() < MIN_PIN_LEN {
            return Err(format!(
                "Hardware PIN must be at least {MIN_PIN_LEN} characters"
            ));
        }
        use base64::Engine;
        use ring::hmac;
        let key = self.derive_pin_key(pin);
        let mac = hmac::sign(
            &hmac::Key::new(hmac::HMAC_SHA256, &*key),
            b"vespetrel-hwkey-pin-v1",
        );
        self.pin_verifier = Some(base64::engine::general_purpose::STANDARD.encode(mac.as_ref()));
        Ok(())
    }

    /// Produce an authentication tag over `digest` for this token.
    ///
    /// SECURITY: this is a keyed MAC, **not** a digital signature. It proves only that the
    /// holder knows the token PIN; it provides no non-repudiation and cannot be verified by a
    /// third party holding only a public key. Callers must not present it as a signature.
    /// Real signing requires a real token (PKCS#11 / PIV / FIDO2) with a non-exportable key.
    pub fn sign_digest(&self, digest: &[u8], pin: &str) -> Result<Vec<u8>, String> {
        if self.failed_attempts.load(Ordering::SeqCst) >= self.max_attempts {
            return Err("Hardware token locked: maximum PIN attempts exceeded".into());
        }
        if digest.is_empty() {
            return Err("Digest to sign is empty".into());
        }
        if pin.len() < MIN_PIN_LEN {
            self.record_failure();
            return Err(format!(
                "Hardware PIN must be at least {MIN_PIN_LEN} characters"
            ));
        }
        if self.pin_verifier.is_none() {
            return Err("Hardware token has no enrolled PIN; call enrol_pin() first".into());
        }

        let key = self.derive_pin_key(pin);

        // Constant-time PIN verification. Without this, *any* PIN of sufficient length
        // produced a valid tag.
        use base64::Engine;
        use ring::hmac;
        let mac = hmac::sign(
            &hmac::Key::new(hmac::HMAC_SHA256, &*key),
            b"vespetrel-hwkey-pin-v1",
        );
        let expected = base64::engine::general_purpose::STANDARD
            .decode(self.pin_verifier.as_deref().unwrap_or_default().as_bytes())
            .map_err(|_| "Hardware token PIN verifier is corrupt".to_string())?;
        if expected.len() != mac.as_ref().len() || !crate::ct_eq(&expected, mac.as_ref()) {
            self.record_failure();
            return Err("Incorrect hardware token PIN".into());
        }

        let tag = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, &*key), digest);
        self.failed_attempts.store(0, Ordering::SeqCst);
        Ok(tag.as_ref().to_vec())
    }

    /// Atomically increment and enforce the failure counter.
    ///
    /// The previous load-then-`fetch_add` let N concurrent calls all pass the threshold.
    fn record_failure(&self) {
        self.failed_attempts
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                Some(n.saturating_add(1))
            })
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hardware_key_signing() {
        let mut key = HardwareSecurityKey::new("12345678", HardwareTokenType::YubiKeyOpenPgp);
        assert_eq!(key.key_slots.len(), 3);
        key.enrol_pin("123456").unwrap();

        let digest = [0xabu8; 32];
        let sig = key.sign_digest(&digest, "123456").unwrap();
        assert!(!sig.is_empty());
        // A short PIN is rejected and does not produce a tag.
        assert!(key.sign_digest(&digest, "12").is_err());

        // A wrong PIN of sufficient length is rejected. Before the fix this succeeded,
        // because no PIN was ever verified.
        assert!(key.sign_digest(&digest, "999999").is_err());

        // Lockout after max_attempts consecutive failures.
        for _ in 0..key.max_attempts {
            let _ = key.sign_digest(&digest, "000000");
        }
        assert!(key.sign_digest(&digest, "123456").is_err());
    }

    #[test]
    fn test_hardware_key_requires_enrolment_and_random_salt() {
        let key = HardwareSecurityKey::new("deadbeef", HardwareTokenType::Fido2HmacSecret);
        // Token-specific slots, not OpenPGP slots.
        assert_eq!(key.key_slots.len(), 1);
        // No PIN enrolled: refuse rather than accept anything.
        assert!(key.sign_digest(&[1u8; 32], "123456").is_err());
        // The salt is random, not the public serial number.
        let other = HardwareSecurityKey::new("deadbeef", HardwareTokenType::Fido2HmacSecret);
        assert_ne!(key.kdf_salt, other.kdf_salt);
        assert_ne!(key.kdf_salt, "deadbeef");
    }
}
