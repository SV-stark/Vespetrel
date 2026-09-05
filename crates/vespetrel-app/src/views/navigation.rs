use vespetrel_core::{Folder, FolderRole};

pub struct NavigationTree {
    pub folders: Vec<Folder>,
    pub selected: Option<String>,
}

impl NavigationTree {
    pub fn new(folders: Vec<Folder>) -> Self {
        Self {
            folders,
            selected: None,
        }
    }

    pub fn accounts_grouped(&self) -> std::collections::HashMap<String, Vec<&Folder>> {
        let mut map: std::collections::HashMap<String, Vec<&Folder>> =
            std::collections::HashMap::new();
        for f in &self.folders {
            map.entry(f.account_id.clone()).or_default().push(f);
        }
        map
    }

    pub fn sorted_folders(&self) -> Vec<&Folder> {
        let mut v: Vec<&Folder> = self.folders.iter().collect();
        v.sort_by_key(|f| folder_sort_key(&f.role));
        v
    }

    pub fn folder_icon(role: &FolderRole) -> &'static str {
        match role {
            FolderRole::Inbox => "📥",
            FolderRole::Drafts => "📝",
            FolderRole::Sent => "📤",
            FolderRole::Archive => "📦",
            FolderRole::Junk => "🚫",
            FolderRole::Trash => "🗑️",
            FolderRole::Custom => "📁",
        }
    }
}

pub fn folder_sort_key(role: &FolderRole) -> u8 {
    match role {
        FolderRole::Inbox => 0,
        FolderRole::Drafts => 1,
        FolderRole::Sent => 2,
        FolderRole::Archive => 3,
        FolderRole::Junk => 4,
        FolderRole::Trash => 5,
        FolderRole::Custom => 10,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_navigation_tree_sorting_and_icons() {
        let f_trash = Folder::new("acc1", "t", "Trash", "Trash").with_role(FolderRole::Trash);
        let f_inbox = Folder::new("acc1", "i", "Inbox", "INBOX").with_role(FolderRole::Inbox);
        let f_sent = Folder::new("acc1", "s", "Sent", "Sent").with_role(FolderRole::Sent);

        let tree = NavigationTree::new(vec![f_trash, f_inbox, f_sent]);
        let sorted = tree.sorted_folders();
        assert_eq!(sorted[0].role, FolderRole::Inbox);
        assert_eq!(sorted[1].role, FolderRole::Sent);
        assert_eq!(sorted[2].role, FolderRole::Trash);
        assert_eq!(NavigationTree::folder_icon(&FolderRole::Inbox), "📥");
    }
}
