use mail_parser::MessageParser;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use vespetrel_core::provider::{
    MailProvider, RemoteFolder, SyncDelta, SyncEvent, SyncMessage, SyncScope,
};

/// A connection checked out of the storage pool.
///
/// `deadpool_sqlite` exports no alias for the handle `Pool::get` returns, so the parameters are
/// spelled out once here rather than at every signature. The `rusqlite` version must match the
/// workspace pin: a mismatch makes this resolve to a *different* type that happens to compile,
/// which fails at the call site rather than here.
type PooledConn = deadpool::managed::Object<deadpool_sqlite::Manager>;

/// Upper bound on the plain-text preview stored in the `body_text_preview` column.
///
/// The full first text part was previously stored verbatim, so a multi-megabyte body was
/// serialised into SQLite on every message and then handed to the spam classifier, which
/// allocates one `String` per token.
const MAX_BODY_PREVIEW_CHARS: usize = 4096;

/// Convert a parsed RFC 5322 `Date` header into a UTC timestamp.
///
/// Returns `None` for the all-zero value mail-parser produces when the header is missing or
/// unparsable, so a bogus date cannot overwrite a good default with 1970.
fn mail_date_to_utc(d: &mail_parser::DateTime) -> Option<chrono::DateTime<chrono::Utc>> {
    if d.year == 0 && d.month == 0 && d.day == 0 {
        return None;
    }
    let naive =
        chrono::NaiveDate::from_ymd_opt(i32::from(d.year), u32::from(d.month), u32::from(d.day))?
            .and_hms_opt(u32::from(d.hour), u32::from(d.minute), u32::from(d.second))?;

    // `tz_before_gmt` is false for "+HHMM"; the sign applies to the offset from UTC.
    let sign = if d.tz_before_gmt { -1i32 } else { 1i32 };
    let offset = chrono::FixedOffset::east_opt(
        sign * (i32::from(d.tz_hour) * 3600 + i32::from(d.tz_minute) * 60),
    )?;
    Some(
        naive
            .and_local_timezone(offset)
            .single()
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|| chrono::DateTime::from_naive_utc_and_offset(naive, chrono::Utc)),
    )
}

#[derive(Debug)]
pub enum WorkerCommand {
    SyncNow,
    IdlePush,
    Stop,
    UpdateFlags {
        folder_remote_id: String,
        uids: Vec<u32>,
        add: Vec<vespetrel_core::message::Flag>,
        remove: Vec<vespetrel_core::message::Flag>,
    },
}

#[derive(Clone)]
pub enum WorkerEventSender {
    Mpsc(mpsc::UnboundedSender<SyncEvent>),
    Flume(flume::Sender<SyncEvent>),
}

impl WorkerEventSender {
    /// Emit a sync event to the UI.
    ///
    /// This is a *synchronous* function called from `sync_once`, which runs on a Tokio
    /// worker thread. `flume::Sender::send` blocks when a bounded channel is full (the
    /// async variant is `send_async`), so a UI that fell behind would block a runtime
    /// thread indefinitely — and once one worker per runtime thread is blocked, the entire
    /// runtime is wedged: no timers, no other tasks, no UI event loop. `create_bounded`
    /// exists precisely to bound memory, which makes the OOM it prevents turn into a
    /// deadlock.
    ///
    /// Use `try_send` and drop the event when the consumer is behind. The dropped events
    /// (`SyncError`, `MessageFlagsUpdated`) are advisory, and `MessagesInserted` is
    /// re-derived from the database on the next refresh.
    pub fn send(&self, ev: SyncEvent) {
        match self {
            Self::Mpsc(tx) => {
                let _ = tx.send(ev);
            }
            Self::Flume(tx) => {
                if let Err(flume::TrySendError::Full(_ev)) = tx.try_send(ev) {
                    debug!(
                        "sync event channel full; dropping SyncEvent rather than blocking the runtime"
                    );
                }
            }
        }
    }
}

/// One Actor per account - §3.2 Sync Engine
pub struct AccountWorker {
    pub account_id: String,
    pub provider: Arc<dyn MailProvider>,
    pub event_tx: WorkerEventSender,
    pub cmd_rx: mpsc::UnboundedReceiver<WorkerCommand>,
    pub poll_interval: Duration,
    pub storage_pool: Option<deadpool_sqlite::Pool>,
    pub blob_store: Option<Arc<vespetrel_storage::blob::BlobStore>>,
    pub classifier: Option<Arc<crate::spam::BayesClassifier>>,
}

impl AccountWorker {
    pub fn new(
        account_id: impl Into<String>,
        provider: Arc<dyn MailProvider>,
        event_tx: mpsc::UnboundedSender<SyncEvent>,
        cmd_rx: mpsc::UnboundedReceiver<WorkerCommand>,
    ) -> Self {
        Self {
            account_id: account_id.into(),
            provider,
            event_tx: WorkerEventSender::Mpsc(event_tx),
            cmd_rx,
            poll_interval: Duration::from_secs(60),
            storage_pool: None,
            blob_store: None,
            classifier: None,
        }
    }

    pub fn new_with_flume(
        account_id: impl Into<String>,
        provider: Arc<dyn MailProvider>,
        event_tx: flume::Sender<SyncEvent>,
        cmd_rx: mpsc::UnboundedReceiver<WorkerCommand>,
    ) -> Self {
        Self {
            account_id: account_id.into(),
            provider,
            event_tx: WorkerEventSender::Flume(event_tx),
            cmd_rx,
            poll_interval: Duration::from_secs(60),
            storage_pool: None,
            blob_store: None,
            classifier: None,
        }
    }

    pub fn with_storage_pool(mut self, pool: deadpool_sqlite::Pool) -> Self {
        self.storage_pool = Some(pool);
        self
    }

    pub fn with_blob_store(mut self, blob_store: Arc<vespetrel_storage::blob::BlobStore>) -> Self {
        self.blob_store = Some(blob_store);
        self
    }

    pub fn with_classifier(mut self, classifier: Arc<crate::spam::BayesClassifier>) -> Self {
        self.classifier = Some(classifier);
        self
    }

