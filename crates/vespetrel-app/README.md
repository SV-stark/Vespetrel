# 🕊️ vespetrel-app

[![Crates.io](https://img.shields.io/crates/v/vespetrel-app.svg)](https://crates.io/crates/vespetrel-app)
[![Documentation](https://docs.rs/vespetrel-app/badge.svg)](https://docs.rs/vespetrel-app)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)

The pure-Rust GPU-accelerated desktop application for **Vespetrel**, built on `gpui-kit 0.6.0` for sub-50MB memory footprints, instant global search, and 120+ FPS rendering.

---

## 📦 Overview

`vespetrel-app` provides the primary desktop interface for the Vespetrel client:
- **3-Pane Fluid Workspace:** Folder hierarchy sidebar, virtualized thread list, and security-hardened message reader.
- **Virtualized Message List:** Zero-lag list virtualization capable of scrolling through 200,000+ emails smoothly.
- **Interactive Setup Wizard:** Guided account creation featuring input states, live credential validation, and password masking with toggle.
- **Modal System & Command Palette:** Global ⌘K command palette, full-featured compose modal with Markdown toggle, and granular settings dialog.
- **Native GPU Rendering:** Direct rendering through Vulkan, Metal, or Direct3D without Electron or browser runtimes.

## 🚀 Running the Application

To run the application locally in debug mode:

```bash
cargo run --bin vespetrel
```

To run with release optimizations:

```bash
cargo run --release --bin vespetrel
```

## 🛠️ Key Components

- `src/main.rs`: Application entry point, logging initialization, and GPUI runtime bootstrapping.
- `src/gui.rs`: Unified workspace rendering, state management, and modal orchestration.
- `src/views/`: Reusable, modular views including navigation, virtualized message lists, reader, and status bars.

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
