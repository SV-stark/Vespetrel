//! RFC 8601 Authentication-Results & DKIM/SPF/DMARC Security Badges §3 Phase 5
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthStatus {
    Pass,
    Fail,
    SoftFail,
    Neutral,
    None,
}

impl AuthStatus {
    pub fn is_trusted(&self) -> bool {
        matches!(self, Self::Pass)
    }

    pub fn badge_color_hex(&self) -> &'static str {
        match self {
            Self::Pass => "#10b981",     // Green
            Self::Fail => "#ef4444",     // Red
            Self::SoftFail => "#f59e0b", // Amber
            Self::Neutral => "#6b7280",  // Gray
            Self::None => "#9ca3af",     // Light Gray
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmailSecuritySummary {
    pub dkim: AuthStatus,
    pub dkim_domain: Option<String>,
    pub spf: AuthStatus,
    pub dmarc: AuthStatus,
    pub is_verified_sender: bool,
}

pub struct AuthBadgeParser;

impl AuthBadgeParser {
    /// Parse RFC 8601 `Authentication-Results` header
    /// e.g. "mx.google.com; dkim=pass header.i=@github.com; spf=pass (google.com: domain of support@github.com designates 192.30.252.204 as permitted sender); dmarc=pass"
    pub fn parse_authentication_results(header: &str) -> EmailSecuritySummary {
        let lower = header.to_lowercase();

        let dkim = if lower.contains("dkim=pass") {
            AuthStatus::Pass
        } else if lower.contains("dkim=fail") {
            AuthStatus::Fail
        } else if lower.contains("dkim=neutral") {
            AuthStatus::Neutral
        } else {
            AuthStatus::None
        };

        let spf = if lower.contains("spf=pass") {
            AuthStatus::Pass
        } else if lower.contains("spf=fail") {
            AuthStatus::Fail
        } else if lower.contains("spf=softfail") {
            AuthStatus::SoftFail
        } else if lower.contains("spf=neutral") {
            AuthStatus::Neutral
        } else {
            AuthStatus::None
        };

        let dmarc = if lower.contains("dmarc=pass") {
            AuthStatus::Pass
        } else if lower.contains("dmarc=fail") {
            AuthStatus::Fail
        } else {
            AuthStatus::None
        };

        // Extract DKIM signing domain if present (header.i=@domain.com or header.d=domain.com)
        let dkim_domain = lower
            .split(';')
            .find(|part| part.contains("header.i=") || part.contains("header.d="))
            .and_then(|part| {
                part.split_whitespace().find_map(|w| {
                    w.strip_prefix("header.i=@")
                        .or_else(|| w.strip_prefix("header.i="))
                        .or_else(|| w.strip_prefix("header.d="))
                        .map(|val| val.trim_matches(';').to_string())
                })
            });

        let is_verified_sender =
            dkim == AuthStatus::Pass && (spf == AuthStatus::Pass || dmarc == AuthStatus::Pass);

        EmailSecuritySummary {
            dkim,
            dkim_domain,
            spf,
            dmarc,
            is_verified_sender,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_auth_results() {
        let auth_hdr = "mx.google.com; dkim=pass header.i=@github.com; spf=pass (google.com: domain of support@github.com designates 192.30.252.204); dmarc=pass";
        let summary = AuthBadgeParser::parse_authentication_results(auth_hdr);

        assert_eq!(summary.dkim, AuthStatus::Pass);
        assert_eq!(summary.spf, AuthStatus::Pass);
        assert_eq!(summary.dmarc, AuthStatus::Pass);
        assert_eq!(summary.dkim_domain.as_deref(), Some("github.com"));
        assert!(summary.is_verified_sender);
        assert_eq!(summary.dkim.badge_color_hex(), "#10b981");

        let spoofed =
            "mx.google.com; dkim=fail; spf=fail (domain does not designate IP); dmarc=fail";
        let bad_summary = AuthBadgeParser::parse_authentication_results(spoofed);
        assert_eq!(bad_summary.dkim, AuthStatus::Fail);
        assert_eq!(bad_summary.spf, AuthStatus::Fail);
        assert!(!bad_summary.is_verified_sender);
        assert_eq!(bad_summary.dkim.badge_color_hex(), "#ef4444");
    }
}
