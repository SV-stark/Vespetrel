use mail_parser::MessageParser;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use vespetrel_core::provider::{MailProvider, SyncEvent};

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
    pub fn send(&self, ev: SyncEvent) {
        match self {
            Self::Mpsc(tx) => {
                let _ = tx.send(ev);
            }
            Self::Flume(tx) => {
                let _ = tx.send(ev);
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
                        Some(WorkerCommand::UpdateFlags{ folder_remote_id: _, uids, add, remove }) => {
                            if let Err(e) = self.provider.update_flags(&uids, &add, &remove).await {
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

    async fn sync_once(&self) -> anyhow::Result<()> {
        let folders = self.provider.sync_folder_list().await?;

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

        for rf in &folders {
            let folder_record =
                vespetrel_core::Folder::new(&self.account_id, &rf.remote_id, &rf.name, &rf.path);
            folder_records.insert(rf.remote_id.clone(), folder_record.clone());
        }

        // Persist folder metadata to storage in a single atomic transaction
        if let Some(conn) = &storage_conn {
            let recs: Vec<vespetrel_core::Folder> = folder_records.values().cloned().collect();
            let res = conn
                .interact(move |c| -> anyhow::Result<()> {
                    let tx = c.transaction()?;
                    for rec in &recs {
                        vespetrel_storage::repo::upsert_folder(&tx, rec)?;
                    }
                    tx.commit()?;
                    Ok(())
                })
                .await;
            if let Err(e) = res {
                warn!(error=%e, "storage interact error batch upserting folders");
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

        for rf in &folders {
            let folder_record = folder_records
                .get(&rf.remote_id)
                .cloned()
                .unwrap_or_else(|| {
                    vespetrel_core::Folder::new(&self.account_id, &rf.remote_id, &rf.name, &rf.path)
                });
            let folder_db_id = folder_record.id.clone();
            let state = account_sync_state.clone();

            match self.provider.sync_messages(&folder_record, state).await {
                Ok(delta) => {
                    let mut summaries = Vec::new();
                    let mut msgs_to_store = Vec::new();
                    let mut blobs_to_write = Vec::new();

                    for sync_msg in &delta.inserted {
                        let raw_bytes_opt = if let Some(ref raw_bytes) = sync_msg.raw_rfc822 {
                            Some(raw_bytes.clone())
                        } else {
                            self.provider
                                .fetch_raw_message(&sync_msg.remote_uid.to_string())
                                .await
                                .ok()
                        };

                        let mut msg = if let Some(ref raw_bytes) = raw_bytes_opt {
                            if let Some(parsed) = MessageParser::default().parse(raw_bytes) {
                                let subject = parsed.subject().unwrap_or("No Subject").to_string();
                                let from_addr = parsed
                                    .from()
                                    .and_then(|f| f.first())
                                    .and_then(|a| a.address.as_deref())
                                    .unwrap_or("unknown@sender.com")
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

                                let mut m = vespetrel_core::Message::new(
                                    &self.account_id,
                                    &folder_db_id,
                                    sync_msg.remote_uid,
                                    subject,
                                    from_addr,
                                    to_addrs,
                                );
                                m.from_name = from_name;
                                m.message_id_header = parsed.message_id().map(|s| s.to_string());
                                m.in_reply_to =
                                    parsed.in_reply_to().as_text().map(|s| s.to_string());
                                m.body_snippet = parsed
                                    .body_text(0)
                                    .map(|t| t.chars().take(200).collect::<String>());
                                m.body_text_preview = parsed.body_text(0).map(|t| t.to_string());
                                m.size_bytes = raw_bytes.len() as i64;
                                let shard = &m.id[..2.min(m.id.len())];
                                m.blob_path = format!("{shard}/{}.lz4", m.id);

                                if let Some(ref classifier) = self.classifier {
                                    let check_text = format!(
                                        "{} {}",
                                        m.subject.as_deref().unwrap_or(""),
                                        m.body_text_preview.as_deref().unwrap_or("")
                                    );
                                    let score = classifier.classify(&check_text);
                                    if score.is_spam {
                                        tracing::warn!(id = %m.id, prob = score.probability, "incoming message classified as spam/junk");
                                    }
                                }
                                m
                            } else {
                                vespetrel_core::Message::new(
                                    &self.account_id,
                                    &folder_db_id,
                                    sync_msg.remote_uid,
                                    format!("Message {}", sync_msg.remote_uid),
                                    "unknown@sender.com",
                                    vec![self.account_id.clone()],
                                )
                            }
                        } else {
                            vespetrel_core::Message::new(
                                &self.account_id,
                                &folder_db_id,
                                sync_msg.remote_uid,
                                format!("Message {}", sync_msg.remote_uid),
                                "unknown@sender.com",
                                vec![self.account_id.clone()],
                            )
                        };

                        // Apply synced flags
                        msg.is_read = sync_msg.flags.contains(&vespetrel_core::Flag::Seen);
                        msg.is_flagged = sync_msg.flags.contains(&vespetrel_core::Flag::Flagged);
                        msg.is_draft = sync_msg.flags.contains(&vespetrel_core::Flag::Draft);

                        if let Some(raw) = raw_bytes_opt.as_ref().filter(|r| !r.is_empty()) {
                            blobs_to_write.push((msg.id.clone(), raw.clone()));
                        }

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

                    // Persist updated delta tokens and folder modseqs back to accounts table only when changed
                    if delta.new_sync_state != vespetrel_core::account::SyncState::default()
                        && delta.new_sync_state != account_sync_state
                    {
                        account_sync_state = delta.new_sync_state.clone();
                        if let Some(conn) = &storage_conn {
                            let acct_id = self.account_id.clone();
                            let sync_state_to_save = account_sync_state.clone();
                            let res = conn
                                .interact(move |c| {
                                    if let Ok(Some(mut acct)) =
                                        vespetrel_storage::repo::get_account(c, &acct_id)
                                    {
                                        acct.sync_state = sync_state_to_save;
                                        if let Err(e) = vespetrel_storage::repo::upsert_account(c, &acct) {
                                            error!(account_id=%acct.id, error=%e, "failed to update account sync state in DB");
                                        }
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
