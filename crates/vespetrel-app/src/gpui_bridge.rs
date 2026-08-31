//! Tokio -> GPUI bridge §6.2
use tokio::sync::mpsc;
use vespetrel_core::provider::SyncEvent;

/// Event bridge receiver converting Tokio SyncEvents to UI view updates
pub struct SyncBridgeReceiver {
    rx: mpsc::UnboundedReceiver<SyncEvent>,
}

impl SyncBridgeReceiver {
    pub fn new(rx: mpsc::UnboundedReceiver<SyncEvent>) -> Self {
        Self { rx }
    }

    pub async fn next_event(&mut self) -> Option<SyncEvent> {
        self.rx.recv().await
    }
}
