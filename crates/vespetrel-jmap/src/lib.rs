//! Vespetrel JMAP - RFC 8620/8621 via stalwart jmap-client

use async_trait::async_trait;
use tracing::{debug, info};

use vespetrel_core::account::SyncState;
use vespetrel_core::folder::Folder;
use vespetrel_core::message::{ComposedMessage, Flag};
use vespetrel_core::provider::{MailProvider, RemoteFolder, SyncDelta};

#[derive(Clone)]
pub struct JmapConfig {
    pub base_url: String,
    pub username: String,
    pub access_token: String,
}

impl std::fmt::Debug for JmapConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JmapConfig")
            .field("base_url", &self.base_url)
            .field("username", &self.username)
            .field("access_token", &"[REDACTED]")
            .finish()
    }
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

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct JmapSession {
    #[serde(rename = "apiUrl")]
    pub api_url: String,
    #[serde(rename = "downloadUrl")]
    pub download_url: String,
    #[serde(rename = "uploadUrl")]
    pub upload_url: String,
    #[serde(rename = "primaryAccounts")]
    pub primary_accounts: Option<std::collections::HashMap<String, String>>,
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

    /// RFC 8620 §2 JMAP session discovery via /.well-known/jmap
    pub async fn discover_session(
        &self,
    ) -> Result<JmapSession, vespetrel_core::provider::ProviderError> {
        let session_url = if self.config.base_url.contains("/.well-known/jmap")
            || self.config.base_url.ends_with("/session")
        {
            self.config.base_url.clone()
        } else {
            format!(
                "{}/.well-known/jmap",
                self.config.base_url.trim_end_matches('/')
            )
        };

        let resp = self
            .http
            .get(&session_url)
            .bearer_auth(&self.config.access_token)
            .send()
            .await
            .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?;

        let session = resp
            .error_for_status()
            .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?
            .json::<JmapSession>()
            .await
            .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?;

        Ok(session)
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

    /// Build JMAP Email/query and Email/get request for a specific mailbox with pagination
    pub fn build_email_query_request(
        &self,
        mailbox_id: &str,
        position: usize,
        limit: usize,
    ) -> serde_json::Value {
        serde_json::json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            "methodCalls": [
                ["Email/query", {
                    "accountId": self.config.username,
                    "filter": { "inMailbox": mailbox_id },
                    "sort": [{ "property": "receivedAt", "isAscending": false }],
                    "position": position,
                    "limit": limit
                }, "0"],
                ["Email/get", {
                    "accountId": self.config.username,
                    "#ids": {
                        "resultOf": "0",
                        "name": "Email/query",
                        "path": "/ids"
                    },
                    "properties": ["id", "blobId", "threadId", "mailboxIds", "keywords", "size", "receivedAt", "from", "to", "subject", "preview", "bodyValues", "textBody", "htmlBody"]
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
    async fn sync_folder_list(
        &self,
    ) -> Result<Vec<RemoteFolder>, vespetrel_core::provider::ProviderError> {
        debug!(url=%self.config.base_url, "JMAP sync_folder_list");
        if self.config.base_url.starts_with("http") && !self.config.access_token.is_empty() {
            let req = self.build_get_mailboxes_request();
            let resp = self
                .http
                .post(&self.config.base_url)
                .bearer_auth(&self.config.access_token)
                .json(&req)
                .send()
                .await
                .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?;
            let json = resp
                .error_for_status()
                .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?
                .json::<serde_json::Value>()
                .await
                .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?;
            let folders = parse_jmap_mailbox_response(&json);
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
                    remote_id: "sent".into(),
                    name: "Sent".into(),
                    path: "Sent".into(),
                    role_hint: Some("sent".into()),
                    uid_validity: None,
                    highest_mod_seq: None,
                },
            ]);
        }

        #[cfg(not(any(test, feature = "mock")))]
        {
            Err(vespetrel_core::provider::ProviderError::Protocol(
                "JMAP server returned no mailboxes or endpoint unreachable".into(),
            ))
        }
    }

    async fn sync_messages(
        &self,
        folder: &Folder,
        state: SyncState,
    ) -> Result<SyncDelta, vespetrel_core::provider::ProviderError> {
        debug!(folder=%folder.name, state=?state.jmap_state, "JMAP sync_messages");
        if self.config.base_url.starts_with("http") && !self.config.access_token.is_empty() {
            let mut delta = SyncDelta::default();

            // If sinceState is present, attempt incremental Email/changes first per RFC 8621 §5.2
            if let Some(since) = &state.jmap_state {
                let changes_req = serde_json::json!({
                    "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
                    "methodCalls": [
                        ["Email/changes", {
                            "accountId": self.config.username,
                            "sinceState": since,
                            "maxChanges": 100
                        }, "0"]
                    ]
                });

                if let Ok(resp) = self
                    .http
                    .post(&self.config.base_url)
                    .bearer_auth(&self.config.access_token)
                    .json(&changes_req)
                    .send()
                    .await
                    && let Ok(json) = resp.json::<serde_json::Value>().await
                    && let Some(res_obj) = json.pointer("/methodResponses/0/1")
                    && let Some(new_state) = res_obj.get("newState").and_then(|v| v.as_str())
                {
                    delta.new_sync_state = SyncState {
                        jmap_state: Some(new_state.to_string()),
                        ..state.clone()
                    };

                    if let Some(destroyed) = res_obj.get("destroyed").and_then(|v| v.as_array()) {
                        for id in destroyed.iter().filter_map(|v| v.as_str()) {
                            delta
                                .deleted_uids
                                .push(vespetrel_core::stable_uid_from_id(id));
                        }
                    }

                    let created = res_obj
                        .get("created")
                        .and_then(|v| v.as_array())
                        .map(|a| a.as_slice())
                        .unwrap_or_default();
                    let updated = res_obj
                        .get("updated")
                        .and_then(|v| v.as_array())
                        .map(|a| a.as_slice())
                        .unwrap_or_default();

                    let ids_to_fetch: Vec<String> = created
                        .iter()
                        .chain(updated.iter())
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();

                    if !ids_to_fetch.is_empty() {
                        let fetch_req = serde_json::json!({
                            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
                            "methodCalls": [
                                ["Email/get", {
                                    "accountId": self.config.username,
                                    "ids": ids_to_fetch,
                                    "properties": ["id", "blobId", "threadId", "mailboxIds", "keywords", "size", "receivedAt", "from", "to", "subject", "preview", "bodyValues", "textBody", "htmlBody"]
                                }, "0"]
                            ]
                        });

                        if let Ok(fetch_resp) = self
                            .http
                            .post(&self.config.base_url)
                            .bearer_auth(&self.config.access_token)
                            .json(&fetch_req)
                            .send()
                            .await
                            && let Ok(fjson) = fetch_resp.json::<serde_json::Value>().await
                            && let Some(items) = fjson
                                .pointer("/methodResponses/0/1/list")
                                .and_then(|v| v.as_array())
                        {
                            for item in items {
                                let id =
                                    item.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                                let subject = item
                                    .get("subject")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("No Subject");
                                let from = item
                                    .pointer("/from/0/email")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("sender@jmap.example");
                                let date_str = chrono::Utc::now().to_rfc2822();
                                let body_text = item
                                    .get("preview")
                                    .and_then(|v| v.as_str())
                                    .or_else(|| {
                                        item.pointer("/bodyValues/1/value").and_then(|v| v.as_str())
                                    })
                                    .unwrap_or("JMAP Message Content");
                                let raw = format!("From: {from}\r\nTo: {}\r\nSubject: {subject}\r\nDate: {date_str}\r\nMessage-ID: <{id}@jmap.example>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body_text}", self.config.username).into_bytes();
                                let mut flags = Vec::new();
                                if let Some(keywords) =
                                    item.get("keywords").and_then(|k| k.as_object())
                                {
                                    if keywords.contains_key("$seen") {
                                        flags.push(vespetrel_core::Flag::Seen);
                                    }
                                    if keywords.contains_key("$flagged") {
                                        flags.push(vespetrel_core::Flag::Flagged);
                                    }
                                    if keywords.contains_key("$draft") {
                                        flags.push(vespetrel_core::Flag::Draft);
                                    }
                                }
                                let remote_uid = vespetrel_core::stable_uid_from_id(id);
                                let sync_msg = vespetrel_core::provider::SyncMessage {
                                    remote_uid,
                                    remote_id: Some(id.to_string()),
                                    flags,
                                    raw_rfc822: Some(raw),
                                    mod_seq: None,
                                };
                                if created.iter().any(|v| v.as_str() == Some(id)) {
                                    delta.inserted.push(sync_msg);
                                } else {
                                    delta.updated.push(sync_msg);
                                }
                            }
                        }
                    }
                    return Ok(delta);
                }
            }

            // Initial or full fallback query via Email/query + Email/get
            let mut position = 0;
            let page_size = 50;

            loop {
                let req = self.build_email_query_request(&folder.remote_id, position, page_size);
                let resp = self
                    .http
                    .post(&self.config.base_url)
                    .bearer_auth(&self.config.access_token)
                    .json(&req)
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

                if let Some(q_state) = json
                    .pointer("/methodResponses/0/1/queryState")
                    .and_then(|v| v.as_str())
                {
                    delta.new_sync_state = SyncState {
                        jmap_state: Some(q_state.to_string()),
                        ..state.clone()
                    };
                }

                let list = json
                    .pointer("/methodResponses/1/1/list")
                    .and_then(|v| v.as_array());

                let count = if let Some(items) = list {
                    let len = items.len();
                    for item in items {
                        let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                        let subject = item
                            .get("subject")
                            .and_then(|v| v.as_str())
                            .unwrap_or("No Subject");
                        let from = item
                            .pointer("/from/0/email")
                            .and_then(|v| v.as_str())
                            .unwrap_or("sender@jmap.example");
                        let date_str = chrono::Utc::now().to_rfc2822();
                        let body_text = item
                            .get("preview")
                            .and_then(|v| v.as_str())
                            .or_else(|| {
                                item.pointer("/bodyValues/1/value").and_then(|v| v.as_str())
                            })
                            .unwrap_or("JMAP Message Content");
                        let raw = format!("From: {from}\r\nTo: {}\r\nSubject: {subject}\r\nDate: {date_str}\r\nMessage-ID: <{id}@jmap.example>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body_text}", self.config.username).into_bytes();
                        let mut flags = Vec::new();
                        if let Some(keywords) = item.get("keywords").and_then(|k| k.as_object()) {
                            if keywords.contains_key("$seen") {
                                flags.push(vespetrel_core::Flag::Seen);
                            }
                            if keywords.contains_key("$flagged") {
                                flags.push(vespetrel_core::Flag::Flagged);
                            }
                            if keywords.contains_key("$draft") {
                                flags.push(vespetrel_core::Flag::Draft);
                            }
                        }
                        let remote_uid = vespetrel_core::stable_uid_from_id(id);

                        delta.inserted.push(vespetrel_core::provider::SyncMessage {
                            remote_uid,
                            remote_id: Some(id.to_string()),
                            flags,
                            raw_rfc822: Some(raw),
                            mod_seq: None,
                        });
                    }
                    len
                } else {
                    0
                };

                if count < page_size {
                    break;
                }
                position += count;
            }
            return Ok(delta);
        }
        info!(folder=%folder.name, "JMAP delta sync completed");
        Ok(SyncDelta::default())
    }

    async fn fetch_raw_message(
        &self,
        remote_id: &str,
    ) -> Result<Vec<u8>, vespetrel_core::provider::ProviderError> {
        debug!(remote_id, "JMAP fetch_raw_message");
        if self.config.base_url.starts_with("http") && !self.config.access_token.is_empty() {
            let download_url = format!(
                "{}/download/{}/{}",
                self.config.base_url.trim_end_matches('/'),
                self.config.username,
                remote_id
            );
            let resp = self
                .http
                .get(&download_url)
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
                "From: jmap@example.com\r\nSubject: JMAP {}\r\n\r\nJMAP Message Body",
                remote_id
            )
            .into_bytes());
        }

        #[cfg(not(any(test, feature = "mock")))]
        {
            Err(vespetrel_core::provider::ProviderError::Protocol(format!(
                "unable to fetch raw JMAP message for {remote_id}: invalid server connection"
            )))
        }
    }

    async fn send_message(
        &self,
        msg: &ComposedMessage,
    ) -> Result<(), vespetrel_core::provider::ProviderError> {
        info!(subject=%msg.subject, "JMAP EmailSubmission");
        if self.config.base_url.starts_with("http") && !self.config.access_token.is_empty() {
            let req = serde_json::json!({
                "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail", "urn:ietf:params:jmap:submission"],
                "methodCalls": [
                    [
                        "Email/set",
                        {
                            "accountId": self.config.username,
                            "create": {
                                "k1": {
                                    "mailboxIds": { "drafts": true },
                                    "subject": msg.subject,
                                    "bodyValues": {
                                        "body": {
                                            "value": msg.body_text
                                        }
                                    }
                                }
                            }
                        },
                        "c1"
                    ],
                    [
                        "EmailSubmission/set",
                        {
                            "accountId": self.config.username,
                            "create": {
                                "sub1": {
                                    "emailId": "#k1"
                                }
                            }
                        },
                        "c2"
                    ]
                ]
            });
            let resp = self
                .http
                .post(&self.config.base_url)
                .bearer_auth(&self.config.access_token)
                .json(&req)
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
        debug!(remote_ids=?remote_ids, add=?add, remove=?remove, "JMAP Email/set keywords by string ID");
        if self.config.base_url.starts_with("http") && !self.config.access_token.is_empty() {
            let mut patch_map = serde_json::Map::new();
            if add.contains(&Flag::Seen) {
                patch_map.insert("keywords/$seen".into(), serde_json::Value::Bool(true));
            }
            if remove.contains(&Flag::Seen) {
                patch_map.insert("keywords/$seen".into(), serde_json::Value::Null);
            }
            if add.contains(&Flag::Flagged) {
                patch_map.insert("keywords/$flagged".into(), serde_json::Value::Bool(true));
            }
            if remove.contains(&Flag::Flagged) {
                patch_map.insert("keywords/$flagged".into(), serde_json::Value::Null);
            }

            if !patch_map.is_empty() {
                let mut update_obj = serde_json::Map::new();
                for id in remote_ids {
                    update_obj.insert(id.clone(), serde_json::Value::Object(patch_map.clone()));
                }

                let req = serde_json::json!({
                    "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
                    "methodCalls": [
                        [
                            "Email/set",
                            {
                                "accountId": self.config.username,
                                "update": update_obj
                            },
                            "u1"
                        ]
                    ]
                });
                let resp = self
                    .http
                    .post(&self.config.base_url)
                    .bearer_auth(&self.config.access_token)
                    .json(&req)
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
