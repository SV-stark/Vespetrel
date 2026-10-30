//! The history cursor: Gmail's delta-sync position, and the rules for advancing it.
//!
//! # Why this is subtle
//!
//! Gmail's `users.history.list` is a paged feed. Each response carries a `historyId` and,
//! until the last page, a `nextPageToken`. The trap is that **the `historyId` on an
//! intermediate page must not be treated as the new cursor**: pages after it have not been
//! fetched yet, so persisting it and resuming from there silently drops everything in the
//! remaining pages. Mail disappears and nothing reports an error.
//!
//! The rules this module enforces:
//!
//! * While a walk is in progress (`page_token` is `Some`), the start `history_id` is held
//!   fixed and the token advances. Resuming passes both, which is what Gmail expects.
//! * Only when a page arrives with no `nextPageToken` — the walk is complete — does the cursor
//!   advance to that page's `history_id`, and the token clears.
//! * The cursor is advanced by [`HistoryCursor::advance`], which is called *after* the caller
//!   has durably applied the page's effects. Advancing first and crashing afterwards loses
//!   mail permanently; the ordering is the caller's responsibility and is asserted by the
//!   doc comment there.

use serde::{Deserialize, Serialize};

/// Position in Gmail's change history for one mailbox.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryCursor {
    /// `historyId` the next `users.history.list` call starts from.
    ///
    /// `None` means no baseline has been taken: the mailbox must be fully synced before any
    /// cursor can be recorded, otherwise changes that land during the full sync are lost.
    pub history_id: Option<String>,
    /// Continuation token for a walk that has not finished.
    pub page_token: Option<String>,
}

impl HistoryCursor {
    /// A cursor positioned at the mailbox's current head, with no walk in progress.
    ///
    /// This is the baseline taken *after* a full sync completes.
    pub fn baseline(history_id: impl Into<String>) -> Self {
        Self {
            history_id: Some(history_id.into()),
            page_token: None,
        }
    }

    /// Has a baseline been recorded?
    ///
    /// When `false` the caller must run a full sync rather than a history walk.
    pub fn is_initialized(&self) -> bool {
        self.history_id.is_some()
    }

    /// Is a multi-page walk in progress?
    pub fn is_walking(&self) -> bool {
        self.page_token.is_some()
    }

    /// The `(startHistoryId, pageToken)` pair to send with the next request.
    ///
    /// `Ok(None)` means there is no cursor to walk from and a full sync is required.
    pub fn request_params(&self) -> Option<(&str, Option<&str>)> {
        self.history_id
            .as_deref()
            .map(|id| (id, self.page_token.as_deref()))
    }

    /// Advance the cursor after a page has been applied.
    ///
    /// # Ordering requirement
    ///
    /// Call this **after** the page's inserts, updates and deletions are committed. Advancing
    /// first means a crash in between permanently skips those changes: the next poll starts
    /// past them and Gmail has no way to tell they were never delivered.
    ///
    /// A page with a `next_page_token` leaves `history_id` untouched and stores the token, so
    /// the remaining pages are still reachable. Only a final page moves the cursor.
    pub fn advance(&mut self, page_history_id: &str, next_page_token: Option<&str>) {
        match next_page_token {
            Some(token) if !token.is_empty() => {
                // Walk continues. Deliberately does not touch `history_id`.
                self.page_token = Some(token.to_string());
            }
            _ => {
                // Walk complete: this page's historyId is the new position.
                self.history_id = Some(page_history_id.to_string());
                self.page_token = None;
            }
        }
    }

    /// Discard the cursor, forcing a full sync on the next attempt.
    ///
    /// Called when Gmail reports the history is no longer retained, or when the local database
    /// is detected as inconsistent with the remote.
    pub fn invalidate(&mut self) {
        *self = Self::default();
    }
}

