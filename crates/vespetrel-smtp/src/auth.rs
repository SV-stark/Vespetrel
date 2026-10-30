//! SASL Authentication mechanisms for ESMTP (RFC 4954, RFC 4616, RFC 7628, RFC 2195).

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;

use crate::error::{SmtpError, SmtpResult};

/// User authentication credentials.
#[derive(Clone, Default)]
pub struct Credentials {
    pub username: String,
    pub password: Option<String>,
    pub oauth_token: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .field(
                "oauth_token",
                &self.oauth_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("host", &self.host)
            .field("port", &self.port)
            .finish()
    }
}

impl Credentials {
    pub fn new(username: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: None,
            oauth_token: None,
            host: None,
            port: None,
        }
    }

    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    pub fn with_oauth_token(mut self, token: impl Into<String>) -> Self {
        self.oauth_token = Some(token.into());
        self
    }

    pub fn with_server(mut self, host: impl Into<String>, port: u16) -> Self {
        self.host = Some(host.into());
        self.port = Some(port);
        self
    }

    /// SASL PLAIN (RFC 4616): base64("\0" + username + "\0" + password)
    pub fn plain_initial_response(&self) -> SmtpResult<String> {
        let password = self
            .password
            .as_deref()
            .or(self.oauth_token.as_deref())
            .ok_or_else(|| SmtpError::Authentication {
                mechanism: "PLAIN".into(),
                message: "missing password or authentication token".into(),
            })?;

        let mut payload = Vec::new();
        payload.push(0);
        payload.extend_from_slice(self.username.as_bytes());
        payload.push(0);
        payload.extend_from_slice(password.as_bytes());

        Ok(B64.encode(payload))
    }

    /// SASL LOGIN (Username step): base64(username)
    pub fn login_username_response(&self) -> String {
        B64.encode(self.username.as_bytes())
    }

    /// SASL LOGIN (Password step): base64(password)
    pub fn login_password_response(&self) -> SmtpResult<String> {
        let password = self
            .password
            .as_deref()
            .or(self.oauth_token.as_deref())
            .ok_or_else(|| SmtpError::Authentication {
                mechanism: "LOGIN".into(),
                message: "missing password or token".into(),
            })?;
        Ok(B64.encode(password.as_bytes()))
    }

    /// SASL XOAUTH2 (Google / Microsoft 365 OAuth2):
    /// base64("user=" + user + "\x01auth=Bearer " + token + "\x01\x01")
    pub fn xoauth2_initial_response(&self) -> SmtpResult<String> {
        let token = self
            .oauth_token
            .as_deref()
            .ok_or_else(|| SmtpError::Authentication {
                mechanism: "XOAUTH2".into(),
                message: "missing OAuth2 bearer token".into(),
            })?;

        let payload = format!("user={}\x01auth=Bearer {}\x01\x01", self.username, token);
        Ok(B64.encode(payload.as_bytes()))
    }

    /// SASL OAUTHBEARER (RFC 7628):
    /// base64("n,a=" + user + ",\x01host=" + host + "\x01port=" + port + "\x01auth=Bearer " + token + "\x01\x01")
    pub fn oauthbearer_initial_response(&self) -> SmtpResult<String> {
        let token = self
            .oauth_token
            .as_deref()
            .ok_or_else(|| SmtpError::Authentication {
                mechanism: "OAUTHBEARER".into(),
                message: "missing OAuth2 token".into(),
            })?;

        let host = self.host.as_deref().unwrap_or("smtp.example.com");
        let port = self.port.unwrap_or(587);

        let payload = format!(
            "n,a={},\x01host={}\x01port={}\x01auth=Bearer {}\x01\x01",
            self.username, host, port, token
        );
        Ok(B64.encode(payload.as_bytes()))
    }

    /// SASL CRAM-MD5 (RFC 2195):
    /// Decodes base64 challenge, computes HMAC-MD5 with password, encodes `username digest`.
    pub fn cram_md5_response(&self, base64_challenge: &str) -> SmtpResult<String> {
        let password = self
            .password
            .as_deref()
            .ok_or_else(|| SmtpError::Authentication {
                mechanism: "CRAM-MD5".into(),
                message: "missing password for CRAM-MD5".into(),
            })?;

        let challenge_bytes = B64
            .decode(base64_challenge.trim().as_bytes())
            .map_err(|e| SmtpError::Authentication {
                mechanism: "CRAM-MD5".into(),
                message: format!("invalid base64 challenge: {e}"),
            })?;

        let digest = compute_hmac_md5(password.as_bytes(), &challenge_bytes);
        let hex_digest = hex_encode(&digest);
        let response_str = format!("{} {}", self.username, hex_digest);

        Ok(B64.encode(response_str.as_bytes()))
    }
}

