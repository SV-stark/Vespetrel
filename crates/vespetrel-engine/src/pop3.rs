//! POP3 Legacy Client Protocol Engine (RFC 1939) §7 Phase 6
use ahash::AHashSet;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pop3MessageInfo {
    pub msg_number: usize,
    pub uid: String,
    pub size_bytes: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Pop3UidlCache {
    pub retrieved_uids: AHashSet<String>,
}

impl Pop3UidlCache {
    pub fn is_seen(&self, uid: &str) -> bool {
        self.retrieved_uids.contains(uid)
    }

    pub fn mark_seen(&mut self, uid: impl Into<String>) {
        self.retrieved_uids.insert(uid.into());
    }
}

/// POP3 command generator
pub struct Pop3Command;

impl Pop3Command {
    pub fn user(username: &str) -> String {
        format!("USER {username}\r\n")
    }

    pub fn pass(password: &str) -> String {
        format!("PASS {password}\r\n")
    }

    pub fn stat() -> &'static str {
        "STAT\r\n"
    }

    pub fn list() -> &'static str {
        "LIST\r\n"
    }

    pub fn uidl() -> &'static str {
        "UIDL\r\n"
    }

    pub fn retr(msg_num: usize) -> String {
        format!("RETR {msg_num}\r\n")
    }

    pub fn top(msg_num: usize, lines: usize) -> String {
        format!("TOP {msg_num} {lines}\r\n")
    }

    pub fn dele(msg_num: usize) -> String {
        format!("DELE {msg_num}\r\n")
    }

    pub fn quit() -> &'static str {
        "QUIT\r\n"
    }

    pub fn stls() -> &'static str {
        "STLS\r\n"
    }
}

/// Parse POP3 response lines
pub fn parse_pop3_status(line: &str) -> Result<String, String> {
    let trimmed = line.trim();
    if let Some(msg) = trimmed.strip_prefix("+OK") {
        Ok(msg.trim().to_string())
    } else if let Some(err) = trimmed.strip_prefix("-ERR") {
        Err(err.trim().to_string())
    } else {
        Err(format!("Invalid POP3 response: {trimmed}"))
    }
}

/// Parse multiline UIDL response
pub fn parse_pop3_uidl_response(lines: &str) -> Vec<Pop3MessageInfo> {
    let mut results = Vec::new();
    for line in lines.lines() {
        let line = line.trim();
        if line == "." || line.starts_with("+OK") || line.is_empty() {
            continue;
        }
        if let Some(info) = line.split_once(' ').and_then(|(num_str, uid_str)| {
            num_str.parse::<usize>().ok().map(|num| Pop3MessageInfo {
                msg_number: num,
                uid: uid_str.trim().to_string(),
                size_bytes: 0,
            })
        }) {
            results.push(info);
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pop3_commands_and_uidl_parsing() {
        assert_eq!(Pop3Command::user("alice"), "USER alice\r\n");
        assert_eq!(Pop3Command::retr(1), "RETR 1\r\n");

        assert_eq!(parse_pop3_status("+OK 2 3200").unwrap(), "2 3200");
        assert!(parse_pop3_status("-ERR invalid password").is_err());

        let uidl_response = "+OK\r\n1 uid-abc-123\r\n2 uid-def-456\r\n.\r\n";
        let uids = parse_pop3_uidl_response(uidl_response);
        assert_eq!(uids.len(), 2);
        assert_eq!(uids[0].msg_number, 1);
        assert_eq!(uids[0].uid, "uid-abc-123");
        assert_eq!(uids[1].msg_number, 2);
        assert_eq!(uids[1].uid, "uid-def-456");
    }

    #[test]
    fn test_pop3_uidl_cache() {
        let mut cache = Pop3UidlCache::default();
        assert!(!cache.is_seen("uid-100"));
        cache.mark_seen("uid-100");
        assert!(cache.is_seen("uid-100"));
    }
}
