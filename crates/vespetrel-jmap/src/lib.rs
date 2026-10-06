//! Vespetrel JMAP - RFC 8620/8621, implemented directly on `reqwest`
//!
//! Requests are hand-built JSON-RPC `methodCalls` arrays and responses are read with `serde_json`.
//! There is no `jmap-client` dependency.

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

/// Make a server-supplied string safe to splice into an RFC 5322 header value.
///
/// CR/LF would let a hostile response inject arbitrary headers into the blob we store; NUL
/// is illegal in a header (RFC 5322 §3.2.6).
pub fn hdr_safe(v: &str) -> String {
    v.chars()
        .map(|c| match c {
            '\r' | '\n' | '\0' => ' ',
            other => other,
        })
        .collect()
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

    /// Resolve `(accountId, identityId, draftsMailboxId)` for a send.
    ///
    /// None of these can be derived from the username: RFC 8620 §1.2 requires an `Id` to be
    /// drawn from `[A-Za-z0-9_-]`, so an email address is not a valid `accountId`. The
    /// previous code used `self.config.username` for all three, which only worked against
    /// servers that happen to use the address as the id.
    async fn resolve_send_ids(
        &self,
    ) -> Result<(String, String, String), vespetrel_core::provider::ProviderError> {
        use vespetrel_core::provider::ProviderError;

        let session = self.discover_session().await?;
        let account_id = session
            .primary_accounts
            .as_ref()
            .and_then(|m| m.get("urn:ietf:params:jmap:mail").cloned())
            .unwrap_or_else(|| self.config.username.clone());

        let req = serde_json::json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail", "urn:ietf:params:jmap:submission"],
            "methodCalls": [
                ["Mailbox/get", { "accountId": account_id, "ids": null }, "0"],
                ["Identity/get", { "accountId": account_id, "ids": null }, "1"]
            ]
        });
        let json: serde_json::Value = self
            .http
            .post(&session.api_url)
            .bearer_auth(&self.config.access_token)
            .json(&req)
            .send()
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?
            .error_for_status()
            .map_err(|e| ProviderError::Protocol(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;
        check_jmap_response(&json).map_err(ProviderError::Protocol)?;

        let mailboxes = json
            .pointer("/methodResponses/0/1/list")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ProviderError::Protocol("JMAP Mailbox/get returned no list".into()))?;
        // RFC 8621 §2: role "drafts" identifies the Drafts mailbox by a server-assigned id.
        let drafts_id = mailboxes
            .iter()
            .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("drafts"))
            .and_then(|m| m.get("id").and_then(|i| i.as_str()))
            .ok_or_else(|| ProviderError::Protocol("JMAP account has no Drafts mailbox".into()))?
            .to_string();

        let identity_id = json
            .pointer("/methodResponses/1/1/list/0/id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProviderError::Protocol("JMAP Identity/get returned no identity".into())
            })?
            .to_string();

        Ok((account_id, identity_id, drafts_id))
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

