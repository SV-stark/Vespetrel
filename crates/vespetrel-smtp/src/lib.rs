//! # vespetrel-smtp
//!
//! A lightweight, feature-complete SMTP client engine inspired by Geary's simplicity
//! and responsiveness, supporting maximum ESMTP features in pure Rust inspired by Thunderbird.
//!
//! ## Key Capabilities
//! - **Geary-style Asynchronous Outbox**: Thread-safe background queuing, non-blocking UI delivery,
//!   offline queuing, and exponential backoff retry for transient network and 4xx status codes.
//! - **Thunderbird-grade Protocol Compliance**: RFC 5321 (ESMTP), RFC 5322 (Internet Message Format),
//!   RFC 2045-2049 (MIME), RFC 2231 (Parameter Value Extensions), RFC 2047 (Header Encoding).
//! - **Pure Rust Transport**: Asynchronous Tokio network engine with Rustls and WebPKI roots,
//!   supporting Direct TLS (SMTPS, port 465) and Strict/Opportunistic STARTTLS (ports 587/25).
//! - **Thunderbird Rich Headers**: Priorities (1-5 / High-Low), Read Receipts (`Disposition-Notification-To`),
//!   Delivery Receipts (`Return-Receipt-To`), Sensitivity, Organization, User-Agent, and strict BCC privacy.
//! - **ESMTP Extensions**: PIPELINING (RFC 2920), 8BITMIME (RFC 6152), SIZE (RFC 1870), DSN (RFC 3461),
//!   SMTPUTF8 (RFC 6531), CHUNKING (RFC 3030), ENHANCEDSTATUSCODES (RFC 2034, RFC 3463).
//! - **Full SASL Authentication**: PLAIN, LOGIN, XOAUTH2 (Gmail & Microsoft 365), OAUTHBEARER, CRAM-MD5.
//! - **Cryptographic Signatures**: RFC 6376 DKIM signing (RSA-SHA256, relaxed/relaxed canonicalization)
//!   and Autocrypt 1.1 header injection.

pub mod auth;
pub mod client;
pub mod dkim;
pub mod dsn;
pub mod error;
pub mod message;
pub mod outbox;
pub mod protocol;
pub mod transport;

// Re-exports
pub use auth::Credentials;
pub use client::{SmtpClient, SmtpConfig, SmtpSender};
pub use dkim::{
    Canonicalization, DkimAlgorithm, DkimSigner, canonicalize_relaxed_body,
    canonicalize_relaxed_body_bytes, canonicalize_relaxed_header, canonicalize_signed_headers,
    is_valid_dkim_domain, is_valid_dkim_selector,
};
pub use dsn::{DsnNotify, DsnOptions, DsnReturnMode};
pub use error::{SmtpError, SmtpResult};
pub use message::{InlineAttachment, Priority, Sensitivity, SmtpMessage};
pub use outbox::{DeliveryStatus, OutboxEvent, OutboxItem, OutboxQueue, RetryPolicy};
pub use protocol::{
    AuthMechanism, EnhancedStatusCode, ServerCapabilities, SmtpCommand, SmtpResponse, codes,
};
pub use transport::{SmtpConnection, SmtpStream, TlsMode, dot_stuff_rfc5321};

#[cfg(test)]
mod tests {
    use super::*;
    use vespetrel_core::message::{Address, ComposedAttachment, ComposedMessage};

    #[test]
    fn test_smtp_build_message_with_autocrypt() {
        let config = SmtpConfig::new("smtp.example.com", 587, "alice", "token")
            .with_autocrypt("addr=alice@example.com; prefer-encrypt=mutual; keydata=mQEN...");
        let client = SmtpClient::new(config);

        let msg = ComposedMessage {
            from: Address {
                name: Some("Alice".into()),
                email: "alice@example.com".into(),
            },
            to: vec![Address {
                name: Some("Bob".into()),
                email: "bob@example.com".into(),
            }],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Encrypted discussion".into(),
            body_text: "Hello Bob".into(),
            body_html: None,
            in_reply_to: None,
            references: Vec::new(),
            attachments: Vec::new(),
        };

        let formatted = client.build_rfc822(&msg).unwrap();
        let raw_str = String::from_utf8_lossy(&formatted);
        assert!(raw_str.contains("Autocrypt: addr=alice@example.com"));
        assert!(raw_str.contains("Subject: Encrypted discussion"));
    }

