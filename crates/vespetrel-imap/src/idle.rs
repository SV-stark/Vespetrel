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

    /// Classify a server-pushed untagged response.
    ///
    /// Previously this substring-matched anywhere on the line, so a message body line
    /// containing `* 1 FETCH (FLAGS (\Seen))` was reported as a flag change, and three real
    /// untagged types that change mailbox state produced *no* event at all:
    /// `* VANISHED (EARLIER) ...` (what a QRESYNC mailbox sends instead of `EXPUNGE`),
    /// `* n RECENT`, and `* n FETCH (UID n MODSEQ (m))` (a CONDSTORE change without `FLAGS`).
    pub fn handle_untagged(&self, line: &str) -> Option<IdleEvent> {
        let trimmed = line.trim();
        // Only untagged responses carry mailbox state; a tagged line must never match.
        if !trimmed.starts_with("* ") {
            return None;
        }
        let rest = trimmed.trim_start_matches("* ").trim_start();
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("VANISHED") {
            return Some(IdleEvent::Vanished);
        }
        if upper.starts_with("EXPUNGE") || upper.contains(" EXPUNGE") {
            return Some(IdleEvent::Expunged);
        }
        if upper.ends_with(" EXISTS") || upper.ends_with(" RECENT") {
            return Some(IdleEvent::NewMail);
        }
        if upper.contains(" FETCH") {
            return Some(IdleEvent::FlagChange);
        }
        None
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
    /// RFC 7162 `* VANISHED (EARLIER) ...`: a QRESYNC mailbox reports removals this way
    /// instead of `EXPUNGE`.
    Vanished,
    FlagChange,
}

/// Async idle runner handling connection heartbeats, RFC 2177 renewals, and pushes
pub async fn run_idle_loop<F>(_on_event: F) -> anyhow::Result<()>
where
    F: Fn(IdleEvent) + Send + Sync + 'static,
{
    anyhow::bail!(
        "run_idle_loop requires explicit ImapConfig; use run_idle_loop_with_config or run_idle_loop_with_shutdown"
    );
}

/// Run a full live IMAP IDLE connection loop with automated 25-minute RFC 2177 DONE/IDLE renewal and reconnection
pub async fn run_idle_loop_with_config<F>(
    config: crate::client::ImapConfig,
    on_event: F,
) -> anyhow::Result<()>
where
    F: Fn(IdleEvent) + Send + Sync + 'static,
{
    run_idle_loop_with_shutdown(config, None, on_event).await
}

