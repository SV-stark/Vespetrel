# Pure Rust `gpui` Mail Client — Implementation Plan & Architecture Specification
**Name:** `Vespetrel` (Vesper + Petrel) — Thunderbird-Parity Rust Desktop Client  
**Target Performance:** 120 FPS GPU rendering, <300ms cold start, <80MB idle RAM, sub-15ms search across 200,000+ emails.

---

## 1. System Architecture Overview

```
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                       VESPETREL DESKTOP APP                                      │
│                                                                                                  │
│  ┌────────────────────────────────────────────────────────────────────────────────────────────┐  │
│  │ GPUI FRONTEND LAYER (GPU Metal / Vulkan / Direct3D)                                        │  │
│  │                                                                                            │  │
│  │  ┌──────────────────┐  ┌─────────────────────────┐  ┌───────────────────────────────────┐  │  │
│  │  │ Navigation Tree  │  │ Virtual Thread List     │  │ Message Reader Viewport           │  │  │
│  │  │ • Accounts       │  │ • 100k+ items virtualized│  │ • Plaintext / Markdown (Native)   │  │  │
│  │  │ • Folders / Tags │  │ • Sub-1ms frame time    │  │ • Rich Email HTML (Sandboxed Wry) │  │  │
│  │  │ • Calendars/PIM  │  │ • Dynamic row heights   │  │ • Remote Tracker & Image Blocker  │  │  │
│  │  └──────────────────┘  └─────────────────────────┘  └───────────────────────────────────┘  │  │
│  │  ┌──────────────────────────────────────────────────────────────────────────────────────┐  │  │
│  │  │ Compose Editor (Tree-sitter, Markdown / Rich Text, Contact Autocomplete Chips)       │  │  │
│  │  └──────────────────────────────────────────────────────────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────┬─────────────────────────────────────────────┘  │
│                                                 │ Event Channel (GPUI cx.spawn / Tokio mpsc)     │
│                                                 ▼                                                │
│  ┌────────────────────────────────────────────────────────────────────────────────────────────┐  │
│  │ TOKIO CORE ASYNC RUNTIME & SYNC ENGINE                                                     │  │
│  │                                                                                            │  │
│  │  ┌────────────────────────┐  ┌────────────────────────┐  ┌──────────────────────────────┐  │  │
│  │  │ Account Sync Workers   │  │ Protocol Dispatcher    │  │ PIM Engine (CalDAV / CardDAV)│  │  │
│  │  │ • 1 Actor per account  │  │ • IMAP (IDLE/QRESYNC)  │  │ • libdav + icalendar         │  │  │
│  │  │ • Auto-reconnect & back│  │ • JMAP (RFC 8620/8621) │  │ • vcard4 contact sync        │  │  │
│  │  │ • Delta change tracker │  │ • Microsoft Graph REST │  │ • Task / VTODO engine        │  │  │
│  │  └────────────────────────┘  └────────────────────────┘  └──────────────────────────────┘  │  │
│  │  ┌──────────────────────────────────────────────────────────────────────────────────────┐  │  │
│  │  │ Security & Parsing Subsystems                                                        │  │  │
│  │  │ • stalwartlabs/mail-parser (Zero-copy MIME)   • rPGP (OpenPGP RFC 9580 v6)           │  │  │
│  │  │ • lettre + mail-send (SMTP + DKIM + XOAUTH2)  • RustCrypto (X.509 S/MIME)            │  │  │
│  │  │ • OAuth2 PKCE Token Engine + OS Keyring       • ammonia + lol_html Sanitizer         │  │  │
│  │  └──────────────────────────────────────────────────────────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────┬─────────────────────────────────────────────┘  │
│                                                 │ Thread-pool I/O (deadpool-sqlite)              │
│                                                 ▼                                                │
│  ┌────────────────────────────────────────────────────────────────────────────────────────────┐  │
│  │ LOCAL STORAGE & INDEXING LAYER                                                             │  │
│  │                                                                                            │  │
│  │  ┌──────────────────────────────────────────────┐  ┌────────────────────────────────────┐  │  │
│  │  │ Rusqlite (WAL Mode, Memory-Mapped I/O)       │  │ SQLite FTS5 Search Engine          │  │  │
│  │  │ • Accounts, Folders, Envelopes, Threads      │  │ • Tokenize = unicode61 + trigram   │  │  │
│  │  │ • Foreign keys, transactional migrations     │  │ • BM25 Ranking, Sub-15ms queries   │  │  │
│  │  └──────────────────────────────────────────────┘  └────────────────────────────────────┘  │  │
│  │  ┌──────────────────────────────────────────────────────────────────────────────────────┐  │  │
│  │  │ Compressed Raw Blob Store (lz4 / zstd on disk for offline message bodies & RFC822)   │  │  │
│  │  └──────────────────────────────────────────────────────────────────────────────────────┘  │  │
│  └────────────────────────────────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Cargo Workspace Architecture

The project is structured as a modular Cargo workspace to enforce clean separation of concerns and fast compilation times:

```
vespetrel/
├── Cargo.toml                  # Workspace manifest
├── rust-toolchain.toml         # Pinned stable Rust toolchain
├── crates/
│   ├── vespetrel-app/          # GPUI UI, views, components, dock layout, wry webview bridge
│   ├── vespetrel-core/         # Domain models (Account, Folder, Message, Event, Contact)
│   ├── vespetrel-storage/      # Rusqlite database, schema migrations, FTS5 indexing, blob store
│   ├── vespetrel-engine/       # Tokio sync coordinator, account worker actors, event bus
│   ├── vespetrel-imap/         # IMAP client with IDLE, CONDSTORE, QRESYNC, XOAUTH2
│   ├── vespetrel-jmap/         # Stalwart JMAP client adapter (RFC 8620/8621)
│   ├── vespetrel-graph/        # Microsoft Graph REST API adapter (Mail, Calendar, Contacts)
│   ├── vespetrel-smtp/         # Lettre + mail-send SMTP submission engine with DKIM & XOAUTH2
│   ├── vespetrel-dav/          # CalDAV & CardDAV sync engine via libdav, icalendar, vcard4
│   ├── vespetrel-crypto/       # rPGP (OpenPGP RFC 9580), RustCrypto S/MIME, keyring integration
│   └── vespetrel-render/       # HTML sanitization (ammonia, lol_html), linkify, tracker stripping
```

---

## 3. Database Schema & Storage Architecture

### 3.1 SQLite PRAGMA Configuration
```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA cache_size = -64000; -- 64MB memory cache
PRAGMA mmap_size = 268435456; -- 256MB memory-mapped I/O
PRAGMA temp_store = MEMORY;
```

### 3.2 Relational DDL
```sql
-- Accounts
CREATE TABLE accounts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    provider_type TEXT NOT NULL, -- 'imap', 'jmap', 'graph', 'gmail'
    auth_config JSON NOT NULL,   -- OAuth2 tokens or password refs in keyring
    sync_state JSON NOT NULL DEFAULT '{}',
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL
);

