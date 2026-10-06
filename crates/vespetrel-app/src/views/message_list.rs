use std::collections::HashSet;
use vespetrel_core::MessageSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListFilter {
    All,
    Unread,
    Flagged,
    WithAttachments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum MessageRowDensity {
    /// 1-line tabular (Sender, Subject, Date)
    Compact,
    /// 2-line modern card (Line 1: Sender + Date, Line 2: Subject + Snippet)
    #[default]
    Comfortable,
    /// 3-line rich preview (Line 1: Sender + Badges + Date, Line 2: Subject, Line 3: Multi-line Snippet)
    Roomy,
}

impl MessageRowDensity {
    pub fn row_height_px(&self) -> f32 {
        match self {
            Self::Compact => 32.0,
            Self::Comfortable => 64.0,
            Self::Roomy => 96.0,
        }
    }
}

/// The message the reader is showing, plus the range of rows to act on.
///
/// The reader needs one "current" message and a bulk action needs "every selected
/// message", and they are usually not the same set. Modelling selection as a
/// separate set keeps the current row readable while a range operation has a
/// defined target, instead of overloading one index to mean both.
#[derive(Debug, Clone, Default)]
pub struct MessageSelection {
    current: Option<String>,
    selected: HashSet<String>,
}

impl MessageSelection {
    pub fn new() -> Self {
        Self::default()
    }

    /// The row the reader is showing, if any.
    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    pub fn set_current(&mut self, id: impl Into<String>) {
        self.current = Some(id.into());
    }

    pub fn clear_current(&mut self) {
        self.current = None;
    }

    pub fn is_selected(&self, id: &str) -> bool {
        self.selected.contains(id)
    }

    pub fn toggle(&mut self, id: &str) {
        if !self.selected.remove(id) {
            self.selected.insert(id.to_string());
        }
    }

    pub fn extend(&mut self, ids: impl IntoIterator<Item = String>) {
        self.selected.extend(ids);
    }

    pub fn clear(&mut self) {
        self.selected.clear();
    }

    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }

    /// The ids a bulk action should apply to.
    ///
    /// With nothing explicitly selected this is the current message, so a keystroke
    /// always does something sensible. With a range selected it is that range, so
    /// Shift-click then Archive acts on what the user highlighted.
    pub fn target_ids(&self) -> Vec<String> {
        if self.selected.is_empty() {
            self.current.iter().cloned().collect()
        } else {
            self.selected.iter().cloned().collect()
        }
    }

    /// Drop ids that are no longer present.
    ///
    /// Sync delivers deletions asynchronously, so a selection can outlive its
    /// message. Acting on a stale id would report a failure the user cannot
    /// explain, so the range is trimmed on every load.
    pub fn retain(&mut self, present: &HashSet<String>) {
        self.selected.retain(|id| present.contains(id));
        if self
            .current
            .as_ref()
            .is_some_and(|id| !present.contains(id))
        {
            self.current = None;
        }
    }
}

/// View-model for the virtual list. In gpui this wraps `gpui::VirtualList`.
pub struct MessageListView {
    pub messages: Vec<MessageSummary>,
    /// Virtualization window
    pub viewport_start: usize,
    pub viewport_len: usize,
    pub filter: ListFilter,
    pub density: MessageRowDensity,
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
            density: MessageRowDensity::Comfortable,
            search_query: String::new(),
            selected_ids: HashSet::new(),
        }
    }

    pub fn set_density(&mut self, density: MessageRowDensity) {
        self.density = density;
    }

    pub fn set_viewport(&mut self, start: usize, len: usize) {
        self.viewport_start = start.min(self.messages.len());
        self.viewport_len = len;
    }

    pub fn set_filter(&mut self, filter: ListFilter) {
        self.filter = filter;
    }

    pub fn set_search(&mut self, query: impl Into<String>) {
        self.search_query = query.into().trim().to_string();
    }

    pub fn filtered_messages(&self) -> Vec<&MessageSummary> {
        let query_bytes = self.search_query.as_bytes();
        let is_empty_query = query_bytes.is_empty();

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

                // Apply zero-allocation case-insensitive search filter
                if !is_empty_query {
                    let subject_match = m
                        .subject
                        .as_deref()
                        .map(|s| contains_ignore_case_ascii(s.as_bytes(), query_bytes))
                        .unwrap_or(false);
                    let from_match =
                        contains_ignore_case_ascii(m.from_address.as_bytes(), query_bytes)
                            || m.from_name
                                .as_deref()
                                .map(|n| contains_ignore_case_ascii(n.as_bytes(), query_bytes))
                                .unwrap_or(false);
                    let snippet_match = m
                        .snippet
                        .as_deref()
                        .map(|s| contains_ignore_case_ascii(s.as_bytes(), query_bytes))
                        .unwrap_or(false);

                    subject_match || from_match || snippet_match
                } else {
                    true
                }
            })
            .collect()
    }

    /// Computes cumulative row height prefix sums for virtual list scrollbar mapping
    pub fn calculate_row_prefix_sums(&self, default_row_height: f32) -> Vec<f32> {
        let filtered = self.filtered_messages();
        let mut prefix_sums = Vec::with_capacity(filtered.len() + 1);
        prefix_sums.push(0.0);
        let mut acc = 0.0;
        for _ in filtered {
            acc += default_row_height;
            prefix_sums.push(acc);
        }
        prefix_sums
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

    #[cfg(feature = "gpui")]
    pub fn virtual_item_sizes(
        &self,
        count: usize,
    ) -> std::rc::Rc<Vec<gpui_kit::gpui::Size<gpui_kit::gpui::Pixels>>> {
        let h = gpui_kit::gpui::px(self.density.row_height_px());
        let sizes: Vec<gpui_kit::gpui::Size<gpui_kit::gpui::Pixels>> = (0..count)
            .map(|_| gpui_kit::gpui::size(gpui_kit::gpui::px(350.0), h))
            .collect();
        std::rc::Rc::new(sizes)
    }
}

/// Zero-allocation case-insensitive ASCII substring search using sliding window
#[inline]
pub fn contains_ignore_case_ascii(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }

    let first_lower = needle[0].to_ascii_lowercase();
    let first_upper = needle[0].to_ascii_uppercase();

    // Use memchr2 to SIMD fast-skip to first matching character
    let mut offset = 0;
    while offset + needle.len() <= haystack.len() {
        let remaining = &haystack[offset..];
        let found = memchr::memchr2(first_lower, first_upper, remaining);
        match found {
            Some(idx) => {
                let check_start = offset + idx;
                if check_start + needle.len() > haystack.len() {
                    return false;
                }
                let candidate = &haystack[check_start..check_start + needle.len()];
                if candidate.eq_ignore_ascii_case(needle) {
                    return true;
                }
                offset = check_start + 1;
            }
            None => return false,
        }
    }
    false
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

        // Density settings
        assert_eq!(view.density, MessageRowDensity::Comfortable);
        assert_eq!(view.density.row_height_px(), 64.0);

        view.set_density(MessageRowDensity::Compact);
        assert_eq!(view.density.row_height_px(), 32.0);

        view.set_density(MessageRowDensity::Roomy);
        assert_eq!(view.density.row_height_px(), 96.0);
    }
}
