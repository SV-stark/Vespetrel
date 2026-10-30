//! RFC 3463 and RFC 2034 Enhanced Mail System Status Codes.

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnhancedStatusCode {
    pub class: u8,
    pub subject: u16,
    pub detail: u16,
}

impl EnhancedStatusCode {
    pub const fn new(class: u8, subject: u16, detail: u16) -> Self {
        Self {
            class,
            subject,
            detail,
        }
    }

    #[inline]
    pub fn is_success(&self) -> bool {
        self.class == 2
    }

    #[inline]
    pub fn is_transient(&self) -> bool {
        self.class == 4
    }

    #[inline]
    pub fn is_permanent(&self) -> bool {
        self.class == 5
    }

    /// Provides a human-readable diagnosis of the enhanced status code according to RFC 3463.
    pub fn description(&self) -> &'static str {
        match (self.subject, self.detail) {
            (0, 0) => "Other undefined status",
            (1, 0) => "Other address status",
            (1, 1) => "Bad destination mailbox address",
            (1, 2) => "Bad destination system address",
            (1, 3) => "Bad destination mailbox address syntax",
            (1, 4) => "Destination mailbox address ambiguous",
            (1, 5) => "Destination address valid",
            (1, 6) => "Destination mailbox has moved, No forwarding address",
            (1, 7) => "Bad sender's mailbox address syntax",
            (1, 8) => "Bad sender's system address",

            (2, 0) => "Other or undefined mailbox status",
            (2, 1) => "Mailbox disabled, not accepting messages",
            (2, 2) => "Mailbox full",
            (2, 3) => "Message length exceeds administrative limit",
            (2, 4) => "Mailing list expansion problem",

            (3, 0) => "Other or undefined mail system status",
            (3, 1) => "Mail system full",
            (3, 2) => "System not accepting network messages",
            (3, 3) => "System not capable of selected features",
            (3, 4) => "Message too big for system",

            (4, 0) => "Other or undefined network or routing status",
            (4, 1) => "No answer from host",
            (4, 2) => "Bad connection",
            (4, 3) => "Directory server failure",
            (4, 4) => "Unable to route",
            (4, 5) => "Mail system congestion",
            (4, 6) => "Routing loop detected",
            (4, 7) => "Delivery time expired",

            (5, 0) => "Other or undefined protocol status",
            (5, 1) => "Invalid command",
            (5, 2) => "Syntax error",
            (5, 3) => "Too many recipients",
            (5, 4) => "Invalid command arguments",
            (5, 5) => "Wrong protocol version",

            (6, 0) => "Other or undefined media error",
            (6, 1) => "Media not supported",
            (6, 2) => "Conversion required and prohibited",
            (6, 3) => "Conversion required but not supported",

            (7, 0) => "Other or undefined security status",
            (7, 1) => "Delivery not authorized, message refused",
            (7, 2) => "Mailing list expansion prohibited",
            (7, 3) => "Security conversion required but not possible",
            (7, 4) => "Security features not supported",
            (7, 5) => "Cryptographic failure",
            (7, 6) => "Cryptographic algorithm not supported",
            (7, 7) => "Message integrity failure",
            (7, 8) => "Authentication credentials invalid",
            (7, 9) => "Authentication mechanism too weak",
            (7, 10) => "Encryption needed",

            _ => match self.class {
                2 => "Success",
                4 => "Persistent transient failure",
                5 => "Permanent failure",
                _ => "Unknown status",
            },
        }
    }

    /// Extract an EnhancedStatusCode from an SMTP response text if present (e.g. `2.1.5` or `5.7.8`).
    pub fn find_in_text(text: &str) -> Option<Self> {
        for word in text.split_whitespace() {
            for part in word.split('-') {
                let candidate = part.trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
                if let Ok(code) = candidate.parse::<EnhancedStatusCode>() {
                    return Some(code);
                }
            }
        }
        None
    }
}

impl fmt::Display for EnhancedStatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.class, self.subject, self.detail)
    }
}

impl FromStr for EnhancedStatusCode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(());
        }

        let class = parts[0].parse::<u8>().map_err(|_| ())?;
        if class != 2 && class != 4 && class != 5 {
            return Err(());
        }

        let subject = parts[1].parse::<u16>().map_err(|_| ())?;
        let detail = parts[2].parse::<u16>().map_err(|_| ())?;

        Ok(Self {
            class,
            subject,
            detail,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_enhanced_code() {
        let code = "5.7.8".parse::<EnhancedStatusCode>().unwrap();
        assert_eq!(code.class, 5);
        assert_eq!(code.subject, 7);
        assert_eq!(code.detail, 8);
        assert!(code.is_permanent());
        assert!(!code.is_transient());
        assert_eq!(code.description(), "Authentication credentials invalid");

        let transient = "4.4.1".parse::<EnhancedStatusCode>().unwrap();
        assert!(transient.is_transient());
        assert_eq!(transient.description(), "No answer from host");
    }

    #[test]
    fn test_find_in_text() {
        let text = "535 5.7.8 Username and Password not accepted.";
        let code = EnhancedStatusCode::find_in_text(text).unwrap();
        assert_eq!(code, EnhancedStatusCode::new(5, 7, 8));

        let text2 = "250-2.1.5 OK message accepted for delivery";
        let code2 = EnhancedStatusCode::find_in_text(text2).unwrap();
        assert_eq!(code2, EnhancedStatusCode::new(2, 1, 5));
        assert!(code2.is_success());
    }
}
