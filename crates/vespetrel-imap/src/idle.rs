use std::time::Duration;
use tracing::{debug, info, warn};

/// IMAP IDLE loop - §4.2
/// Real implementation runs on Tokio task with raw TcpStream
/// This module provides the state machine logic.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleState {
    Idle,
    Active,
    DoneSent,
    Reconnecting,
}

pub struct IdleLoop {
    pub state: IdleState,
    /// RFC 2177 - renew before 29min. We use 25min.
    pub renew_interval: Duration,
}

impl IdleLoop {
    pub fn new() -> Self {
        Self { state: IdleState::Active, renew_interval: Duration::from_secs(25 * 60) }
    }

    /// Logic for handling server EXISTS / FETCH / EXPUNGE untagged responses
    pub fn handle_untagged(&self, line: &str) -> Option<IdleEvent> {
        let upper = line.to_uppercase();
        if upper.contains(" EXISTS") {
            Some(IdleEvent::NewMail)
        } else if upper.contains(" EXPUNGE") {
            Some(IdleEvent::Expunged)
        } else if upper.contains(" FETCH") && upper.contains("FLAGS") {
            Some(IdleEvent::FlagChange)
        } else {
            None
        }
    }

    pub fn should_renew(&self, elapsed: Duration) -> bool {
        elapsed >= self.renew_interval
    }
}

impl Default for IdleLoop {
    fn default() -> Self { Self::new() }
}

#[derive(Debug, Clone)]
pub enum IdleEvent {
    NewMail,
    Expunged,
    FlagChange,
}

/// Async idle runner - pseudo implementation showing DONE handling
pub async fn run_idle_loop<F>(_on_event: F) -> anyhow::Result<()>
where
    F: Fn(IdleEvent) + Send + 'static,
{
    // In production:
    // 1. Send "IDLE\r\n", wait for "+ idling"
    // 2. Read lines until timeout or server push
    // 3. On outgoing work, send "DONE\r\n", wait for tagged OK, do work, re-enter IDLE
    // 4. On 25min timeout, send DONE + re-IDLE to keep connection alive
    info!("idle loop started (stub - would hold IMAP connection)");
    debug!("would send IDLE and await server pushes");
    // Stub does not block forever in tests
    Ok(())
}
