//! ESMTP Response parsing according to RFC 5321 §4.2.

use crate::error::{SmtpError, SmtpResult};
use crate::protocol::codes;
use crate::protocol::enhanced::EnhancedStatusCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpResponse {
    pub code: u16,
    pub lines: Vec<String>,
    pub enhanced: Option<EnhancedStatusCode>,
}

impl SmtpResponse {
    pub fn new(code: u16, lines: Vec<String>) -> Self {
        let enhanced = lines
            .first()
            .and_then(|l| EnhancedStatusCode::find_in_text(l));
        Self {
            code,
            lines,
            enhanced,
        }
    }

    #[inline]
    pub fn is_positive_completion(&self) -> bool {
        codes::is_positive_completion(self.code)
    }

    #[inline]
    pub fn is_positive_intermediate(&self) -> bool {
        codes::is_positive_intermediate(self.code)
    }

    #[inline]
    pub fn is_transient_negative(&self) -> bool {
        codes::is_transient_negative(self.code)
    }

    #[inline]
    pub fn is_permanent_negative(&self) -> bool {
        codes::is_permanent_negative(self.code)
    }

    /// Returns the combined text of the response lines joined with spaces or newlines.
    pub fn message(&self) -> String {
        self.lines.join(" ")
    }

    /// Converts non-2xx/3xx responses into a SmtpError::Protocol.
    pub fn check_ok(&self) -> SmtpResult<()> {
        if self.is_positive_completion() {
            Ok(())
        } else {
            Err(SmtpError::Protocol {
                code: self.code,
                enhanced: self.enhanced,
                message: self.message(),
            })
        }
    }

    /// Converts non-intermediate responses (e.g. not 354 or 334) into a SmtpError::Protocol.
    pub fn check_intermediate(&self) -> SmtpResult<()> {
        if self.is_positive_intermediate() {
            Ok(())
        } else {
            Err(SmtpError::Protocol {
                code: self.code,
                enhanced: self.enhanced,
                message: self.message(),
            })
        }
    }

    /// Parses an array of raw strings (each line without trailing CRLF) into a structured SmtpResponse.
    pub fn parse_raw_lines<S: AsRef<str>>(raw_lines: &[S]) -> SmtpResult<Self> {
        if raw_lines.is_empty() {
            return Err(SmtpError::Handshake("empty response from server".into()));
        }

        let mut lines = Vec::with_capacity(raw_lines.len());
        let mut expected_code = None;

        for (idx, raw) in raw_lines.iter().enumerate() {
            let s = raw.as_ref();
            if s.len() < 3 {
                return Err(SmtpError::Handshake(format!(
                    "malformed SMTP status line: '{s}'"
                )));
            }

            let code_str = &s[..3];
            let code = code_str
                .parse::<u16>()
                .map_err(|_| SmtpError::Handshake(format!("invalid reply code in line '{s}'")))?;

            if let Some(exp) = expected_code {
                if exp != code {
                    return Err(SmtpError::Handshake(format!(
                        "mismatched reply code across multiline response: expected {exp}, got {code}"
                    )));
                }
            } else {
                expected_code = Some(code);
            }

            let text = if s.len() > 4 {
                s[4..].trim_end().to_string()
            } else {
                String::new()
            };

            lines.push(text);

            let is_last = if s.len() > 3 {
                s.as_bytes()[3] != b'-'
            } else {
                true
            };

            if is_last && idx != raw_lines.len() - 1 {
                // If marked as last line but more lines follow, this batch ended
            }
        }

        let code = expected_code.unwrap_or(500);
        Ok(Self::new(code, lines))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_line_response() {
        let lines = ["250 OK"];
        let resp = SmtpResponse::parse_raw_lines(&lines).unwrap();
        assert_eq!(resp.code, 250);
        assert!(resp.is_positive_completion());
        assert_eq!(resp.lines, vec!["OK"]);
    }

    #[test]
    fn test_multiline_ehlo_response() {
        let lines = [
            "250-smtp.example.com at your service",
            "250-SIZE 35882577",
            "250-8BITMIME",
            "250-STARTTLS",
            "250 OK",
        ];
        let resp = SmtpResponse::parse_raw_lines(&lines).unwrap();
        assert_eq!(resp.code, 250);
        assert_eq!(resp.lines.len(), 5);
        assert_eq!(resp.lines[1], "SIZE 35882577");
    }

    #[test]
    fn test_enhanced_code_extraction() {
        let lines = ["550 5.1.1 <nobody@example.com>: Recipient address rejected: User unknown"];
        let resp = SmtpResponse::parse_raw_lines(&lines).unwrap();
        assert_eq!(resp.code, 550);
        assert!(resp.is_permanent_negative());
        let enh = resp.enhanced.unwrap();
        assert_eq!(enh, EnhancedStatusCode::new(5, 1, 1));
        assert_eq!(enh.description(), "Bad destination mailbox address");
    }
}
