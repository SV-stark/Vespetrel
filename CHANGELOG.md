# Changelog

All notable changes to the **Vespetrel** mail client and protocol engine are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added
- **Interactive New Mail Setup Wizard**:
  - Real, interactive text input fields powered by `gpui-kit::component::input::Input` and `InputState` for manual account setup.
  - Support for custom **Email Address**, **Display Name**, **Password / App Password**, **Incoming Server & Port**, and **Outgoing Server & Port**.
  - Masked password field with native show/hide eye toggle (`.mask_toggle()`).
  - Quick preset chips ("Personal", "Work", "Team") that dynamically pre-populate input fields.
- **OAuth2 Browser Redirect Flow (PKCE)**:
  - Ephemeral loopback HTTP listener on `127.0.0.1:0` with automatic OS port allocation.
  - Cryptographically secure SHA-256 PKCE code challenge and CSRF state validation.
  - Default system web browser launch targeting official consent endpoints (`accounts.google.com` / `login.microsoftonline.com`).
  - Strict loopback Host header and CSRF verification protecting against DNS rebinding and cross-site injection.
  - Friendly HTML confirmation page returned directly to the user's browser upon authorization.
  - Automatic OpenID userinfo discovery endpoint integration (`https://openidconnect.googleapis.com/v1/userinfo` and `https://graph.microsoft.com/v1.0/me`) to automatically extract email and display names.
  - Secure credential storage in native OS keyring (Windows Credential Manager / macOS Keychain / Secret Service).
- **Dual Authentication Modes for Gmail**:
  - `🌐 Browser OAuth2`: One-click web browser authorization with PKCE and token exchange.
  - `🔑 App Password (IMAP)`: Direct IMAP/SMTP connection using 16-character Google App Passwords (`imap.gmail.com:993` / `smtp.gmail.com:587`).
  - Contextual setup hints linking directly to `myaccount.google.com/apppasswords`.
- **Persistent Outbox & Message Dispatch**:
  - Disk-backed SQLite Outbox table with automatic retry backoff and scheduled send support.
  - Background worker thread with graceful shutdown and immediate dispatch via `flume` wake triggers.
- **Proactive OAuth2 Token Refresh**:
  - Automatic token expiration detection with 60-second proactive refresh window before expiry.
  - Seamless fallback to cached credentials if token refresh fails.
- **Protocol Engine Hardening**:
  - IMAP `QRESYNC` and `CONDSTORE` support with incremental UID synchronization.
  - RFC 7162 `VANISHED (EARLIER)` and RFC 3501 `EXPUNGE` message deletion propagation.
  - PGP/MIME and S/MIME cryptographic signature verification with hardware key support.
  - Full-text search (FTS5) transactional integrity with SQLite WAL and accent folding.
  - Sandboxed HTML rendering with strict Content Security Policy (CSP), CID rewrite, and tracking pixel removal.

- **Account Setup & Domain Autodiscovery**:
  - Added domain-based autodiscovery for major email providers (Gmail, Outlook, Fastmail, iCloud, Yahoo, Zoho) with default host and port configurations.
  - Multi-account management with secure OS keyring credential deletion on account removal.
  - Connection error surfacing with informative desktop toast notifications.
- **Folder Tree & Unified Inbox**:
  - Per-folder selection dynamically reloading message summaries from local SQLite storage.
  - Real unread and total badge counts retrieved via transactional SQLite queries.
  - Unified Inbox mode aggregating messages across all active user accounts.
- **Message List Threading, Sorting & Density**:
  - Virtual thread count badges (`🧵 N`) and reply indentation (`↳`) based on JWZ threading.
  - Newest First and Oldest First date sorting order controls.
  - Switchable row density (`Compact` @ 28px, `Comfortable` @ 40px, `Roomy` @ 56px).
  - Diacritic-folding SIMD quick filter chips (`All`, `Unread`, `Starred`, `Files`).
- **Sanitized HTML Reader & Security Badges**:
  - Sandboxed HTML reader with inline CID image rewriting and Content Security Policy (CSP).
  - Tracking pixel blocking and remote image toggle (`RemoteImagePolicy`).
  - Cryptographic and authentication badges (DKIM/SPF Pass, S/MIME, PGP, TLS channel status).
  - Reader attachment tray showing filenames, MIME types, and formatted sizes.
- **Interactive Compose Modal & Drafts**:
  - Interactive GPUI text inputs for `To`, `Subject`, and `Body`.
  - Markdown preview toggle and `[💾 Save Draft]` persisting directly to SQLite Drafts folder.
  - Reply (`Re:`), reply-all, and forward (`Fwd:`) quotation templates.
- **Sent/Trash Lifecycle & Undo Send**:
  - Two-stage deletion semantics (Trash folder before permanent database purge).
  - Configurable Undo Send delay with outbox cancellation via `cancel_outbox` and action toasts.
