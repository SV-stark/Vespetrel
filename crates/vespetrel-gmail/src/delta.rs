//! Folding Gmail history pages into a net set of changes.
//!
//! Gmail's history feed reports *events*, not *state*. Within a single page the same message
//! can appear as added and then deleted, or have a label added and removed, and the same
//! message can recur across pages. Accumulating events directly therefore produces phantom
//! messages and redundant fetches — a message that was added and deleted inside one page would
//! be inserted, then deleted, costing two fetches and leaving a row behind if the delete is
//! processed before the insert.
//!
//! [`DeltaAccumulator`] folds events into net state instead: which messages to fetch, which
//! labels to add and remove, and which are gone.

use std::collections::{BTreeMap, BTreeSet};

/// Net effect of a set of history pages.
///
/// `BTreeMap`/`BTreeSet` rather than hash containers so that output is deterministic. Sync
/// ordering must be reproducible: a non-deterministic plan makes failures unreproducible and
/// makes idempotency impossible to reason about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryDelta {
    /// Messages whose content must be fetched: added, or relabelled.
    pub fetch: BTreeSet<String>,
    /// Messages confirmed gone from the mailbox.
    pub deleted: BTreeSet<String>,
    /// Net label changes per message: `(message_id, added, removed)`.
    pub label_changes: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)>,
    /// Messages Gmail reported as added and then deleted inside the same fold.
    ///
    /// Dropped entirely rather than inserted-then-deleted.
    pub added_and_deleted: BTreeSet<String>,
}

impl HistoryDelta {
    /// Total messages this delta would fetch.
    pub fn fetch_count(&self) -> usize {
        self.fetch.len()
    }

    /// Is there nothing to do?
    pub fn is_empty(&self) -> bool {
        self.fetch.is_empty() && self.deleted.is_empty() && self.label_changes.is_empty()
    }

    /// Reconcile the two label sets for one message.
    ///
    /// A label present in both is a no-op, not an add-then-remove. That happens legitimately:
    /// Gmail can report a label being added and removed within one fold when a filter moves a
    /// message between labels.
    fn record_label_change(&mut self, message_id: &str, added: &[String], removed: &[String]) {
        if added.is_empty() && removed.is_empty() {
            return;
        }
        let entry = self
            .label_changes
            .entry(message_id.to_string())
            .or_insert_with(|| (BTreeSet::new(), BTreeSet::new()));

        // Resolve each label to its *last* direction within this fold, then report both sets.
        //
        // Reporting removals is correct and necessary: marking a message read is exactly a
        // `UNREAD` removal, and suppressing it would leave read state permanently stale.
        //
        // The intermediate churn case (added then removed, or vice versa) resolves to whichever
        // came last, which is the message's actual net state. Applying it is safe even when the
        // label was never present, because `batchModify` sets and unsets labels idempotently —
        // a no-op against a label the message does not carry.
        for label in added {
            entry.1.remove(label);
            entry.0.insert(label.clone());
        }
        for label in removed {
            entry.0.remove(label);
            entry.1.insert(label.clone());
        }

        // Drop the entry entirely once it nets to nothing, so a fold of pure churn is a no-op.
        if entry.0.is_empty() && entry.1.is_empty() {
            self.label_changes.remove(message_id);
        }
    }

    /// Resolve the accumulated events into a final, self-consistent plan.
    ///
    /// A message that was added and then deleted never needs fetching: the delete wins, and the
    /// message is left absent. Doing it the other way round costs a fetch and risks leaving a
    /// row behind if the two are processed out of order.
    pub fn resolve(self) -> ResolvedDelta {
        let mut plan = ResolvedDelta {
            fetch: BTreeSet::new(),
            deleted: BTreeSet::new(),
            label_changes: BTreeMap::new(),
        };

        // Ids that came and went inside this fold: never stored, so nothing to fetch or delete.
        for message_id in &self.added_and_deleted {
            plan.fetch.remove(message_id);
            plan.deleted.remove(message_id);
            plan.label_changes.remove(message_id);
        }

        for message_id in self.deleted {
            // A deletion supersedes any pending fetch of the same message.
            plan.fetch.remove(&message_id);
            plan.deleted.insert(message_id);
        }

        // An id in both `deleted` and `added_and_deleted` was deleted and then re-added, so it
        // exists now and must be fetched. The re-add already removed it from `deleted`.
        for message_id in self.fetch {
            if plan.deleted.contains(&message_id) {
                continue;
            }
            plan.fetch.insert(message_id);
        }

        for (message_id, (added, removed)) in self.label_changes {
            // A message that is gone — deleted outright, or added-then-deleted inside this
            // fold — has no labels worth reconciling. Applying them would either resurrect a
            // deleted message or write rows for one that never existed.
            if plan.deleted.contains(&message_id) || self.added_and_deleted.contains(&message_id) {
                continue;
            }
            if added.is_empty() && removed.is_empty() {
                continue;
            }
            plan.label_changes.insert(message_id, (added, removed));
        }

        plan
    }
}

