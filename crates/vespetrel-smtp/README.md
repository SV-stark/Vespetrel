# 📤 vespetrel-smtp

[![Crates.io](https://img.shields.io/crates/v/vespetrel-smtp.svg)](https://crates.io/crates/vespetrel-smtp)
[![Documentation](https://docs.rs/vespetrel-smtp/badge.svg)](https://docs.rs/vespetrel-smtp)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)

Asynchronous SMTP submission engine built on `lettre` with support for STARTTLS, SMTPS, DKIM signatures, and XOAUTH2 for **Vespetrel**.

---

## 📦 Overview

`vespetrel-smtp` delivers reliable, secure outgoing email transmission:
- **Transport Security:** STARTTLS and implicit TLS via Rustls and AWS-LC-RS.
- **Modern Authentication:** SASL PLAIN, LOGIN, and XOAUTH2 token submission.
- **DKIM Signing:** Direct outbound DKIM header generation.
- **Multi-Part MIME Construction:** Standards-compliant MIME packaging with attachments, alternative text/html parts, and inline CID images.

## 🚀 Key Capabilities

- **Outbox Integration:** Connects directly with the persistent SQLite transactional outbox.
- **Error Diagnostic Classification:** Precise SMTP status code classification (temporary 4xx vs. permanent 5xx failures).

## 💻 Example Usage

```rust
use vespetrel_smtp::SmtpSender;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let sender = SmtpSender::new("smtp.example.com", 587);
    println!("SMTP submission transport ready");

    Ok(())
}
```

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
