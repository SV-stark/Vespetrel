use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    pub remote_id: Option<String>,
    pub display_name: Option<String>,
    pub email: String,
    pub vcard_data: Option<String>,
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
