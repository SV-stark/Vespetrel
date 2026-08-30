use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
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
