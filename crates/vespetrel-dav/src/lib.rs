//! Vespetrel DAV - CalDAV / CardDAV sync §4 PIM
use tracing::debug;

#[derive(Clone)]
pub struct DavConfig {
    pub base_url: String,
    pub username: String,
    pub password_or_token: String,
}

impl std::fmt::Debug for DavConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DavConfig")
            .field("base_url", &self.base_url)
            .field("username", &self.username)
            .field("password_or_token", &"[REDACTED]")
            .finish()
    }
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

        if self.config.base_url.starts_with("http") && !self.config.password_or_token.is_empty() {
            let propfind_method =
                reqwest::Method::from_bytes(b"PROPFIND").unwrap_or(reqwest::Method::POST);
            let resp = self
                .http
                .request(propfind_method, &url)
                .basic_auth(&self.config.username, Some(&self.config.password_or_token))
                .header("Depth", "1")
                .header("Content-Type", "application/xml; charset=utf-8")
                .body(Self::build_propfind_xml())
                .send()
                .await?;

            let xml = resp.error_for_status()?.text().await?;
            let mut calendars = Vec::new();
            for block in find_xml_elements(&xml, "response") {
                let name = find_first_xml_element(block, "displayname")
                    .unwrap_or("")
                    .trim();
                let href = find_first_xml_element(block, "href").unwrap_or("").trim();
                if !name.is_empty() {
                    calendars.push(RemoteCalendar {
                        id: href.trim_matches('/').to_string(),
                        name: name.to_string(),
                        color: Some("#3b82f6".into()),
                    });
                }
            }
            return Ok(calendars);
        }

        #[cfg(any(test, feature = "mock"))]
        {
            Ok(vec![RemoteCalendar {
                id: "personal".into(),
                name: "Personal".into(),
                color: Some("#3b82f6".into()),
            }])
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

        if self.config.base_url.starts_with("http") && !self.config.password_or_token.is_empty() {
            let report_method =
                reqwest::Method::from_bytes(b"REPORT").unwrap_or(reqwest::Method::POST);
            let body = Self::build_sync_report_xml(sync_token);
            let resp = self
                .http
                .request(report_method, &url)
                .basic_auth(&self.config.username, Some(&self.config.password_or_token))
                .header("Content-Type", "application/xml; charset=utf-8")
                .body(body)
                .send()
                .await?;

            let xml = resp.error_for_status()?.text().await?;
            let mut events = Vec::new();
            for block in find_xml_elements(&xml, "response") {
                let ical = find_first_xml_element(block, "calendar-data")
                    .unwrap_or("")
                    .trim();
                let href = find_first_xml_element(block, "href").unwrap_or("").trim();
                let etag = find_first_xml_element(block, "getetag")
                    .unwrap_or("1")
                    .trim()
                    .trim_matches('"');
                if !ical.is_empty() {
                    events.push(CalendarEventRaw {
                        href: href.to_string(),
                        etag: etag.to_string(),
                        ical: ical.to_string(),
                    });
                }
            }
            let new_tok = find_first_xml_element(&xml, "sync-token").map(|s| s.trim().to_string());
            return Ok(CalendarSyncResult {
                events,
                new_sync_token: new_tok.or_else(|| Some("sync-token-active".into())),
            });
        }

        #[cfg(any(test, feature = "mock"))]
        {
            Ok(CalendarSyncResult {
                events: vec![],
                new_sync_token: Some("sync-token-1".into()),
            })
        }

        #[cfg(not(any(test, feature = "mock")))]
        {
            anyhow::bail!("CalDAV sync failed: server unreachable or invalid credentials");
        }
    }

    pub async fn list_contacts(&self) -> anyhow::Result<Vec<RemoteContact>> {
        let url = self.config.addressbook_home();
        debug!(url=%url, "CardDAV addressbook-query");

        if self.config.base_url.starts_with("http") && !self.config.password_or_token.is_empty() {
            let report_method =
                reqwest::Method::from_bytes(b"REPORT").unwrap_or(reqwest::Method::POST);
            let body = Self::build_carddav_query_xml();
            let resp = self
                .http
                .request(report_method, &url)
                .basic_auth(&self.config.username, Some(&self.config.password_or_token))
                .header("Depth", "1")
                .header("Content-Type", "application/xml; charset=utf-8")
                .body(body)
                .send()
                .await?;

            let xml = resp.error_for_status()?.text().await?;
            let mut contacts = Vec::new();
            for block in find_xml_elements(&xml, "response") {
                let vcard = find_first_xml_element(block, "address-data")
                    .unwrap_or("")
                    .trim();
                let href = find_first_xml_element(block, "href").unwrap_or("").trim();
                let etag = find_first_xml_element(block, "getetag")
                    .unwrap_or("1")
                    .trim()
                    .trim_matches('"');
                if !vcard.is_empty() {
                    contacts.push(RemoteContact {
                        href: href.trim_matches('/').to_string(),
                        etag: etag.to_string(),
                        vcard: vcard.to_string(),
                    });
                }
            }
            return Ok(contacts);
        }

        #[cfg(any(test, feature = "mock"))]
        {
            Ok(vec![])
        }

        #[cfg(not(any(test, feature = "mock")))]
        {
            anyhow::bail!(
                "CardDAV list_contacts failed: server unreachable or invalid credentials"
            );
        }
    }

    /// Create or update a VEVENT on the CalDAV server via HTTP PUT
    pub async fn create_or_update_event(
        &self,
        calendar_id: &str,
        event_uid: &str,
        ical_data: &str,
    ) -> anyhow::Result<()> {
        let url = format!(
            "{}/{}/{}.ics",
            self.config.calendar_home(),
            calendar_id,
            event_uid
        );
        debug!(url=%url, "CalDAV PUT event");

        if self.config.base_url.starts_with("http") && !self.config.password_or_token.is_empty() {
            let resp = self
                .http
                .put(&url)
                .basic_auth(&self.config.username, Some(&self.config.password_or_token))
                .header("Content-Type", "text/calendar; charset=utf-8")
                .body(ical_data.to_string())
                .send()
                .await?;
            resp.error_for_status()?;
        }
        Ok(())
    }

    /// Delete an event from the CalDAV server via HTTP DELETE
    pub async fn delete_event(&self, calendar_id: &str, event_uid: &str) -> anyhow::Result<()> {
        let url = format!(
            "{}/{}/{}.ics",
            self.config.calendar_home(),
            calendar_id,
            event_uid
        );
        debug!(url=%url, "CalDAV DELETE event");

        if self.config.base_url.starts_with("http") && !self.config.password_or_token.is_empty() {
            let resp = self
                .http
                .delete(&url)
                .basic_auth(&self.config.username, Some(&self.config.password_or_token))
                .send()
                .await?;
            if resp.status().is_client_error() && resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(());
            }
            resp.error_for_status()?;
        }
        Ok(())
    }

    /// Create or update a VCARD on the CardDAV server via HTTP PUT
    pub async fn create_or_update_contact(
        &self,
        contact_uid: &str,
        vcard_data: &str,
    ) -> anyhow::Result<()> {
        let url = format!("{}/{}.vcf", self.config.addressbook_home(), contact_uid);
        debug!(url=%url, "CardDAV PUT contact");

        if self.config.base_url.starts_with("http") && !self.config.password_or_token.is_empty() {
            let resp = self
                .http
                .put(&url)
                .basic_auth(&self.config.username, Some(&self.config.password_or_token))
                .header("Content-Type", "text/vcard; charset=utf-8")
                .body(vcard_data.to_string())
                .send()
                .await?;
            resp.error_for_status()?;
        }
        Ok(())
    }

    /// Delete a contact from the CardDAV server via HTTP DELETE
    pub async fn delete_contact(&self, contact_uid: &str) -> anyhow::Result<()> {
        let url = format!("{}/{}.vcf", self.config.addressbook_home(), contact_uid);
        debug!(url=%url, "CardDAV DELETE contact");

        if self.config.base_url.starts_with("http") && !self.config.password_or_token.is_empty() {
            let resp = self
                .http
                .delete(&url)
                .basic_auth(&self.config.username, Some(&self.config.password_or_token))
                .send()
                .await?;
            if resp.status().is_client_error() && resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(());
            }
            resp.error_for_status()?;
        }
        Ok(())
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

