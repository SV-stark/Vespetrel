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

    /// Snooze a thread until a designated future timestamp
    pub fn snooze(
        &mut self,
        thread_id: impl Into<String>,
        account_id: impl Into<String>,
        original_folder_id: impl Into<String>,
        wake_at: DateTime<Utc>,
    ) {
        let tid = thread_id.into();
        self.snoozed.insert(
            tid.clone(),
            SnoozedThread {
                thread_id: tid,
                account_id: account_id.into(),
                snoozed_at: Utc::now(),
                wake_at,
                original_folder_id: original_folder_id.into(),
            },
        );
    }

    /// Unsnooze a thread manually
    pub fn unsnooze(&mut self, thread_id: &str) -> Option<SnoozedThread> {
        self.snoozed.remove(thread_id)
    }

    /// Check and drain all threads that are due to wake back into the inbox
    pub fn drain_waking_threads(&mut self, now: DateTime<Utc>) -> Vec<SnoozedThread> {
        let mut waking_ids = Vec::new();
        for (tid, thread) in &self.snoozed {
            if thread.wake_at <= now {
                waking_ids.push(tid.clone());
            }
        }

        let mut waking = Vec::new();
        for tid in waking_ids {
            if let Some(t) = self.snoozed.remove(&tid) {
                waking.push(t);
            }
        }
        waking
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
}
