//! Thread Snoozing & Inbox Reminder Queue §7 Phase 7
use ahash::AHashMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnoozedThread {
    pub thread_id: String,
    pub account_id: String,
    pub snoozed_at: DateTime<Utc>,
    pub wake_at: DateTime<Utc>,
    pub original_folder_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnoozeManager {
    pub snoozed: AHashMap<String, SnoozedThread>,
}

impl SnoozeManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snooze a thread until a designated future timestamp.
    ///
    /// Re-snoozing keeps the **earliest** wake time and the **original** restore target.
    /// The previous unconditional `insert` overwrote both, so snoozing thread `th_x` from
    /// folder A and again from folder B moved the thread to B on wake — the wrong folder.
    pub fn snooze(
        &mut self,
        thread_id: impl Into<String>,
        account_id: impl Into<String>,
        original_folder_id: impl Into<String>,
        wake_at: DateTime<Utc>,
    ) {
        let tid = thread_id.into();
        self.snoozed
            .entry(tid.clone())
            .and_modify(|existing| {
                existing.wake_at = existing.wake_at.min(wake_at);
                // original_folder_id / snoozed_at stay as first recorded.
            })
            .or_insert(SnoozedThread {
                thread_id: tid,
                account_id: account_id.into(),
                snoozed_at: Utc::now(),
                wake_at,
                original_folder_id: original_folder_id.into(),
            });
    }

    /// Unsnooze a thread manually
    pub fn unsnooze(&mut self, thread_id: &str) -> Option<SnoozedThread> {
        self.snoozed.remove(thread_id)
    }

    /// Threads that are due to wake, in wake order, **without removing them**.
    ///
    /// Peeking matters because the entry is the only record of where the thread came from.
    /// The previous `drain_waking_threads` removed the record before the caller performed
    /// the move, so a failed move or a process exit left the messages stranded with nothing
    /// left to restore them from. Call [`Self::complete_wake`] after a successful restore.
    pub fn due_waking_threads(&self, now: DateTime<Utc>) -> Vec<SnoozedThread> {
        let mut due: Vec<&SnoozedThread> =
            self.snoozed.values().filter(|t| t.wake_at <= now).collect();
        due.sort_by_key(|t| t.wake_at);
        due.into_iter().cloned().collect()
    }

    /// Forget a snooze after the thread has been restored successfully.
    pub fn complete_wake(&mut self, thread_id: &str) {
        self.snoozed.remove(thread_id);
    }

    /// Threads due to wake, removed. Retained for callers that restore immediately and
    /// cannot fail.
    pub fn drain_waking_threads(&mut self, now: DateTime<Utc>) -> Vec<SnoozedThread> {
        let due = self.due_waking_threads(now);
        for t in &due {
            self.snoozed.remove(&t.thread_id);
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_snooze_lifecycle() {
        let mut manager = SnoozeManager::new();
        let now = Utc::now();
        let tomorrow = now + Duration::days(1);

        manager.snooze("thread-101", "acc-1", "folder-inbox", tomorrow);
        assert_eq!(manager.snoozed.len(), 1);

        // Before wake time
        let waking = manager.drain_waking_threads(now + Duration::hours(12));
        assert!(waking.is_empty());

        // After wake time
        let waking = manager.drain_waking_threads(now + Duration::days(2));
        assert_eq!(waking.len(), 1);
        assert_eq!(waking[0].thread_id, "thread-101");
        assert_eq!(waking[0].original_folder_id, "folder-inbox");
        assert_eq!(manager.snoozed.len(), 0);
    }

    /// Regression: re-snoozing overwrote `original_folder_id`, so a thread snoozed from A and
    /// then B was restored to B.
    #[test]
    fn re_snoozing_keeps_the_original_folder_and_earliest_wake() {
        let mut m = SnoozeManager::new();
        let now = Utc::now();
        m.snooze("th_x", "acc", "folder-A", now + Duration::hours(2));
        m.snooze("th_x", "acc", "folder-B", now + Duration::hours(5));

        let s = m.snoozed.get("th_x").unwrap();
        assert_eq!(
            s.original_folder_id, "folder-A",
            "restore target was overwritten"
        );
        assert_eq!(
            s.wake_at,
            now + Duration::hours(2),
            "wake time should stay at the earliest"
        );

        // A shorter re-snooze shortens it, but still restores to A.
        m.snooze("th_x", "acc", "folder-C", now + Duration::hours(1));
        let s = m.snoozed.get("th_x").unwrap();
        assert_eq!(s.wake_at, now + Duration::hours(1));
        assert_eq!(s.original_folder_id, "folder-A");
    }

    /// The snooze record is the only record of where a thread came from, so peeking must not
    /// destroy it: a failed restore has to be retryable.
    #[test]
    fn due_waking_threads_peeks_without_removing() {
        let mut m = SnoozeManager::new();
        let now = Utc::now();
        m.snooze("th_y", "acc", "folder-A", now + Duration::minutes(1));

        let due = m.due_waking_threads(now + Duration::hours(1));
        assert_eq!(due.len(), 1);
        assert_eq!(m.snoozed.len(), 1, "peek must not remove the record");
        // A retry sees the same restore target.
        assert_eq!(
            m.due_waking_threads(now + Duration::hours(1))[0].original_folder_id,
            "folder-A"
        );

        m.complete_wake("th_y");
        assert!(m.snoozed.is_empty());
    }
}
