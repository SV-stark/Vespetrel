# 🏢 vespetrel-graph

[![Crates.io](https://img.shields.io/crates/v/vespetrel-graph.svg)](https://crates.io/crates/vespetrel-graph)
[![Documentation](https://docs.rs/vespetrel-graph/badge.svg)](https://docs.rs/vespetrel-graph)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)

Microsoft Graph REST API adapter for Microsoft 365 and Exchange Online accounts in **Vespetrel**.

---

## 📦 Overview

`vespetrel-graph` enables first-class synchronization with modern Microsoft 365 / Exchange Online corporate infrastructure:
- **Delta Link Synchronization:** Utilizes `/me/mailFolders/{id}/messages/delta` tokens for optimal incremental changesets without full mailbox scanning.
- **REST Mail Operations:** Send, draft, move, delete, and flag messages using standard Graph REST endpoints.
- **OAuth2 Token Handling:** Seamless bearer token integration with automatic token refreshing.

## 🚀 Key Capabilities

- **JSON Payload Serialization:** Native Serde mappings for Microsoft Graph resources.
- **Rate-Limiting & Throttling Handling:** Automatic recognition and backoff handling for HTTP 429 (`Retry-After`).

## 💻 Example Usage

```rust
use vespetrel_graph::GraphClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = GraphClient::new("access_token_here");
    let folders = client.list_mail_folders().await?;
    println!("Retrieved {} Microsoft Graph folder(s)", folders.len());

    Ok(())
}
```

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
