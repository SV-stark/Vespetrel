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
        Self { access_token: access_token.into(), base_url: GRAPH_BASE.into() }
    }

    pub fn delta_url(&self, folder_id: &str, delta_token: Option<&str>) -> String {
        if let Some(tok) = delta_token {
            format!("{}/me/mailFolders/{}/messages/delta?$deltatoken={}", self.base_url, folder_id, tok)
        } else {
            format!("{}/me/mailFolders/{}/messages/delta", self.base_url, folder_id)
        }
    }

    pub fn folders_url(&self) -> String {
        format!("{}/me/mailFolders", self.base_url)
    }
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
        let http = reqwest::Client::builder().user_agent("Vespetrel/0.1 Graph").build().unwrap_or_else(|_| reqwest::Client::new());
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
            RemoteFolder { remote_id: "inbox".into(), name: "Inbox".into(), path: "Inbox".into(), role_hint: Some("inbox".into()), uid_validity: None, highest_mod_seq: None },
            RemoteFolder { remote_id: "sentitems".into(), name: "Sent Items".into(), path: "Sent Items".into(), role_hint: Some("sent".into()), uid_validity: None, highest_mod_seq: None },
        ])
    }

    async fn sync_messages(&self, folder: &Folder, state: SyncState) -> anyhow::Result<SyncDelta> {
        let url = self.config.delta_url(&folder.remote_id, state.graph_delta_token.as_deref());
        debug!(folder=%folder.name, url=%url, "Graph delta query");
        // Real delta: GET url with bearer, parse @odata.deltaLink for next token
        info!(folder=%folder.name, "Graph delta sync stub");
        let mut new_state = SyncState::default();
        new_state.graph_delta_token = Some("stub-delta-token".into());
        Ok(SyncDelta { new_sync_state: new_state, ..Default::default() })
    }

    async fn fetch_raw_message(&self, remote_id: &str) -> anyhow::Result<Vec<u8>> {
        debug!(remote_id, "Graph fetch MIME");
        // Real: GET /me/messages/{id}/$value
        Ok(format!("From: graph@example.com\r\nSubject: Graph {}\r\n\r\nStub", remote_id).into_bytes())
    }

    async fn send_message(&self, msg: &ComposedMessage) -> anyhow::Result<()> {
        info!(subject=%msg.subject, "Graph sendMail");
        // Real: POST /me/sendMail
        Ok(())
    }

    async fn update_flags(&self, remote_ids: &[u32], add: &[Flag], remove: &[Flag]) -> anyhow::Result<()> {
        debug!(uids=?remote_ids, "Graph PATCH isRead/flag");
        // Real: PATCH /me/messages/{id} with {"isRead": true}
        let _ = (add, remove);
        Ok(())
    }
}
