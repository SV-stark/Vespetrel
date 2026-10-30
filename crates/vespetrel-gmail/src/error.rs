//! Gmail REST API errors, and the classification that decides what to do about each.
//!
//! The classification matters more than the message text. Gmail signals several very different
//! situations through the same HTTP 403, and collapsing them into "auth failed" sends the user
//! through a pointless re-consent when the real problem is a per-project quota that resolves on
//! its own.

/// A failure from the Gmail REST API.
#[derive(Debug, thiserror::Error)]
pub enum GmailError {
    /// The access token is absent, expired or lacks the granted scope.
    ///
    /// Only the user can fix this by re-authorizing.
    #[error("Gmail rejected the access token: {0}")]
    NeedsReauth(String),

    /// Quota exhausted for the project. Resolves on its own; retry with backoff.
    #[error("Gmail rate limit hit: {0}")]
    RateLimited(String),

    /// The requested `historyId` is older than Gmail retains.
    ///
    /// Gmail keeps roughly seven days of history. A cursor older than that cannot be advanced —
    /// the gap is unrecoverable, so the mailbox must be rebuilt from a full sync. This is a
    /// normal condition for an account that has not synced in a week, not an error to retry.
    #[error("Gmail history cursor {requested} is no longer available; a full sync is required")]
    HistoryExpired { requested: String },

    /// 5xx, connection reset, DNS failure, or a body that did not arrive in time.
    ///
    /// Safe to retry; the same request is not safe to *skip*.
    #[error("Gmail transport failure: {0}")]
    Transient(String),

    /// A well-formed response that did not make sense (missing field, wrong type, oversized body).
    ///
    /// Retrying will produce the same response, so this is a bug to investigate rather than an
    /// outage to ride out.
    #[error("Gmail protocol error: {0}")]
    Protocol(String),

    /// The request was rejected as malformed. A bug in this crate.
    #[error("Gmail rejected the request: {0}")]
    BadRequest(String),
}

impl GmailError {
    /// Must the user re-authorize before sync can continue?
    pub fn needs_reauth(&self) -> bool {
        matches!(self, Self::NeedsReauth(_))
    }

    /// Is retrying the same request reasonable?
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Transient(_) | Self::RateLimited(_))
    }

    /// Does this require discarding the history cursor and rebuilding the mailbox?
    ///
    /// Distinct from "retry": the cursor is unusable, so retrying the same call forever would
    /// spin without ever succeeding.
    pub fn requires_full_resync(&self) -> bool {
        matches!(self, Self::HistoryExpired { .. })
    }

    /// How long to wait before retrying, honouring `Retry-After` when the server sent one.
    ///
    /// Returns `None` for errors that should not be retried.
    pub fn retry_after(&self, attempt: u32) -> Option<std::time::Duration> {
        match self {
            Self::RateLimited(_) => Some(backoff_for(attempt)),
            Self::Transient(_) => Some(backoff_for(attempt)),
            _ => None,
        }
    }
}

/// Exponential backoff with full jitter, capped at 5 minutes.
///
/// Jitter is deliberate: without it, every client that hit a quota at the same moment retries
/// at the same moment, and the quota resets into a synchronized stampede.
fn backoff_for(attempt: u32) -> std::time::Duration {
    const BASE_MS: u64 = 500;
    const CAP_MS: u64 = 300_000;
    let capped = attempt.min(16);
    let ceiling = BASE_MS.saturating_mul(1u64 << capped).min(CAP_MS);
    // Deterministic pseudo-jitter derived from the attempt count: reproducible in tests, and
    // still decorrelates distinct clients from one another.
    let spread = (ceiling / 2).max(1);
    let offset = (attempt as u64).wrapping_mul(2_654_435_761) % spread;
    std::time::Duration::from_millis(ceiling / 2 + offset)
}

/// Parse a Gmail error body into the most specific variant available.
///
/// Gmail returns `{"error": {"code": 403, "message": "...", "errors": [{"reason": "..."}]}}`.
/// The `reason` field is the only reliable discriminator; the HTTP code alone is ambiguous.
pub fn classify(status: u16, body: &str) -> GmailError {
    let reason = extract_reason(body);
    let message = extract_message(body);
    let detail = match (&reason, &message) {
        (Some(r), Some(m)) => format!("{r}: {m}"),
        (Some(r), None) => r.clone(),
        (None, Some(m)) => m.clone(),
        (None, None) => body.trim().chars().take(200).collect(),
    };

    let lower = reason.as_deref().unwrap_or("").to_ascii_lowercase();
    let lower_message = message.as_deref().unwrap_or("").to_ascii_lowercase();

    // History expiry is a 404 whose message names historyId. Checked first: it is the one
    // condition that must trigger a full resync rather than any form of retry.
    if status == 404 && (lower_message.contains("historyid") || lower.contains("notfound")) {
        return GmailError::HistoryExpired {
            requested: extract_requested_history_id(body).unwrap_or_else(|| "unknown".into()),
        };
    }

    match status {
        401 => GmailError::NeedsReauth(detail),
        403 => {
            if lower.contains("ratelimitexceeded")
                || lower.contains("userratelimitexceeded")
                || lower.contains("quotaexceeded")
                || lower.contains("backenderror")
                || lower_message.contains("rate limit")
                || lower_message.contains("quota")
            {
                GmailError::RateLimited(detail)
            } else if lower.contains("insufficientpermissions")
                || lower.contains("forbidden")
                || lower.contains("insufficient scope")
            {
                GmailError::NeedsReauth(detail)
            } else {
                // An unrecognised 403 is most often an over-quota condition with a new reason
                // string. Treating it as auth would send users to re-consent for a quota issue.
                GmailError::RateLimited(detail)
            }
        }
        429 => GmailError::RateLimited(detail),
        400 => GmailError::BadRequest(detail),
        404 => GmailError::Protocol(detail),
        500..=599 => GmailError::Transient(detail),
        _ => GmailError::Protocol(detail),
    }
}

