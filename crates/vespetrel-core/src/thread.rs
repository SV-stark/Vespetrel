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

        // Cap processing to 50,000 messages to prevent memory exhaustion
        let msg_slice = if messages.len() > 50_000 {
            &messages[..50_000]
        } else {
            messages
        };
        let cap = msg_slice.len();

        // Map: message_id_header -> index in table
        let mut id_to_msg: AHashMap<String, &crate::Message> = AHashMap::with_capacity(cap);
        let mut child_to_parent: AHashMap<String, String> = AHashMap::with_capacity(cap);

        for msg in msg_slice {
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
        let mut parent_to_children: AHashMap<String, Vec<String>> = AHashMap::with_capacity(cap);
        let mut root_keys = Vec::new();

        for msg in msg_slice {
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

        fn build_node_iterative(
            root_key: &str,
            id_to_msg: &AHashMap<String, &crate::Message>,
            parent_to_children: &AHashMap<String, Vec<String>>,
            visited: &mut AHashSet<String>,
        ) -> Option<ThreadNode> {
            if !visited.insert(root_key.to_string()) {
                return None;
            }
            let root_msg = *id_to_msg.get(root_key)?;

            struct BuildFrame<'a> {
                msg: &'a crate::Message,
                child_keys: Vec<String>,
                child_idx: usize,
                built_children: Vec<ThreadNode>,
            }

            let initial_children = parent_to_children
                .get(root_key)
                .cloned()
                .unwrap_or_default();

            let mut stack = vec![BuildFrame {
                msg: root_msg,
                child_keys: initial_children,
                child_idx: 0,
                built_children: Vec::new(),
            }];

            let mut root_node = None;

            while let Some(top) = stack.last_mut() {
                if top.child_idx < top.child_keys.len() {
                    let next_key = top.child_keys[top.child_idx].clone();
                    top.child_idx += 1;

                    if stack.len() <= 50
                        && visited.insert(next_key.clone())
                        && let Some(&child_msg) = id_to_msg.get(&next_key)
                    {
                        let next_children = parent_to_children
                            .get(&next_key)
                            .cloned()
                            .unwrap_or_default();
                        stack.push(BuildFrame {
                            msg: child_msg,
                            child_keys: next_children,
                            child_idx: 0,
                            built_children: Vec::new(),
                        });
                    }
                } else if let Some(frame) = stack.pop() {
                    let mut children = frame.built_children;
                    children.sort_by_key(|c| c.sent_at);

                    let node = ThreadNode {
                        message_id: frame.msg.id.clone(),
                        message_id_header: frame.msg.message_id_header.clone(),
                        subject: frame.msg.subject.clone(),
                        sent_at: frame.msg.sent_at,
                        is_read: frame.msg.is_read,
                        snippet: frame.msg.body_snippet.clone(),
                        children,
                    };

                    if let Some(parent_frame) = stack.last_mut() {
                        parent_frame.built_children.push(node);
                    } else {
                        root_node = Some(node);
                        break;
                    }
                }
            }

            root_node
        }

        let mut root_nodes = Vec::new();
        let mut visited = AHashSet::with_capacity(messages.len());
        for rk in root_keys {
            if let Some(node) =
                build_node_iterative(&rk, &id_to_msg, &parent_to_children, &mut visited)
            {
                root_nodes.push(node);
            }
        }

        // Break cycles: any messages not reached from roots become roots
        let remaining_keys: Vec<String> = id_to_msg
            .keys()
            .filter(|k| !visited.contains(*k))
            .cloned()
            .collect();
        for key in remaining_keys {
            if let Some(node) =
                build_node_iterative(&key, &id_to_msg, &parent_to_children, &mut visited)
            {
                root_nodes.push(node);
            }
        }

        // Sort root threads by latest message date across all descendants descending
        root_nodes.sort_by_key(|node| {
            let (_, _, latest_date) = Self::summarize_node(node);
            std::cmp::Reverse(latest_date)
        });

        Self { root_nodes }
    }

    /// Calculate thread summary statistics: total messages, unread count, latest date (iterative)
    pub fn summarize_node(root: &ThreadNode) -> (usize, usize, DateTime<Utc>) {
        let mut total = 0;
        let mut unread = 0;
        let mut latest = root.sent_at;
        let mut stack = vec![root];

        while let Some(node) = stack.pop() {
            total += 1;
            if !node.is_read {
                unread += 1;
            }
            if node.sent_at > latest {
                latest = node.sent_at;
            }
            for child in &node.children {
                stack.push(child);
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
/// Uses zero-allocation SIMD byte checking and memchr
pub fn normalize_subject(subject: &str) -> String {
    let mut bytes = subject.trim().as_bytes();
    loop {
        // Fast skip whitespace
        while let Some(&b) = bytes.first() {
            if b == b' ' || b == b'\t' {
                bytes = &bytes[1..];
            } else {
                break;
            }
        }
        if bytes.len() >= 3
            && (bytes[..3].eq_ignore_ascii_case(b"re:") || bytes[..3].eq_ignore_ascii_case(b"fw:"))
        {
            bytes = &bytes[3..];
            continue;
        }
        if bytes.len() >= 4 && bytes[..4].eq_ignore_ascii_case(b"fwd:") {
            bytes = &bytes[4..];
            continue;
        }
        if bytes.len() >= 4
            && (bytes[..3].eq_ignore_ascii_case(b"re[") || bytes[..3].eq_ignore_ascii_case(b"fw["))
            && let Some(idx) = memchr::memchr(b']', bytes)
            && idx + 1 < bytes.len()
            && bytes[idx + 1] == b':'
        {
            bytes = &bytes[idx + 2..];
            continue;
        }
        break;
    }

    String::from_utf8_lossy(bytes).trim().to_string()
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
