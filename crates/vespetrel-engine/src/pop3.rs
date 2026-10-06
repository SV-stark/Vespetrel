//! POP3 Legacy Client Protocol Engine (RFC 1939) §7 Phase 6
use ahash::AHashSet;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pop3MessageInfo {
    pub msg_number: usize,
    pub uid: String,
    /// Message size in octets.
    ///
    /// A `UIDL` response carries no size at all (RFC 1939 §6.5: `n uid`), so this is `None`
    /// after [`parse_pop3_uidl_response`]. Populating it from a `UIDL` line would report every
    /// message as zero bytes. Use [`parse_pop3_list_response`] plus [`merge_sizes`] to fill it.
    pub size_bytes: Option<usize>,
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

    /// Drop every UID that is no longer present on the server, returning them.
    ///
    /// POP3 UIDs are unique per message, but messages get deleted server-side all the time.
    /// Without reconciliation the cache grows without bound for the life of the process and a
    /// server that recycles UIDs (several broken implementations return the message number as
    /// the UID) would make a re-issued message look permanently seen and never download it.
    pub fn reconcile(&mut self, live_uids: &AHashSet<String>) -> Vec<String> {
        let stale: Vec<String> = self
            .retrieved_uids
            .iter()
            .filter(|uid| !live_uids.contains(*uid))
            .cloned()
            .collect();
        for uid in &stale {
            self.retrieved_uids.remove(uid);
        }
        stale
    }
}

/// POP3 command generator
pub struct Pop3Command;

impl Pop3Command {
    /// Reject any argument that would break out of its command line.
    ///
    /// Stripping CR/LF silently mangles the credential (`"pa\r\nDELE 1\r\nss"` becomes
    /// `"paDELE 1ss"`), which then fails authentication with no hint as to why. Refusing is
    /// the only safe behaviour: POP3 arguments are line-oriented and cannot legally contain a
    /// bare CR or LF.
    fn command_arg(value: &str) -> Result<String, String> {
        if value.contains(['\r', '\n', '\0']) {
            return Err("POP3 argument contains CR, LF or NUL".to_string());
        }
        Ok(value.to_string())
    }

    pub fn user(username: &str) -> Result<String, String> {
        Ok(format!("USER {}\r\n", Self::command_arg(username)?))
    }

    pub fn pass(password: &str) -> Result<String, String> {
        Ok(format!("PASS {}\r\n", Self::command_arg(password)?))
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

/// Parse multiline `UIDL` response (RFC 1939 §6.5).
///
/// `size_bytes` is left `None`: the response format has no size field.
pub fn parse_pop3_uidl_response(lines: &str) -> Vec<Pop3MessageInfo> {
    let mut results = Vec::new();
    for line in lines.lines() {
        let line = line.trim();
        if line == "." || line.starts_with("+OK") || line.is_empty() {
            continue;
        }
        let mut fields = line.split_whitespace();
        let (Some(num_str), Some(uid)) = (fields.next(), fields.next()) else {
            continue;
        };
        if fields.next().is_some() {
            continue;
        }
        let Ok(num) = num_str.parse::<usize>() else {
            continue;
        };
        // A server without UIDL support replies `<no unique id>`; caching that literal would
        // make every message look like the same already-retrieved message.
        if uid.starts_with('<') {
            continue;
        }
        results.push(Pop3MessageInfo {
            msg_number: num,
            uid: uid.to_string(),
            size_bytes: None,
        });
    }
    results
}

/// Parse a multiline `LIST` response (RFC 1939 §6.3) into message number -> octet count.
pub fn parse_pop3_list_response(lines: &str) -> ahash::AHashMap<usize, usize> {
    let mut sizes = ahash::AHashMap::new();
    for line in lines.lines() {
        let line = line.trim();
        if line == "." || line.starts_with("+OK") || line.is_empty() {
            continue;
        }
        let mut fields = line.split_whitespace();
        let (Some(num_str), Some(size_str)) = (fields.next(), fields.next()) else {
            continue;
        };
        if fields.next().is_some() {
            continue;
        }
        if let (Ok(num), Ok(size)) = (num_str.parse::<usize>(), size_str.parse::<usize>()) {
            sizes.insert(num, size);
        }
    }
    sizes
}

/// Fill in [`Pop3MessageInfo::size_bytes`] from a parsed `LIST` response.
pub fn merge_sizes(infos: &mut [Pop3MessageInfo], sizes: &ahash::AHashMap<usize, usize>) {
    for info in infos.iter_mut() {
        info.size_bytes = sizes.get(&info.msg_number).copied();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pop3_commands_and_uidl_parsing() {
        assert_eq!(Pop3Command::user("alice").unwrap(), "USER alice\r\n");
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

    /// A `UIDL` line carries no size. Reporting `0` made every message look empty and any
    /// caller that used it for a progress bar or a size cap silently did the wrong thing.
    #[test]
    fn test_pop3_uidl_has_no_phantom_size() {
        let mut infos = parse_pop3_uidl_response("+OK\r\n1 uid-a\r\n.\r\n");
        assert_eq!(infos[0].size_bytes, None);

        let sizes = parse_pop3_list_response("+OK 1 1234\r\n1 1234\r\n.\r\n");
        merge_sizes(&mut infos, &sizes);
        assert_eq!(infos[0].size_bytes, Some(1234));
    }

    /// `<no unique id>` is what a server without UIDL support sends. Caching that literal
    /// collapses the whole mailbox into one "already seen" UID.
    #[test]
    fn test_pop3_uidl_rejects_no_unique_id_sentinel() {
        let infos = parse_pop3_uidl_response("+OK\r\n1 <no unique id>\r\n2 uid-real\r\n.\r\n");
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].uid, "uid-real");
    }

    /// Stripping CR/LF turned `"pa\r\nDELE 1\r\nss"` into `"paDELE 1ss"`, so the mangled
    /// credential was sent and the user saw an opaque authentication failure.
    #[test]
    fn test_pop3_rejects_crlf_in_credentials() {
        assert!(Pop3Command::pass("hunter2\r\nDELE 1").is_err());
        assert!(Pop3Command::user("alice\n").is_err());
        assert_eq!(Pop3Command::pass("hunter2").unwrap(), "PASS hunter2\r\n");
    }

    #[test]
    fn test_pop3_uidl_cache() {
        let mut cache = Pop3UidlCache::default();
        assert!(!cache.is_seen("uid-100"));
        cache.mark_seen("uid-100");
        assert!(cache.is_seen("uid-100"));
    }

    /// Server-side deletion must not leave a UID cached forever, otherwise a recycled UID
    /// makes a brand new message look already-retrieved.
    #[test]
    fn test_pop3_uidl_cache_reconcile_drops_deleted() {
        let mut cache = Pop3UidlCache::default();
        cache.mark_seen("uid-100");
        cache.mark_seen("uid-101");
        cache.mark_seen("uid-102");

        let live: AHashSet<String> = ["uid-100".to_string(), "uid-102".to_string()].into();
        let mut stale = cache.reconcile(&live);
        stale.sort();
        assert_eq!(stale, vec!["uid-101".to_string()]);
        assert!(!cache.is_seen("uid-101"));
        assert!(cache.is_seen("uid-100"));
        assert!(cache.is_seen("uid-102"));
    }
}
