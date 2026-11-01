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

    /// Decode XML character references and normalise CRLF in element text.
    ///
    /// `find_xml_elements` returns the *raw* inner substring, and element text is
    /// entity-escaped by every conforming server. A contact with `NOTE:Ben & Jerry's`
    /// arrives as `&amp;` and was stored verbatim, so every later vCard/ICS parse saw the
    /// literal characters `& a m p ;`.
    pub fn xml_unescape(s: &str) -> String {
        if !s.contains('&') {
            // Still normalise line endings: servers may use bare CR.
            return s.replace("\r\n", "\n").replace('\r', "\n");
        }
        let mut out = String::with_capacity(s.len());
        let b = s.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            if b[i] != b'&' {
                // Copy one full UTF-8 char.
                let ch_len = s[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                out.push_str(&s[i..i + ch_len]);
                i += ch_len;
                continue;
            }
            let Some(semi_rel) = s[i..].find(';') else {
                out.push('&');
                i += 1;
                continue;
            };
            let semi = i + semi_rel;
            let name = &s[i + 1..semi];
            let replacement = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                // Numeric character reference: &#NN; or &#xHH; / &#XHH;
                _ => name.strip_prefix('#').and_then(|d| {
                    let (radix, digits) = match d.strip_prefix(['x', 'X']) {
                        Some(hex) => (16, hex),
                        None => (10, d),
                    };
                    if digits.is_empty() {
                        return None;
                    }
                    u32::from_str_radix(digits, radix)
                        .ok()
                        .and_then(char::from_u32)
                }),
            };
            match replacement {
                Some(c) => {
                    out.push(c);
                    i = semi + 1;
                }
                None => {
                    out.push('&');
                    i += 1;
                }
            }
        }
        out.replace("\r\n", "\n").replace('\r', "\n")
    }

    /// Build a URL under the calendar home, percent-encoding each path segment.
    ///
    /// The previous code interpolated `calendar_id` / `event_uid` raw, so a UID containing
    /// `/`, `?` or `#` (CalDAV servers generate those freely) produced a wrong URL, and a
    /// UID like `a/../../b` traversed the path.
    fn calendar_url(&self, calendar_id: &str, event_uid: Option<&str>) -> String {
        let home = self.config.calendar_home();
        let mut url = format!(
            "{home}/{}",
            urlencoding::encode(calendar_id.trim_matches('/'))
        );
        if let Some(uid) = event_uid {
            url.push('/');
            url.push_str(&urlencoding::encode(uid));
            url.push_str(".ics");
        }
        url
    }

    /// Build a URL under the address book home, percent-encoding each path segment.
    fn addressbook_url(&self, book_id: &str, contact_uid: Option<&str>) -> String {
        let home = self.config.addressbook_home();
        let mut url = format!("{home}/{}", urlencoding::encode(book_id.trim_matches('/')));
        if let Some(uid) = contact_uid {
            url.push('/');
            url.push_str(&urlencoding::encode(uid));
            url.push_str(".vcf");
        }
        url
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
                // Only real calendar collections. The PROPFIND body requests
                // <resourcetype/>, but the parser ignored it, so with Depth: 1 every
                // non-calendar collection under the home (a WebDAV directory, a shared
                // collection, an address book) was returned as a calendar and then failed
                // its sync-collection REPORT.
                if find_first_xml_element(block, "calendar").is_none() {
                    continue;
                }
                let name =
                    Self::xml_unescape(find_first_xml_element(block, "displayname").unwrap_or(""))
                        .trim()
                        .to_string();
                let href = find_first_xml_element(block, "href").unwrap_or("").trim();
                if name.is_empty() {
                    continue;
                }
                // Store the calendar *name*, not the whole href. `sync_calendar` builds its
                // URL from `calendar_home()` + id, so storing the full href produced
                // `https://host/calendars/alice/calendars/alice/work` — a 404 for every
                // calendar, so calendar sync could never succeed.
                let last_segment = href.split('/').rfind(|s| !s.is_empty()).unwrap_or("");
                let id = urlencoding::decode(last_segment)
                    .map(|c| c.into_owned())
                    .unwrap_or_else(|_| last_segment.to_string());
                calendars.push(RemoteCalendar {
                    id,
                    name,
                    color: Some("#3b82f6".into()),
                });
            }
            return Ok(calendars);
        }

        #[cfg(test)]
        {
            Ok(vec![RemoteCalendar {
                id: "personal".into(),
                name: "Personal".into(),
                color: Some("#3b82f6".into()),
            }])
        }

        #[cfg(not(test))]
        {
            anyhow::bail!("CalDAV server returned no calendars or authentication failed");
        }
    }

    pub async fn sync_calendar(
        &self,
        calendar_id: &str,
        sync_token: Option<&str>,
    ) -> anyhow::Result<CalendarSyncResult> {
        let url = self.calendar_url(calendar_id, None);
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
                let ical = Self::xml_unescape(
                    find_first_xml_element(block, "calendar-data").unwrap_or(""),
                )
                .trim()
                .to_string();
                let href = find_first_xml_element(block, "href").unwrap_or("").trim();
                // A 404 <propstat> has no calendar-data, meaning the object is gone. Emit a
                // tombstone with empty ical so the caller can delete the local copy; the
                // previous code skipped the block entirely, so remote deletions never
                // propagated and the local calendar grew forever.
                let etag = match find_first_xml_element(block, "getetag") {
                    // `find_first_xml_element` returns Some("") for a self-closing element,
                    // so `unwrap_or("1")` was dead and the ETag silently became "".
                    Some(v) if !v.trim().is_empty() => v.trim().trim_matches('"').to_string(),
                    _ => String::new(),
                };
                events.push(CalendarEventRaw {
                    href: href.to_string(),
                    etag,
                    ical,
                });
            }
            // Never fabricate a token. RFC 6578 §3.2 requires the server to return one, and
            // inventing a constant poisoned every subsequent sync: the client sent the fake
            // token back, the server rejected it (403/409), and calendar sync was permanently
            // broken with the real cause masked.
            let new_tok = find_first_xml_element(&xml, "sync-token")
                .map(|s| Self::xml_unescape(s).trim().to_string())
                .filter(|s| !s.is_empty());
            if new_tok.is_none() {
                tracing::warn!(
                    calendar_id,
                    "CalDAV response contained no sync-token; keeping the previous cursor and forcing a full re-read next time"
                );
            }
            return Ok(CalendarSyncResult {
                events,
                new_sync_token: new_tok.or_else(|| sync_token.map(|s| s.to_string())),
            });
        }

        #[cfg(test)]
        {
            Ok(CalendarSyncResult {
                events: vec![],
                new_sync_token: Some("sync-token-1".into()),
            })
        }

        #[cfg(not(test))]
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
                let vcard =
                    Self::xml_unescape(find_first_xml_element(block, "address-data").unwrap_or(""))
                        .trim()
                        .to_string();
                let href = find_first_xml_element(block, "href").unwrap_or("").trim();
                let etag = match find_first_xml_element(block, "getetag") {
                    Some(v) if !v.trim().is_empty() => v.trim().trim_matches('"').to_string(),
                    _ => String::new(),
                };
                if !vcard.is_empty() {
                    contacts.push(RemoteContact {
                        href: href.trim_matches('/').to_string(),
                        etag,
                        vcard,
                    });
                }
            }
            return Ok(contacts);
        }

        #[cfg(test)]
        {
            Ok(vec![])
        }

        #[cfg(not(test))]
        {
            anyhow::bail!(
                "CardDAV list_contacts failed: server unreachable or invalid credentials"
            );
        }
    }

    /// Create or update a VEVENT on the CalDAV server via HTTP PUT.
    ///
    /// `etag` enables optimistic concurrency: pass the ETag from the last read to make the
    /// write conditional (`If-Match`), or `None` for a create-only write (`If-None-Match: *`).
    /// The previous implementation sent an unconditional PUT, so a write racing another
    /// client silently clobbered the other version and a create could silently overwrite an
    /// existing resource.
    pub async fn create_or_update_event(
        &self,
        calendar_id: &str,
        event_uid: &str,
        ical_data: &str,
        etag: Option<&str>,
    ) -> anyhow::Result<()> {
        let url = self.calendar_url(calendar_id, Some(event_uid));
        debug!(url=%url, "CalDAV PUT event");

        if self.config.base_url.starts_with("http") && !self.config.password_or_token.is_empty() {
            let mut req = self
                .http
                .put(&url)
                .basic_auth(&self.config.username, Some(&self.config.password_or_token))
                .header("Content-Type", "text/calendar; charset=utf-8")
                .body(ical_data.to_string());
            match etag {
                Some(e) if !e.trim().is_empty() => {
                    req = req.header("If-Match", format!("\"{}\"", e.trim().trim_matches('"')));
                }
                _ => {
                    req = req.header("If-None-Match", "*");
                }
            }
            let resp = req.send().await?;
            if resp.status() == reqwest::StatusCode::PRECONDITION_FAILED {
                anyhow::bail!(
                    "CalDAV precondition failed: event {event_uid} was modified on the server"
                );
            }
            resp.error_for_status()?;
        }
        Ok(())
    }

    /// Delete an event from the CalDAV server via HTTP DELETE
    pub async fn delete_event(&self, calendar_id: &str, event_uid: &str) -> anyhow::Result<()> {
        let url = self.calendar_url(calendar_id, Some(event_uid));
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
        let url = self.addressbook_url("default", Some(contact_uid));
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
        let url = self.addressbook_url("default", Some(contact_uid));
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
        let tag_local = tag_ident.split(':').next_back().unwrap_or(tag_ident);

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
                    let close_local = close_header.split(':').next_back().unwrap_or(close_header);
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

/// RFC 5545 §3.1: unfold content lines.
///
/// A CRLF followed by a single WSP is a continuation of the previous line. Folding at 75
/// octets is mandatory and servers fold aggressively; without unfolding, a long SUMMARY is
/// truncated at the fold and the continuation is silently discarded.
pub fn unfold_ical(src: &str) -> Vec<String> {
    let normalized = src.replace("\r\n", "\n").replace('\r', "\n");
    let mut out: Vec<String> = Vec::new();
    for raw in normalized.lines() {
        if (raw.starts_with(' ') || raw.starts_with('\t')) && !out.is_empty() {
            // Keep the single leading WSP (it is significant content, not a separator).
            if let Some(last) = out.last_mut() {
                last.push_str(raw);
            }
        } else {
            out.push(raw.to_string());
        }
    }
    out
}

/// Split `NAME;PARAM=VAL:VALUE` into `(NAME, params, VALUE)`.
///
/// The previous code used `strip_prefix("SUMMARY:")`, which does not match
/// `SUMMARY;LANGUAGE=en:Hello`, so every parameterised property was silently dropped and the
/// event was stored as "Untitled Event". Thunderbird, Outlook and Google Calendar all emit
/// `;LANGUAGE=` and `;ALTREP=`.
pub fn ical_prop(line: &str) -> Option<(&str, &str, &str)> {
    let colon = line.find(':')?;
    let name_part = &line[..colon];
    let value = &line[colon + 1..];
    let name = name_part.split(';').next()?;
    let params = &name_part[name.len()..];
    Some((name, params, value))
}

/// Parse a DTSTART/DTEND value, honouring an explicit `TZID` and the UTC (`Z`) form.
///
/// A bare `YYYYMMDDTHHMMSS` with no `Z` and no `TZID` is a *floating* time: it denotes local
/// time at the viewer's location, not UTC. The previous code discarded the parameters and
/// reinterpreted everything as UTC, so a 9am New York meeting was stored 4 hours off.
pub fn parse_ical_datetime(value: &str, params: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let clean = value.trim();
    if clean.is_empty() {
        return None;
    }
    // Some servers emit a full RFC 3339 timestamp.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(clean) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    if clean.ends_with('Z') {
        return chrono::NaiveDateTime::parse_from_str(clean, "%Y%m%dT%H%M%SZ")
            .ok()
            .map(|n| chrono::DateTime::from_naive_utc_and_offset(n, chrono::Utc));
    }
    let naive = chrono::NaiveDateTime::parse_from_str(clean, "%Y%m%dT%H%M%S")
        .ok()
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(clean, "%Y%m%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
        })?;

    if let Some(tzid) = extract_tzid(params) {
        return resolve_tzid(&naive, &tzid);
    }
    // Floating time: no zone information available. Fall back to UTC rather than guessing.
    Some(chrono::DateTime::from_naive_utc_and_offset(
        naive,
        chrono::Utc,
    ))
}

