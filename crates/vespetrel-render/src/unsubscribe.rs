//! 1-Click List-Unsubscribe & Newsletter Unsubscribe Parser (RFC 2369 / RFC 8058) §7 Phase 7
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnsubscribeAction {
    /// RFC 8058 One-Click HTTP POST request
    OneClickPost { url: String },
    /// Standard Web landing page to visit
    WebUrl { url: String },
    /// Mailto email to dispatch
    Mailto {
        email: String,
        subject: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListUnsubscribe {
    pub actions: Vec<UnsubscribeAction>,
    pub is_one_click: bool,
}

impl ListUnsubscribe {
    /// Parse RFC 2369 `List-Unsubscribe` and RFC 8058 `List-Unsubscribe-Post` headers
    pub fn parse(list_unsub_header: &str, unsub_post_header: Option<&str>) -> Option<Self> {
        let is_one_click = unsub_post_header
            .map(|h| {
                let clean: String = h.chars().filter(|c| !c.is_whitespace()).collect();
                clean.eq_ignore_ascii_case("list-unsubscribe=one-click")
            })
            .unwrap_or(false);

        let mut actions = Vec::new();

        for part in list_unsub_header.split(',') {
            let trimmed = part.trim().trim_matches('<').trim_matches('>');
            if trimmed.starts_with("https://") {
                if is_one_click {
                    actions.push(UnsubscribeAction::OneClickPost {
                        url: trimmed.to_string(),
                    });
                } else {
                    actions.push(UnsubscribeAction::WebUrl {
                        url: trimmed.to_string(),
                    });
                }
            } else if trimmed.starts_with("http://") {
                // RFC 8058 §3.1: The POST method MUST use the HTTPS scheme.
                actions.push(UnsubscribeAction::WebUrl {
                    url: trimmed.to_string(),
                });
            } else if let Some(mailto_str) = trimmed.strip_prefix("mailto:") {
                let (email, subject) = if let Some((e, query)) = mailto_str.split_once('?') {
                    let mut subj = None;
                    for q in query.split('&') {
                        if let Some(s) = q.strip_prefix("subject=") {
                            subj = Some(s.to_string());
                        }
                    }
                    (e.to_string(), subj)
                } else {
                    (mailto_str.to_string(), None)
                };

                actions.push(UnsubscribeAction::Mailto { email, subject });
            }
        }

        if actions.is_empty() {
            None
        } else {
            Some(Self {
                actions,
                is_one_click,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_list_unsubscribe_headers() {
        let header =
            "<mailto:unsub@example.com?subject=unsubscribe>, <https://example.com/unsub?id=123>";
        let unsub = ListUnsubscribe::parse(header, Some("List-Unsubscribe=One-Click")).unwrap();
        assert!(unsub.is_one_click);
        assert_eq!(unsub.actions.len(), 2);

        match &unsub.actions[0] {
            UnsubscribeAction::Mailto { email, subject } => {
                assert_eq!(email, "unsub@example.com");
                assert_eq!(subject.as_deref(), Some("unsubscribe"));
            }
            _ => panic!("Expected mailto action"),
        }

        match &unsub.actions[1] {
            UnsubscribeAction::OneClickPost { url } => {
                assert_eq!(url, "https://example.com/unsub?id=123");
            }
            _ => panic!("Expected one click post action"),
        }
    }
}
