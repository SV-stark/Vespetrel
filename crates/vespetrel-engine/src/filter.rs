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
            (FilterField::FromAddress, FilterPredicate::Contains(val)) => msg
                .from_address
                .to_lowercase()
                .contains(&val.to_lowercase()),
            (FilterField::FromAddress, FilterPredicate::Equals(val)) => {
                msg.from_address.eq_ignore_ascii_case(val)
            }
            (FilterField::FromAddress, FilterPredicate::StartsWith(val)) => msg
                .from_address
                .to_lowercase()
                .starts_with(&val.to_lowercase()),
            (FilterField::FromAddress, FilterPredicate::EndsWith(val)) => msg
                .from_address
                .to_lowercase()
                .ends_with(&val.to_lowercase()),

            (FilterField::FromName, FilterPredicate::Contains(val)) => msg
                .from_name
                .as_deref()
                .map(|n| n.to_lowercase().contains(&val.to_lowercase()))
                .unwrap_or(false),

            (FilterField::Subject, FilterPredicate::Contains(val)) => msg
                .subject
                .as_deref()
                .map(|s| s.to_lowercase().contains(&val.to_lowercase()))
                .unwrap_or(false),
            (FilterField::Subject, FilterPredicate::StartsWith(val)) => msg
                .subject
                .as_deref()
                .map(|s| s.to_lowercase().starts_with(&val.to_lowercase()))
                .unwrap_or(false),
            (FilterField::Subject, FilterPredicate::Equals(val)) => msg
                .subject
                .as_deref()
                .map(|s| s.eq_ignore_ascii_case(val))
                .unwrap_or(false),

            (FilterField::Snippet, FilterPredicate::Contains(val)) => msg
                .snippet
                .as_deref()
                .map(|s| s.to_lowercase().contains(&val.to_lowercase()))
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
