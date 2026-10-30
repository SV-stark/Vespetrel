//! RFC 6376 and RFC 8463 DKIM (DomainKeys Identified Mail) signing engine.

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{KeyPair, RSA_PKCS1_SHA256, RsaKeyPair};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::error::{SmtpError, SmtpResult};

/// DKIM canonicalization algorithms (RFC 6376 §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Canonicalization {
    #[default]
    RelaxedRelaxed,
    SimpleSimple,
    RelaxedSimple,
    SimpleRelaxed,
}

impl Canonicalization {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RelaxedRelaxed => "relaxed/relaxed",
            Self::SimpleSimple => "simple/simple",
            Self::RelaxedSimple => "relaxed/simple",
            Self::SimpleRelaxed => "simple/relaxed",
        }
    }
}

/// DKIM Signature algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DkimAlgorithm {
    #[default]
    RsaSha256,
    Ed25519Sha256,
}

impl DkimAlgorithm {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RsaSha256 => "rsa-sha256",
            Self::Ed25519Sha256 => "ed25519-sha256",
        }
    }
}

/// Configuration options for signing messages with DKIM.
#[derive(Clone)]
pub struct DkimSigner {
    pub domain: String,
    pub selector: String,
    pub private_key: String,
    pub canonicalization: Canonicalization,
    pub algorithm: DkimAlgorithm,
    pub headers_to_sign: Vec<String>,
    pub include_timestamp: bool,
    pub expire_seconds: Option<u64>,
}

impl std::fmt::Debug for DkimSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DkimSigner")
            .field("domain", &self.domain)
            .field("selector", &self.selector)
            .field("private_key", &"[REDACTED]")
            .field("canonicalization", &self.canonicalization)
            .field("algorithm", &self.algorithm)
            .field("headers_to_sign", &self.headers_to_sign)
            .finish()
    }
}

impl DkimSigner {
    pub fn new(
        domain: impl Into<String>,
        selector: impl Into<String>,
        private_key: impl Into<String>,
    ) -> SmtpResult<Self> {
        let domain_str = domain.into();
        let selector_str = selector.into();

        if !is_valid_dkim_domain(&domain_str) {
            return Err(SmtpError::Dkim(format!(
                "invalid DKIM signing domain syntax: '{domain_str}'"
            )));
        }
        if !is_valid_dkim_selector(&selector_str) {
            return Err(SmtpError::Dkim(format!(
                "invalid DKIM selector syntax: '{selector_str}'"
            )));
        }

        Ok(Self {
            domain: domain_str,
            selector: selector_str,
            private_key: private_key.into(),
            canonicalization: Canonicalization::RelaxedRelaxed,
            algorithm: DkimAlgorithm::RsaSha256,
            headers_to_sign: vec![
                "from".into(),
                "to".into(),
                "subject".into(),
                "date".into(),
                "message-id".into(),
            ],
            include_timestamp: true,
            expire_seconds: None,
        })
    }

    pub fn with_canonicalization(mut self, canon: Canonicalization) -> Self {
        self.canonicalization = canon;
        self
    }

    pub fn with_headers(mut self, headers: Vec<String>) -> Self {
        self.headers_to_sign = headers;
        self
    }

