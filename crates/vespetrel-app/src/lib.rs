//! Vespetrel App - GPUI frontend library (re-exports views for bin)

pub mod app;
pub mod command_palette;
pub mod keybindings;
pub mod platform;
pub mod state;
pub mod views;

pub use command_palette::{ActionCategory, CommandPalette, PaletteAction};

// Real GPUI bridge lives here when gpui dep is enabled (uncomment in Cargo.toml):
// #[cfg(feature = "gpui")]
// pub mod gpui_bridge;
