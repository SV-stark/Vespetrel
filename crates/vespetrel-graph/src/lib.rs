//! Vespetrel Graph - Microsoft Graph REST for Mail/Calendar/Contacts §4.4

use async_trait::async_trait;
use serde::Deserialize;
use tracing::{debug, info};

use vespetrel_core::account::SyncState;
use vespetrel_core::folder::Folder;
use vespetrel_core::message::{ComposedMessage, Flag};
use vespetrel_core::provider::{MailProvider, RemoteFolder, SyncDelta};

const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";

#[derive(Debug, Clone)]
pub struct GraphConfig {
    pub access_token: String,
    pub base_url: String,
}

impl GraphConfig {
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            base_url: GRAPH_BASE.into(),
        }
    }

    pub fn delta_url(&self, folder_id: &str, delta_token: Option<&str>) -> String {
        if let Some(tok) = delta_token {
            format!(
                "{}/me/mailFolders/{}/messages/delta?$deltatoken={}",
                self.base_url, folder_id, tok
            )
        } else {
            format!(
                "{}/me/mailFolders/{}/messages/delta",
                self.base_url, folder_id
            )
        }
    }

    pub fn folders_url(&self) -> String {
        format!("{}/me/mailFolders", self.base_url)
    }

    pub fn send_mail_url(&self) -> String {
        format!("{}/me/sendMail", self.base_url)
    }

    pub fn message_mime_url(&self, message_id: &str) -> String {
        format!("{}/me/messages/{message_id}/$value", self.base_url)
    }
}

/// Convert ComposedMessage into Microsoft Graph sendMail JSON payload
pub fn build_graph_sendmail_payload(msg: &ComposedMessage) -> serde_json::Value {
    let to_recipients: Vec<serde_json::Value> = msg
        .to
        .iter()
        .map(|a| {
            serde_json::json!({
                "emailAddress": {
                    "name": a.name.clone().unwrap_or_default(),
                    "address": a.email
                }
            })
        })
        .collect();

    serde_json::json!({
        "message": {
            "subject": msg.subject,
            "body": {
                "contentType": if msg.body_html.is_some() { "HTML" } else { "Text" },
                "content": msg.body_html.clone().unwrap_or_else(|| msg.body_text.clone())
            },
            "toRecipients": to_recipients
        },
        "saveToSentItems": true
    })
}

/// Parse Microsoft Graph `/me/mailFolders` response into RemoteFolder list
pub fn parse_graph_folders_response(json: &serde_json::Value) -> Vec<RemoteFolder> {
    let mut folders = Vec::new();
    if let Some(list) = json.get("value").and_then(|v| v.as_array()) {
        for item in list {
            if let (Some(id), Some(display_name)) = (
                item.get("id").and_then(|i| i.as_str()),
                item.get("displayName").and_then(|n| n.as_str()),
            ) {
                let role = match display_name.to_lowercase().as_str() {
                    "inbox" => Some("inbox".into()),
                    "sent items" => Some("sent".into()),
                    "drafts" => Some("drafts".into()),
                    "deleted items" => Some("trash".into()),
                    "junk email" => Some("junk".into()),
                    "archive" => Some("archive".into()),
                    _ => None,
                };
                folders.push(RemoteFolder {
                    remote_id: id.to_string(),
                    name: display_name.to_string(),
                    path: display_name.to_string(),
                    role_hint: role,
                    uid_validity: None,
                    highest_mod_seq: None,
                });
            }
        }
    }
    folders
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GraphFolder {
    id: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "childFolderCount")]
    child_folder_count: Option<i32>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GraphFolderList {
    value: Vec<GraphFolder>,
}

pub struct GraphProvider {
    config: GraphConfig,
    http: reqwest::Client,
}

impl GraphProvider {
    pub fn new(config: GraphConfig) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("Vespetrel/0.1 Graph")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { config, http }
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.http
    }
}

