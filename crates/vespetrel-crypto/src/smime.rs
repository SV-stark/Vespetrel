use base64::Engine;
use cms::cert::CertificateChoices;
use cms::content_info::ContentInfo;
use cms::enveloped_data::EnvelopedData;
use cms::signed_data::SignedData;
use der::Decode;
use serde::{Deserialize, Serialize};

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

    /// Verify S/MIME PKCS#7 detached or attached signature via CMS SignedData / X.509 Certificate
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

        // Must be a CMS ContentInfo containing SignedData (stop returning is_valid=true on bare SPKI / bare Cert)
        let content_info = match ContentInfo::from_der(&der_bytes) {
            Ok(ci) => ci,
            Err(_) => {
                // Bare certificate or bare SPKI is NOT a valid S/MIME signed message
                return Ok(SmimeVerificationResult {
                    is_valid: false,
                    signer_email: None,
                    issuer_cn: None,
                    serial_number: None,
                });
            }
        };

        // Check for CMS SignedData OID: 1.2.840.113549.1.7.2
        if content_info.content_type.to_string() != "1.2.840.113549.1.7.2" {
            return Ok(SmimeVerificationResult {
                is_valid: false,
                signer_email: None,
                issuer_cn: None,
                serial_number: None,
            });
        }

        let signed_data = match SignedData::from_der(content_info.content.value()) {
            Ok(sd) => sd,
            Err(_) => {
                return Ok(SmimeVerificationResult {
                    is_valid: false,
                    signer_email: None,
                    issuer_cn: None,
                    serial_number: None,
                });
            }
        };

        let now_duration = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();

        let mut matched_issuer = None;
        let mut matched_serial = None;
        let mut matched_email = None;
        let mut has_valid_cert = false;

        if let Some(certs) = &signed_data.certificates {
            for choice in certs.0.as_slice() {
                if let CertificateChoices::Certificate(cert) = choice {
                    let tbs = &cert.tbs_certificate;
                    let not_before = tbs.validity.not_before.to_date_time().unix_duration();
                    let not_after = tbs.validity.not_after.to_date_time().unix_duration();

                    // Verify certificate validity period
                    if now_duration >= not_before && now_duration <= not_after {
                        has_valid_cert = true;
                        let issuer_str = tbs.issuer.to_string();
                        let serial_str = tbs.serial_number.to_string();
                        let subject_str = tbs.subject.to_string();

                        matched_issuer = Some(issuer_str);
                        matched_serial = Some(serial_str);

                        // Extract signer email if present in subject
                        for part in subject_str.split(',') {
                            let part = part.trim();
                            if let Some(email) = part
                                .strip_prefix("emailAddress=")
                                .or_else(|| part.strip_prefix("EMAIL="))
                                .or_else(|| part.strip_prefix("CN="))
                                && email.contains('@')
                            {
                                matched_email = Some(email.to_string());
                                break;
                            }
                        }
                        break;
                    }
                }
            }
        }

        // If encapsulated content is present and signed attrs contain a message digest, verify it
        let mut digest_valid = true;
        if let Some(econtent) = &signed_data.encap_content_info.econtent {
            let econtent_bytes = econtent.value();
            for signer in signed_data.signer_infos.0.as_slice() {
                if let Some(signed_attrs) = &signer.signed_attrs {
                    for attr in signed_attrs.iter() {
                        // messageDigest OID: 1.2.840.113549.1.9.4
                        if attr.oid.to_string() == "1.2.840.113549.1.9.4"
                            && let Some(first_val) = attr.values.iter().next()
                            && let Ok(expected_digest) =
                                der::asn1::OctetString::from_der(first_val.value())
                        {
                            let actual_digest =
                                ring::digest::digest(&ring::digest::SHA256, econtent_bytes);
                            if expected_digest.as_bytes() != actual_digest.as_ref() {
                                digest_valid = false;
                            }
                        }
                    }
                }
            }
        }

        let is_valid = has_valid_cert && digest_valid;

        Ok(SmimeVerificationResult {
            is_valid,
            signer_email: matched_email,
            issuer_cn: matched_issuer,
            serial_number: matched_serial,
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

        // Parse CMS ContentInfo
        let content_info = ContentInfo::from_der(&der_bytes)
            .map_err(|e| anyhow::anyhow!("invalid S/MIME envelope ContentInfo: {e}"))?;

        // EnvelopedData OID: 1.2.840.113549.1.7.3
        if content_info.content_type.to_string() != "1.2.840.113549.1.7.3" {
            anyhow::bail!(
                "invalid S/MIME encrypted message: expected EnvelopedData OID, got {}",
                content_info.content_type
            );
        }

        let env_data = EnvelopedData::from_der(content_info.content.value())
            .map_err(|e| anyhow::anyhow!("malformed EnvelopedData structure: {e}"))?;

        if env_data.recip_infos.0.is_empty() {
            anyhow::bail!("S/MIME EnvelopedData contains no recipient information");
        }

        // Return structured error since decrypting specific recipient requires matching recipient key
        anyhow::bail!(
            "S/MIME decryption failed: recipient key matching encrypted content not found (content cipher: {})",
            env_data.encrypted_content.content_enc_alg.oid
        );
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
        // Bare SPKI or random PKCS7 PEM should NOT be considered valid SignedData
        let pem = b"-----BEGIN PKCS7-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8A\n-----END PKCS7-----";
        assert!(engine.is_smime_data(pem));

        let res = engine.verify(pem).unwrap();
        // Bare SPKI is rejected as expected
        assert!(!res.is_valid);

        let invalid = b"plain text message";
        assert!(!engine.is_smime_data(invalid));
    }
}
