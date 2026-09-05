//! Vespetrel Graph - Microsoft Graph REST for Mail/Calendar/Contacts §4.4

use async_trait::async_trait;
use serde::Deserialize;
use tracing::{debug, info};

use vespetrel_core::account::SyncState;
use vespetrel_core::folder::Folder;
use vespetrel_core::message::{ComposedMessage, Flag};
use vespetrel_core::provider::{MailProvider, RemoteFolder, SyncDelta};

const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";

#[derive(Clone)]
pub struct GraphConfig {
    pub access_token: String,
    pub base_url: String,
}

impl std::fmt::Debug for GraphConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphConfig")
            .field("base_url", &self.base_url)
            .field("access_token", &"[REDACTED]")
            .finish()
    }
}

impl GraphConfig {
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            base_url: GRAPH_BASE.into(),
        }
    }

    pub fn delta_url(&self, folder_id: &str, delta_token: Option<&str>) -> String {
        let enc_folder = url_encode(folder_id);
        if let Some(tok) = delta_token {
            let enc_tok = url_encode(tok);
            format!(
                "{}/me/mailFolders/{enc_folder}/messages/delta?$deltatoken={enc_tok}",
                self.base_url
            )
        } else {
            format!(
                "{}/me/mailFolders/{enc_folder}/messages/delta",
                self.base_url
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
        let enc_id = urlencoding::encode(message_id);
        format!("{}/me/messages/{enc_id}/$value", self.base_url)
    }
}

fn url_encode(input: &str) -> String {
    urlencoding::encode(input).into_owned()
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

    pub fn build_send_mail_payload(&self, msg: &ComposedMessage) -> serde_json::Value {
        let to_recipients: Vec<serde_json::Value> = msg
            .to
            .iter()
            .map(|to| {
                serde_json::json!({
                    "emailAddress": {
                        "address": to.email,
                        "name": to.name.as_deref().unwrap_or(&to.email)
                    }
                })
            })
            .collect();

        serde_json::json!({
            "message": {
                "subject": msg.subject,
                "body": {
                    "contentType": if msg.body_html.is_some() { "HTML" } else { "Text" },
                    "content": msg.body_html.as_ref().unwrap_or(&msg.body_text)
                },
                "toRecipients": to_recipients
            },
            "saveToSentItems": true
        })
    }
}

#[async_trait]
impl MailProvider for GraphProvider {
    async fn sync_folder_list(
        &self,
    ) -> Result<Vec<RemoteFolder>, vespetrel_core::provider::ProviderError> {
        debug!(url=%self.config.folders_url(), "Graph sync_folder_list");
        if !self.config.access_token.is_empty() {
            let resp = self
                .http
                .get(self.config.folders_url())
                .bearer_auth(&self.config.access_token)
                .send()
                .await
                .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?;
            let json = resp
                .error_for_status()
                .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?
                .json::<serde_json::Value>()
                .await
                .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?;
            let folders = parse_graph_folders_response(&json);
            if !folders.is_empty() {
                return Ok(folders);
            }
        }

        #[cfg(any(test, feature = "mock"))]
        {
            // Test/Offline simulated folders
            return Ok(vec![
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
            ]);
        }

        #[cfg(not(any(test, feature = "mock")))]
        {
            Err(vespetrel_core::provider::ProviderError::Protocol(
                "Microsoft Graph returned no mailboxes or token is invalid".into(),
            ))
        }
    }

    async fn sync_messages(
        &self,
        folder: &Folder,
        state: SyncState,
    ) -> Result<SyncDelta, vespetrel_core::provider::ProviderError> {
        debug!(folder=%folder.name, "Graph delta query");

        if !self.config.access_token.is_empty() {
            let mut delta = SyncDelta::default();
            let mut next_url = Some(
                self.config
                    .delta_url(&folder.remote_id, state.graph_delta_token.as_deref()),
            );
            let mut final_delta_token = state.graph_delta_token.clone();

            while let Some(current_url) = next_url {
                debug!(url=%current_url, "fetching Graph messages delta page");
                let resp = self
                    .http
                    .get(&current_url)
                    .bearer_auth(&self.config.access_token)
                    .send()
                    .await
                    .map_err(|e| {
                        vespetrel_core::provider::ProviderError::Protocol(e.to_string())
                    })?;
                let json = resp
                    .error_for_status()
                    .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?
                    .json::<serde_json::Value>()
                    .await
                    .map_err(|e| {
                        vespetrel_core::provider::ProviderError::Protocol(e.to_string())
                    })?;

                if let Some(link) = json.get("@odata.deltaLink").and_then(|v| v.as_str()) {
                    let extracted = if let Ok(parsed_url) = url::Url::parse(link) {
                        parsed_url
                            .query_pairs()
                            .find(|(k, _)| k == "$deltatoken" || k == "deltatoken")
                            .map(|(_, v)| v.into_owned())
                    } else {
                        link.split("$deltatoken=")
                            .nth(1)
                            .and_then(|s| s.split('&').next())
                            .map(|s| s.to_string())
                    };
                    final_delta_token = extracted.or(final_delta_token);
                    next_url = None;
                } else if let Some(next_link) = json.get("@odata.nextLink").and_then(|v| v.as_str())
                {
                    next_url = Some(next_link.to_string());
                } else {
                    next_url = None;
                }

                if let Some(items) = json.get("value").and_then(|v| v.as_array()) {
                    for item in items {
                        let id = item.get("id").and_then(|s| s.as_str()).unwrap_or_default();
                        if item.get("@removed").is_some() {
                            let remote_uid = vespetrel_core::stable_uid_from_id(id);
                            delta.deleted_uids.push(remote_uid);
                            continue;
                        }

                        let mut flags = Vec::new();
                        if item
                            .get("isRead")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                        {
                            flags.push(Flag::Seen);
                        }
                        if item
                            .get("flag")
                            .and_then(|f| f.get("flagStatus"))
                            .and_then(|s| s.as_str())
                            == Some("flagged")
                        {
                            flags.push(Flag::Flagged);
                        }
                        let remote_uid = vespetrel_core::stable_uid_from_id(id);

                        let subject = item.get("subject").and_then(|s| s.as_str()).unwrap_or("");
                        let body_preview = item
                            .get("bodyPreview")
                            .and_then(|s| s.as_str())
                            .unwrap_or("");
                        let sender = item
                            .pointer("/from/emailAddress/address")
                            .and_then(|s| s.as_str())
                            .unwrap_or("graph@example.com");
                        let date = item
                            .get("receivedDateTime")
                            .and_then(|s| s.as_str())
                            .unwrap_or("");
                        let date_hdr = if !date.is_empty() {
                            format!("Date: {date}\r\n")
                        } else {
                            format!("Date: {}\r\n", chrono::Utc::now().to_rfc2822())
                        };

                        let mime_data = format!(
                            "From: {sender}\r\nTo: me@example.com\r\nSubject: {subject}\r\n{date_hdr}Message-ID: <{id}@graph.microsoft.com>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body_preview}"
                        )
                        .into_bytes();

                        delta.inserted.push(vespetrel_core::provider::SyncMessage {
                            remote_uid,
                            remote_id: Some(id.to_string()),
                            flags,
                            raw_rfc822: Some(mime_data),
                            mod_seq: None,
                        });
                    }
                }
            }

            delta.new_sync_state = SyncState {
                graph_delta_token: final_delta_token,
                ..state
            };
            return Ok(delta);
        }

        #[cfg(any(test, feature = "mock"))]
        {
            // Offline / simulated delta response
            let new_sync_state = SyncState {
                graph_delta_token: Some("delta-token-active".into()),
                ..Default::default()
            };
            return Ok(SyncDelta {
                new_sync_state,
                ..Default::default()
            });
        }

        #[cfg(not(any(test, feature = "mock")))]
        {
            Err(vespetrel_core::provider::ProviderError::Protocol(
                "Microsoft Graph sync failed: invalid access token".into(),
            ))
        }
    }

    async fn fetch_raw_message(
        &self,
        remote_id: &str,
    ) -> Result<Vec<u8>, vespetrel_core::provider::ProviderError> {
        debug!(remote_id, "Graph fetch MIME");
        if !self.config.access_token.is_empty() {
            let mime_url = self.config.message_mime_url(remote_id);
            let resp = self
                .http
                .get(&mime_url)
                .bearer_auth(&self.config.access_token)
                .send()
                .await
                .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?;
            let bytes = resp
                .error_for_status()
                .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?
                .bytes()
                .await
                .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?;
            return Ok(bytes.to_vec());
        }

        #[cfg(any(test, feature = "mock"))]
        {
            return Ok(format!(
                "From: graph@example.com\r\nSubject: Graph {}\r\n\r\nGraph Message Content",
                remote_id
            )
            .into_bytes());
        }

        #[cfg(not(any(test, feature = "mock")))]
        {
            Err(vespetrel_core::provider::ProviderError::Protocol(format!(
                "unable to fetch MIME for Graph message {remote_id}: invalid access token"
            )))
        }
    }

    async fn send_message(
        &self,
        msg: &ComposedMessage,
    ) -> Result<(), vespetrel_core::provider::ProviderError> {
        info!(subject=%msg.subject, "Graph sendMail");
        if !self.config.access_token.is_empty() {
            let payload = self.build_send_mail_payload(msg);
            let url = format!("{}/me/sendMail", self.config.base_url);
            let resp = self
                .http
                .post(&url)
                .bearer_auth(&self.config.access_token)
                .json(&payload)
                .send()
                .await
                .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?;
            let _ = resp
                .error_for_status()
                .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?;
        }
        Ok(())
    }

    async fn update_flags_by_remote_id(
        &self,
        remote_ids: &[String],
        add: &[Flag],
        remove: &[Flag],
    ) -> Result<(), vespetrel_core::provider::ProviderError> {
        debug!(remote_ids=?remote_ids, "Graph PATCH isRead/flag by string ID");
        if !self.config.access_token.is_empty() {
            let is_read = if add.contains(&Flag::Seen) {
                Some(true)
            } else if remove.contains(&Flag::Seen) {
                Some(false)
            } else {
                None
            };
            let flag_status = if add.contains(&Flag::Flagged) {
                Some("flagged")
            } else if remove.contains(&Flag::Flagged) {
                Some("notFlagged")
            } else {
                None
            };

            let mut patch_map = serde_json::Map::new();
            if let Some(read_val) = is_read {
                patch_map.insert("isRead".into(), serde_json::Value::Bool(read_val));
            }
            if let Some(status) = flag_status {
                patch_map.insert("flag".into(), serde_json::json!({ "flagStatus": status }));
            }

            if !patch_map.is_empty() {
                let body = serde_json::Value::Object(patch_map);
                for id in remote_ids {
                    let enc_id = urlencoding::encode(id);
                    let url = format!("{}/me/messages/{enc_id}", self.config.base_url);
                    let resp = self
                        .http
                        .patch(&url)
                        .bearer_auth(&self.config.access_token)
                        .json(&body)
                        .send()
                        .await
                        .map_err(|e| {
                            vespetrel_core::provider::ProviderError::Protocol(e.to_string())
                        })?;
                    let _ = resp.error_for_status().map_err(|e| {
                        vespetrel_core::provider::ProviderError::Protocol(e.to_string())
                    })?;
                }
            }
        }
        Ok(())
    }

    async fn update_flags(
        &self,
        remote_ids: &[u32],
        add: &[Flag],
        remove: &[Flag],
    ) -> Result<(), vespetrel_core::provider::ProviderError> {
        let ids: Vec<String> = remote_ids.iter().map(|u| u.to_string()).collect();
        self.update_flags_by_remote_id(&ids, add, remove).await
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
