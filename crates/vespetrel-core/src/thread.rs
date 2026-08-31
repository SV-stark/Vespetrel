use ahash::{AHashMap, AHashSet};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub id: String,
    pub account_id: String,
    pub subject: Option<String>,
    pub last_message_at: DateTime<Utc>,
    pub message_count: i64,
    pub unread_count: i64,
    pub snippet: Option<String>,
}

impl Thread {
    pub fn new(account_id: impl Into<String>, subject: Option<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            account_id: account_id.into(),
            subject,
            last_message_at: Utc::now(),
            message_count: 1,
            unread_count: 1,
            snippet: None,
        }
    }
}

/// A node in the JWZ message thread tree
#[derive(Debug, Clone)]
pub struct ThreadNode {
    pub message_id: String,
    pub message_id_header: Option<String>,
    pub subject: Option<String>,
    pub sent_at: DateTime<Utc>,
    pub is_read: bool,
    pub snippet: Option<String>,
    pub children: Vec<ThreadNode>,
}

/// Thread container for grouping and sorting messages into conversational threads
#[derive(Debug, Clone)]
pub struct ThreadTree {
    pub root_nodes: Vec<ThreadNode>,
}

impl ThreadTree {
    /// Build thread trees from a flat slice of messages using RFC 5322 In-Reply-To & References
    /// Uses AHashMap for hardware-accelerated (AES-NI) hashing and cycle detection
    pub fn build(messages: &[crate::Message]) -> Self {
        if messages.is_empty() {
            return Self {
                root_nodes: Vec::new(),
            };
        }

        // Map: message_id_header -> index in table
        let mut id_to_msg: AHashMap<String, &crate::Message> =
            AHashMap::with_capacity(messages.len());
        let mut child_to_parent: AHashMap<String, String> = AHashMap::with_capacity(messages.len());

        for msg in messages {
            let key = msg
                .message_id_header
                .clone()
                .unwrap_or_else(|| msg.id.clone());
            id_to_msg.insert(key.clone(), msg);

            // Resolve parent from In-Reply-To or last ID in References
            let parent = msg
                .in_reply_to
                .as_deref()
                .map(clean_message_id)
                .filter(|t| !t.is_empty())
                .or_else(|| {
                    msg.references
                        .as_deref()
                        .and_then(|refs| refs.split_whitespace().last().map(clean_message_id))
                        .filter(|t| !t.is_empty())
                });

            if let Some(p) = parent {
                child_to_parent.insert(key, p);
            }
        }

        // Build child adjacency
        let mut parent_to_children: AHashMap<String, Vec<String>> =
            AHashMap::with_capacity(messages.len());
        let mut root_keys = Vec::new();

        for msg in messages {
            let key = msg
                .message_id_header
                .clone()
                .unwrap_or_else(|| msg.id.clone());
            if let Some(parent_key) = child_to_parent
                .get(&key)
                .filter(|pk| id_to_msg.contains_key(*pk))
            {
                parent_to_children
                    .entry(parent_key.clone())
                    .or_default()
                    .push(key);
                continue;
            }
            // If parent not found or no parent, this is a root
            root_keys.push(key);
        }

        fn build_node(
            key: &str,
            id_to_msg: &AHashMap<String, &crate::Message>,
            parent_to_children: &AHashMap<String, Vec<String>>,
            visited: &mut AHashSet<String>,
            depth: usize,
        ) -> Option<ThreadNode> {
            if depth > 100 || !visited.insert(key.to_string()) {
                return None; // Guard against infinite recursion & cycles
            }

            let msg = id_to_msg.get(key)?;
            let mut children = Vec::new();
            if let Some(child_keys) = parent_to_children.get(key) {
                for ck in child_keys {
                    if let Some(child_node) =
                        build_node(ck, id_to_msg, parent_to_children, visited, depth + 1)
                    {
                        children.push(child_node);
                    }
                }
            }
            // Sort children by date ascending
            children.sort_by_key(|c| c.sent_at);

            Some(ThreadNode {
                message_id: msg.id.clone(),
                message_id_header: msg.message_id_header.clone(),
                subject: msg.subject.clone(),
                sent_at: msg.sent_at,
                is_read: msg.is_read,
                snippet: msg.body_snippet.clone(),
                children,
            })
        }

        let mut root_nodes = Vec::new();
        let mut visited = AHashSet::with_capacity(messages.len());
        for rk in root_keys {
            if let Some(node) = build_node(&rk, &id_to_msg, &parent_to_children, &mut visited, 0) {
                root_nodes.push(node);
            }
        }

        // Break cycles: any messages not reached from roots become roots
        for key in id_to_msg.keys() {
            if !visited.contains(key) {
                if let Some(node) =
                    build_node(key, &id_to_msg, &parent_to_children, &mut visited, 0)
                {
                    root_nodes.push(node);
                }
            }
        }

        // Sort root threads by latest message date across all descendants descending
        root_nodes.sort_by_key(|node| {
            let (_, _, latest_date) = Self::summarize_node(node);
            std::cmp::Reverse(latest_date)
        });

        Self { root_nodes }
    }

