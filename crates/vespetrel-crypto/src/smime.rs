use base64::Engine;
use cms::cert::CertificateChoices;
use cms::content_info::ContentInfo;
use cms::enveloped_data::EnvelopedData;
use cms::signed_data::{SignedData, SignerInfo};
use der::Decode;
use serde::{Deserialize, Serialize};

/// CMS content types (RFC 5652 §3).
const OID_SIGNED_DATA: &str = "1.2.840.113549.1.7.2";
const OID_ENVELOPED_DATA: &str = "1.2.840.113549.1.7.3";
/// CMS signed attributes (RFC 5652 §11).
const OID_MESSAGE_DIGEST: &str = "1.2.840.113549.1.9.4";
const OID_CONTENT_TYPE: &str = "1.2.840.113549.1.9.3";

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
        // PEM header. Tolerate leading whitespace: MIME parts are routinely indented
        // and `>`-quoted, and a strict `starts_with` made real messages undecodable.
        if let Ok(text) = std::str::from_utf8(data) {
            let trimmed = text.trim_start();
            if trimmed.starts_with("-----BEGIN PKCS7-----")
                || trimmed.starts_with("-----BEGIN CMS-----")
                || trimmed.starts_with("-----BEGIN SIGNED-----")
                || trimmed.starts_with("-----BEGIN CERTIFICATE-----")
            {
                return true;
            }
        }
        // DER SEQUENCE (0x30) with a definite length.
        //
        // Every definite-length form must be accepted: 0x81 covers 128..=255 bytes,
        // 0x84+ covers >= 64 KiB. Rejecting those classified legitimate SignedData
        // blobs as "not S/MIME", silently marking them unverified.
        match data.get(0..2) {
            Some([0x30, b]) => matches!(b, 0x00..=0x7F | 0x81 | 0x82 | 0x83 | 0x84),
            _ => false,
        }
    }

    /// Verify an S/MIME (CMS `SignedData`) signature.
    ///
    /// SECURITY: this fails closed. A positive result means all of the following held:
    ///   1. the blob decodes to a CMS `ContentInfo`/`SignedData`,
    ///   2. the encapsulated content is present (attached, not detached),
    ///   3. the `messageDigest` signed attribute matches a digest of that content,
    ///      computed with the algorithm the signer actually declared,
    ///   4. the `SignatureValue` cryptographically verifies over the DER `SET OF`
    ///      signed attributes using the public key of the certificate the `SignerInfo`
    ///      points at (never "the first certificate in the bag"),
    ///   5. that certificate is inside its validity window.
    ///
    /// The previous implementation checked only (5) plus a digest comparison that could
    /// never execute, so an attacker could attach any self-signed certificate and an
    /// empty `signerInfos` set and have the message reported as validly signed.
    pub fn verify(&self, cms_data: &[u8]) -> anyhow::Result<SmimeVerificationResult> {
        let invalid = SmimeVerificationResult {
            is_valid: false,
            signer_email: None,
            issuer_cn: None,
            serial_number: None,
        };
        if !self.is_smime_data(cms_data) {
            return Ok(invalid);
        }

        let der_bytes = decode_cms(cms_data)?;
        if der_bytes.is_empty() {
            return Ok(invalid);
        }

        // Must be a CMS ContentInfo containing SignedData (stop returning is_valid=true
        // on bare SPKI / bare Cert).
        let content_info = match ContentInfo::from_der(&der_bytes) {
            Ok(ci) => ci,
            Err(_) => return Ok(invalid),
        };
        if content_info.content_type.to_string() != OID_SIGNED_DATA {
            return Ok(invalid);
        }

        let signed_data = match SignedData::from_der(content_info.content.value()) {
            Ok(sd) => sd,
            Err(_) => return Ok(invalid),
        };

        // (2) RFC 5652 §5.1: detached signatures have no eContent. Without the bytes we
        // cannot bind the signature to anything, so refuse rather than guess.
        let econtent = match &signed_data.encap_content_info.econtent {
            Some(e) => e.value().to_vec(),
            None => {
                return Ok(SmimeVerificationResult {
                    is_valid: false,
                    issuer_cn: Some("detached signature: content not supplied".into()),
                    ..invalid
                });
            }
        };

        // (4) Bind each signer to its certificate via SignerInfo.sid.
        let certs: Vec<&x509_cert::Certificate> = signed_data
            .certificates
            .as_ref()
            .map(|set| {
                set.0
                    .as_slice()
                    .iter()
                    .filter_map(|c| match c {
                        CertificateChoices::Certificate(c) => Some(c),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();

        if signed_data.signer_infos.0.is_empty() {
            return Ok(SmimeVerificationResult {
                is_valid: false,
                issuer_cn: Some("SignedData carries no SignerInfo".into()),
                ..invalid
            });
        }

        let now_duration = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();

        for signer in signed_data.signer_infos.0.as_slice() {
            // (4a) Find the certificate this SignerInfo claims, not just any certificate.
            let Some(cert) = certs
                .iter()
                .copied()
                .find(|c| signer_identifier_matches(signer, c))
            else {
                continue;
            };

            // (5) Certificate validity window.
            let tbs = &cert.tbs_certificate;
            let not_before = tbs.validity.not_before.to_date_time().unix_duration();
            let not_after = tbs.validity.not_after.to_date_time().unix_duration();
            if now_duration < not_before || now_duration > not_after {
                continue;
            }

            // (3) The signedAttrs MUST contain a messageDigest over the content, computed
            // with the digest algorithm the signer declared.
            if !message_digest_matches(signer, &econtent) {
                continue;
            }

            // (4b) Verify the SignatureValue over the re-tagged signedAttrs (or the
            // content itself when no signedAttrs are present).
            if !verify_signer_signature(signer, cert, &econtent) {
                continue;
            }

            return Ok(SmimeVerificationResult {
                is_valid: true,
                signer_email: extract_signer_email(&tbs.subject.to_string()),
                issuer_cn: Some(tbs.issuer.to_string()),
                serial_number: Some(tbs.serial_number.to_string()),
            });
        }

        Ok(invalid)
    }

    /// Decrypt S/MIME EnvelopedData message using private key
    pub fn decrypt(&self, cms_data: &[u8], private_key: &[u8]) -> anyhow::Result<Vec<u8>> {
        if !self.is_smime_data(cms_data) {
            anyhow::bail!("invalid S/MIME encrypted message envelope");
        }
        if private_key.is_empty() {
            anyhow::bail!("missing private key for S/MIME decryption");
        }

        let der_bytes = decode_cms(cms_data)?;

        // Parse CMS ContentInfo
        let content_info = ContentInfo::from_der(&der_bytes)
            .map_err(|e| anyhow::anyhow!("invalid S/MIME envelope ContentInfo: {e}"))?;

        // EnvelopedData OID: 1.2.840.113549.1.7.3
        if content_info.content_type.to_string() != OID_ENVELOPED_DATA {
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

/// Decode a CMS blob from either raw DER or a PEM-armored block.
///
/// The previous implementation filtered out any line containing `:` and concatenated the
/// rest, which mangled armored blocks carrying `Proc-Type:`/`DEK-Info:` headers, silently
/// swallowed base64 errors into an empty buffer, and accepted a `BEGIN CERTIFICATE` block as
/// if it were a `SignedData` payload.
fn decode_cms(cms_data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let text = match std::str::from_utf8(cms_data) {
        Ok(t) => t,
        Err(_) => return Ok(cms_data.to_vec()),
    };
    let trimmed = text.trim_start();
    if !trimmed.starts_with("-----BEGIN") {
        return Ok(cms_data.to_vec());
    }

    // Take only the first complete BEGIN/END block.
    let mut body = String::with_capacity(trimmed.len());
    let mut inside = false;
    let mut saw_end = false;
    for line in trimmed.lines() {
        let line = line.trim();
        if line.starts_with("-----BEGIN") {
            inside = true;
            continue;
        }
        if line.starts_with("-----END") {
            saw_end = true;
            break;
        }
        if !inside || line.is_empty() {
            continue;
        }
        // Legacy PEM armor headers (Proc-Type:, DEK-Info:, Version:) are not base64.
        if line.contains(": ") {
            continue;
        }
        body.push_str(line);
    }

    if !saw_end {
        anyhow::bail!("S/MIME PEM block is missing its END marker");
    }

    base64::engine::general_purpose::STANDARD
        .decode(body.as_bytes())
        .map_err(|e| anyhow::anyhow!("S/MIME PEM body is not valid base64: {e}"))
}

/// Does this SignerInfo identify the given certificate?
///
/// `sid` is either an `IssuerAndSerialNumber` or a `SubjectKeyIdentifier`. Matching on it is
/// what binds the reported identity to the message; scanning for "any in-date certificate"
/// does not.
fn signer_identifier_matches(signer: &SignerInfo, cert: &x509_cert::Certificate) -> bool {
    use cms::signed_data::SignerIdentifier;
    let tbs = &cert.tbs_certificate;
    match &signer.sid {
        SignerIdentifier::IssuerAndSerialNumber(i_and_n) => {
            i_and_n.issuer == tbs.issuer && i_and_n.serial_number == tbs.serial_number
        }
        SignerIdentifier::SubjectKeyIdentifier(ski) => {
            // The certificate's SKI lives in an extension; without it we cannot bind the
            // signer, so fail closed rather than guessing.
            tbs.extensions
                .as_ref()
                .and_then(|exts| {
                    exts.iter().find_map(|e| {
                        if e.extn_id == x509_cert::ext::pkix::ID_CE_SUBJECT_KEY_IDENTIFIER {
                            // extn_value is the extension's contents, which for an SKI is the
                            // KeyIdentifier OCTET STRING's contents.
                            Some(e.extn_value.as_bytes().to_vec())
                        } else {
                            None
                        }
                    })
                })
                .is_some_and(|cert_ski| cert_ski == ski.0.as_bytes())
        }
    }
}

/// Verify the `messageDigest` signed attribute against the encapsulated content.
///
/// Fail-closed: returns false when the attribute is absent, malformed, uses an unsupported
/// digest, or does not match. The previous version started `digest_valid` at `true`, parsed
/// the attribute value as a full DER TLV (it is only the contents octets, so the parse
/// essentially always failed), and hardcoded SHA-256 regardless of the declared algorithm.
fn message_digest_matches(signer: &SignerInfo, content: &[u8]) -> bool {
    let Some(attrs) = &signer.signed_attrs else {
        return false;
    };
    let Some(oid) = ring_digest_from_cms_oid(&signer.digest_alg.oid.to_string()) else {
        return false;
    };
    let Some(expected) = find_attr_octet_string(attrs, OID_MESSAGE_DIGEST) else {
        return false;
    };
    // RFC 5652 §5.4 requires a contentType attribute on signedAttrs as well.
    if find_attr_octet_string(attrs, OID_CONTENT_TYPE).is_none() {
        return false;
    }
    let actual = ring::digest::digest(oid, content);
    expected.len() == actual.as_ref().len() && expected == *actual.as_ref()
}

/// Verify `SignerInfo.signature` over the re-tagged `signedAttrs` (RFC 5652 §5.4).
///
/// When no `signedAttrs` are present the signature is over the content itself.
fn verify_signer_signature(
    signer: &SignerInfo,
    cert: &x509_cert::Certificate,
    content: &[u8],
) -> bool {
    use der::Encode;

    let signed_bytes: Vec<u8> = match &signer.signed_attrs {
        // The signature is computed over the attributes DER-encoded as a SET OF
        // (tag 0x31), not over the [0] IMPLICIT re-tagging used on the wire.
        Some(attrs) => {
            let Ok(mut der) = attrs.to_der() else {
                return false;
            };
            if der.is_empty() {
                return false;
            }
            // attrs.to_der() yields a SEQUENCE (0x30); swap the tag byte to 0x31.
            der[0] = 0x31;
            der
        }
        None => content.to_vec(),
    };

    let Some(pk) = public_key_bytes(&cert.tbs_certificate.subject_public_key_info) else {
        return false;
    };
    let Some(alg) = signature_alg_from_cms_oid(&signer.signature_algorithm.oid.to_string()) else {
        return false;
    };
    ring::signature::UnparsedPublicKey::new(alg, &pk)
        .verify(&signed_bytes, signer.signature.as_bytes())
        .is_ok()
}

/// Map a CMS digest-algorithm OID to a ring digest.
///
/// SHA-1 is deliberately not mapped: it is broken for collision resistance, so accepting it
/// would let an attacker re-sign a modified message.
fn ring_digest_from_cms_oid(oid: &str) -> Option<&'static ring::digest::Algorithm> {
    match oid {
        "2.16.840.1.101.3.4.2.1" => Some(&ring::digest::SHA256),
        "2.16.840.1.101.3.4.2.2" => Some(&ring::digest::SHA384),
        "2.16.840.1.101.3.4.2.3" => Some(&ring::digest::SHA512),
        _ => None,
    }
}

/// Map a CMS signature-algorithm OID to a ring verification algorithm.
///
/// Only fixed-hash algorithms are mapped: ring exposes no RSA-PSS with a parameterized hash.
/// Anything else fails closed.
fn signature_alg_from_cms_oid(
    oid: &str,
) -> Option<&'static dyn ring::signature::VerificationAlgorithm> {
    match oid {
        "1.2.840.113549.1.1.11" => {
            Some(&ring::signature::RSA_PKCS1_2048_8192_SHA1_FOR_LEGACY_USE_ONLY)
        }
        "1.2.840.113549.1.1.12" => Some(&ring::signature::RSA_PKCS1_2048_8192_SHA256),
        "1.2.840.113549.1.1.13" => Some(&ring::signature::RSA_PKCS1_2048_8192_SHA384),
        "1.2.840.113549.1.1.14" => Some(&ring::signature::RSA_PKCS1_2048_8192_SHA512),
        "1.2.840.10045.4.3.2" => Some(&ring::signature::ECDSA_P256_SHA256_ASN1),
        "1.2.840.10045.4.3.3" => Some(&ring::signature::ECDSA_P384_SHA384_ASN1),
        "1.3.101.112" => Some(&ring::signature::ED25519),
        _ => None,
    }
}

/// Extract the first attribute with the given OID and decode its value as an OCTET STRING.
fn find_attr_octet_string(attrs: &x509_cert::attr::Attributes, oid_str: &str) -> Option<Vec<u8>> {
    for attr in attrs.iter() {
        if attr.oid.to_string() != oid_str {
            continue;
        }
        for value in attr.values.iter() {
            // `Any::value()` returns only the *contents* octets of the TLV, so decoding it
            // as a full TLV fails. `decode_as` re-parses the tagged value correctly.
            if let Ok(octets) = value.decode_as::<der::asn1::OctetString>() {
                return Some(octets.as_bytes().to_vec());
            }
        }
    }
    None
}

/// Extract the SPKI subjectPublicKey BIT STRING contents, which ring needs.
fn public_key_bytes(spki: &x509_cert::spki::SubjectPublicKeyInfoOwned) -> Option<Vec<u8>> {
    // `as_bytes()` yields the content octets with the unused-bit prefix already stripped,
    // which is exactly what ring's UnparsedPublicKey expects.
    spki.subject_public_key.as_bytes().map(|b| b.to_vec())
}

/// Pull an email-ish identity out of an RFC 4514 subject DN.
fn extract_signer_email(subject_dn: &str) -> Option<String> {
    for part in subject_dn.split(',') {
        let part = part.trim();
        // Only trust an actual emailAddress attribute. The previous code also accepted
        // `CN=`, so a certificate whose CN merely *contained* an '@' was reported as
        // having verified signer identity.
        if let Some(email) = part
            .strip_prefix("emailAddress=")
            .or_else(|| part.strip_prefix("EMAIL="))
        {
            let email = email.trim_matches('"');
            if email.contains('@') && !email.contains(' ') {
                return Some(email.to_ascii_lowercase());
            }
        }
    }
    None
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
