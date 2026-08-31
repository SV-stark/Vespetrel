//! Color-coded Message Tags and Label System compatible with Thunderbird §4 Phase 5
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageTag {
    pub id: String,
    pub name: String,
    pub color_hex: String,
    pub shortcut_key: Option<char>, // e.g. '1'..'5'
    pub ordinal: usize,
}

impl MessageTag {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        color_hex: impl Into<String>,
        shortcut_key: Option<char>,
        ordinal: usize,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            color_hex: color_hex.into(),
            shortcut_key,
            ordinal,
        }
    }
}

/// Predefined standard Thunderbird tags ($label1..$label5)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagStore {
    pub tags: Vec<MessageTag>,
}

impl TagStore {
    pub fn standard_thunderbird_tags() -> Self {
        Self {
            tags: vec![
                MessageTag::new("$label1", "Important", "#ef4444", Some('1'), 1), // Red
                MessageTag::new("$label2", "Work", "#f59e0b", Some('2'), 2),      // Amber
                MessageTag::new("$label3", "Personal", "#10b981", Some('3'), 3),  // Emerald
                MessageTag::new("$label4", "To Do", "#3b82f6", Some('4'), 4),     // Blue
                MessageTag::new("$label5", "Later", "#8b5cf6", Some('5'), 5),     // Purple
            ],
        }
    }

    pub fn add_tag(&mut self, tag: MessageTag) {
        if !self.tags.iter().any(|t| t.id == tag.id) {
            self.tags.push(tag);
        }
    }

    pub fn get_by_id(&self, id: &str) -> Option<&MessageTag> {
        self.tags.iter().find(|t| t.id == id)
    }

    pub fn get_by_shortcut(&self, key: char) -> Option<&MessageTag> {
        self.tags.iter().find(|t| t.shortcut_key == Some(key))
    }
}

impl Default for TagStore {
    fn default() -> Self {
        Self::standard_thunderbird_tags()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_tags_and_shortcuts() {
        let store = TagStore::default();
        assert_eq!(store.tags.len(), 5);

        let important = store.get_by_id("$label1").unwrap();
        assert_eq!(important.name, "Important");
        assert_eq!(important.color_hex, "#ef4444");

        let tag2 = store.get_by_shortcut('2').unwrap();
        assert_eq!(tag2.name, "Work");
    }

    #[test]
    fn test_custom_tag_addition() {
        let mut store = TagStore::default();
        store.add_tag(MessageTag::new(
            "receipts",
            "Receipts",
            "#06b6d4",
            Some('6'),
            6,
        ));

        assert_eq!(store.tags.len(), 6);
        let custom = store.get_by_shortcut('6').unwrap();
        assert_eq!(custom.name, "Receipts");
    }
}
