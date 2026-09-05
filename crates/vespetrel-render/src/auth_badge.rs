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
    /// Parse RFC 8601 `Authentication-Results` header with domain alignment and authserv-id verification
    pub fn parse_authentication_results_aligned(
        header: &str,
        from_email: Option<&str>,
        expected_authserv_id: Option<&str>,
    ) -> EmailSecuritySummary {
        let lower = header.to_lowercase();
        let segments: Vec<&str> = lower.split(';').map(|s| s.trim()).collect();

        if segments.is_empty() {
            return EmailSecuritySummary {
                dkim: AuthStatus::None,
                dkim_domain: None,
                spf: AuthStatus::None,
                dmarc: AuthStatus::None,
                is_verified_sender: false,
            };
        }

        // Validate authserv-id if expected
        let header_authserv_id = segments[0].split_whitespace().next().unwrap_or_default();
        if let Some(expected_id) = expected_authserv_id {
            let clean_expected = expected_id.to_lowercase();
            if !header_authserv_id.eq_ignore_ascii_case(&clean_expected)
                && !header_authserv_id.ends_with(&clean_expected)
            {
                return EmailSecuritySummary {
                    dkim: AuthStatus::None,
                    dkim_domain: None,
                    spf: AuthStatus::None,
                    dmarc: AuthStatus::None,
                    is_verified_sender: false,
                };
            }
        }

        let mut dkim = AuthStatus::None;
        let mut spf = AuthStatus::None;
        let mut dmarc = AuthStatus::None;

        for seg in &segments {
            // Match DKIM
            if seg.starts_with("dkim=") || seg.contains(" dkim=") {
                if seg.contains("dkim=pass") {
                    dkim = AuthStatus::Pass;
                } else if seg.contains("dkim=fail") {
                    dkim = AuthStatus::Fail;
                } else if seg.contains("dkim=neutral") {
                    dkim = AuthStatus::Neutral;
                }
            }
            // Match SPF
            if seg.starts_with("spf=") || seg.contains(" spf=") {
                if seg.contains("spf=pass") {
                    spf = AuthStatus::Pass;
                } else if seg.contains("spf=softfail") {
                    spf = AuthStatus::SoftFail;
                } else if seg.contains("spf=fail") {
                    spf = AuthStatus::Fail;
                } else if seg.contains("spf=neutral") {
                    spf = AuthStatus::Neutral;
                }
            }
            // Match DMARC
            if seg.starts_with("dmarc=") || seg.contains(" dmarc=") {
                if seg.contains("dmarc=pass") {
                    dmarc = AuthStatus::Pass;
                } else if seg.contains("dmarc=fail") {
                    dmarc = AuthStatus::Fail;
                }
            }
        }

        // Extract DKIM signing domain if present (header.i=@domain.com or header.d=domain.com)
        let dkim_domain = segments
            .iter()
            .find(|part| part.contains("header.i=") || part.contains("header.d="))
            .and_then(|part| {
                part.split_whitespace().find_map(|w| {
                    w.strip_prefix("header.i=@")
                        .or_else(|| w.strip_prefix("header.i="))
                        .or_else(|| w.strip_prefix("header.d="))
                        .map(|val| val.trim_matches(';').to_string())
                })
            });

        // Verify domain alignment against From address
        let domain_aligned = if let (Some(from), Some(dkim_d)) = (from_email, &dkim_domain) {
            let from_domain = from
                .split('@')
                .nth(1)
                .unwrap_or("")
                .trim_matches('>')
                .trim();
            !from_domain.is_empty()
                && (from_domain.eq_ignore_ascii_case(dkim_d)
                    || from_domain.ends_with(&format!(".{dkim_d}"))
                    || dkim_d.ends_with(&format!(".{from_domain}")))
        } else {
            dkim_domain.is_some()
        };

        let is_verified_sender = dkim == AuthStatus::Pass
            && domain_aligned
            && (spf == AuthStatus::Pass || dmarc == AuthStatus::Pass);

        EmailSecuritySummary {
            dkim,
            dkim_domain,
            spf,
            dmarc,
            is_verified_sender,
        }
    }

    /// Parse RFC 8601 `Authentication-Results` header
    /// e.g. "mx.google.com; dkim=pass header.i=@github.com; spf=pass (google.com: domain of support@github.com designates 192.30.252.204 as permitted sender); dmarc=pass"
    pub fn parse_authentication_results(header: &str) -> EmailSecuritySummary {
        Self::parse_authentication_results_aligned(header, None, None)
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
