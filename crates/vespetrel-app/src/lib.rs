//! Vespetrel App - GPUI frontend library (re-exports views for bin)

pub mod app;
pub mod command_palette;
pub mod keybindings;
pub mod platform;
pub mod state;
pub mod views;

pub use command_palette::{ActionCategory, CommandPalette, PaletteAction};
pub use views::quick_filter::QuickFilterState;

pub mod gpui_bridge;
pub mod gui;
