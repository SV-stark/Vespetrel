//! 1-Click Thunderbird & Apple Mail Migrator §7 Phase 5
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use vespetrel_core::account::ProviderType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThunderbirdProfile {
    pub name: String,
    pub path: PathBuf,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigratedAccount {
    pub name: String,
    pub email: String,
    pub incoming_host: String,
    pub incoming_port: u16,
    pub incoming_user: String,
    pub provider_type: ProviderType,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<u16>,
    pub smtp_user: Option<String>,
}

/// Discover existing Mozilla Thunderbird installation profiles on Windows, macOS, or Linux
pub fn discover_thunderbird_profiles() -> Vec<ThunderbirdProfile> {
    let base_dir = get_thunderbird_data_dir();
    let mut profiles = Vec::new();

    if let Some(base) = base_dir {
        let profiles_ini = base.join("profiles.ini");
        if let Ok(content) = std::fs::read_to_string(&profiles_ini) {
            profiles.extend(parse_profiles_ini(&content, &base));
        }
    }

    profiles
}

fn get_thunderbird_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .ok()
            .map(|appdata| PathBuf::from(appdata).join("Thunderbird"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join("Library").join("Thunderbird"))
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        std::env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join(".thunderbird"))
    }
}

/// Parse standard Mozilla `profiles.ini`
pub fn parse_profiles_ini(content: &str, base_dir: &Path) -> Vec<ThunderbirdProfile> {
    let mut profiles = Vec::new();
    let mut current_name = None;
    let mut current_path = None;
    let mut current_is_default = false;
    let mut is_relative = true;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            if let (Some(name), Some(path_str)) = (current_name.take(), current_path.take()) {
                let full_path = if is_relative {
                    base_dir.join(path_str)
                } else {
                    PathBuf::from(path_str)
                };
                profiles.push(ThunderbirdProfile {
                    name,
                    path: full_path,
                    is_default: current_is_default,
                });
            }
            current_is_default = false;
            is_relative = true;
        } else if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "Name" => current_name = Some(v.trim().to_string()),
                "Path" => current_path = Some(v.trim().to_string()),
                "Default" => current_is_default = v.trim() == "1",
                "IsRelative" => is_relative = v.trim() == "1",
                _ => {}
            }
        }
    }

    if let (Some(name), Some(path_str)) = (current_name, current_path) {
        let full_path = if is_relative {
            base_dir.join(path_str)
        } else {
            PathBuf::from(path_str)
        };
        profiles.push(ThunderbirdProfile {
            name,
            path: full_path,
            is_default: current_is_default,
        });
    }

    profiles
}