    /// Calculate thread summary statistics: total messages, unread count, latest date
    pub fn summarize_node(node: &ThreadNode) -> (usize, usize, DateTime<Utc>) {
        let mut total = 1;
        let mut unread = if node.is_read { 0 } else { 1 };
        let mut latest = node.sent_at;

        for child in &node.children {
            let (c_total, c_unread, c_latest) = Self::summarize_node(child);
            total += c_total;
            unread += c_unread;
            if c_latest > latest {
                latest = c_latest;
            }
        }

        (total, unread, latest)
    }
}

fn clean_message_id(s: &str) -> String {
    let s = s.trim();
    let extracted = s
        .split_once('<')
        .and_then(|(_, rest)| rest.split_once('>'))
        .map(|(inner, _)| inner.trim());
    if let Some(inner) = extracted {
        return inner.to_string();
    }
    s.trim_matches('<').trim_matches('>').trim().to_string()
}

/// Helper to normalize email subjects by stripping Re:, Fwd:, etc.
/// Uses memchr for fast delimiter scanning
pub fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_lowercase();
        if lower.starts_with("re:") || lower.starts_with("fw:") {
            s = s[3..].trim();
        } else if lower.starts_with("fwd:") {
            s = s[4..].trim();
        } else if lower.starts_with("re[") || lower.starts_with("fw[") {
            if let Some(idx) =
                memchr::memchr(b']', s.as_bytes()).filter(|&idx| s[idx + 1..].starts_with(':'))
            {
                s = s[idx + 2..].trim();
                continue;
            }
            break;
        } else {
            break;
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    #[test]
    fn test_normalize_subject() {
        assert_eq!(normalize_subject("Re: Hello World"), "Hello World");
        assert_eq!(
            normalize_subject("Fwd: Re: [Team] Meeting"),
            "[Team] Meeting"
        );
        assert_eq!(
            normalize_subject("RE:  FWD:  Status Report "),
            "Status Report"
        );
        assert_eq!(normalize_subject("Re[2]: Update"), "Update");
        assert_eq!(normalize_subject("Normal Subject"), "Normal Subject");
    }

    #[test]
    fn test_jwz_thread_tree() {
        let mut msg1 = Message::new(
            "acct1",
            "inbox",
            101,
            "Project Launch",
            "alice@example.com",
            vec!["bob@example.com".into()],
        );
        msg1.message_id_header = Some("msg1@example.com".into());
        msg1.is_read = true;

        let mut msg2 = Message::new(
            "acct1",
            "inbox",
            102,
            "Re: Project Launch",
            "bob@example.com",
            vec!["alice@example.com".into()],
        );
        msg2.message_id_header = Some("msg2@example.com".into());
        msg2.in_reply_to = Some("msg1@example.com".into());
        msg2.is_read = false;

        let mut msg3 = Message::new(
            "acct1",
            "inbox",
            103,
            "Re: Project Launch",
            "charlie@example.com",
            vec!["alice@example.com".into()],
        );
        msg3.message_id_header = Some("msg3@example.com".into());
        msg3.in_reply_to = Some("msg2@example.com".into());
        msg3.is_read = false;

        let messages = vec![msg1, msg2, msg3];
        let tree = ThreadTree::build(&messages);

        assert_eq!(tree.root_nodes.len(), 1);
        let root = &tree.root_nodes[0];
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].children.len(), 1);

        let (total, unread, _) = ThreadTree::summarize_node(root);
        assert_eq!(total, 3);
        assert_eq!(unread, 2);
    }

    #[test]
    fn test_cyclic_thread_prevention() {
        let mut msg1 = Message::new("acct1", "inbox", 101, "Loop A", "a@example.com", vec![]);
        msg1.message_id_header = Some("id_a".into());
        msg1.in_reply_to = Some("id_b".into());

        let mut msg2 = Message::new("acct1", "inbox", 102, "Loop B", "b@example.com", vec![]);
        msg2.message_id_header = Some("id_b".into());
        msg2.in_reply_to = Some("id_a".into());

        let tree = ThreadTree::build(&[msg1, msg2]);
        assert!(!tree.root_nodes.is_empty());
    }
}