/// One page of `users.history.list`, reduced to what the cursor needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPage {
    /// The `historyId` covering this page. On an intermediate page this is *not* the new cursor.
    pub history_id: String,
    /// Token for the following page, or `None` when this was the last one.
    pub next_page_token: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_uninitialised_cursor_refuses_to_build_a_request() {
        let cursor = HistoryCursor::default();
        assert!(!cursor.is_initialized());
        assert!(
            cursor.request_params().is_none(),
            "without a baseline the caller must full-sync, not walk history"
        );
    }

    #[test]
    fn a_baseline_walks_from_the_recorded_id() {
        let cursor = HistoryCursor::baseline("1000");
        assert!(cursor.is_initialized());
        assert!(!cursor.is_walking());
        assert_eq!(cursor.request_params(), Some(("1000", None)));
    }

    /// The bug this whole module exists to prevent.
    #[test]
    fn an_intermediate_page_does_not_advance_the_history_id() {
        let mut cursor = HistoryCursor::baseline("1000");

        cursor.advance("1010", Some("tok-abc"));

        assert_eq!(
            cursor.history_id.as_deref(),
            Some("1000"),
            "advancing to an intermediate page's historyId would skip the remaining pages"
        );
        assert_eq!(cursor.page_token.as_deref(), Some("tok-abc"));
        assert!(cursor.is_walking());
        // Resuming must pass both, or Gmail restarts the walk and re-delivers everything.
        assert_eq!(
            cursor.request_params(),
            Some(("1000", Some("tok-abc"))),
            "resume must keep the original start id and carry the token"
        );
    }

    #[test]
    fn the_final_page_advances_the_cursor_and_clears_the_token() {
        let mut cursor = HistoryCursor::baseline("1000");
        cursor.advance("1010", Some("tok-abc"));
        cursor.advance("1025", None);

        assert_eq!(cursor.history_id.as_deref(), Some("1025"));
        assert_eq!(cursor.page_token, None);
        assert!(!cursor.is_walking());
        assert_eq!(cursor.request_params(), Some(("1025", None)));
    }

    #[test]
    fn an_empty_token_string_is_treated_as_the_end_of_the_walk() {
        // Gmail can return `"nextPageToken": ""`. Treating that as a live token would wedge the
        // cursor permanently, re-requesting an empty page token forever.
        let mut cursor = HistoryCursor::baseline("1000");
        cursor.advance("1001", Some(""));
        assert_eq!(cursor.page_token, None);
        assert_eq!(cursor.history_id.as_deref(), Some("1001"));
    }

    #[test]
    fn a_multi_page_walk_never_skips_a_page() {
        // Simulate the whole walk and assert every page id is reachable from the cursor the
        // previous step would have persisted.
        let pages = [
            ("1010", Some("t1")),
            ("1020", Some("t2")),
            ("1030", Some("t3")),
            ("1040", None),
        ];

        let mut cursor = HistoryCursor::baseline("1000");
        let mut persisted: Vec<HistoryCursor> = Vec::new();

        for (history_id, token) in pages {
            // Persist-after-apply is what a crash-and-restart would observe: the page just
            // fetched has been committed, and the cursor reflects exactly that.
            cursor.advance(history_id, token);
            persisted.push(cursor.clone());
        }

        // The final cursor points at the last page.
        assert_eq!(cursor.history_id.as_deref(), Some("1040"));
        // While a walk is in progress the history id never moves: each snapshot must equal the one
        // before it except for the page token, so no restart can skip an unfetched page.
        for (index, snapshot) in persisted.iter().enumerate() {
            if snapshot.is_walking() {
                assert_eq!(
                    snapshot.history_id.as_deref(),
                    Some("1000"),
                    "persisted cursor at step {index} advanced past unfetched pages"
                );
            }
        }
        // The final snapshot is the only one that carries a moved history id.
        let last = persisted.last().expect("at least one page");
        assert_eq!(last.history_id.as_deref(), Some("1040"));
        assert_eq!(last.page_token, None);
    }

    #[test]
    fn invalidate_clears_both_halves() {
        let mut cursor = HistoryCursor::baseline("1000");
        cursor.advance("1010", Some("tok"));
        cursor.invalidate();
        assert!(!cursor.is_initialized());
        assert!(!cursor.is_walking());
        assert_eq!(cursor.request_params(), None);
    }

    #[test]
    fn cursor_round_trips_through_json() {
        // Persisted in `SyncState`, so a restart must resume mid-walk rather than restarting.
        let mut cursor = HistoryCursor::baseline("1000");
        cursor.advance("1010", Some("tok-abc"));
        let encoded = serde_json::to_string(&cursor).expect("encode");
        let decoded: HistoryCursor = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(decoded, cursor);
    }
}
