# 🛡️ vespetrel-render

[![Crates.io](https://img.shields.io/crates/v/vespetrel-render.svg)](https://crates.io/crates/vespetrel-render)
[![Documentation](https://docs.rs/vespetrel-render/badge.svg)](https://docs.rs/vespetrel-render)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)

Privacy-first, secure HTML sanitization, tracking pixel removal, and rendering pipeline for **Vespetrel**.

---

## 📦 Overview

`vespetrel-render` sanitizes and formats incoming emails to protect user privacy and ensure safe display within the native GPUI interface:
- **HTML Sanitization:** Strict element and attribute filtering with `ammonia` to eliminate JavaScript, unsafe CSS, and clickjacking attacks.
- **Streaming DOM Rewriting:** Low-latency tag transformation with `lol_html` for 1x1 tracking pixel stripping, external image blocking, and CID inline image resolving.
- **Markdown & Plaintext Pipeline:** Converts plaintext quote trees (`> ...`) and Markdown into structured, readable GPUI layouts.
- **Phishing URL Inspection:** Validates link targets against display text to detect deceptive homoglyph or mismatched link destinations.

## 🚀 Key Capabilities

- **Zero-JavaScript Execution:** Strips `<script>`, `<iframe>`, `<object>`, and inline event handlers (`onclick`, `onerror`).
- **External Asset Blocking:** Automatically blocks remote images until explicitly allowed by the user.

## 💻 Example Usage

```rust
use vespetrel_render::sanitize_html;

fn main() {
    let untrusted_input = r#"<p>Hello <script>alert('xss')</script><img src="http://tracker.com/1x1.png" width="1" height="1"></p>"#;
    let safe_output = sanitize_html(untrusted_input);
    assert!(!safe_output.contains("script"));
    println!("Cleaned HTML: {}", safe_output);
}
```

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
