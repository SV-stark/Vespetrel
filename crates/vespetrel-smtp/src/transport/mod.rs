//! Pure-Rust asynchronous network transport for ESMTP with Tokio and Rustls.

use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tracing::{debug, info, warn};

use crate::auth::Credentials;
use crate::error::{SmtpError, SmtpResult};
use crate::protocol::command::SmtpCommand;
use crate::protocol::extension::{AuthMechanism, ServerCapabilities};
use crate::protocol::response::SmtpResponse;

/// TLS enforcement policy for the SMTP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsMode {
    /// Connects on submission port (587 or 25) and enforces STARTTLS.
    /// Aborts immediately if the server fails or does not advertise STARTTLS (prevents MITM downgrade attacks).
    #[default]
    StrictStartTls,
    /// Upgrades with STARTTLS if advertised by server; otherwise falls back to plaintext.
    OpportunisticStartTls,
    /// Direct TLS (SMTPS on port 465) from initial connection.
    DirectTls,
    /// Plaintext only (intended strictly for localhost/testing).
    PlaintextOnly,
}

/// Active transport stream (either plain TCP or TLS encrypted).
pub enum SmtpStream {
    Plain(BufReader<TcpStream>),
    Tls(Box<BufReader<TlsStream<TcpStream>>>),
}

impl SmtpStream {
    pub async fn read_line(&mut self, buf: &mut String) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read_line(buf).await,
            Self::Tls(stream) => stream.read_line(buf).await,
        }
    }

    pub async fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => {
                stream.write_all(data).await?;
                stream.flush().await
            }
            Self::Tls(stream) => {
                stream.write_all(data).await?;
                stream.flush().await
            }
        }
    }
}

/// Pure-Rust SMTP Connection session.
pub struct SmtpConnection {
    pub host: String,
    pub port: u16,
    pub timeout_duration: Duration,
    pub stream: Option<SmtpStream>,
    pub capabilities: ServerCapabilities,
    pub banner: Option<String>,
}

