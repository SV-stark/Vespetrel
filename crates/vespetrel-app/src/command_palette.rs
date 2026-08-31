//! Command Palette (Ctrl+K / Cmd+K Superhuman Action Launcher) §7 Phase 7
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionCategory {
    Navigation,
    Mail,
    Edit,
    Settings,
    View,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaletteAction {
    pub id: String,
    pub title: String,
    pub shortcut: Option<String>,
    pub category: ActionCategory,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandPalette {
    pub is_open: bool,
    pub query: String,
    pub selected_index: usize,
    pub actions: Vec<PaletteAction>,
}

impl CommandPalette {
    pub fn new() -> Self {
        let mut palette = Self {
            is_open: false,
            query: String::new(),
            selected_index: 0,
            actions: Vec::new(),
        };
        palette.register_default_actions();
        palette
    }

    pub fn open(&mut self) {
        self.is_open = true;
        self.query.clear();
        self.selected_index = 0;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.query.clear();
        self.selected_index = 0;
    }

    pub fn register_action(&mut self, action: PaletteAction) {
        self.actions.push(action);
    }

    pub fn register_default_actions(&mut self) {
        self.actions = vec![
            PaletteAction {
                id: "mail.compose".into(),
                title: "Compose New Message".into(),
                shortcut: Some("c".into()),
                category: ActionCategory::Mail,
                keywords: vec!["write".into(), "new".into(), "email".into()],
            },
            PaletteAction {
                id: "nav.inbox".into(),
                title: "Go to Inbox".into(),
                shortcut: Some("g i".into()),
                category: ActionCategory::Navigation,
                keywords: vec!["home".into(), "messages".into()],
            },
            PaletteAction {
                id: "nav.flagged".into(),
                title: "Go to Starred / Flagged".into(),
                shortcut: Some("g s".into()),
                category: ActionCategory::Navigation,
                keywords: vec!["starred".into(), "important".into()],
            },
            PaletteAction {
                id: "mail.sync".into(),
                title: "Sync All Accounts Now".into(),
                shortcut: Some("Ctrl+R".into()),
                category: ActionCategory::Mail,
                keywords: vec!["refresh".into(), "fetch".into()],
            },
            PaletteAction {
                id: "view.calendar".into(),
                title: "Switch to Calendar View".into(),
                shortcut: Some("Alt+2".into()),
                category: ActionCategory::View,
                keywords: vec!["cal".into(), "events".into(), "meetings".into()],
            },
            PaletteAction {
                id: "view.contacts".into(),
                title: "Switch to Contacts View".into(),
                shortcut: Some("Alt+3".into()),
                category: ActionCategory::View,
                keywords: vec!["address book".into(), "people".into()],
            },
            PaletteAction {
                id: "view.tasks".into(),
                title: "Switch to Tasks View".into(),
                shortcut: Some("Alt+4".into()),
                category: ActionCategory::View,
                keywords: vec!["todo".into(), "reminders".into()],
            },
            PaletteAction {
                id: "settings.keybindings".into(),
                title: "Configure Keyboard Shortcuts".into(),
                shortcut: Some("Ctrl+,".into()),
                category: ActionCategory::Settings,
                keywords: vec!["keys".into(), "vim".into(), "gmail".into()],
            },
        ];
    }

    /// Filter actions matching query case-insensitively across titles and keywords
    pub fn filtered_actions(&self) -> Vec<&PaletteAction> {
        let q = self.query.trim().to_lowercase();
        if q.is_empty() {
            return self.actions.iter().collect();
        }

        self.actions
            .iter()
            .filter(|a| {
                a.title.to_lowercase().contains(&q)
                    || a.keywords.iter().any(|k| k.to_lowercase().contains(&q))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_palette_filtering() {
        let mut palette = CommandPalette::new();
        palette.open();
        assert!(palette.is_open);

        palette.query = "calendar".into();
        let filtered = palette.filtered_actions();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "view.calendar");

        palette.query = "sync".into();
        let filtered = palette.filtered_actions();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "mail.sync");

        palette.close();
        assert!(!palette.is_open);
    }
}