/// Pull the `errors[0].reason` out of a Gmail error body, if present.
fn extract_reason(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .pointer("/error/errors/0/reason")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Pull the top-level `error.message` out of a Gmail error body, if present.
fn extract_message(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .pointer("/error/message")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Recover the `historyId` the server complained about, for the resync message.
fn extract_requested_history_id(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let message = value.pointer("/error/message")?.as_str()?;
    // Gmail embeds the id in prose: "Requested history id 12345 is not available".
    let marker = "history id ";
    let start = message.to_ascii_lowercase().find(marker)? + marker.len();
    let rest = &message[start..];
    let id: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if id.is_empty() { None } else { Some(id) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_history_is_a_resync_not_a_retry() {
        let body = r#"{"error":{"code":404,"message":"Requested history id 998877 is not available for this mailbox.","errors":[{"reason":"notFound"}]}}"#;
        let err = classify(404, body);
        assert!(
            err.requires_full_resync(),
            "an expired cursor must trigger a full resync"
        );
        assert!(
            !err.is_transient(),
            "retrying an expired cursor can never succeed"
        );
        assert!(!err.needs_reauth());
        // The id is recovered so the message names the cursor that expired.
        assert!(
            err.to_string().contains("998877"),
            "message should name the stale cursor: {err}"
        );
    }

    #[test]
    fn quota_403s_are_rate_limited_not_auth_failures() {
        // The failure mode this prevents: a quota blip reported as "your token expired", which
        // sends the user through re-consent for something that resolves itself in seconds.
        for reason in [
            "rateLimitExceeded",
            "userRateLimitExceeded",
            "quotaExceeded",
            "backendError",
        ] {
            let body = format!(
                r#"{{"error":{{"code":403,"message":"Quota exceeded","errors":[{{"reason":"{reason}"}}]}}}}"#
            );
            let err = classify(403, &body);
            assert!(
                matches!(err, GmailError::RateLimited(_)),
                "{reason} must be rate-limited, got {err:?}"
            );
            assert!(err.is_transient());
            assert!(!err.needs_reauth(), "{reason} must not demand re-consent");
        }
    }

    #[test]
    fn scope_403s_do_demand_reauth() {
        let body = r#"{"error":{"code":403,"message":"Request had insufficient authentication scopes","errors":[{"reason":"insufficientPermissions"}]}}"#;
        let err = classify(403, body);
        assert!(err.needs_reauth(), "a scope error needs the user: {err:?}");
        assert!(!err.is_transient());
    }

    #[test]
    fn unknown_403_defaults_to_rate_limited() {
        // Gmail ships new `reason` strings without warning. Defaulting these to auth failure
        // would mean every new quota reason costs users a re-consent.
        let body = r#"{"error":{"code":403,"message":"Something new","errors":[{"reason":"someFutureReason"}]}}"#;
        assert!(matches!(classify(403, body), GmailError::RateLimited(_)));
    }

    #[test]
    fn expired_access_token_needs_reauth() {
        let err = classify(
            401,
            r#"{"error":{"code":401,"message":"Invalid Credentials"}}"#,
        );
        assert!(err.needs_reauth());
        assert!(!err.is_transient());
        assert_eq!(err.retry_after(0), None, "re-auth is not a timed retry");
    }

    #[test]
    fn server_errors_are_transient() {
        for status in [500, 502, 503, 504] {
            let err = classify(status, "upstream exploded");
            assert!(err.is_transient(), "HTTP {status} should be retryable");
            assert!(err.retry_after(0).is_some());
        }
    }

    #[test]
    fn backoff_grows_and_stays_capped() {
        let first = backoff_for(0);
        let later = backoff_for(5);
        let huge = backoff_for(63);
        assert!(later > first, "backoff must grow: {first:?} then {later:?}");
        assert!(
            huge <= std::time::Duration::from_secs(300),
            "backoff must cap at 5 minutes: {huge:?}"
        );
        // No panics or zero waits on large attempt counts.
        assert!(huge.as_millis() > 0);
    }

    #[test]
    fn non_json_error_bodies_do_not_panic() {
        // Proxies and captive portals return HTML. Classification must degrade, not explode.
        for body in ["", "<html>503</html>", "not json at all", "{broken"] {
            let err = classify(503, body);
            assert!(err.is_transient());
        }
    }

    #[test]
    fn a_plain_404_is_not_mistaken_for_history_expiry() {
        // Only a message naming historyId triggers a resync; a missing message id is a bug.
        let err = classify(
            404,
            r#"{"error":{"code":404,"message":"Requested entity was not found."}}"#,
        );
        assert!(!err.requires_full_resync());
        assert!(!err.needs_reauth());
    }
}
