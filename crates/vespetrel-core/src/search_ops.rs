//! Search operators parsed from a user query.
//!
//! Supports `from:`, `to:`, `has:attachment`, `before:` (YYYY-MM-DD),
//! `is:unread` / `is:read`, and `label:` on top of free-text terms.

use chrono::{NaiveDate, TimeZone, Utc};

#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    /// Free-text terms fed to FTS5.
    pub text: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub has_attachment: bool,
    pub before: Option<i64>,
    pub is_unread: Option<bool>,
    pub label: Option<String>,
}

impl SearchFilters {
    pub fn parse_from_str(raw: &str) -> Self {
        parse_search_query(raw)
    }
}

pub fn parse_search_query(raw: &str) -> SearchFilters {
    let mut filters = SearchFilters::default();
    let mut text_terms = Vec::new();
    for token in raw.split_whitespace() {
        if let Some((key, val)) = token.split_once(':') {
            match key.to_lowercase().as_str() {
                "from" => {
                    filters.from = Some(val.to_string());
                    continue;
                }
                "to" => {
                    filters.to = Some(val.to_string());
                    continue;
                }
                "has" if val.eq_ignore_ascii_case("attachment") => {
                    filters.has_attachment = true;
                    continue;
                }
                "before" => {
                    if let Ok(d) = NaiveDate::parse_from_str(val, "%Y-%m-%d") {
                        let dt = d.and_hms_opt(0, 0, 0).unwrap();
                        filters.before = Some(Utc.from_utc_datetime(&dt).timestamp());
                    }
                    continue;
                }
                "is" => {
                    if val.eq_ignore_ascii_case("unread") {
                        filters.is_unread = Some(true);
                    } else if val.eq_ignore_ascii_case("read") {
                        filters.is_unread = Some(false);
                    }
                    continue;
                }
                "label" => {
                    filters.label = Some(val.to_string());
                    continue;
                }
                _ => {}
            }
        }
        text_terms.push(token.to_string());
    }
    filters.text = text_terms.join(" ");
    filters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_operators() {
        let f = parse_search_query(
            "from:alice@example.com report has:attachment before:2026-01-01 is:unread",
        );
        assert_eq!(f.from.as_deref(), Some("alice@example.com"));
        assert_eq!(f.text, "report");
        assert!(f.has_attachment);
        assert!(f.before.is_some());
        assert_eq!(f.is_unread, Some(true));
    }
}
