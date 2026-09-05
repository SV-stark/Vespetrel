use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::info;

use vespetrel_core::provider::{MailProvider, SyncEvent};

use crate::worker::{AccountWorker, WorkerCommand};

pub struct SyncCoordinator {
    /// account_id -> command sender
    workers: HashMap<String, mpsc::UnboundedSender<WorkerCommand>>,
    /// worker join handles for supervision and graceful shutdown (account_id -> handle)
    worker_handles: HashMap<String, tokio::task::JoinHandle<()>>,
    /// IDLE task handles per account
    idle_handles: HashMap<String, tokio::task::JoinHandle<()>>,
    /// UI event sender (Tokio mpsc -> GPUI)
    event_tx: mpsc::UnboundedSender<SyncEvent>,
    /// Bounded event sender for backpressure control
    flume_tx: Option<flume::Sender<SyncEvent>>,
    /// Optional shared SQLite storage pool
    storage_pool: Option<deadpool_sqlite::Pool>,
    /// Optional shared BlobStore for persisting message RFC822 bodies
    blob_store: Option<Arc<vespetrel_storage::blob::BlobStore>>,
}

impl SyncCoordinator {
    /// Create coordinator and return the UI-side receiver.
    /// The UI should own the receiver and forward SyncEvent into GPUI via cx.spawn.
    pub fn create() -> (Self, mpsc::UnboundedReceiver<SyncEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let coord = Self {
            workers: HashMap::new(),
            worker_handles: HashMap::new(),
            idle_handles: HashMap::new(),
            event_tx: tx,
            flume_tx: None,
            storage_pool: None,
            blob_store: None,
        };
        (coord, rx)
    }

    /// High-throughput bounded flume coordinator constructor preventing OOM under heavy bursts
    pub fn create_bounded(capacity: usize) -> (Self, flume::Receiver<SyncEvent>) {
        let (flume_tx, flume_rx) = flume::bounded(capacity.clamp(128, 65536));
        let (tx, _rx) = mpsc::unbounded_channel();
        let coord = Self {
            workers: HashMap::new(),
            worker_handles: HashMap::new(),
            idle_handles: HashMap::new(),
            event_tx: tx,
            flume_tx: Some(flume_tx),
            storage_pool: None,
            blob_store: None,
        };
        (coord, flume_rx)
    }

    /// High-throughput flume channel constructor for zero-contention cross-thread event streaming
    pub fn create_flume_bridge() -> (flume::Sender<SyncEvent>, flume::Receiver<SyncEvent>) {
        flume::unbounded()
    }

    /// High-throughput bounded flume channel constructor to prevent OOM under heavy bursts
    pub fn create_flume_bridge_bounded(
        capacity: usize,
    ) -> (flume::Sender<SyncEvent>, flume::Receiver<SyncEvent>) {
        flume::bounded(capacity.clamp(128, 65536))
    }

    pub fn with_storage_pool(mut self, pool: deadpool_sqlite::Pool) -> Self {
        self.storage_pool = Some(pool);
        self
    }

    pub fn with_blob_store(mut self, blob_store: Arc<vespetrel_storage::blob::BlobStore>) -> Self {
        self.blob_store = Some(blob_store);
        self
    }

    pub fn event_sender(&self) -> mpsc::UnboundedSender<SyncEvent> {
        self.event_tx.clone()
    }

    pub fn flume_sender(&self) -> Option<flume::Sender<SyncEvent>> {
        self.flume_tx.clone()
    }

    pub fn spawn_worker(&mut self, account_id: impl Into<String>, provider: Arc<dyn MailProvider>) {
        let account_id = account_id.into();
        if self.workers.contains_key(&account_id) {
            info!(account_id=%account_id, "worker already active for account, skipping duplicate spawn");
            return;
        }
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let mut worker = if let Some(flume_tx) = &self.flume_tx {
            AccountWorker::new_with_flume(account_id.clone(), provider, flume_tx.clone(), cmd_rx)
        } else {
            AccountWorker::new(account_id.clone(), provider, self.event_tx.clone(), cmd_rx)
        };
        if let Some(pool) = &self.storage_pool {
            worker = worker.with_storage_pool(pool.clone());
        }
        if let Some(store) = &self.blob_store {
            worker = worker.with_blob_store(store.clone());
        }
        let handle = tokio::spawn(worker.run());
        self.worker_handles.insert(account_id.clone(), handle);
        self.workers.insert(account_id.clone(), cmd_tx);
        info!(account_id=%account_id, "spawned worker with storage wiring");
    }

    pub fn trigger_sync(&self, account_id: &str) {
        if let Some(tx) = self.workers.get(account_id) {
            let _ = tx.send(WorkerCommand::SyncNow);
        }
    }