/// Finds all occurrences of an XML element with the given local tag name (ignoring any namespace prefix).
/// Returns the inner content of each matching element.
pub fn find_xml_elements<'a>(xml: &'a str, local_name: &str) -> Vec<&'a str> {
    let mut results = Vec::new();
    let mut cursor = 0;
    while cursor < xml.len() {
        let start_bracket = match xml[cursor..].find('<') {
            Some(pos) => cursor + pos,
            None => break,
        };
        if xml[start_bracket..].starts_with("</")
            || xml[start_bracket..].starts_with("<?")
            || xml[start_bracket..].starts_with("<!")
        {
            cursor = start_bracket + 2;
            continue;
        }
        let end_bracket = match xml[start_bracket..].find('>') {
            Some(pos) => start_bracket + pos,
            None => break,
        };
        let tag_header = xml[start_bracket + 1..end_bracket].trim();
        let tag_ident = tag_header.split_whitespace().next().unwrap_or("");
        let tag_local = tag_ident.split(':').last().unwrap_or(tag_ident);

        if tag_local.eq_ignore_ascii_case(local_name) {
            if tag_header.ends_with('/') {
                results.push("");
                cursor = end_bracket + 1;
                continue;
            }
            let mut search_pos = end_bracket + 1;
            let mut found = false;
            while let Some(close_start) = xml[search_pos..].find("</") {
                let abs_close_start = search_pos + close_start;
                if let Some(close_end) = xml[abs_close_start..].find('>') {
                    let abs_close_end = abs_close_start + close_end;
                    let close_header = xml[abs_close_start + 2..abs_close_end].trim();
                    let close_local = close_header.split(':').last().unwrap_or(close_header);
                    if close_local.eq_ignore_ascii_case(local_name) {
                        results.push(&xml[end_bracket + 1..abs_close_start]);
                        cursor = abs_close_end + 1;
                        found = true;
                        break;
                    } else {
                        search_pos = abs_close_end + 1;
                    }
                } else {
                    break;
                }
            }
            if !found {
                cursor = end_bracket + 1;
            }
        } else {
            cursor = end_bracket + 1;
        }
    }
    results
}

