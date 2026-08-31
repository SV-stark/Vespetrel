//! Thunderbird-style Quick Filter Bar Toolbar & Filter Predicates §4 Phase 5
use serde::{Deserialize, Serialize};
use vespetrel_core::MessageSummary;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuickFilterState {
    pub unread_only: bool,
    pub starred_only: bool,
    pub has_attachment_only: bool,
    pub tag_filter: Option<String>,
    pub search_query: String,
}

impl QuickFilterState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        self.unread_only
            || self.starred_only
            || self.has_attachment_only
            || self.tag_filter.is_some()
            || !self.search_query.trim().is_empty()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Fast filtering of message summaries using SIMD vector scanning
    pub fn filter_messages<'a>(&self, messages: &'a [MessageSummary]) -> Vec<&'a MessageSummary> {
        if !self.is_active() {
            return messages.iter().collect();
        }

        let query = self.search_query.trim();
        let query_bytes = query.as_bytes();

        messages
            .iter()
            .filter(|m| {
                if self.unread_only && m.is_read {
                    return false;
                }
                if self.starred_only && !m.is_flagged {
                    return false;
                }
                if self.has_attachment_only && !m.has_attachments {
                    return false;
                }

                if !query.is_empty() {
                    let match_subject = m.subject.as_deref().is_some_and(|s| {
                        crate::views::message_list::contains_ignore_case_ascii(
                            s.as_bytes(),
                            query_bytes,
                        )
                    });
                    let match_sender = crate::views::message_list::contains_ignore_case_ascii(
                        m.from_address.as_bytes(),
                        query_bytes,
                    ) || m.from_name.as_deref().is_some_and(|n| {
                        crate::views::message_list::contains_ignore_case_ascii(
                            n.as_bytes(),
                            query_bytes,
                        )
                    });
                    let match_snippet = m.snippet.as_deref().is_some_and(|s| {
                        crate::views::message_list::contains_ignore_case_ascii(
                            s.as_bytes(),
                            query_bytes,
                        )
                    });

                    if !match_subject && !match_sender && !match_snippet {
                        return false;
                    }
                }

                true
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_quick_filter_predicates() {
        let msg1 = MessageSummary {
            id: "1".into(),
            thread_id: Some("t1".into()),
            subject: Some("Invoice from Vendor".into()),
            from_address: "vendor@sales.com".into(),
            from_name: Some("Vendor Inc".into()),
            snippet: Some("Please find attached invoice".into()),
            sent_at: Utc::now(),
            is_read: false,
            is_flagged: true,
            has_attachments: true,
        };

        let msg2 = MessageSummary {
            id: "2".into(),
            thread_id: Some("t2".into()),
            subject: Some("Weekly Newsletter".into()),
            from_address: "news@daily.com".into(),
            from_name: None,
            snippet: Some("Top stories today".into()),
            sent_at: Utc::now(),
            is_read: true,
            is_flagged: false,
            has_attachments: false,
        };

        let list = vec![msg1, msg2];

        // 1. Unread only
        let mut filter = QuickFilterState {
            unread_only: true,
            ..Default::default()
        };
        let res = filter.filter_messages(&list);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].id, "1");

        // 2. Starred only
        filter.unread_only = false;
        filter.starred_only = true;
        let res = filter.filter_messages(&list);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].id, "1");

        // 3. Search query
        filter.starred_only = false;
        filter.search_query = "newsletter".into();
        let res = filter.filter_messages(&list);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].id, "2");

        // 4. Clear filter
        filter.clear();
        assert_eq!(filter.filter_messages(&list).len(), 2);
    }
}
