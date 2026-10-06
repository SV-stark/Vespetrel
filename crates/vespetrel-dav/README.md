# 📅 vespetrel-dav

[![Crates.io](https://img.shields.io/crates/v/vespetrel-dav.svg)](https://crates.io/crates/vespetrel-dav)
[![Documentation](https://docs.rs/vespetrel-dav/badge.svg)](https://docs.rs/vespetrel-dav)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)

CalDAV (RFC 4791) and CardDAV (RFC 6352) synchronization engine for **Vespetrel**.

Requests and responses are built and parsed directly on `reqwest`: the WebDAV method set,
XML multistatus handling and iCalendar/vCard parsing are implemented in-crate.

---

## 📦 Overview

`vespetrel-dav` provides two-way calendar and contact synchronization with services such as Nextcloud, iCloud, Google Calendar, Fastmail, and Radicale:
- **CalDAV (RFC 4791):** Calendar home discovery (`calendar_home`), `PROPFIND` listing (`list_calendars`), event fetch and write (`sync_calendar`, `create_or_update_event`, `delete_event`), plus `VEVENT` and `VTODO` parsing.
- **CardDAV (RFC 6352):** Address book collection discovery (`addressbook_home`), `list_contacts`, and contact write/delete.
- **WebDAV Sync-Collection (RFC 6578):** Delta synchronization using sync-tokens to minimize bandwidth and roundtrips.

## 🚀 Key Capabilities

- **Well-known URL bootstrapping:** `/.well-known/caldav` and `/.well-known/carddav`. DNS SRV
  lookup (RFC 6764) is *not* implemented.
- **Conflict Resolution:** HTTP ETag-based optimistic concurrency control.
- **Not implemented:** `RRULE` recurrence expansion. Events are read and written verbatim, so a
  recurring series round-trips unchanged but individual occurrences are not expanded into
  separate entries.

## 💻 Example Usage

```rust
use vespetrel_dav::{DavClient, DavConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = DavConfig::new("https://dav.example.com", "user", "pass");
    let client = DavClient::new(config);

    let calendars = client.list_calendars().await?;
    let contacts = client.list_contacts().await?;

    Ok(())
}
```

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
