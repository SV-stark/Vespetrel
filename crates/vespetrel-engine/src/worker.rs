use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use vespetrel_core::provider::{MailProvider, SyncEvent};

#[derive(Debug)]
pub enum WorkerCommand {
    SyncNow,
    Stop,
    UpdateFlags { folder_remote_id: String, uids: Vec<u32>, add: Vec<vespetrel_core::message::Flag>, remove: Vec<vespetrel_core::message::Flag> },
}

/// One Actor per account - §3.2 Sync Engine
pub struct AccountWorker {
    pub account_id: String,
    pub provider: Arc<dyn MailProvider>,
    pub event_tx: mpsc::UnboundedSender<SyncEvent>,
    pub cmd_rx: mpsc::UnboundedReceiver<WorkerCommand>,
    pub poll_interval: Duration,
    pub storage_pool: Option<deadpool_sqlite::Pool>,
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
            event_tx,
            cmd_rx,
            poll_interval: Duration::from_secs(60),
            storage_pool: None,
        }
    }

    pub fn with_storage_pool(mut self, pool: deadpool_sqlite::Pool) -> Self {
        self.storage_pool = Some(pool);
        self
    }

    pub async fn run(mut self) {
        info!(account_id = %self.account_id, "starting account worker");
        let mut interval = tokio::time::interval(self.poll_interval);
        // For IMAP IDLE, provider handles long-poll internally; this interval is fallback/delta poll for JMAP/Graph

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = self.sync_once().await {
                        warn!(account_id=%self.account_id, error=%e, "periodic sync failed");
                        self.emit(SyncEvent::SyncError{ folder: "all".into(), error: e.to_string() });
                    }
                }
                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(WorkerCommand::SyncNow) => {
                            if let Err(e) = self.sync_once().await {
                                error!(error=%e, "on-demand sync failed");
                                self.emit(SyncEvent::SyncError{ folder: "all".into(), error:e.to_string() });
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

        // Persist folders to storage if pool is provided
        if let Some(pool) = &self.storage_pool {
            if let Ok(conn) = pool.get().await {
                for rf in &folders {
                    let folder_record = vespetrel_core::Folder::new(&self.account_id, &rf.remote_id, &rf.name, &rf.path);
                    let _ = conn.interact(move |c| {
                        vespetrel_storage::repo::upsert_folder(c, &folder_record)
                    }).await;
                }
            }
        }

        for rf in &folders {
            let folder_meta = vespetrel_core::Folder::new(&self.account_id, &rf.remote_id, &rf.name, &rf.path);
            let state = vespetrel_core::account::SyncState::default();
            match self.provider.sync_messages(&folder_meta, state).await {
                Ok(delta) => {
                    let mut summaries = Vec::new();

                    // Persist synced messages to storage
                    if let Some(pool) = &self.storage_pool {
                        if let Ok(conn) = pool.get().await {
                            for sync_msg in &delta.inserted {
                                let msg = vespetrel_core::Message::new(
                                    &self.account_id,
                                    &rf.remote_id,
                                    sync_msg.remote_uid,
                                    format!("Message {}", sync_msg.remote_uid),
                                    "sender@example.com",
                                    vec![self.account_id.clone()],
                                );
                                summaries.push(msg.summary());
                                let _ = conn.interact(move |c| {
                                    vespetrel_storage::repo::insert_message(c, &msg)
                                }).await;
                            }
                        }
                    } else {
                        // In memory summaries for mock/test runs
                        for sync_msg in &delta.inserted {
                            let msg = vespetrel_core::Message::new(
                                &self.account_id,
                                &rf.remote_id,
                                sync_msg.remote_uid,
                                format!("Message {}", sync_msg.remote_uid),
                                "sender@example.com",
                                vec![self.account_id.clone()],
                            );
                            summaries.push(msg.summary());
                        }
                    }

                    if !summaries.is_empty() {
                        self.emit(SyncEvent::MessagesInserted(summaries));
                    }

                    if !delta.deleted_uids.is_empty() {
                        let deleted_ids = delta.deleted_uids.iter().map(|u| u.to_string()).collect();
                        self.emit(SyncEvent::MessagesDeleted(deleted_ids));
                    }
                }
                Err(e) => {
                    warn!(folder=%rf.name, error=%e, "folder sync failed");
                    self.emit(SyncEvent::SyncError{ folder: rf.name.clone(), error: e.to_string() });
                }
            }
        }
        self.emit(SyncEvent::SyncFinished{ account_id: self.account_id.clone() });
        Ok(())
    }

    fn emit(&self, ev: SyncEvent) {
        let _ = self.event_tx.send(ev);
    }
}
