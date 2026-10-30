//! ESMTP Extensions parsing according to RFC 5321, RFC 1869, RFC 4954, RFC 3461.

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AuthMechanism {
    Plain,
    Login,
    XOAuth2,
    OAuthBearer,
    CramMd5,
    ScramSha1,
    ScramSha256,
    DigestMd5,
    Anonymous,
    Custom(String),
}

impl AuthMechanism {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Plain => "PLAIN",
            Self::Login => "LOGIN",
            Self::XOAuth2 => "XOAUTH2",
            Self::OAuthBearer => "OAUTHBEARER",
            Self::CramMd5 => "CRAM-MD5",
            Self::ScramSha1 => "SCRAM-SHA-1",
            Self::ScramSha256 => "SCRAM-SHA-256",
            Self::DigestMd5 => "DIGEST-MD5",
            Self::Anonymous => "ANONYMOUS",
            Self::Custom(s) => s.as_str(),
        }
    }
}

impl fmt::Display for AuthMechanism {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for AuthMechanism {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let upper = s.trim().to_ascii_uppercase();
        let mech = match upper.as_str() {
            "PLAIN" => Self::Plain,
            "LOGIN" => Self::Login,
            "XOAUTH2" => Self::XOAuth2,
            "OAUTHBEARER" => Self::OAuthBearer,
            "CRAM-MD5" => Self::CramMd5,
            "SCRAM-SHA-1" => Self::ScramSha1,
            "SCRAM-SHA-256" => Self::ScramSha256,
            "DIGEST-MD5" => Self::DigestMd5,
            "ANONYMOUS" => Self::Anonymous,
            _ => Self::Custom(upper),
        };
        Ok(mech)
    }
}

/// Discovered server ESMTP capabilities.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerCapabilities {
    pub supports_8bitmime: bool,
    pub max_message_size: Option<u64>,
    pub supports_pipelining: bool,
    pub supports_starttls: bool,
    pub auth_mechanisms: HashSet<AuthMechanism>,
    pub supports_dsn: bool,
    pub supports_smtputf8: bool,
    pub supports_chunking: bool,
    pub supports_enhanced_status_codes: bool,
    pub supports_binarymime: bool,
    pub raw_extensions: Vec<String>,
}

impl ServerCapabilities {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses EHLO 250 response lines to extract advertised ESMTP capabilities.
    pub fn from_ehlo_lines<S: AsRef<str>>(lines: &[S]) -> Self {
        let mut caps = Self::default();

        for line in lines {
            let line_ref = line.as_ref().trim();
            caps.raw_extensions.push(line_ref.to_string());

            let mut parts = line_ref.split_whitespace();
            let Some(keyword) = parts.next() else {
                continue;
            };
            let keyword_upper = keyword.to_ascii_uppercase();

            match keyword_upper.as_str() {
                "8BITMIME" => caps.supports_8bitmime = true,
                "PIPELINING" => caps.supports_pipelining = true,
                "STARTTLS" => caps.supports_starttls = true,
                "DSN" => caps.supports_dsn = true,
                "SMTPUTF8" => caps.supports_smtputf8 = true,
                "CHUNKING" => caps.supports_chunking = true,
                "ENHANCEDSTATUSCODES" => caps.supports_enhanced_status_codes = true,
                "BINARYMIME" => caps.supports_binarymime = true,
                "SIZE" => {
                    if let Some(limit) = parts.next().and_then(|param| param.parse::<u64>().ok()) {
                        caps.max_message_size = Some(limit);
                    }
                }
                "AUTH" => {
                    for mech_str in parts {
                        let mech: AuthMechanism = mech_str.parse().unwrap();
                        caps.auth_mechanisms.insert(mech);
                    }
                }
                _ => {}
            }
        }

        caps
    }

    pub fn supports_auth(&self, mech: &AuthMechanism) -> bool {
        self.auth_mechanisms.contains(mech)
    }

    /// Selects the strongest available authentication mechanism from the server capabilities
    /// given the user's available credentials.
    pub fn select_best_auth(
        &self,
        has_oauth_token: bool,
        has_password: bool,
    ) -> Option<AuthMechanism> {
        if has_oauth_token {
            if self.supports_auth(&AuthMechanism::XOAuth2) {
                return Some(AuthMechanism::XOAuth2);
            }
            if self.supports_auth(&AuthMechanism::OAuthBearer) {
                return Some(AuthMechanism::OAuthBearer);
            }
        }

        if has_password {
            // Prefer modern salted hashes if supported
            if self.supports_auth(&AuthMechanism::ScramSha256) {
                return Some(AuthMechanism::ScramSha256);
            }
            if self.supports_auth(&AuthMechanism::ScramSha1) {
                return Some(AuthMechanism::ScramSha1);
            }
            if self.supports_auth(&AuthMechanism::CramMd5) {
                return Some(AuthMechanism::CramMd5);
            }
            if self.supports_auth(&AuthMechanism::Plain) {
                return Some(AuthMechanism::Plain);
            }
            if self.supports_auth(&AuthMechanism::Login) {
                return Some(AuthMechanism::Login);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ehlo_capabilities() {
        let lines = [
            "smtp.gmail.com at your service, [203.0.113.1]",
            "SIZE 35882577",
            "8BITMIME",
            "STARTTLS",
            "ENHANCEDSTATUSCODES",
            "PIPELINING",
            "CHUNKING",
            "SMTPUTF8",
            "AUTH LOGIN PLAIN XOAUTH2 PLAIN-CLIENTTOKEN OAUTHBEARER XOAUTH",
        ];

        let caps = ServerCapabilities::from_ehlo_lines(&lines);
        assert!(caps.supports_8bitmime);
        assert_eq!(caps.max_message_size, Some(35882577));
        assert!(caps.supports_pipelining);
        assert!(caps.supports_starttls);
        assert!(caps.supports_smtputf8);
        assert!(caps.supports_chunking);
        assert!(caps.supports_enhanced_status_codes);
        assert!(caps.supports_auth(&AuthMechanism::XOAuth2));
        assert!(caps.supports_auth(&AuthMechanism::OAuthBearer));
        assert!(caps.supports_auth(&AuthMechanism::Plain));
        assert!(caps.supports_auth(&AuthMechanism::Login));
        assert!(!caps.supports_auth(&AuthMechanism::CramMd5));

        let best_oauth = caps.select_best_auth(true, false).unwrap();
        assert_eq!(best_oauth, AuthMechanism::XOAuth2);

        let best_pass = caps.select_best_auth(false, true).unwrap();
        assert_eq!(best_pass, AuthMechanism::Plain);
    }
}
