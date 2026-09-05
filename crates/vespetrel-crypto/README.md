# 🔐 vespetrel-crypto

[![Crates.io](https://img.shields.io/crates/v/vespetrel-crypto.svg)](https://crates.io/crates/vespetrel-crypto)
[![Documentation](https://docs.rs/vespetrel-crypto/badge.svg)](https://docs.rs/vespetrel-crypto)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)

End-to-end encryption, signature verification, OAuth2 PKCE, and OS keyring credential management for **Vespetrel**.

---

## 📦 Overview

`vespetrel-crypto` provides robust security and cryptographic services:
- **OpenPGP (RFC 9580):** Full modern OpenPGP support via `rPGP`, including Ed25519/Cv25519 modern v6 keys, key generation, encryption, decryption, and signature verification.
- **S/MIME & CMS:** X.509 certificate validation and Cryptographic Message Syntax (CMS) verification via `x509-cert` and `cms`.
- **Autocrypt Support:** Parsing and generation of `Autocrypt` e-mail headers for frictionless peer-to-peer encryption setup.
- **OAuth2 PKCE Flow:** Loopback 127.0.0.1:0 HTTP redirect server with CSRF verification and host validation.
- **OS Native Keyring:** Secure credential storage backed by Windows Credential Manager (DPAPI), macOS Keychain, and Linux Secret Service.

## 🚀 Key Capabilities

- **Zeroize Memory Safety:** Sensitive secret keys and passphrases are wiped on drop using `zeroize`.
- **WebPKI Root Verification:** Secure TLS certificate bundle inspection using `webpki-roots`.

## 💻 Example Usage

```rust
use vespetrel_crypto::KeyringStore;

fn main() -> anyhow::Result<()> {
    let store = KeyringStore::new("vespetrel");
    // Securely retrieve account credentials from OS keyring
    let _creds = store.get_password("user@example.com");
    println!("Keyring store operational");

    Ok(())
}
```

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