/// Reject RFC 8620 §3.6.2 method-level errors and §5.3 `SetError` entries.
///
/// JMAP method-level failures are returned with **HTTP 200** by design, so `error_for_status()`
/// never fires and a request that did nothing looks like a success. Without this check the
/// client reported "sent" for mail that was never delivered.
pub fn check_jmap_response(json: &serde_json::Value) -> Result<(), String> {
    let Some(calls) = json.pointer("/methodResponses").and_then(|v| v.as_array()) else {
        return Err("JMAP response has no methodResponses array".into());
    };
    for call in calls {
        let name = call.get(0).and_then(|v| v.as_str()).unwrap_or("?");
        let args = call.get(1);
        if name == "error" {
            let ty = args
                .and_then(|a| a.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Err(format!("JMAP method-level error: {ty}"));
        }
        if let Some(args) = args {
            for key in ["notCreated", "notUpdated", "notDestroyed"] {
                if let Some(objs) = args.get(key).and_then(|v| v.as_object())
                    && let Some((id, e)) = objs.iter().next()
                {
                    let ty = e.get("type").and_then(|v| v.as_str()).unwrap_or("unknown");
                    return Err(format!("JMAP {key} failed for '{id}': {ty}"));
                }
            }
        }
    }
    Ok(())
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

                let json = match self
                    .http
                    .post(&self.config.base_url)
                    .bearer_auth(&self.config.access_token)
                    .json(&changes_req)
                    .send()
                    .await
                {
                    Ok(resp) => resp
                        .error_for_status()
                        .map_err(|e| {
                            vespetrel_core::provider::ProviderError::Protocol(e.to_string())
                        })?
                        .json::<serde_json::Value>()
                        .await
                        .map_err(|e| {
                            vespetrel_core::provider::ProviderError::Protocol(e.to_string())
                        })?,
                    // A transport failure must not advance the cursor. Returning an empty
                    // delta here while `new_sync_state` was still updated lost every change
                    // in the window permanently.
                    Err(e) => {
                        return Err(vespetrel_core::provider::ProviderError::Protocol(format!(
                            "Email/changes request failed: {e}"
                        )));
                    }
                };

                if check_jmap_response(&json).is_ok()
                    && let Some(res_obj) = json.pointer("/methodResponses/0/1")
                    && let Some(new_state) = res_obj.get("newState").and_then(|v| v.as_str())
                {
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

                        // The cursor may only be advanced once the payloads are in hand. A
                        // method-level error (HTTP 200 with `["error", ...]`) used to
                        // short-circuit the `if let` chain, leaving `inserted` empty while
                        // `newState` was already recorded, so those messages were never
                        // fetched again.
                        let fjson: serde_json::Value = self
                            .http
                            .post(&self.config.base_url)
                            .bearer_auth(&self.config.access_token)
                            .json(&fetch_req)
                            .send()
                            .await
                            .map_err(|e| {
                                vespetrel_core::provider::ProviderError::Protocol(e.to_string())
                            })?
                            .error_for_status()
                            .map_err(|e| {
                                vespetrel_core::provider::ProviderError::Protocol(e.to_string())
                            })?
                            .json()
                            .await
                            .map_err(|e| {
                                vespetrel_core::provider::ProviderError::Protocol(e.to_string())
                            })?;
                        check_jmap_response(&fjson).map_err(|e| {
                            vespetrel_core::provider::ProviderError::Protocol(format!(
                                "Email/get failed; refusing to advance the sync cursor: {e}"
                            ))
                        })?;
                        let items = fjson
                            .pointer("/methodResponses/0/1/list")
                            .and_then(|v| v.as_array())
                            .ok_or_else(|| {
                                vespetrel_core::provider::ProviderError::Protocol(
                                    "JMAP Email/get returned no list; refusing to advance the sync cursor".into(),
                                )
                            })?;
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
                                // Server-controlled values land in header positions; CR/LF would let a
                                // hostile response inject headers into the blob we store.
                                let raw = format!("From: {}\r\nTo: {}\r\nSubject: {}\r\nDate: {date_str}\r\nMessage-ID: <{}@jmap.example>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}", hdr_safe(from), hdr_safe(&self.config.username), hdr_safe(subject), hdr_safe(id), body_text.replace('\0', "")).into_bytes();
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

                    // RFC 8620 §5.2: `hasMoreChanges` means the server truncated the
                    // change list. The previous code stored `newState` and returned, so
                    // everything beyond `maxChanges` was silently dropped and never
                    // re-delivered, because the next sync's `sinceState` was already past it.
                    if res_obj
                        .get("hasMoreChanges")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        // Fall through to a full folder query rather than losing the tail.
                        tracing::warn!(
                            folder=%folder.name,
                            "Email/changes reported hasMoreChanges; falling back to a full query to avoid dropping changes"
                        );
                    } else {
                        // Every payload is in hand: only now is it safe to move the cursor.
                        delta.new_sync_state = SyncState {
                            jmap_state: Some(new_state.to_string()),
                            ..state.clone()
                        };
                        return Ok(delta);
                    }
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
                        // Server-controlled values land in header positions; CR/LF would let a
                        // hostile response inject headers into the blob we store.
                        let raw = format!("From: {}\r\nTo: {}\r\nSubject: {}\r\nDate: {date_str}\r\nMessage-ID: <{}@jmap.example>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}", hdr_safe(from), hdr_safe(&self.config.username), hdr_safe(subject), hdr_safe(id), body_text.replace('\0', "")).into_bytes();
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
        use vespetrel_core::provider::ProviderError;

        info!(subject=%msg.subject, "JMAP EmailSubmission");
        if !(self.config.base_url.starts_with("http") && !self.config.access_token.is_empty()) {
            return Err(ProviderError::Protocol(
                "JMAP send requires a base URL and an access token".into(),
            ));
        }
        if msg.to.is_empty() {
            return Err(ProviderError::Protocol(
                "refusing to send a message with no recipients".into(),
            ));
        }

        // Resolve the server-assigned account id, Drafts mailbox id and identity id.
        // `self.config.username` is an email address, and RFC 8620 §1.2 requires an `Id` to
        // be drawn from [A-Za-z0-9_-] — an `@` is not permitted, so the username is not a
        // valid accountId. Using it worked only against servers that happen to use the
        // address as the id.
        let (account_id, identity_id, drafts_id) = self.resolve_send_ids().await?;

        let addr = |a: &vespetrel_core::Address| serde_json::json!({ "email": a.email });
        let addrs = |list: &[vespetrel_core::Address]| -> Vec<serde_json::Value> {
            list.iter().map(addr).collect()
        };

        let mut body_values = serde_json::Map::new();
        body_values.insert(
            "1".into(),
            serde_json::json!({ "value": msg.body_text, "isTruncated": false }),
        );
        let mut email = serde_json::Map::new();
        email.insert("mailboxIds".into(), serde_json::json!({ drafts_id: true }));
        email.insert("from".into(), serde_json::json!([addr(&msg.from)]));
        email.insert("to".into(), serde_json::json!(addrs(&msg.to)));
        if !msg.cc.is_empty() {
            email.insert("cc".into(), serde_json::json!(addrs(&msg.cc)));
        }
        if !msg.bcc.is_empty() {
            email.insert("bcc".into(), serde_json::json!(addrs(&msg.bcc)));
        }
        email.insert("subject".into(), serde_json::json!(msg.subject));
        if let Some(irt) = &msg.in_reply_to {
            email.insert("inReplyTo".into(), serde_json::json!([irt]));
        }
        // RFC 8621 §5.1: bodyValues must be referenced by a textBody/htmlBody part. The
        // previous request had `bodyValues: {"body": ...}` with no part referencing it.
        email.insert("bodyValues".into(), serde_json::Value::Object(body_values));
        email.insert(
            "textBody".into(),
            serde_json::json!([{ "partId": "1", "type": "text/plain" }]),
        );
        if msg.body_html.is_some() {
            email.insert(
                "htmlBody".into(),
                serde_json::json!([{ "partId": "2", "type": "text/html" }]),
            );
        }

        let req = serde_json::json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail", "urn:ietf:params:jmap:submission"],
            "methodCalls": [
                [
                    "Email/set",
                    {
                        "accountId": account_id,
                        "create": { "k1": serde_json::Value::Object(email) }
                    },
                    "c1"
                ],
                [
                    "EmailSubmission/set",
                    {
                        "accountId": account_id,
                        // RFC 8621 §7.1: identityId is mandatory.
                        "create": {
                            "sub1": { "emailId": "#k1", "identityId": identity_id, "onSuccessUpdateEmail": "#k1" }
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
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;
        let json = resp
            .error_for_status()
            .map_err(|e| ProviderError::Protocol(e.to_string()))?
            .json::<serde_json::Value>()
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        // Method-level errors arrive with HTTP 200, so `error_for_status` alone would report
        // a successful send for a request that did nothing.
        check_jmap_response(&json).map_err(ProviderError::Protocol)?;
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
                let json = resp
                    .error_for_status()
                    .map_err(|e| vespetrel_core::provider::ProviderError::Protocol(e.to_string()))?
                    .json::<serde_json::Value>()
                    .await
                    .map_err(|e| {
                        vespetrel_core::provider::ProviderError::Protocol(e.to_string())
                    })?;
                // `Email/set` reports per-id failures in `notUpdated`. Checking only the
                // HTTP status reported success even when every id failed to update, so
                // read/star state silently never reached the server.
                check_jmap_response(&json).map_err(|e| {
                    vespetrel_core::provider::ProviderError::Protocol(format!(
                        "JMAP Email/set keyword update failed: {e}"
                    ))
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
