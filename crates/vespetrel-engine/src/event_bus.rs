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
        // `broadcast::Sender::send` errors when there are no receivers, which is the
        // normal state between `new()` and the first `subscribe()`. The old `let _ =`
        // discarded those *and* genuine send failures identically, so a dropped event was
        // indistinguishable from a delivered one.
        if self.tx.receiver_count() == 0 {
            tracing::debug!("no event-bus subscribers; event discarded");
            return;
        }
        if let Err(e) = self.tx.send(event) {
            tracing::warn!(error=%e, "event bus send failed");
        }
    }

    /// Receive the next event, transparently handling lag.
    ///
    /// A subscriber that falls more than `capacity` behind gets `RecvError::Lagged(n)` and
    /// silently loses `n` events, so a consumer could not tell "nothing happened" from "I
    /// missed 40 folder updates". This surfaces the lag and keeps waiting.
    pub async fn recv_lossy(rx: &mut broadcast::Receiver<SyncEvent>) -> Option<SyncEvent> {
        loop {
            match rx.recv().await {
                Ok(ev) => return Some(ev),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "event bus lagged; resynchronising");
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
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
