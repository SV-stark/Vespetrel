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

    /// Fast filtering of message summaries with SIMD scanning, diacritic-folding, and regex support
    pub fn filter_messages<'a>(&self, messages: &'a [MessageSummary]) -> Vec<&'a MessageSummary> {
        if !self.is_active() {
            return messages.iter().collect();
        }

        let query = self.search_query.trim();
        let (is_regex_pattern, regex_str) =
            if query.starts_with('/') && query.ends_with('/') && query.len() >= 2 {
                (true, &query[1..query.len() - 1])
            } else {
                (false, query)
            };

        let regex_matcher = if is_regex_pattern && !regex_str.is_empty() {
            regex::RegexBuilder::new(regex_str)
                .case_insensitive(true)
                .build()
                .ok()
        } else {
            None
        };

        let folded_query = fold_diacritics(query).to_lowercase();
        let query_bytes = folded_query.as_bytes();

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
                    if let Some(ref re) = regex_matcher {
                        let match_subject = m.subject.as_deref().is_some_and(|s| re.is_match(s));
                        let match_sender = re.is_match(&m.from_address)
                            || m.from_name.as_deref().is_some_and(|n| re.is_match(n));
                        let match_snippet = m.snippet.as_deref().is_some_and(|s| re.is_match(s));

                        if !match_subject && !match_sender && !match_snippet {
                            return false;
                        }
                    } else {
                        let match_subject = m.subject.as_deref().is_some_and(|s| {
                            let folded = fold_diacritics(s).to_lowercase();
                            crate::views::message_list::contains_ignore_case_ascii(
                                folded.as_bytes(),
                                query_bytes,
                            )
                        });
                        let match_sender = {
                            let folded_addr = fold_diacritics(&m.from_address).to_lowercase();
                            crate::views::message_list::contains_ignore_case_ascii(
                                folded_addr.as_bytes(),
                                query_bytes,
                            ) || m.from_name.as_deref().is_some_and(|n| {
                                let folded_name = fold_diacritics(n).to_lowercase();
                                crate::views::message_list::contains_ignore_case_ascii(
                                    folded_name.as_bytes(),
                                    query_bytes,
                                )
                            })
                        };
                        let match_snippet = m.snippet.as_deref().is_some_and(|s| {
                            let folded_snip = fold_diacritics(s).to_lowercase();
                            crate::views::message_list::contains_ignore_case_ascii(
                                folded_snip.as_bytes(),
                                query_bytes,
                            )
                        });

                        if !match_subject && !match_sender && !match_snippet {
                            return false;
                        }
                    }
                }

                true
            })
            .collect()
    }
}

/// Convert international accented characters into normalized ASCII representations for search
pub fn fold_diacritics(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' | 'Á' | 'À' | 'Â' | 'Ä' | 'Ã'
            | 'Å' => out.push('a'),
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' | 'É' | 'È' | 'Ê' | 'Ë' => {
                out.push('e')
            }
            'í' | 'ì' | 'î' | 'ï' | 'ī' | 'ĭ' | 'į' | 'İ' | 'Í' | 'Ì' | 'Î' | 'Ï' => {
                out.push('i')
            }
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' | 'ŏ' | 'ő' | 'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ'
            | 'Ø' => out.push('o'),
            'ú' | 'ù' | 'û' | 'ü' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' | 'Ú' | 'Ù' | 'Û' | 'Ü' => {
                out.push('u')
            }
            'ñ' | 'ń' | 'ņ' | 'ň' | 'Ñ' | 'Ń' => out.push('n'),
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' | 'Ç' | 'Ć' => out.push('c'),
            'ß' => out.push_str("ss"),
            'æ' | 'Æ' => out.push_str("ae"),
            'œ' | 'Œ' => out.push_str("oe"),
            _ => out.push(c),
        }
    }
    out
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

    #[test]
    fn test_quick_filter_diacritics_and_regex() {
        let msg1 = MessageSummary {
            id: "1".into(),
            thread_id: Some("t1".into()),
            subject: Some("Café Rendezvous in München".into()),
            from_address: "pierre@cafe.fr".into(),
            from_name: Some("Pierre François".into()),
            snippet: Some("Let's meet at the café".into()),
            sent_at: Utc::now(),
            is_read: true,
            is_flagged: false,
            has_attachments: false,
        };

        let msg2 = MessageSummary {
            id: "2".into(),
            thread_id: Some("t2".into()),
            subject: Some("Invoice-2026-X".into()),
            from_address: "billing@acme.com".into(),
            from_name: None,
            snippet: Some("Receipt #8899".into()),
            sent_at: Utc::now(),
            is_read: true,
            is_flagged: false,
            has_attachments: true,
        };

        let list = vec![msg1, msg2];

        // 1. Accent-folded search: "cafe" matches "Café", "francois" matches "François"
        let mut filter = QuickFilterState {
            search_query: "cafe".into(),
            ..Default::default()
        };
        let res = filter.filter_messages(&list);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].id, "1");

        filter.search_query = "francois".into();
        assert_eq!(filter.filter_messages(&list).len(), 1);

        filter.search_query = "munchen".into();
        assert_eq!(filter.filter_messages(&list).len(), 1);

        // 2. Regex search: "/invoice-\d+/" matches "Invoice-2026-X"
        filter.search_query = r#"/invoice-\d+/"#.into();
        let regex_res = filter.filter_messages(&list);
        assert_eq!(regex_res.len(), 1);
        assert_eq!(regex_res[0].id, "2");
    }
}
