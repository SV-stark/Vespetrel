//! Vespetrel Engine - Tokio sync coordinator + account worker actors

pub mod coordinator;
pub mod event_bus;
pub mod worker;

pub use coordinator::SyncCoordinator;
pub use event_bus::{EventBus, EventReceiver, EventSender};
pub use worker::{AccountWorker, WorkerCommand};

use vespetrel_core::provider::SyncEvent;
