//! Configurable Keybinding Engine (Thunderbird, Gmail, Vim Presets) §7 Phase 6
//!
//! The map is the application's own keymap model: it is serialisable, it names
//! [`KeyAction`] values rather than GPUI internals, and it can be rendered in
//! Settings without a window. [`to_gpui_keystroke`] is the only place that knows
//! how one of these tokens becomes something GPUI's keymap parser accepts.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

impl KeyAction {
    /// Every action, in the order the Settings table lists them.
    pub const ALL: [KeyAction; 17] = [
        KeyAction::NextMessage,
        KeyAction::PrevMessage,
        KeyAction::Archive,
        KeyAction::Delete,
        KeyAction::MarkRead,
        KeyAction::MarkUnread,
        KeyAction::ToggleStar,
        KeyAction::Reply,
        KeyAction::ReplyAll,
        KeyAction::Forward,
        KeyAction::Compose,
        KeyAction::FocusSearch,
        KeyAction::SyncNow,
        KeyAction::ToggleNavigationPane,
        KeyAction::GoToInbox,
        KeyAction::GoToSent,
        KeyAction::GoToDrafts,
    ];

    /// The name shown for this action in Settings and the command palette.
    pub fn label(&self) -> &'static str {
        match self {
            KeyAction::NextMessage => "Next message",
            KeyAction::PrevMessage => "Previous message",
            KeyAction::Archive => "Archive",
            KeyAction::Delete => "Delete",
            KeyAction::MarkRead => "Mark as read",
            KeyAction::MarkUnread => "Mark as unread",
            KeyAction::ToggleStar => "Toggle star",
            KeyAction::Reply => "Reply",
            KeyAction::ReplyAll => "Reply all",
            KeyAction::Forward => "Forward",
            KeyAction::Compose => "Compose",
            KeyAction::FocusSearch => "Search",
            KeyAction::SyncNow => "Check mail",
            KeyAction::ToggleNavigationPane => "Toggle folder tree",
            KeyAction::GoToInbox => "Go to Inbox",
            KeyAction::GoToSent => "Go to Sent",
            KeyAction::GoToDrafts => "Go to Drafts",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyPreset {
    Thunderbird,
    Gmail,
    Vim,
    Custom,
}

/// Carry the stored preference into the live keymap.
///
/// `UserSettings::keymap_preset` is the persisted form, so a mapping has to exist
/// between it and this engine's preset. An unrecognised stored value would
/// otherwise leave the keymap empty, which reads to the user as "no shortcuts
/// work"; the default is used instead.
pub fn preset_from_settings(stored: vespetrel_core::KeymapPreset) -> KeyPreset {
    match stored {
        vespetrel_core::KeymapPreset::Gmail => KeyPreset::Gmail,
        vespetrel_core::KeymapPreset::Thunderbird => KeyPreset::Thunderbird,
        vespetrel_core::KeymapPreset::Vim => KeyPreset::Vim,
        vespetrel_core::KeymapPreset::Custom => KeyPreset::Custom,
    }
}

impl KeyPreset {
    pub const ALL: [KeyPreset; 4] = [
        KeyPreset::Thunderbird,
        KeyPreset::Gmail,
        KeyPreset::Vim,
        KeyPreset::Custom,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            KeyPreset::Thunderbird => "Thunderbird",
            KeyPreset::Gmail => "Gmail",
            KeyPreset::Vim => "Vim",
            KeyPreset::Custom => "Custom",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindingMap {
    pub preset: KeyPreset,
    /// Maps key sequence (e.g. "j", "k", "g i", "Ctrl+r") to action
    pub bindings: HashMap<String, KeyAction>,
}

impl KeybindingMap {
    pub fn new(preset: KeyPreset) -> Self {
        let mut map = Self {
            preset,
            bindings: HashMap::new(),
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
                self.bindings.insert("F5".into(), KeyAction::SyncNow);
            }
            KeyPreset::Vim => {
                self.bindings.insert("j".into(), KeyAction::NextMessage);
                self.bindings.insert("k".into(), KeyAction::PrevMessage);
                self.bindings.insert("d".into(), KeyAction::Delete);
                self.bindings.insert("y".into(), KeyAction::Archive);
                self.bindings.insert("s".into(), KeyAction::ToggleStar);
                self.bindings.insert("r".into(), KeyAction::Reply);
                self.bindings.insert("R".into(), KeyAction::ReplyAll);
                self.bindings.insert("o".into(), KeyAction::Compose);
                self.bindings.insert("g g".into(), KeyAction::GoToInbox);
                self.bindings.insert("/".into(), KeyAction::FocusSearch);
                self.bindings
                    .insert("Ctrl+w".into(), KeyAction::ToggleNavigationPane);
                self.bindings.insert("F5".into(), KeyAction::SyncNow);
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

    /// The bindings, sorted by the action's display name.
    ///
    /// A `HashMap` iterates in an arbitrary order, so rendering it directly makes
    /// the Settings shortcut table shuffle on every redraw.
    pub fn sorted_entries(&self) -> Vec<(String, KeyAction)> {
        let mut entries: Vec<(String, KeyAction)> =
            self.bindings.iter().map(|(k, v)| (k.clone(), *v)).collect();
        entries.sort_by(|a, b| a.1.label().cmp(b.1.label()).then_with(|| a.0.cmp(&b.0)));
        entries
    }
}

impl Default for KeybindingMap {
    fn default() -> Self {
        Self::new(KeyPreset::Gmail)
    }
}

/// Translate one `KeybindingMap` token into GPUI's keystroke syntax.
///
/// GPUI's `KeyBinding::new` panics on anything it cannot parse, so a user-editable
/// keymap must be validated before it reaches the parser. Every token the presets
/// and the Settings UI can produce is covered here; anything else returns `None`
/// and is reported to the user instead of being passed through.
pub fn to_gpui_keystroke(token: &str) -> Option<String> {
    // A chord is two or more keystrokes separated by whitespace; each part is
    // translated on its own and rejoined.
    let parts: Vec<&str> = token.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    let mut out: Vec<String> = Vec::with_capacity(parts.len());
    for part in parts {
        out.push(to_gpui_single(part)?);
    }
    Some(out.join(" "))
}

/// Translate a single keystroke token (no chords).
fn to_gpui_single(token: &str) -> Option<String> {
    let mut modifiers: Vec<&'static str> = Vec::new();
    let mut rest = token;

    // Peel modifier prefixes. `secondary` is the cross-platform "the key this
    // platform uses for a command": Command on macOS, Control elsewhere. A macOS
    // user expects Command-N for Compose, so the presets' `Ctrl+` must map here
    // rather than to a literal `ctrl-`.
    // `Ctrl+Shift+` has to be matched before `Ctrl+`, and it contributes two
    // modifiers, so it is peeled on its own rather than through the single-
    // modifier table below.
    if let Some(key_part) = rest
        .strip_prefix("Ctrl+Shift+")
        .or_else(|| rest.strip_prefix("Control+Shift+"))
    {
        rest = key_part;
        modifiers.push("secondary");
        modifiers.push("shift");
    }

    loop {
        let peeled = rest
            .strip_prefix("Ctrl+")
            .map(|r| (r, "secondary"))
            .or_else(|| rest.strip_prefix("Control+").map(|r| (r, "secondary")))
            .or_else(|| rest.strip_prefix("Shift+").map(|r| (r, "shift")))
            .or_else(|| rest.strip_prefix("Alt+").map(|r| (r, "alt")));

        match peeled {
            Some((key_part, modifier)) => {
                rest = key_part;
                if !modifiers.contains(&modifier) {
                    modifiers.push(modifier);
                }
                if rest.is_empty() {
                    // A trailing modifier with no key: not a keystroke.
                    return None;
                }
            }
            None => break,
        }
    }

    let key = normalize_key(rest)?;
    // `modifiers` holds literals and `key` comes from the closed tables below, so
    // both are `&'static str` and the joined result is the only allocation. This
    // runs during every Settings render, so leaking a name per call is not an
    // option.
    let mut parts: Vec<&str> = modifiers;
    parts.push(key);
    Some(parts.join("-"))
}

/// Map a single key name onto GPUI's spelling.
///
/// Returns a `&'static str` from the closed tables below, so no allocation depends
/// on user input. Anything not in those tables is `None`: GPUI's `KeyBinding::new`
/// panics on input it cannot parse, so an unrecognised key must be rejected by the
/// validator rather than forwarded.
fn normalize_key(raw: &str) -> Option<&'static str> {
    // Named keys, matched case-insensitively.
    let named = match raw.to_ascii_lowercase().as_str() {
        "delete" | "del" => Some("delete"),
        "backspace" => Some("backspace"),
        "space" => Some("space"),
        "enter" | "return" => Some("enter"),
        "escape" | "esc" => Some("escape"),
        "tab" => Some("tab"),
        "up" => Some("up"),
        "down" => Some("down"),
        "left" => Some("left"),
        "right" => Some("right"),
        "home" => Some("home"),
        "end" => Some("end"),
        "pageup" | "page_up" => Some("pageup"),
        "pagedown" | "page_down" => Some("pagedown"),
        // `f` alone is the letter F, not a malformed function key, so the digits
        // have to be present and non-empty. An empty parse here would reject every
        // single-letter binding that starts with F.
        other => match other.strip_prefix('f') {
            Some(digits) if !digits.is_empty() => {
                let n: u32 = digits.parse().ok()?;
                if (1..=12).contains(&n) {
                    Some(FUNCTION_KEYS[(n - 1) as usize])
                } else {
                    None
                }
            }
            _ => None,
        },
    };
    if let Some(k) = named {
        return Some(k);
    }

    // Punctuation that GPUI does not name: the physical key plus Shift.
    let shifted = match raw {
        "#" => Some("shift-3"),
        "!" => Some("shift-1"),
        "@" => Some("shift-2"),
        "$" => Some("shift-4"),
        "%" => Some("shift-5"),
        "^" => Some("shift-6"),
        "&" => Some("shift-7"),
        "*" => Some("shift-8"),
        "(" => Some("shift-9"),
        ")" => Some("shift-0"),
        "_" => Some("shift--"),
        "+" => Some("shift-="),
        _ => None,
    };
    if let Some(k) = shifted {
        return Some(k);
    }

    // Any other single printable ASCII punctuation key names itself: `/`, `.`,
    // `,`, `-`, `=`, `;`, `'`, `[`, `]`, `\`, `` ` ``. The Gmail preset binds `/`
    // for search, so these have to survive the translation.
    if let Some(k) = punctuation_key(raw) {
        return Some(k);
    }

    // A single unshifted ASCII letter or digit.
    let mut chars = raw.chars();
    let (Some(ch), None) = (chars.next(), chars.next()) else {
        return None;
    };
    match ch {
        'a'..='z' => Some(LOWER_KEYS[ch as usize - 'a' as usize]),
        '0'..='9' => Some(NUMBER_KEYS[ch as usize - '0' as usize]),
        // An upper-case ASCII letter already carries its own Shift in the token, and
        // GPUI reads `I` as Shift+i, so the name is the lower-case letter.
        'A'..='Z' => Some(LOWER_KEYS[ch as usize - 'A' as usize]),
        _ => None,
    }
}

const LOWER_KEYS: [&str; 26] = [
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
    "t", "u", "v", "w", "x", "y", "z",
];

const NUMBER_KEYS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

const FUNCTION_KEYS: [&str; 12] = [
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
];

/// The GPUI key name for one printable ASCII punctuation character.
///
/// The printable punctuation code points fall into four contiguous blocks that are
/// not adjacent, so each is listed with its own base rather than one shared offset.
/// A code point outside them returns `None`, so the caller rejects it instead of
/// forwarding an unexpected token to `KeyBinding::new`, which panics.
fn punctuation_key(raw: &str) -> Option<&'static str> {
    let mut chars = raw.chars();
    let (Some(ch), None) = (chars.next(), chars.next()) else {
        return None;
    };
    if !ch.is_ascii_punctuation() {
        return None;
    }
    Some(match ch as usize {
        0x21 => "!",
        0x22 => "\"",
        0x23 => "#",
        0x24 => "$",
        0x25 => "%",
        0x26 => "&",
        0x27 => "'",
        0x28 => "(",
        0x29 => ")",
        0x2A => "*",
        0x2B => "+",
        0x2C => ",",
        0x2D => "-",
        0x2E => ".",
        0x2F => "/",
        0x3A => ":",
        0x3B => ";",
        0x3C => "<",
        0x3D => "=",
        0x3E => ">",
        0x3F => "?",
        0x40 => "@",
        0x5B => "[",
        0x5C => "\\",
        0x5D => "]",
        0x5E => "^",
        0x5F => "_",
        0x60 => "`",
        0x7B => "{",
        0x7C => "|",
        0x7D => "}",
        0x7E => "~",
        _ => return None,
    })
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

    /// Every token any preset can produce must survive the translation, or pressing
    /// the shortcut panics inside GPUI's parser instead of doing nothing.
    #[test]
    fn every_preset_token_translates_to_a_gpui_keystroke() {
        for preset in [
            KeyPreset::Thunderbird,
            KeyPreset::Gmail,
            KeyPreset::Vim,
            KeyPreset::Custom,
        ] {
            let map = KeybindingMap::new(preset);
            for (token, _) in map.sorted_entries() {
                assert!(
                    to_gpui_keystroke(&token).is_some(),
                    "{preset:?} token {token:?} does not translate"
                );
            }
        }
    }

    #[test]
    fn ctrl_maps_to_the_platform_command_modifier() {
        assert_eq!(to_gpui_keystroke("Ctrl+n").as_deref(), Some("secondary-n"));
        assert_eq!(
            to_gpui_keystroke("Ctrl+Shift+r").as_deref(),
            Some("secondary-shift-r")
        );
        assert_eq!(to_gpui_keystroke("Alt+f").as_deref(), Some("alt-f"));
    }

    #[test]
    fn chords_and_shifted_and_named_keys_translate() {
        assert_eq!(to_gpui_keystroke("g i").as_deref(), Some("g i"));
        assert_eq!(to_gpui_keystroke("F5").as_deref(), Some("f5"));
        assert_eq!(to_gpui_keystroke("Delete").as_deref(), Some("delete"));
        assert_eq!(to_gpui_keystroke("/").as_deref(), Some("/"));
        // An upper-case letter already implies Shift in GPUI's notation.
        assert_eq!(to_gpui_keystroke("I").as_deref(), Some("i"));
        assert_eq!(to_gpui_keystroke("#").as_deref(), Some("shift-3"));
    }

    /// A user-editable keymap reaches `KeyBinding::new`, which panics on input it
    /// cannot parse. Anything unrecognised has to be rejected here instead.
    #[test]
    fn unrecognised_tokens_are_rejected_not_forwarded() {
        assert_eq!(to_gpui_keystroke(""), None);
        assert_eq!(to_gpui_keystroke("   "), None);
        assert_eq!(to_gpui_keystroke("Ctrl+"), None);
        assert_eq!(to_gpui_keystroke("F13"), None);
        assert_eq!(to_gpui_keystroke("Ctrl+F13"), None);
        assert_eq!(to_gpui_keystroke("ç"), None);
        assert_eq!(to_gpui_keystroke("Ctrl+<script>"), None);
        assert_eq!(to_gpui_keystroke("too many keys"), None);
    }

    /// Settings renders the table every redraw; an unordered map would shuffle.
    #[test]
    fn sorted_entries_are_ordered_by_action_name() {
        let map = KeybindingMap::new(KeyPreset::Gmail);
        let entries = map.sorted_entries();
        let labels: Vec<&str> = entries.iter().map(|(_, a)| a.label()).collect();
        let mut expected = labels.clone();
        expected.sort_unstable();
        assert_eq!(labels, expected);
        assert_eq!(entries.len(), map.bindings.len());
    }
}
