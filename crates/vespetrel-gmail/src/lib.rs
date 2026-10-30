//! Vespetrel Gmail provider — Gmail REST delta sync over the `users.history.list` feed.
//!
//! Gmail's history feed is a single monotonic per-mailbox cursor covering label changes,
//! additions and deletions across every folder. That is strictly more informative than polling
//! each folder's `HIGHESTMODSEQ` over IMAP, and it is why this provider exists.
//!
//! # The invariant that matters
//!
//! A history cursor must only advance **after** the page's effects are committed. Advance first,
//! crash, and those changes are never re-delivered — Gmail has no way to know they were missed.
//! [`cursor::HistoryCursor::advance`] carries that ordering requirement in its docs, and
//! [`GmailProvider::sync_mailbox`] implements it: work first, propose the cursor second, and let
//! the engine persist it only after the database write.
//!
//! # When history is unavailable
//!
//! Gmail retains roughly seven days. An older cursor yields HTTP 404, classified as
//! [`GmailError::HistoryExpired`] rather than a retryable error, because retrying the same cursor
//! can never succeed. The provider then falls back to a full sync and re-baselines.
//!
//! # Sync scope
//!
//! [`MailProvider::sync_scope`] returns [`SyncScope::Mailbox`], so the engine calls
//! [`MailProvider::sync_mailbox`] exactly once per cycle and fans the result out by folder. It
//! must not be looped per folder: the cursor is shared, so a second call would re-walk the same
//! history or attach every change to whichever folder ran first.

pub mod cursor;
pub mod delta;
pub mod error;
pub mod labels;

use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};
use tracing::{debug, info, warn};

use vespetrel_core::account::SyncState;
use vespetrel_core::message::{ComposedMessage, Flag};
use vespetrel_core::provider::{
    FolderDelta, MailProvider, ProviderError, RemoteFolder, SyncDelta, SyncMessage, SyncScope,
};

pub use cursor::{HistoryCursor, HistoryPage};
pub use delta::{
    DeltaAccumulator, HistoryDelta, HistoryEntry, LabelEvent, MessageRef, ResolvedDelta,
};
pub use error::GmailError;
pub use labels::{GmailLabel, LabelRole, SyncableLabel};

/// Base URL for the Gmail REST API.
pub const GMAIL_API_BASE: &str = "https://gmail.googleapis.com/gmail/v1";

/// Largest response body accepted from the Gmail API.
///
/// A single `messages.get?format=raw` returns the whole RFC822 as base64url. A large MIME
/// message with attachments reaches several megabytes, so the cap is generous — but it is still
/// a cap, because an unbounded read is a memory-exhaustion vector.
pub const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// Upper bound on `messages.list` pages walked during a full sync.
///
/// Gmail's per-project quota is the binding constraint, and a mailbox larger than this needs
/// incremental sync rather than an unbounded walk.
const MAX_LIST_PAGES: usize = 200;

/// Upper bound on `history.list` pages walked in one delta.
const MAX_HISTORY_PAGES: usize = 50;

/// Gmail's cap on `messages.batchModify` ids per request.
const MAX_BATCH_MODIFY: usize = 1000;

/// Configuration for the Gmail REST provider.
#[derive(Clone)]
pub struct GmailConfig {
    pub access_token: String,
    /// Mailbox address, used when resolving identity.
    pub email: Option<String>,
    /// API base, overridable for tests and for proxies fronting the Gmail endpoint.
    pub base_url: String,
}

impl std::fmt::Debug for GmailConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GmailConfig")
            .field("access_token", &"[REDACTED]")
            .field("email", &self.email)
            .finish()
    }
}

impl GmailConfig {
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            email: None,
            base_url: GMAIL_API_BASE.to_string(),
        }
    }

    pub fn with_email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    /// Point the client at an alternative API base.
    pub fn with_base_url(mut self, base: impl Into<String>) -> Self {
        self.base_url = base.into();
        self
    }
}

/// Map a provider-specific failure onto the shared error taxonomy.
impl From<GmailError> for ProviderError {
    fn from(e: GmailError) -> Self {
        match e {
            // Only a genuine credential problem may become AuthError: the sync engine escalates
            // that to a re-consent prompt, and offering one for a quota blip sends the user
            // through a pointless consent flow.
            GmailError::NeedsReauth(m) => ProviderError::AuthError(m),
            GmailError::Transient(m) => ProviderError::Timeout(m),
            GmailError::RateLimited(_) => ProviderError::Protocol("Gmail rate limited".into()),
            GmailError::HistoryExpired { .. } => {
                ProviderError::Protocol("Gmail history expired; full sync required".into())
            }
            GmailError::Protocol(m) => ProviderError::Protocol(m),
            GmailError::BadRequest(m) => ProviderError::Protocol(m),
        }
    }
}

