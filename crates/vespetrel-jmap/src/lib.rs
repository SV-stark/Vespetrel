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
    pub fn new(base_url: impl Into<String>, username: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), username: username.into(), access_token: access_token.into() }
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
}

#[async_trait]
impl MailProvider for JmapProvider {
    async fn sync_folder_list(&self) -> anyhow::Result<Vec<RemoteFolder>> {
        debug!(url=%self.config.base_url, "JMAP sync_folder_list");
        // Real: POST /.well-known/jmap + session discovery -> Mailbox/get
        // Stub
        Ok(vec![
            RemoteFolder { remote_id: "inbox".into(), name: "Inbox".into(), path: "Inbox".into(), role_hint: Some("inbox".into()), uid_validity: None, highest_mod_seq: None },
            RemoteFolder { remote_id: "sent".into(), name: "Sent".into(), path: "Sent".into(), role_hint: Some("sent".into()), uid_validity: None, highest_mod_seq: None },
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
        Ok(format!("From: jmap@example.com\r\nSubject: JMAP {}\r\n\r\nStub", remote_id).into_bytes())
    }

    async fn send_message(&self, msg: &ComposedMessage) -> anyhow::Result<()> {
        info!(subject=%msg.subject, "JMAP EmailSubmission");
        // Real: Email/set create + EmailSubmission/create
        Ok(())
    }

    async fn update_flags(&self, remote_ids: &[u32], add: &[Flag], remove: &[Flag]) -> anyhow::Result<()> {
        debug!(uids=?remote_ids, add=?add, remove=?remove, "JMAP Email/set keywords");
        Ok(())
    }
}