fn extract_tzid(params: &str) -> Option<String> {
    params
        .split(';')
        .filter_map(|p| {
            p.trim()
                .strip_prefix("TZID=")
                .map(|v| v.trim_matches('"').to_string())
        })
        .next()
}

/// Resolve a TZID against the platform zone database.
///
/// `VTIMEZONE` components in the payload are not consulted: they define a custom zone and
/// honouring them requires a full RRULE-driven offset evaluator. A standard IANA id (what
/// every mainstream CalDAV server emits) is resolved exactly; an unknown custom id falls back
/// to UTC.
fn resolve_tzid(
    naive: &chrono::NaiveDateTime,
    tzid: &str,
) -> Option<chrono::DateTime<chrono::Utc>> {
    use std::str::FromStr;
    for cand in [tzid.to_string(), windows_tz_to_iana(tzid)] {
        if let Ok(tz) = <chrono_tz::Tz as FromStr>::from_str(&cand)
            && let Some(dt) = naive.and_local_timezone(tz).single()
        {
            return Some(dt.with_timezone(&chrono::Utc));
        }
    }
    Some(chrono::DateTime::from_naive_utc_and_offset(
        *naive,
        chrono::Utc,
    ))
}

/// Map the common Windows time-zone names to IANA ids.
fn windows_tz_to_iana(w: &str) -> String {
    match w {
        "Eastern Standard Time" => "America/New_York",
        "Central Standard Time" => "America/Chicago",
        "Mountain Standard Time" => "America/Denver",
        "Pacific Standard Time" => "America/Los_Angeles",
        "GMT Standard Time" => "Europe/London",
        "W. Europe Standard Time" => "Europe/Berlin",
        "Romance Standard Time" => "Europe/Paris",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "China Standard Time" => "Asia/Shanghai",
        "India Standard Time" => "Asia/Kolkata",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        other => other,
    }
    .to_string()
}