    pub fn trigger_idle_push(&self, account_id: &str) {
        if let Some(tx) = self.workers.get(account_id) {
            let _ = tx.send(WorkerCommand::IdlePush);
        }
    }

    pub fn spawn_idle_task<F, Fut>(&mut self, account_id: &str, idle_runner: F)
    where
        F: FnOnce(mpsc::UnboundedSender<WorkerCommand>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let account_id_owned = account_id.to_string();
        if let Some(tx) = self.workers.get(account_id).cloned() {
            let handle = tokio::spawn(idle_runner(tx));
            self.idle_handles.insert(account_id_owned.clone(), handle);
            info!(account_id=%account_id_owned, "spawned IDLE background task");
        }
    }

    /// Spawns an account worker and automatically wires a background IDLE task using the provided idle runner.
    pub fn spawn_worker_with_idle<F, Fut>(
        &mut self,
        account_id: impl Into<String>,
        provider: Arc<dyn MailProvider>,
        idle_runner: F,
    ) where
        F: FnOnce(mpsc::UnboundedSender<WorkerCommand>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let account_id = account_id.into();
        self.spawn_worker(account_id.clone(), provider);
        self.spawn_idle_task(&account_id, idle_runner);
    }

    pub fn stop_worker(&mut self, account_id: &str) {
        if let Some(tx) = self.workers.remove(account_id) {
            let _ = tx.send(WorkerCommand::Stop);
        }
        if let Some(handle) = self.worker_handles.remove(account_id) {
            handle.abort();
        }
        if let Some(handle) = self.idle_handles.remove(account_id) {
            handle.abort();
        }
    }

    pub fn stop_all(&mut self) {
        for (_, tx) in self.workers.drain() {
            let _ = tx.send(WorkerCommand::Stop);
        }
        for (_, handle) in self.worker_handles.drain() {
            handle.abort();
        }
        for (_, handle) in self.idle_handles.drain() {
            handle.abort();
        }
    }
}

impl Drop for SyncCoordinator {
    fn drop(&mut self) {
        self.stop_all();
    }
}