    /// Signs an RFC 5322 serialized email message, returning the email with the `DKIM-Signature` header prepended.
    pub fn sign_message(&self, raw_message: &[u8]) -> SmtpResult<Vec<u8>> {
        // Split header block and body
        let (header_bytes, body_bytes) = split_message_header_and_body(raw_message);

        // 1. Compute Body Hash (bh)
        let canon_body = match self.canonicalization {
            Canonicalization::RelaxedRelaxed | Canonicalization::SimpleRelaxed => {
                canonicalize_relaxed_body_bytes(body_bytes)
            }
            Canonicalization::SimpleSimple | Canonicalization::RelaxedSimple => {
                canonicalize_simple_body_bytes(body_bytes)
            }
        };

        let mut hasher = Sha256::new();
        hasher.update(&canon_body);
        let bh = B64.encode(hasher.finalize());

        // 2. Determine signed headers present in message
        let fields = split_header_fields(header_bytes);
        let mut actual_signed_names = Vec::new();

        for want_name in &self.headers_to_sign {
            let lower_want = want_name.to_ascii_lowercase();
            if fields.iter().any(|(n, _)| n == &lower_want) {
                actual_signed_names.push(lower_want);
            }
        }

        if !actual_signed_names.iter().any(|n| n == "from") {
            return Err(SmtpError::Dkim(
                "cannot sign message: missing mandatory 'From' header".into(),
            ));
        }

        let signed_headers_list = actual_signed_names.join(":");

        // 3. Construct signature header tag-value string (without b= signature value)
        let mut sig_tags = format!(
            "v=1; a={}; d={}; s={}; c={}; q=dns/txt; h={}; bh={};",
            self.algorithm.as_str(),
            self.domain,
            self.selector,
            self.canonicalization.as_str(),
            signed_headers_list,
            bh
        );

        if self.include_timestamp {
            let now = Utc::now().timestamp();
            sig_tags.push_str(&format!(" t={now};"));
            if let Some(exp) = self.expire_seconds {
                let expires = now + (exp as i64);
                sig_tags.push_str(&format!(" x={expires};"));
            }
        }

        sig_tags.push_str(" b=");

        // 4. Canonicalize headers to sign
        let mut signed_data = match self.canonicalization {
            Canonicalization::RelaxedRelaxed | Canonicalization::RelaxedSimple => {
                canonicalize_headers_relaxed(&fields, &actual_signed_names)
            }
            Canonicalization::SimpleSimple | Canonicalization::SimpleRelaxed => {
                canonicalize_headers_simple(header_bytes, &actual_signed_names)
            }
        };

        // Append the DKIM-Signature header itself (without b= value)
        let canon_sig_header = canonicalize_relaxed_header("dkim-signature", &sig_tags);
        signed_data.extend_from_slice(canon_sig_header.as_bytes());

        // 5. Compute cryptographic signature over signed_data
        let der_bytes = decode_private_key(&self.private_key)?;
        let key_pair = RsaKeyPair::from_pkcs8(&der_bytes)
            .or_else(|_| RsaKeyPair::from_der(&der_bytes))
            .map_err(|e| SmtpError::Dkim(format!("invalid RSA private key for DKIM: {e}")))?;

        let rng = SystemRandom::new();
        let mut sig = vec![0; key_pair.public_key().modulus_len()];
        key_pair
            .sign(&RSA_PKCS1_SHA256, &rng, &signed_data, &mut sig)
            .map_err(|e| SmtpError::Dkim(format!("RSASSA-PKCS1-v1_5 signing failed: {e}")))?;

        let b = B64.encode(&sig);
        let final_dkim_header = format!("DKIM-Signature: {sig_tags}{b}\r\n");

        // Prepend DKIM-Signature header to original message
        let mut final_message = Vec::with_capacity(final_dkim_header.len() + raw_message.len());
        final_message.extend_from_slice(final_dkim_header.as_bytes());
        final_message.extend_from_slice(raw_message);

        Ok(final_message)
    }
}

fn split_message_header_and_body(raw: &[u8]) -> (&[u8], &[u8]) {
    if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
        (&raw[..pos], &raw[pos + 4..])
    } else if let Some(pos) = raw.windows(2).position(|w| w == b"\n\n") {
        (&raw[..pos], &raw[pos + 2..])
    } else {
        (raw, &[])
    }
}

pub fn canonicalize_relaxed_body_bytes(body: &[u8]) -> Vec<u8> {
    if body.is_empty() {
        return Vec::new();
    }
    let s = String::from_utf8_lossy(body);
    let mut lines = Vec::new();
    for line in s.lines() {
        let mut canon_line = String::with_capacity(line.len());
        let mut in_wsp = false;
        for c in line.trim_end().chars() {
            if c == ' ' || c == '\t' {
                if !in_wsp {
                    canon_line.push(' ');
                    in_wsp = true;
                }
            } else {
                canon_line.push(c);
                in_wsp = false;
            }
        }
        lines.push(canon_line);
    }
    while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    if lines.is_empty() {
        return Vec::new();
    }
    let mut result = lines.join("\r\n");
    result.push_str("\r\n");
    result.into_bytes()
}

