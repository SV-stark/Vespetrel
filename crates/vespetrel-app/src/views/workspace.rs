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

#[cfg(feature = "gpui")]
mod gpui_impl {
    use super::*;
    use gpui_kit::component::resizable::h_resizable;
    use gpui_kit::gpui::*;

    impl Render for WorkspaceView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            h_resizable("workspace-panels")
                .child(
                    div()
                        .w(px(220.0))
                        .h_full()
                        .child("Navigation Pane")
                        .into_any_element(),
                )
                .child(
                    div()
                        .w(px(350.0))
                        .h_full()
                        .child("Message List")
                        .into_any_element(),
                )
                .child(
                    div()
                        .flex_1()
                        .h_full()
                        .child("Message Reader")
                        .into_any_element(),
                )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_view_panes_and_description() {
        let state = AppState::new();
        let mut ws = WorkspaceView::new(state);
        assert_eq!(ws.panes, [0.22, 0.38, 0.40]);
        ws.set_panes([0.20, 0.40, 0.40]);
        assert_eq!(ws.panes, [0.20, 0.40, 0.40]);
        let desc = ws.description();
        assert!(desc.contains("Workspace left=20% center=40% right=40%"));
    }
}