impl SmtpConnection {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            timeout_duration: Duration::from_secs(30),
            stream: None,
            capabilities: ServerCapabilities::default(),
            banner: None,
        }
    }

    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout_duration = d;
        self
    }

    /// Establishes the connection, completes greeting and TLS negotiation, and reads EHLO capabilities.
    pub async fn connect(&mut self, tls_mode: TlsMode, client_hostname: &str) -> SmtpResult<()> {
        info!(host=%self.host, port=self.port, ?tls_mode, "connecting to SMTP server");

        let tcp_stream = timeout(
            self.timeout_duration,
            TcpStream::connect((self.host.as_str(), self.port)),
        )
        .await
        .map_err(|_| SmtpError::Timeout(self.timeout_duration))?
        .map_err(|e| SmtpError::Connection {
            host: self.host.clone(),
            port: self.port,
            message: e.to_string(),
        })?;

        if tls_mode == TlsMode::DirectTls {
            let tls_connector = create_tls_connector()?;
            let server_name = ServerName::try_from(self.host.clone())
                .map_err(|e| SmtpError::Tls(format!("invalid server hostname: {e}")))?;
            let tls_stream = tls_connector
                .connect(server_name, tcp_stream)
                .await
                .map_err(|e| SmtpError::Tls(format!("direct TLS handshake failed: {e}")))?;

            self.stream = Some(SmtpStream::Tls(Box::new(BufReader::new(tls_stream))));
        } else {
            self.stream = Some(SmtpStream::Plain(BufReader::new(tcp_stream)));
        }

        // 1. Read greeting banner (220)
        let banner_resp = self.read_response().await?;
        banner_resp.check_ok()?;
        self.banner = Some(banner_resp.message());

        // 2. Initial EHLO
        self.ehlo_or_helo(client_hostname).await?;

        // 3. STARTTLS if required or opportunistic
        if tls_mode == TlsMode::StrictStartTls || tls_mode == TlsMode::OpportunisticStartTls {
            if self.capabilities.supports_starttls {
                self.perform_starttls(client_hostname).await?;
            } else if tls_mode == TlsMode::StrictStartTls {
                return Err(SmtpError::Tls(
                    "server does not advertise STARTTLS; strict TLS mode rejected plaintext fallback"
                        .into(),
                ));
            } else {
                warn!(
                    host=%self.host,
                    "opportunistic TLS selected but STARTTLS not advertised; proceeding unencrypted"
                );
            }
        }

        Ok(())
    }

    /// Upgrades an existing plain TCP stream to TLS using STARTTLS.
    async fn perform_starttls(&mut self, client_hostname: &str) -> SmtpResult<()> {
        debug!(host=%self.host, "issuing STARTTLS");
        let resp = self.execute(&SmtpCommand::StartTls).await?;
        resp.check_ok()?;

        let Some(SmtpStream::Plain(reader)) = self.stream.take() else {
            return Err(SmtpError::Tls("cannot upgrade non-plain stream".into()));
        };
        let tcp_stream = reader.into_inner();

        let tls_connector = create_tls_connector()?;
        let server_name = ServerName::try_from(self.host.clone())
            .map_err(|e| SmtpError::Tls(format!("invalid server hostname: {e}")))?;

        let tls_stream = tls_connector
            .connect(server_name, tcp_stream)
            .await
            .map_err(|e| SmtpError::Tls(format!("STARTTLS handshake failed: {e}")))?;

        self.stream = Some(SmtpStream::Tls(Box::new(BufReader::new(tls_stream))));
        debug!(host=%self.host, "STARTTLS handshake established successfully");

        // Per RFC 3207 §4.2, client MUST re-issue EHLO after successful TLS upgrade
        self.ehlo_or_helo(client_hostname).await?;
        Ok(())
    }

    /// Performs EHLO, falling back to HELO if rejected.
    pub async fn ehlo_or_helo(&mut self, client_hostname: &str) -> SmtpResult<()> {
        let ehlo_cmd = SmtpCommand::Ehlo(client_hostname.to_string());
        match self.execute(&ehlo_cmd).await {
            Ok(resp) if resp.is_positive_completion() => {
                self.capabilities = ServerCapabilities::from_ehlo_lines(&resp.lines);
                debug!(caps=?self.capabilities, "EHLO negotiation completed");
                Ok(())
            }
            _ => {
                debug!("EHLO rejected, falling back to legacy HELO");
                let helo_cmd = SmtpCommand::Helo(client_hostname.to_string());
                let resp = self.execute(&helo_cmd).await?;
                resp.check_ok()?;
                self.capabilities = ServerCapabilities::default();
                Ok(())
            }
        }
    }

    /// Authenticates with the server using the best available mechanism.
    pub async fn authenticate(&mut self, creds: &Credentials) -> SmtpResult<()> {
        let has_oauth = creds.oauth_token.is_some();
        let has_pass = creds.password.is_some();

        let fallback_mech = if has_oauth {
            Some(AuthMechanism::XOAuth2)
        } else if has_pass {
            Some(AuthMechanism::Plain)
        } else {
            None
        };

        let mech = self
            .capabilities
            .select_best_auth(has_oauth, has_pass)
            .or(fallback_mech)
            .ok_or_else(|| SmtpError::Authentication {
                mechanism: "NONE".into(),
                message: "no compatible authentication mechanism supported by server".into(),
            })?;

        debug!(mechanism=%mech, "authenticating with SMTP server");

        match mech {
            AuthMechanism::Plain => {
                let initial = creds.plain_initial_response()?;
                let cmd = SmtpCommand::Auth {
                    mechanism: AuthMechanism::Plain,
                    initial_response: Some(initial),
                };
                let resp = self.execute(&cmd).await?;
                resp.check_ok()?;
            }
            AuthMechanism::Login => {
                let cmd = SmtpCommand::Auth {
                    mechanism: AuthMechanism::Login,
                    initial_response: None,
                };
                let resp = self.execute(&cmd).await?;
                resp.check_intermediate()?;

                // Send username
                let user_b64 = creds.login_username_response();
                self.send_raw_line(&user_b64).await?;
                let resp_pass = self.read_response().await?;
                resp_pass.check_intermediate()?;

                // Send password
                let pass_b64 = creds.login_password_response()?;
                self.send_raw_line(&pass_b64).await?;
                let resp_done = self.read_response().await?;
                resp_done.check_ok()?;
            }
            AuthMechanism::XOAuth2 => {
                let initial = creds.xoauth2_initial_response()?;
                let cmd = SmtpCommand::Auth {
                    mechanism: AuthMechanism::XOAuth2,
                    initial_response: Some(initial),
                };
                let resp = self.execute(&cmd).await?;
                if resp.is_positive_intermediate() {
                    // Google/MS error challenge, send empty line to finalize rejection
                    self.send_raw_line("").await?;
                    let err_resp = self.read_response().await?;
                    return Err(SmtpError::Authentication {
                        mechanism: "XOAUTH2".into(),
                        message: err_resp.message(),
                    });
                }
                resp.check_ok()?;
            }
            AuthMechanism::OAuthBearer => {
                let initial = creds.oauthbearer_initial_response()?;
                let cmd = SmtpCommand::Auth {
                    mechanism: AuthMechanism::OAuthBearer,
                    initial_response: Some(initial),
                };
                let resp = self.execute(&cmd).await?;
                resp.check_ok()?;
            }
            AuthMechanism::CramMd5 => {
                let cmd = SmtpCommand::Auth {
                    mechanism: AuthMechanism::CramMd5,
                    initial_response: None,
                };
                let resp = self.execute(&cmd).await?;
                resp.check_intermediate()?;

                let challenge = resp.lines.first().cloned().unwrap_or_default();
                let client_resp = creds.cram_md5_response(&challenge)?;
                self.send_raw_line(&client_resp).await?;
                let resp_done = self.read_response().await?;
                resp_done.check_ok()?;
            }
            _ => {
                return Err(SmtpError::Authentication {
                    mechanism: mech.to_string(),
                    message: "unimplemented SASL mechanism".into(),
                });
            }
        }

        info!("SMTP authentication successful");
        Ok(())
    }

    /// Transmits an email using high-efficiency pipelining or standard sequence.
    pub async fn transmit_message(
        &mut self,
        sender: &str,
        recipients: &[String],
        raw_rfc5322_data: &[u8],
    ) -> SmtpResult<String> {
        if recipients.is_empty() {
            return Err(SmtpError::Formatting("no recipients specified".into()));
        }

        // Preemptive message size check
        if self
            .capabilities
            .max_message_size
            .is_some_and(|max_sz| (raw_rfc5322_data.len() as u64) > max_sz)
        {
            let max_sz = self.capabilities.max_message_size.unwrap_or_default();
            return Err(SmtpError::Protocol {
                code: 552,
                enhanced: None,
                message: format!(
                    "message size {} exceeds server limit of {}",
                    raw_rfc5322_data.len(),
                    max_sz
                ),
            });
        }

        let body_8bit = self.capabilities.supports_8bitmime;
        let smtputf8 = self.capabilities.supports_smtputf8;

        let mail_cmd = SmtpCommand::MailFrom {
            sender: sender.to_string(),
            size: Some(raw_rfc5322_data.len() as u64),
            body_8bit,
            smtputf8,
            dsn_ret: None,
            dsn_envid: None,
        };

        if self.capabilities.supports_pipelining {
            // High-throughput PIPELINING (RFC 2920): batch MAIL FROM + all RCPT TO + DATA
            let mut batch = Vec::new();
            batch.extend_from_slice(&mail_cmd.to_wire_bytes()?);

            for rcpt in recipients {
                let rcpt_cmd = SmtpCommand::RcptTo {
                    recipient: rcpt.clone(),
                    dsn_notify: None,
                    dsn_orcpt: None,
                };
                batch.extend_from_slice(&rcpt_cmd.to_wire_bytes()?);
            }
            batch.extend_from_slice(&SmtpCommand::Data.to_wire_bytes()?);

            self.stream_mut()?.write_all(&batch).await?;

            // Read MAIL FROM response
            let mail_resp = self.read_response().await?;
            mail_resp.check_ok()?;

            // Read RCPT TO responses
            let mut at_least_one_accepted = false;
            for _ in recipients {
                let rcpt_resp = self.read_response().await?;
                if rcpt_resp.is_positive_completion() {
                    at_least_one_accepted = true;
                }
            }

            if !at_least_one_accepted {
                let _ = self.read_response().await; // consume DATA response
                return Err(SmtpError::Protocol {
                    code: 550,
                    enhanced: None,
                    message: "all recipients were rejected by the server".into(),
                });
            }

            // Read DATA response (354)
            let data_resp = self.read_response().await?;
            data_resp.check_intermediate()?;
        } else {
            // Standard sequential interaction
            let mail_resp = self.execute(&mail_cmd).await?;
            mail_resp.check_ok()?;

            let mut at_least_one_accepted = false;
            for rcpt in recipients {
                let rcpt_cmd = SmtpCommand::RcptTo {
                    recipient: rcpt.clone(),
                    dsn_notify: None,
                    dsn_orcpt: None,
                };
                let resp = self.execute(&rcpt_cmd).await?;
                if resp.is_positive_completion() {
                    at_least_one_accepted = true;
                }
            }

            if !at_least_one_accepted {
                return Err(SmtpError::Protocol {
                    code: 550,
                    enhanced: None,
                    message: "all recipients were rejected by the server".into(),
                });
            }

            let data_resp = self.execute(&SmtpCommand::Data).await?;
            data_resp.check_intermediate()?;
        }

        // Send dot-stuffed body and terminating "\r\n.\r\n"
        let stuffed_data = dot_stuff_rfc5321(raw_rfc5322_data);
        self.stream_mut()?.write_all(&stuffed_data).await?;

        // Read final delivery response (250)
        let done_resp = self.read_response().await?;
        done_resp.check_ok()?;

        let server_id = done_resp.message();
        info!(server_id=%server_id, "SMTP message delivered and acknowledged");
        Ok(server_id)
    }

    /// Closes the SMTP session gracefully with QUIT.
    pub async fn quit(&mut self) -> SmtpResult<()> {
        if self.stream.is_some() {
            let _ = self.execute(&SmtpCommand::Quit).await;
            self.stream = None;
        }
        Ok(())
    }

    pub async fn execute(&mut self, cmd: &SmtpCommand) -> SmtpResult<SmtpResponse> {
        let wire = cmd.to_wire_bytes()?;
        self.stream_mut()?.write_all(&wire).await?;
        self.read_response().await
    }

    pub async fn send_raw_line(&mut self, line: &str) -> SmtpResult<()> {
        let mut wire = line.as_bytes().to_vec();
        wire.extend_from_slice(b"\r\n");
        self.stream_mut()?.write_all(&wire).await?;
        Ok(())
    }

    pub async fn read_response(&mut self) -> SmtpResult<SmtpResponse> {
        let timeout_dur = self.timeout_duration;
        let stream = self.stream_mut()?;
        let mut raw_lines = Vec::new();

        loop {
            let mut line = String::new();
            let bytes_read = timeout(timeout_dur, stream.read_line(&mut line))
                .await
                .map_err(|_| SmtpError::Timeout(timeout_dur))?
                .map_err(SmtpError::Io)?;

            if bytes_read == 0 {
                return Err(SmtpError::Handshake(
                    "connection unexpectedly closed by remote host".into(),
                ));
            }

            let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
            let is_last = trimmed.len() <= 3 || trimmed.as_bytes()[3] != b'-';
            raw_lines.push(trimmed);

            if is_last {
                break;
            }
        }

        SmtpResponse::parse_raw_lines(&raw_lines)
    }

    fn stream_mut(&mut self) -> SmtpResult<&mut SmtpStream> {
        self.stream.as_mut().ok_or_else(|| SmtpError::Connection {
            host: self.host.clone(),
            port: self.port,
            message: "not connected".into(),
        })
    }
}

