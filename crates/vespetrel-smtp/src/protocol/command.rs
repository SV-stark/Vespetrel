//! ESMTP Command formatting and sanitization.

use crate::dsn::{DsnNotify, DsnReturnMode};
use crate::error::{SmtpError, SmtpResult};
use crate::protocol::extension::AuthMechanism;

/// Sanitizes a string parameter to ensure no CR or LF injection is possible.
pub fn sanitize_smtp_string(val: &str) -> SmtpResult<String> {
    if val.contains('\r') || val.contains('\n') {
        return Err(SmtpError::Formatting(
            "CR or LF character detected in SMTP command parameter".to_string(),
        ));
    }
    Ok(val.trim().to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmtpCommand {
    Ehlo(String),
    Helo(String),
    StartTls,
    Auth {
        mechanism: AuthMechanism,
        initial_response: Option<String>,
    },
    MailFrom {
        sender: String,
        size: Option<u64>,
        body_8bit: bool,
        smtputf8: bool,
        dsn_ret: Option<DsnReturnMode>,
        dsn_envid: Option<String>,
    },
    RcptTo {
        recipient: String,
        dsn_notify: Option<Vec<DsnNotify>>,
        dsn_orcpt: Option<String>,
    },
    Data,
    Bdat {
        size: usize,
        last: bool,
    },
    Rset,
    Noop,
    Quit,
}

impl SmtpCommand {
    /// Formats the command into wire bytes with trailing `\r\n`.
    pub fn to_wire_bytes(&self) -> SmtpResult<Vec<u8>> {
        let mut s = String::new();
        match self {
            Self::Ehlo(domain) => {
                let clean = sanitize_smtp_string(domain)?;
                s.push_str("EHLO ");
                s.push_str(&clean);
            }
            Self::Helo(domain) => {
                let clean = sanitize_smtp_string(domain)?;
                s.push_str("HELO ");
                s.push_str(&clean);
            }
            Self::StartTls => {
                s.push_str("STARTTLS");
            }
            Self::Auth {
                mechanism,
                initial_response,
            } => {
                s.push_str("AUTH ");
                s.push_str(mechanism.as_str());
                if let Some(resp) = initial_response {
                    let clean = sanitize_smtp_string(resp)?;
                    s.push(' ');
                    s.push_str(&clean);
                }
            }
            Self::MailFrom {
                sender,
                size,
                body_8bit,
                smtputf8,
                dsn_ret,
                dsn_envid,
            } => {
                let clean = sanitize_smtp_string(sender)?;
                s.push_str("MAIL FROM:<");
                s.push_str(&clean);
                s.push('>');

                if let Some(sz) = size {
                    s.push_str(&format!(" SIZE={sz}"));
                }
                if *body_8bit {
                    s.push_str(" BODY=8BITMIME");
                }
                if *smtputf8 {
                    s.push_str(" SMTPUTF8");
                }
                if let Some(ret) = dsn_ret {
                    s.push_str(&format!(" RET={}", ret.as_esmtp_str()));
                }
                if let Some(envid) = dsn_envid {
                    let clean_envid = sanitize_smtp_string(envid)?;
                    s.push_str(&format!(" ENVID={clean_envid}"));
                }
            }
            Self::RcptTo {
                recipient,
                dsn_notify,
                dsn_orcpt,
            } => {
                let clean = sanitize_smtp_string(recipient)?;
                s.push_str("RCPT TO:<");
                s.push_str(&clean);
                s.push('>');

                if let Some(notifies) = dsn_notify.as_ref().filter(|n| !n.is_empty()) {
                    let notify_str: Vec<&str> =
                        notifies.iter().map(DsnNotify::as_esmtp_str).collect();
                    s.push_str(&format!(" NOTIFY={}", notify_str.join(",")));
                }
                if let Some(orcpt) = dsn_orcpt {
                    let clean_orcpt = sanitize_smtp_string(orcpt)?;
                    s.push_str(&format!(" ORCPT=rfc822;{clean_orcpt}"));
                }
            }
            Self::Data => {
                s.push_str("DATA");
            }
            Self::Bdat { size, last } => {
                s.push_str(&format!("BDAT {size}"));
                if *last {
                    s.push_str(" LAST");
                }
            }
            Self::Rset => {
                s.push_str("RSET");
            }
            Self::Noop => {
                s.push_str("NOOP");
            }
            Self::Quit => {
                s.push_str("QUIT");
            }
        }

        s.push_str("\r\n");
        Ok(s.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ehlo_command() {
        let cmd = SmtpCommand::Ehlo("client.vespetrel.org".into());
        let wire = String::from_utf8(cmd.to_wire_bytes().unwrap()).unwrap();
        assert_eq!(wire, "EHLO client.vespetrel.org\r\n");
    }

    #[test]
    fn test_mail_from_with_extensions() {
        let cmd = SmtpCommand::MailFrom {
            sender: "alice@example.com".into(),
            size: Some(102400),
            body_8bit: true,
            smtputf8: true,
            dsn_ret: Some(DsnReturnMode::HeadersOnly),
            dsn_envid: Some("tx-12345".into()),
        };
        let wire = String::from_utf8(cmd.to_wire_bytes().unwrap()).unwrap();
        assert_eq!(
            wire,
            "MAIL FROM:<alice@example.com> SIZE=102400 BODY=8BITMIME SMTPUTF8 RET=HDRS ENVID=tx-12345\r\n"
        );
    }

    #[test]
    fn test_rcpt_to_with_dsn() {
        let cmd = SmtpCommand::RcptTo {
            recipient: "bob@example.com".into(),
            dsn_notify: Some(vec![DsnNotify::Success, DsnNotify::Failure]),
            dsn_orcpt: Some("bob@example.com".into()),
        };
        let wire = String::from_utf8(cmd.to_wire_bytes().unwrap()).unwrap();
        assert_eq!(
            wire,
            "RCPT TO:<bob@example.com> NOTIFY=SUCCESS,FAILURE ORCPT=rfc822;bob@example.com\r\n"
        );
    }

    #[test]
    fn test_crlf_injection_protection() {
        let cmd = SmtpCommand::Ehlo("attacker.com\r\nMAIL FROM:<evil@bad.com>".into());
        assert!(cmd.to_wire_bytes().is_err());
    }
}
