//! Standard RFC 5321 and RFC 4954 SMTP reply codes.

pub const SYSTEM_STATUS: u16 = 211;
pub const HELP_MESSAGE: u16 = 214;
pub const SERVICE_READY: u16 = 220;
pub const SERVICE_CLOSING: u16 = 221;
pub const AUTH_SUCCESS: u16 = 235;
pub const OK: u16 = 250;
pub const WILL_FORWARD: u16 = 251;
pub const CANNOT_VRFY: u16 = 252;

pub const AUTH_CONTINUE: u16 = 334;
pub const START_MAIL_INPUT: u16 = 354;

pub const SERVICE_NOT_AVAILABLE: u16 = 421;
pub const MAILBOX_TEMPORARILY_UNAVAILABLE: u16 = 450;
pub const LOCAL_ERROR: u16 = 451;
pub const INSUFFICIENT_STORAGE: u16 = 452;
pub const TLS_TEMPORARILY_UNAVAILABLE: u16 = 454;

pub const SYNTAX_ERROR: u16 = 500;
pub const SYNTAX_ERROR_PARAMS: u16 = 501;
pub const COMMAND_NOT_IMPLEMENTED: u16 = 502;
pub const BAD_SEQUENCE: u16 = 503;
pub const PARAMETER_NOT_IMPLEMENTED: u16 = 504;
pub const AUTH_REQUIRED: u16 = 530;
pub const AUTH_CREDENTIALS_INVALID: u16 = 535;
pub const AUTH_ENCRYPTION_REQUIRED: u16 = 538;
pub const MAILBOX_UNAVAILABLE: u16 = 550;
pub const USER_NOT_LOCAL: u16 = 551;
pub const EXCEEDED_STORAGE_ALLOCATION: u16 = 552;
pub const MAILBOX_NAME_NOT_ALLOWED: u16 = 553;
pub const TRANSACTION_FAILED: u16 = 554;

/// Returns true if the code is 2xx (Positive Completion).
#[inline]
pub fn is_positive_completion(code: u16) -> bool {
    (200..300).contains(&code)
}

/// Returns true if the code is 3xx (Positive Intermediate, e.g. 354 or 334).
#[inline]
pub fn is_positive_intermediate(code: u16) -> bool {
    (300..400).contains(&code)
}

/// Returns true if the code is 4xx (Transient Negative Completion).
#[inline]
pub fn is_transient_negative(code: u16) -> bool {
    (400..500).contains(&code)
}

/// Returns true if the code is 5xx (Permanent Negative Completion).
#[inline]
pub fn is_permanent_negative(code: u16) -> bool {
    (500..600).contains(&code)
}
