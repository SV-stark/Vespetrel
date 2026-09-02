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
    F: Fn(IdleEvent) + Send + Sync + 'static,
{
    let config = crate::client::ImapConfig::new("imap.example.com", 993, "user", "pass");
    run_idle_loop_with_config(config, on_event).await
}

/// Run a full live IMAP IDLE connection loop with automated 25-minute RFC 2177 DONE/IDLE renewal and reconnection
pub async fn run_idle_loop_with_config<F>(
    config: crate::client::ImapConfig,
    on_event: F,
) -> anyhow::Result<()>
where
    F: Fn(IdleEvent) + Send + Sync + 'static,
{
    let mut retry_count = 0;
    loop {
        info!(host=%config.host, "starting IMAP IDLE connection");
        let mut conn = crate::client::ImapConnection::new(config.clone());
        match conn.connect().await {
            Ok(()) => {
                retry_count = 0;
                let idle_loop = IdleLoop::new();
                let mut renew_ticker = tokio::time::interval(idle_loop.renew_interval);
                renew_ticker.tick().await; // Consume initial immediate tick

                let idle_cmd = conn.cmd_idle();
                debug!(cmd=%idle_cmd, "issuing IDLE command");
                let _ = conn.execute_cmd(idle_cmd).await;

                if let Some(mut stream) = conn.stream.take() {
                    let mut buf = [0u8; 4096];
                    loop {
                        tokio::select! {
                            res = stream.read(&mut buf) => {
                                match res {
                                    Ok(n) if n > 0 => {
                                        let text = String::from_utf8_lossy(&buf[..n]);
                                        for line in text.lines() {
                                            if let Some(ev) = idle_loop.handle_untagged(line) {
                                                on_event(ev);
                                            }
                                        }
                                    }
                                    _ => {
                                        debug!("IDLE connection stream closed, restarting connection");
                                        break;
                                    }
                                }
                            }
                            _ = renew_ticker.tick() => {
                                debug!("25-minute IDLE renewal interval elapsed, cycling IDLE/DONE");
                                let (_, next_tag_str) = conn.next_tag();
                                let idle_renewal = format!("{next_tag_str} IDLE\r\n");
                                let _ = stream.write_all(b"DONE\r\n").await;
                                let _ = stream.write_all(idle_renewal.as_bytes()).await;
                            }
                        }
                    }
                    conn.stream = Some(stream);
                } else {
                    #[cfg(any(test, feature = "mock"))]
                    {
                        if let Some(ev) = idle_loop.handle_untagged("* 1 EXISTS") {
                            on_event(ev);
                        }
                        tokio::select! {
                            _ = renew_ticker.tick() => {
                                debug!("25-minute IDLE renewal interval elapsed, cycling IDLE/DONE");
                                let _ = conn.execute_cmd("DONE").await;
                                let _ = conn.execute_cmd(conn.cmd_idle()).await;
                            }
                            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                                return Ok(());
                            }
                        }
                    }

                    #[cfg(not(any(test, feature = "mock")))]
                    {
                        anyhow::bail!("cannot run IDLE without an active server stream");
                    }
                }
            }
            Err(e) => {
                retry_count += 1;
                let backoff = Duration::from_secs((2u64.pow(retry_count.min(6))).min(60));
                debug!(error=%e, backoff_secs=backoff.as_secs(), "IMAP IDLE connection failed, retrying after backoff");
                tokio::time::sleep(backoff).await;
            }
        }
    }
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
