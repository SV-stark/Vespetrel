//! Vespetrel JMAP - RFC 8620/8621 via stalwart jmap-client

use async_trait::async_trait;
use tracing::{debug, info};

use vespetrel_core::account::SyncState;
use vespetrel_core::folder::Folder;
use vespetrel_core::message::{ComposedMessage, Flag};
use vespetrel_core::provider::{MailProvider, RemoteFolder, SyncDelta};

#[derive(Debug, Clone)]
pub struct JmapConfig {
    pub base_url: String,
    pub username: String,
    pub access_token: String,
}

impl JmapConfig {
    pub fn new(
        base_url: impl Into<String>,
        username: impl Into<String>,
        access_token: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            username: username.into(),
            access_token: access_token.into(),
        }
    }
}

pub struct JmapProvider {
    config: JmapConfig,
    http: reqwest::Client,
}

impl JmapProvider {
    pub fn new(config: JmapConfig) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("Vespetrel/0.1 JMAP")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { config, http }
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.http
    }

    /// Build JMAP request body - single roundtrip multi-call §4.3
    pub fn build_get_mailboxes_request(&self) -> serde_json::Value {
        serde_json::json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            "methodCalls": [
                ["Mailbox/get", { "accountId": self.config.username, "ids": null }, "0"],
                ["Email/queryChanges", { "accountId": self.config.username, "sinceState": null }, "1"]
            ]
        })
    }

    /// Build JMAP Email/query and Email/get request for a specific mailbox
    pub fn build_email_query_request(&self, mailbox_id: &str, limit: usize) -> serde_json::Value {
        serde_json::json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            "methodCalls": [
                ["Email/query", {
                    "accountId": self.config.username,
                    "filter": { "inMailbox": mailbox_id },
                    "sort": [{ "property": "receivedAt", "isAscending": false }],
                    "limit": limit
                }, "0"],
                ["Email/get", {
                    "accountId": self.config.username,
                    "#ids": {
                        "resultOf": "0",
                        "name": "Email/query",
                        "path": "/ids"
                    },
                    "properties": ["id", "blobId", "threadId", "mailboxIds", "keywords", "size", "receivedAt", "from", "to", "subject", "preview"]
                }, "1"]
            ]
        })
    }

    /// Build JMAP EmailSubmission request for outbound email
    pub fn build_email_submission_request(&self, msg: &ComposedMessage) -> serde_json::Value {
        serde_json::json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail", "urn:ietf:params:jmap:submission"],
            "methodCalls": [
                ["Email/set", {
                    "accountId": self.config.username,
                    "create": {
                        "draftMsg": {
                            "from": [{ "name": msg.from.name.clone().unwrap_or_default(), "email": msg.from.email }],
                            "to": msg.to.iter().map(|t| serde_json::json!({ "name": t.name.clone().unwrap_or_default(), "email": t.email })).collect::<Vec<_>>(),
                            "subject": msg.subject,
                            "bodyValues": {
                                "1": { "value": msg.body_text }
                            },
                            "textBody": [{ "partId": "1", "type": "text/plain" }]
                        }
                    }
                }, "0"],
                ["EmailSubmission/create", {
                    "accountId": self.config.username,
                    "create": {
                        "sub1": {
                            "emailId": "#draftMsg",
                            "identityId": self.config.username
                        }
                    }
                }, "1"]
            ]
        })
    }
}

/// Parse JMAP Mailbox/get response into RemoteFolder list
pub fn parse_jmap_mailbox_response(resp: &serde_json::Value) -> Vec<RemoteFolder> {
    let mut folders = Vec::new();
    let calls = resp
        .get("methodResponses")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or_default();

    for call in calls {
        let is_mailbox_get = call.get(0).and_then(|n| n.as_str()) == Some("Mailbox/get");
        if is_mailbox_get {
            let list = call
                .get(1)
                .and_then(|o| o.get("list"))
                .and_then(|l| l.as_array())
                .map(|a| a.as_slice())
                .unwrap_or_default();

            for item in list {
                if let (Some(id), Some(name)) = (
                    item.get("id").and_then(|i| i.as_str()),
                    item.get("name").and_then(|n| n.as_str()),
                ) {
                    let role = item
                        .get("role")
                        .and_then(|r| r.as_str())
                        .map(|r| r.to_string());
                    folders.push(RemoteFolder {
                        remote_id: id.to_string(),
                        name: name.to_string(),
                        path: name.to_string(),
                        role_hint: role,
                        uid_validity: None,
                        highest_mod_seq: None,
                    });
                }
            }
        }
    }
    folders
}

#[async_trait]
impl MailProvider for JmapProvider {
    async fn sync_folder_list(&self) -> anyhow::Result<Vec<RemoteFolder>> {
        debug!(url=%self.config.base_url, "JMAP sync_folder_list");
        // Real: POST /.well-known/jmap + session discovery -> Mailbox/get
        // Stub
        Ok(vec![
            RemoteFolder {
                remote_id: "inbox".into(),
                name: "Inbox".into(),
                path: "Inbox".into(),
                role_hint: Some("inbox".into()),
                uid_validity: None,
                highest_mod_seq: None,
            },
            RemoteFolder {
                remote_id: "sent".into(),
                name: "Sent".into(),
                path: "Sent".into(),
                role_hint: Some("sent".into()),
                uid_validity: None,
                highest_mod_seq: None,
            },
        ])
    }

    async fn sync_messages(&self, folder: &Folder, state: SyncState) -> anyhow::Result<SyncDelta> {
        debug!(folder=%folder.name, state=?state.jmap_state, "JMAP sync_messages");
        // Real: Email/queryChanges with sinceState, then Email/get
        // Push via EventSource SSE
        info!(folder=%folder.name, "JMAP delta sync stub");
        Ok(SyncDelta::default())
    }

    async fn fetch_raw_message(&self, remote_id: &str) -> anyhow::Result<Vec<u8>> {
        debug!(remote_id, "JMAP fetch_raw_message");
        // Real: Email/get with bodyProperties + fetch blob
        Ok(format!(
            "From: jmap@example.com\r\nSubject: JMAP {}\r\n\r\nStub",
            remote_id
        )
        .into_bytes())
    }

    async fn send_message(&self, msg: &ComposedMessage) -> anyhow::Result<()> {
        info!(subject=%msg.subject, "JMAP EmailSubmission");
        // Real: Email/set create + EmailSubmission/create
        Ok(())
    }

    async fn update_flags(
        &self,
        remote_ids: &[u32],
        add: &[Flag],
        remove: &[Flag],
    ) -> anyhow::Result<()> {
        debug!(uids=?remote_ids, add=?add, remove=?remove, "JMAP Email/set keywords");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jmap_requests_and_parsing() {
        let cfg = JmapConfig::new(
            "https://api.fastmail.com/jmap",
            "user@fastmail.com",
            "token123",
        );
        let provider = JmapProvider::new(cfg);

        let req = provider.build_get_mailboxes_request();
        assert!(req.get("using").is_some());
        assert!(req.get("methodCalls").is_some());

        let mock_resp = serde_json::json!({
            "methodResponses": [
                ["Mailbox/get", {
                    "list": [
                        { "id": "mbx-inbox", "name": "Inbox", "role": "inbox" },
                        { "id": "mbx-sent", "name": "Sent Items", "role": "sent" }
                    ]
                }, "0"]
            ]
        });

        let folders = parse_jmap_mailbox_response(&mock_resp);
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0].remote_id, "mbx-inbox");
        assert_eq!(folders[0].role_hint.as_deref(), Some("inbox"));
        assert_eq!(folders[1].name, "Sent Items");
    }
}
