//! Virtualized message list - 100k+ items, 120 FPS, dynamic row heights §6.1

use vespetrel_core::MessageSummary;

/// View-model for the virtual list. In gpui this wraps `gpui::VirtualList`.
pub struct MessageListView {
    pub messages: Vec<MessageSummary>,
    /// Virtualization window
    pub viewport_start: usize,
    pub viewport_len: usize,
}

impl MessageListView {
    pub fn new(messages: Vec<MessageSummary>) -> Self {
        Self { messages, viewport_start: 0, viewport_len: 50 }
    }

    pub fn set_viewport(&mut self, start: usize, len: usize) {
        self.viewport_start = start.min(self.messages.len());
        self.viewport_len = len;
    }

    pub fn visible(&self) -> &[MessageSummary] {
        let end = (self.viewport_start + self.viewport_len).min(self.messages.len());
        &self.messages[self.viewport_start..end]
    }

    pub fn handle_sync_event(&mut self, event: vespetrel_core::provider::SyncEvent) {
        match event {
            vespetrel_core::provider::SyncEvent::MessagesInserted(new_msgs) => {
                // Splice at 0 - newest first, instant GPU redraw via cx.notify() in gpui
                self.messages.splice(0..0, new_msgs);
            }
            vespetrel_core::provider::SyncEvent::MessageFlagsUpdated { id, is_read, is_flagged } => {
                if let Some(m) = self.messages.iter_mut().find(|m| m.id == id) {
                    m.is_read = is_read;
                    m.is_flagged = is_flagged;
                }
            }
            vespetrel_core::provider::SyncEvent::MessagesDeleted(ids) => {
                self.messages.retain(|m| !ids.contains(&m.id));
            }
            _ => {}
        }
    }

    /// For non-gpui builds: simulate tokio->gpui bridge pattern from spec §6.2
    pub fn spawn_bridge(mut self, mut rx: tokio::sync::mpsc::UnboundedReceiver<vespetrel_core::provider::SyncEvent>) -> tokio::task::JoinHandle<Self> {
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                self.handle_sync_event(ev);
            }
            self
        })
    }
}
