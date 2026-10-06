//! Smart Virtual Folders & Cross-Account Unified Inboxes §7 Phase 5
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VirtualFolderType {
    /// All Inboxes across all active accounts
    UnifiedInbox,
    /// All Flagged / Starred messages across all accounts
    UnifiedFlagged,
    /// All Unread messages across all accounts
    UnifiedUnread,
    /// Messages received within the last 24 hours
    Today,
    /// Messages with attachments across all accounts
    HasAttachments,
    /// Saved search query with dynamic parameters
    SavedSearch { query: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualFolder {
    pub id: String,
    pub name: String,
    pub folder_type: VirtualFolderType,
    pub icon: String,
}

impl VirtualFolder {
    pub fn unified_inbox() -> Self {
        Self {
            id: "virtual:inbox".into(),
            name: "All Inboxes".into(),
            folder_type: VirtualFolderType::UnifiedInbox,
            icon: "inbox".into(),
        }
    }

    pub fn unified_flagged() -> Self {
        Self {
            id: "virtual:flagged".into(),
            name: "Flagged".into(),
            folder_type: VirtualFolderType::UnifiedFlagged,
            icon: "star".into(),
        }
    }

    pub fn unified_unread() -> Self {
        Self {
            id: "virtual:unread".into(),
            name: "Unread".into(),
            folder_type: VirtualFolderType::UnifiedUnread,
            icon: "mail-unread".into(),
        }
    }

    pub fn today() -> Self {
        Self {
            id: "virtual:today".into(),
            name: "Today".into(),
            folder_type: VirtualFolderType::Today,
            icon: "calendar".into(),
        }
    }

    pub fn saved_search(name: impl Into<String>, query: impl Into<String>) -> Self {
        let q = query.into();
        Self {
            id: format!("virtual:search:{}", uuid::Uuid::new_v4()),
            name: name.into(),
            folder_type: VirtualFolderType::SavedSearch { query: q },
            icon: "search".into(),
        }
    }

    /// Generate SQL `WHERE` clause fragment for this virtual folder
    pub fn to_sql_filter(&self, now_timestamp: i64) -> String {
        match &self.folder_type {
            VirtualFolderType::UnifiedInbox => {
                "folder_id IN (SELECT id FROM folders WHERE role = 'inbox')".to_string()
            }
            VirtualFolderType::UnifiedFlagged => "is_flagged = 1".to_string(),
            VirtualFolderType::UnifiedUnread => "is_read = 0".to_string(),
            VirtualFolderType::Today => {
                let yesterday = now_timestamp - 86400;
                format!("received_at >= {yesterday}")
            }
            VirtualFolderType::HasAttachments => "has_attachments = 1".to_string(),
            VirtualFolderType::SavedSearch { query } => crate::search::fts5_predicate(query),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_folder_definitions_and_sql() {
        let inbox = VirtualFolder::unified_inbox();
        assert_eq!(inbox.name, "All Inboxes");
        assert!(inbox.to_sql_filter(100000).contains("role = 'inbox'"));

        let flagged = VirtualFolder::unified_flagged();
        assert_eq!(flagged.to_sql_filter(100000), "is_flagged = 1");

        let today = VirtualFolder::today();
        assert_eq!(today.to_sql_filter(100000), "received_at >= 13600");
    }

    #[test]
    fn saved_search_supports_field_prefixes_and_never_wildcards() {
        // The old local sanitizer ignored `from:`, so a sender search was run as a literal
        // phrase and returned nothing.
        let saved = VirtualFolder::saved_search("From Alice", "from:alice@example.com");
        assert!(saved.to_sql_filter(0).contains('?'));

        // An empty query must not become `MATCH '"*"'`, which FTS5 rejects as a syntax error.
        for query in ["", "   ", "*", "^ ()"] {
            let filter = VirtualFolder::saved_search("Empty", query).to_sql_filter(0);
            assert_eq!(filter, "0", "query {query:?} produced {filter}");
            assert!(!filter.contains('*'));
        }
    }
}
