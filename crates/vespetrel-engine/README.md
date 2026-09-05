# ⚙️ vespetrel-engine

[![Crates.io](https://img.shields.io/crates/v/vespetrel-engine.svg)](https://crates.io/crates/vespetrel-engine)
[![Documentation](https://docs.rs/vespetrel-engine/badge.svg)](https://docs.rs/vespetrel-engine)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)

Tokio-powered asynchronous synchronization coordinator and account actor orchestrator for **Vespetrel**.

---

## 📦 Overview

`vespetrel-engine` coordinates all multi-protocol background tasks and account lifecycle events:
- **Account Worker Actors:** Dedicated Tokio actors per configured account handling concurrent sync loops, IDLE listeners, and delta fetches.
- **Protocol Dispatch:** Dynamically routes mail operations to IMAP, JMAP, or Microsoft Graph adapters based on account configuration.
- **Outbox Worker:** Persistent transactional queue processor with exponential backoff, circuit-breaking, and rate-limiting.
- **Cross-Thread Bridge:** Flume MPSC event streaming from Tokio background tasks to the UI thread for instant 120 FPS reactivity.

## 🚀 Key Capabilities

- **Graceful Shutdown:** Clean cancellation token propagation ensuring all in-flight sync operations and database transactions finish safely.
- **State Snapshot Publishing:** Atomic lock-free UI state updates using `ArcSwap`.
- **Scheduled Sending:** Background timer-based release of scheduled outbox drafts.

## 💻 Example Usage

```rust
use std::sync::Arc;
use vespetrel_engine::SyncCoordinator;
use vespetrel_storage::Database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = Arc::new(Database::open_in_memory()?);
    let coordinator = SyncCoordinator::new(db);

    // Spawn background sync workers
    coordinator.start().await?;

    Ok(())
}
```

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
