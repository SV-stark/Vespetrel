use arc_swap::ArcSwap;
use std::sync::Arc;
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

/// Lock-free, atomically swappable application state container for 120 FPS UI reads
#[derive(Debug)]
pub struct SharedAppState {
    inner: ArcSwap<AppState>,
}

impl SharedAppState {
    pub fn new(state: AppState) -> Self {
        Self {
            inner: ArcSwap::from_pointee(state),
        }
    }

    /// Fast lock-free load for rendering (wait-free, zero contention)
    pub fn load(&self) -> arc_swap::Guard<Arc<AppState>> {
        self.inner.load()
    }

    /// Atomically swap updated state into place
    pub fn store(&self, state: AppState) {
        self.inner.store(Arc::new(state));
    }

    /// Mutate state with an update closure and atomically swap
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut AppState),
    {
        let mut cloned = (**self.inner.load()).clone();
        f(&mut cloned);
        self.store(cloned);
    }
}

impl Default for SharedAppState {
    fn default() -> Self {
        Self::new(AppState::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_app_state_atomic_swap() {
        let shared = SharedAppState::default();
        assert_eq!(shared.load().messages.len(), 0);

        shared.update(|st| {
            st.search_query = "inbox".into();
        });

        assert_eq!(shared.load().search_query, "inbox");
    }
}
