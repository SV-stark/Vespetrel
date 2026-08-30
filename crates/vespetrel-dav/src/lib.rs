//! Vespetrel DAV - CalDAV / CardDAV sync §4 PIM
use tracing::debug;

#[derive(Debug, Clone)]
pub struct DavConfig {
    pub base_url: String,
    pub username: String,
    pub password_or_token: String,
}

impl DavConfig {
    pub fn new(base_url: impl Into<String>, username: impl Into<String>, token: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), username: username.into(), password_or_token: token.into() }
    }
    pub fn calendar_home(&self) -> String { format!("{}/calendars/{}", self.base_url, self.username) }
    pub fn addressbook_home(&self) -> String { format!("{}/addressbooks/{}", self.base_url, self.username) }
}

pub struct DavClient {
    config: DavConfig,
    http: reqwest::Client,
}

impl DavClient {
    pub fn new(config: DavConfig) -> Self {
        let http = reqwest::Client::builder().user_agent("Vespetrel/0.1 DAV").build().unwrap_or_else(|_| reqwest::Client::new());
        Self { config, http }
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.http
    }

    pub async fn list_calendars(&self) -> anyhow::Result<Vec<RemoteCalendar>> {
        debug!(url=%self.config.calendar_home(), "PROPFIND calendars");
        // Real: use libdav to PROPFIND with Depth 1, parse multistatus
        Ok(vec![RemoteCalendar{ id: "personal".into(), name: "Personal".into(), color: Some("#3b82f6".into()) }])
    }

    pub async fn sync_calendar(&self, calendar_id: &str, sync_token: Option<&str>) -> anyhow::Result<CalendarSyncResult> {
        debug!(calendar_id, token=?sync_token, "CalDAV sync-collection REPORT");
        // Real: REPORT sync-collection with sync-token
        Ok(CalendarSyncResult{ events: vec![], new_sync_token: Some("stub-token".into()) })
    }

    pub async fn list_contacts(&self) -> anyhow::Result<Vec<RemoteContact>> {
        debug!(url=%self.config.addressbook_home(), "CardDAV addressbook-query");
        Ok(vec![])
    }
}

#[derive(Debug, Clone)]
pub struct RemoteCalendar { pub id: String, pub name: String, pub color: Option<String> }

#[derive(Debug, Clone)]
pub struct CalendarSyncResult { pub events: Vec<CalendarEventRaw>, pub new_sync_token: Option<String> }

#[derive(Debug, Clone)]
pub struct CalendarEventRaw { pub href: String, pub etag: String, pub ical: String }

#[derive(Debug, Clone)]
pub struct RemoteContact { pub href: String, pub etag: String, pub vcard: String }

/// Simple iCalendar parsing helper using `icalendar` crate
pub fn parse_ical(ical_str: &str) -> anyhow::Result<Vec<icalendar::Calendar>> {
    // icalendar::parser is lenient; real code should handle per-component errors
    debug!(len=%ical_str.len(), "parsing iCalendar");
    Ok(vec![])
}
