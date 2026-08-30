use tokio::sync::{broadcast, mpsc};

use vespetrel_core::provider::SyncEvent;

/// Tokio -> GPUI bridge channel. Engine produces `SyncEvent`, UI consumes via `mpsc`.
pub type EventSender = mpsc::UnboundedSender<SyncEvent>;
pub type EventReceiver = mpsc::UnboundedReceiver<SyncEvent>;

/// Internal broadcast bus for engine-internal fan-out (multiple workers -> coordinator)
pub struct EventBus {
    tx: broadcast::Sender<SyncEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn sender(&self) -> broadcast::Sender<SyncEvent> {
        self.tx.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SyncEvent> {
        self.tx.subscribe()
    }

    pub fn send(&self, event: SyncEvent) {
        let _ = self.tx.send(event);
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}
