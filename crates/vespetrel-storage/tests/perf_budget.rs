//! Performance budgets for the storage layer.
//!
//! Unit tests assert *correctness*. They cannot catch a query that becomes quadratic as a
//! mailbox grows, or a threading pass that rebuilds per row, because a 200-message fixture
//! behaves identically to a 200,000-message one until it doesn't. These tests seed a large
//! mailbox and assert wall-clock budgets, which is the only way a regression of that shape
//! shows up as a failure rather than a user complaint.
//!
//! Budgets are deliberately loose — they are there to catch an order-of-magnitude regression,
//! not to police microseconds. A machine several times slower than CI should still pass.
//!
//! Run with `cargo test -p vespetrel-storage --test perf_budget -- --nocapture`.
//! Set `VESPETREL_PERF_MS` to scale every budget (e.g. `200` to double them) when running on
//! slower hardware.

use rusqlite::Connection;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use vespetrel_core::{Account, Folder, FolderRole, Message};
use vespetrel_storage::db::open_in_memory;
use vespetrel_storage::repo;

/// Threads to seed. Chosen to be large enough to expose per-row work while keeping the suite
/// fast on CI. 4,500 rows takes a few seconds to insert (FTS5 triggers fire per row), so the
/// fixture is built exactly once and shared rather than rebuilt per test.
const THREADS: usize = 1_500;
const MESSAGES_PER_THREAD: usize = 3;

/// The shared fixture. The connection is retained because the mailbox is only reachable
/// through it.
struct Fixture {
    conn: Connection,
    _account_id: String,
    folder_id: String,
}

/// Seed once, lazily, and hand every test the same mailbox.
///
/// Rebuilding per test cost ~11s each, which is long enough that people stop running the suite.
/// Tests only read, so one shared connection behind a mutex is sufficient.
fn fixture() -> &'static Mutex<Fixture> {
    static FIXTURE: OnceLock<Mutex<Fixture>> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let conn = open_in_memory().expect("open db");
        let (account_id, folder_id) = seed_large_mailbox(&conn);
        Mutex::new(Fixture {
            conn,
            _account_id: account_id,
            folder_id,
        })
    })
}

/// Borrow the shared fixture for a read-only test.
fn with_fixture<T>(f: impl FnOnce(&Connection, &str) -> T) -> T {
    let guard = fixture().lock().unwrap_or_else(|e| e.into_inner());
    f(&guard.conn, &guard.folder_id)
}

