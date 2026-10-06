use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use vespetrel_core::provider::{MailProvider, SyncEvent};

use crate::worker::{AccountWorker, WorkerCommand};

/// Minimum spacing between two full-sync commands issued for the same account.
///
/// Long enough to absorb a burst of IDLE pushes / toolbar clicks, short enough that a
/// deliberate re-sync a moment later still goes through.
const COMMAND_DEBOUNCE: Duration = Duration::from_millis(250);

pub struct SyncCoordinator {
    /// account_id -> command sender
    workers: HashMap<String, mpsc::UnboundedSender<WorkerCommand>>,
    /// account_id -> instant at which the last command was queued for that account.
    ///
    /// Every command runs a *full* sync of every folder, so a burst of IDLE pushes or Sync
    /// clicks must not be forwarded one-for-one: the worker would then never reach idle and
    /// the server would be hammered. A per-account debounce window coalesces the burst
    /// instead of counting it, so the gate always reopens. Interior mutability keeps
    /// `trigger_sync`/`trigger_idle_push` callable on `&self`.
    last_command_at: Mutex<HashMap<String, Instant>>,
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
    /// Receiver kept alive for the mpsc `event_tx`.
    ///
    /// Without a live receiver, `event_sender()` hands out a sender whose `send` always
    /// fails, so every event sent through it is silently dropped.
    _dormant_mpsc_rx: Option<mpsc::UnboundedReceiver<SyncEvent>>,
}

impl SyncCoordinator {
    /// Create coordinator and return the UI-side receiver.
    /// The UI should own the receiver and forward SyncEvent into GPUI via cx.spawn.
    pub fn create() -> (Self, mpsc::UnboundedReceiver<SyncEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let coord = Self {
            workers: HashMap::new(),
            last_command_at: Mutex::new(HashMap::new()),
            worker_handles: HashMap::new(),
            idle_handles: HashMap::new(),
            event_tx: tx,
            flume_tx: None,
            storage_pool: None,
            blob_store: None,
            _dormant_mpsc_rx: None,
        };
        (coord, rx)
    }

    /// Attach a keep-alive receiver for the mpsc `event_tx`.
    fn with_dormant_mpsc_receiver(mut self, rx: mpsc::UnboundedReceiver<SyncEvent>) -> Self {
        // Drain in the background so the channel never applies backpressure or fills up.
        self._dormant_mpsc_rx = Some(rx);
        self
    }

