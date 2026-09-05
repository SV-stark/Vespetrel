# 📅 vespetrel-dav

[![Crates.io](https://img.shields.io/crates/v/vespetrel-dav.svg)](https://crates.io/crates/vespetrel-dav)
[![Documentation](https://docs.rs/vespetrel-dav/badge.svg)](https://docs.rs/vespetrel-dav)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)

CalDAV (RFC 4791) and CardDAV (RFC 6352) synchronization engine for **Vespetrel**, powered by `libdav`, `icalendar`, and `vcard4`.

---

## 📦 Overview

`vespetrel-dav` provides two-way calendar and contact synchronization with services such as Nextcloud, iCloud, Google Calendar, Fastmail, and Radicale:
- **CalDAV (RFC 4791):** Calendar home discovery, event fetch (`VEVENT`), recurrence rule evaluation (`RRULE`), and calendar scheduling.
- **CardDAV (RFC 6352):** Address book collection discovery, vCard 3.0 & 4.0 contact sync, and address book query reports.
- **WebDAV Sync-Collection (RFC 6578):** High-efficiency delta synchronization using sync-tokens to minimize bandwidth and roundtrips.

## 🚀 Key Capabilities

- **Automatic Service Discovery:** RFC 6764 DNS SRV and well-known URL bootstrapping (`/.well-known/caldav` and `/.well-known/carddav`).
- **Conflict Resolution:** HTTP ETag-based optimistic concurrency control.

## 💻 Example Usage

```rust
use vespetrel_dav::DavClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = DavClient::new("https://dav.example.com", "user", "pass");
    println!("DAV client configured for CalDAV & CardDAV sync");

    Ok(())
}
```

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
