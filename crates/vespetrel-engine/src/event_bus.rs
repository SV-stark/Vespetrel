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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_bus_broadcast() {
        let bus = EventBus::new(32);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        bus.send(SyncEvent::SyncFinished {
            account_id: "acc_42".into(),
        });

        match rx1.recv().await.unwrap() {
            SyncEvent::SyncFinished { account_id } => assert_eq!(account_id, "acc_42"),
            _ => panic!("Unexpected event"),
        }

        match rx2.recv().await.unwrap() {
            SyncEvent::SyncFinished { account_id } => assert_eq!(account_id, "acc_42"),
            _ => panic!("Unexpected event"),
        }
    }
}