    pub async fn run(mut self) {
        info!(account_id = %self.account_id, "starting account worker");
        let mut interval = tokio::time::interval(self.poll_interval);
        let mut backoff = Duration::from_secs(1);
        let max_backoff = Duration::from_secs(60);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match self.sync_once().await {
                        Ok(_) => {
                            backoff = Duration::from_secs(1);
                            let jitter_ms = rand::random::<u64>() % 2000;
                            interval.reset_after(self.poll_interval + Duration::from_millis(jitter_ms));
                        }
                        Err(e) => {
                            warn!(account_id=%self.account_id, error=%e, backoff_secs=backoff.as_secs(), "periodic sync failed, backing off");
                            self.emit(SyncEvent::SyncError{ folder: "all".into(), error: e.to_string() });
                            interval.reset_after(backoff);
                            backoff = (backoff * 2).min(max_backoff);
                        }
                    }
                }
                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(WorkerCommand::SyncNow) => {
                            info!(account_id=%self.account_id, "manual full sync requested");
                            match self.sync_once().await {
                                Ok(_) => {
                                    backoff = Duration::from_secs(1);
                                }
                                Err(e) => {
                                    error!(error=%e, "manual sync failed");
                                    self.emit(SyncEvent::SyncError{ folder: "all".into(), error: e.to_string() });
                                }
                            }
                        }
                        Some(WorkerCommand::IdlePush) => {
                            debug!(account_id=%self.account_id, "realtime IDLE push received, running incremental delta sync");
                            if let Err(e) = self.sync_once().await {
                                debug!(error=%e, "incremental IDLE sync failed");
                            }
                        }
                        Some(WorkerCommand::UpdateFlags{ folder_remote_id, uids, add, remove }) => {
                            let mut remote_ids = Vec::new();
                            if let Some(pool) = &self.storage_pool {
                                // `messages.folder_id` stores the *local* folder UUID (it
                                // references folders(id)), not the provider's remote id. The
                                // old predicate matched the remote id against that column and
                                // therefore matched zero rows, so `update_flags_by_remote_id`
                                // was never used and the `u32`-based fallback ran instead —
                                // which 404s for JMAP and Graph, so read/star state never
                                // reached Microsoft 365 or Fastmail.
                                let acct = self.account_id.clone();
                                let rid = folder_remote_id.clone();
                                let folder_db_id =
                                    match pool.get().await.ok().map(|c| async move {
                                        c.interact(move |conn| {
                                            vespetrel_storage::repo::get_folder(conn, &acct, &rid)
                                        })
                                        .await
                                    }) {
                                        Some(fut) => match fut.await {
                                            Ok(Ok(Some(f))) => Some(f.id),
                                            _ => None,
                                        },
                                        None => None,
                                    };
                                match folder_db_id {
                                    None => {
                                        warn!(
                                            account_id=%self.account_id,
                                            folder=%folder_remote_id,
                                            "no local folder row for that remote id; skipping flag push"
                                        );
                                    }
                                    Some(fid) => {
                                        if let Ok(conn) = pool.get().await {
                                            let uids_c = uids.clone();
                                        let q = conn.interact(move |c| -> anyhow::Result<Vec<String>> {
                                            let mut ids = Vec::new();
                                            for chunk in uids_c.chunks(500) {
                                                let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                                                let sql = format!("SELECT remote_id FROM messages WHERE folder_id = ? AND remote_uid IN ({placeholders}) AND remote_id IS NOT NULL");
                                                let mut stmt = c.prepare(&sql)?;
                                                let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(chunk.len() + 1);
                                                params.push(&fid);
                                                for u in chunk { params.push(u); }
                                                let rows = stmt.query_map(rusqlite::params_from_iter(params), |r| r.get::<_, String>(0))?;
                                                for id_res in rows { ids.push(id_res?); }
                                            }
                                            Ok(ids)
                                        }).await;
                                            if let Ok(Ok(ids)) = q {
                                                remote_ids = ids;
                                            }
                                        }
                                    }
                                }
                            }
                            let res = if !remote_ids.is_empty() {
                                self.provider.update_flags_by_remote_id(&remote_ids, &add, &remove).await
                            } else {
                                self.provider.update_flags(&uids, &add, &remove).await
                            };
                            if let Err(e) = res {
                                error!(error=%e, "update_flags failed");
                            }
                        }
                        Some(WorkerCommand::Stop) | None => {
                            info!(account_id=%self.account_id, "stopping worker");
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Refresh the account's OAuth token and rebuild the provider.
    ///
    /// The connection is scoped so it returns to the pool *before* the refresh runs. The
    /// previous code held it across `resolve_and_refresh_token`, which itself checks out
    /// another connection — so the worker needed two at once, on top of the one `sync_once`
    /// already holds. With `pool_size == 1` (the `:memory:` / headless configuration) the
    /// second checkout could never be satisfied, so auth refresh always failed and the
    /// original 401 was retried forever.
    pub async fn try_refresh_auth(&mut self) -> anyhow::Result<()> {
        let pool = match &self.storage_pool {
            Some(p) => p.clone(),
            None => anyhow::bail!("cannot refresh auth: no storage pool configured"),
        };
        let acct = {
            let aid = self.account_id.clone();
            let conn = pool.get().await?;
            conn.interact(move |c| vespetrel_storage::repo::get_account(c, &aid))
                .await
                .map_err(|e| anyhow::anyhow!("interact failed: {e}"))??
                .ok_or_else(|| {
                    anyhow::anyhow!("account {} not found in storage", self.account_id)
                })?
            // `conn` is dropped here, releasing the pool slot.
        };

        let new_token = crate::coordinator::resolve_and_refresh_token(&acct, Some(&pool))
            .await
            .map_err(|e| anyhow::anyhow!("token refresh failed: {e}"))?;

        self.provider = crate::coordinator::make_provider_with_token(&acct, new_token);
        info!(account_id=%self.account_id, "successfully refreshed token and updated provider");
        Ok(())
    }

    async fn sync_once(&mut self) -> anyhow::Result<()> {
        let mut folders_res = self.provider.sync_folder_list().await;
        if let Err(ref e) = folders_res {
            let err_str = e.to_string();
            if err_str.contains("401")
                || err_str.to_lowercase().contains("unauthorized")
                || err_str.to_lowercase().contains("authenticate")
            {
                warn!(account_id=%self.account_id, "authentication error in sync_folder_list, attempting OAuth refresh");
                if self.try_refresh_auth().await.is_ok() {
                    folders_res = self.provider.sync_folder_list().await;
                }
            }
        }
        let folders = folders_res?;

        // Acquire connection from storage pool if available with 5s timeout
        let storage_conn = match &self.storage_pool {
            Some(pool) => match tokio::time::timeout(Duration::from_secs(5), pool.get()).await {
                Ok(Ok(conn)) => Some(conn),
                Ok(Err(e)) => {
                    anyhow::bail!("Storage unavailable for account {}: {}", self.account_id, e);
                }
                Err(_) => {
                    anyhow::bail!(
                        "Timed out acquiring connection from storage pool for account {}",
                        self.account_id
                    );
                }
            },
            None => None,
        };

        let mut folder_records: ahash::AHashMap<String, vespetrel_core::Folder> =
            ahash::AHashMap::with_capacity(folders.len());

        // Load the existing rows first so the upsert preserves state the server does not
        // re-send. `Folder::new` hardcodes role=Custom, uid_validity=None,
        // highest_mod_seq=0, total_count=0, unread_count=0 and color=None, and the upsert
        // overwrites all of those — so before this fix every sync cycle (every 60s) reset
        // role to 'custom' and counts to 0. Since `list_unified_inbox_messages` filters on
        // `role = 'inbox'`, the unified inbox was permanently empty, unread badges read 0,
        // and user-set folder colours were erased.
        let existing_folders: ahash::AHashMap<String, vespetrel_core::Folder> = if let Some(conn) =
            &storage_conn
        {
            let acct_id = self.account_id.clone();
            let remote_ids: Vec<String> = folders.iter().map(|f| f.remote_id.clone()).collect();
            match conn
                .interact(
                    move |c| -> rusqlite::Result<ahash::AHashMap<String, vespetrel_core::Folder>> {
                        let mut map = ahash::AHashMap::with_capacity(remote_ids.len());
                        for rid in &remote_ids {
                            if let Ok(Some(prev)) =
                                vespetrel_storage::repo::get_folder(c, &acct_id, rid)
                            {
                                map.insert(rid.clone(), prev);
                            }
                        }
                        Ok(map)
                    },
                )
                .await
            {
                Ok(Ok(map)) => map,
                Ok(Err(e)) => {
                    warn!(error=%e, "failed to read existing folders; their metadata will be re-derived");
                    ahash::AHashMap::new()
                }
                Err(e) => {
                    warn!(error=%e, "storage interact error reading existing folders");
                    ahash::AHashMap::new()
                }
            }
        } else {
            ahash::AHashMap::new()
        };

        for rf in &folders {
            let mut folder_record =
                vespetrel_core::Folder::new(&self.account_id, &rf.remote_id, &rf.name, &rf.path);
            // Inferred from the server's SPECIAL-USE / well-known name. `Folder::infer_role`
            // existed but was never called anywhere, so every folder was stored as 'custom'.
            folder_record.role =
                vespetrel_core::Folder::infer_role(&rf.name, rf.role_hint.as_deref());
            if let Some(prev) = existing_folders.get(&rf.remote_id) {
                // Server-assigned cursor and local-only state survive the upsert.
                folder_record.id = prev.id.clone();
                folder_record.uid_validity = prev.uid_validity;
                folder_record.highest_mod_seq = prev.highest_mod_seq;
                folder_record.total_count = prev.total_count;
                folder_record.unread_count = prev.unread_count;
                folder_record.color = prev.color.clone();
            }
            folder_records.insert(rf.remote_id.clone(), folder_record);
        }

        // Persist folder metadata to storage in a single atomic transaction.
        // `upsert_folder` returns the id that actually persisted, which is NOT the one
        // `Folder::new` just generated once the (account_id, remote_id) row already exists.
        // Messages must reference the persisted id or the FK on messages.folder_id fails.
        if let Some(conn) = &storage_conn {
            let recs: Vec<vespetrel_core::Folder> = folder_records.values().cloned().collect();
            let res = conn
                .interact(
                    move |c| -> anyhow::Result<ahash::AHashMap<String, String>> {
                        let mut persisted: ahash::AHashMap<String, String> =
                            ahash::AHashMap::with_capacity(recs.len());
                        {
                            let tx = c.transaction()?;
                            for rec in &recs {
                                let db_id = vespetrel_storage::repo::upsert_folder(&tx, rec)?;
                                persisted.insert(rec.remote_id.clone(), db_id);
                            }
                            tx.commit()?;
                        }
                        Ok(persisted)
                    },
                )
                .await;
            match res {
                Ok(Ok(persisted)) => {
                    for (remote_id, record) in folder_records.iter_mut() {
                        if let Some(db_id) = persisted.get(remote_id) {
                            record.id = db_id.clone();
                        }
                    }
                }
                Ok(Err(e)) => {
                    warn!(error=%e, "failed to batch upsert folders");
                }
                Err(e) => {
                    warn!(error=%e, "storage interact error batch upserting folders");
                }
            }
        }

        // Emit updated folder list after persisting to database
        self.emit(SyncEvent::FolderListUpdated(folders.clone()));

        // Load persistent sync state for account from storage
        let mut account_sync_state = vespetrel_core::account::SyncState::default();
        if let Some(conn) = &storage_conn {
            let acct_id = self.account_id.clone();
            if let Ok(Ok(Some(acct))) = conn
                .interact(move |c| vespetrel_storage::repo::get_account(c, &acct_id))
                .await
            {
                account_sync_state = acct.sync_state;
            }
        }

        // A mailbox-scoped provider (Gmail) is called exactly once and returns deltas already
        // partitioned by folder. Iterating it per folder instead would re-walk the same shared
        // cursor N times and attach every change to whichever folder ran first.
        if self.provider.sync_scope() == SyncScope::Mailbox {
            return self
                .sync_mailbox_cycle(&folders, &folder_records, account_sync_state, storage_conn)
                .await;
        }

        for rf in &folders {
            let folder_record = folder_records
                .get(&rf.remote_id)
                .cloned()
                .unwrap_or_else(|| {
                    vespetrel_core::Folder::new(&self.account_id, &rf.remote_id, &rf.name, &rf.path)
                });
            let folder_db_id = folder_record.id.clone();
            let state = account_sync_state.clone();

            let mut sync_res = self
                .provider
                .sync_messages(&folder_record, state.clone())
                .await;
            if let Err(ref e) = sync_res {
                let err_str = e.to_string();
                if err_str.contains("401")
                    || err_str.to_lowercase().contains("unauthorized")
                    || err_str.to_lowercase().contains("authenticate")
                {
                    warn!(account_id=%self.account_id, "authentication error in sync_messages, attempting OAuth refresh");
                    if self.try_refresh_auth().await.is_ok() {
                        sync_res = self.provider.sync_messages(&folder_record, state).await;
                    }
                }
            }

            match sync_res {
                Ok(delta) => {
                    let mut summaries = Vec::new();
                    let mut msgs_to_store = Vec::new();
                    let mut blobs_to_write = Vec::new();
                    // Cleared to false by any message we cannot materialise. The sync cursor
                    // must not advance while messages are still missing, or the server will
                    // never re-deliver them: IMAP uses CHANGEDSINCE <modseq> and JMAP
                    // sinceState, both of which are past the dropped message forever.
                    let mut ingest_complete = true;

                    for sync_msg in &delta.inserted {
                        // `fetch_raw_message` takes the provider's own id. Passing the
                        // numeric `remote_uid` 404s for JMAP and Graph, whose ids are not
                        // the sequence-derived UIDs the provider synthesised.
                        let fetch_id = sync_msg
                            .remote_id
                            .clone()
                            .unwrap_or_else(|| sync_msg.remote_uid.to_string());
                        let raw_bytes_opt = match &sync_msg.raw_rfc822 {
                            Some(b) if !b.is_empty() => Some(b.clone()),
                            Some(_) => {
                                // Empty payload from the provider: fetch it properly.
                                match self.provider.fetch_raw_message(&fetch_id).await {
                                    Ok(b) if !b.is_empty() => Some(b),
                                    Ok(_) => None,
                                    Err(e) => {
                                        error!(
                                            remote_uid = sync_msg.remote_uid,
                                            folder = %rf.name,
                                            error=%e,
                                            "failed to fetch message body; aborting this folder's sync so the cursor is not advanced past it"
                                        );
                                        // Transient failure. Leave the cursor alone so the
                                        // next sync re-fetches this message.
                                        return Ok(());
                                    }
                                }
                            }
                            None => match self.provider.fetch_raw_message(&fetch_id).await {
                                Ok(b) if !b.is_empty() => Some(b),
                                Ok(_) => None,
                                Err(e) => {
                                    error!(
                                        remote_uid = sync_msg.remote_uid,
                                        folder = %rf.name,
                                        error=%e,
                                        "failed to fetch message body; aborting this folder's sync so the cursor is not advanced past it"
                                    );
                                    return Ok(());
                                }
                            },
                        };

                        let raw_bytes = match raw_bytes_opt {
                            Some(ref b) if !b.is_empty() => b.clone(),
                            _ => {
                                warn!(
                                    remote_uid = sync_msg.remote_uid,
                                    folder = %rf.name,
                                    "server returned no message body; not advancing the sync cursor past it"
                                );
                                ingest_complete = false;
                                continue;
                            }
                        };

                        let parsed = match MessageParser::default().parse(&raw_bytes) {
                            Some(p) => p,
                            None => {
                                warn!(
                                    remote_uid = sync_msg.remote_uid,
                                    folder = %rf.name,
                                    "failed to parse RFC822 message; not advancing the sync cursor past it"
                                );
                                ingest_complete = false;
                                continue;
                            }
                        };

                        let from_addr = match parsed
                            .from()
                            .and_then(|f| f.first())
                            .and_then(|a| a.address.as_deref())
                        {
                            Some(a) if !a.is_empty() => a.to_string(),
                            _ => {
                                warn!(
                                    remote_uid = sync_msg.remote_uid,
                                    folder = %rf.name,
                                    "message has no From address; not advancing the sync cursor past it"
                                );
                                ingest_complete = false;
                                continue;
                            }
                        };

                        let from_name = parsed
                            .from()
                            .and_then(|f| f.first())
                            .and_then(|a| a.name.as_deref())
                            .map(|s| s.to_string());
                        let to_addrs = parsed
                            .to()
                            .map(|t| {
                                t.iter()
                                    .filter_map(|a| a.address.as_deref())
                                    .map(|s| s.to_string())
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();

                        let subject = parsed.subject().unwrap_or("No Subject").to_string();

                        let mut msg = vespetrel_core::Message::new(
                            &self.account_id,
                            &folder_db_id,
                            sync_msg.remote_uid,
                            &subject,
                            from_addr,
                            to_addrs,
                        );
                        msg.remote_id = sync_msg.remote_id.clone();
                        msg.from_name = from_name;
                        msg.message_id_header = parsed.message_id().map(|s| s.to_string());
                        msg.in_reply_to = parsed.in_reply_to().as_text().map(|s| s.to_string());
                        msg.references = parsed.references().as_text().map(|s| s.to_string());
                        msg.body_snippet = parsed
                            .body_text(0)
                            .map(|t| t.chars().take(200).collect::<String>());
                        // Cap the preview. Storing the entire first text part meant a
                        // multi-megabyte body was serialised into a SQLite column on every
                        // message and then fed to the spam classifier.
                        msg.body_text_preview = parsed
                            .body_text(0)
                            .map(|t| t.chars().take(MAX_BODY_PREVIEW_CHARS).collect::<String>());

                        // `Message::new` sets sent_at = received_at = now(). Without reading
                        // the Date header every synced message got the *sync* time, so
                        // `ORDER BY sent_at DESC` returned a mailbox in essentially random
                        // order and the order was unstable across re-syncs.
                        if let Some(dt) = parsed.date().and_then(mail_date_to_utc) {
                            msg.sent_at = dt;
                            msg.received_at = dt;
                        }

                        // `has_attachments` stayed false for every message, so a
                        // `FilterField::HasAttachments` rule never matched anything.
                        msg.has_attachments = parsed.attachment_count() > 0;

                        // cc/bcc/reply-to were never populated, so Reply targeted the From
                        // address instead of Reply-To.
                        msg.cc_addresses = parsed
                            .cc()
                            .map(|list| {
                                list.iter()
                                    .filter_map(|a| {
                                        a.address.as_deref().map(|e| vespetrel_core::Address {
                                            name: a.name.as_deref().map(|s| s.to_string()),
                                            email: e.to_string(),
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        msg.bcc_addresses = parsed
                            .bcc()
                            .map(|list| {
                                list.iter()
                                    .filter_map(|a| {
                                        a.address.as_deref().map(|e| vespetrel_core::Address {
                                            name: a.name.as_deref().map(|s| s.to_string()),
                                            email: e.to_string(),
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        // `reply_to` is a list, matching the storage column.
                        msg.reply_to = parsed.reply_to().map(|list| {
                            list.iter()
                                .filter_map(|addr| {
                                    addr.address().map(|e| vespetrel_core::Address {
                                        name: addr.name().map(|s| s.to_string()),
                                        email: e.to_string(),
                                    })
                                })
                                .collect()
                        });

                        msg.size_bytes = raw_bytes.len() as i64;
                        let shard = &msg.id[..2.min(msg.id.len())];
                        msg.blob_path = format!("{shard}/{}.lz4", msg.id);

                        // Deterministic thread ID, namespaced by account.
                        //
                        // `threads.id` is a bare PRIMARY KEY, and the previous hash input
                        // contained no account id, so two accounts receiving the same
                        // `In-Reply-To` (any shared mailing list) collided on one row: B's
                        // message overwrote A's, and because `list_threads` filters on
                        // account_id the thread then vanished from A's list entirely.
                        use sha2::Digest;
                        let thread_id = {
                            let mut hasher = sha2::Sha256::new();
                            hasher.update(self.account_id.as_bytes());
                            hasher.update(b"\0");
                            if let Some(ref reply_to) = msg.in_reply_to {
                                hasher.update(reply_to.trim().trim_matches(['<', '>']).as_bytes());
                            } else {
                                hasher
                                    .update(vespetrel_core::normalize_subject(&subject).as_bytes());
                            }
                            format!("th_{:x}", hasher.finalize())
                        };
                        msg.thread_id = Some(thread_id.clone());

                        // Upsert thread record in storage. Counters are recomputed from the
                        // messages table rather than incremented here: the previous code
                        // hardcoded message_count = 1 and derived unread_count from the
                        // single message being synced, so a 12-message thread always
                        // reported "1 message", and re-syncing a read message zeroed the
                        // unread count even when 6 unread messages remained. Recomputing is
                        // also idempotent, so a re-sync does not double-count.
                        if let Some(conn) = &storage_conn {
                            let th = vespetrel_core::thread::Thread {
                                id: thread_id,
                                account_id: self.account_id.clone(),
                                subject: Some(subject.clone()),
                                last_message_at: msg.received_at,
                                message_count: 0, // recomputed below
                                unread_count: 0,
                                snippet: msg.body_snippet.clone(),
                            };
                            let _ = conn
                                .interact(move |c| {
                                    vespetrel_storage::repo::upsert_thread(c, &th)?;
                                    vespetrel_storage::repo::recount_thread(c, &th.id)
                                })
                                .await;
                        }

                        if let Some(ref classifier) = self.classifier {
                            let check_text = format!(
                                "{} {}",
                                msg.subject.as_deref().unwrap_or(""),
                                msg.body_text_preview.as_deref().unwrap_or("")
                            );
                            let score = classifier.classify(&check_text);
                            if score.is_spam {
                                tracing::warn!(id = %msg.id, prob = score.probability, "incoming message classified as spam/junk");
                            }
                        }

                        // Apply synced flags
                        msg.is_read = sync_msg.flags.contains(&vespetrel_core::Flag::Seen);
                        msg.is_flagged = sync_msg.flags.contains(&vespetrel_core::Flag::Flagged);
                        msg.is_draft = sync_msg.flags.contains(&vespetrel_core::Flag::Draft);

                        blobs_to_write.push((msg.id.clone(), raw_bytes));
                        summaries.push(msg.summary());
                        msgs_to_store.push(msg);
                    }

                    // Batch write blobs concurrently in a single background task
                    if let Some(store) = self
                        .blob_store
                        .clone()
                        .filter(|_| !blobs_to_write.is_empty())
                    {
                        let res = tokio::task::spawn_blocking(move || {
                            for (id, raw) in blobs_to_write {
                                if let Err(e) = store.write(&id, &raw) {
                                    error!(msg_id=%id, error=%e, "failed to write message to BlobStore");
                                }
                            }
                        })
                        .await;
                        if let Err(e) = res {
                            error!(error=%e, "blob store batch write task panicked");
                        }
                    }

                    // Batch persist synced messages to storage in a single atomic transaction
                    if let Some(conn) = storage_conn.as_ref().filter(|_| !msgs_to_store.is_empty())
                    {
                        let msgs_batch = msgs_to_store.clone();
                        let res = conn
                            .interact(move |c| -> anyhow::Result<()> {
                                let tx = c.transaction()?;
                                for m in &msgs_batch {
                                    vespetrel_storage::repo::insert_message(&tx, m)?;
                                }
                                tx.commit()?;
                                Ok(())
                            })
                            .await;
                        if let Err(e) = res {
                            error!(error=%e, "storage interact error batch inserting messages");
                        }
                    }

                    if !summaries.is_empty() {
                        self.emit(SyncEvent::MessagesInserted(summaries));
                    }

                    // Process updated message flags from delta.updated
                    if !delta.updated.is_empty() {
                        let updated_sync_msgs = delta.updated.clone();
                        let fid = folder_db_id.clone();
                        if let Some(conn) = &storage_conn {
                            let updated_events = conn.interact(move |c| -> anyhow::Result<Vec<SyncEvent>> {
                                let mut events = Vec::new();
                                let mut stmt = c.prepare(
                                    "SELECT id, is_read, is_flagged FROM messages WHERE folder_id = ?1 AND remote_uid = ?2"
                                )?;
                                for umsg in &updated_sync_msgs {
                                    let new_read = umsg.flags.contains(&vespetrel_core::Flag::Seen);
                                    let new_flagged = umsg.flags.contains(&vespetrel_core::Flag::Flagged);
                                    if let Ok(mut rows) = stmt.query(rusqlite::params![fid, umsg.remote_uid as i64])
                                        && let Ok(Some(row)) = rows.next()
                                    {
                                        let id: String = row.get(0)?;
                                        let cur_read: i64 = row.get(1)?;
                                        let cur_flagged: i64 = row.get(2)?;
                                        let is_read_changed = (cur_read != 0) != new_read;
                                        let is_flagged_changed = (cur_flagged != 0) != new_flagged;
                                        if is_read_changed || is_flagged_changed {
                                            let _ = vespetrel_storage::repo::update_message_flags(
                                                c,
                                                &id,
                                                if is_read_changed { Some(new_read) } else { None },
                                                if is_flagged_changed { Some(new_flagged) } else { None },
                                            );
                                            events.push(SyncEvent::MessageFlagsUpdated {
                                                id,
                                                is_read: new_read,
                                                is_flagged: new_flagged,
                                            });
                                        }
                                    }
                                }
                                Ok(events)
                            }).await;
                            if let Ok(Ok(events)) = updated_events {
                                for ev in events {
                                    self.emit(ev);
                                }
                            }
                        }
                    }

                    if !delta.deleted_uids.is_empty() {
                        let mut deleted_message_ids = Vec::new();
                        if let Some(conn) = &storage_conn {
                            let uids = delta.deleted_uids.clone();
                            let fid = folder_db_id.clone();
                            let queried = conn
                                .interact(move |c| -> anyhow::Result<Vec<String>> {
                                    let mut ids = Vec::new();
                                    for chunk in uids.chunks(500) {
                                        let placeholders =
                                            chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                                        let sql = format!(
                                            "SELECT id FROM messages WHERE folder_id = ? AND remote_uid IN ({placeholders})"
                                        );
                                        let mut stmt = c.prepare(&sql)?;
                                        let mut params: Vec<&dyn rusqlite::ToSql> =
                                            Vec::with_capacity(chunk.len() + 1);
                                        params.push(&fid);
                                        for u in chunk {
                                            params.push(u);
                                        }
                                        let rows = stmt.query_map(
                                            rusqlite::params_from_iter(params),
                                            |r| r.get::<_, String>(0),
                                        )?;
                                        for id_res in rows {
                                            ids.push(id_res?);
                                        }
                                    }
                                    for id in &ids {
                                        let _ = vespetrel_storage::repo::delete_message(c, id);
                                    }
                                    Ok(ids)
                                })
                                .await;
                            if let Ok(Ok(ids)) = queried {
                                deleted_message_ids = ids;
                            }
                        } else {
                            deleted_message_ids =
                                delta.deleted_uids.iter().map(|u| u.to_string()).collect();
                        }

                        if let Some(store) = self.blob_store.clone() {
                            let ids_to_delete = deleted_message_ids.clone();
                            let _ = tokio::task::spawn_blocking(move || {
                                for id in ids_to_delete {
                                    let _ = store.delete(&id);
                                }
                            })
                            .await;
                        }
                        self.emit(SyncEvent::MessagesDeleted(deleted_message_ids));
                    }

                    // Persist updated folder uid_validity and highest_mod_seq back to folders table
                    if (rf.uid_validity.is_some() || rf.highest_mod_seq.is_some())
                        && let Some(conn) = &storage_conn
                    {
                        let mut updated_folder = folder_record.clone();
                        if let Some(uv) = rf.uid_validity {
                            updated_folder.uid_validity = Some(uv);
                        }
                        if let Some(hms) = rf.highest_mod_seq {
                            updated_folder.highest_mod_seq = Some(hms);
                        }
                        let _ = conn
                            .interact(move |c| {
                                vespetrel_storage::repo::upsert_folder(c, &updated_folder)
                            })
                            .await;
                    }

                    // Persist updated delta tokens and folder modseqs back to accounts table
                    // only when changed AND the folder was fully materialised. Advancing
                    // the cursor past messages we failed to store means the server never
                    // re-delivers them: IMAP uses CHANGEDSINCE <modseq> and JMAP sinceState.
                    if !ingest_complete {
                        warn!(
                            account_id=%self.account_id,
                            folder=%rf.name,
                            "folder sync incomplete; leaving the sync cursor unchanged so missing messages are retried"
                        );
                    } else if delta.new_sync_state != vespetrel_core::account::SyncState::default()
                        && delta.new_sync_state != account_sync_state
                    {
                        account_sync_state = delta.new_sync_state.clone();
                        if let Some(conn) = &storage_conn {
                            let acct_id = self.account_id.clone();
                            let sync_state_to_save = account_sync_state.clone();
                            // Write only the sync cursor. A get_account -> upsert_account
                            // round-trip would rewrite every column, so a lossy read of a
                            // deserialization-failed auth_config would overwrite the user's
                            // stored credentials with a default and destroy them.
                            let res = conn
                                .interact(move |c| {
                                    if let Err(e) = vespetrel_storage::repo::update_account_sync_state(
                                        c,
                                        &acct_id,
                                        &sync_state_to_save,
                                    ) {
                                        error!(error=%e, "failed to update account sync state in DB");
                                    }
                                })
                                .await;
                            if let Err(e) = res {
                                error!(account_id=%self.account_id, error=%e, "storage interact error saving sync state");
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(folder=%rf.name, error=%e, "folder sync failed");
                    self.emit(SyncEvent::SyncError {
                        folder: rf.name.clone(),
                        error: e.to_string(),
                    });
                }
            }
        }
        self.emit(SyncEvent::SyncFinished {
            account_id: self.account_id.clone(),
        });
        Ok(())
    }

    fn emit(&self, ev: SyncEvent) {
        self.event_tx.send(ev);
    }
}

/// Result of persisting one folder's slice of a mailbox delta.
enum IngestOutcome {
    /// Committed. Carries the summaries to announce and the local ids removed.
    Ok(Vec<vespetrel_core::message::MessageSummary>, Vec<String>),
    /// Some message could not be materialised; the cursor must not advance.
    Incomplete,
    /// A hard failure worth reporting.
    Failed(anyhow::Error),
}

impl AccountWorker {
    /// One sync cycle for a mailbox-scoped provider.
    ///
    /// The provider returns one delta per affected folder, already partitioned. This mirrors the
    /// per-folder loop's contract — persist messages, apply deletions, then advance the cursor
    /// only once everything committed — but iterates a single mailbox delta rather than
    /// re-walking a shared cursor per folder.
    async fn sync_mailbox_cycle(
        &mut self,
        folders: &[RemoteFolder],
        folder_records: &ahash::AHashMap<String, vespetrel_core::Folder>,
        account_sync_state: vespetrel_core::account::SyncState,
        storage_conn: Option<PooledConn>,
    ) -> anyhow::Result<()> {
        // Hand the provider the set of ids already mirrored locally, so it can tell a genuinely
        // new message from one that merely changed labels. Without this every star toggle costs
        // a full RFC822 refetch, which exhausts Gmail's per-project quota on an active mailbox.
        if let Some(conn) = &storage_conn {
            let acct_id = self.account_id.clone();
            if let Ok(Ok(ids)) = conn
                .interact(move |c| vespetrel_storage::repo::list_all_remote_ids(c, &acct_id))
                .await
            {
                self.provider.set_stored_message_ids(ids);
            }
        }

        let mut mailbox_res = self.provider.sync_mailbox(account_sync_state.clone()).await;
        if let Err(ref e) = mailbox_res {
            let err_str = e.to_string().to_lowercase();
            if err_str.contains("401")
                || err_str.contains("unauthorized")
                || err_str.contains("authenticate")
            {
                warn!(account_id=%self.account_id, "authentication error in sync_mailbox, attempting OAuth refresh");
                if self.try_refresh_auth().await.is_ok() {
                    mailbox_res = self.provider.sync_mailbox(account_sync_state.clone()).await;
                }
            }
        }

        let deltas = match mailbox_res {
            Ok(d) => d,
            Err(e) => {
                warn!(account_id=%self.account_id, error=%e, "mailbox sync failed");
                self.emit(SyncEvent::SyncError {
                    folder: "mailbox".into(),
                    error: e.to_string(),
                });
                return Ok(());
            }
        };

        let mut newest_state = account_sync_state;
        let mut ingest_complete = true;
        let mut any_changed = false;
        let mut all_summaries: Vec<vespetrel_core::message::MessageSummary> = Vec::new();
        let mut all_deleted: Vec<String> = Vec::new();

        for folder_delta in deltas {
            let delta = folder_delta.delta;

            // The empty remote id marks a mailbox-wide change (Gmail deletions), which applies to
            // every folder and is resolved by message id rather than by folder.
            let rf = if folder_delta.folder_remote_id.is_empty() {
                None
            } else {
                folders
                    .iter()
                    .find(|f| f.remote_id == folder_delta.folder_remote_id)
            };

            let outcome = match rf {
                Some(rf) => {
                    let record = folder_records
                        .get(&rf.remote_id)
                        .cloned()
                        .unwrap_or_else(|| {
                            vespetrel_core::Folder::new(
                                &self.account_id,
                                &rf.remote_id,
                                &rf.name,
                                &rf.path,
                            )
                        });
                    self.ingest_delta(rf, &record, &delta, storage_conn.as_ref())
                        .await
                }
                None => {
                    self.ingest_mailbox_wide(&delta, storage_conn.as_ref())
                        .await
                }
            };

            match outcome {
                IngestOutcome::Ok(summaries, deleted) => {
                    all_summaries.extend(summaries);
                    all_deleted.extend(deleted);
                }
                IngestOutcome::Incomplete => ingest_complete = false,
                IngestOutcome::Failed(err) => {
                    error!(
                        account_id=%self.account_id,
                        folder=%rf.map(|f| f.name.as_str()).unwrap_or("mailbox"),
                        error=%err,
                        "mailbox delta ingest failed"
                    );
                    ingest_complete = false;
                }
            }

            if !delta.is_empty() {
                any_changed = true;
            }
            if delta.new_sync_state != vespetrel_core::account::SyncState::default() {
                newest_state = delta.new_sync_state;
            }
        }

        if !all_summaries.is_empty() {
            self.emit(SyncEvent::MessagesInserted(all_summaries));
        }
        if !all_deleted.is_empty() {
            if let Some(store) = self.blob_store.clone() {
                let ids = all_deleted.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    for id in ids {
                        let _ = store.delete(&id);
                    }
                })
                .await;
            }
            self.emit(SyncEvent::MessagesDeleted(all_deleted));
        }

        // The cursor advances only after every folder committed. This is the ordering that makes
        // a history-based provider safe: Gmail has no other queue, so a cursor advanced past
        // uncommitted work means those messages were never delivered and never would be.
        if !ingest_complete {
            warn!(
                account_id=%self.account_id,
                "mailbox sync incomplete; leaving the sync cursor unchanged so missing messages are retried"
            );
        } else if any_changed
            && newest_state != vespetrel_core::account::SyncState::default()
            && let Some(conn) = &storage_conn
        {
            let acct_id = self.account_id.clone();
            let state_to_save = newest_state.clone();
            let res = conn
                .interact(move |c| {
                    if let Err(e) = vespetrel_storage::repo::update_account_sync_state(
                        c,
                        &acct_id,
                        &state_to_save,
                    ) {
                        error!(error=%e, "failed to update account sync state in DB");
                    }
                })
                .await;
            if let Err(e) = res {
                error!(account_id=%self.account_id, error=%e, "storage interact error saving sync state");
            }
        }

        self.emit(SyncEvent::SyncFinished {
            account_id: self.account_id.clone(),
        });
        Ok(())
    }

    /// Parse raw RFC822 into a storable message, queueing its body for the blob store.
    ///
    /// Returns `Err` with the reason rather than a default message: a body the parser rejects or
    /// a message with no `From` address is not materialisable, and storing a placeholder would
    /// put a broken row in the mailbox while advancing the cursor past the real content.
    fn build_message_from_raw(
        &self,
        folder_db_id: &str,
        sync_msg: &SyncMessage,
        raw_bytes: &[u8],
        blobs_to_write: &mut Vec<(String, Vec<u8>)>,
    ) -> Result<vespetrel_core::Message, String> {
        let parsed = MessageParser::default()
            .parse(raw_bytes)
            .ok_or_else(|| "failed to parse RFC822 message".to_string())?;

        let from_addr = parsed
            .from()
            .and_then(|f| f.first())
            .and_then(|a| a.address.as_deref())
            .filter(|a| !a.is_empty())
            .ok_or_else(|| "message has no From address".to_string())?
            .to_string();

        let from_name = parsed
            .from()
            .and_then(|f| f.first())
            .and_then(|a| a.name.as_deref())
            .map(|s| s.to_string());
        let to_addrs = parsed
            .to()
            .map(|t| {
                t.iter()
                    .filter_map(|a| a.address.as_deref())
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let subject = parsed.subject().unwrap_or("No Subject").to_string();
        let mut msg = vespetrel_core::Message::new(
            &self.account_id,
            folder_db_id,
            sync_msg.remote_uid,
            &subject,
            from_addr,
            to_addrs,
        );
        msg.remote_id = sync_msg.remote_id.clone();
        msg.from_name = from_name;
        msg.message_id_header = parsed.message_id().map(|s| s.to_string());
        msg.in_reply_to = parsed.in_reply_to().as_text().map(|s| s.to_string());
        msg.references = parsed.references().as_text().map(|s| s.to_string());
        // Cap the preview. Storing the entire first text part meant a multi-megabyte body was
        // serialised into a SQLite column on every message and then fed to the spam classifier.
        msg.body_snippet = parsed
            .body_text(0)
            .map(|t| t.chars().take(MAX_BODY_PREVIEW_CHARS).collect::<String>());
        msg.body_text_preview = parsed
            .body_text(0)
            .map(|t| t.chars().take(MAX_BODY_PREVIEW_CHARS).collect::<String>());

        // `Message::new` sets sent_at = received_at = now(). Without reading the Date header
        // every synced message got the *sync* time, so `ORDER BY sent_at DESC` returned a
        // mailbox in essentially random order and the order was unstable across re-syncs.
        if let Some(dt) = parsed.date().and_then(mail_date_to_utc) {
            msg.sent_at = dt;
            msg.received_at = dt;
        }

        msg.has_attachments = parsed.attachment_count() > 0;

        // cc/bcc/reply-to were never populated, so Reply targeted the From address instead of
        // Reply-To.
        //
        // `mail_parser` returns an address *group* (`Option<&Address>`) whose items are `Addr`
        // with `name()`/`address()` accessors, while the storage column is a `Vec<Address>`.
        let to_core_addresses =
            |group: Option<&mail_parser::Address>| -> Vec<vespetrel_core::Address> {
                group
                    .map(|g| {
                        g.iter()
                            .filter_map(|a| {
                                a.address().map(|email| vespetrel_core::Address {
                                    name: a.name().map(|s| s.to_string()),
                                    email: email.to_string(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };
        msg.cc_addresses = to_core_addresses(parsed.cc());
        msg.bcc_addresses = to_core_addresses(parsed.bcc());
        msg.reply_to = parsed
            .reply_to()
            .map(|g| to_core_addresses(Some(g)))
            .filter(|v| !v.is_empty());

        // Provider-reported flags win over the headers: a message marked unread in Gmail but
        // carrying a `Status: RO` header must stay unread.
        msg.is_read = sync_msg.flags.contains(&vespetrel_core::Flag::Seen);
        msg.is_flagged = sync_msg.flags.contains(&vespetrel_core::Flag::Flagged);

        msg.size_bytes = raw_bytes.len() as i64;
        msg.blob_path = msg.id.clone();
        blobs_to_write.push((msg.id.clone(), raw_bytes.to_vec()));

        Ok(msg)
    }

    /// Persist one folder's slice of a mailbox delta.
    ///
    /// Returns the summaries to announce and the local ids removed. `Incomplete` means some
    /// message could not be materialised, which must suppress the cursor advance.
    async fn ingest_delta(
        &mut self,
        rf: &RemoteFolder,
        folder_record: &vespetrel_core::Folder,
        delta: &SyncDelta,
        storage_conn: Option<&PooledConn>,
    ) -> IngestOutcome {
        let mut summaries = Vec::new();
        let mut msgs_to_store = Vec::new();
        let mut blobs_to_write = Vec::new();
        let mut ingest_complete = true;

        for sync_msg in &delta.inserted {
            let fetch_id = sync_msg
                .remote_id
                .clone()
                .unwrap_or_else(|| sync_msg.remote_uid.to_string());
            let raw_bytes = match &sync_msg.raw_rfc822 {
                Some(b) if !b.is_empty() => b.clone(),
                _ => match self.provider.fetch_raw_message(&fetch_id).await {
                    Ok(b) if !b.is_empty() => b,
                    Ok(_) => {
                        ingest_complete = false;
                        continue;
                    }
                    Err(e) => {
                        error!(
                            account_id=%self.account_id,
                            folder=%rf.name,
                            error=%e,
                            "failed to fetch message body; aborting so the cursor is not advanced past it"
                        );
                        return IngestOutcome::Incomplete;
                    }
                },
            };

            match self.build_message_from_raw(
                &folder_record.id,
                sync_msg,
                &raw_bytes,
                &mut blobs_to_write,
            ) {
                Ok(message) => {
                    summaries.push(message.summary());
                    msgs_to_store.push(message);
                }
                Err(msg) => {
                    warn!(
                        account_id=%self.account_id,
                        folder=%rf.name,
                        reason=%msg,
                        "could not materialise message; not advancing the sync cursor past it"
                    );
                    ingest_complete = false;
                }
            }
        }

        // Blobs before rows: a message row whose body was never written would render empty.
        if let Some(store) = &self.blob_store
            && !blobs_to_write.is_empty()
        {
            let store = store.clone();
            let blobs = blobs_to_write.clone();
            let res = tokio::task::spawn_blocking(move || {
                for (message_id, bytes) in blobs {
                    if let Err(e) = store.write(&message_id, &bytes) {
                        error!(message_id=%message_id, error=%e, "failed to write message body");
                    }
                }
            })
            .await;
            if let Err(e) = res {
                error!(error=%e, "blob store batch write task panicked");
                ingest_complete = false;
            }
        }

        if let Some(conn) = storage_conn.filter(|_| !msgs_to_store.is_empty()) {
            let msgs_batch = msgs_to_store.clone();
            let res = conn
                .interact(move |c| -> anyhow::Result<()> {
                    let tx = c.transaction()?;
                    for m in &msgs_batch {
                        vespetrel_storage::repo::insert_message(&tx, m)?;
                    }
                    tx.commit()?;
                    Ok(())
                })
                .await;
            if let Err(e) = res {
                error!(error=%e, "storage interact error batch inserting messages");
                ingest_complete = false;
            }
        }

        let events = self.apply_flag_updates(&delta.updated, storage_conn).await;
        for ev in events {
            self.emit(ev);
        }

        let deleted = self.apply_deletions(delta, storage_conn).await;

        if ingest_complete {
            IngestOutcome::Ok(summaries, deleted)
        } else {
            IngestOutcome::Incomplete
        }
    }

    /// Apply a mailbox-wide deletion slice, resolved by provider-native message id.
    async fn ingest_mailbox_wide(
        &mut self,
        delta: &SyncDelta,
        storage_conn: Option<&PooledConn>,
    ) -> IngestOutcome {
        if delta.inserted.is_empty() && delta.updated.is_empty() {
            // Deletions only: record the state so the caller can advance the cursor.
            let deleted = self.apply_deletions(delta, storage_conn).await;
            return IngestOutcome::Ok(Vec::new(), deleted);
        }

        let Some(conn) = storage_conn else {
            return IngestOutcome::Failed(anyhow::anyhow!(
                "mailbox delta carries message data but no storage connection is available"
            ));
        };

        let acct_id = self.account_id.clone();
        let remote_ids = delta.deleted_remote_ids.clone();
        let removed = conn
            .interact(move |c| -> anyhow::Result<Vec<String>> {
                let mut removed = Vec::new();
                for remote_id in &remote_ids {
                    if let Some(id) =
                        vespetrel_storage::repo::find_message_by_remote_id(c, &acct_id, remote_id)?
                    {
                        let _ = vespetrel_storage::repo::delete_message(c, &id);
                        removed.push(id);
                    }
                }
                Ok(removed)
            })
            .await;

        // `interact` returns `Result<Result<T, E>, InteractError>`; both layers are failures here.
        match removed {
            Ok(Ok(ids)) => IngestOutcome::Ok(Vec::new(), ids),
            Ok(Err(e)) => {
                error!(error=%e, "failed to apply mailbox-wide deletions");
                IngestOutcome::Incomplete
            }
            Err(e) => {
                error!(error=%e, "storage interaction failed during deletion pass");
                IngestOutcome::Incomplete
            }
        }
    }

    /// Reconcile read/starred state for messages a provider reported as changed.
    ///
    /// Resolves by `remote_id`: Gmail ids are opaque strings carrying no numeric UID, so
    /// matching on `remote_uid` would silently update nothing.
    async fn apply_flag_updates(
        &self,
        updated: &[SyncMessage],
        storage_conn: Option<&PooledConn>,
    ) -> Vec<SyncEvent> {
        if updated.is_empty() {
            return Vec::new();
        }
        let Some(conn) = storage_conn else {
            warn!("flag updates received but no storage connection is available");
            return Vec::new();
        };

        let acct_id = self.account_id.clone();
        let updated = updated.to_vec();
        match conn
            .interact(move |c| -> anyhow::Result<Vec<SyncEvent>> {
                let mut events = Vec::new();
                for umsg in &updated {
                    let new_read = umsg.flags.contains(&vespetrel_core::Flag::Seen);
                    let new_flagged = umsg.flags.contains(&vespetrel_core::Flag::Flagged);

                    let Some(id) = umsg.remote_id.as_ref().and_then(|rid| {
                        vespetrel_storage::repo::find_message_by_remote_id(c, &acct_id, rid)
                            .ok()
                            .flatten()
                    }) else {
                        // Never stored: its flags arrive with its first fetch.
                        continue;
                    };

                    let (cur_read, cur_flagged): (i64, i64) = c.query_row(
                        "SELECT is_read, is_flagged FROM messages WHERE id = ?1",
                        rusqlite::params![id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )?;
                    let is_read_changed = (cur_read != 0) != new_read;
                    let is_flagged_changed = (cur_flagged != 0) != new_flagged;
                    if !is_read_changed && !is_flagged_changed {
                        continue;
                    }
                    let _ = vespetrel_storage::repo::update_message_flags(
                        c,
                        &id,
                        if is_read_changed {
                            Some(new_read)
                        } else {
                            None
                        },
                        if is_flagged_changed {
                            Some(new_flagged)
                        } else {
                            None
                        },
                    );
                    events.push(SyncEvent::MessageFlagsUpdated {
                        id,
                        is_read: new_read,
                        is_flagged: new_flagged,
                    });
                }
                Ok(events)
            })
            .await
        {
            // Both layers are failures: the inner is the closure's error, the outer is the pool.
            Ok(Ok(events)) => events,
            Ok(Err(e)) => {
                warn!(error=%e, "flag update pass failed");
                Vec::new()
            }
            Err(e) => {
                warn!(error=%e, "storage interaction failed during flag update pass");
                Vec::new()
            }
        }
    }

    /// Delete the messages a provider reported gone, by UID or by remote id.
    async fn apply_deletions(
        &self,
        delta: &SyncDelta,
        storage_conn: Option<&PooledConn>,
    ) -> Vec<String> {
        if delta.deleted_uids.is_empty() && delta.deleted_remote_ids.is_empty() {
            return Vec::new();
        }
        let Some(conn) = storage_conn else {
            warn!("deletions received but no storage connection is available");
            return Vec::new();
        };

        let acct_id = self.account_id.clone();
        let uids = delta.deleted_uids.clone();
        let remote_ids = delta.deleted_remote_ids.clone();

        match conn
            .interact(move |c| -> anyhow::Result<Vec<String>> {
                let mut removed: Vec<String> = Vec::new();

                // Remote ids are mailbox-wide: a deleted message may sit in several folders.
                for remote_id in &remote_ids {
                    if let Some(id) =
                        vespetrel_storage::repo::find_message_by_remote_id(c, &acct_id, remote_id)?
                    {
                        let _ = vespetrel_storage::repo::delete_message(c, &id);
                        removed.push(id);
                    }
                }

                // Numeric UIDs are only unambiguous when the provider gave no remote ids
                // alongside them; a provider that supplies both has already scoped the change.
                if !uids.is_empty() && remote_ids.is_empty() {
                    let mut stmt = c.prepare(
                        "SELECT id FROM messages WHERE account_id = ?1 AND remote_uid = ?2",
                    )?;
                    for uid in &uids {
                        let mut rows = stmt.query(rusqlite::params![acct_id, *uid as i64])?;
                        while let Ok(Some(row)) = rows.next() {
                            let id: String = row.get(0)?;
                            let _ = vespetrel_storage::repo::delete_message(c, &id);
                            removed.push(id);
                        }
                    }
                }

                Ok(removed)
            })
            .await
        {
            // Both layers are failures: the inner is the closure's error, the outer is the pool.
            Ok(Ok(ids)) => ids,
            Ok(Err(e)) => {
                warn!(error=%e, "deletion pass failed");
                Vec::new()
            }
            Err(e) => {
                warn!(error=%e, "storage interaction failed during deletion pass");
                Vec::new()
            }
        }
    }
}
