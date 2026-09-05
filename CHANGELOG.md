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

### Changed
- **Migrated UI Stack to `gpui-kit 0.6.0`**:
  - Replaced raw git `gpui` dependency with crates.io `gpui-kit`, utilizing standard component layers (`gpui_kit::component::Root`, `gpui_kit::init`, `gpui_kit::component::input`).
  - Installed `aws-lc-rs` default crypto provider to eliminate dual-crypto-provider ambiguity on Windows.
- **Code Style & Formatting**:
  - Ran `cargo fmt --all` across all 11 workspace crates.