#[async_trait]
impl MailProvider for GraphProvider {
    async fn sync_folder_list(&self) -> anyhow::Result<Vec<RemoteFolder>> {
        debug!(url=%self.config.folders_url(), "Graph sync_folder_list");
        // Real: GET /me/mailFolders with auth header
        // let resp = self.http.get(self.config.folders_url()).bearer_auth(&self.config.access_token).send().await?;
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
                remote_id: "sentitems".into(),
                name: "Sent Items".into(),
                path: "Sent Items".into(),
                role_hint: Some("sent".into()),
                uid_validity: None,
                highest_mod_seq: None,
            },
        ])
    }

    async fn sync_messages(&self, folder: &Folder, state: SyncState) -> anyhow::Result<SyncDelta> {
        let url = self
            .config
            .delta_url(&folder.remote_id, state.graph_delta_token.as_deref());
        debug!(folder=%folder.name, url=%url, "Graph delta query");
        // Real delta: GET url with bearer, parse @odata.deltaLink for next token
        info!(folder=%folder.name, "Graph delta sync stub");
        let new_sync_state = SyncState {
            graph_delta_token: Some("stub-delta-token".into()),
            ..Default::default()
        };
        Ok(SyncDelta {
            new_sync_state,
            ..Default::default()
        })
    }

    async fn fetch_raw_message(&self, remote_id: &str) -> anyhow::Result<Vec<u8>> {
        debug!(remote_id, "Graph fetch MIME");
        // Real: GET /me/messages/{id}/$value
        Ok(format!(
            "From: graph@example.com\r\nSubject: Graph {}\r\n\r\nStub",
            remote_id
        )
        .into_bytes())
    }

    async fn send_message(&self, msg: &ComposedMessage) -> anyhow::Result<()> {
        info!(subject=%msg.subject, "Graph sendMail");
        // Real: POST /me/sendMail
        Ok(())
    }

    async fn update_flags(
        &self,
        remote_ids: &[u32],
        add: &[Flag],
        remove: &[Flag],
    ) -> anyhow::Result<()> {
        debug!(uids=?remote_ids, "Graph PATCH isRead/flag");
        // Real: PATCH /me/messages/{id} with {"isRead": true}
        let _ = (add, remove);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vespetrel_core::Address;

    #[test]
    fn test_graph_urls_and_payloads() {
        let cfg = GraphConfig::new("mock_token_123");
        assert_eq!(
            cfg.folders_url(),
            "https://graph.microsoft.com/v1.0/me/mailFolders"
        );
        assert_eq!(
            cfg.delta_url("inbox_id", Some("tok_abc")),
            "https://graph.microsoft.com/v1.0/me/mailFolders/inbox_id/messages/delta?$deltatoken=tok_abc"
        );

        let msg = ComposedMessage {
            from: Address {
                name: None,
                email: "me@example.com".into(),
            },
            to: vec![Address {
                name: Some("Boss".into()),
                email: "boss@example.com".into(),
            }],
            cc: vec![],
            bcc: vec![],
            subject: "Quarterly Review".into(),
            body_text: "Attached".into(),
            body_html: Some("<p>Attached</p>".into()),
            in_reply_to: None,
            references: vec![],
            attachments: vec![],
        };

        let payload = build_graph_sendmail_payload(&msg);
        assert!(payload.get("message").is_some());
        assert_eq!(
            payload["message"]["toRecipients"][0]["emailAddress"]["address"],
            "boss@example.com"
        );

        let mock_folders_json = serde_json::json!({
            "value": [
                { "id": "fld_1", "displayName": "Inbox" },
                { "id": "fld_2", "displayName": "Sent Items" }
            ]
        });
        let folders = parse_graph_folders_response(&mock_folders_json);
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0].role_hint.as_deref(), Some("inbox"));
        assert_eq!(folders[1].role_hint.as_deref(), Some("sent"));
    }
}
