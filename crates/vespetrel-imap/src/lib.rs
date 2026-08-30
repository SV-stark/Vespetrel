//! Vespetrel IMAP Engine - IDLE, CONDSTORE, QRESYNC, XOAUTH2

pub mod client;
pub mod idle;
pub mod sync;

pub use client::{ImapConfig, ImapConnection};
pub use sync::ImapProvider;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImapError {
    #[error("connection error: {0}")]
    Connection(String),
    #[error("auth failed: {0}")]
    Auth(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("uidvalidity changed - cache invalid")]
    UidValidityChanged,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