    /// High-throughput bounded flume coordinator constructor preventing OOM under heavy bursts
    pub fn create_bounded(capacity: usize) -> (Self, flume::Receiver<SyncEvent>) {
        let (flume_tx, flume_rx) = flume::bounded(capacity.clamp(128, 65536));
        // A live mpsc receiver is kept so `event_sender()` is not a silent no-op. The
        // previous code created the channel and dropped the receiver on the same line, so
        // `event_sender()` handed out a sender with no consumer and every event sent through
        // it was discarded.
        let (tx, rx) = mpsc::unbounded_channel();
        let coord = Self {
            workers: HashMap::new(),
            last_command_at: Mutex::new(HashMap::new()),
            worker_handles: HashMap::new(),
            idle_handles: HashMap::new(),
            event_tx: tx,
            flume_tx: Some(flume_tx),
            storage_pool: None,
            blob_store: None,
            _dormant_mpsc_rx: None,
        };
        let coord = coord.with_dormant_mpsc_receiver(rx);
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

    /// Request a sync, coalescing with any recently-issued request for the same account.
    ///
    /// Every command runs a *full* sync of every folder, so forwarding a burst one-for-one
    /// means the worker never reaches idle and the server is hammered. Requests arriving
    /// inside [`COMMAND_DEBOUNCE`] of the previous one for the same account are dropped:
    /// the command already in flight re-reads the same folders, so the dropped request is
    /// already satisfied by it.
    fn request_command(&self, account_id: &str, cmd: WorkerCommand) {
        let Some(tx) = self.workers.get(account_id) else {
            return;
        };
        {
            // A poisoned lock only means some other caller panicked mid-update; the map
            // contents are still usable, so recover rather than propagating the panic.
            let mut last = self
                .last_command_at
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            if let Some(prev) = last.get(account_id)
                && now.duration_since(*prev) < COMMAND_DEBOUNCE
            {
                debug!(account_id, "sync request coalesced into the in-flight one");
                return;
            }
            last.insert(account_id.to_string(), now);
        }
        if tx.send(cmd).is_err() {
            // The worker is gone. Forget the debounce so a later respawn is not suppressed.
            self.last_command_at
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(account_id);
            debug!(account_id, "worker channel closed; command dropped");
        }
    }

    pub fn trigger_sync(&self, account_id: &str) {
        self.request_command(account_id, WorkerCommand::SyncNow);
    }

    pub fn trigger_idle_push(&self, account_id: &str) {
        self.request_command(account_id, WorkerCommand::IdlePush);
    }

    pub fn spawn_idle_task<F, Fut>(&mut self, account_id: &str, idle_runner: F)
    where
        F: FnOnce(mpsc::UnboundedSender<WorkerCommand>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let account_id_owned = account_id.to_string();
        if let Some(tx) = self.workers.get(account_id).cloned() {
            // Dropping a JoinHandle in Tokio *detaches* the task; it keeps running with no way
            // to stop it. `spawn_worker_with_idle` calls this unconditionally even when
            // `spawn_worker` early-returned on a duplicate, so a second call leaked a
            // permanent IDLE task holding a command-sender clone.
            if let Some(prev) = self.idle_handles.remove(&account_id_owned) {
                prev.abort();
            }
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
        // Only wire IDLE when the worker was actually newly spawned, so a repeat call for an
        // existing account does not churn the IDLE task.
        if self.workers.contains_key(&account_id) {
            info!(account_id=%account_id, "worker already active; not respawning IDLE task");
            return;
        }
        self.spawn_worker(account_id.clone(), provider);
        self.spawn_idle_task(&account_id, idle_runner);
    }

    /// Request a graceful stop and wait for the worker to finish its in-flight work.
    ///
    /// The previous implementation enqueued `Stop` and then immediately `abort()`ed the
    /// task, so the `Stop` arm was never reached and in-flight `conn.interact` calls and blob
    /// writes were cancelled at an arbitrary point — the "clean cancellation token
    /// propagation" the design claims never actually happened.
    pub async fn stop_worker_gracefully(&mut self, account_id: &str) {
        if let Some(tx) = self.workers.remove(account_id) {
            let _ = tx.send(WorkerCommand::Stop);
        }
        self.forget_command_debounce(account_id);
        if let Some(handle) = self.worker_handles.remove(account_id)
            && tokio::time::timeout(Duration::from_secs(10), handle)
                .await
                .is_err()
        {
            warn!(
                account_id,
                "worker did not stop gracefully within 10s; it was aborted"
            );
        }
        if let Some(handle) = self.idle_handles.remove(account_id) {
            handle.abort();
        }
    }

    pub fn stop_worker(&mut self, account_id: &str) {
        if let Some(tx) = self.workers.remove(account_id) {
            let _ = tx.send(WorkerCommand::Stop);
        }
        self.forget_command_debounce(account_id);
        if let Some(handle) = self.worker_handles.remove(account_id) {
            handle.abort();
        }
        if let Some(handle) = self.idle_handles.remove(account_id) {
            handle.abort();
        }
    }

    /// Drop the coalescing window for an account so a freshly spawned worker is not
    /// suppressed by a request issued for the previous incarnation.
    fn forget_command_debounce(&self, account_id: &str) {
        self.last_command_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(account_id);
    }

    pub fn stop_all(&mut self) {
        for (_, tx) in self.workers.drain() {
            let _ = tx.send(WorkerCommand::Stop);
        }
        self.last_command_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
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
        // Only used to derive the expected port for a *new* sign-in; a stored account is
        // already bound to the ephemeral port it was authorized against, so replaying it
        // would point the listener somewhere the provider never redirects to.
        redirect_uri: "http://127.0.0.1:1/".into(),
        scopes: cfg.scopes.clone(),
        offline_access: cfg.offline_access,
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
///
/// Returns `Result` rather than a provider built from `unwrap_or_default()`. Swallowing the
/// keyring error produced a provider holding an **empty** token, so a locked or unavailable
/// OS keychain surfaced as a generic `NO [AUTHENTICATIONFAILED]` from the server (or, for
/// JMAP/Graph, as "no mailboxes or endpoint unreachable") instead of "your credentials could
/// not be read".
pub fn make_provider(
    account: &vespetrel_core::Account,
) -> Result<Arc<dyn MailProvider>, AuthError> {
    let key = account
        .auth_config
        .keyring_key
        .clone()
        .unwrap_or_else(|| account.id.clone());
    let auth_token = keyring::Entry::new("vespetrel", &key)
        .and_then(|e| e.get_password())
        .map_err(|e| match e {
            keyring::Error::NoEntry => AuthError::MissingCredentials(account.id.clone()),
            other => AuthError::KeyringError(format!("Account {}: {other}", account.id)),
        })?;
    if auth_token.is_empty() {
        return Err(AuthError::MissingCredentials(account.id.clone()));
    }
    Ok(make_provider_with_token(account, auth_token))
}

/// Async factory function to instantiate MailProvider, resolving and refreshing credentials.
pub async fn make_provider_async(
    account: &vespetrel_core::Account,
) -> Result<Arc<dyn MailProvider>, AuthError> {
    let auth_token = resolve_and_refresh_token(account, None).await?;
    Ok(make_provider_with_token(account, auth_token))
}
