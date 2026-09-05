# 🕊️ vespetrel-core

[![Crates.io](https://img.shields.io/crates/v/vespetrel-core.svg)](https://crates.io/crates/vespetrel-core)
[![Documentation](https://docs.rs/vespetrel-core/badge.svg)](https://docs.rs/vespetrel-core)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)

Foundational domain models, data structures, and protocol abstractions for the **Vespetrel** high-performance email and personal information management (PIM) client.

---

## 📦 Overview

`vespetrel-core` provides zero-overhead, memory-safe data structures representing all primary entities within an email ecosystem:
- **Mail Entities:** `Account`, `Folder`, `FolderRole`, `Message`, `MessageSummary`, `Thread`, and `Attachment`.
- **PIM Entities:** `Contact` (vCard 4.0 RFC 6350), `CalendarEvent` (iCalendar RFC 5545), and `TaskItem`.
- **Configuration & Security:** `UserSettings`, `ProviderType` (IMAP, JMAP, Microsoft Graph), and `SecurityStatus` (PGP, S/MIME, TLS).
- **Sync & Event Bus:** `SyncEvent` enum powering the cross-thread bridge between background Tokio actors and UI rendering pipelines.

## 🚀 Key Features

- **Zero-Allocation Substring Matching:** Custom sliding window byte search optimized for quick filtering.
- **SIMD UTF-8 Validation:** Blazing-fast string sanity checks using `simdutf8`.
- **Deterministic Hashing:** High-speed, DoS-resistant hashing with `ahash` across message identifiers and cache keys.
- **Serde Serialization:** Complete support for JSON serialization across all state representations.

## 💻 Example Usage

```rust
use vespetrel_core::{Account, Folder, FolderRole, ProviderType};

// Initialize an account descriptor
let account = Account::new(
    "acct_work",
    "work@company.com",
    "Work Email",
    ProviderType::Jmap,
);

// Define folder hierarchy
let inbox = Folder::new("inbox_01", "Inbox", FolderRole::Inbox, &account.id);
assert_eq!(inbox.role, FolderRole::Inbox);
```

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
