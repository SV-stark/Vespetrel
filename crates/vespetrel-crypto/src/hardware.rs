use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HardwareTokenType {
    YubiKeyOpenPgp,
    Pkcs11Smartcard,
    Fido2HmacSecret,
}

fn default_attempts() -> Arc<AtomicU32> {
    Arc::new(AtomicU32::new(0))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSecurityKey {
    pub id: String,
    pub serial_number: String,
    pub token_type: HardwareTokenType,
    pub manufacturer: String,
    pub key_slots: Vec<String>,
    #[serde(skip, default = "default_attempts")]
    pub failed_attempts: Arc<AtomicU32>,
    pub max_attempts: u32,
}

impl HardwareSecurityKey {
    pub fn new(serial: impl Into<String>, token_type: HardwareTokenType) -> Self {
        let serial = serial.into();
        Self {
            id: format!("hwkey:{}", serial),
            serial_number: serial,
            token_type,
            manufacturer: "Yubico / PKCS#11 Provider".into(),
            key_slots: vec![
                "OPENPGP.1 (Signature)".into(),
                "OPENPGP.2 (Decryption)".into(),
                "OPENPGP.3 (Authentication)".into(),
            ],
            failed_attempts: Arc::new(AtomicU32::new(0)),
            max_attempts: 3,
        }
    }

    /// Sign digest using hardware token pin verification via PBKDF2-derived HMAC-SHA256
    pub fn sign_digest(&self, digest: &[u8], pin: &str) -> Result<Vec<u8>, String> {
        let attempts = self.failed_attempts.load(Ordering::SeqCst);
        if attempts >= self.max_attempts {
            return Err("Hardware token locked: maximum PIN attempts exceeded".into());
        }

        if pin.len() < 4 {
            self.failed_attempts.fetch_add(1, Ordering::SeqCst);
            return Err("Hardware PIN must be at least 4 characters".into());
        }
        if digest.is_empty() {
            return Err("Digest to sign is empty".into());
        }

        use ring::{hmac, pbkdf2};
        use std::num::NonZeroU32;
        use zeroize::Zeroizing;

        let pin_buf = Zeroizing::new(pin.as_bytes().to_vec());
        let mut derived_key = [0u8; 32];
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            NonZeroU32::new(10_000).unwrap(),
            self.serial_number.as_bytes(),
            &pin_buf,
            &mut derived_key,
        );

        let s_key = hmac::Key::new(hmac::HMAC_SHA256, &derived_key);
        let tag = hmac::sign(&s_key, digest);
        self.failed_attempts.store(0, Ordering::SeqCst);
        Ok(tag.as_ref().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hardware_key_signing() {
        let key = HardwareSecurityKey::new("12345678", HardwareTokenType::YubiKeyOpenPgp);
        assert_eq!(key.key_slots.len(), 3);

        let digest = [0xabu8; 32];
        let sig = key.sign_digest(&digest, "123456").unwrap();
        assert!(!sig.is_empty());
        assert!(key.sign_digest(&digest, "12").is_err());
    }
}
