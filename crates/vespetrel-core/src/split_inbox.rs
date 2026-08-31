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

/// Fast SIMD substring search
#[inline]
fn simd_contains(haystack: &str, needle: &str) -> bool {
    let hay = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return true;
    }
    if hay.len() < n.len() {
        return false;
    }
    memchr::memmem::find(hay, n).is_some()
}

/// Classify an incoming message into a split inbox category using SIMD header & domain heuristics
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
        if simd_contains(&headers_lower, "list-unsubscribe")
            || simd_contains(&headers_lower, "list-id")
        {
            if simd_contains(&headers_lower, "newsletter")
                || simd_contains(&headers_lower, "digest")
                || simd_contains(&headers_lower, "substack")
            {
                return InboxCategory::Newsletters;
            }
            return InboxCategory::Promotions;
        }
        if simd_contains(&headers_lower, "precedence: bulk")
            || simd_contains(&headers_lower, "precedence: list")
        {
            return InboxCategory::Updates;
        }
    }

    // 2. Check Social networks
    if simd_contains(&from_lower, "twitter.com")
        || simd_contains(&from_lower, "x.com")
        || simd_contains(&from_lower, "linkedin.com")
        || simd_contains(&from_lower, "facebookmail.com")
        || simd_contains(&from_lower, "instagram.com")
        || simd_contains(&from_lower, "redditmail.com")
    {
        return InboxCategory::Social;
    }

    // 3. Check Newsletters & Subscriptions
    if simd_contains(&from_lower, "substack.com")
        || simd_contains(&from_lower, "medium.com")
        || simd_contains(&from_lower, "newsletter")
        || simd_contains(&subject_lower, "weekly digest")
        || simd_contains(&subject_lower, "newsletter")
    {
        return InboxCategory::Newsletters;
    }

    // 4. Check Promotions & Deals
    if simd_contains(&subject_lower, "% off")
        || simd_contains(&subject_lower, "discount")
        || simd_contains(&subject_lower, "sale")
        || simd_contains(&subject_lower, "promo")
        || simd_contains(&subject_lower, "deal")
        || simd_contains(&from_lower, "promotions")
        || simd_contains(&from_lower, "marketing")
    {
        return InboxCategory::Promotions;
    }

    // 5. Check System Updates & Notifications
    if simd_contains(&from_lower, "no-reply")
        || simd_contains(&from_lower, "noreply")
        || simd_contains(&from_lower, "notifications@")
        || simd_contains(&from_lower, "github.com")
        || simd_contains(&from_lower, "gitlab.com")
        || simd_contains(&subject_lower, "security alert")
        || simd_contains(&subject_lower, "invoice")
        || simd_contains(&subject_lower, "receipt")
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