    #[test]
    fn test_smtp_build_message_with_dkim() {
        let rsa_pkcs8 = "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCy+OYeW6EsC8TQ4z6RiAIxRM5kEz6gbYkyemoQXiZLDrJf4yVj5EfI4lwEbAAr6D2v+QBJofXQWdigjMooqk7RW5lr0LYsNN7UhfnWsOt74BGzuQ8k6zf2m1EJoD20I04UyWfil+IMvpDVqcd6UbZZCGlLjlPSEs1B6tncbHHSPtHhGJcCpeyO5/KFqcDc9oXwB4skNQ6eRA2KYLY9O4jqThjHueNlA4hic6r9CyF0hpGZtxe90F5hwJ8s3TLBWTn8n5tZD2B8euCTBmgMiH6fnOKOo22VEqzGh9bSioxm+EzFzB0nYoPbAri7dgch7X3r7dNGpkhjwzjhp2R4MYeBAgMBAAECggEAe1E/Fmniivvs+FWsVhCWGiaj45bTDy2KXEq27GJDFnKg+6sCp2qy/7rg1ncoQxi58Jes2A+N1asitbVs0kpPFrh75SshayJe66cI+CJdj7Rb3i9EPRcKL5TjaLON8KJm+bGxMBOhQVDJcT+T4DePYpeGHfaK0PP9lE7jIJtkbg2Nzvbw1XHmnTXdCD36q3gt2uLwnydQXXtoK6WfXmz1hgfiRkdhAo5bOwKpupgGzoBR3aZFtNhQqRVnz/dR4qqQHpvpatmnoxwrbRG0b0YcIheutUictZpbk2JNpEjEfKG8JvkFbsCFsZVEyYE4K2UzLJFof7mb9JCOlOd4B/HZ1QKBgQDe0uDpGC/6jVmHId4xEr3b7+pDxnkZAOy8xXRBl/hXxDAjNU+01EACRFVoWnHWHDeZVfILdh6Az8N+wxl4HY1CtVAhujXU/ANr6cRyPPKMm/P0YuJ85RsxmnOuGsYeZmiEwspJVASJ472KW0mOJnMieqGcGarn3ajAacGqu2gWpwKBgQDNnpUyuVKHssOrBmWBwZ9F6uhK2iSoxtLAQfUBK9KWNN2hURMXZW1QoKT3Kp/0CDEgIUz9v9zfylER2NsO7C1p2azj+W+jNKkOsITgIeAcrcp5wYS2K2q4x6U+JkARkKna0V4PwsHBgyZrIIycd6y6zF5hBMan+Hxy8DfggZLdlwKBgQCOtrP0t1grepLn2QpNlfpiPpRlml3/ZLc75J+kT2hxFifatQ96+yKQESI+twcIIoR9wi1Hp/y7ddZ5fw31/791BVnwcCqAYnTyjgQTQvP6mPwz/42efsLfD1SeI2nXGLJCrdwQAS7y/hls3zEKSZgecjrGFy5+WVr2+gVfi66MKwKBgAcH8jAe2Cydt0Uk3dm3Bjw80R6mIPTIf7JlTvxwRC4wtpdqj02QgVFtfNaa1YdhtFRV7y0KH4Jjh6wljzAOcWsaL2hIQkIBbfp7nL+RSPmSE8dgD6qvB2I0KXlbk3tGSBicaiv9y+RTGMA3B7fd+8ETdfK5WBWsUI0Zm7+Ijr4XAoGBANLc+H3c9h1faSb2YSN+QHtcyWJxhZONrE7pbBTMuVbgewtqjR2Et4AvmIuhwNfmZI6gnrbd1TWvGeGhZbdFQHpgms/92HjfAl61rttB+LgaKWoMod4XWqMxtChLNBpLqXEcY1xND9gAM66IGSPw7XKFiygzgDBucFcdJdwMo/Jk";
        let config = SmtpConfig::new("smtp.example.com", 465, "alice", "token").with_dkim(
            "example.com",
            "default",
            rsa_pkcs8,
        );
        let client = SmtpClient::new(config);

        let msg = ComposedMessage {
            from: Address {
                name: Some("Alice".into()),
                email: "alice@example.com".into(),
            },
            to: vec![Address {
                name: Some("Bob".into()),
                email: "bob@example.com".into(),
            }],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "DKIM Signed Message".into(),
            body_text: "Hello Bob, this email is signed with DKIM.".into(),
            body_html: None,
            in_reply_to: None,
            references: Vec::new(),
            attachments: Vec::new(),
        };

        let formatted = client.build_rfc822(&msg).unwrap();
        let raw_str = String::from_utf8_lossy(&formatted);
        assert!(raw_str.contains("DKIM-Signature:"));
        assert!(raw_str.contains("d=example.com"));
        assert!(raw_str.contains("s=default"));
    }