-- Mail Folders / Mailboxes
CREATE TABLE folders (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    remote_id TEXT NOT NULL,
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'custom', -- 'inbox', 'sent', 'drafts', 'trash', 'archive', 'junk'
    uid_validity INTEGER,
    highest_mod_seq INTEGER DEFAULT 0,
    total_count INTEGER DEFAULT 0,
    unread_count INTEGER DEFAULT 0,
    UNIQUE(account_id, remote_id)
);

-- Message Threads (JWZ Threading algorithm output)
CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    subject TEXT,
    last_message_at INTEGER NOT NULL,
    message_count INTEGER NOT NULL DEFAULT 1,
    unread_count INTEGER NOT NULL DEFAULT 0,
    snippet TEXT
);

-- Messages (Metadata & Envelopes)
CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    thread_id TEXT REFERENCES threads(id) ON DELETE SET NULL,
    remote_uid INTEGER NOT NULL,
    message_id_header TEXT,
    in_reply_to TEXT,
    subject TEXT,
    from_address TEXT NOT NULL,
    from_name TEXT,
    to_addresses JSON NOT NULL,   -- Array of {name, email}
    cc_addresses JSON NOT NULL,   -- Array of {name, email}
    bcc_addresses JSON NOT NULL,  -- Array of {name, email}
    reply_to JSON,
    sent_at INTEGER NOT NULL,
    received_at INTEGER NOT NULL,
    is_read INTEGER NOT NULL DEFAULT 0,
    is_flagged INTEGER NOT NULL DEFAULT 0,
    is_draft INTEGER NOT NULL DEFAULT 0,
    has_attachments INTEGER NOT NULL DEFAULT 0,
    body_snippet TEXT,
    body_text_preview TEXT,
    blob_path TEXT NOT NULL,      -- Path to compressed lz4 raw RFC822 message
    size_bytes INTEGER NOT NULL,
    UNIQUE(folder_id, remote_uid)
);

-- Message Labels (Gmail & Custom Tags)
CREATE TABLE message_labels (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    PRIMARY KEY(message_id, label)
);

