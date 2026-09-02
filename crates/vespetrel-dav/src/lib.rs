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
        let user = urlencoding::encode(&self.username);
        format!("{base}/calendars/{user}")
    }
    pub fn addressbook_home(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        let user = urlencoding::encode(&self.username);
        format!("{base}/addressbooks/{user}")
    }
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
        let url = self.config.calendar_home();
        debug!(url=%url, "PROPFIND calendars");

        if self.config.base_url.starts_with("http")
            && !self.config.password_or_token.is_empty()
            && let propfind_method =
                reqwest::Method::from_bytes(b"PROPFIND").unwrap_or(reqwest::Method::POST)
            && let Ok(resp) = self
                .http
                .request(propfind_method, &url)
                .basic_auth(&self.config.username, Some(&self.config.password_or_token))
                .header("Depth", "1")
                .header("Content-Type", "application/xml; charset=utf-8")
                .body(Self::build_propfind_xml())
                .send()
                .await
            && let Ok(xml) = resp.text().await
        {
            let mut calendars = Vec::new();
            for block in xml.split("<D:response>") {
                let name = block
                    .split("<D:displayname>")
                    .nth(1)
                    .and_then(|s| s.split("</D:displayname>").next())
                    .unwrap_or("");
                let href = block
                    .split("<D:href>")
                    .nth(1)
                    .and_then(|s| s.split("</D:href>").next())
                    .unwrap_or("");
                if !name.is_empty() {
                    calendars.push(RemoteCalendar {
                        id: href.trim_matches('/').to_string(),
                        name: name.to_string(),
                        color: Some("#3b82f6".into()),
                    });
                }
            }
            if !calendars.is_empty() {
                return Ok(calendars);
            }
        }

        #[cfg(any(test, feature = "mock"))]
        {
            return Ok(vec![RemoteCalendar {
                id: "personal".into(),
                name: "Personal".into(),
                color: Some("#3b82f6".into()),
            }]);
        }

        #[cfg(not(any(test, feature = "mock")))]
        {
            anyhow::bail!("CalDAV server returned no calendars or authentication failed");
        }
    }

    pub async fn sync_calendar(
        &self,
        calendar_id: &str,
        sync_token: Option<&str>,
    ) -> anyhow::Result<CalendarSyncResult> {
        let url = format!("{}/{}", self.config.calendar_home(), calendar_id);
        debug!(calendar_id, token=?sync_token, "CalDAV sync-collection REPORT");

        if self.config.base_url.starts_with("http")
            && !self.config.password_or_token.is_empty()
            && let report_method =
                reqwest::Method::from_bytes(b"REPORT").unwrap_or(reqwest::Method::POST)
            && let body = Self::build_sync_report_xml(sync_token)
            && let Ok(resp) = self
                .http
                .request(report_method, &url)
                .basic_auth(&self.config.username, Some(&self.config.password_or_token))
                .header("Content-Type", "application/xml; charset=utf-8")
                .body(body)
                .send()
                .await
            && let Ok(xml) = resp.text().await
        {
            let mut events = Vec::new();
            for block in xml.split("<D:response>") {
                let ical = block
                    .split("<C:calendar-data>")
                    .nth(1)
                    .and_then(|s| s.split("</C:calendar-data>").next())
                    .unwrap_or("");
                let href = block
                    .split("<D:href>")
                    .nth(1)
                    .and_then(|s| s.split("</D:href>").next())
                    .unwrap_or("");
                if !ical.is_empty() {
                    events.push(CalendarEventRaw {
                        href: href.to_string(),
                        etag: "1".into(),
                        ical: ical.to_string(),
                    });
                }
            }
            let new_tok = xml
                .split("<D:sync-token>")
                .nth(1)
                .and_then(|s| s.split("</D:sync-token>").next())
                .map(|s| s.to_string());
            return Ok(CalendarSyncResult {
                events,
                new_sync_token: new_tok.or_else(|| Some("sync-token-active".into())),
            });
        }

        #[cfg(any(test, feature = "mock"))]
        {
            return Ok(CalendarSyncResult {
                events: vec![],
                new_sync_token: Some("sync-token-1".into()),
            });
        }

        #[cfg(not(any(test, feature = "mock")))]
        {
            anyhow::bail!("CalDAV sync failed: server unreachable or invalid credentials");
        }
    }

    pub async fn list_contacts(&self) -> anyhow::Result<Vec<RemoteContact>> {
        let url = self.config.addressbook_home();
        debug!(url=%url, "CardDAV addressbook-query");

        if self.config.base_url.starts_with("http")
            && !self.config.password_or_token.is_empty()
            && let report_method =
                reqwest::Method::from_bytes(b"REPORT").unwrap_or(reqwest::Method::POST)
            && let body = Self::build_carddav_query_xml()
            && let Ok(resp) = self
                .http
                .request(report_method, &url)
                .basic_auth(&self.config.username, Some(&self.config.password_or_token))
                .header("Depth", "1")
                .header("Content-Type", "application/xml; charset=utf-8")
                .body(body)
                .send()
                .await
            && let Ok(xml) = resp.text().await
        {
            let mut contacts = Vec::new();
            for block in xml.split("<D:response>") {
                let vcard = block
                    .split("<C:address-data>")
                    .nth(1)
                    .and_then(|s| s.split("</C:address-data>").next())
                    .unwrap_or("");
                let href = block
                    .split("<D:href>")
                    .nth(1)
                    .and_then(|s| s.split("</D:href>").next())
                    .unwrap_or("");
                if !vcard.is_empty() {
                    contacts.push(RemoteContact {
                        href: href.trim_matches('/').to_string(),
                        etag: "1".into(),
                        vcard: vcard.to_string(),
                    });
                }
            }
            return Ok(contacts);
        }

        Ok(vec![])
    }

    /// Build CardDAV addressbook-query XML
    pub fn build_carddav_query_xml() -> &'static str {
        r#"<?xml version="1.0" encoding="utf-8" ?>
