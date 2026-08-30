use vespetrel_core::{Folder, MessageSummary};

/// Application state shared between Tokio engine and UI
#[derive(Debug, Clone)]
pub struct AppState {
    pub accounts: Vec<vespetrel_core::Account>,
    pub folders: Vec<Folder>,
    pub messages: Vec<MessageSummary>,
    pub selected_folder: Option<String>,
    pub selected_message: Option<String>,
    pub search_query: String,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            accounts: Vec::new(),
            folders: Vec::new(),
            messages: Vec::new(),
            selected_folder: None,
            selected_message: None,
            search_query: String::new(),
        }
    }

    pub fn handle_sync_event(&mut self, event: vespetrel_core::provider::SyncEvent) {
        match event {
            vespetrel_core::provider::SyncEvent::MessagesInserted(new_msgs) => {
                // Prepend to virtual list (newest first) - splice 0..0 pattern from spec §6.2
                self.messages.splice(0..0, new_msgs);
            }
            vespetrel_core::provider::SyncEvent::MessageFlagsUpdated {
                id,
                is_read,
                is_flagged,
            } => {
                if let Some(msg) = self.messages.iter_mut().find(|m| m.id == id) {
                    msg.is_read = is_read;
                    msg.is_flagged = is_flagged;
                }
            }
            vespetrel_core::provider::SyncEvent::MessagesDeleted(ids) => {
                self.messages.retain(|m| !ids.contains(&m.id));
            }
            vespetrel_core::provider::SyncEvent::FolderListUpdated(remote_folders) => {
                self.folders = remote_folders
                    .into_iter()
                    .map(|rf| {
                        vespetrel_core::Folder::new("default", &rf.remote_id, &rf.name, &rf.path)
                    })
                    .collect();
            }
            _ => {}
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
