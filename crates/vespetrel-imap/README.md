# 📬 vespetrel-imap

[![Crates.io](https://img.shields.io/crates/v/vespetrel-imap.svg)](https://crates.io/crates/vespetrel-imap)
[![Documentation](https://docs.rs/vespetrel-imap/badge.svg)](https://docs.rs/vespetrel-imap)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)

High-performance, RFC-compliant async IMAP client implementation for **Vespetrel**, with support for IDLE, CONDSTORE, QRESYNC, and XOAUTH2.

---

## 📦 Overview

`vespetrel-imap` delivers an efficient, asynchronous IMAP client built directly on Tokio and Rustls:
- **Real-Time Push:** RFC 2177 `IDLE` for instantaneous incoming message detection without polling.
- **Delta Synchronization:** RFC 7162 `CONDSTORE` and `QRESYNC` for ultra-fast incremental synchronization of changed flags and new messages.
- **Authentication:** SASL PLAIN, LOGIN, and XOAUTH2 (OAuth2 bearer tokens for Gmail, Outlook, Yahoo).
- **Transport Security:** Strict TLS with AWS-LC-RS cryptographic backend.

## 🚀 Key Capabilities

- **Streaming Body Parsing:** Incremental FETCH parsing avoiding unnecessary memory allocation.
- **UID Validity Handling:** Automatic mailbox desynchronization detection and recovery.
- **Auto-Reconnect & Keepalive:** Resilient connection state management over unstable networks.

## 💻 Example Usage

```rust
use vespetrel_imap::{ImapConfig, ImapConnection};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // OAuth2 (Gmail/Outlook) — XOAUTH2 with the access token as the secret:
    let config = ImapConfig::new("imap.gmail.com", 993, "user@gmail.com", access_token)
        .with_xoauth2();
    let mut conn = ImapConnection::new(config);
    conn.connect().await?;

    // Or a plain password / app password:
    let config = ImapConfig::new("imap.example.com", 993, "user@example.com", "password");
    let mut conn = ImapConnection::new(config);
    conn.connect().await?;

    Ok(())
}
```

`ImapProvider` is the type the engine consumes, implementing `MailProvider` on top of
`ImapConnection`; use it for folder listing and message fetch rather than driving the
connection directly.

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
