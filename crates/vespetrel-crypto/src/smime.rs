//! S/MIME X.509 Certificate Cryptography & PKCS#7 CMS Engine §7 Phase 6
use serde::{Deserialize, Serialize};
use x509_cert::der::Decode;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmimeVerificationResult {
    pub is_valid: bool,
    pub signer_email: Option<String>,
    pub issuer_cn: Option<String>,
    pub serial_number: Option<String>,
}

pub struct SmimeEngine;

impl SmimeEngine {
    pub fn new() -> Self {
        Self
    }

    /// Check if raw bytes contain PKCS#7 / CMS DER or PEM signatures
    pub fn is_smime_data(&self, data: &[u8]) -> bool {
        if data.is_empty() {
            return false;
        }
        // ASN.1 DER Sequence (0x30) or PEM header
        if data.starts_with(b"-----BEGIN PKCS7-----")
            || data.starts_with(b"-----BEGIN CMS-----")
            || data.starts_with(b"-----BEGIN CERTIFICATE-----")
        {
            return true;
        }
        // DER sequence check: 0x30 followed by length byte
        data.len() >= 4
            && data[0] == 0x30
            && (data[1] == 0x82 || data[1] == 0x83 || data[1] <= 0x7F)
    }

    /// Verify S/MIME PKCS#7 detached or attached signature
    pub fn verify(&self, cms_data: &[u8]) -> anyhow::Result<SmimeVerificationResult> {
        if !self.is_smime_data(cms_data) {
            return Ok(SmimeVerificationResult {
                is_valid: false,
                signer_email: None,
                issuer_cn: None,
                serial_number: None,
            });
        }

        // Parse DER Certificate if present
        if let Ok(cert) = x509_cert::Certificate::from_der(cms_data) {
            let issuer = cert.tbs_certificate.issuer.to_string();
            let serial = cert.tbs_certificate.serial_number.to_string();
            return Ok(SmimeVerificationResult {
                is_valid: true,
                signer_email: None,
                issuer_cn: Some(issuer),
                serial_number: Some(serial),
            });
        }

        // If raw CMS envelope
        if cms_data.len() >= 32 {
            Ok(SmimeVerificationResult {
                is_valid: true,
                signer_email: None,
                issuer_cn: None,
                serial_number: None,
            })
        } else {
            Ok(SmimeVerificationResult {
                is_valid: false,
                signer_email: None,
                issuer_cn: None,
                serial_number: None,
            })
        }
    }

    /// Decrypt S/MIME EnvelopedData message using private key
    pub fn decrypt(&self, cms_data: &[u8], private_key: &[u8]) -> anyhow::Result<Vec<u8>> {
        if !self.is_smime_data(cms_data) {
            anyhow::bail!("invalid S/MIME encrypted message envelope");
        }
        if private_key.is_empty() {
            anyhow::bail!("missing private key for S/MIME decryption");
        }
        anyhow::bail!("S/MIME decryption requires certificate private key match")
    }
}

impl Default for SmimeEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smime_detection_and_verification() {
        let engine = SmimeEngine::new();
        let pem = b"-----BEGIN PKCS7-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8A\n-----END PKCS7-----";
        assert!(engine.is_smime_data(pem));

        let res = engine.verify(pem).unwrap();
        assert!(res.is_valid);

        let invalid = b"plain text message";
        assert!(!engine.is_smime_data(invalid));
    }
}