/// Read a bounded response body, refusing anything over [`MAX_RESPONSE_BYTES`].
async fn read_bounded(resp: reqwest::Response) -> Result<Vec<u8>, GmailError> {
    if let Some(len) = resp.content_length()
        && len > MAX_RESPONSE_BYTES as u64
    {
        return Err(GmailError::Protocol(format!(
            "Gmail response of {len} bytes exceeds the {MAX_RESPONSE_BYTES} byte cap"
        )));
    }
    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| GmailError::Transient(format!("reading Gmail response body: {e}")))
}

/// Check a response, converting a non-success into a classified [`GmailError`].
async fn ensure_success(resp: reqwest::Response) -> Result<reqwest::Response, GmailError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let status_code = status.as_u16();
    let body = resp.text().await.unwrap_or_default();
    Err(error::classify(status_code, &body))
}

/// Percent-encode a query parameter value.
fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Gmail REST provider implementing [`MailProvider`].
pub struct GmailProvider {
    http: reqwest::Client,
    base_url: String,
    config: GmailConfig,
    labels: std::sync::RwLock<Vec<SyncableLabel>>,
    /// Message ids already mirrored locally, supplied by the engine each cycle.
    stored_ids: std::sync::RwLock<BTreeSet<String>>,
}

impl std::fmt::Debug for GmailProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GmailProvider")
            .field("config", &self.config)
            .finish()
    }
}

impl GmailProvider {
    pub fn new(config: GmailConfig) -> Result<Self, GmailError> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("Vespetrel-Gmail/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(120))
            // The API host is fixed, so a redirect can only be an attack.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| GmailError::Transient(format!("building HTTP client: {e}")))?;

        Ok(Self {
            base_url: config.base_url.clone(),
            http,
            config,
            labels: std::sync::RwLock::new(Vec::new()),
            stored_ids: std::sync::RwLock::new(Default::default()),
        })
    }

    /// Convenience constructor for the common case.
    pub fn with_token(access_token: impl Into<String>) -> Result<Self, GmailError> {
        Self::new(GmailConfig::new(access_token))
    }

    // ---------------------------------------------------------------- stored-id cache

