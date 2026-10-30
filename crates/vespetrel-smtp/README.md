# 📤 vespetrel-smtp

[![Crates.io](https://img.shields.io/crates/v/vespetrel-smtp.svg)](https://crates.io/crates/vespetrel-smtp)
[![Documentation](https://docs.rs/vespetrel-smtp/badge.svg)](https://docs.rs/vespetrel-smtp)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)

A lightweight, feature-complete SMTP client engine inspired by **Geary's** responsiveness and simplicity, supporting maximum ESMTP features in pure Rust inspired by **Thunderbird**.

---

## 📦 Overview

`vespetrel-smtp` delivers ultra-fast, robust outgoing email transmission with:
- **Geary-Style Asynchronous Outbox:** Non-blocking background mail dispatch, offline message queue, automatic retry with exponential backoff for 4xx temporary failures, and live event streaming.
- **Thunderbird Maximum Feature Set:** Rich RFC 5322 headers, message priorities (Highest to Lowest), Read Receipts (`Disposition-Notification-To`), Delivery Receipts (`Return-Receipt-To`), Sensitivity, Organization, and strict BCC recipient privacy.
- **Pure Rust Transport:** Native asynchronous Tokio network stack with Rustls and WebPKI roots. Supports Direct TLS (SMTPS on port 465) and Strict/Opportunistic STARTTLS (ports 587/25) with protection against MITM downgrade attacks.
- **ESMTP Extensions:** Full capability negotiation for `PIPELINING` (RFC 2920), `8BITMIME` (RFC 6152), `SIZE` (RFC 1870), `DSN` (RFC 3461), `SMTPUTF8` (RFC 6531), `CHUNKING` (RFC 3030), and `ENHANCEDSTATUSCODES` (RFC 2034, RFC 3463).
- **Comprehensive SASL Authentication:** `PLAIN`, `LOGIN`, `XOAUTH2` (Google Gmail & Microsoft 365), `OAUTHBEARER` (RFC 7628), and `CRAM-MD5` (RFC 2195).
- **Cryptographic Signatures:** RFC 6376 DKIM signing (RSA-SHA256, relaxed/relaxed canonicalization) and Autocrypt 1.1 decentralized encryption header injection.
- **Advanced MIME Hierarchy:** Singlepart text/html, `multipart/alternative`, `multipart/related` for inline images (`cid:...`), and `multipart/mixed` for file attachments with RFC 2047 / RFC 2231 encoding.

---

## 🚀 Key Capabilities

### 1. Geary Asynchronous Outbox & Offline Queue
When sending email, the user interface should never freeze or wait on remote network latency. `vespetrel-smtp` provides a thread-safe asynchronous outbox:
- **Instant Non-Blocking Submission:** Enqueues messages immediately and returns a tracking UUID.
- **Intelligent Retry Engine:** Distinguishes temporary transient failures (network timeouts, DNS glitches, 4xx reply codes) from permanent errors (5xx codes, authentication rejections).
- **Exponential Backoff:** Automatically reschedules delivery attempts with configurable backoff policies.
- **Live Event Stream:** Broadcasts real-time events (`Enqueued`, `Sending`, `Sent`, `TransientFailure`, `PermanentFailure`, `Cancelled`) for UI progress indicators and notifications.

### 2. Thunderbird-Grade ESMTP Protocol Compliance
- **Strict TLS Mode:** Rejects unencrypted fallback if STARTTLS is not offered, preventing MITM stripping.
- **High-Throughput PIPELINING:** Batches `MAIL FROM`, `RCPT TO`, and `DATA` into a single network round-trip when supported by the server, drastically reducing latency.
- **BCC Recipient Privacy:** Bcc recipients are delivered in the SMTP envelope (`RCPT TO`) but completely removed from the serialized RFC 5322 header block to ensure recipient confidentiality.
- **Read & Delivery Receipts:** Configurable headers for `Disposition-Notification-To` and `Return-Receipt-To`.

---

## 💻 Example Usage

### Lightweight One-Line Sending (Geary Style)

```rust
use vespetrel_smtp::SmtpSender;
use vespetrel_core::message::{Address, ComposedMessage};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let sender = SmtpSender::new("smtp.gmail.com", 587)
        .with_auth("user@gmail.com", "oauth_access_token")
        .with_xoauth2();

    let msg = ComposedMessage {
        from: Address { name: Some("Alice".into()), email: "user@gmail.com".into() },
        to: vec![Address { name: Some("Bob".into()), email: "bob@example.com".into() }],
        cc: vec![],
        bcc: vec![],
        subject: "Hello from Vespetrel".into(),
        body_text: "Sent with lightweight pure-Rust SMTP.".into(),
        body_html: None,
        in_reply_to: None,
        references: vec![],
        attachments: vec![],
    };

    sender.send(&msg).await?;
    println!("Delivered successfully!");
    Ok(())
}
```

### Thunderbird-Style Rich Message with Outbox Queue

```rust
use vespetrel_smtp::{
    Address, DeliveryStatus, OutboxQueue, Priority, Sensitivity, SmtpClient, SmtpConfig, SmtpMessage,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = SmtpConfig::new("smtp.example.com", 587, "alice", "secret_pass")
        .with_hostname("vespetrel.org");

    let client = SmtpClient::new(config);

    // Compose rich message
    let msg = SmtpMessage::new(
        Address { name: Some("Alice Smith".into()), email: "alice@example.com".into() },
        "Quarterly Executive Update",
    )
    .to(Address { name: Some("Bob".into()), email: "bob@example.com".into() })
    .bcc(Address { name: None, email: "audit@example.com".into() }) // Stripped from headers, sent in envelope
    .priority(Priority::Highest)
    .request_read_receipt("alice@example.com")
    .organization("Acme Corp")
    .body_text("Please see the attached financial summary.")
    .body_html("<h1>Financial Summary</h1><p>Please review attached PDF.</p>")
    .attach("summary.pdf", "application/pdf", b"%PDF-1.4 dummy data".to_vec());

    // Asynchronously enqueue into Geary-style outbox
    let task_id = client.send_via_outbox(msg).await;
    println!("Message queued with outbox task ID: {task_id}");

    Ok(())
}
```

---

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
