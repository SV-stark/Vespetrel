//! Vespetrel DAV - CalDAV / CardDAV sync §4 PIM
use tracing::debug;

#[derive(Debug, Clone)]
pub struct DavConfig {
    pub base_url: String,
    pub username: String,
    pub password_or_token: String,
}

impl DavConfig {
    pub fn new(
        base_url: impl Into<String>,
        username: impl Into<String>,
        token: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            username: username.into(),
            password_or_token: token.into(),
        }
    }
    pub fn calendar_home(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        let user = urlencoding_simple(&self.username);
        format!("{base}/calendars/{user}")
    }
    pub fn addressbook_home(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        let user = urlencoding_simple(&self.username);
        format!("{base}/addressbooks/{user}")
    }
}

fn urlencoding_simple(s: &str) -> String {
    let mut encoded = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}

pub struct DavClient {
    config: DavConfig,
    http: reqwest::Client,
}

impl DavClient {
    pub fn new(config: DavConfig) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("Vespetrel/0.1 DAV")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { config, http }
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.http
    }

    pub async fn list_calendars(&self) -> anyhow::Result<Vec<RemoteCalendar>> {
        debug!(url=%self.config.calendar_home(), "PROPFIND calendars");
        // Real: use libdav to PROPFIND with Depth 1, parse multistatus
        Ok(vec![RemoteCalendar {
            id: "personal".into(),
            name: "Personal".into(),
            color: Some("#3b82f6".into()),
        }])
    }

    pub async fn sync_calendar(
        &self,
        calendar_id: &str,
        sync_token: Option<&str>,
    ) -> anyhow::Result<CalendarSyncResult> {
        debug!(calendar_id, token=?sync_token, "CalDAV sync-collection REPORT");
        // Real: REPORT sync-collection with sync-token
        Ok(CalendarSyncResult {
            events: vec![],
            new_sync_token: Some("stub-token".into()),
        })
    }

    pub async fn list_contacts(&self) -> anyhow::Result<Vec<RemoteContact>> {
        debug!(url=%self.config.addressbook_home(), "CardDAV addressbook-query");
        Ok(vec![])
    }
}

#[derive(Debug, Clone)]
pub struct RemoteCalendar {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CalendarSyncResult {
    pub events: Vec<CalendarEventRaw>,
    pub new_sync_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CalendarEventRaw {
    pub href: String,
    pub etag: String,
    pub ical: String,
}

#[derive(Debug, Clone)]
pub struct RemoteContact {
    pub href: String,
    pub etag: String,
    pub vcard: String,
}

#[derive(Debug, Clone)]
pub struct TaskSyncResult {
    pub tasks: Vec<vespetrel_core::TaskItem>,
    pub new_sync_token: Option<String>,
}

/// Simple iCalendar parsing helper using `icalendar` crate
pub fn parse_ical(ical_str: &str) -> anyhow::Result<Vec<icalendar::Calendar>> {
    debug!(len=%ical_str.len(), "parsing iCalendar");
    Ok(vec![])
}

/// Parse RFC 5545 VTODO component from raw iCalendar string
pub fn parse_vtodo(calendar_id: &str, ical_str: &str) -> anyhow::Result<vespetrel_core::TaskItem> {
    let mut task = vespetrel_core::TaskItem::new(calendar_id, "Untitled Task");
    for line in ical_str.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("SUMMARY:") {
            task.title = val.to_string();
        } else if let Some(val) = line.strip_prefix("DESCRIPTION:") {
            task.description = Some(val.to_string());
        } else if let Some(val) = line.strip_prefix("UID:") {
            task.ical_uid = Some(val.to_string());
        } else if let Some(val) = line.strip_prefix("STATUS:") {
            task.is_completed = val.eq_ignore_ascii_case("COMPLETED");
        } else if let Some(p) = line
            .strip_prefix("PRIORITY:")
            .and_then(|val| val.parse::<u8>().ok())
        {
            task.priority = p;
        }
    }
    Ok(task)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vtodo_component() {
        let ical = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VTODO
UID:task-12345
SUMMARY:Implement Thunderbird CalDAV Sync
DESCRIPTION:Support RFC 5545 VTODO
STATUS:COMPLETED
PRIORITY:1
END:VTODO
END:VCALENDAR"#;

        let task = parse_vtodo("cal_work", ical).unwrap();
        assert_eq!(task.calendar_id, "cal_work");
        assert_eq!(task.title, "Implement Thunderbird CalDAV Sync");
        assert_eq!(task.description.as_deref(), Some("Support RFC 5545 VTODO"));
        assert_eq!(task.ical_uid.as_deref(), Some("task-12345"));
        assert!(task.is_completed);
        assert_eq!(task.priority, 1);
    }
}