/// Run a full live IMAP IDLE connection loop with automated 25-minute RFC 2177 DONE/IDLE renewal,
/// strict '+' continuation enforcement, and graceful shutdown signal handling.
pub async fn run_idle_loop_with_shutdown<F>(
    config: crate::client::ImapConfig,
    mut shutdown: Option<tokio::sync::watch::Receiver<bool>>,
    on_event: F,
) -> anyhow::Result<()>
where
    F: Fn(IdleEvent) + Send + Sync + 'static,
{
    let mut retry_count = 0;
    loop {
        if let Some(ref rx) = shutdown
            && *rx.borrow()
        {
            debug!("IMAP IDLE shutdown requested before connect");
            return Ok(());
        }

        info!(host=%config.host, "starting IMAP IDLE connection");
        let mut conn = crate::client::ImapConnection::new(config.clone());
        // RFC 3501 §6.1.4 / RFC 2177: `IDLE` is only valid in the SELECTED state.
        // `connect()` performs CAPABILITY, AUTHENTICATE/LOGIN and ENABLE QRESYNC and
        // nothing else, so the previous code sent IDLE straight after authenticating and
        // every conforming server answered `BAD`/`NO Please select a mailbox first`, which
        // propagated out of this function and killed the worker permanently.
        let mut select_ok = false;
        match conn.connect().await {
            Ok(()) => {
                if conn.is_mock() {
                    select_ok = true;
                } else {
                    let select_cmd = if conn.has_capability("QRESYNC")
                        && config.uid_validity.is_some()
                        && config.highest_mod_seq > 0
                    {
                        conn.cmd_select_qresync(
                            &config.mailbox,
                            config.uid_validity.unwrap_or(0),
                            config.highest_mod_seq,
                        )
                    } else {
                        conn.cmd_select(&config.mailbox)
                    };
                    let select_tag = conn.last_tag.clone();
                    match conn.execute_cmd(&select_cmd).await {
                        Ok(lines) => {
                            if lines.iter().any(|l| {
                                l.starts_with(&format!("{select_tag} NO"))
                                    || l.starts_with(&format!("{select_tag} BAD"))
                            }) {
                                debug!(
                                    mailbox=%config.mailbox,
                                    "SELECT rejected before IDLE; falling back to incremental sync"
                                );
                            } else {
                                select_ok = true;
                            }
                        }
                        Err(e) => {
                            debug!(error=%e, "SELECT failed before IDLE");
                        }
                    }
                }
            }
            Err(e) => {
                debug!(error=%e, "IMAP IDLE connect failed");
            }
        }

        if select_ok {
            let idle_loop = IdleLoop::new();
            let mut renew_ticker = tokio::time::interval(idle_loop.renew_interval);
            renew_ticker.tick().await; // Consume initial immediate tick

            if let Some(mut stream) = conn.stream.take() {
                let mut tag_state = conn.next_tag();
                let mut idle_cmd = format!("{} IDLE\r\n", tag_state.1);
                debug!(tag=%tag_state.1, "issuing IDLE command and awaiting '+' continuation");
                if let Err(e) = stream.write_all(idle_cmd.as_bytes()).await {
                    debug!(error=%e, "failed to send IDLE command");
                    continue;
                }

                // Await '+' continuation per RFC 2177
                let mut cont_buf = [0u8; 1024];
                let mut idling = false;
                while !idling {
                    match stream.read(&mut cont_buf).await {
                        Ok(n) if n > 0 => {
                            let text = String::from_utf8_lossy(&cont_buf[..n]);
                            for line in text.lines() {
                                if line.starts_with('+') || line.contains(" +") {
                                    idling = true;
                                } else if let Some(ev) = idle_loop.handle_untagged(line) {
                                    on_event(ev);
                                } else if line.contains(&format!("{} NO", tag_state.1))
                                    || line.contains(&format!("{} BAD", tag_state.1))
                                {
                                    anyhow::bail!("server rejected IDLE command: {line}");
                                }
                            }
                        }
                        _ => {
                            debug!("stream closed while awaiting IDLE '+' continuation");
                            break;
                        }
                    }
                }

                if !idling {
                    continue;
                }

                let mut buf = [0u8; 4096];
                let mut should_exit = false;

                while !should_exit {
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
                            debug!("25-minute IDLE renewal interval elapsed, sending DONE and waiting for OK");
                            let _ = stream.write_all(b"DONE\r\n").await;
                            // Wait for tagged OK for the previous IDLE command
                            let mut ok_buf = [0u8; 1024];
                            let mut got_ok = false;
                            while !got_ok {
                                match stream.read(&mut ok_buf).await {
                                    Ok(n) if n > 0 => {
                                        let text = String::from_utf8_lossy(&ok_buf[..n]);
                                        for line in text.lines() {
                                            if let Some(ev) = idle_loop.handle_untagged(line) {
                                                on_event(ev);
                                            }
                                            if line.contains(&format!("{} OK", tag_state.1)) {
                                                got_ok = true;
                                            }
                                        }
                                    }
                                    _ => break,
                                }
                            }

                            tag_state = conn.next_tag();
                            idle_cmd = format!("{} IDLE\r\n", tag_state.1);
                            let _ = stream.write_all(idle_cmd.as_bytes()).await;
                            // Wait for '+' continuation
                            let mut cont_buf = [0u8; 1024];
                            let mut renewed = false;
                            while !renewed {
                                match stream.read(&mut cont_buf).await {
                                    Ok(n) if n > 0 => {
                                        let text = String::from_utf8_lossy(&cont_buf[..n]);
                                        for line in text.lines() {
                                            if line.starts_with('+') || line.contains(" +") {
                                                renewed = true;
                                            } else if let Some(ev) = idle_loop.handle_untagged(line) {
                                                on_event(ev);
                                            }
                                        }
                                    }
                                    _ => break,
                                }
                            }
                            if !renewed {
                                debug!("failed to renew IDLE session, reconnecting");
                                break;
                            }
                        }
                        _ = async {
                            if let Some(ref mut rx) = shutdown {
                                while !*rx.borrow() {
                                    if rx.changed().await.is_err() {
                                        break;
                                    }
                                }
                            } else {
                                std::future::pending::<()>().await;
                            }
                        } => {
                            info!("shutdown signal received in IDLE loop, exiting cleanly");
                            let _ = stream.write_all(b"DONE\r\n").await;
                            let _ = stream.write_all(b"A9999 LOGOUT\r\n").await;
                            should_exit = true;
                        }
                    }
                }

                if should_exit {
                    return Ok(());
                }
            } else if conn.is_mock() {
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
            } else {
                debug!("cannot run IDLE without an active server stream");
            }
            // The session ended. Only a session that actually idled (rather than being
            // torn down immediately) counts as a success and clears the backoff.
            retry_count = 0;
        }

        // Back off on *every* (re)connect, not just on a connect error. A server that
        // completes the TLS handshake and authentication and then immediately closes the
        // stream took the success path, so the loop restarted with zero delay and hammered
        // the server with full handshakes hundreds of times a second.
        retry_count += 1;
        let backoff = Duration::from_secs((2u64.pow(retry_count.min(6))).min(60));
        debug!(
            backoff_secs = backoff.as_secs(),
            "IMAP IDLE reconnect backoff"
        );
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            _ = async {
                if let Some(ref mut rx) = shutdown {
                    while !*rx.borrow() {
                        if rx.changed().await.is_err() {
                            break;
                        }
                    }
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                return Ok(());
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