/// Decode RFC 5545 §3.3.11 escaped text (`\n`, `\,`, `\;`, `\\`).
pub fn decode_ical_text(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(',') => out.push(','),
            Some(';') => out.push(';'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Parse RFC 5545 VEVENT components from raw iCalendar string
pub fn parse_ical_events(
    calendar_id: &str,
    ical_str: &str,
) -> anyhow::Result<Vec<vespetrel_core::CalendarEvent>> {
    let mut events = Vec::new();
    // Reset the time fields on BEGIN:VEVENT too. Previously only the text fields were
    // cleared, so a VEVENT with no DTSTART silently inherited the previous event's start
    // and end, and a batch import stamped every event with the import time.
    let mut in_vevent = false;
    let mut title = String::new();
    let mut description = None;
    let mut location = None;
    let mut ical_uid = None;
    let mut start = chrono::Utc::now();
    let mut end = start + chrono::Duration::hours(1);

    for line in unfold_ical(ical_str) {
        // NB: do *not* trim here. RFC 5545 §3.1 folding is a leading space/tab on the
        // continuation line, and `trim()` would erase the very marker `unfold_ical` used.
        let line = line.trim_end();
        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            in_vevent = true;
            title.clear();
            description = None;
            location = None;
            ical_uid = None;
            start = chrono::Utc::now();
            end = start + chrono::Duration::hours(1);
            continue;
        }
        if line.eq_ignore_ascii_case("END:VEVENT") {
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
            continue;
        }
        if !in_vevent {
            continue;
        }
        let Some((name, params, val)) = ical_prop(line) else {
            continue;
        };
        match name.to_ascii_uppercase().as_str() {
            "SUMMARY" => title = decode_ical_text(val),
            "DESCRIPTION" => description = Some(decode_ical_text(val)),
            "LOCATION" => location = Some(decode_ical_text(val)),
            "UID" => ical_uid = Some(val.trim().to_string()),
            "DTSTART" => {
                if let Some(dt) = parse_ical_datetime(val, params) {
                    start = dt;
                }
            }
            "DTEND" => {
                if let Some(dt) = parse_ical_datetime(val, params) {
                    end = dt;
                }
            }
            _ => {}
        }
    }

    Ok(events)
}

/// Parse RFC 5545 VTODO component from raw iCalendar string
pub fn parse_vtodo(calendar_id: &str, ical_str: &str) -> anyhow::Result<vespetrel_core::TaskItem> {
    let mut task = vespetrel_core::TaskItem::new(calendar_id, "Untitled Task");
    // Scope to the VTODO component and take the *first* occurrence of each property. The
    // previous scan covered the whole calendar, so a VEVENT or VTIMEZONE appearing after
    // the VTODO overwrote the task's title, description, status and priority.
    let mut in_vtodo = false;
    for line in unfold_ical(ical_str) {
        // Do not trim: the leading WSP of a folded continuation is significant.
        let line = line.trim_end();
        if line.eq_ignore_ascii_case("BEGIN:VTODO") {
            in_vtodo = true;
            continue;
        }
        if line.eq_ignore_ascii_case("END:VTODO") {
            break;
        }
        if !in_vtodo {
            continue;
        }
        let Some((name, _params, val)) = ical_prop(line) else {
            continue;
        };
        match name.to_ascii_uppercase().as_str() {
            "SUMMARY" if task.title == "Untitled Task" => task.title = decode_ical_text(val),
            "DESCRIPTION" if task.description.is_none() => {
                task.description = Some(decode_ical_text(val))
            }
            "UID" if task.ical_uid.is_none() => task.ical_uid = Some(val.trim().to_string()),
            "STATUS" => {
                // A cancelled task is not pending either.
                task.is_completed =
                    val.eq_ignore_ascii_case("COMPLETED") || val.eq_ignore_ascii_case("CANCELLED");
            }
            "PRIORITY" => {
                if let Ok(p) = val.trim().parse::<u8>() {
                    task.priority = p;
                }
            }
            _ => {}
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

    /// Regression: `VTIMEZONE`/`VEVENT` after a `VTODO` overwrote the task's fields, and
    /// folded lines were truncated.
    #[test]
    fn parse_vtodo_is_scoped_and_unfolds() {
        // Built with concat! because a `\`-continued Rust string literal strips the leading
        // whitespace, which is exactly the fold marker under test.
        let ical = concat!(
            "BEGIN:VCALENDAR\r\n",
            "BEGIN:VTODO\r\n",
            "UID:t-1\r\n",
            "SUMMARY:A very long task title that the server\r\n",
            " folded across two lines\r\n",
            "STATUS:CANCELLED\r\n",
            "END:VTODO\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:e-1\r\n",
            "SUMMARY:Some unrelated event\r\n",
            "STATUS:NEEDS-ACTION\r\n",
            "PRIORITY:9\r\n",
            "END:VEVENT\r\n",
            "END:VCALENDAR",
        );
        let task = parse_vtodo("cal", ical).unwrap();
        assert_eq!(
            task.title,
            "A very long task title that the server folded across two lines"
        );
        assert_eq!(task.ical_uid.as_deref(), Some("t-1"));
        // CANCELLED is not pending.
        assert!(task.is_completed);
        // The VEVENT's properties must not leak in.
        assert_ne!(task.title, "Some unrelated event");
        assert_eq!(task.priority, 0);
    }

    /// Regression: `strip_prefix("SUMMARY:")` did not match `SUMMARY;LANGUAGE=en:...`, so
    /// parameterised properties were silently dropped and events became "Untitled Event".
    #[test]
    fn parse_ical_events_handles_parameters_and_folding() {
        let ical = concat!(
            "BEGIN:VCALENDAR\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:e1\r\n",
            "SUMMARY;LANGUAGE=en-GB:Quarterly review\r\n",
            " with a long description\r\n",
            "DTSTART;TZID=America/New_York:20260915T090000\r\n",
            "DTEND;TZID=America/New_York:20260915T100000\r\n",
            "END:VEVENT\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:e2\r\n",
            "END:VEVENT\r\n",
            "END:VCALENDAR",
        );
        let events = parse_ical_events("cal", ical).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].title, "Quarterly review with a long description");
        // 9am New York on 2026-09-15 is EDT (UTC-4), so 13:00 UTC.
        assert_eq!(events[0].start.to_rfc3339(), "2026-09-15T13:00:00+00:00");
        assert_eq!(events[0].end.to_rfc3339(), "2026-09-15T14:00:00+00:00");
        // A VEVENT with no DTSTART must not inherit the previous event's times.
        assert_ne!(events[1].start, events[0].start);
    }

    #[test]
    fn ical_prop_splits_name_params_and_value() {
        let (n, p, v) = ical_prop("SUMMARY;LANGUAGE=en:Hello").unwrap();
        assert_eq!(n, "SUMMARY");
        assert_eq!(p, ";LANGUAGE=en");
        assert_eq!(v, "Hello");
        let (n, _, v) = ical_prop("UID:plain").unwrap();
        assert_eq!((n, v), ("UID", "plain"));
        assert!(ical_prop("no-colon").is_none());
    }

    #[test]
    fn unfold_ical_joins_continuations() {
        let out = unfold_ical("A:one\r\n two\r\nB:three\r\n four\r\nC:five");
        assert_eq!(out, vec!["A:one two", "B:three four", "C:five"]);
    }

    /// Element text is entity-escaped by every conforming server; storing the raw substring
    /// left `&amp;` in every vCard and calendar body.
    #[test]
    fn xml_unescape_decodes_entities() {
        assert_eq!(
            DavClient::xml_unescape("NOTE:meet me at 5 &amp; 6"),
            "NOTE:meet me at 5 & 6"
        );
        assert_eq!(DavClient::xml_unescape("A &lt;tag&gt;"), "A <tag>");
        assert_eq!(
            DavClient::xml_unescape("quote &quot;q&quot;"),
            "quote \"q\""
        );
        assert_eq!(DavClient::xml_unescape("it&apos;s"), "it's");
        assert_eq!(DavClient::xml_unescape("cr&#13;lf"), "cr\nlf");
        assert_eq!(DavClient::xml_unescape("hex&#x41;"), "hexA");
        // An unknown entity is left alone rather than mangled.
        assert_eq!(DavClient::xml_unescape("a &bogus; b"), "a &bogus; b");
        // Non-ASCII survives.
        assert_eq!(DavClient::xml_unescape("Кай &amp; 邦"), "Кай & 邦");
    }

    /// Regression: `list_calendars` stored the whole href, and `sync_calendar` rebuilt the
    /// URL from `calendar_home()` + id, producing `.../calendars/alice/calendars/alice/work`.
    #[test]
    fn calendar_url_does_not_double_the_home_path() {
        let client = DavClient::new(DavConfig::new("https://dav.example.com/", "alice", "tok"));
        let url = client.calendar_url("work", None);
        assert_eq!(url, "https://dav.example.com/calendars/alice/work");
        assert!(!url.contains("alice/calendars/alice"));

        // Path segments are percent-encoded, so a UID with `/` or `?` cannot traverse.
        let ev = client.calendar_url("work", Some("a/../../b"));
        assert!(!ev.contains("/b.ics") || ev.contains("%2F"));
        assert!(!ev.contains("a/../"));
        let q = client.calendar_url("work", Some("x?y#z"));
        assert!(
            !q.contains('?'),
            "query separator leaked into the path: {q}"
        );
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
