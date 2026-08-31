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
pub fn parse_thunderbird_prefs(prefs_js: &str) -> Vec<MigratedAccount> {
    use ahash::AHashMap;
    let mut server_hosts: AHashMap<String, String> = AHashMap::new();
    let mut server_users: AHashMap<String, String> = AHashMap::new();
    let mut server_ports: AHashMap<String, u16> = AHashMap::new();
    let mut server_types: AHashMap<String, String> = AHashMap::new();
    let mut identities: AHashMap<String, (String, String)> = AHashMap::new(); // id -> (name, email)

    for line in prefs_js.lines() {
        let line = line.trim();
        if !line.starts_with("user_pref(\"") {
            continue;
        }
        let stripped = line
            .strip_prefix("user_pref(\"")
            .and_then(|s| s.strip_suffix(");"));
        if let Some((key, val)) = stripped.and_then(|s| s.split_once("\", ")) {
            let val_clean = val.trim_matches('"');
            if let Some(server_id) = key.strip_prefix("mail.server.") {
                let parts: Vec<&str> = server_id.split('.').collect();
                if parts.len() >= 2 {
                    let sid = parts[0];
                    match parts[1] {
                        "hostname" => {
                            server_hosts.insert(sid.to_string(), val_clean.to_string());
                        }
                        "userName" => {
                            server_users.insert(sid.to_string(), val_clean.to_string());
                        }
                        "port" => {
                            if let Ok(p) = val_clean.parse::<u16>() {
                                server_ports.insert(sid.to_string(), p);
                            }
                        }
                        "type" => {
                            server_types.insert(sid.to_string(), val_clean.to_string());
                        }
                        _ => {}
                    }
                }
            } else if let Some(identity_id) = key.strip_prefix("mail.identity.") {
                let parts: Vec<&str> = identity_id.split('.').collect();
                if parts.len() >= 2 {
                    let id = parts[0];
                    let entry = identities.entry(id.to_string()).or_default();
                    if parts[1] == "useremail" {
                        entry.1 = val_clean.to_string();
                    } else if parts[1] == "fullName" {
                        entry.0 = val_clean.to_string();
                    }
                }
            }
        }
    }

    let mut accounts = Vec::new();
    for (sid, host) in server_hosts {
        let user = server_users.get(&sid).cloned().unwrap_or_default();
        let port = server_ports.get(&sid).copied().unwrap_or(993);
        let _stype = server_types.get(&sid).map(|s| s.as_str()).unwrap_or("imap");
        let provider_type = ProviderType::Imap;

        let (name, email) = identities
            .values()
            .next()
            .cloned()
            .unwrap_or_else(|| (user.clone(), user.clone()));

        accounts.push(MigratedAccount {
            name: if name.is_empty() { user.clone() } else { name },
            email: if email.is_empty() {
                user.clone()
            } else {
                email
            },
            incoming_host: host,
            incoming_port: port,
            incoming_user: user,
            provider_type,
            smtp_host: None,
            smtp_port: None,
            smtp_user: None,
        });
    }

    accounts
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
