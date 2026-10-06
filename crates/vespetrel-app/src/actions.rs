//! Typed commands for the Vespetrel window.
//!
//! Every user-visible command is one `Action`, so a shortcut, a toolbar button and
//! a palette row all reach the same handler instead of three copies of the logic.
//! The binding itself lives in [`crate::keybindings`], which owns the keymap model;
//! this module only declares what each command *is*.
use gpui_kit::actions;

// The key context the window root declares. A binding whose predicate is
// `Some(WINDOW_CONTEXT)` only fires while focus is inside the window, which keeps a
// shortcut from firing behind a modal that has taken focus.
pub const WINDOW_CONTEXT: &str = "VespetrelWindow";

actions!(
    vespetrel,
    [
        // Message list movement
        NextMessage,
        PrevMessage,
        // Triage
        Archive,
        Delete,
        ToggleStar,
        ToggleRead,
        // Composing
        Compose,
        Reply,
        ReplyAll,
        Forward,
        SaveDraft,
        // Navigation
        FocusSearch,
        SyncNow,
        GoToInbox,
        GoToSent,
        GoToDrafts,
        // View state
        ToggleThreading,
        ToggleRemoteImages,
        CycleSortOrder,
        CycleRowDensity,
        ToggleFolderTree,
        ShowCommandPalette,
        ShowSettings,
        // Overlays
        DismissOverlay,
        SelectNextMessage,
        SelectPreviousMessage,
    ]
);

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::Action;

    /// Every declared Action must carry a distinct, namespaced registry name.
    ///
    /// `actions!` registers `namespace::Variant`, and GPUI panics at startup on a
    /// duplicate registration. Asserting the names here means a typo in the list is a
    /// test failure rather than a window that refuses to open, and it documents the
    /// spelling the command palette and the keymap refer to.
    #[test]
    fn declared_actions_have_distinct_namespaced_names() {
        let actions: Vec<Box<dyn Action>> = vec![
            Box::new(NextMessage),
            Box::new(PrevMessage),
            Box::new(Archive),
            Box::new(Delete),
            Box::new(ToggleStar),
            Box::new(ToggleRead),
            Box::new(Compose),
            Box::new(Reply),
            Box::new(ReplyAll),
            Box::new(Forward),
            Box::new(SaveDraft),
            Box::new(FocusSearch),
            Box::new(SyncNow),
            Box::new(GoToInbox),
            Box::new(GoToSent),
            Box::new(GoToDrafts),
            Box::new(ToggleThreading),
            Box::new(ToggleRemoteImages),
            Box::new(CycleSortOrder),
            Box::new(CycleRowDensity),
            Box::new(ToggleFolderTree),
            Box::new(ShowCommandPalette),
            Box::new(ShowSettings),
            Box::new(DismissOverlay),
            Box::new(SelectNextMessage),
            Box::new(SelectPreviousMessage),
        ];

        let mut names: Vec<String> = actions.iter().map(|a| a.name().to_string()).collect();
        let total = names.len();
        names.sort();
        names.dedup();

        assert_eq!(
            names.len(),
            total,
            "two Actions register the same name, which GPUI rejects at startup"
        );
        for name in &names {
            assert!(
                name.starts_with("vespetrel::"),
                "{name} is not in this module's namespace: {names:?}"
            );
        }
    }

    /// Every key context predicate the bindings use has to be parseable.
    ///
    /// `KeyBinding::new` panics on a context it cannot parse, so an unparseable
    /// context is caught here rather than at window creation.
    #[test]
    fn the_binding_context_parses() {
        assert!(gpui_kit::gpui::KeyBindingContextPredicate::parse(WINDOW_CONTEXT).is_ok());
    }
}