-- Attachments Metadata
CREATE TABLE attachments (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    content_id TEXT,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    blob_path TEXT NOT NULL,
    is_inline INTEGER NOT NULL DEFAULT 0
);

-- Full-Text Search Virtual Table (FTS5)
CREATE VIRTUAL TABLE messages_fts USING fts5(
    message_id UNINDEXED,
    account_id UNINDEXED,
    subject,
    from_address,
    from_name,
    to_addresses,
    body_content,
    tokenize = 'unicode61 remove_diacritics 2'
);

-- Triggers for FTS5 automatic sync
CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(message_id, account_id, subject, from_address, from_name, to_addresses, body_content)
    VALUES (new.id, new.account_id, new.subject, new.from_address, new.from_name, new.to_addresses, new.body_text_preview);
END;

CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE message_id = old.id;
END;
```

---

## 4. Protocol Implementation Details

### 4.1 Unified Provider Trait Architecture
All backend engines implement a unified asynchronous trait:

```rust
#[async_trait]
pub trait MailProvider: Send + Sync {
    /// Perform full or delta synchronization of mailboxes
    async fn sync_folder_list(&self) -> Result<Vec<RemoteFolder>>;
    
    /// Sync message changes within a folder using CONDSTORE/QRESYNC/Delta tokens
    async fn sync_messages(&self, folder: &Folder, state: SyncState) -> Result<SyncDelta>;
    
    /// Fetch raw RFC822 MIME payload for offline storage
    async fn fetch_raw_message(&self, remote_id: &str) -> Result<Vec<u8>>;
    
    /// Submit composed message for delivery
    async fn send_message(&self, message: &ComposedMessage) -> Result<()>;
    
    /// Update message flags (Read, Flagged, Deleted, Moved)
    async fn update_flags(&self, remote_ids: &[u32], add: &[Flag], remove: &[Flag]) -> Result<()>;
}
```

### 4.2 IMAP Protocol Sync Engine (`vespetrel-imap`)
* **State Machine Actor**: Runs on an isolated Tokio worker thread per account.
* **Capabilities Negotiated**: `ENABLE RFC822.SIZE CONDSTORE QRESYNC IDLE SPECIAL-USE MOVE AUTH=XOAUTH2`.
* **IDLE Loop**:
  1. Issues `IDLE` command to monitor real-time inbox changes.
  2. Automatically renews every 25 minutes (RFC 2177 specifies 29-minute timeout).
  3. Interrupted via `DONE` when outgoing operations (flag change, send) are queued.
* **Delta Synchronization**:
  * Evaluates `UIDVALIDITY`. If changed, triggers complete folder cache rebuild.
  * Uses `UID FETCH (UID FLAGS MODSEQ)` with `CHANGEDSINCE` to download only modified records.

### 4.3 JMAP Protocol Engine (`vespetrel-jmap`)
* Uses `stalwartlabs/jmap-client` (0.3.x) targeting RFC 8620 (Core) & RFC 8621 (Mail).
* Real-time push via `EventSource` (SSE) / WebSocket push channels.
* Atomic multi-call queries: `Email/get`, `Email/queryChanges`, `Mailbox/get` in a single HTTP POST round-trip.

### 4.4 Microsoft Graph REST Engine (`vespetrel-graph`)
* For Microsoft 365 / Corporate Exchange environments where IMAP is disabled.
* Built on `reqwest` + `rustls` (pure Rust).
* **Delta Query API**: Polls `GET https://graph.microsoft.com/v1.0/me/mailFolders/{id}/messages/delta?$deltatoken=...` for incremental sync.
* **Events & Contacts**: Connects to `/me/events` and `/me/contacts` for native Exchange PIM synchronization.

### 4.5 Authentication & Secrets Management (`vespetrel-crypto`)
* **OAuth2 Engine**:
  * Google OAuth2 with PKCE: `https://accounts.google.com/o/oauth2/v2/auth`
  * Microsoft Entra ID with PKCE: `https://login.microsoftonline.com/common/oauth2/v2.0/authorize`
  * Local loopback listener bound to `http://127.0.0.1:8989/callback` for browser redirect.
* **Keyring Storage**:
  * Tokens encrypted and saved via `keyring-rs` (v3) targeting macOS Keychain, Windows Credential Manager, and Linux Secret Service.
  * Master Password fallback with `Argon2id` key derivation and `AES-256-GCM` SQLite encryption for portable/headless Linux setups.

---

## 5. Security, MIME Parsing & HTML Rendering Pipeline