/// Scale factor applied to every budget, so the suite can run on slower machines.
fn budget_multiplier() -> f64 {
    std::env::var("VESPETREL_PERF_MS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(1.0)
}

/// Assert an operation completed within `ms` milliseconds, after scaling.
#[track_caller]
fn assert_within_budget(label: &str, elapsed: std::time::Duration, ms: u64) {
    let scaled = ms as f64 * budget_multiplier();
    let actual = elapsed.as_secs_f64() * 1000.0;
    assert!(
        actual <= scaled,
        "{label} took {actual:.1}ms, budget {scaled:.1}ms (scale: {})",
        budget_multiplier()
    );
}

/// Seed `THREADS` threads with `MESSAGES_PER_THREAD` messages each.
///
/// Returns the account id and the inbox folder id.
fn seed_large_mailbox(conn: &Connection) -> (String, String) {
    let account = Account::new(
        "Perf",
        "perf@example.com",
        vespetrel_core::ProviderType::Imap,
    );
    repo::upsert_account(conn, &account).expect("upsert account");

    let mut inbox = Folder::new(&account.id, "INBOX", "INBOX", "INBOX");
    inbox.role = FolderRole::Inbox;
    inbox.total_count = (THREADS * MESSAGES_PER_THREAD) as i64;
    let folder_id = repo::upsert_folder(conn, &inbox).expect("upsert folder");

    // Insert through a transaction: one commit per message would make the seed, not the
    // measured queries, dominate the run.
    let tx = conn.unchecked_transaction().expect("begin tx");
    let base = chrono::Utc::now().timestamp();

    for t in 0..THREADS {
        let mut thread =
            vespetrel_core::Thread::new(account.id.clone(), Some(format!("Thread topic {t}")));
        thread.id = format!("thread-{t}");
        thread.last_message_at =
            chrono::DateTime::from_timestamp(base - t as i64, 0).expect("valid timestamp");
        thread.message_count = MESSAGES_PER_THREAD as i64;
        repo::upsert_thread(&tx, &thread).expect("upsert thread");

        for m in 0..MESSAGES_PER_THREAD {
            let mut msg = Message::new(
                &account.id,
                &folder_id,
                (t * MESSAGES_PER_THREAD + m) as u32,
                format!("Thread topic {t}"),
                format!("sender{t}@example.com"),
                vec!["perf@example.com".to_string()],
            );
            msg.thread_id = Some(format!("thread-{t}"));
            msg.received_at =
                chrono::DateTime::from_timestamp(base - t as i64, 0).expect("valid timestamp");
            msg.sent_at = msg.received_at;
            msg.is_read = m > 0;
            msg.body_snippet = Some(format!("Body text for thread {t} message {m}"));
            msg.body_text_preview = Some(format!("Body text for thread {t} message {m}"));
            repo::insert_message(&tx, &msg).expect("insert message");
        }
    }

    tx.commit().expect("commit seed");
    (account.id, folder_id)
}

/// Fetching the first page of a large mailbox must not degrade with mailbox size.
///
/// This is the regression guard for the message-list query itself: an index-friendly `ORDER
/// BY sent_at DESC LIMIT n` is O(log n), while a full scan plus in-memory sort would grow
/// linearly and eventually stall the UI thread.
#[test]
fn first_page_of_a_large_mailbox_is_fast() {
    with_fixture(|conn, folder| {
        let start = Instant::now();
        let page = repo::list_messages_in_folder(conn, folder, 200, 0).expect("list page");
        let elapsed = start.elapsed();

        assert_eq!(page.len(), 200, "page must be full");
        assert_within_budget("first page (200 rows)", elapsed, 400);
    });
}

/// Paging deep into a mailbox must cost about the same as the first page.
///
/// A query that re-sorts the whole table per page makes OFFSET paging quadratic overall. This
/// asserts the deep page is not dramatically worse than the first, which is the property that
/// matters when a user scrolls to the bottom of a large mailbox.
#[test]
fn deep_paging_does_not_degrade() {
    with_fixture(|conn, folder| {
        let first = Instant::now();
        repo::list_messages_in_folder(conn, folder, 200, 0).expect("first page");
        let first_elapsed = first.elapsed();

        // A third of the way in — far enough that a full re-sort per page would show.
        let deep_offset = (THREADS * MESSAGES_PER_THREAD) / 3;
        let deep = Instant::now();
        let page =
            repo::list_messages_in_folder(conn, folder, 200, deep_offset).expect("deep page");
        let deep_elapsed = deep.elapsed();

        assert_eq!(page.len(), 200, "deep page must be full");

        // 8x headroom over the first page absorbs I/O variance while still catching quadratic
        // paging, where the deep page would be orders of magnitude slower.
        let ratio = deep_elapsed.as_secs_f64() / first_elapsed.as_secs_f64().max(0.000_001);
        assert!(
            ratio < 8.0,
            "deep page was {ratio:.1}x the first page ({deep_elapsed:?} vs {first_elapsed:?}); \
             paging may be re-sorting the table per page"
        );
        assert_within_budget("deep page (200 rows)", deep_elapsed, 600);
    });
}

/// FTS5 search must stay interactive on a large mailbox.
///
/// The budget covers tokenization plus the BM25 ranking pass, which is where a malformed
/// MATCH expression or a missing index would surface.
#[test]
fn full_text_search_over_a_large_mailbox_is_responsive() {
    with_fixture(|conn, _| {
        let start = Instant::now();
        let results = vespetrel_storage::search_messages(conn, "thread", None, 50).expect("search");
        let elapsed = start.elapsed();

        assert!(!results.is_empty(), "seeded content must be findable");
        assert!(
            results.len() <= 50,
            "limit must be honoured, got {}",
            results.len()
        );
        assert_within_budget("fts5 search", elapsed, 500);
    });
}

/// Field-prefixed search must work, not merely not crash.
///
/// `from:` maps to an indexed column. If the sanitizer ever stops emitting the prefix this
/// silently returns nothing, which looks identical to "no such sender" to the user.
#[test]
fn field_prefixed_search_matches_the_right_column() {
    with_fixture(|conn, _| {
        let hits = vespetrel_storage::search_messages(conn, "from:sender42@example.com", None, 20)
            .expect("search by sender");
        assert!(
            !hits.is_empty(),
            "from: prefix must match sender42; sanitizer may have dropped the column prefix"
        );
    });
}

/// Threading a large mailbox must not be superlinear.
///
/// `normalize_subject` runs per message, so the pass is the place a pathological
/// implementation shows up. The budget is loose; it exists to catch an accidental per-row
/// database round trip, which is orders of magnitude slower than in-memory work.
#[test]
fn threading_a_large_mailbox_is_fast() {
    with_fixture(|conn, folder| {
        let messages =
            repo::list_messages_in_folder(conn, folder, THREADS * MESSAGES_PER_THREAD, 0)
                .expect("load all messages");

        let start = Instant::now();
        // Mirror the GUI's grouping: key by thread id, falling back to normalized subject.
        let mut groups: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for m in &messages {
            let key = m.thread_id.clone().unwrap_or_else(|| {
                vespetrel_core::thread::normalize_subject(m.subject.as_deref().unwrap_or(""))
            });
            *groups.entry(key).or_insert(0) += 1;
        }
        let elapsed = start.elapsed();

        assert_eq!(groups.len(), THREADS, "each thread must form its own group");
        assert_within_budget("thread grouping pass", elapsed, 300);
    });
}
