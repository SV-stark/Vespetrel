//! 3-Pane dock layout §6.1
//! When `gpui` feature is enabled this implements gpui::View.
//! Otherwise this is a plain logic struct that can be unit-tested.

use crate::state::AppState;

pub struct WorkspaceView {
    pub state: AppState,
    /// Pane sizes as fractions (left, center, right) - dock layout
    pub panes: [f32; 3],
}

impl WorkspaceView {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            panes: [0.22, 0.38, 0.40],
        }
    }

    pub fn set_panes(&mut self, panes: [f32; 3]) {
        self.panes = panes;
    }

    pub fn description(&self) -> String {
        format!(
            "Workspace left={:.0}% center={:.0}% right={:.0}% | {} msgs | folder={:?}",
            self.panes[0] * 100.0,
            self.panes[1] * 100.0,
            self.panes[2] * 100.0,
            self.state.messages.len(),
            self.state.selected_folder
        )
    }
}

// Real GPUI impl (enable when gpui dep is present):
// #[cfg(feature = "gpui")]
// mod gpui_impl {
//     use super::*;
//     use gpui::*;
//     // impl Render for WorkspaceView { ... }
// }