- **SQLite FTS5 BM25 Ranked Global Search**:
  - Global full-text search with BM25 ranking preserved during message hydration.
  - Header search bar with clear `✕` button that restores current folder messages.
- **JMAP RFC 8620/8621 & Graph Sync Parity**:
  - Full body and preview synchronization in JMAP and Microsoft Graph protocol engines.
  - Real-time status bar with offline and sync error indicators.
- **Contact Harvesting & Autocomplete Chips**:
  - Automatic contact harvesting into SQLite `contacts` table upon sending messages.
  - Interactive recipient autocomplete suggestion chips rendered below the `To:` field.
- **Real-Time Sync Notification Toasts**:
  - Desktop toast notifications for incoming emails with sender and subject previews.
  - Sync error toast notifications surfacing protocol issues.
- **Interactive Configuration & SQLite Persistence**:
  - Settings panel with interactive controls for themes (`Dark Slate`, `OLED Black`, `Catppuccin Mocha`, `Light Paper`, `System Default`).
  - Interactive row density selector and Undo Send delay chips.
  - Toggle switches for tracking pixel stripping and anti-phishing link warnings.
  - Immediate persistence to SQLite database via `save_user_settings`.
- **Anti-Phishing Engine & S/MIME UI**:
  - Heuristic anti-phishing scanner detecting deceptive display domains, punycode homographs, raw IP links, and userinfo spoofing.
  - Prominent phishing alert banner displayed above suspicious email content.
  - Automatic detection of S/MIME signatures (`.p7s`/`.p7m`/`pkcs7`) and OpenPGP armored blocks.
- **Attachment Download & Compose Tray**:
  - Filename sanitization protecting against directory traversal attacks.
  - Single-click attachment downloading to the user's `Downloads` folder.
  - Compose tray attachment loading with per-file removal button (`✕`).
- **Modern `gpui-kit 0.6.0` UI Component Suite**:
  - Application top header encapsulated in `TitleBar::new()` and bottom workspace status in `StatusBar::new()`.
  - Main navigation and settings view converted to `TabBar` and `Tab` elements.
  - Three-pane email workspace powered by `h_resizable` with dynamic split dividers.
  - Message reader view powered by `v_resizable` cleanly separating header and security details from scrollable content.
  - Message list virtualized scrolling via `v_virtual_list` and `MessageListView::virtual_item_sizes` for high-performance 60fps scrolling across massive inboxes.
  - Folder tree integration using `NavigationTree::folder_icon` and `NavigationTree::sorted_folders`.
  - Modal and overlay migrations:
    - `AddAccount`: `Dialog` with `Form::vertical()` and structured `Field` inputs.
    - `Compose`: Slide-over `Sheet` with form fields and autocomplete contact chips.
    - `CommandPalette`: `Dialog` driving `Command` and `CommandItem` search.
    - Header quick info `Popover` with keyboard shortcut details.
  - Native system desktop notifications dispatched through `WindowExt::push_notification` with `Notification::error` and `Notification::success`.
  - Comprehensive theme token styling adopting `cx.theme()` tokens (`background`, `foreground`, `border`) with `ActiveTheme` and `Sizable` traits.

### Fixed
- **IMAP Connection Error Surfacing & Mock Handling**:
  - Eliminated the opaque release-mode error (`"IMAP connection not connected to live server and mock fallback is disabled in release mode"`).
  - Ensured live server connections (Gmail, Outlook, Yahoo, custom IMAP) propagate genuine network, TLS handshake, and timeout errors instead of silently returning disconnected stream states.
  - Added domain-aware `is_mock()` checking for test/demo domains (`example.com`, `imap.example.com`, `.example`, `.invalid`, `.test`, `localhost`, `127.0.0.1`), allowing seamless offline testing and preset exploration without runtime panics.
  - Persisted user-specified `server_host` and `server_port` in `AuthConfig` during setup wizard completion, ensuring custom host/port values are respected by the engine coordinator.
- **Login Wizard Autodiscover Domain Matching**:
  - Fixed fallback host handling in domain autodiscovery when unrecognized domains are provided, ensuring valid default host/port configuration.

### Changed
- **Migrated UI Stack to `gpui-kit 0.6.0`**:
  - Replaced raw git `gpui` dependency with crates.io `gpui-kit`, utilizing standard component layers (`gpui_kit::component::Root`, `gpui_kit::init`, `gpui_kit::component::input`).
  - Installed `aws-lc-rs` default crypto provider to eliminate dual-crypto-provider ambiguity on Windows.
- **Dependency Updates**:
  - Ran `cargo update` upgrading 36 dependencies to their latest compatible versions.
- **Code Style & Formatting**:
  - Ran `cargo fmt --all` across all 11 workspace crates.

