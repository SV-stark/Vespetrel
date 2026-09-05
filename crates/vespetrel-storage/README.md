# 💾 vespetrel-storage

[![Crates.io](https://img.shields.io/crates/v/vespetrel-storage.svg)](https://crates.io/crates/vespetrel-storage)
[![Documentation](https://docs.rs/vespetrel-storage/badge.svg)](https://docs.rs/vespetrel-storage)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)

High-performance, transactional local database and full-text search engine for **Vespetrel**, powered by SQLite (WAL mode), FTS5 with BM25 ranking, and compressed raw blob storage.

---

## 📦 Overview

`vespetrel-storage` handles all persistence, caching, and querying requirements for hundreds of thousands of emails, calendar entries, and contacts:
- **Transactional SQLite Engine:** Powered by `rusqlite` and `deadpool-sqlite` with Write-Ahead Logging (WAL), synchronous normal, and memory-mapped I/O.
- **SQLite FTS5 Full-Text Search:** Sub-15ms search across 200,000+ indexed messages with BM25 relevance ranking and unicode61 diacritic stripping.
- **Compressed Raw RFC822 Blob Store:** Transparent compression using LZ4 or Zstandard for raw message bodies and attachments.
- **In-Memory Caching:** High-concurrency TinyLFU caching via `moka` paired with `arc-swap` for lock-free 120 FPS UI reads.

## 🚀 Key Capabilities

- **Outbox Persistence:** Durable offline transactional outbox with delivery state tracking and exponential retry schedules.
- **Schema Migration Engine:** Robust, forward-compatible schema versioning ensuring database integrity across upgrades.
- **Integrity Validation:** Automatic startup `PRAGMA quick_check` verification.

## 💻 Example Usage

```rust
use vespetrel_storage::Database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Open or create SQLite database with WAL mode and FTS5 indices
    let db = Database::open_in_memory()?;

    // Perform BM25 search across indexed messages
    let results = db.search_messages("meeting budget 2026", 50)?;
    println!("Found {} matching messages", results.len());

    Ok(())
}
```

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