### 5.1 MIME Extraction
* `stalwartlabs/mail-parser`:
  * Zero-copy string slicing with `Cow<str>`.
  * Fully compliant with RFC 5322, RFC 2045-2049, RFC 2231, and RFC 6532 (Internationalized Email).
  * Automatically handles 41 legacy character encodings.
  * Streams large attachments directly to disk via `lz4` compression.

### 5.2 HTML Sanitization & Tracking Protection Pipeline

```
Raw HTML from MIME 
       │
       ▼
[lol_html Streaming Rewriter]
   ├─ Remove <script>, <iframe>, <object>, <embed>, <applet>
   ├─ Strip remote tracking pixels (1x1 transparent GIFs/PNGs)
   ├─ Rewrite `cid:image001.png` to local temporary `blob://` URI
   └─ Disable external images (`src="https://..."` -> `data-blocked-src="..."`)
       │
       ▼
[ammonia Sanitizer]
   ├─ Strict HTML tag and CSS attribute allowlist
   ├─ Enforce `rel="noopener noreferrer"` and `target="_blank"` on all <a> tags
   └─ Clean inline CSS styles (prevent position: fixed overlays / phishing)
       │
       ▼
[Sandboxed Viewport]
   ├─ Native GPUI Markdown (for plaintext / simple messages)
   └─ Isolated Sandboxed OS WebView via `wry` (for complex marketing emails)
```

### 5.3 OpenPGP & S/MIME Encryption
* **OpenPGP**:
  * `rPGP` (pure Rust, RFC 9580 v6 specification, AEAD support, zero C dependencies).
  * Autocrypt 1.1 header generation and automated key exchange.
* **S/MIME**:
  * `RustCrypto` (`x509-cert`, `cms`, `aes-gcm`, `rsa`).
  * Validates corporate X.509 certificate chains and decrypts CMS payloads.

---

## 6. GPUI Desktop Frontend & UI Architecture

### 6.1 View Hierarchy & Layout
Built using `gpui` + `longbridge/gpui-component`:

```
App::run(cx) -> MainWindow
  ├─ TitleBar (Window controls, account switcher, global search bar)
  ├─ WorkspaceView (Dock Layout)
  │   ├─ Left Pane: Navigation Tree (Folders, Tags, Calendars, Contacts)
  │   ├─ Center Pane: Virtual Message List (100k+ rows, 120 FPS, dynamic height)
  │   └─ Right Pane: Message Viewer & Action Bar (Reply, Forward, Archive, Delete)
  ├─ Modal Layer (Compose Window, Account Setup Wizard, Settings)
  └─ Notification Toast Overlay (Sync progress, errors, new email banners)
```

### 6.2 The Tokio-to-GPUI Asynchronous Bridge
```rust
pub struct MessageListView {
    messages: Vec<MessageSummary>,
    sync_rx: tokio::sync::mpsc::UnboundedReceiver<SyncEvent>,
}

impl MessageListView {
    pub fn new(cx: &mut ViewContext<Self>, mut sync_rx: tokio::sync::mpsc::UnboundedReceiver<SyncEvent>) -> Self {
        // Spawn asynchronous Tokio listener bound to GPUI event loop
        cx.spawn(|this, mut cx| async move {
            while let Some(event) = sync_rx.recv().await {
                cx.update(|cx| {
                    this.update(cx, |view, cx| {
                        view.handle_sync_event(event, cx);
                    });
                }).ok();
            }
        }).detach();

        Self {
            messages: Vec::new(),
            sync_rx,
        }
    }
    
