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
        self.emit(SyncEvent::FolderListUpdated(folders.clone()));

        // Acquire connection from storage pool if available with 5s timeout
        let storage_conn = match &self.storage_pool {
            Some(pool) => match tokio::time::timeout(Duration::from_secs(5), pool.get()).await {
                Ok(Ok(conn)) => Some(conn),
                Ok(Err(e)) => {
                    warn!(account_id=%self.account_id, error=%e, "failed to get connection from storage pool");
                    None
                }
                Err(_) => {
                    warn!(account_id=%self.account_id, "timed out acquiring connection from storage pool");
                    None
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

            // Persist folder metadata to storage
            if let Some(conn) = &storage_conn {
                let rec = folder_record.clone();
                let res = conn
                    .interact(move |c| vespetrel_storage::repo::upsert_folder(c, &rec))
                    .await;
                match res {
                    Ok(Ok(_)) => {}
                    Ok(Err(db_err)) => {
                        warn!(folder=%rf.name, error=%db_err, "failed to upsert folder in storage");
                    }
                    Err(interact_err) => {
                        warn!(folder=%rf.name, error=%interact_err, "storage interact error");
                    }
                }
            }
        }

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
                                let to_addrs: Vec<String> = parsed
                                    .to()
                                    .map(|addrs| {
                                        addrs
                                            .iter()
                                            .filter_map(|a| {
                                                a.address.as_deref().map(|s| s.to_string())
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_else(|| vec![self.account_id.clone()]);

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
                                let shard = if m.id.len() >= 2 { &m.id[..2] } else { "00" };
                                m.blob_path = format!("{shard}/{}.lz4", m.id);
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

                        // Persist raw RFC822 bytes into BlobStore non-blockingly via spawn_blocking
                        if let Some(store) = self.blob_store.clone() {
                            if let Some(raw) = sync_msg.raw_rfc822.clone() {
                                if !raw.is_empty() {
                                    let msg_id = msg.id.clone();
                                    let res = tokio::task::spawn_blocking(move || {
                                        store.write(&msg_id, &raw)
                                    })
                                    .await;
                                    if let Ok(Err(e)) = res {
                                        error!(msg_id=%msg.id, error=%e, "failed to write raw message to BlobStore");
                                    }
                                }
                            }
                        }

                        summaries.push(msg.summary());

                        // Persist synced message to storage
                        if let Some(conn) = &storage_conn {
                            let msg_to_store = msg.clone();
                            let res = conn
                                .interact(move |c| {
                                    vespetrel_storage::repo::insert_message(c, &msg_to_store)
                                })
                                .await;
                            match res {
                                Ok(Ok(_)) => {}
                                Ok(Err(db_err)) => {
                                    error!(msg_id=%msg.id, error=%db_err, "failed to store message in DB");
                                }
                                Err(interact_err) => {
                                    error!(msg_id=%msg.id, error=%interact_err, "storage interact error");
                                }
                            }
                        }
                    }

                    if !summaries.is_empty() {
                        self.emit(SyncEvent::MessagesInserted(summaries));
                    }

                    if !delta.deleted_uids.is_empty() {
                        let deleted_ids =
                            delta.deleted_uids.iter().map(|u| u.to_string()).collect();
                        self.emit(SyncEvent::MessagesDeleted(deleted_ids));
                    }

                    // Persist updated delta tokens and folder modseqs back to accounts table
                    if delta.new_sync_state != vespetrel_core::account::SyncState::default() {
                        account_sync_state = delta.new_sync_state.clone();
                    }
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
