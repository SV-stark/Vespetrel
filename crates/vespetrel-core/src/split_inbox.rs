//! Split Inbox Categories & Auto-Sorting Classifier §7 Phase 7
use crate::MessageSummary;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InboxCategory {
    Primary,
    Updates,
    Promotions,
    Newsletters,
    Social,
}

impl InboxCategory {
    pub fn display_name(&self) -> &'static str {
        match self {
            InboxCategory::Primary => "Primary",
            InboxCategory::Updates => "Updates",
            InboxCategory::Promotions => "Promotions",
            InboxCategory::Newsletters => "Newsletters",
            InboxCategory::Social => "Social",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            InboxCategory::Primary => "inbox",
            InboxCategory::Updates => "bell",
            InboxCategory::Promotions => "tag",
            InboxCategory::Newsletters => "newspaper",
            InboxCategory::Social => "users",
        }
    }
}

/// Classify an incoming message into a split inbox category using header & domain heuristics
pub fn classify_inbox_category(msg: &MessageSummary, raw_headers: Option<&str>) -> InboxCategory {
    let from_lower = msg.from_address.to_lowercase();
    let subject_lower = msg
        .subject
        .as_deref()
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    // 1. Check RFC headers if available
    if let Some(headers) = raw_headers {
        let headers_lower = headers.to_lowercase();
        if headers_lower.contains("list-unsubscribe") || headers_lower.contains("list-id") {
            if headers_lower.contains("newsletter")
                || headers_lower.contains("digest")
                || headers_lower.contains("substack")
            {
                return InboxCategory::Newsletters;
            }
            return InboxCategory::Promotions;
        }
        if headers_lower.contains("precedence: bulk") || headers_lower.contains("precedence: list")
        {
            return InboxCategory::Updates;
        }
    }

    // 2. Check Social networks
    if from_lower.contains("twitter.com")
        || from_lower.contains("x.com")
        || from_lower.contains("linkedin.com")
        || from_lower.contains("facebookmail.com")
        || from_lower.contains("instagram.com")
        || from_lower.contains("redditmail.com")
    {
        return InboxCategory::Social;
    }

    // 3. Check Newsletters & Subscriptions
    if from_lower.contains("substack.com")
        || from_lower.contains("medium.com")
        || from_lower.contains("newsletter")
        || subject_lower.contains("weekly digest")
        || subject_lower.contains("newsletter")
    {
        return InboxCategory::Newsletters;
    }

    // 4. Check Promotions & Deals
    if subject_lower.contains("% off")
        || subject_lower.contains("discount")
        || subject_lower.contains("sale")
        || subject_lower.contains("promo")
        || subject_lower.contains("deal")
        || from_lower.contains("promotions")
        || from_lower.contains("marketing")
    {
        return InboxCategory::Promotions;
    }

    // 5. Check System Updates & Notifications
    if from_lower.contains("no-reply")
        || from_lower.contains("noreply")
        || from_lower.contains("notifications@")
        || from_lower.contains("github.com")
        || from_lower.contains("gitlab.com")
        || subject_lower.contains("security alert")
        || subject_lower.contains("invoice")
        || subject_lower.contains("receipt")
    {
        return InboxCategory::Updates;
    }

    InboxCategory::Primary
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_classify_inbox_categories() {
        let promo = MessageSummary {
            id: "msg1".into(),
            thread_id: None,
            subject: Some("Special Weekend 50% Off Sale!".into()),
            from_address: "sales@store.com".into(),
            from_name: None,
            snippet: None,
            sent_at: Utc::now(),
            is_read: false,
            is_flagged: false,
            has_attachments: false,
        };
        assert_eq!(
            classify_inbox_category(&promo, None),
            InboxCategory::Promotions
        );

        let social = MessageSummary {
            id: "msg2".into(),
            thread_id: None,
            subject: Some("You have a new connection request".into()),
            from_address: "updates@linkedin.com".into(),
            from_name: None,
            snippet: None,
            sent_at: Utc::now(),
            is_read: false,
            is_flagged: false,
            has_attachments: false,
        };
        assert_eq!(
            classify_inbox_category(&social, None),
            InboxCategory::Social
        );

        let newsletter = MessageSummary {
            id: "msg3".into(),
            thread_id: None,
            subject: Some("Rust Weekly Digest #400".into()),
            from_address: "digest@rust-weekly.org".into(),
            from_name: None,
            snippet: None,
            sent_at: Utc::now(),
            is_read: false,
            is_flagged: false,
            has_attachments: false,
        };
        assert_eq!(
            classify_inbox_category(&newsletter, None),
            InboxCategory::Newsletters
        );
    }
}