/// Extract first element's inner text by local name
pub fn find_first_xml_element<'a>(xml: &'a str, local_name: &str) -> Option<&'a str> {
    find_xml_elements(xml, local_name).into_iter().next()
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
        return naive_date
            .and_hms_opt(0, 0, 0)
            .map(|naive| chrono::DateTime::from_naive_utc_and_offset(naive, chrono::Utc));
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
            } else if let Some(dt) = line
                .strip_prefix("DTSTART")
                .and_then(|_| line.split_once(':'))
                .and_then(|(_, val)| parse_ical_datetime(val))
            {
                start = dt;
            } else if let Some(dt) = line
                .strip_prefix("DTEND")
                .and_then(|_| line.split_once(':'))
                .and_then(|(_, val)| parse_ical_datetime(val))
            {
                end = dt;
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

    #[test]
    fn test_find_xml_elements_namespace_agnostic() {
        let sample_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<multistatus xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <response>
    <href>/calendars/user/work/</href>
    <propstat>
      <prop>
        <displayname>Work Calendar</displayname>
        <C:calendar-data>BEGIN:VCALENDAR...</C:calendar-data>
        <getetag>"etag-12345"</getetag>
      </prop>
      <status>HTTP/1.1 200 OK</status>
    </propstat>
  </response>
  <D:response xmlns:D="DAV:">
    <D:href>/calendars/user/home/</D:href>
    <D:displayname>Home</D:displayname>
  </D:response>
</multistatus>"#;

        let responses = find_xml_elements(sample_xml, "response");
        assert_eq!(responses.len(), 2);

        let href0 = find_first_xml_element(responses[0], "href").unwrap();
        assert_eq!(href0, "/calendars/user/work/");
        let name0 = find_first_xml_element(responses[0], "displayname").unwrap();
        assert_eq!(name0, "Work Calendar");
        let cal_data = find_first_xml_element(responses[0], "calendar-data").unwrap();
        assert_eq!(cal_data, "BEGIN:VCALENDAR...");

        let href1 = find_first_xml_element(responses[1], "href").unwrap();
        assert_eq!(href1, "/calendars/user/home/");
        let name1 = find_first_xml_element(responses[1], "displayname").unwrap();
        assert_eq!(name1, "Home");
    }
}