/// A [`HistoryDelta`] after cancellation, ready to be applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedDelta {
    pub fetch: BTreeSet<String>,
    pub deleted: BTreeSet<String>,
    pub label_changes: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)>,
}

impl ResolvedDelta {
    pub fn is_empty(&self) -> bool {
        self.fetch.is_empty() && self.deleted.is_empty() && self.label_changes.is_empty()
    }

    /// Messages needing a network fetch, after label-only changes are excluded.
    ///
    /// A relabelled message already in the database needs no refetch — only its labels changed.
    /// Refetching the full RFC822 for every label change is the single biggest source of wasted
    /// Gmail quota.
    pub fn fetch_set(&self, already_stored: &BTreeSet<String>) -> BTreeSet<String> {
        let mut needed: BTreeSet<String> = self
            .fetch
            .difference(&self.label_only_ids())
            .cloned()
            .collect();
        // Anything already in the database only needs its flags reconciled, not re-fetched.
        needed = needed.difference(already_stored).cloned().collect();
        needed
    }

    /// Ids that appear *only* in `label_changes` and nowhere in `fetch`.
    fn label_only_ids(&self) -> BTreeSet<String> {
        self.label_changes
            .keys()
            .filter(|id| !self.fetch.contains(*id))
            .cloned()
            .collect()
    }
}