    fn handle_sync_event(&mut self, event: SyncEvent, cx: &mut ViewContext<Self>) {
        match event {
            SyncEvent::MessagesInserted(new_msgs) => {
                self.messages.splice(0..0, new_msgs);
                cx.notify(); // Triggers instant GPU redraw
            }
            SyncEvent::MessageFlagsUpdated { id, is_read, is_flagged } => {
                if let Some(msg) = self.messages.iter_mut().find(|m| m.id == id) {
                    msg.is_read = is_read;
                    msg.is_flagged = is_flagged;
                    cx.notify();
                }
            }
            _ => {}
        }
    }
}
```

---

## 7. Phased Implementation Roadmap

```
┌────────────────────────────────────────────────────────────────────────┐
│                        DEVELOPMENT ROADMAP                             │
│                                                                        │
│  Phase 0: Storage Core, Search Engine & Edition 2024 (COMPLETE)        │
│  ├── [x] Full workspace migration to Rust 2024 Edition                 │
│  ├── [x] Rusqlite schema + FTS5 indexing + _schema_migrations tracking │
│  ├── [x] Zero-copy stalwartlabs/mail-parser MIME pipeline              │
│  └── [x] Compressed raw message blob store (lz4_flex + zstd)           │
│                                                                        │
│  Phase 1: GPUI 3-Pane UI Shell & Reader (IN PROGRESS)                  │
│  ├── [x] 3-Pane dock state model & view logic                          │
│  ├── [x] HTML sanitizer (ammonia) & tracker stripper (lol_html)        │
│  ├── [x] Sandboxed HTML viewport with Content-Security-Policy (CSP)    │
│  ├── [ ] 120 FPS Virtualized message list (gpui-component)             │
│  └── [ ] Sandboxed HTML message reader (wry webview)                   │
│                                                                        │
│  Phase 2: Authentication, Multi-Provider & Sending (GMAIL READY)       │
│  ├── [x] OAuth2 PKCE engine + Loopback TCP server (127.0.0.1:8989)     │
│  ├── [x] Google OAuth2 access token auto-refresh routine               │
│  ├── [x] OS Keyring token storage (keyring-rs v3)                      │
│  ├── [x] Tokio Account Worker actors with SQLite pool persistence      │
│  ├── [x] IMAP IDLE state machine, QRESYNC/CONDSTORE & XOAUTH2 commands │
│  ├── [x] JMAP client adapter (Fastmail / Stalwart RFC 8620/8621)       │
│  ├── [x] Microsoft Graph REST engine (Exchange Online)                 │
│  ├── [x] SMTP submit engine + Lettre live relay (send_live)            │
│  └── [ ] Interactive GUI login wizard modal & compose view             │
│                                                                        │
│  Phase 3: PIM (Calendar, Contacts, Tasks) & Encryption                 │
│  ├── [x] CalDAV / CardDAV sync client foundation (libdav, icalendar)   │
│  ├── [x] rPGP OpenPGP armor detector & Autocrypt 1.1 engine            │
│  └── [ ] Calendar grid views & contact address book                    │
│                                                                        │
│  Phase 4: Polish, Windows Hardening & Distribution                     │
│  ├── [x] Headless testable runner (VespetrelApp)                       │
│  ├── [ ] Windows Direct3D / IME stabilization                          │
│  └── [ ] Native OS packaging (.dmg, .deb, .rpm, .msi)                  │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 8. Root `Cargo.toml` Workspace Configuration

```toml
[workspace]
resolver = "2"
members = [
    "crates/vespetrel-app",
    "crates/vespetrel-core",
    "crates/vespetrel-storage",
    "crates/vespetrel-engine",
    "crates/vespetrel-imap",
    "crates/vespetrel-jmap",
    "crates/vespetrel-graph",
    "crates/vespetrel-smtp",
    "crates/vespetrel-dav",
    "crates/vespetrel-crypto",
    "crates/vespetrel-render",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"

[workspace.dependencies]
# Async & Runtime
tokio = { version = "1.38", features = ["full"] }
async-trait = "0.1"
futures = "0.3"

# UI & Rendering
gpui = { git = "https://github.com/zed-industries/zed" }
gpui-component = { git = "https://github.com/longbridge/gpui-component" }
wry = "0.41"

# Database & Storage
rusqlite = { version = "0.31", features = ["bundled", "bundled-sqlcipher", "chrono", "serde_json"] }
deadpool-sqlite = "0.8"
lz4_flex = "0.11"
zstd = "0.13"

# Mail & Protocols
mail-parser = "0.11"
mail-builder = "0.4"
mail-send = { version = "0.4", default-features = false, features = ["rustls-tls"] }
lettre = { version = "0.11", default-features = false, features = ["tokio1-rustls", "builder", "smtp-transport"] }
jmap-client = "0.3"
imap-codec = "2.0"
imap-types = "2.0"
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }

# PIM (Calendar & Contacts)
libdav = "0.10"
icalendar = "0.16"
vcard4 = "0.4"

# Security & Crypto
rustls = { version = "0.23", default-features = false, features = ["aws_lc_rs"] }
oauth2 = "4.4"
keyring = { version = "3.0", features = ["apple-native", "windows-native", "sync-secret-service"] }
rpgp = "0.14"
x509-cert = "0.2"
cms = "0.2"
zeroize = { version = "1.8", features = ["zeroize_derive"] }

# HTML Sanitization
ammonia = "4.0"
lol_html = "2.0"

# Serialization & Utils
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```
