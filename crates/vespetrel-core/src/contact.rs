use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    pub remote_id: Option<String>,
    pub display_name: Option<String>,
    pub email: String,
    pub vcard_data: Option<String>,
}

impl Contact {
    pub fn new(display_name: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            remote_id: None,
            display_name: Some(display_name.into()),
            email: email.into(),
            vcard_data: None,
        }
    }

    /// Parse a Contact from an RFC 6350 vCard 4.0 payload using `vcard4`
    pub fn from_vcard(vcard_str: &str) -> anyhow::Result<Self> {
        let cards =
            vcard4::parse(vcard_str).map_err(|e| anyhow::anyhow!("vCard parse error: {e:?}"))?;
        let card = cards
            .first()
            .ok_or_else(|| anyhow::anyhow!("empty vCard data"))?;

        let display_name = card
            .formatted_name
            .first()
            .map(|fn_prop| fn_prop.value.clone());
        let email = card
            .email
            .first()
            .map(|e| e.value.clone())
            .unwrap_or_default();

        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            remote_id: None,
            display_name,
            email,
            vcard_data: Some(vcard_str.to_string()),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub id: String,
    pub calendar_id: String,
    pub title: String,
    pub description: Option<String>,
    pub start: chrono::DateTime<chrono::Utc>,
    pub end: chrono::DateTime<chrono::Utc>,
    pub location: Option<String>,
    pub ical_uid: Option<String>,
    pub raw_ical: Option<String>,
}

impl CalendarEvent {
    /// Convert to structured `icalendar::Event` component
    pub fn to_ical_event(&self) -> icalendar::Event {
        use icalendar::{Component, EventLike};
        let mut event = icalendar::Event::new();
        event.summary(&self.title);
        if let Some(desc) = &self.description {
            event.description(desc);
        }
        if let Some(loc) = &self.location {
            event.location(loc);
        }
        if let Some(uid) = &self.ical_uid {
            event.uid(uid);
        } else {
            event.uid(&self.id);
        }
        event.starts(self.start);
        event.ends(self.end);
        event
    }
}

/// RFC 5545 VTODO item representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskItem {
    pub id: String,
    pub calendar_id: String,
    pub title: String,
    pub description: Option<String>,
    pub due_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_completed: bool,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub priority: u8, // 0 = undefined, 1 = high, 5 = normal, 9 = low
    pub ical_uid: Option<String>,
}

impl TaskItem {
    pub fn new(calendar_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            calendar_id: calendar_id.into(),
            title: title.into(),
            description: None,
            due_at: None,
            is_completed: false,
            completed_at: None,
            priority: 0,
            ical_uid: None,
        }
    }

    /// Convert to structured `icalendar::Todo` component
    pub fn to_ical_todo(&self) -> icalendar::Todo {
        use icalendar::Component;
        let mut todo = icalendar::Todo::new();
        todo.summary(&self.title);

        if let Some(desc) = &self.description {
            todo.description(desc);
        }
        if let Some(uid) = &self.ical_uid {
            todo.uid(uid);
        } else {
            todo.uid(&self.id);
        }
        if let Some(due) = self.due_at {
            todo.due(due);
        }
        if self.priority > 0 {
            todo.priority(self.priority as u32);
        }
        if self.is_completed {
            todo.percent_complete(100);
        }
        todo
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use icalendar::Component;

    #[test]
    fn test_task_and_contact_creation() {
        let task = TaskItem::new("cal_1", "Finish RFC 5545 Support");
        assert_eq!(task.calendar_id, "cal_1");
        assert_eq!(task.title, "Finish RFC 5545 Support");
        assert!(!task.is_completed);

        let contact = Contact {
            id: "cnt_1".into(),
            remote_id: Some("rem_1".into()),
            display_name: Some("Alice Smith".into()),
            email: "alice@example.com".into(),
            vcard_data: None,
        };
        assert_eq!(contact.email, "alice@example.com");

        // 2. vCard 4.0 Parsing
        let vcard_str = "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Dr. John Watson\r\nEMAIL:watson@bakerstreet.com\r\nEND:VCARD\r\n";
        let parsed_contact = Contact::from_vcard(vcard_str).unwrap();
        assert_eq!(
            parsed_contact.display_name.as_deref(),
            Some("Dr. John Watson")
        );
        assert_eq!(parsed_contact.email, "watson@bakerstreet.com");

        // 3. iCalendar Event & Todo generation
        let now = chrono::Utc::now();
        let event = CalendarEvent {
            id: "evt_1".into(),
            calendar_id: "cal_1".into(),
            title: "Sprint Review".into(),
            description: Some("Q3 Demo".into()),
            start: now,
            end: now + chrono::Duration::hours(1),
            location: Some("Room 101".into()),
            ical_uid: Some("uid-101".into()),
            raw_ical: None,
        };
        let ical_evt = event.to_ical_event();
        let ical_str = ical_evt.to_string();
        assert!(ical_str.contains("Sprint Review"));
        assert!(ical_str.contains("uid-101"));

        let ical_todo = task.to_ical_todo();
        let todo_str = ical_todo.to_string();
        assert!(todo_str.contains("Finish RFC 5545 Support"));
    }
}