/// One `history` entry from `users.history.list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct HistoryEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub messages_added: Vec<MessageEvent>,
    #[serde(default)]
    pub messages_deleted: Vec<MessageEvent>,
    #[serde(default)]
    pub labels_added: Vec<LabelEvent>,
    #[serde(default)]
    pub labels_removed: Vec<LabelEvent>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct MessageEvent {
    #[serde(default)]
    pub message: MessageRef,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct MessageRef {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub thread_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct LabelEvent {
    #[serde(default)]
    pub message: MessageRef,
    #[serde(default)]
    pub label_ids: Vec<String>,
}

/// Folds history entries into a net [`HistoryDelta`].
///
/// Accumulates across pages so that an add on page 1 and a delete on page 3 cancel correctly.
#[derive(Debug, Default)]
pub struct DeltaAccumulator {
    delta: HistoryDelta,
    /// Ids seen in `messages_added` *and not yet cancelled*, so a later delete can be recognised
    /// as cancelling an addition rather than deleting something that only ever existed remotely.
    ///
    /// Removing the id on cancellation is what makes the fold order-independent: once an
    /// add/delete pair has cancelled, a subsequent event for the same id is judged on its own
    /// rather than inheriting the earlier one.
    pending_adds: BTreeSet<String>,
}

impl DeltaAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one page's entries.
    pub fn push_entry(&mut self, entry: &HistoryEntry) {
        for event in &entry.messages_added {
            if event.message.id.is_empty() {
                continue;
            }
            let id = event.message.id.clone();
            // Re-adding after a delete means it exists now. Clear the deletion and treat this
            // as a fresh addition needing a fetch, so the fold's result does not depend on the
            // order Gmail happened to report the two events in.
            self.delta.deleted.remove(&id);
            self.pending_adds.insert(id.clone());
            self.delta.fetch.insert(id);
        }

        for event in &entry.messages_deleted {
            if event.message.id.is_empty() {
                continue;
            }
            let id = event.message.id.clone();
            self.delta.fetch.remove(&id);
            if self.pending_adds.remove(&id) {
                // Added and deleted within this fold without ever having been stored locally:
                // neither fetching nor deleting is right, it simply never existed as far as this
                // mailbox is concerned.
                self.delta.deleted.remove(&id);
                self.delta.added_and_deleted.insert(id);
                continue;
            }
            self.delta.deleted.insert(id);
        }

        // Within a single entry, a label appearing in both directions is a no-op: Gmail reports it
        // when a filter both adds and removes a label in the same change. Cancellation here
        // (rather than "later wins") is what makes it disappear instead of being applied as a
        // removal. Across separate entries, ordering is real and `record_label_change` resolves
        // it by letting the later event win.
        let intra_added: BTreeSet<&String> = entry
            .labels_added
            .iter()
            .flat_map(|e| e.label_ids.iter())
            .collect();
        let intra_removed: BTreeSet<&String> = entry
            .labels_removed
            .iter()
            .flat_map(|e| e.label_ids.iter())
            .collect();
        let churned: BTreeSet<&String> =
            intra_added.intersection(&intra_removed).copied().collect();

        for event in &entry.labels_added {
            if event.message.id.is_empty() || event.label_ids.is_empty() {
                continue;
            }
            let effective: Vec<String> = event
                .label_ids
                .iter()
                .filter(|l| !churned.contains(*l))
                .cloned()
                .collect();
            self.delta
                .record_label_change(&event.message.id, &effective, &[]);
        }

        for event in &entry.labels_removed {
            if event.message.id.is_empty() || event.label_ids.is_empty() {
                continue;
            }
            let effective: Vec<String> = event
                .label_ids
                .iter()
                .filter(|l| !churned.contains(*l))
                .cloned()
                .collect();
            self.delta
                .record_label_change(&event.message.id, &[], &effective);
        }
    }

    pub fn push_page(&mut self, entries: &[HistoryEntry]) {
        for entry in entries {
            self.push_entry(entry);
        }
    }

    /// Finish, cancelling added-then-deleted pairs.
    pub fn resolve(self) -> ResolvedDelta {
        self.delta.resolve()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str) -> MessageEvent {
        MessageEvent {
            message: MessageRef {
                id: id.into(),
                thread_id: "t1".into(),
            },
        }
    }

    fn label_event(id: &str, labels: &[&str]) -> LabelEvent {
        LabelEvent {
            message: MessageRef {
                id: id.into(),
                thread_id: "t1".into(),
            },
            label_ids: labels.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_plain_addition_needs_one_fetch() {
        let mut acc = DeltaAccumulator::new();
        acc.push_entry(&HistoryEntry {
            id: "1".into(),
            messages_added: vec![msg("m1")],
            ..Default::default()
        });
        let plan = acc.resolve();
        assert!(plan.fetch.contains("m1"));
        assert!(plan.deleted.is_empty());
        assert!(plan.label_changes.is_empty());
    }

    /// The case that produces phantom rows if events are applied in order.
    #[test]
    fn added_then_deleted_in_one_page_is_never_fetched() {
        let mut acc = DeltaAccumulator::new();
        acc.push_entry(&HistoryEntry {
            id: "1".into(),
            messages_added: vec![msg("m1")],
            ..Default::default()
        });
        acc.push_entry(&HistoryEntry {
            id: "2".into(),
            messages_deleted: vec![msg("m1")],
            ..Default::default()
        });

        let plan = acc.resolve();
        assert!(
            !plan.fetch.contains("m1"),
            "a message deleted inside the fold must not be fetched"
        );
        assert!(
            plan.deleted.is_empty(),
            "it never existed locally, so there is nothing to delete"
        );
    }

    #[test]
    fn a_pure_deletion_is_reported() {
        let mut acc = DeltaAccumulator::new();
        acc.push_entry(&HistoryEntry {
            id: "1".into(),
            messages_deleted: vec![msg("m9")],
            ..Default::default()
        });
        let plan = acc.resolve();
        assert!(plan.deleted.contains("m9"));
        assert!(plan.fetch.is_empty());
    }

    /// Delete-then-add nets to a live message: it exists now and must be fetched.
    ///
    /// Gmail can report the events in either order, so both must agree — otherwise the same
    /// mailbox state syncs differently depending on which page carried which event.
    #[test]
    fn deletion_then_addition_yields_a_fetch() {
        let mut acc = DeltaAccumulator::new();
        acc.push_entry(&HistoryEntry {
            id: "1".into(),
            messages_deleted: vec![msg("m1")],
            ..Default::default()
        });
        acc.push_entry(&HistoryEntry {
            id: "2".into(),
            messages_added: vec![msg("m1")],
            ..Default::default()
        });
        let plan = acc.resolve();
        assert!(
            plan.fetch.contains("m1"),
            "a message deleted then re-added exists now and must be fetched"
        );
        assert!(
            plan.deleted.is_empty(),
            "the re-add supersedes the deletion; reporting both would delete a fetched message"
        );
    }

    #[test]
    fn duplicate_events_across_pages_are_deduplicated() {
        let mut acc = DeltaAccumulator::new();
        for id in ["1", "2", "3"] {
            acc.push_entry(&HistoryEntry {
                id: id.into(),
                messages_added: vec![msg("m1"), msg("m2")],
                ..Default::default()
            });
        }
        let plan = acc.resolve();
        assert_eq!(
            plan.fetch.len(),
            2,
            "repeated events must not multiply work"
        );
    }

    /// Add-then-remove nets to a removal, which is the message's actual end state.
    ///
    /// This is not a spurious call: the history feed cannot say whether `STARRED` was present
    /// before this fold, so the only correct claim is "it is not there now". `batchModify`
    /// unsets a label idempotently, so applying it against a message that never carried the label
    /// is harmless — whereas suppressing it would leave a genuinely unstarred message starred
    /// forever.
    #[test]
    fn a_label_added_then_removed_nets_to_a_removal() {
        let mut acc = DeltaAccumulator::new();
        acc.push_entry(&HistoryEntry {
            id: "1".into(),
            labels_added: vec![label_event("m1", &["STARRED"])],
            ..Default::default()
        });
        acc.push_entry(&HistoryEntry {
            id: "2".into(),
            labels_removed: vec![label_event("m1", &["STARRED"])],
            ..Default::default()
        });
        let plan = acc.resolve();
        let (added, removed) = plan
            .label_changes
            .get("m1")
            .expect("the net removal must be reported");
        assert!(added.is_empty(), "net state is unstarred");
        assert!(removed.contains("STARRED"));
    }

    #[test]
    fn a_genuine_label_change_survives() {
        let mut acc = DeltaAccumulator::new();
        acc.push_entry(&HistoryEntry {
            id: "1".into(),
            labels_added: vec![label_event("m1", &["STARRED"])],
            labels_removed: vec![label_event("m1", &["UNREAD"])],
            ..Default::default()
        });
        let plan = acc.resolve();
        let (added, removed) = plan.label_changes.get("m1").expect("label change recorded");
        assert!(added.contains("STARRED"));
        assert!(removed.contains("UNREAD"));
    }

    /// A label present in both directions cancels rather than becoming add-then-remove.
    #[test]
    fn a_label_in_both_directions_cancels() {
        let mut acc = DeltaAccumulator::new();
        acc.push_entry(&HistoryEntry {
            id: "1".into(),
            labels_added: vec![label_event("m1", &["IMPORTANT"])],
            labels_removed: vec![label_event("m1", &["IMPORTANT"])],
            ..Default::default()
        });
        assert!(acc.resolve().label_changes.is_empty());
    }

    /// The main quota saving: relabelling must not trigger an RFC822 refetch.
    #[test]
    fn a_label_change_does_not_trigger_a_fetch_for_a_known_message() {
        let mut acc = DeltaAccumulator::new();
        acc.push_entry(&HistoryEntry {
            id: "1".into(),
            labels_added: vec![label_event("m1", &["\u{2605}"])],
            ..Default::default()
        });
        let plan = acc.resolve();

        let stored: BTreeSet<String> = ["m1".to_string()].into_iter().collect();
        assert!(
            plan.fetch_set(&stored).is_empty(),
            "refetching a whole message for a label change wastes Gmail quota"
        );
    }

    #[test]
    fn a_message_being_fetched_and_relabelled_still_fetches_once() {
        let mut acc = DeltaAccumulator::new();
        acc.push_entry(&HistoryEntry {
            id: "1".into(),
            messages_added: vec![msg("m1")],
            labels_added: vec![label_event("m1", &["STARRED"])],
            ..Default::default()
        });
        let plan = acc.resolve();
        assert_eq!(plan.fetch_set(&BTreeSet::new()).len(), 1);
        assert!(plan.label_changes.contains_key("m1"));
    }

    #[test]
    fn changes_for_deleted_messages_are_dropped() {
        let mut acc = DeltaAccumulator::new();
        acc.push_entry(&HistoryEntry {
            id: "1".into(),
            messages_added: vec![msg("m1")],
            ..Default::default()
        });
        acc.push_entry(&HistoryEntry {
            id: "2".into(),
            messages_deleted: vec![msg("m1")],
            labels_removed: vec![label_event("m1", &["INBOX"])],
            ..Default::default()
        });
        let plan = acc.resolve();
        assert!(plan.fetch.is_empty());
        assert!(plan.deleted.is_empty());
        assert!(
            plan.label_changes.is_empty(),
            "labelling a message that no longer exists is meaningless"
        );
    }

    #[test]
    fn an_empty_page_yields_an_empty_plan() {
        let acc = DeltaAccumulator::new();
        assert!(acc.resolve().is_empty());
    }

    #[test]
    fn events_with_empty_ids_are_ignored() {
        // Defensive: a malformed entry must not become a phantom message keyed on "".
        let mut acc = DeltaAccumulator::new();
        acc.push_entry(&HistoryEntry {
            id: "1".into(),
            messages_added: vec![msg("")],
            labels_added: vec![label_event("", &["INBOX"])],
            ..Default::default()
        });
        let plan = acc.resolve();
        assert!(plan.fetch.is_empty());
        assert!(plan.label_changes.is_empty());
    }
}
