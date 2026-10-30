use thiserror::Error;

use crate::protocol::enhanced::EnhancedStatusCode;

/// Result alias for SMTP operations
pub type SmtpResult<T> = Result<T, SmtpError>;

/// Detailed SMTP client errors with classification for retry logic.
#[derive(Debug, Error)]
pub enum SmtpError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("connection failed to {host}:{port}: {message}")]
    Connection {
        host: String,
        port: u16,
        message: String,
    },

    #[error("TLS negotiation error: {0}")]
    Tls(String),

    #[error("server greeted with invalid banner or protocol violation: {0}")]
    Handshake(String),

    #[error("SMTP command rejected with reply code {code}: {message}")]
    Protocol {
        code: u16,
        enhanced: Option<EnhancedStatusCode>,
        message: String,
    },

    #[error("authentication failed using mechanism {mechanism}: {message}")]
    Authentication { mechanism: String, message: String },

    #[error("timed out after {0:?}")]
    Timeout(std::time::Duration),

    #[error("message formatting error: {0}")]
    Formatting(String),

    #[error("DKIM signing error: {0}")]
    Dkim(String),

    #[error("queue error: {0}")]
    Queue(String),

    #[error("{0}")]
    Other(String),
}

impl SmtpError {
    /// Returns true if this error represents a transient failure (e.g. 4xx reply code,
    /// network timeout, temporary DNS blip) that should be retried in the outbox queue.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Io(_) | Self::Timeout(_) => true,
            Self::Connection { .. } => true,
            Self::Protocol { code, enhanced, .. } => {
                if let Some(enh) = enhanced {
                    enh.is_transient()
                } else {
                    *code >= 400 && *code < 500
                }
            }
            Self::Authentication { .. } => false,
            Self::Tls(_) => false,
            Self::Handshake(_) => false,
            Self::Formatting(_) => false,
            Self::Dkim(_) => false,
            Self::Queue(_) => false,
            Self::Other(_) => false,
        }
    }

    /// Returns true if this error represents a permanent failure (e.g. 5xx reply code,
    /// invalid credentials, malformed message) that should NOT be retried.
    #[must_use]
    pub fn is_permanent(&self) -> bool {
        !self.is_transient()
    }

    /// Returns the SMTP reply code if this error originated from a server response.
    #[must_use]
    pub fn reply_code(&self) -> Option<u16> {
        match self {
            Self::Protocol { code, .. } => Some(*code),
            _ => None,
        }
    }

    /// Returns the RFC 3463 Enhanced Status Code if present.
    #[must_use]
    pub fn enhanced_code(&self) -> Option<&EnhancedStatusCode> {
        match self {
            Self::Protocol { enhanced, .. } => enhanced.as_ref(),
            _ => None,
        }
    }
}