/// Applies RFC 5321 §4.5.2 transparency dot-stuffing and trailing CRLF.CRLF.
pub fn dot_stuff_rfc5321(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + 64);
    let mut at_line_start = true;

    for &b in raw {
        if at_line_start && b == b'.' {
            out.push(b'.'); // escape leading dot
        }

        out.push(b);

        if b == b'\n' {
            at_line_start = true;
        } else if b != b'\r' {
            at_line_start = false;
        }
    }

    // Ensure ending with \r\n.\r\n
    if !out.ends_with(b"\r\n") {
        if out.ends_with(b"\n") {
            out.pop();
            out.extend_from_slice(b"\r\n");
        } else {
            out.extend_from_slice(b"\r\n");
        }
    }
    out.extend_from_slice(b".\r\n");
    out
}

/// Creates a pure-Rust Rustls ClientConfig with WebPKI system roots.
fn create_tls_connector() -> SmtpResult<TlsConnector> {
    let mut root_store = RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Ok(TlsConnector::from(Arc::new(config)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dot_stuffing() {
        let raw = b"Line 1\r\n.Line starting with dot\r\nLine 3\r\n";
        let stuffed = dot_stuff_rfc5321(raw);
        let s = String::from_utf8(stuffed).unwrap();
        assert_eq!(s, "Line 1\r\n..Line starting with dot\r\nLine 3\r\n.\r\n");
    }
}