    #[test]
    fn dkim_signs_the_serialized_headers_not_the_model() {
        let raw = b"From: =?utf-8?B?w4FsaWNl?= <alice@example.com>\r\n\
To: Bob <bob@example.com>\r\n\
Subject: =?utf-8?Q?Caf=C3=A9_meeting?=\r\n\
Date: Mon, 01 Jan 2024 00:00:00 +0000\r\n\
\r\n\
body\r\n";
        let canon =
            canonicalize_signed_headers(raw, &["from", "to", "subject", "date"]).expect("headers");
        let canon = String::from_utf8(canon).unwrap();
        assert_eq!(
            canon,
            "from:=?utf-8?B?w4FsaWNl?= <alice@example.com>\r\n\
             to:Bob <bob@example.com>\r\n\
             subject:=?utf-8?Q?Caf=C3=A9_meeting?=\r\n\
             date:Mon, 01 Jan 2024 00:00:00 +0000\r\n"
                .replace("             ", "")
        );
    }

    #[test]
    fn dkim_canonicalization_fails_closed_on_missing_header() {
        let raw = b"From: a@b.com\r\nTo: c@d.com\r\n\r\nbody\r\n";
        assert!(canonicalize_signed_headers(raw, &["from", "to", "subject"]).is_none());
    }

    #[test]
    fn dkim_uses_the_bottom_most_instance_of_a_duplicated_header() {
        let raw = b"Subject: attacker\r\n\
To: c@d.com\r\n\
Subject: real\r\n\
\r\nbody\r\n";
        let canon = canonicalize_signed_headers(raw, &["subject"]).expect("headers");
        assert_eq!(String::from_utf8(canon).unwrap(), "subject:real\r\n");
    }

    #[test]
    fn dkim_unfolds_continuation_lines() {
        let raw = b"To: Bob <bob@example.com>,\r\n  Carol <carol@example.com>\r\n\r\nbody\r\n";
        let canon = canonicalize_signed_headers(raw, &["to"]).expect("headers");
        assert_eq!(
            String::from_utf8(canon).unwrap(),
            "to:Bob <bob@example.com>, Carol <carol@example.com>\r\n"
        );
    }

    #[test]
    fn dkim_selector_and_domain_syntax() {
        assert!(is_valid_dkim_selector("default"));
        assert!(is_valid_dkim_selector("s1"));
        assert!(is_valid_dkim_selector("mail.sel1.sel2"));
        assert!(!is_valid_dkim_selector(""));
        assert!(!is_valid_dkim_selector("has space"));
        assert!(!is_valid_dkim_selector("crlf\r\nInjected: x"));
        assert!(!is_valid_dkim_selector("selé"));
        assert!(!is_valid_dkim_selector(".leading"));
        assert!(!is_valid_dkim_selector("trailing."));
        assert!(!is_valid_dkim_selector("-lead"));

        assert!(is_valid_dkim_domain("example.com"));
        assert!(is_valid_dkim_domain("mx1.eu-west-1.amazonses.com"));
        assert!(!is_valid_dkim_domain(""));
        assert!(!is_valid_dkim_domain("example.com."));
        assert!(!is_valid_dkim_domain("bad domain.com"));
        assert!(!is_valid_dkim_domain("evil\r\nX-Injected: 1"));
        assert!(!is_valid_dkim_domain("exämple.com"));
    }

    #[test]
    fn test_smtp_build_message_with_cc_bcc_and_html() {
        let config = SmtpConfig::new("smtp.example.com", 465, "alice", "token");
        let client = SmtpClient::new(config);

        let msg = ComposedMessage {
            from: Address {
                name: Some("Alice".into()),
                email: "alice@example.com".into(),
            },
            to: vec![Address {
                name: Some("Bob".into()),
                email: "bob@example.com".into(),
            }],
            cc: vec![Address {
                name: Some("Carol".into()),
                email: "carol@example.com".into(),
            }],
            bcc: vec![Address {
                name: None,
                email: "secret@example.com".into(),
            }],
            subject: "Sprint Update".into(),
            body_text: "Update".into(),
            body_html: Some("<h1>Sprint Update</h1>".into()),
            in_reply_to: Some("<prev-msg@example.com>".into()),
            references: vec!["<prev-msg@example.com>".into()],
            attachments: Vec::new(),
        };

        let formatted = client.build_rfc822(&msg).unwrap();
        let raw_str = String::from_utf8_lossy(&formatted);
        assert!(raw_str.contains("Cc: Carol <carol@example.com>"));
        assert!(raw_str.contains("In-Reply-To: <prev-msg@example.com>"));
        assert!(raw_str.contains("<h1>Sprint Update</h1>"));
    }

