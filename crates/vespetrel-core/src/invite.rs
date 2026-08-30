//! iCalendar Meeting Invitations & iTIP RSVP Engine (RFC 5545 / RFC 5546) §7 Phase 5
use chrono::{DateTime, Utc};
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

        for line in ics_str.lines() {
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
                if let Ok(dt) = DateTime::parse_from_rfc3339(val).map(|d| d.with_timezone(&Utc)) {
                    start_at = dt;
                }
            } else if let Some(val) = line.strip_prefix("DTEND:") {
                if let Ok(dt) = DateTime::parse_from_rfc3339(val).map(|d| d.with_timezone(&Utc)) {
                    end_at = dt;
                }
            } else if line.starts_with("ORGANIZER") {
                if let Some((params, mail)) = line.split_once(':') {
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
                && line
                    .to_lowercase()
                    .contains(&current_user_email.to_lowercase())
            {
                if line.contains("PARTSTAT=ACCEPTED") {
                    status = RsvpStatus::Accepted;
                } else if line.contains("PARTSTAT=DECLINED") {
                    status = RsvpStatus::Declined;
                } else if line.contains("PARTSTAT=TENTATIVE") {
                    status = RsvpStatus::Tentative;
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
            .map(|n| format!(";CN=\"{}\"", n))
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
            self.uid,
            self.sequence,
            dtstamp,
            dtstart,
            dtend,
            self.organizer_email,
            name_attr,
            rsvp.as_partstat(),
            responder_email,
            self.summary
        )
    }
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
}
