//! iCalendar Meeting Invitations & iTIP RSVP Engine (RFC 5545 / RFC 5546) §7 Phase 5
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RsvpStatus {
    NeedsAction,
    Accepted,
    Declined,
    Tentative,
}

impl RsvpStatus {
    pub fn as_partstat(&self) -> &'static str {
        match self {
            RsvpStatus::NeedsAction => "NEEDS-ACTION",
            RsvpStatus::Accepted => "ACCEPTED",
            RsvpStatus::Declined => "DECLINED",
            RsvpStatus::Tentative => "TENTATIVE",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingInvitation {
    pub uid: String,
    pub sequence: u32,
    pub summary: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub organizer_name: Option<String>,
    pub organizer_email: String,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
    pub status: RsvpStatus,
}

impl MeetingInvitation {
    /// Parse iCalendar invitation (VEVENT with METHOD:REQUEST)
    pub fn parse_ics(ics_str: &str, current_user_email: &str) -> anyhow::Result<Self> {
        let mut uid = None;
        let mut sequence = 0;
        let mut summary = "Meeting Invitation".to_string();
        let mut description = None;
        let mut location = None;
        let mut organizer_name = None;
        let mut organizer_email = String::new();
        let mut start_at = Utc::now();
        let mut end_at = Utc::now();
        let mut status = RsvpStatus::NeedsAction;

        let unfolded = unfold_ical(ics_str);
        for line in &unfolded {
            let line = line.trim();
            if let Some(val) = line.strip_prefix("UID:") {
                uid = Some(val.to_string());
            } else if let Some(val) = line.strip_prefix("SUMMARY:") {
                summary = val.to_string();
            } else if let Some(val) = line.strip_prefix("DESCRIPTION:") {
                description = Some(val.to_string());
            } else if let Some(val) = line.strip_prefix("LOCATION:") {
                location = Some(val.to_string());
            } else if let Some(val) = line.strip_prefix("SEQUENCE:") {
                if let Ok(seq) = val.parse::<u32>() {
                    sequence = seq;
                }
            } else if let Some(val) = line.strip_prefix("DTSTART:") {
                if let Some(dt) = parse_ical_datetime(val) {
                    start_at = dt;
                }
            } else if let Some(dt) = line
                .strip_prefix("DTSTART;")
                .and_then(|r| r.split_once(':'))
                .and_then(|(_, v)| parse_ical_datetime(v))
            {
                start_at = dt;
            } else if let Some(val) = line.strip_prefix("DTEND:") {
                if let Some(dt) = parse_ical_datetime(val) {
                    end_at = dt;
                }
            } else if let Some(dt) = line
                .strip_prefix("DTEND;")
                .and_then(|r| r.split_once(':'))
                .and_then(|(_, v)| parse_ical_datetime(v))
            {
                end_at = dt;
            } else if line.starts_with("ORGANIZER") {
                if let Some((params, mail)) = split_ical_line(line) {
                    organizer_email = mail
                        .trim_start_matches("mailto:")
                        .trim_start_matches("MAILTO:")
                        .to_string();
                    if let Some(cn_part) = params.split(';').find(|p| p.starts_with("CN=")) {
                        organizer_name = cn_part
                            .strip_prefix("CN=")
                            .map(|s| s.trim_matches('"').to_string());
                    }
                }
            } else if line.starts_with("ATTENDEE")
                && let Some((_, mail_part)) = split_ical_line(line)
            {
                let mail = mail_part
                    .trim_start_matches("mailto:")
                    .trim_start_matches("MAILTO:")
                    .trim();
                if mail.eq_ignore_ascii_case(current_user_email.trim()) {
                    if line.contains("PARTSTAT=ACCEPTED") {
                        status = RsvpStatus::Accepted;
                    } else if line.contains("PARTSTAT=DECLINED") {
                        status = RsvpStatus::Declined;
                    } else if line.contains("PARTSTAT=TENTATIVE") {
                        status = RsvpStatus::Tentative;
                    }
                }
            }
        }

        let uid = uid.ok_or_else(|| anyhow::anyhow!("Missing UID in iCalendar invitation"))?;

        Ok(Self {
            uid,
            sequence,
            summary,
            description,
            location,
            organizer_name,
            organizer_email,
            start_at,
            end_at,
            status,
        })
    }

    /// Generate an RFC 5546 iTIP `METHOD:REPLY` payload to respond to the organizer
    pub fn generate_rsvp_ics(
        &self,
        rsvp: RsvpStatus,
        responder_email: &str,
        responder_name: Option<&str>,
    ) -> String {
        let dtstamp = Utc::now().format("%Y%m%dT%H%M%SZ");
        let dtstart = self.start_at.format("%Y%m%dT%H%M%SZ");
        let dtend = self.end_at.format("%Y%m%dT%H%M%SZ");

        let name_attr = responder_name
            .map(|n| format!(";CN=\"{}\"", escape_ical_param(n)))
            .unwrap_or_default();

        format!(
            "BEGIN:VCALENDAR\r\n\
             PRODID:-//Vespetrel//Mail Client//EN\r\n\
             VERSION:2.0\r\n\
             METHOD:REPLY\r\n\
             BEGIN:VEVENT\r\n\
             UID:{}\r\n\
             SEQUENCE:{}\r\n\
             DTSTAMP:{}\r\n\
             DTSTART:{}\r\n\
             DTEND:{}\r\n\
             ORGANIZER:mailto:{}\r\n\
             ATTENDEE{};PARTSTAT={}:mailto:{}\r\n\
             SUMMARY:{}\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n",
            escape_ical_text(&self.uid),
            self.sequence,
            dtstamp,
            dtstart,
            dtend,
            escape_ical_param(&self.organizer_email),
            name_attr,
            rsvp.as_partstat(),
            escape_ical_param(responder_email),
            escape_ical_text(&self.summary)
        )
    }
}

/// Split an iCalendar line into (Property+Params, Value) ignoring colons inside quoted strings
fn split_ical_line(line: &str) -> Option<(&str, &str)> {
    let mut in_quotes = false;
    for (i, c) in line.char_indices() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c == ':' && !in_quotes {
            return Some((&line[..i], &line[i + 1..]));
        }
    }
    None
}

fn escape_ical_text(val: &str) -> String {
    val.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
        .replace('\r', "")
}

fn escape_ical_param(val: &str) -> String {
    val.replace(['\r', '\n', '"'], "")
}

fn parse_ical_datetime(val: &str) -> Option<DateTime<Utc>> {
    let clean = val.trim();
    // 1. Try RFC 5545 format: 20260815T100000Z
    if let Ok(dt) = NaiveDateTime::parse_from_str(clean, "%Y%m%dT%H%M%SZ") {
        return Some(DateTime::from_naive_utc_and_offset(dt, Utc));
    }
    // 2. Try RFC 5545 format without Z: 20260815T100000
    if let Ok(dt) = NaiveDateTime::parse_from_str(clean, "%Y%m%dT%H%M%S") {
        return Some(DateTime::from_naive_utc_and_offset(dt, Utc));
    }
    // 3. Try RFC 3339 / ISO 8601: 2026-08-15T10:00:00Z
    if let Ok(dt) = DateTime::parse_from_rfc3339(clean) {
        return Some(dt.with_timezone(&Utc));
    }
    // 4. Try DATE only: 20260815
    if let Ok(date) = NaiveDate::parse_from_str(clean, "%Y%m%d") {
        return date
            .and_hms_opt(0, 0, 0)
            .map(|naive_dt| DateTime::from_naive_utc_and_offset(naive_dt, Utc));
    }
    None
}

/// Unfold multiline iCalendar continuation lines (RFC 5545 §3.1)
pub fn unfold_ical(ics_str: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();

    for raw_line in ics_str.lines() {
        let trimmed_end = raw_line.trim_end_matches('\r');
        if trimmed_end.starts_with(' ') || trimmed_end.starts_with('\t') {
            current_line.push_str(&trimmed_end[1..]);
        } else {
            if !current_line.is_empty() {
                lines.push(current_line);
            }
            current_line = trimmed_end.to_string();
        }
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_invitation_and_generate_reply() {
        let ics_payload = r#"BEGIN:VCALENDAR
VERSION:2.0
METHOD:REQUEST
BEGIN:VEVENT
UID:event-999-abc
SEQUENCE:1
SUMMARY:Quarterly Rust Planning
DESCRIPTION:Discuss SIMD optimizations
ORGANIZER;CN="Boss Alice":mailto:alice@example.com
ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:bob@example.com
LOCATION:Conference Room A
END:VEVENT
END:VCALENDAR"#;

        let invite = MeetingInvitation::parse_ics(ics_payload, "bob@example.com").unwrap();
        assert_eq!(invite.uid, "event-999-abc");
        assert_eq!(invite.summary, "Quarterly Rust Planning");
        assert_eq!(invite.organizer_email, "alice@example.com");
        assert_eq!(invite.organizer_name.as_deref(), Some("Boss Alice"));
        assert_eq!(invite.location.as_deref(), Some("Conference Room A"));
        assert_eq!(invite.status, RsvpStatus::NeedsAction);

        let reply_ics =
            invite.generate_rsvp_ics(RsvpStatus::Accepted, "bob@example.com", Some("Bob Smith"));
        assert!(reply_ics.contains("METHOD:REPLY"));
        assert!(reply_ics.contains("PARTSTAT=ACCEPTED"));
        assert!(reply_ics.contains("UID:event-999-abc"));
        assert!(reply_ics.contains("CN=\"Bob Smith\""));
    }

    #[test]
    fn test_parse_ical_datetime_formats() {
        let ics = "BEGIN:VCALENDAR\r\n\
BEGIN:VEVENT\r\n\
UID:time-test\r\n\
DTSTART:20260815T100000Z\r\n\
DTEND:20260815T110000Z\r\n\
END:VEVENT\r\n\
END:VCALENDAR";
        let invite = MeetingInvitation::parse_ics(ics, "test@example.com").unwrap();
        assert_eq!(invite.start_at.timestamp(), 1786788000);
        assert_eq!(invite.end_at.timestamp(), 1786791600);
    }

    #[test]
    fn test_unfold_ical_continuation_lines() {
        let folded = "SUMMARY:This is a very long \r\n summary that was \r\n\tfolded across lines\r\nUID:fold-123\r\n";
        let unfolded = unfold_ical(folded);
        assert_eq!(unfolded.len(), 2);
        assert_eq!(
            unfolded[0],
            "SUMMARY:This is a very long summary that was folded across lines"
        );
        assert_eq!(unfolded[1], "UID:fold-123");
    }
}
