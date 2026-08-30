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
}

fn folder_sort_key(role: &FolderRole) -> u8 {
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
