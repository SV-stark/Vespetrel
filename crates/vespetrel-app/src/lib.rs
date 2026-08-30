//! Vespetrel App - GPUI frontend library (re-exports views for bin)

pub mod app;
pub mod state;
pub mod views;

// Real GPUI bridge lives here when gpui dep is enabled (uncomment in Cargo.toml):
// #[cfg(feature = "gpui")]
// pub mod gpui_bridge;