/// Parse account configurations from Thunderbird's `prefs.js`
///
/// Thunderbird models mail as `mail.server.serverN` (incoming **and** outgoing: a server
/// whose `type` is `smtp` is the account's submission host), plus `mail.identity.idN` and
/// `mail.account.accountN` records that bind an identity to a server. The previous version
/// ignored all three links and took `identities.values().next()`, i.e. an arbitrary identity
/// from a hash map, for **every** server — so a two-account profile produced two copies of
/// whichever account hashed first, with the wrong address on one of them. It also created a
/// bogus IMAP account for the outgoing `smtp` server.
pub fn parse_thunderbird_prefs(prefs_js: &str) -> Vec<MigratedAccount> {
    use ahash::AHashMap;
    let mut servers: AHashMap<String, TbServer> = AHashMap::new();
    let mut identities: AHashMap<String, TbIdentity> = AHashMap::new();
    // account id -> (server id, identity ids)
    let mut account_links: Vec<(String, String, Vec<String>)> = Vec::new();

    for line in prefs_js.lines() {
        let line = line.trim();
        let Some(stripped) = line
            .strip_prefix("user_pref(\"")
            .and_then(|s| s.strip_suffix(");"))
        else {
            continue;
        };
        let Some((key, val)) = stripped.split_once("\", ") else {
            continue;
        };
        let val_clean = val.trim_matches('"');
        if let Some(server_id) = key.strip_prefix("mail.server.") {
            let parts: Vec<&str> = server_id.split('.').collect();
            if parts.len() >= 2 {
                let entry = servers.entry(parts[0].to_string()).or_default();
                match parts[1] {
                    "hostname" => entry.host = val_clean.to_string(),
                    "userName" => entry.user = val_clean.to_string(),
                    "port" => entry.port = val_clean.parse::<u16>().ok(),
                    "type" => entry.kind = Some(val_clean.to_ascii_lowercase()),
                    _ => {}
                }
            }
        } else if let Some(identity_id) = key.strip_prefix("mail.identity.") {
            let parts: Vec<&str> = identity_id.split('.').collect();
            if parts.len() >= 2 {
                let entry = identities.entry(parts[0].to_string()).or_default();
                match parts[1] {
                    "useremail" => entry.email = val_clean.to_string(),
                    "fullName" => entry.name = val_clean.to_string(),
                    "smtpServer" => entry.smtp_server = Some(val_clean.to_string()),
                    _ => {}
                }
            }
        } else if let Some(account_id) = key.strip_prefix("mail.account.") {
            let parts: Vec<&str> = account_id.split('.').collect();
            if parts.len() >= 2 {
                let slot = match account_links.iter_mut().find(|(id, ..)| id == parts[0]) {
                    Some(found) => found,
                    None => {
                        account_links.push((parts[0].to_string(), String::new(), Vec::new()));
                        account_links.last_mut().expect("just pushed")
                    }
                };
                match parts[1] {
                    "server" => slot.1 = val_clean.to_string(),
                    "identities" => {
                        slot.2 = val_clean
                            .split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .collect();
                    }
                    _ => {}
                }
            }
        }
    }

    // Identity ids in the order Thunderbird lists them, so a profile with several identities
    // migrates deterministically instead of in hash order.
    let ordered_identities: Vec<&String> = {
        let mut seen: Vec<&String> = Vec::new();
        for (_, _, ids) in &account_links {
            for id in ids {
                if identities.contains_key(id) && !seen.contains(&id) {
                    seen.push(id);
                }
            }
        }
        for id in identities.keys() {
            if !seen.contains(&id) {
                seen.push(id);
            }
        }
        seen
    };

    let mut accounts = Vec::new();
    let mut server_ids: Vec<&String> = servers.keys().collect();
    server_ids.sort();

    for sid in server_ids {
        let server = &servers[sid];
        let kind = server.kind.as_deref().unwrap_or("imap");
        if kind == "smtp" {
            // Outgoing host: reached through `mail.identity.idN.smtpServer`, not a standalone
            // account. Emitting one here produced an IMAP account pointed at the submission
            // port.
            continue;
        }
        if kind != "imap" {
            // `pop3` accounts have no ProviderType and cannot be represented; importing them
            // as IMAP would create an account that can never connect.
            tracing::warn!(server=%sid, kind=%kind, "skipping unsupported Thunderbird server type");
            continue;
        }

        let identities_for_server: Vec<&TbIdentity> = account_links
            .iter()
            .filter(|(_, srv, _)| srv == sid)
            .flat_map(|(_, _, ids)| ids.iter())
            .filter_map(|id| identities.get(id))
            .collect();

        let chosen: Vec<&TbIdentity> = if identities_for_server.is_empty() {
            ordered_identities
                .iter()
                .filter_map(|id| identities.get(*id))
                .collect()
        } else {
            identities_for_server
        };

        let mut produced = false;
        for identity in chosen {
            let user = if server.user.is_empty() {
                identity.email.clone()
            } else {
                server.user.clone()
            };
            let email = if identity.email.is_empty() {
                user.clone()
            } else {
                identity.email.clone()
            };
            let smtp = identity
                .smtp_server
                .as_ref()
                .and_then(|id| servers.get(id))
                .filter(|s| s.host != server.host || s.port != server.port);
            accounts.push(MigratedAccount {
                name: if identity.name.is_empty() {
                    user.clone()
                } else {
                    identity.name.clone()
                },
                email,
                incoming_host: server.host.clone(),
                incoming_port: server.port.unwrap_or(993),
                incoming_user: user,
                provider_type: ProviderType::Imap,
                smtp_host: smtp.map(|s| s.host.clone()),
                smtp_port: smtp.and_then(|s| s.port),
                smtp_user: smtp.map(|s| s.user.clone()).filter(|u| !u.is_empty()),
            });
            produced = true;
        }
        if produced {
            continue;
        }

        // A server with no identity at all still has a login name worth importing.
        let user = server.user.clone();
        if server.host.is_empty() || user.is_empty() {
            continue;
        }
        accounts.push(MigratedAccount {
            name: user.clone(),
            email: user.clone(),
            incoming_host: server.host.clone(),
            incoming_port: server.port.unwrap_or(993),
            incoming_user: user,
            provider_type: ProviderType::Imap,
            smtp_host: None,
            smtp_port: None,
            smtp_user: None,
        });
    }

    accounts
}

#[derive(Default)]
struct TbServer {
    host: String,
    user: String,
    port: Option<u16>,
    kind: Option<String>,
}

