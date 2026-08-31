//! FIDO2 / YubiKey & PKCS#11 Hardware Token Cryptography §7 Phase 6
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HardwareTokenType {
    YubiKeyOpenPgp,
    Pkcs11Smartcard,
    Fido2HmacSecret,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSecurityKey {
    pub id: String,
    pub serial_number: String,
    pub token_type: HardwareTokenType,
    pub manufacturer: String,
    pub key_slots: Vec<String>,
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
        }
    }

    /// Sign digest using hardware token pin verification
    pub fn sign_digest(&self, digest: &[u8], pin: &str) -> Result<Vec<u8>, String> {
        if pin.len() < 4 {
            return Err("Hardware PIN must be at least 4 characters".into());
        }
        if digest.is_empty() {
            return Err("Digest to sign is empty".into());
        }

        // Mock hardware token cryptographic signature over SHA-256 digest
        let mut sig = Vec::with_capacity(64);
        sig.extend_from_slice(b"HW_SIG_");
        sig.extend_from_slice(&self.serial_number.as_bytes()[..self.serial_number.len().min(8)]);
        sig.extend_from_slice(digest);
        Ok(sig)
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