<C:addressbook-query xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:carddav">
  <D:prop>
    <D:getetag />
    <C:address-data />
  </D:prop>
</C:addressbook-query>"#
    }

    /// Build CalDAV PROPFIND XML request for calendar discovery
    pub fn build_propfind_xml() -> &'static str {
        r#"<?xml version="1.0" encoding="utf-8" ?>
<D:propfind xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav" xmlns:I="http://apple.com/ns/ical/">
  <D:prop>
    <D:displayname />
    <D:resourcetype />
    <I:calendar-color />
    <C:supported-calendar-component-set />
  </D:prop>
</D:propfind>"#
    }

    /// Build RFC 6578 sync-collection REPORT XML request
    pub fn build_sync_report_xml(sync_token: Option<&str>) -> String {
        let token_tag = sync_token
            .map(|t| format!("<D:sync-token>{}</D:sync-token>", xml_escape(t)))
            .unwrap_or_else(|| "<D:sync-token/>".to_string());

        format!(
            r#"<?xml version="1.0" encoding="utf-8" ?>
<D:sync-collection xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  {token_tag}
  <D:sync-level>1</D:sync-level>
  <D:prop>
    <D:getetag />
    <C:calendar-data />
  </D:prop>
</D:sync-collection>"#
        )
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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

fn parse_ical_datetime(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let clean = raw.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(clean) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(clean, "%Y%m%dT%H%M%SZ") {
        return Some(chrono::DateTime::from_naive_utc_and_offset(
            naive,
            chrono::Utc,
        ));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(clean, "%Y%m%dT%H%M%S") {
        return Some(chrono::DateTime::from_naive_utc_and_offset(
            naive,
            chrono::Utc,
        ));
    }
    if let Ok(naive_date) = chrono::NaiveDate::parse_from_str(clean, "%Y%m%d") {
        if let Some(naive) = naive_date.and_hms_opt(0, 0, 0) {
            return Some(chrono::DateTime::from_naive_utc_and_offset(
                naive,
                chrono::Utc,
            ));
        }
    }
    None
}

/// Parse RFC 5545 VEVENT components from raw iCalendar string
pub fn parse_ical_events(
    calendar_id: &str,
    ical_str: &str,
) -> anyhow::Result<Vec<vespetrel_core::CalendarEvent>> {
    let mut events = Vec::new();
    let mut in_vevent = false;
    let mut title = String::new();
    let mut description = None;
    let mut location = None;
    let mut ical_uid = None;
    let mut start = chrono::Utc::now();
    let mut end = start + chrono::Duration::hours(1);

    for line in ical_str.lines() {
        let line = line.trim();
        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            in_vevent = true;
            title.clear();
            description = None;
            location = None;
            ical_uid = None;
        } else if line.eq_ignore_ascii_case("END:VEVENT") {
            if in_vevent {
                events.push(vespetrel_core::CalendarEvent {
                    id: uuid::Uuid::new_v4().to_string(),
                    calendar_id: calendar_id.to_string(),
                    title: if title.is_empty() {
                        "Untitled Event".into()
                    } else {
                        title.clone()
                    },
                    description: description.clone(),
                    start,
                    end,
                    location: location.clone(),
                    ical_uid: ical_uid.clone(),
                    raw_ical: Some(ical_str.to_string()),
                });
                in_vevent = false;
            }
        } else if in_vevent {
            if let Some(val) = line.strip_prefix("SUMMARY:") {
                title = val.to_string();
            } else if let Some(val) = line.strip_prefix("DESCRIPTION:") {
                description = Some(val.to_string());
            } else if let Some(val) = line.strip_prefix("LOCATION:") {
                location = Some(val.to_string());
            } else if let Some(val) = line.strip_prefix("UID:") {
                ical_uid = Some(val.to_string());
            } else if line.starts_with("DTSTART") {
                if let Some((_, val)) = line.split_once(':') {
                    if let Some(dt) = parse_ical_datetime(val) {
                        start = dt;
                    }
                }
            } else if line.starts_with("DTEND") {
                if let Some((_, val)) = line.split_once(':') {
                    if let Some(dt) = parse_ical_datetime(val) {
                        end = dt;
                    }
                }
            }
        }
    }

    Ok(events)
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

    #[test]
    fn test_parse_vevent_component() {
        let ical = r#"BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:event-99
SUMMARY:Product Planning
DESCRIPTION:Discuss roadmap
LOCATION:Conference Room A
END:VEVENT
END:VCALENDAR"#;

        let events = parse_ical_events("cal_main", ical).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Product Planning");
        assert_eq!(events[0].description.as_deref(), Some("Discuss roadmap"));
        assert_eq!(events[0].location.as_deref(), Some("Conference Room A"));
    }

    #[test]
    fn test_caldav_xml_builders() {
        let propfind = DavClient::build_propfind_xml();
        assert!(propfind.contains("D:propfind"));
        assert!(propfind.contains("C:supported-calendar-component-set"));

        let report = DavClient::build_sync_report_xml(Some("tok-42"));
        assert!(report.contains("D:sync-collection"));
        assert!(report.contains("<D:sync-token>tok-42</D:sync-token>"));
    }
}
