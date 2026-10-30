//! RFC 3461 Delivery Status Notification (DSN) parameters for ESMTP.

use serde::{Deserialize, Serialize};

/// Delivery Status Notification return of content option (`RET` parameter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DsnReturnMode {
    /// Return the full message body if delivery fails (`RET=FULL`).
    Full,
    /// Return only the headers if delivery fails (`RET=HDRS`).
    HeadersOnly,
}

impl DsnReturnMode {
    pub fn as_esmtp_str(&self) -> &'static str {
        match self {
            Self::Full => "FULL",
            Self::HeadersOnly => "HDRS",
        }
    }
}

/// Delivery Status Notification conditions (`NOTIFY` parameter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DsnNotify {
    /// Never send a notification (`NOTIFY=NEVER`). Must be used exclusively.
    Never,
    /// Notify on successful delivery (`NOTIFY=SUCCESS`).
    Success,
    /// Notify on failure (`NOTIFY=FAILURE`).
    Failure,
    /// Notify if delivery is delayed (`NOTIFY=DELAY`).
    Delay,
}

impl DsnNotify {
    pub fn as_esmtp_str(&self) -> &'static str {
        match self {
            Self::Never => "NEVER",
            Self::Success => "SUCCESS",
            Self::Failure => "FAILURE",
            Self::Delay => "DELAY",
        }
    }
}

/// Consolidated Delivery Status Notification options for an outgoing message or recipient.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DsnOptions {
    /// Request full message or headers-only bounce report.
    pub return_mode: Option<DsnReturnMode>,
    /// Tracking Envelope ID (`ENVID`).
    pub envelope_id: Option<String>,
    /// Notification conditions for recipients.
    pub notify: Option<Vec<DsnNotify>>,
    /// Original recipient format address (`ORCPT`).
    pub original_recipient: Option<String>,
}

impl DsnOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_return_mode(mut self, mode: DsnReturnMode) -> Self {
        self.return_mode = Some(mode);
        self
    }

    pub fn with_envelope_id(mut self, envid: impl Into<String>) -> Self {
        self.envelope_id = Some(envid.into());
        self
    }

    pub fn with_notify(mut self, conditions: Vec<DsnNotify>) -> Self {
        self.notify = Some(conditions);
        self
    }

    pub fn with_original_recipient(mut self, orcpt: impl Into<String>) -> Self {
        self.original_recipient = Some(orcpt.into());
        self
    }
}