/// Computes HMAC-MD5 (RFC 2104 / RFC 1321) in pure Rust without external C dependencies.
fn compute_hmac_md5(key: &[u8], data: &[u8]) -> [u8; 16] {
    let block_size = 64;
    let mut padded_key = [0u8; 64];

    if key.len() > block_size {
        let hashed = md5_hash(key);
        padded_key[..16].copy_from_slice(&hashed);
    } else {
        padded_key[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= padded_key[i];
        opad[i] ^= padded_key[i];
    }

    let mut inner_buf = Vec::with_capacity(64 + data.len());
    inner_buf.extend_from_slice(&ipad);
    inner_buf.extend_from_slice(data);
    let inner_hash = md5_hash(&inner_buf);

    let mut outer_buf = Vec::with_capacity(64 + 16);
    outer_buf.extend_from_slice(&opad);
    outer_buf.extend_from_slice(&inner_hash);
    md5_hash(&outer_buf)
}

/// Pure Rust implementation of RFC 1321 MD5 message-digest algorithm.
fn md5_hash(data: &[u8]) -> [u8; 16] {
    let mut a: u32 = 0x67452301;
    let mut b: u32 = 0xefcdab89;
    let mut c: u32 = 0x98badcfe;
    let mut d: u32 = 0x10325476;

    let original_len_bits = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&original_len_bits.to_le_bytes());

    for chunk in msg.as_chunks::<64>().0 {
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }

        let mut aa = a;
        let mut bb = b;
        let mut cc = c;
        let mut dd = d;

        macro_rules! ff {
            ($a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr, $i:expr) => {
                $a = $a
                    .wrapping_add(($b & $c) | ((!$b) & $d))
                    .wrapping_add(m[$k])
                    .wrapping_add($i);
                $a = $a.rotate_left($s).wrapping_add($b);
            };
        }
        macro_rules! gg {
            ($a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr, $i:expr) => {
                $a = $a
                    .wrapping_add(($b & $d) | ($c & (!$d)))
                    .wrapping_add(m[$k])
                    .wrapping_add($i);
                $a = $a.rotate_left($s).wrapping_add($b);
            };
        }
        macro_rules! hh {
            ($a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr, $i:expr) => {
                $a = $a
                    .wrapping_add($b ^ $c ^ $d)
                    .wrapping_add(m[$k])
                    .wrapping_add($i);
                $a = $a.rotate_left($s).wrapping_add($b);
            };
        }
        macro_rules! ii {
            ($a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr, $i:expr) => {
                $a = $a
                    .wrapping_add($c ^ ($b | (!$d)))
                    .wrapping_add(m[$k])
                    .wrapping_add($i);
                $a = $a.rotate_left($s).wrapping_add($b);
            };
        }

        ff!(aa, bb, cc, dd, 0, 7, 0xd76aa478);
        ff!(dd, aa, bb, cc, 1, 12, 0xe8c7b756);
        ff!(cc, dd, aa, bb, 2, 17, 0x242070db);
        ff!(bb, cc, dd, aa, 3, 22, 0xc1bdceee);
        ff!(aa, bb, cc, dd, 4, 7, 0xf57c0faf);
        ff!(dd, aa, bb, cc, 5, 12, 0x4787c62a);
        ff!(cc, dd, aa, bb, 6, 17, 0xa8304613);
        ff!(bb, cc, dd, aa, 7, 22, 0xfd469501);
        ff!(aa, bb, cc, dd, 8, 7, 0x698098d8);
        ff!(dd, aa, bb, cc, 9, 12, 0x8b44f7af);
        ff!(cc, dd, aa, bb, 10, 17, 0xffff5bb1);
        ff!(bb, cc, dd, aa, 11, 22, 0x895cd7be);
        ff!(aa, bb, cc, dd, 12, 7, 0x6b901122);
        ff!(dd, aa, bb, cc, 13, 12, 0xfd987193);
        ff!(cc, dd, aa, bb, 14, 17, 0xa679438e);
        ff!(bb, cc, dd, aa, 15, 22, 0x49b40821);

        gg!(aa, bb, cc, dd, 1, 5, 0xf61e2562);
        gg!(dd, aa, bb, cc, 6, 9, 0xc040b340);
        gg!(cc, dd, aa, bb, 11, 14, 0x265e5a51);
        gg!(bb, cc, dd, aa, 0, 20, 0xe9b6c7aa);
        gg!(aa, bb, cc, dd, 5, 5, 0xd62f105d);
        gg!(dd, aa, bb, cc, 10, 9, 0x02441453);
        gg!(cc, dd, aa, bb, 15, 14, 0xd8a1e681);
        gg!(bb, cc, dd, aa, 4, 20, 0xe7d3fbc8);
        gg!(aa, bb, cc, dd, 9, 5, 0x21e1cde6);
        gg!(dd, aa, bb, cc, 14, 9, 0xc33707d6);
        gg!(cc, dd, aa, bb, 3, 14, 0xf4d50d87);
        gg!(bb, cc, dd, aa, 8, 20, 0x455a14ed);
        gg!(aa, bb, cc, dd, 13, 5, 0xa9e3e905);
        gg!(dd, aa, bb, cc, 2, 9, 0xfcefa3f8);
        gg!(cc, dd, aa, bb, 7, 14, 0x676f02d9);
        gg!(bb, cc, dd, aa, 12, 20, 0x8d2a4c8a);

        hh!(aa, bb, cc, dd, 5, 4, 0xfffa3942);
        hh!(dd, aa, bb, cc, 8, 11, 0x8771f681);
        hh!(cc, dd, aa, bb, 11, 16, 0x6d9d6122);
        hh!(bb, cc, dd, aa, 14, 23, 0xfde5380c);
        hh!(aa, bb, cc, dd, 1, 4, 0xa4beea44);
        hh!(dd, aa, bb, cc, 4, 11, 0x4bdecfa9);
        hh!(cc, dd, aa, bb, 7, 16, 0xf6bb4b60);
        hh!(bb, cc, dd, aa, 10, 23, 0xbebfbc70);
        hh!(aa, bb, cc, dd, 13, 4, 0x289b7ec6);
        hh!(dd, aa, bb, cc, 0, 11, 0xeaa127fa);
        hh!(cc, dd, aa, bb, 3, 16, 0xd4ef3085);
        hh!(bb, cc, dd, aa, 6, 23, 0x04881d05);
        hh!(aa, bb, cc, dd, 9, 4, 0xd9d4d039);
        hh!(dd, aa, bb, cc, 12, 11, 0xe6db99e5);
        hh!(cc, dd, aa, bb, 15, 16, 0x1fa27cf8);
        hh!(bb, cc, dd, aa, 2, 23, 0xc4ac5665);

        ii!(aa, bb, cc, dd, 0, 6, 0xf4292244);
        ii!(dd, aa, bb, cc, 7, 10, 0x432aff97);
        ii!(cc, dd, aa, bb, 14, 15, 0xab9423a7);
        ii!(bb, cc, dd, aa, 5, 21, 0xfc93a039);
        ii!(aa, bb, cc, dd, 12, 6, 0x655b59c3);
        ii!(dd, aa, bb, cc, 3, 10, 0x8f0ccc92);
        ii!(cc, dd, aa, bb, 10, 15, 0xffeff47d);
        ii!(bb, cc, dd, aa, 1, 21, 0x85845dd1);
        ii!(aa, bb, cc, dd, 8, 6, 0x6fa87e4f);
        ii!(dd, aa, bb, cc, 15, 10, 0xfe2ce6e0);
        ii!(cc, dd, aa, bb, 6, 15, 0xa3014314);
        ii!(bb, cc, dd, aa, 13, 21, 0x4e0811a1);
        ii!(aa, bb, cc, dd, 4, 6, 0xf7537e82);
        ii!(dd, aa, bb, cc, 11, 10, 0xbd3af235);
        ii!(cc, dd, aa, bb, 2, 15, 0x2ad7d2bb);
        ii!(bb, cc, dd, aa, 9, 21, 0xeb86d391);

        a = a.wrapping_add(aa);
        b = b.wrapping_add(bb);
        c = c.wrapping_add(cc);
        d = d.wrapping_add(dd);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a.to_le_bytes());
    out[4..8].copy_from_slice(&b.to_le_bytes());
    out[8..12].copy_from_slice(&c.to_le_bytes());
    out[12..16].copy_from_slice(&d.to_le_bytes());
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_auth() {
        let creds = Credentials::new("alice").with_password("secret123");
        let encoded = creds.plain_initial_response().unwrap();
        let decoded = B64.decode(encoded.as_bytes()).unwrap();
        assert_eq!(decoded, b"\0alice\0secret123");
    }

    #[test]
    fn test_login_auth() {
        let creds = Credentials::new("alice").with_password("secret123");
        assert_eq!(
            B64.decode(creds.login_username_response().as_bytes())
                .unwrap(),
            b"alice"
        );
        assert_eq!(
            B64.decode(creds.login_password_response().unwrap().as_bytes())
                .unwrap(),
            b"secret123"
        );
    }

    #[test]
    fn test_xoauth2_auth() {
        let creds = Credentials::new("alice@gmail.com").with_oauth_token("ya29.v-mock-token");
        let encoded = creds.xoauth2_initial_response().unwrap();
        let decoded = String::from_utf8(B64.decode(encoded.as_bytes()).unwrap()).unwrap();
        assert_eq!(
            decoded,
            "user=alice@gmail.com\x01auth=Bearer ya29.v-mock-token\x01\x01"
        );
    }

    #[test]
    fn test_oauthbearer_auth() {
        let creds = Credentials::new("alice@outlook.com")
            .with_oauth_token("EwBA...token")
            .with_server("smtp.office365.com", 587);
        let encoded = creds.oauthbearer_initial_response().unwrap();
        let decoded = String::from_utf8(B64.decode(encoded.as_bytes()).unwrap()).unwrap();
        assert_eq!(
            decoded,
            "n,a=alice@outlook.com,\x01host=smtp.office365.com\x01port=587\x01auth=Bearer EwBA...token\x01\x01"
        );
    }

    #[test]
    fn test_cram_md5() {
        let creds = Credentials::new("tim").with_password("tanstaafl");
        // Challenge: `<1896.697170952@postoffice.reston.mci.net>` in base64:
        let challenge = "PDE4OTYuNjk3MTcwOTUyQHBvc3RvZmZpY2UucmVzdG9uLm1jaS5uZXQ+";
        let resp = creds.cram_md5_response(challenge).unwrap();
        let decoded = String::from_utf8(B64.decode(resp.as_bytes()).unwrap()).unwrap();
        assert_eq!(decoded, "tim 84ec099ec2b1e79cad1d3bc135300ac9");
    }
}
