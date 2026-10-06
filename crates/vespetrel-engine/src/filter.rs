//! Message Filter & Automation Rule Engine §7 Phase 5
use serde::{Deserialize, Serialize};
use vespetrel_core::MessageSummary;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterField {
    FromAddress,
    FromName,
    Subject,
    Snippet,
    HasAttachments,
    IsRead,
    IsFlagged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterPredicate {
    Contains(String),
    Equals(String),
    StartsWith(String),
    EndsWith(String),
    IsTrue,
    IsFalse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterCondition {
    pub field: FilterField,
    pub predicate: FilterPredicate,
}

impl FilterCondition {
    pub fn matches(&self, msg: &MessageSummary) -> bool {
        match (&self.field, &self.predicate) {
            (FilterField::FromAddress, FilterPredicate::Contains(val)) => {
                contains_case_insensitive(&msg.from_address, val)
            }
            (FilterField::FromAddress, FilterPredicate::Equals(val)) => {
                msg.from_address.eq_ignore_ascii_case(val)
            }
            (FilterField::FromAddress, FilterPredicate::StartsWith(val)) => {
                starts_with_case_insensitive(&msg.from_address, val)
            }
            (FilterField::FromAddress, FilterPredicate::EndsWith(val)) => {
                ends_with_case_insensitive(&msg.from_address, val)
            }

            (FilterField::FromName, FilterPredicate::Contains(val)) => msg
                .from_name
                .as_deref()
                .map(|n| contains_case_insensitive(n, val))
                .unwrap_or(false),

            (FilterField::Subject, FilterPredicate::Contains(val)) => msg
                .subject
                .as_deref()
                .map(|s| contains_case_insensitive(s, val))
                .unwrap_or(false),
            (FilterField::Subject, FilterPredicate::StartsWith(val)) => msg
                .subject
                .as_deref()
                .map(|s| starts_with_case_insensitive(s, val))
                .unwrap_or(false),
            (FilterField::Subject, FilterPredicate::Equals(val)) => msg
                .subject
                .as_deref()
                .map(|s| s.eq_ignore_ascii_case(val))
                .unwrap_or(false),

            (FilterField::Snippet, FilterPredicate::Contains(val)) => msg
                .snippet
                .as_deref()
                .map(|s| contains_case_insensitive(s, val))
                .unwrap_or(false),

            (FilterField::HasAttachments, FilterPredicate::IsTrue) => msg.has_attachments,
            (FilterField::HasAttachments, FilterPredicate::IsFalse) => !msg.has_attachments,

            (FilterField::IsRead, FilterPredicate::IsTrue) => msg.is_read,
            (FilterField::IsRead, FilterPredicate::IsFalse) => !msg.is_read,

            (FilterField::IsFlagged, FilterPredicate::IsTrue) => msg.is_flagged,
            (FilterField::IsFlagged, FilterPredicate::IsFalse) => !msg.is_flagged,

            _ => false,
        }
    }
}

#[inline]
fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        // A vacuous "contains nothing" test must not be vacuously *true*: a rule like
        // `{field: Subject, predicate: Contains(""), actions: [Delete]}` would otherwise
        // match every message and delete all of them.
        return false;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    if haystack.is_ascii() && needle.is_ascii() {
        let n_bytes = needle.as_bytes();
        let h_bytes = haystack.as_bytes();
        return h_bytes
            .windows(n_bytes.len())
            .any(|window| window.eq_ignore_ascii_case(n_bytes));
    }
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

#[inline]
fn starts_with_case_insensitive(haystack: &str, needle: &str) -> bool {
    if needle.len() > haystack.len() || !haystack.is_char_boundary(needle.len()) {
        // Byte-index slicing requires a char boundary. A byte-length check alone is not
        // enough: a 6-byte needle against "Re: 議事録" (13 bytes) passes the length test and
        // then panics on `haystack[..6]`, because index 6 is inside the 3-byte '議'.
        // Remote message content reaches this, so it was a remotely triggerable panic.
        return false;
    }
    haystack[..needle.len()].eq_ignore_ascii_case(needle)
}

#[inline]
fn ends_with_case_insensitive(haystack: &str, needle: &str) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    let start = haystack.len() - needle.len();
    if !haystack.is_char_boundary(start) {
        return false;
    }
    haystack[start..].eq_ignore_ascii_case(needle)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionCombinator {
    MatchAll, // AND
    MatchAny, // OR
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterAction {
    MoveToFolder(String),
    MarkRead,
    MarkUnread,
    MarkFlagged,
    MarkUnflagged,
    AddTag(String),
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterRule {
    pub id: String,
    pub name: String,
    pub is_enabled: bool,
    pub combinator: ConditionCombinator,
    pub conditions: Vec<FilterCondition>,
    pub actions: Vec<FilterAction>,
    pub stop_processing: bool,
}

impl FilterRule {
    pub fn new(name: impl Into<String>, combinator: ConditionCombinator) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            is_enabled: true,
            combinator,
            conditions: Vec::new(),
            actions: Vec::new(),
            stop_processing: false,
        }
    }

    pub fn matches(&self, msg: &MessageSummary) -> bool {
        if !self.is_enabled || self.conditions.is_empty() {
            return false;
        }

        match self.combinator {
            ConditionCombinator::MatchAll => self.conditions.iter().all(|c| c.matches(msg)),
            ConditionCombinator::MatchAny => self.conditions.iter().any(|c| c.matches(msg)),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FilterEngine {
    pub rules: Vec<FilterRule>,
}

impl FilterEngine {
    pub fn new(rules: Vec<FilterRule>) -> Self {
        Self { rules }
    }

    pub fn add_rule(&mut self, rule: FilterRule) {
        self.rules.push(rule);
    }

    /// Evaluate all active rules in sequence against an incoming message
    pub fn evaluate(&self, msg: &MessageSummary) -> Vec<FilterAction> {
        let mut executed_actions = Vec::new();

        for rule in &self.rules {
            if rule.matches(msg) {
                executed_actions.extend(rule.actions.clone());
                if rule.stop_processing {
                    break;
                }
            }
        }

        executed_actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    /// Regression: the guards compared *byte* lengths and then sliced at those byte offsets,
    /// which panics unless the offset is a char boundary. Remote message content reaches
    /// these functions, so this was a remotely triggerable panic.
    #[test]
    fn prefix_and_suffix_matching_never_panics_on_multibyte_text() {
        // "Re: " is 4 bytes; "議" spans bytes 4..7. A 6-byte needle would slice at index 6,
        // which is inside '議'.
        assert!(!starts_with_case_insensitive("Re: 議事録", "議事"));
        // The same needle on a haystack that *is* long enough does match.
        assert!(starts_with_case_insensitive("議事録", "議事"));

        // "café" is 5 bytes with the final 'é' at 3..5. A 3-byte needle would slice at
        // index 2, which is inside that 'é' — so it must not match, and must not panic.
        let needle_into_char: String = "fé".to_string() + "x"; // 4 bytes; starts at index 1
        assert!(!ends_with_case_insensitive("café", "éab"));
        assert!(!ends_with_case_insensitive("café", &needle_into_char));
        // Valid boundaries still work.
        assert!(ends_with_case_insensitive("café", "fé"));
        assert!(ends_with_case_insensitive("café", "é"));

        // Empty needles are valid boundaries and must not panic.
        assert!(starts_with_case_insensitive("anything", ""));
        assert!(ends_with_case_insensitive("anything", ""));
        // Mixed scripts at every alignment: must be panic-free regardless of outcome.
        for hay in [
            "", "a", "é", "議", "aé", "議a", "é議", "a議é", "🙂x議", "café",
        ] {
            for needle in ["", "a", "é", "議", "aé", "議a", "🙂", "x議", "fé"] {
                let _ = starts_with_case_insensitive(hay, needle);
                let _ = ends_with_case_insensitive(hay, needle);
            }
        }
    }

    /// A vacuous "contains nothing" rule must not match everything, or
    /// `{Subject, Contains(""), [Delete]}` deletes all mail.
    #[test]
    fn empty_contains_needle_matches_nothing() {
        assert!(!contains_case_insensitive("anything", ""));
        assert!(contains_case_insensitive("anything", "thing"));
        assert!(starts_with_case_insensitive("anything", ""));
        assert!(ends_with_case_insensitive("anything", ""));
    }

    #[test]
    fn test_filter_rule_matching_and_actions() {
        let msg = MessageSummary {
            id: "msg_123".into(),
            thread_id: None,
            subject: Some("[GitHub] Notifications: New PR".into()),
            from_address: "notifications@github.com".into(),
            from_name: Some("GitHub".into()),
            snippet: Some("A new PR was opened".into()),
            sent_at: Utc::now(),
            is_read: false,
            is_flagged: false,
            has_attachments: false,
        };

        let mut rule = FilterRule::new("GitHub Filter", ConditionCombinator::MatchAll);
        rule.conditions.push(FilterCondition {
            field: FilterField::FromAddress,
            predicate: FilterPredicate::Contains("github.com".into()),
        });
        rule.conditions.push(FilterCondition {
            field: FilterField::Subject,
            predicate: FilterPredicate::Contains("[GitHub]".into()),
        });
        rule.actions
            .push(FilterAction::MoveToFolder("github_folder".into()));
        rule.actions.push(FilterAction::MarkRead);

        assert!(rule.matches(&msg));

        let engine = FilterEngine::new(vec![rule]);
        let actions = engine.evaluate(&msg);
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0],
            FilterAction::MoveToFolder("github_folder".into())
        );
        assert_eq!(actions[1], FilterAction::MarkRead);
    }
}
