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
        }
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
        // Simplified: sync folder list then each folder's deltas
        // Concrete providers inject SyncState from storage
        let folders = self.provider.sync_folder_list().await?;
        self.emit(SyncEvent::FolderListUpdated(folders.clone()));

        for rf in &folders {
            // Load sync state from storage in real implementation; here use default
            let folder_meta = vespetrel_core::Folder::new(&self.account_id, &rf.remote_id, &rf.name, &rf.path);
            let state = vespetrel_core::account::SyncState::default();
            match self.provider.sync_messages(&folder_meta, state).await {
                Ok(delta) => {
                    // In real engine: persist via vespetrel-storage, emit MessagesInserted
                    let count = delta.inserted.len();
                    if count > 0 {
                        info!(folder=%rf.name, count, "synced new messages");
                    }
                    for d in delta.deleted_uids {
                        let _ = d;
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