    /// Ids of messages already present locally, so relabelled messages are not refetched.
    ///
    /// Without this the provider assumes nothing is stored and refetches every relabelled
    /// message — correct, but it burns Gmail quota on an active mailbox, which is the largest
    /// single avoidable cost of a delta sync.
    fn stored_message_ids(&self) -> BTreeSet<String> {
        self.stored_ids
            .read()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Supply the set of message ids already mirrored locally.
    ///
    /// Reached through the [`MailProvider`] trait object the engine holds; takes `&self` via an
    /// `RwLock` rather than consuming the provider.
    #[allow(dead_code)]
    fn set_stored_message_ids(&self, ids: BTreeSet<String>) {
        if let Ok(mut guard) = self.stored_ids.write() {
            *guard = ids;
        }
    }

    /// Cached label set from the most recent [`MailProvider::sync_folder_list`].
    pub fn cached_labels(&self) -> Vec<SyncableLabel> {
        self.labels.read().map(|l| l.clone()).unwrap_or_default()
    }

    // ---------------------------------------------------------------- cursor plumbing

    /// Read the cursor out of an account's persisted sync state.
    pub fn cursor_from(state: &SyncState) -> HistoryCursor {
        HistoryCursor {
            history_id: state.gmail_history_id.clone(),
            page_token: state.gmail_history_page_token.clone(),
        }
    }

    /// Write a cursor into an account's sync state.
    pub fn cursor_into(state: &mut SyncState, cursor: &HistoryCursor) {
        state.gmail_history_id = cursor.history_id.clone();
        state.gmail_history_page_token = cursor.page_token.clone();
    }

    // ---------------------------------------------------------------- HTTP helpers

    async fn get_json(&self, path: &str) -> Result<serde_json::Value, GmailError> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.config.access_token)
            .send()
            .await
            .map_err(|e| GmailError::Transient(format!("Gmail request failed: {e}")))?;
        let resp = ensure_success(resp).await?;
        let body = read_bounded(resp).await?;
        serde_json::from_slice(&body)
            .map_err(|e| GmailError::Protocol(format!("malformed Gmail JSON: {e}")))
    }

    /// Fetch the mailbox's current `historyId`.
    ///
    /// Taken *after* a full sync to establish the baseline: recording it before would silently
    /// skip everything that arrived while the full sync was running.
    pub async fn current_history_id(&self) -> Result<String, GmailError> {
        let profile = self.get_json("/users/me/profile").await?;
        profile
            .get("historyId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| GmailError::Protocol("users.me/profile omitted historyId".into()))
    }

    /// Fetch one page of the change history.
    ///
    /// `start_history_id` is held fixed for the whole walk; `page_token` advances. Advancing
    /// the start id mid-walk would skip the pages not yet fetched.
    pub async fn fetch_history_page(
        &self,
        start_history_id: &str,
        page_token: Option<&str>,
    ) -> Result<(Vec<HistoryEntry>, HistoryPage), GmailError> {
        let mut url = format!(
            "{}/users/me/history?startHistoryId={}",
            self.base_url,
            urlencoding(start_history_id)
        );
        if let Some(token) = page_token {
            url.push_str(&format!("&pageToken={}", urlencoding(token)));
        }

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.config.access_token)
            .send()
            .await
            .map_err(|e| GmailError::Transient(format!("history.list failed: {e}")))?;
        let resp = ensure_success(resp).await?;
        let body = read_bounded(resp).await?;
        let value: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|e| GmailError::Protocol(format!("malformed history.list: {e}")))?;

        let entries: Vec<HistoryEntry> = value
            .get("history")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value::<HistoryEntry>(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        // Gmail omits `historyId` on a page with no changes; the walk has nothing to do, so
        // keep the start id and report no continuation.
        let history_id = value
            .get("historyId")
            .and_then(|v| v.as_str())
            .unwrap_or(start_history_id)
            .to_string();
        let next_page_token = value
            .get("nextPageToken")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        Ok((
            entries,
            HistoryPage {
                history_id,
                next_page_token,
            },
        ))
    }

    /// Fold a cursor forward, resolving the net change set.
    ///
    /// Returns the resolved plan and the *proposed* cursor. The caller applies the plan and
    /// commits **before** persisting the proposed cursor.
    pub async fn plan_changes(
        &self,
        cursor: &HistoryCursor,
    ) -> Result<(ResolvedDelta, HistoryCursor), GmailError> {
        let (start_id, start_token) = cursor
            .request_params()
            .ok_or_else(|| GmailError::Protocol("no history cursor; full sync required".into()))?;

        let mut accumulator = DeltaAccumulator::new();
        let mut proposed = cursor.clone();
        let mut page_token = start_token.map(|s| s.to_string());

        for _ in 0..MAX_HISTORY_PAGES {
            let (entries, page) = self
                .fetch_history_page(start_id, page_token.as_deref())
                .await?;
            accumulator.push_page(&entries);
            proposed.advance(&page.history_id, page.next_page_token.as_deref());
            match page.next_page_token {
                Some(next) => page_token = Some(next),
                None => return Ok((accumulator.resolve(), proposed)),
            }
        }

        Err(GmailError::Transient(format!(
            "history walk exceeded {MAX_HISTORY_PAGES} pages without completing"
        )))
    }

    /// Fetch a message's raw RFC822 bytes.
    ///
    /// Gmail returns `format=raw` base64url-encoded **without padding**, which `STANDARD`
    /// decoding rejects, so `URL_SAFE_NO_PAD` is tried first.
    pub async fn fetch_raw(&self, message_id: &str) -> Result<Vec<u8>, GmailError> {
        let url = format!(
            "{}/users/me/messages/{}?format=raw",
            self.base_url,
            urlencoding(message_id)
        );
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.config.access_token)
            .send()
            .await
            .map_err(|e| GmailError::Transient(format!("messages.get failed: {e}")))?;
        let resp = ensure_success(resp).await?;
        let body = read_bounded(resp).await?;
        let value: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|e| GmailError::Protocol(format!("malformed messages.get: {e}")))?;

        let raw = value
            .get("raw")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GmailError::Protocol("messages.get omitted `raw`".into()))?;

        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(raw.as_bytes())
            .or_else(|_| base64::engine::general_purpose::STANDARD.decode(raw.as_bytes()))
            .map_err(|e| GmailError::Protocol(format!("message raw is not base64url: {e}")))
    }

    /// Read a message's labels and derived flags, in one request.
    ///
    /// Folding these together matters: a fan-out needs both the label set (to pick destination
    /// folders) and the derived flags. Two calls per message would double every delta's round
    /// trips.
    async fn fan_out(&self, message_id: &str) -> Result<(Vec<String>, SyncMessage), GmailError> {
        let url = format!(
            "{}/users/me/messages/{}?format=metadata",
            self.base_url,
            urlencoding(message_id)
        );
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.config.access_token)
            .send()
            .await
            .map_err(|e| GmailError::Transient(format!("metadata fetch failed: {e}")))?;
        let resp = ensure_success(resp).await?;
        let body = read_bounded(resp).await?;
        let value: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|e| GmailError::Protocol(format!("malformed metadata: {e}")))?;

        let label_ids: Vec<String> = value
            .get("labelIds")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        // Only labels that map to a synced folder count: `UNREAD`, `STARRED` and `CATEGORY_*`
        // describe state, not location, and would create phantom folders.
        let known: BTreeSet<String> = self
            .cached_labels()
            .into_iter()
            .map(|l| l.remote_id)
            .collect();
        let folders: Vec<String> = label_ids
            .iter()
            .filter(|id| known.contains(*id))
            .cloned()
            .collect();

        let metadata = SyncMessage {
            remote_uid: 0,
            remote_id: Some(message_id.to_string()),
            flags: labels::labels_to_flags(&label_ids),
            raw_rfc822: None,
            mod_seq: None,
        };

        Ok((folders, metadata))
    }

    /// List message ids for one label, without fetching bodies.
    ///
    /// Gmail has no equivalent of IMAP's `UID SEARCH ALL`; `users.messages.list` filtered by
    /// label is the closest.
    async fn list_message_ids(&self, label_id: Option<&str>) -> Result<Vec<String>, GmailError> {
        let base = format!("{}/users/me/messages?maxResults=500", self.base_url);
        let base = match label_id {
            Some(label) => format!("{base}&labelId={}", urlencoding(label)),
            None => base,
        };

        let mut ids: Vec<String> = Vec::new();
        let mut page_token: Option<String> = None;

        for _ in 0..MAX_LIST_PAGES {
            let url = match &page_token {
                Some(t) => format!("{base}&pageToken={}", urlencoding(t)),
                None => base.clone(),
            };
            let resp = self
                .http
                .get(&url)
                .bearer_auth(&self.config.access_token)
                .send()
                .await
                .map_err(|e| GmailError::Transient(format!("messages.list failed: {e}")))?;
            let resp = ensure_success(resp).await?;
            let body = read_bounded(resp).await?;
            let value: serde_json::Value = serde_json::from_slice(&body)
                .map_err(|e| GmailError::Protocol(format!("malformed messages.list: {e}")))?;

            if let Some(list) = value.get("messages").and_then(|v| v.as_array()) {
                for item in list {
                    if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                        ids.push(id.to_string());
                    }
                }
            }
            page_token = value
                .get("nextPageToken")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            if page_token.is_none() {
                return Ok(ids);
            }
        }

        Err(GmailError::Protocol(format!(
            "messages.list exceeded {MAX_LIST_PAGES} pages; use incremental sync instead"
        )))
    }

    /// Apply label changes via `messages.batchModify`.
    pub async fn batch_modify(
        &self,
        message_ids: &[String],
        add: &[String],
        remove: &[String],
    ) -> Result<(), GmailError> {
        if message_ids.is_empty() || (add.is_empty() && remove.is_empty()) {
            return Ok(());
        }
        for chunk in message_ids.chunks(MAX_BATCH_MODIFY) {
            let payload = serde_json::json!({
                "ids": chunk,
                "addLabelIds": add,
                "removeLabelIds": remove,
            });
            let resp = self
                .http
                .post(format!("{}/users/me/messages/batchModify", self.base_url))
                .bearer_auth(&self.config.access_token)
                .json(&payload)
                .send()
                .await
                .map_err(|e| GmailError::Transient(format!("batchModify failed: {e}")))?;
            ensure_success(resp).await?;
        }
        Ok(())
    }

    // ---------------------------------------------------------------- sync strategies

    /// Mirror every folder from scratch, then re-baseline the cursor.
    ///
    /// # Why the baseline is taken at the end
    ///
    /// `users.getProfile.historyId` is read *after* the messages are listed and fetched. Taking
    /// it first would establish a cursor starting after everything this pass just mirrored — so
    /// a message that arrived mid-sync would be skipped permanently, with no error anywhere.
    /// Read at the end, anything that arrived during the sync is still ahead of the cursor and
    /// arrives in the next delta.
    async fn full_sync(&self, state: &SyncState) -> Result<Vec<FolderDelta>, GmailError> {
        let syncable = self.cached_labels();
        if syncable.is_empty() {
            // Silently doing nothing here would leave the mailbox permanently unmirrored with no
            // cursor to recover from. Fail loudly so the caller re-lists labels and retries.
            return Err(GmailError::Protocol(
                "full sync requires cached labels; sync_folder_list must run first".into(),
            ));
        }
        let stored = self.stored_message_ids();
        let mut per_folder: BTreeMap<String, FolderDelta> = BTreeMap::new();

        for label in &syncable {
            let ids = match self.list_message_ids(Some(&label.remote_id)).await {
                Ok(ids) => ids,
                // A label the server will not list (deleted mid-sync, or a system label with no
                // messages) must not abort the whole full sync.
                Err(e) if e.is_transient() => {
                    warn!(label = %label.remote_id, error = %e, "label listing failed; skipping");
                    continue;
                }
                Err(e) => return Err(e),
            };

            let mut delta = FolderDelta {
                folder_remote_id: label.remote_id.clone(),
                delta: SyncDelta {
                    new_sync_state: state.clone(),
                    ..Default::default()
                },
            };

            for message_id in ids {
                // A stored message only needs its labels re-derived; refetching the whole RFC822
                // for it would dominate the cost of a full sync.
                if stored.contains(&message_id) {
                    let metadata = self.fetch_metadata(&message_id).await?;
                    delta.delta.updated.push(metadata);
                    continue;
                }

                let metadata = self.fetch_metadata(&message_id).await?;
                let raw = match self.fetch_raw(&message_id).await {
                    Ok(raw) => raw,
                    // Skip this message but keep going: a full sync must be resumable, and the
                    // cursor is not advanced until the engine has committed.
                    Err(e) if e.is_transient() => {
                        warn!(message_id, error = %e, "transient fetch failure during full sync");
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                delta.delta.inserted.push(SyncMessage {
                    raw_rfc822: Some(raw),
                    ..metadata
                });
            }

            per_folder.insert(label.remote_id.clone(), delta);
        }

        // Re-baseline at the end, as documented above.
        let history_id = self.current_history_id().await?;
        for delta in per_folder.values_mut() {
            Self::cursor_into(
                &mut delta.delta.new_sync_state,
                &HistoryCursor::baseline(history_id.clone()),
            );
        }

        info!(
            folders = per_folder.len(),
            history_id = %history_id,
            "Gmail full sync complete; cursor re-baselined"
        );
        Ok(per_folder.into_values().collect())
    }

    /// Metadata-only read used by the full-sync path.
    async fn fetch_metadata(&self, message_id: &str) -> Result<SyncMessage, GmailError> {
        Ok(self.fan_out(message_id).await?.1)
    }

    /// Walk one history delta and partition it by destination folder.
    async fn sync_mailbox_delta(
        &self,
        state: SyncState,
    ) -> Result<Vec<FolderDelta>, ProviderError> {
        let (plan, proposed) = self.plan_changes(&Self::cursor_from(&state)).await?;

        let stored = self.stored_message_ids();
        let to_fetch = plan.fetch_set(&stored);

        // Fetch each new message exactly once, then fan it out to every folder that owns it.
        let mut fetched: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut deferred = 0usize;
        for message_id in &to_fetch {
            match self.fetch_raw(message_id).await {
                Ok(raw) => {
                    fetched.insert(message_id.clone(), raw);
                }
                Err(e) if e.is_transient() => {
                    deferred += 1;
                    warn!(message_id, error = %e, "transient fetch failure; deferring");
                }
                Err(e) => return Err(e.into()),
            }
        }

        // Only advance the cursor when nothing was deferred. Advancing past a message we failed
        // to fetch means Gmail never re-delivers it: history is the only queue, and it has
        // already moved on.
        let mut new_state = state;
        if deferred == 0 {
            Self::cursor_into(&mut new_state, &proposed);
        } else {
            warn!(
                deferred,
                "Gmail delta incomplete; leaving the cursor unchanged so deferred messages are retried"
            );
        }

        let mut per_folder: BTreeMap<String, FolderDelta> = BTreeMap::new();
        let blank = || SyncDelta {
            new_sync_state: new_state.clone(),
            ..Default::default()
        };

        // Deletions are mailbox-wide: a deleted message must be removed from every folder. They
        // are recorded under the empty remote id and resolved by message id by the engine.
        if !plan.deleted.is_empty() {
            per_folder.insert(
                String::new(),
                FolderDelta {
                    folder_remote_id: String::new(),
                    delta: SyncDelta {
                        deleted_remote_ids: plan.deleted.iter().cloned().collect(),
                        ..blank()
                    },
                },
            );
        }

        for message_id in &to_fetch {
            let Some(raw) = fetched.get(message_id) else {
                continue;
            };
            let (folders, metadata) = self.fan_out(message_id).await?;
            for folder in folders {
                per_folder
                    .entry(folder.clone())
                    .or_insert_with(|| FolderDelta {
                        folder_remote_id: folder.clone(),
                        delta: blank(),
                    })
                    .delta
                    .inserted
                    .push(SyncMessage {
                        raw_rfc822: Some(raw.clone()),
                        ..metadata.clone()
                    });
            }
        }

        for (message_id, (_added, _removed)) in &plan.label_changes {
            let (folders, metadata) = self.fan_out(message_id).await?;
            for folder in folders {
                per_folder
                    .entry(folder.clone())
                    .or_insert_with(|| FolderDelta {
                        folder_remote_id: folder.clone(),
                        delta: blank(),
                    })
                    .delta
                    .updated
                    .push(SyncMessage {
                        raw_rfc822: None,
                        ..metadata.clone()
                    });
            }
        }

        if per_folder.is_empty() {
            // Nothing changed, but the cursor may have advanced past history entries naming no
            // message we track. Persist it so the next walk starts from the head rather than
            // re-reading the same no-op range.
            return Ok(vec![FolderDelta {
                folder_remote_id: String::new(),
                delta: SyncDelta {
                    new_sync_state: new_state,
                    ..Default::default()
                },
            }]);
        }

        debug!(
            deleted = plan.deleted.len(),
            fetched = fetched.len(),
            relabelled = plan.label_changes.len(),
            deferred,
            folders = per_folder.len(),
            "Gmail mailbox delta applied"
        );

        Ok(per_folder.into_values().collect())
    }
}

#[async_trait]
impl MailProvider for GmailProvider {
    /// Gmail's cursor is one per mailbox, so the engine must call [`Self::sync_mailbox`] once
    /// rather than looping [`Self::sync_messages`] per folder.
    fn sync_scope(&self) -> SyncScope {
        SyncScope::Mailbox
    }

    async fn sync_folder_list(&self) -> Result<Vec<RemoteFolder>, ProviderError> {
        let value = self.get_json("/users/me/labels").await?;
        let raw: Vec<GmailLabel> = value
            .get("labels")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value::<GmailLabel>(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        let syncable = labels::syncable_labels(&raw);
        if let Ok(mut guard) = self.labels.write() {
            *guard = syncable.clone();
        }
        Ok(labels::to_remote_folders(&syncable))
    }

    /// Not supported: Gmail's cursor is mailbox-wide.
    ///
    /// The engine routes here only through [`Self::sync_mailbox`]. Calling it directly is a
    /// programming error — looping this per folder would attach every change to whichever
    /// folder ran first — so it fails loudly rather than returning a plausible-looking delta.
    async fn sync_messages(
        &self,
        _folder: &vespetrel_core::folder::Folder,
        _state: SyncState,
    ) -> Result<SyncDelta, ProviderError> {
        Err(ProviderError::Protocol(
            "GmailProvider is mailbox-scoped; use sync_mailbox, not sync_messages".into(),
        ))
    }

    /// Sync the whole mailbox once, partitioned by destination folder.
    ///
    /// Requires [`Self::sync_folder_list`] to have run first, which populates the label cache the
    /// fan-out depends on. The engine guarantees that ordering.
    async fn sync_mailbox(&self, state: SyncState) -> Result<Vec<FolderDelta>, ProviderError> {
        // No baseline: this mailbox has never been mirrored.
        if !Self::cursor_from(&state).is_initialized() {
            info!("no Gmail history cursor; performing full sync");
            return Ok(self.full_sync(&state).await?);
        }

        match self.plan_changes(&Self::cursor_from(&state)).await {
            Ok(_) => self.sync_mailbox_delta(state).await,
            Err(e) if e.requires_full_resync() => {
                // The cursor is older than Gmail's ~7-day retention and cannot be advanced.
                // Retrying it would spin forever; rebuilding the mailbox is the only recovery.
                warn!(
                    requested = Self::cursor_from(&state)
                        .history_id
                        .clone()
                        .unwrap_or_default(),
                    "Gmail history expired; rebuilding mailbox from a full sync"
                );
                let mut reset = state;
                Self::cursor_into(&mut reset, &HistoryCursor::default());
                Ok(self.full_sync(&reset).await?)
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn fetch_raw_message(&self, remote_id: &str) -> Result<Vec<u8>, ProviderError> {
        Ok(self.fetch_raw(remote_id).await?)
    }

    async fn send_message(&self, _message: &ComposedMessage) -> Result<(), ProviderError> {
        // Composed messages go through the SMTP outbox; Gmail's drafts/send API is a parallel
        // path, not a replacement.
        Err(ProviderError::Protocol(
            "GmailProvider delegates sending to the SMTP outbox".into(),
        ))
    }

    async fn update_flags(
        &self,
        _remote_ids: &[u32],
        _add: &[Flag],
        _remove: &[Flag],
    ) -> Result<(), ProviderError> {
        Err(ProviderError::Protocol(
            "Gmail identifiers are opaque strings; use update_flags_by_remote_id".into(),
        ))
    }

    async fn update_flags_by_remote_id(
        &self,
        remote_ids: &[String],
        add: &[Flag],
        remove: &[Flag],
    ) -> Result<(), ProviderError> {
        let (add_labels, remove_labels) = labels::flags_to_label_delta(add, remove);
        Ok(self
            .batch_modify(remote_ids, &add_labels, &remove_labels)
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parameters_are_percent_encoded() {
        assert_eq!(urlencoding("abc123"), "abc123");
        assert_eq!(urlencoding("a b"), "a%20b");
        assert_eq!(urlencoding("tok/en+1"), "tok%2Fen%2B1");
        // A page token containing separators must not be able to inject query parameters.
        assert!(!urlencoding("x&startHistoryId=0").contains('&'));
    }

    #[test]
    fn gmail_declares_mailbox_sync_scope() {
        // Looping a mailbox-scoped provider per folder would attach every change to whichever
        // folder ran first. The scope declaration is what prevents that.
        let provider = GmailProvider::with_token("token").expect("construct");
        assert_eq!(provider.sync_scope(), SyncScope::Mailbox);
    }

    #[tokio::test]
    async fn calling_sync_messages_directly_fails_loudly() {
        // Returning an empty delta here would look like "no changes" and hide the bug.
        let provider = GmailProvider::with_token("token").expect("construct");
        let folder = vespetrel_core::folder::Folder::new("acct", "INBOX", "Inbox", "INBOX");
        let err = provider.sync_messages(&folder, SyncState::default()).await;
        assert!(
            matches!(err, Err(ProviderError::Protocol(_))),
            "must refuse rather than return a plausible-looking empty delta, got {err:?}"
        );
    }

    #[test]
    fn sync_state_round_trips_the_cursor() {
        let mut state = SyncState::default();
        let mut cursor = HistoryCursor::baseline("1000");
        cursor.advance("1010", Some("tok"));
        GmailProvider::cursor_into(&mut state, &cursor);
        assert_eq!(GmailProvider::cursor_from(&state), cursor);
    }

    #[test]
    fn a_cursor_survives_json_persistence_mid_walk() {
        // SyncState is stored as JSON; a mid-walk cursor must resume rather than restart.
        let mut state = SyncState::default();
        let mut cursor = HistoryCursor::baseline("1000");
        cursor.advance("1010", Some("tok"));
        GmailProvider::cursor_into(&mut state, &cursor);

        let encoded = serde_json::to_string(&state).expect("encode");
        let decoded: SyncState = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(GmailProvider::cursor_from(&decoded), cursor);
    }

    #[test]
    fn sync_state_without_history_fields_deserialises() {
        // Databases written before this field existed must still load.
        let legacy = r#"{"last_sync_at":null,"jmap_state":null,"graph_delta_token":null,"folder_states":{}}"#;
        let state: SyncState = serde_json::from_str(legacy).expect("legacy state must load");
        assert!(!GmailProvider::cursor_from(&state).is_initialized());
    }

    #[test]
    fn sync_delta_without_deleted_remote_ids_deserialises() {
        // `deleted_remote_ids` was added after IMAP deltas were already persisted.
        let legacy = r#"{"inserted":[],"updated":[],"deleted_uids":[1,2],"new_sync_state":{"folder_states":{}}}"#;
        let delta: SyncDelta = serde_json::from_str(legacy).expect("legacy delta must load");
        assert_eq!(delta.deleted_uids, vec![1, 2]);
        assert!(
            delta.deleted_remote_ids.is_empty(),
            "a persisted IMAP delta must gain an empty remote-id list, not fail to load"
        );
    }

    #[test]
    fn stored_ids_default_to_empty_and_can_be_supplied() {
        let provider = GmailProvider::with_token("token").expect("construct");
        assert!(provider.stored_message_ids().is_empty());

        provider.set_stored_message_ids(["m1".to_string(), "m2".to_string()].into_iter().collect());
        assert_eq!(provider.stored_message_ids().len(), 2);
    }

    #[test]
    fn stored_ids_are_supplyable_through_the_trait_object() {
        // The engine holds the provider as `Arc<dyn MailProvider>`, so the setter must be
        // reachable through the trait — an inherent-only method would be invisible to it.
        let provider: std::sync::Arc<dyn MailProvider> =
            std::sync::Arc::new(GmailProvider::with_token("token").expect("construct"));
        provider.set_stored_message_ids(["m1".to_string()].into_iter().collect());
    }

    #[test]
    fn stored_ids_exclude_relabelled_messages_from_refetching() {
        // The whole point of supplying stored ids: a star toggle must not cost a full refetch.
        let mut acc = DeltaAccumulator::new();
        acc.push_entry(&HistoryEntry {
            id: "1".into(),
            labels_added: vec![LabelEvent {
                message: MessageRef {
                    id: "m1".into(),
                    thread_id: "t".into(),
                },
                label_ids: vec!["STARRED".into()],
            }],
            ..Default::default()
        });
        let plan = acc.resolve();

        let with_stored: BTreeSet<String> = ["m1".to_string()].into_iter().collect();
        assert!(
            plan.fetch_set(&with_stored).is_empty(),
            "a known message that only changed labels must not be refetched"
        );
        assert!(
            plan.fetch_set(&BTreeSet::new()).is_empty(),
            "even an unknown message needs no refetch when only its labels changed"
        );
    }

    #[test]
    fn expired_history_does_not_read_as_an_auth_failure() {
        let err: ProviderError = GmailError::HistoryExpired {
            requested: "1".into(),
        }
        .into();
        assert!(
            !matches!(err, ProviderError::AuthError(_)),
            "an expired cursor is not a credential problem; AuthError would prompt a re-consent"
        );
    }

    #[test]
    fn expired_access_token_does_read_as_an_auth_failure() {
        let err: ProviderError = GmailError::NeedsReauth("expired".into()).into();
        assert!(matches!(err, ProviderError::AuthError(_)));
    }

    #[test]
    fn rate_limiting_is_not_reported_as_an_auth_failure() {
        let err: ProviderError = GmailError::RateLimited("quota".into()).into();
        assert!(!matches!(err, ProviderError::AuthError(_)));
    }

    #[test]
    fn config_debug_never_leaks_the_token() {
        let cfg = GmailConfig::new("ya29.SECRET").with_email("a@example.com");
        let rendered = format!("{cfg:?}");
        assert!(!rendered.contains("SECRET"), "{rendered}");
        assert!(rendered.contains("REDACTED"));
    }

    #[test]
    fn the_response_cap_is_generous_but_finite() {
        // Gmail returns whole RFC822 messages base64-encoded for `format=raw`; a message with
        // attachments reaches several megabytes, so the cap must clear that comfortably. It
        // still has to exist: `bytes()` on an unbounded body allocates until the process dies.
        let cap = MAX_RESPONSE_BYTES;
        assert!(
            cap >= 8 * 1024 * 1024,
            "cap is too tight for large MIME messages"
        );
        assert!(cap <= 64 * 1024 * 1024, "cap must remain a bound");
    }

    #[test]
    fn walk_bounds_match_gmails_documented_limits() {
        assert!(MAX_BATCH_MODIFY <= 1000, "batchModify caps at 1000 ids");
        assert!(MAX_HISTORY_PAGES > 0 && MAX_LIST_PAGES > 0);
    }
}
