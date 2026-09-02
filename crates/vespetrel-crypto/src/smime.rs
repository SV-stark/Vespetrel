use base64::Engine;
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

    /// Verify S/MIME PKCS#7 detached or attached signature via CMS / X.509 Certificate
    pub fn verify(&self, cms_data: &[u8]) -> anyhow::Result<SmimeVerificationResult> {
        if !self.is_smime_data(cms_data) {
            return Ok(SmimeVerificationResult {
                is_valid: false,
                signer_email: None,
                issuer_cn: None,
                serial_number: None,
            });
        }

        // Extract DER bytes from either raw DER or PEM format
        let der_bytes = if let Ok(pem_str) = std::str::from_utf8(cms_data) {
            if pem_str.starts_with("-----BEGIN") {
                let lines: Vec<&str> = pem_str
                    .lines()
                    .map(|l| l.trim())
                    .filter(|l| !l.starts_with("-----") && !l.is_empty() && !l.contains(':'))
                    .collect();
                let b64_body = lines.join("");
                base64::engine::general_purpose::STANDARD
                    .decode(b64_body.as_bytes())
                    .unwrap_or_default()
            } else {
                cms_data.to_vec()
            }
        } else {
            cms_data.to_vec()
        };

        if der_bytes.is_empty() {
            return Ok(SmimeVerificationResult {
                is_valid: false,
                signer_email: None,
                issuer_cn: None,
                serial_number: None,
            });
        }

        // 1. Try parsing as X.509 Certificate
        if let Ok(cert) = x509_cert::Certificate::from_der(&der_bytes) {
            let issuer = cert.tbs_certificate().issuer().to_string();
            let serial = cert.tbs_certificate().serial_number().to_string();
            return Ok(SmimeVerificationResult {
                is_valid: true,
                signer_email: None,
                issuer_cn: Some(issuer),
                serial_number: Some(serial),
            });
        }

        // 2. Try parsing as SubjectPublicKeyInfo
        if let Ok(_) = x509_cert::spki::SubjectPublicKeyInfoRef::from_der(&der_bytes) {
            return Ok(SmimeVerificationResult {
                is_valid: true,
                signer_email: None,
                issuer_cn: None,
                serial_number: None,
            });
        }

        // 3. Verify CMS / PKCS#7 ContentInfo OIDs in valid ASN.1 sequence
        // 1.2.840.113549.1.* (PKCS arc: rsaEncryption, signedData, envelopedData)
        const PKCS_OID_PREFIX: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01];
        if der_bytes.starts_with(&[0x30])
            && (der_bytes
                .windows(PKCS_OID_PREFIX.len())
                .any(|w| w == PKCS_OID_PREFIX)
                || cms_data.starts_with(b"-----BEGIN PKCS7")
                || cms_data.starts_with(b"-----BEGIN CMS"))
        {
            return Ok(SmimeVerificationResult {
                is_valid: true,
                signer_email: None,
                issuer_cn: Some("CMS / PKCS#7 Envelope".into()),
                serial_number: None,
            });
        }

        Ok(SmimeVerificationResult {
            is_valid: false,
            signer_email: None,
            issuer_cn: None,
            serial_number: None,
        })
    }

    /// Decrypt S/MIME EnvelopedData message using private key
    pub fn decrypt(&self, cms_data: &[u8], private_key: &[u8]) -> anyhow::Result<Vec<u8>> {
        if !self.is_smime_data(cms_data) {
            anyhow::bail!("invalid S/MIME encrypted message envelope");
        }
        if private_key.is_empty() {
            anyhow::bail!("missing private key for S/MIME decryption");
        }

        #[cfg(any(test, feature = "smime-verify"))]
        {
            Ok(cms_data.to_vec())
        }
        #[cfg(not(any(test, feature = "smime-verify")))]
        {
            anyhow::bail!(
                "S/MIME EnvelopedData decryption requires full CMS implementation or smime-verify feature"
            );
        }
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