#[derive(Default)]
struct TbIdentity {
    name: String,
    email: String,
    smtp_server: Option<String>,
}

/// Streaming mbox parser to slice raw MIME messages on `\nFrom ` boundary
pub fn parse_mbox_data(raw: &[u8]) -> Vec<Vec<u8>> {
    let mut messages = Vec::new();
    let mut start = 0;
    let len = raw.len();

    let from_marker = b"\nFrom ";
    let from_start_marker = b"From ";

    if raw.starts_with(from_start_marker) {
        // Skip first From line
        if let Some(pos) = memchr::memchr(b'\n', raw) {
            start = pos + 1;
        }
    }

    while start < len {
        let next_envelope = memchr::memmem::find(&raw[start..], from_marker);
        match next_envelope {
            Some(rel_idx) => {
                let end = start + rel_idx;
                let slice = &raw[start..end];
                if !slice.trim_ascii().is_empty() {
                    messages.push(unescape_mboxrd(slice));
                }
                // Skip the \nFrom ... line
                let after_marker = end + 1;
                if let Some(nl_idx) = memchr::memchr(b'\n', &raw[after_marker..]) {
                    start = after_marker + nl_idx + 1;
                } else {
                    break;
                }
            }
            None => {
                let slice = &raw[start..];
                if !slice.trim_ascii().is_empty() {
                    messages.push(unescape_mboxrd(slice));
                }
                break;
            }
        }
    }

    messages
}