/// Builds a concrete MailProvider instance with the given credentials.
pub fn make_provider_with_token(
    account: &vespetrel_core::Account,
    auth_token: String,
) -> Arc<dyn MailProvider> {
    match account.provider_type {
        vespetrel_core::ProviderType::Imap => {
            let host = account.auth_config.server_host.clone().unwrap_or_else(|| {
                let domain = account.email.split('@').nth(1).unwrap_or("localhost");
                match domain {
                    "outlook.com" | "hotmail.com" | "live.com" | "office365.com" => {
                        "outlook.office365.com".to_string()
                    }
                    "gmail.com" => "imap.gmail.com".to_string(),
                    "yahoo.com" => "imap.mail.yahoo.com".to_string(),
                    "icloud.com" => "imap.mail.me.com".to_string(),
                    other => format!("imap.{other}"),
                }
            });
            let port = account.auth_config.server_port.unwrap_or(993);
            let config = vespetrel_imap::ImapConfig::new(host, port, &account.email, auth_token);
            Arc::new(vespetrel_imap::ImapProvider::new(config))
        }
        vespetrel_core::ProviderType::Gmail => {
            let host = account
                .auth_config
                .server_host
                .clone()
                .unwrap_or_else(|| "imap.gmail.com".to_string());
            let port = account.auth_config.server_port.unwrap_or(993);
            let config = vespetrel_imap::ImapConfig::new(host, port, &account.email, auth_token)
                .with_xoauth2();
            Arc::new(vespetrel_imap::ImapProvider::new(config))
        }
        vespetrel_core::ProviderType::Jmap => {
            let domain = account.email.split('@').nth(1).unwrap_or("localhost");
            let base_url = format!("https://{domain}/jmap");
            let config = vespetrel_jmap::JmapConfig::new(base_url, &account.email, auth_token);
            Arc::new(vespetrel_jmap::JmapProvider::new(config))
        }
        vespetrel_core::ProviderType::Graph => {
            let config = vespetrel_graph::GraphConfig::new(auth_token);
            Arc::new(vespetrel_graph::GraphProvider::new(config))
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Keyring entry missing or inaccessible: {0}")]
    KeyringError(String),
    #[error("OAuth token refresh failed for account {account_id}: {reason}")]
    OAuthRefreshFailed { account_id: String, reason: String },
    #[error("Missing authentication credentials for account {0}")]
    MissingCredentials(String),
}

pub fn oauth_config_from_core(
    cfg: &vespetrel_core::account::OAuthConfig,
) -> vespetrel_crypto::OAuth2Config {
    vespetrel_crypto::OAuth2Config {
        client_id: cfg.client_id.clone(),
        client_secret: None,
        auth_url: cfg.auth_url.clone(),
        token_url: cfg.token_url.clone(),
        redirect_uri: cfg.redirect_uri.clone(),
        scopes: cfg.scopes.clone(),
    }
}

/// Resolves the auth token for an account, performing proactive OAuth2 token refresh
/// if the token is expired or within 60s of expiring.
pub async fn resolve_and_refresh_token(
    account: &vespetrel_core::Account,
    storage_pool: Option<&deadpool_sqlite::Pool>,
) -> Result<String, AuthError> {
    let now_ts = chrono::Utc::now().timestamp();
    let is_expired = account
        .auth_config
        .expires_at
        .map(|exp| now_ts >= exp - 60)
        .unwrap_or(false);

    if is_expired
        && let (Some(rk), Some(oauth_cfg)) = (
            &account.auth_config.refresh_token_keyring_key,
            &account.auth_config.oauth,
        )
    {
        let rk_owned = rk.clone();
        let refresh_token = tokio::task::spawn_blocking(move || {
            keyring::Entry::new("vespetrel", &rk_owned)
                .and_then(|e| e.get_password())
                .ok()
        })
        .await
        .unwrap_or(None);

        if let Some(ref rt) = refresh_token {
            let engine = vespetrel_crypto::OAuth2Engine::new(oauth_config_from_core(oauth_cfg));
            match engine.refresh_access_token(rt).await {
                Ok(bundle) => {
                    info!(account_id=%account.id, "successfully refreshed OAuth2 access token");
                    let new_access = bundle.access_token.clone();
                    let new_refresh = bundle.refresh_token.clone();
                    let new_expires_at = now_ts + bundle.expires_in as i64;

                    let ak_key = account
                        .auth_config
                        .keyring_key
                        .clone()
                        .unwrap_or_else(|| account.id.clone());
                    let rk_key = rk.clone();
                    let access_copy = new_access.clone();
                    tokio::task::spawn_blocking(move || {
                        if let Ok(entry) = keyring::Entry::new("vespetrel", &ak_key) {
                            let _ = entry.set_password(&access_copy);
                        }
                        if let (Some(new_rt), rk) = (new_refresh, rk_key)
                            && let Ok(entry) = keyring::Entry::new("vespetrel", &rk)
                        {
                            let _ = entry.set_password(&new_rt);
                        }
                    })
                    .await
                    .map_err(|e| AuthError::KeyringError(e.to_string()))?;

                    if let Some(pool) = storage_pool
                        && let Ok(conn) = pool.get().await
                    {
                        let mut updated = account.clone();
                        updated.auth_config.expires_at = Some(new_expires_at);
                        let _ = conn
                            .interact(move |c| vespetrel_storage::repo::upsert_account(c, &updated))
                            .await;
                    }

                    return Ok(new_access);
                }
                Err(e) => {
                    tracing::warn!(account_id=%account.id, error=%e, "OAuth2 proactive refresh failed, falling back to cached token");
                }
            }
        }
    }

    let key_to_lookup = account
        .auth_config
        .keyring_key
        .clone()
        .unwrap_or_else(|| account.id.clone());
    let token = tokio::task::spawn_blocking(move || {
        keyring::Entry::new("vespetrel", &key_to_lookup)
            .and_then(|e| e.get_password())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| AuthError::KeyringError(e.to_string()))?
    .map_err(|e| AuthError::KeyringError(format!("Account {}: {e}", account.id)))?;

    if token.is_empty() {
        return Err(AuthError::MissingCredentials(account.id.clone()));
    }

    Ok(token)
}

/// Factory function to instantiate the concrete MailProvider implementation for an account.
pub fn make_provider(account: &vespetrel_core::Account) -> Arc<dyn MailProvider> {
    let auth_token = if let Some(ref k) = account.auth_config.keyring_key {
        keyring::Entry::new("vespetrel", k)
            .and_then(|e| e.get_password())
            .unwrap_or_default()
    } else {
        keyring::Entry::new("vespetrel", &account.id)
            .and_then(|e| e.get_password())
            .unwrap_or_default()
    };

    make_provider_with_token(account, auth_token)
}

/// Async factory function to instantiate MailProvider, resolving and refreshing credentials
pub async fn make_provider_async(account: &vespetrel_core::Account) -> Arc<dyn MailProvider> {
    let auth_token = resolve_and_refresh_token(account, None)
        .await
        .unwrap_or_default();
    make_provider_with_token(account, auth_token)
}
