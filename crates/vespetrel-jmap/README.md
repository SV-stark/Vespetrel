# ⚡ vespetrel-jmap

[![Crates.io](https://img.shields.io/crates/v/vespetrel-jmap.svg)](https://crates.io/crates/vespetrel-jmap)
[![Documentation](https://docs.rs/vespetrel-jmap/badge.svg)](https://docs.rs/vespetrel-jmap)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)

Modern Stalwart JMAP adapter implementing RFC 8620 (Core JMAP) and RFC 8621 (JMAP for Mail) for **Vespetrel**.

---

## 📦 Overview

`vespetrel-jmap` provides native, fast client synchronization for JMAP-enabled mail services (Fastmail, Stalwart Mail Server, Apache James):
- **Core Protocol (RFC 8620):** Auto-discovery via `.well-known/jmap`, authenticated session retrieval, batch method call pipelining.
- **Mail Handling (RFC 8621):** Fast `Email/query`, `Email/get`, `Email/changes`, and `Mailbox/changes` tracking.
- **Push EventSource:** Server-Sent Events (SSE) push notifications for low-latency update dispatch.
- **Efficient Binary Upload/Download:** Native JMAP blob upload and download streams.

## 🚀 Key Capabilities

- **Batched Request Pipelining:** Group multiple API calls (query + fetch) into single HTTP roundtrips.
- **Incremental Changeset Sync:** Precise delta calculation utilizing state tokens.

## 💻 Example Usage

```rust
use vespetrel_jmap::JmapClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = JmapClient::discover("https://jmap.example.com", "bearer_token").await?;
    let accounts = client.session().accounts();
    println!("Discovered {} JMAP account(s)", accounts.len());

    Ok(())
}
```

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
