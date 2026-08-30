//! Configurable Keybinding Engine (Thunderbird, Gmail, Vim Presets) §7 Phase 6
use ahash::AHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyAction {
    NextMessage,
    PrevMessage,
    Archive,
    Delete,
    MarkRead,
    MarkUnread,
    ToggleStar,
    Reply,
    ReplyAll,
    Forward,
    Compose,
    FocusSearch,
    SyncNow,
    ToggleNavigationPane,
    GoToInbox,
    GoToSent,
    GoToDrafts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyPreset {
    Thunderbird,
    Gmail,
    Vim,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindingMap {
    pub preset: KeyPreset,
    /// Maps key sequence (e.g. "j", "k", "g i", "Ctrl+r") to action
    pub bindings: AHashMap<String, KeyAction>,
}

impl KeybindingMap {
    pub fn new(preset: KeyPreset) -> Self {
        let mut map = Self {
            preset,
            bindings: AHashMap::new(),
        };
        map.load_preset(preset);
        map
    }

    pub fn load_preset(&mut self, preset: KeyPreset) {
        self.preset = preset;
        self.bindings.clear();

        match preset {
            KeyPreset::Thunderbird => {
                self.bindings.insert("f".into(), KeyAction::NextMessage);
                self.bindings.insert("b".into(), KeyAction::PrevMessage);
                self.bindings.insert("a".into(), KeyAction::Archive);
                self.bindings.insert("Delete".into(), KeyAction::Delete);
                self.bindings.insert("m".into(), KeyAction::MarkRead);
                self.bindings.insert("s".into(), KeyAction::ToggleStar);
                self.bindings.insert("Ctrl+r".into(), KeyAction::Reply);
                self.bindings
                    .insert("Ctrl+Shift+r".into(), KeyAction::ReplyAll);
                self.bindings.insert("Ctrl+l".into(), KeyAction::Forward);
                self.bindings.insert("Ctrl+n".into(), KeyAction::Compose);
                self.bindings
                    .insert("Ctrl+k".into(), KeyAction::FocusSearch);
                self.bindings.insert("F5".into(), KeyAction::SyncNow);
            }
            KeyPreset::Gmail => {
                self.bindings.insert("j".into(), KeyAction::NextMessage);
                self.bindings.insert("k".into(), KeyAction::PrevMessage);
                self.bindings.insert("e".into(), KeyAction::Archive);
                self.bindings.insert("#".into(), KeyAction::Delete);
                self.bindings.insert("I".into(), KeyAction::MarkRead);
                self.bindings.insert("U".into(), KeyAction::MarkUnread);
                self.bindings.insert("s".into(), KeyAction::ToggleStar);
                self.bindings.insert("r".into(), KeyAction::Reply);
                self.bindings.insert("a".into(), KeyAction::ReplyAll);
                self.bindings.insert("f".into(), KeyAction::Forward);
                self.bindings.insert("c".into(), KeyAction::Compose);
                self.bindings.insert("/".into(), KeyAction::FocusSearch);
                self.bindings.insert("g i".into(), KeyAction::GoToInbox);
                self.bindings.insert("g t".into(), KeyAction::GoToSent);
                self.bindings.insert("g d".into(), KeyAction::GoToDrafts);
            }
            KeyPreset::Vim => {
                self.bindings.insert("j".into(), KeyAction::NextMessage);
                self.bindings.insert("k".into(), KeyAction::PrevMessage);
                self.bindings.insert("d".into(), KeyAction::Delete);
                self.bindings.insert("y".into(), KeyAction::Archive);
                self.bindings.insert("r".into(), KeyAction::Reply);
                self.bindings.insert("R".into(), KeyAction::ReplyAll);
                self.bindings.insert("o".into(), KeyAction::Compose);
                self.bindings.insert("/".into(), KeyAction::FocusSearch);
                self.bindings
                    .insert("Ctrl+w".into(), KeyAction::ToggleNavigationPane);
            }
            KeyPreset::Custom => {}
        }
    }

    pub fn resolve(&self, key: &str) -> Option<KeyAction> {
        self.bindings.get(key).copied()
    }

    pub fn bind(&mut self, key: impl Into<String>, action: KeyAction) {
        self.preset = KeyPreset::Custom;
        self.bindings.insert(key.into(), action);
    }
}

impl Default for KeybindingMap {
    fn default() -> Self {
        Self::new(KeyPreset::Thunderbird)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keybinding_presets() {
        let tb = KeybindingMap::new(KeyPreset::Thunderbird);
        assert_eq!(tb.resolve("a"), Some(KeyAction::Archive));
        assert_eq!(tb.resolve("Ctrl+n"), Some(KeyAction::Compose));

        let gmail = KeybindingMap::new(KeyPreset::Gmail);
        assert_eq!(gmail.resolve("j"), Some(KeyAction::NextMessage));
        assert_eq!(gmail.resolve("k"), Some(KeyAction::PrevMessage));
        assert_eq!(gmail.resolve("g i"), Some(KeyAction::GoToInbox));

        let vim = KeybindingMap::new(KeyPreset::Vim);
        assert_eq!(vim.resolve("j"), Some(KeyAction::NextMessage));
        assert_eq!(vim.resolve("o"), Some(KeyAction::Compose));
    }

    #[test]
    fn test_custom_binding() {
        let mut map = KeybindingMap::new(KeyPreset::Gmail);
        map.bind("Space", KeyAction::NextMessage);
        assert_eq!(map.resolve("Space"), Some(KeyAction::NextMessage));
        assert_eq!(map.preset, KeyPreset::Custom);
    }
}