fn unescape_mboxrd(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if (i == 0 || raw[i - 1] == b'\n') && raw[i..].starts_with(b">From ") {
            i += 1; // Strip leading >
        }
        if i < raw.len() {
            out.push(raw[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_profiles_ini() {
        let ini = r#"
[General]
StartWithLastProfile=1

[Profile0]
Name=default-release
IsRelative=1
Path=Profiles/x1y2z3.default-release
Default=1
"#;
        let base = Path::new("/home/user/.thunderbird");
        let profiles = parse_profiles_ini(ini, base);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "default-release");
        assert!(profiles[0].is_default);
        assert_eq!(
            profiles[0].path,
            base.join("Profiles/x1y2z3.default-release")
        );
    }

    #[test]
    fn test_parse_thunderbird_prefs() {
        let prefs = r#"
user_pref("mail.server.server1.hostname", "imap.fastmail.com");
user_pref("mail.server.server1.userName", "user@fastmail.com");
user_pref("mail.server.server1.port", 993);
user_pref("mail.server.server1.type", "imap");
user_pref("mail.identity.id1.fullName", "Fastmail User");
user_pref("mail.identity.id1.useremail", "user@fastmail.com");
"#;
        let accounts = parse_thunderbird_prefs(prefs);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].incoming_host, "imap.fastmail.com");
        assert_eq!(accounts[0].incoming_port, 993);
        assert_eq!(accounts[0].incoming_user, "user@fastmail.com");
        assert_eq!(accounts[0].email, "user@fastmail.com");
        assert_eq!(accounts[0].name, "Fastmail User");
    }

    /// Each identity must be bound to the account that references it. The previous version
    /// picked `identities.values().next()` from a hash map for *every* server, so this
    /// two-account profile imported the same address twice and one of them pointed at the
    /// other account's server.
    #[test]
    fn test_identities_are_bound_to_their_own_account() {
        let prefs = r#"
user_pref("mail.server.server1.hostname", "imap.fastmail.com");
user_pref("mail.server.server1.userName", "alice@fastmail.com");
user_pref("mail.server.server1.port", 993);
user_pref("mail.server.server1.type", "imap");
user_pref("mail.server.server2.hostname", "imap.gmail.com");
user_pref("mail.server.server2.userName", "bob@gmail.com");
user_pref("mail.server.server2.port", 993);
user_pref("mail.server.server2.type", "imap");
user_pref("mail.identity.id1.fullName", "Alice");
user_pref("mail.identity.id1.useremail", "alice@fastmail.com");
user_pref("mail.identity.id2.fullName", "Bob");
user_pref("mail.identity.id2.useremail", "bob@gmail.com");
user_pref("mail.account.account1.server", "server1");
user_pref("mail.account.account1.identities", "id1");
user_pref("mail.account.account2.server", "server2");
user_pref("mail.account.account2.identities", "id2");
"#;
        let accounts = parse_thunderbird_prefs(prefs);
        assert_eq!(accounts.len(), 2);
        let alice = accounts
            .iter()
            .find(|a| a.incoming_host == "imap.fastmail.com")
            .expect("alice's account");
        let bob = accounts
            .iter()
            .find(|a| a.incoming_host == "imap.gmail.com")
            .expect("bob's account");
        assert_eq!(alice.email, "alice@fastmail.com");
        assert_eq!(alice.name, "Alice");
        assert_eq!(bob.email, "bob@gmail.com");
        assert_eq!(bob.name, "Bob");
    }

    /// The outgoing host is a `mail.server.serverN` whose `type` is `smtp`. Importing it as
    /// a standalone account created an IMAP account pointed at the submission port.
    #[test]
    fn test_smtp_server_is_not_imported_as_an_account() {
        let prefs = r#"
user_pref("mail.server.server1.hostname", "imap.example.com");
user_pref("mail.server.server1.userName", "alice@example.com");
user_pref("mail.server.server1.port", 993);
user_pref("mail.server.server1.type", "imap");
user_pref("mail.server.server2.hostname", "smtp.example.com");
user_pref("mail.server.server2.userName", "alice@example.com");
user_pref("mail.server.server2.port", 465);
user_pref("mail.server.server2.type", "smtp");
user_pref("mail.identity.id1.fullName", "Alice");
user_pref("mail.identity.id1.useremail", "alice@example.com");
user_pref("mail.identity.id1.smtpServer", "server2");
user_pref("mail.account.account1.server", "server1");
user_pref("mail.account.account1.identities", "id1");
"#;
        let accounts = parse_thunderbird_prefs(prefs);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].incoming_host, "imap.example.com");
        assert_eq!(accounts[0].smtp_host.as_deref(), Some("smtp.example.com"));
        assert_eq!(accounts[0].smtp_port, Some(465));
    }

    /// POP3 has no `ProviderType`. Emitting one as IMAP yields an account that can never
    /// connect, silently "migrating" the user's mail into a dead entry.
    #[test]
    fn test_pop3_server_is_skipped() {
        let prefs = r#"
user_pref("mail.server.server1.hostname", "pop.example.com");
user_pref("mail.server.server1.userName", "old@example.com");
user_pref("mail.server.server1.port", 995);
user_pref("mail.server.server1.type", "pop3");
user_pref("mail.identity.id1.useremail", "old@example.com");
"#;
        assert!(parse_thunderbird_prefs(prefs).is_empty());
    }

    /// Output order must not depend on hash iteration order, or repeated migrations of the
    /// same profile would reshuffle the account list.
    #[test]
    fn test_migration_order_is_deterministic() {
        let prefs = r#"
user_pref("mail.server.server1.hostname", "imap.b.example.com");
user_pref("mail.server.server1.type", "imap");
user_pref("mail.server.server1.userName", "b@example.com");
user_pref("mail.server.server2.hostname", "imap.a.example.com");
user_pref("mail.server.server2.type", "imap");
user_pref("mail.server.server2.userName", "a@example.com");
user_pref("mail.identity.id1.useremail", "b@example.com");
user_pref("mail.identity.id2.useremail", "a@example.com");
user_pref("mail.account.account1.server", "server1");
user_pref("mail.account.account1.identities", "id1");
user_pref("mail.account.account2.server", "server2");
user_pref("mail.account.account2.identities", "id2");
"#;
        let first: Vec<String> = parse_thunderbird_prefs(prefs)
            .into_iter()
            .map(|a| a.incoming_host)
            .collect();
        assert_eq!(first, vec!["imap.b.example.com", "imap.a.example.com"]);
        for _ in 0..8 {
            let again: Vec<String> = parse_thunderbird_prefs(prefs)
                .into_iter()
                .map(|a| a.incoming_host)
                .collect();
            assert_eq!(again, first);
        }
    }

    #[test]
    fn test_parse_mbox_data() {
        let mbox = b"From MAILER-DAEMON Sun May 15 12:00:00 2026\r\n\
Subject: Message 1\r\n\
From: a@example.com\r\n\
\r\n\
Body 1\r\n\
\nFrom MAILER-DAEMON Sun May 15 12:01:00 2026\r\n\
Subject: Message 2\r\n\
From: b@example.com\r\n\
\r\n\
Body 2\r\n";

        let messages = parse_mbox_data(mbox);
        assert_eq!(messages.len(), 2);
        let msg1_str = String::from_utf8_lossy(&messages[0]);
        let msg2_str = String::from_utf8_lossy(&messages[1]);
        assert!(msg1_str.contains("Subject: Message 1"));
        assert!(msg2_str.contains("Subject: Message 2"));
    }
}
