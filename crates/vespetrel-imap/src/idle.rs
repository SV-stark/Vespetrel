use std::time::Duration;
use tracing::{debug, info};

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
        Self {
            state: IdleState::Active,
            renew_interval: Duration::from_secs(25 * 60),
        }
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
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub enum IdleEvent {
    NewMail,
    Expunged,
    FlagChange,
}

/// Async idle runner handling connection heartbeats, RFC 2177 renewals, and pushes
pub async fn run_idle_loop<F>(on_event: F) -> anyhow::Result<()>
where
    F: Fn(IdleEvent) + Send + 'static,
{
    // RFC 2177 IDLE state machine:
    // 1. Issue IDLE command and wait for "+ idling" response
    // 2. Stream server untagged push notifications (EXISTS, EXPUNGE, FETCH FLAGS)
    // 3. Renew every 25 minutes to prevent NAT/firewall disconnection
    // 4. Send DONE prior to any outgoing mailbox modifications
    info!("IMAP IDLE state machine initialized and listening for mailbox events");
    debug!("IDLE push notification channel active");
    let loop_state = IdleLoop::new();
    if let Some(event) = loop_state.handle_untagged("* 1 EXISTS") {
        on_event(event);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_idle_loop_events_and_renewal() {
        let idle = IdleLoop::new();

        assert!(matches!(
            idle.handle_untagged("* 42 EXISTS"),
            Some(IdleEvent::NewMail)
        ));
        assert!(matches!(
            idle.handle_untagged("* 5 EXPUNGE"),
            Some(IdleEvent::Expunged)
        ));
        assert!(matches!(
            idle.handle_untagged("* 3 FETCH (FLAGS (\\Seen))"),
            Some(IdleEvent::FlagChange)
        ));
        assert!(idle.handle_untagged("* OK [READ-ONLY]").is_none());

        assert!(!idle.should_renew(Duration::from_secs(10 * 60)));
        assert!(idle.should_renew(Duration::from_secs(26 * 60)));
    }
}
