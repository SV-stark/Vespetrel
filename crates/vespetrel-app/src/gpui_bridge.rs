//! Tokio -> UI / GPUI event bridge §6.2
use tokio::sync::mpsc;
use vespetrel_core::provider::SyncEvent;

enum BridgeSource {
    Mpsc(mpsc::UnboundedReceiver<SyncEvent>),
    Flume(flume::Receiver<SyncEvent>),
}

/// Event bridge receiver converting Tokio/Worker SyncEvents to UI view updates
pub struct SyncBridgeReceiver {
    source: BridgeSource,
}

impl SyncBridgeReceiver {
    pub fn new(rx: mpsc::UnboundedReceiver<SyncEvent>) -> Self {
        Self {
            source: BridgeSource::Mpsc(rx),
        }
    }

    pub fn new_bounded(rx: flume::Receiver<SyncEvent>) -> Self {
        Self {
            source: BridgeSource::Flume(rx),
        }
    }

    pub async fn next_event(&mut self) -> Option<SyncEvent> {
        match &mut self.source {
            BridgeSource::Mpsc(rx) => rx.recv().await,
            BridgeSource::Flume(rx) => rx.recv_async().await.ok(),
        }
    }
}