    #[test]
    fn test_smtp_build_message_with_attachments() {
        let config = SmtpConfig::new("smtp.example.com", 587, "alice", "token");
        let client = SmtpClient::new(config);

        let msg = ComposedMessage {
            from: Address {
                name: Some("Alice".into()),
                email: "alice@example.com".into(),
            },
            to: vec![Address {
                name: Some("Bob".into()),
                email: "bob@example.com".into(),
            }],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Quarterly Report".into(),
            body_text: "Please find report attached.".into(),
            body_html: None,
            in_reply_to: None,
            references: Vec::new(),
            attachments: vec![ComposedAttachment {
                filename: "report.pdf".into(),
                content_type: "application/pdf".into(),
                data: b"%PDF-1.4 dummy pdf content".to_vec(),
            }],
        };

        let formatted = client.build_rfc822(&msg).unwrap();
        let raw_str = String::from_utf8_lossy(&formatted);
        assert!(raw_str.contains("report.pdf"));
        assert!(raw_str.contains("application/pdf"));
        assert!(raw_str.contains("multipart/mixed"));
        assert!(raw_str.contains("Message-ID:"));
    }

    #[tokio::test]
    async fn test_geary_style_outbox_queue_and_events() {
        let queue = OutboxQueue::new();
        let mut rx = queue.subscribe();

        let msg = SmtpMessage::new(
            Address {
                name: Some("Alice".into()),
                email: "alice@example.com".into(),
            },
            "Test Async Delivery",
        )
        .to(Address {
            name: Some("Bob".into()),
            email: "bob@example.com".into(),
        })
        .priority(Priority::High)
        .request_read_receipt("alice@example.com");

        let id = queue.enqueue(msg).await;

        let event = rx.recv().await.unwrap();
        assert!(matches!(event, OutboxEvent::Enqueued { id: ref eid, .. } if eid == &id));

        let item = queue.get(&id).await.unwrap();
        assert_eq!(item.status, DeliveryStatus::Queued);
        assert_eq!(item.message.priority, Some(Priority::High));

        // Mark sent
        queue.mark_sent(&id, "250 OK Message accepted".into()).await;
        let event2 = rx.recv().await.unwrap();
        assert!(matches!(event2, OutboxEvent::Sent { id: ref eid, .. } if eid == &id));

        let completed_item = queue.get(&id).await.unwrap();
        assert!(completed_item.status.is_terminal());
    }

    #[test]
    fn test_thunderbird_pure_rust_message_features() {
        let msg = SmtpMessage::new(
            Address {
                name: Some("Suyash Stark".into()),
                email: "suyash@vespetrel.org".into(),
            },
            "Release Announcement",
        )
        .to(Address {
            name: Some("Community".into()),
            email: "community@vespetrel.org".into(),
        })
        .bcc(Address {
            name: None,
            email: "archive@vespetrel.org".into(),
        })
        .priority(Priority::Highest)
        .request_read_receipt("suyash@vespetrel.org")
        .request_delivery_receipt("suyash@vespetrel.org")
        .organization("Vespetrel Project")
        .autocrypt("addr=suyash@vespetrel.org; keydata=mQEN...")
        .body_text("Vespetrel 0.1 is here!")
        .body_html("<h1>Vespetrel 0.1 is here!</h1><img src=\"cid:badge\">")
        .attach_inline("badge", "badge.png", "image/png", b"fake-png".to_vec())
        .attach(
            "release_notes.txt",
            "text/plain",
            b"Full release notes".to_vec(),
        );

        let recipients = msg.envelope_recipients();
        assert_eq!(recipients.len(), 2);
        assert!(recipients.contains(&"community@vespetrel.org".to_string()));
        assert!(recipients.contains(&"archive@vespetrel.org".to_string()));

        let wire = msg.build_rfc5322_bytes("vespetrel.org").unwrap();
        let s = String::from_utf8(wire).unwrap();

        // Check privacy: BCC recipient MUST NOT be in header!
        assert!(!s.contains("archive@vespetrel.org"));
        assert!(!s.contains("Bcc:"));

        // Check Thunderbird headers
        assert!(s.contains("X-Priority: 1 (Highest)"));
        assert!(s.contains("Importance: high"));
        assert!(s.contains("Disposition-Notification-To: <suyash@vespetrel.org>"));
        assert!(s.contains("Return-Receipt-To: <suyash@vespetrel.org>"));
        assert!(s.contains("Organization: Vespetrel Project"));
        assert!(s.contains("Autocrypt: addr=suyash@vespetrel.org"));
        assert!(s.contains("User-Agent: Vespetrel/0.1.0"));

        // Check nested MIME structure
        assert!(s.contains("multipart/mixed"));
        assert!(s.contains("multipart/related"));
        assert!(s.contains("multipart/alternative"));
        assert!(s.contains("Content-ID: <badge>"));
        assert!(s.contains("filename=\"release_notes.txt\""));
    }
}