pub fn canonicalize_simple_body_bytes(body: &[u8]) -> Vec<u8> {
    if body.is_empty() {
        return Vec::new();
    }
    let mut end = body.len();
    while end >= 2 && &body[end - 2..end] == b"\r\n" {
        end -= 2;
    }
    while end >= 1 && (body[end - 1] == b'\n' || body[end - 1] == b'\r') {
        end -= 1;
    }
    if end == 0 {
        return Vec::new();
    }
    let mut out = body[..end].to_vec();
    out.extend_from_slice(b"\r\n");
    out
}

pub fn canonicalize_relaxed_header(name: &str, value: &str) -> String {
    let lower_name = name.trim().to_ascii_lowercase();
    let mut canon_val = String::with_capacity(value.len());
    let mut in_wsp = false;
    for c in value.trim().chars() {
        if c == ' ' || c == '\t' || c == '\r' || c == '\n' {
            if !in_wsp {
                canon_val.push(' ');
                in_wsp = true;
            }
        } else {
            canon_val.push(c);
            in_wsp = false;
        }
    }
    format!("{lower_name}:{canon_val}\r\n")
}

pub fn split_header_fields(raw: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(raw);
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in text.split_inclusive(['\r', '\n']) {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = fields.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim_start());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            fields.push((name.trim().to_ascii_lowercase(), value.to_string()));
        }
    }
    fields
}

pub fn canonicalize_headers_relaxed(fields: &[(String, String)], names: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for name in names {
        let want = name.trim().to_ascii_lowercase();
        if let Some((_, val)) = fields.iter().rev().find(|(n, _)| *n == want) {
            out.extend_from_slice(canonicalize_relaxed_header(name, val).as_bytes());
        }
    }
    out
}

pub fn canonicalize_headers_simple(header_bytes: &[u8], names: &[String]) -> Vec<u8> {
    let text = String::from_utf8_lossy(header_bytes);
    let mut out = Vec::new();
    for name in names {
        let want = name.to_ascii_lowercase();
        // find matching line
        for line in text.lines() {
            if line
                .split_once(':')
                .is_some_and(|(k, _)| k.trim().to_ascii_lowercase() == want)
            {
                out.extend_from_slice(line.as_bytes());
                out.extend_from_slice(b"\r\n");
                break;
            }
        }
    }
    out
}

pub fn canonicalize_relaxed_body(body: &str) -> Vec<u8> {
    canonicalize_relaxed_body_bytes(body.as_bytes())
}

pub fn canonicalize_signed_headers(raw: &[u8], names: &[&str]) -> Option<Vec<u8>> {
    let fields = split_header_fields(raw);
    let mut out = Vec::new();
    for name in names {
        let want = name.trim().to_ascii_lowercase();
        let value = fields
            .iter()
            .rev()
            .find(|(n, _)| *n == want)
            .map(|(_, v)| v.as_str())?;
        out.extend_from_slice(canonicalize_relaxed_header(name, value).as_bytes());
    }
    Some(out)
}

pub fn is_valid_dkim_domain(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > 255 || domain.ends_with('.') {
        return false;
    }
    domain.split('.').all(valid_ldh_label)
}

pub fn is_valid_dkim_selector(selector: &str) -> bool {
    if selector.is_empty() || selector.len() > 255 {
        return false;
    }
    selector.split('.').all(valid_ldh_label)
}

fn valid_ldh_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && !label.starts_with('-')
        && !label.ends_with('-')
}

