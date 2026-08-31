//! Matrix & Decentralized Chat Protocol Bridge §7 Phase 6
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixRoom {
    pub room_id: String,
    pub display_name: String,
    pub topic: Option<String>,
    pub member_count: usize,
    pub unread_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixEvent {
    pub event_id: String,
    pub room_id: String,
    pub sender: String,
    pub body: String,
    pub timestamp: DateTime<Utc>,
    pub thread_root: Option<String>,
}

pub struct MatrixBridge {
    pub homeserver_url: String,
    pub user_id: String,
    pub access_token: String,
}

impl MatrixBridge {
    pub fn new(
        homeserver_url: impl Into<String>,
        user_id: impl Into<String>,
        access_token: impl Into<String>,
    ) -> Self {
        Self {
            homeserver_url: homeserver_url.into(),
            user_id: user_id.into(),
            access_token: access_token.into(),
        }
    }

    /// Build `/sync` endpoint URL with since token
    pub fn sync_url(&self, since_token: Option<&str>) -> String {
        match since_token {
            Some(token) => format!(
                "{}/_matrix/client/v3/sync?since={token}&timeout=30000",
                self.homeserver_url
            ),
            None => format!(
                "{}/_matrix/client/v3/sync?timeout=30000",
                self.homeserver_url
            ),
        }
    }

    /// Build send message endpoint URL
    pub fn send_message_url(&self, room_id: &str, txn_id: &str) -> String {
        format!(
            "{}/_matrix/client/v3/rooms/{room_id}/send/m.room.message/{txn_id}",
            self.homeserver_url
        )
    }

    /// Convert Matrix message event into a unified MessageSummary
    pub fn to_message_summary(
        event: &MatrixEvent,
        room_name: &str,
    ) -> vespetrel_core::MessageSummary {
        vespetrel_core::MessageSummary {
            id: format!("matrix:{}", event.event_id),
            thread_id: event.thread_root.clone(),
            subject: Some(format!(
                "[Matrix: {}] Message from {}",
                room_name, event.sender
            )),
            from_address: event.sender.clone(),
            from_name: Some(event.sender.clone()),
            snippet: Some(event.body.clone()),
            sent_at: event.timestamp,
            is_read: false,
            is_flagged: false,
            has_attachments: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matrix_bridge_urls_and_conversion() {
        let bridge = MatrixBridge::new("https://matrix.org", "@alice:matrix.org", "syt_token_123");
        assert_eq!(
            bridge.sync_url(Some("s100_200")),
            "https://matrix.org/_matrix/client/v3/sync?since=s100_200&timeout=30000"
        );

        let event = MatrixEvent {
            event_id: "$event_999".into(),
            room_id: "!rust:matrix.org".into(),
            sender: "@bob:matrix.org".into(),
            body: "Great progress on Phase 5 and 6!".into(),
            timestamp: Utc::now(),
            thread_root: None,
        };

        let summary = MatrixBridge::to_message_summary(&event, "Rust Core");
        assert_eq!(summary.from_address, "@bob:matrix.org");
        assert_eq!(
            summary.snippet.as_deref(),
            Some("Great progress on Phase 5 and 6!")
        );
    }
}
