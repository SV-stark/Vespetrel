//! RFC 3798 Message Disposition Notifications (MDN / Read Receipts) §3 Phase 5
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MdnRequest {
    /// Recipient email requested to receive the read receipt
    pub notification_to: String,
    pub original_message_id: Option<String>,
    pub original_subject: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispositionType {
    /// The user viewed or read the message manually
    Displayed,
    /// Message deleted without display
    Deleted,
    /// Message dispatched/forwarded
    Dispatched,
}

impl DispositionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Displayed => "displayed",
            Self::Deleted => "deleted",
            Self::Dispatched => "dispatched",
        }
    }
}

pub struct MdnEngine;

impl MdnEngine {
    /// Parse `Disposition-Notification-To` or `Return-Receipt-To` headers from an email
    pub fn parse_receipt_request(
        disposition_to: Option<&str>,
        return_receipt_to: Option<&str>,
        message_id: Option<&str>,
        subject: Option<&str>,
    ) -> Option<MdnRequest> {
        let recipient = disposition_to.or(return_receipt_to)?.trim();
        if recipient.is_empty() || !recipient.contains('@') {
            return None;
        }

        Some(MdnRequest {
            notification_to: recipient.to_string(),
            original_message_id: message_id.map(|s| s.trim().to_string()),
            original_subject: subject.map(|s| s.trim().to_string()),
        })
    }

    fn sanitize_crlf(val: &str) -> String {
        val.replace(['\r', '\n'], " ").trim().to_string()
    }

    /// Build an RFC 3798 MDN `multipart/report; report-type=disposition-notification` response body.
    /// Requires explicit user consent to prevent tracking beacon leaks.
    pub fn build_mdn_report(
        req: &MdnRequest,
        reader_email: &str,
        disp: DispositionType,
        user_consented: bool,
    ) -> Result<String, &'static str> {
        if !user_consented {
            return Err("User consent is required to dispatch an MDN read receipt");
        }

        let raw_msg_id = req.original_message_id.as_deref().unwrap_or("unknown");
        let raw_subject = req.original_subject.as_deref().unwrap_or("(no subject)");

        let msg_id = Self::sanitize_crlf(raw_msg_id);
        let subject = Self::sanitize_crlf(raw_subject);
        let safe_reader = Self::sanitize_crlf(reader_email);

        Ok(format!(
            "This is a Return Receipt for the mail that you sent to {safe_reader}.\r\n\r\n\
            Note: This Return Receipt only acknowledges that the message was displayed on the recipient's computer. There is no guarantee that the recipient has read or understood the message contents.\r\n\r\n\
            Reporting-UA: Vespetrel/0.1.0\r\n\
            Original-Recipient: rfc822;{safe_reader}\r\n\
            Final-Recipient: rfc822;{safe_reader}\r\n\
            Original-Message-ID: {msg_id}\r\n\
            Subject: {subject}\r\n\
            Disposition: manual-action/MDN-sent-manually; {}\r\n",
            disp.as_str()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mdn_parse_and_generate() {
        let req = MdnEngine::parse_receipt_request(
            Some("boss@company.com"),
            None,
            Some("<msg-42@company.com>\r\nInjected: Header"),
            Some("Urgent Q4 Plan\r\nBcc: evil@phishing.com"),
        )
        .unwrap();

        assert_eq!(req.notification_to, "boss@company.com");

        // Without consent, must fail
        assert!(
            MdnEngine::build_mdn_report(
                &req,
                "employee@company.com",
                DispositionType::Displayed,
                false
            )
            .is_err()
        );

        // With consent, succeeds and CRLF is stripped
        let report = MdnEngine::build_mdn_report(
            &req,
            "employee@company.com",
            DispositionType::Displayed,
            true,
        )
        .unwrap();
        assert!(report.contains("Original-Recipient: rfc822;employee@company.com"));
        assert!(report.contains("Original-Message-ID: <msg-42@company.com>  Injected: Header"));
        assert!(report.contains("Subject: Urgent Q4 Plan  Bcc: evil@phishing.com"));
        assert!(!report.contains("\r\nInjected: Header"));
        assert!(report.contains("Disposition: manual-action/MDN-sent-manually; displayed"));
    }
}
