use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::info;

use vespetrel_core::provider::{MailProvider, SyncEvent};

use crate::worker::{AccountWorker, WorkerCommand};

pub struct SyncCoordinator {
    /// account_id -> command sender
    workers: HashMap<String, mpsc::UnboundedSender<WorkerCommand>>,
    /// UI event sender (Tokio mpsc -> GPUI)
    event_tx: mpsc::UnboundedSender<SyncEvent>,
}

impl SyncCoordinator {
    /// Create coordinator and return the UI-side receiver.
    /// The UI should own the receiver and forward SyncEvent into GPUI via cx.spawn.
    pub fn create() -> (Self, mpsc::UnboundedReceiver<SyncEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let coord = Self { workers: HashMap::new(), event_tx: tx };
        (coord, rx)
    }

    pub fn event_sender(&self) -> mpsc::UnboundedSender<SyncEvent> {
        self.event_tx.clone()
    }

    pub fn spawn_worker(&mut self, account_id: impl Into<String>, provider: Arc<dyn MailProvider>) {
        let account_id = account_id.into();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let worker = AccountWorker::new(account_id.clone(), provider, self.event_tx.clone(), cmd_rx);
        tokio::spawn(worker.run());
        self.workers.insert(account_id.clone(), cmd_tx);
        info!(account_id=%account_id, "spawned worker");
    }

    pub fn trigger_sync(&self, account_id: &str) {
        if let Some(tx) = self.workers.get(account_id) {
            let _ = tx.send(WorkerCommand::SyncNow);
        }
    }

    pub fn stop_worker(&mut self, account_id: &str) {
        if let Some(tx) = self.workers.remove(account_id) {
            let _ = tx.send(WorkerCommand::Stop);
        }
    }

    pub fn stop_all(&mut self) {
        for (_, tx) in self.workers.drain() {
            let _ = tx.send(WorkerCommand::Stop);
        }
    }
}

impl Default for SyncCoordinator {
    fn default() -> Self {
        let (tx, _rx) = mpsc::unbounded_channel();
        Self { workers: HashMap::new(), event_tx: tx }
    }
}