fn decode_private_key(key: &str) -> SmtpResult<Vec<u8>> {
    if key.contains("-----BEGIN") {
        let stripped = key
            .lines()
            .filter(|l| !l.starts_with("-----"))
            .collect::<Vec<_>>()
            .concat();
        B64.decode(stripped.trim().as_bytes())
            .map_err(|e| SmtpError::Dkim(format!("invalid PEM base64: {e}")))
    } else if let Ok(decoded) = B64.decode(key.trim().as_bytes()) {
        Ok(decoded)
    } else {
        Ok(key.as_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSA_PKCS8_TEST: &str = "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCy+OYeW6EsC8TQ4z6RiAIxRM5kEz6gbYkyemoQXiZLDrJf4yVj5EfI4lwEbAAr6D2v+QBJofXQWdigjMooqk7RW5lr0LYsNN7UhfnWsOt74BGzuQ8k6zf2m1EJoD20I04UyWfil+IMvpDVqcd6UbZZCGlLjlPSEs1B6tncbHHSPtHhGJcCpeyO5/KFqcDc9oXwB4skNQ6eRA2KYLY9O4jqThjHueNlA4hic6r9CyF0hpGZtxe90F5hwJ8s3TLBWTn8n5tZD2B8euCTBmgMiH6fnOKOo22VEqzGh9bSioxm+EzFzB0nYoPbAri7dgch7X3r7dNGpkhjwzjhp2R4MYeBAgMBAAECggEAe1E/Fmniivvs+FWsVhCWGiaj45bTDy2KXEq27GJDFnKg+6sCp2qy/7rg1ncoQxi58Jes2A+N1asitbVs0kpPFrh75SshayJe66cI+CJdj7Rb3i9EPRcKL5TjaLON8KJm+bGxMBOhQVDJcT+T4DePYpeGHfaK0PP9lE7jIJtkbg2Nzvbw1XHmnTXdCD36q3gt2uLwnydQXXtoK6WfXmz1hgfiRkdhAo5bOwKpupgGzoBR3aZFtNhQqRVnz/dR4qqQHpvpatmnoxwrbRG0b0YcIheutUictZpbk2JNpEjEfKG8JvkFbsCFsZVEyYE4K2UzLJFof7mb9JCOlOd4B/HZ1QKBgQDe0uDpGC/6jVmHId4xEr3b7+pDxnkZAOy8xXRBl/hXxDAjNU+01EACRFVoWnHWHDeZVfILdh6Az8N+wxl4HY1CtVAhujXU/ANr6cRyPPKMm/P0YuJ85RsxmnOuGsYeZmiEwspJVASJ472KW0mOJnMieqGcGarn3ajAacGqu2gWpwKBgQDNnpUyuVKHssOrBmWBwZ9F6uhK2iSoxtLAQfUBK9KWNN2hURMXZW1QoKT3Kp/0CDEgIUz9v9zfylER2NsO7C1p2azj+W+jNKkOsITgIeAcrcp5wYS2K2q4x6U+JkARkKna0V4PwsHBgyZrIIycd6y6zF5hBMan+Hxy8DfggZLdlwKBgQCOtrP0t1grepLn2QpNlfpiPpRlml3/ZLc75J+kT2hxFifatQ96+yKQESI+twcIIoR9wi1Hp/y7ddZ5fw31/791BVnwcCqAYnTyjgQTQvP6mPwz/42efsLfD1SeI2nXGLJCrdwQAS7y/hls3zEKSZgecjrGFy5+WVr2+gVfi66MKwKBgAcH8jAe2Cydt0Uk3dm3Bjw80R6mIPTIf7JlTvxwRC4wtpdqj02QgVFtfNaa1YdhtFRV7y0KH4Jjh6wljzAOcWsaL2hIQkIBbfp7nL+RSPmSE8dgD6qvB2I0KXlbk3tGSBicaiv9y+RTGMA3B7fd+8ETdfK5WBWsUI0Zm7+Ijr4XAoGBANLc+H3c9h1faSb2YSN+QHtcyWJxhZONrE7pbBTMuVbgewtqjR2Et4AvmIuhwNfmZI6gnrbd1TWvGeGhZbdFQHpgms/92HjfAl61rttB+LgaKWoMod4XWqMxtChLNBpLqXEcY1xND9gAM66IGSPw7XKFiygzgDBucFcdJdwMo/Jk";

    #[test]
    fn test_dkim_signing_flow() {
        let signer = DkimSigner::new("example.com", "default", RSA_PKCS8_TEST).unwrap();
        let raw = b"From: alice@example.com\r\nTo: bob@example.com\r\nSubject: Test Subject\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\nMessage-ID: <123@example.com>\r\n\r\nHello World\r\n";
        let signed = signer.sign_message(raw).unwrap();
        let signed_str = String::from_utf8(signed).unwrap();

        assert!(signed_str.contains("DKIM-Signature:"));
        assert!(signed_str.contains("v=1;"));
        assert!(signed_str.contains("a=rsa-sha256;"));
        assert!(signed_str.contains("d=example.com;"));
        assert!(signed_str.contains("s=default;"));
        assert!(signed_str.contains("bh="));
        assert!(signed_str.contains("b="));
    }
}
