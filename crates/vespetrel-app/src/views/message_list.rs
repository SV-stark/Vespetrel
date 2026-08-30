use std::collections::HashSet;
use vespetrel_core::MessageSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListFilter {
    All,
    Unread,
    Flagged,
    WithAttachments,
}

/// View-model for the virtual list. In gpui this wraps `gpui::VirtualList`.
pub struct MessageListView {
    pub messages: Vec<MessageSummary>,
    /// Virtualization window
    pub viewport_start: usize,
    pub viewport_len: usize,
    pub filter: ListFilter,
    pub search_query: String,
    pub selected_ids: HashSet<String>,
}

impl MessageListView {
    pub fn new(messages: Vec<MessageSummary>) -> Self {
        Self {
            messages,
            viewport_start: 0,
            viewport_len: 50,
            filter: ListFilter::All,
            search_query: String::new(),
            selected_ids: HashSet::new(),
        }
    }

    pub fn set_viewport(&mut self, start: usize, len: usize) {
        self.viewport_start = start.min(self.messages.len());
        self.viewport_len = len;
    }

    pub fn set_filter(&mut self, filter: ListFilter) {
        self.filter = filter;
    }

    pub fn set_search(&mut self, query: impl Into<String>) {
        self.search_query = query.into().trim().to_lowercase();
    }

    pub fn filtered_messages(&self) -> Vec<&MessageSummary> {
        self.messages
            .iter()
            .filter(|m| {
                // Apply flag filter
                let flag_match = match self.filter {
                    ListFilter::All => true,
                    ListFilter::Unread => !m.is_read,
                    ListFilter::Flagged => m.is_flagged,
                    ListFilter::WithAttachments => m.has_attachments,
                };
                if !flag_match {
                    return false;
                }

                // Apply search filter
                if !self.search_query.is_empty() {
                    let subject_match = m
                        .subject
                        .as_deref()
                        .map(|s| s.to_lowercase().contains(&self.search_query))
                        .unwrap_or(false);
                    let from_match = m.from_address.to_lowercase().contains(&self.search_query)
                        || m.from_name
                            .as_deref()
                            .map(|n| n.to_lowercase().contains(&self.search_query))
                            .unwrap_or(false);
                    let snippet_match = m
                        .snippet
                        .as_deref()
                        .map(|s| s.to_lowercase().contains(&self.search_query))
                        .unwrap_or(false);

                    subject_match || from_match || snippet_match
                } else {
                    true
                }
            })
            .collect()
    }

    pub fn visible(&self) -> Vec<&MessageSummary> {
        let filtered = self.filtered_messages();
        let end = (self.viewport_start + self.viewport_len).min(filtered.len());
        if self.viewport_start >= filtered.len() {
            Vec::new()
        } else {
            filtered[self.viewport_start..end].to_vec()
        }
    }

    pub fn toggle_selection(&mut self, id: &str) {
        if self.selected_ids.contains(id) {
            self.selected_ids.remove(id);
        } else {
            self.selected_ids.insert(id.to_string());
        }
    }

    pub fn select_all(&mut self) {
        let ids: Vec<String> = self
            .filtered_messages()
            .iter()
            .map(|m| m.id.clone())
            .collect();
        self.selected_ids.extend(ids);
    }

    pub fn clear_selection(&mut self) {
        self.selected_ids.clear();
    }

    pub fn handle_sync_event(&mut self, event: vespetrel_core::provider::SyncEvent) {
        match event {
            vespetrel_core::provider::SyncEvent::MessagesInserted(new_msgs) => {
                // Splice at 0 - newest first, instant GPU redraw via cx.notify() in gpui
                self.messages.splice(0..0, new_msgs);
            }
            vespetrel_core::provider::SyncEvent::MessageFlagsUpdated {
                id,
                is_read,
                is_flagged,
            } => {
                if let Some(m) = self.messages.iter_mut().find(|m| m.id == id) {
                    m.is_read = is_read;
                    m.is_flagged = is_flagged;
                }
            }
            vespetrel_core::provider::SyncEvent::MessagesDeleted(ids) => {
                self.messages.retain(|m| !ids.contains(&m.id));
                self.selected_ids.retain(|id| !ids.contains(id));
            }
            _ => {}
        }
    }

    /// For non-gpui builds: simulate tokio->gpui bridge pattern from spec §6.2
    pub fn spawn_bridge(
        mut self,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<vespetrel_core::provider::SyncEvent>,
    ) -> tokio::task::JoinHandle<Self> {
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                self.handle_sync_event(ev);
            }
            self
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_message_list_filtering_and_selection() {
        let m1 = MessageSummary {
            id: "m1".into(),
            thread_id: None,
            subject: Some("Rust 2024 update".into()),
            from_address: "alice@example.com".into(),
            from_name: Some("Alice".into()),
            snippet: Some("Great progress".into()),
            sent_at: Utc::now(),
            is_read: false,
            is_flagged: true,
            has_attachments: false,
        };
        let m2 = MessageSummary {
            id: "m2".into(),
            thread_id: None,
            subject: Some("Invoice Attached".into()),
            from_address: "billing@example.com".into(),
            from_name: None,
            snippet: Some("Please review".into()),
            sent_at: Utc::now(),
            is_read: true,
            is_flagged: false,
            has_attachments: true,
        };

        let mut view = MessageListView::new(vec![m1, m2]);
        assert_eq!(view.filtered_messages().len(), 2);

        view.set_filter(ListFilter::Unread);
        assert_eq!(view.filtered_messages().len(), 1);
        assert_eq!(view.filtered_messages()[0].id, "m1");

        view.set_filter(ListFilter::WithAttachments);
        assert_eq!(view.filtered_messages().len(), 1);
        assert_eq!(view.filtered_messages()[0].id, "m2");

        view.set_filter(ListFilter::All);
        view.set_search("invoice");
        assert_eq!(view.filtered_messages().len(), 1);
        assert_eq!(view.filtered_messages()[0].id, "m2");

        view.set_search("");
        view.toggle_selection("m1");
        assert!(view.selected_ids.contains("m1"));
        view.toggle_selection("m1");
        assert!(!view.selected_ids.contains("m1"));
    }
}
