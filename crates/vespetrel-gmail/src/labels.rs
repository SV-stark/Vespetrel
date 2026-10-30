//! Gmail label metadata and its mapping onto Vespetrel's folder roles.
//!
//! Gmail's folder model is labels, not mailboxes: a message can carry `INBOX`, `UNREAD`,
//! `STARRED` and a user label simultaneously. Vespetrel's model expects one folder per role
//! with messages inside it, so the mapping is lossy by construction and must be chosen
//! deliberately.
//!
//! Two decisions matter:
//!
//! * **System labels** (`INBOX`, `SENT`, `DRAFT`, `SPAM`, `TRASH`, `ARCHIVE`, `STARRED`,
//!   `IMPORTANT`, `CATEGORY_*`) carry semantics and map to roles.
//! * **User labels** are a flat namespace with `/` as a visual grouping separator, *not* a
//!   hierarchy. Gmail itself does not nest them. Treating `Work/Clients` as a child of `Work`
//!   would invent structure the server never agreed to, so the full path is kept as the
//!   folder's `path` and the leaf is its `name`.

use serde::{Deserialize, Serialize};
use vespetrel_core::provider::RemoteFolder;

/// Largest label id or name accepted.
///
/// Label names come from the user and can be arbitrarily long; they end up in SQL bound
/// parameters and in the sync key. Bounding them keeps a pathological mailbox from producing
/// an unbounded allocation per label.
pub const MAX_LABEL_TEXT: usize = 255;

/// Gmail's system label for the inbox.
pub const LABEL_INBOX: &str = "INBOX";
pub const LABEL_SENT: &str = "SENT";
pub const LABEL_DRAFT: &str = "DRAFT";
pub const LABEL_SPAM: &str = "SPAM";
pub const LABEL_TRASH: &str = "TRASH";
pub const LABEL_ARCHIVE: &str = "ARCHIVE";
pub const LABEL_UNREAD: &str = "UNREAD";
pub const LABEL_STARRED: &str = "STARRED";
pub const LABEL_IMPORTANT: &str = "IMPORTANT";

/// One label as returned by `users.labels.list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GmailLabel {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// `system` or `user`.
    #[serde(default)]
    #[serde(rename = "type")]
    pub label_type: String,
    /// Number of unread messages, when Gmail reports it.
    #[serde(default)]
    pub messages_unread: Option<u32>,
    #[serde(default)]
    pub messages_total: Option<u32>,
}

/// Which Vespetrel folder role a label corresponds to.
///
/// Gmail exposes the special-use folders as system labels with stable ids, so this is a
/// lookup rather than a heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelRole {
    Inbox,
    Sent,
    Drafts,
    Spam,
    Trash,
    Archive,
    Custom,
}

/// Map a system label id to its role.
///
/// Returns `None` for anything that is not a folder-bearing system label — `UNREAD`,
/// `STARRED`, `IMPORTANT` and `CATEGORY_*` describe *state*, not location, and must not
/// become folders or a settings view would grow one entry per category.
pub fn role_for_system_label(label_id: &str) -> Option<LabelRole> {
    match label_id {
        LABEL_INBOX => Some(LabelRole::Inbox),
        LABEL_SENT => Some(LabelRole::Sent),
        LABEL_DRAFT => Some(LabelRole::Drafts),
        LABEL_SPAM => Some(LabelRole::Spam),
        LABEL_TRASH => Some(LabelRole::Trash),
        LABEL_ARCHIVE => Some(LabelRole::Archive),
        _ => None,
    }
}

/// A label that maps onto a folder Vespetrel should sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncableLabel {
    pub remote_id: String,
    pub name: String,
    pub path: String,
    pub role: LabelRole,
    pub unread_count: i64,
    pub total_count: i64,
}

/// Split a user label name into its display leaf and full path.
///
/// `Work/Clients` has leaf `Clients` and path `Work/Clients`. Gmail's `/` is decorative, so the
/// path is kept verbatim rather than expanded into nested folders the server does not model.
fn split_user_label(name: &str) -> (String, String) {
    let trimmed = name.trim();
    if let Some(idx) = trimmed.rfind('/') {
        let leaf = trimmed[idx + 1..].trim();
        if !leaf.is_empty() {
            return (leaf.to_string(), trimmed.to_string());
        }
    }
    (trimmed.to_string(), trimmed.to_string())
}

/// Decide which labels become folders, and with what role.
///
/// `INBOX` is always included even when Gmail's label list omits it, because a mailbox with no
/// inbox folder is unusable and the label is guaranteed to exist.
pub fn syncable_labels(labels: &[GmailLabel]) -> Vec<SyncableLabel> {
    let mut out: Vec<SyncableLabel> = Vec::new();

    for label in labels {
        if label.id.is_empty() {
            continue;
        }
        let name = label.name.trim();
        if name.is_empty() || name.chars().count() > MAX_LABEL_TEXT {
            continue;
        }

        let role = match role_for_system_label(&label.id) {
            Some(role) => role,
            None if label.label_type == "user" => LabelRole::Custom,
            // A system label with no folder meaning (UNREAD, STARRED, CATEGORY_*).
            None => continue,
        };

        let (leaf, path) = if role == LabelRole::Custom {
            split_user_label(name)
        } else {
            (name.to_string(), name.to_string())
        };

        out.push(SyncableLabel {
            remote_id: label.id.clone(),
            name: leaf,
            path,
            role,
            unread_count: i64::from(label.messages_unread.unwrap_or(0)),
            total_count: i64::from(label.messages_total.unwrap_or(0)),
        });
    }

    if !out.iter().any(|l| l.role == LabelRole::Inbox) {
        out.insert(
            0,
            SyncableLabel {
                remote_id: LABEL_INBOX.into(),
                name: "Inbox".into(),
                path: LABEL_INBOX.into(),
                role: LabelRole::Inbox,
                unread_count: 0,
                total_count: 0,
            },
        );
    }

    out
}

/// Convert syncable labels into the provider-neutral folder shape.
pub fn to_remote_folders(labels: &[SyncableLabel]) -> Vec<RemoteFolder> {
    labels
        .iter()
        .map(|l| RemoteFolder {
            remote_id: l.remote_id.clone(),
            name: l.name.clone(),
            path: l.path.clone(),
            role_hint: Some(
                match l.role {
                    LabelRole::Inbox => "inbox",
                    LabelRole::Sent => "sent",
                    LabelRole::Drafts => "drafts",
                    LabelRole::Spam => "junk",
                    LabelRole::Trash => "trash",
                    LabelRole::Archive => "archive",
                    LabelRole::Custom => "custom",
                }
                .to_string(),
            ),
            // Gmail has no UIDVALIDITY and no MODSEQ. Both are IMAP concepts; Gmail's ordering
            // comes from `internalDate` and the history cursor instead.
            uid_validity: None,
            highest_mod_seq: None,
        })
        .collect()
}

/// Translate Vespetrel flags into Gmail label ids for an update.
///
/// Gmail models read/starred/draft as labels, so a flag change is a label change. `Flag::Seen`
/// removing `UNREAD` is the one inversion: Gmail's `UNREAD` is present when unread.
pub fn flags_to_label_delta(
    add: &[vespetrel_core::Flag],
    remove: &[vespetrel_core::Flag],
) -> (Vec<String>, Vec<String>) {
    use vespetrel_core::Flag;

    let mut add_ids: Vec<String> = Vec::new();
    let mut remove_ids: Vec<String> = Vec::new();

    let apply =
        |flag: &Flag, adding: bool, add_ids: &mut Vec<String>, remove_ids: &mut Vec<String>| {
            match flag {
                // `Seen` is the inverse of Gmail's UNREAD: marking seen removes it, unmarking
                // re-adds it. Getting this backwards would make "Mark as read" mark unread.
                Flag::Seen => {
                    if adding {
                        remove_ids.push(LABEL_UNREAD.to_string());
                    } else {
                        add_ids.push(LABEL_UNREAD.to_string());
                    }
                }
                // Gmail has no "answered" label.
                Flag::Answered => {}
                Flag::Flagged => {
                    if adding {
                        add_ids.push(LABEL_STARRED.to_string());
                    } else {
                        remove_ids.push(LABEL_STARRED.to_string());
                    }
                }
                Flag::Deleted => {
                    if adding {
                        add_ids.push(LABEL_TRASH.to_string());
                    } else {
                        remove_ids.push(LABEL_TRASH.to_string());
                    }
                }
                Flag::Draft => {
                    if adding {
                        add_ids.push(LABEL_DRAFT.to_string());
                    } else {
                        remove_ids.push(LABEL_DRAFT.to_string());
                    }
                }
            }
        };

    for flag in add {
        apply(flag, true, &mut add_ids, &mut remove_ids);
    }
    for flag in remove {
        apply(flag, false, &mut add_ids, &mut remove_ids);
    }

    add_ids.sort();
    add_ids.dedup();
    remove_ids.sort();
    remove_ids.dedup();
    // A label in both lists is a no-op; dropping it avoids a pointless round trip.
    remove_ids.retain(|id| !add_ids.contains(id));

    (add_ids, remove_ids)
}

/// Translate a message's Gmail label ids into Vespetrel flags.
///
/// `UNREAD` is checked first and short-circuits: a message that is unread is never "seen",
/// regardless of what else it carries.
pub fn labels_to_flags(label_ids: &[String]) -> Vec<vespetrel_core::Flag> {
    use vespetrel_core::Flag;
    let mut flags = Vec::new();
    // `Flag` has no Unread variant: unread is the absence of `Seen`.
    if !label_ids.iter().any(|l| l == LABEL_UNREAD) {
        flags.push(Flag::Seen);
    }
    if label_ids.iter().any(|l| l == LABEL_STARRED) {
        flags.push(Flag::Flagged);
    }
    if label_ids.iter().any(|l| l == LABEL_DRAFT) {
        flags.push(Flag::Draft);
    }
    if label_ids.iter().any(|l| l == LABEL_TRASH) {
        flags.push(Flag::Deleted);
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(id: &str) -> GmailLabel {
        GmailLabel {
            id: id.into(),
            name: id.into(),
            label_type: "system".into(),
            messages_unread: Some(2),
            messages_total: Some(10),
        }
    }

    fn user(id: &str, name: &str) -> GmailLabel {
        GmailLabel {
            id: id.into(),
            name: name.into(),
            label_type: "user".into(),
            messages_unread: None,
            messages_total: None,
        }
    }

    #[test]
    fn special_use_labels_map_to_their_roles() {
        assert_eq!(role_for_system_label(LABEL_INBOX), Some(LabelRole::Inbox));
        assert_eq!(role_for_system_label(LABEL_SENT), Some(LabelRole::Sent));
        assert_eq!(role_for_system_label(LABEL_TRASH), Some(LabelRole::Trash));
    }

    /// State labels must not become folders, or the folder tree grows meaningless entries.
    #[test]
    fn state_only_system_labels_do_not_become_folders() {
        for label in [
            LABEL_UNREAD,
            LABEL_STARRED,
            LABEL_IMPORTANT,
            "CATEGORY_PROMOTIONS",
        ] {
            assert_eq!(
                role_for_system_label(label),
                None,
                "{label} describes state, not location"
            );
        }
        let folders = syncable_labels(&[system(LABEL_UNREAD), system(LABEL_STARRED)]);
        assert!(
            !folders.iter().any(|f| f.remote_id == LABEL_UNREAD),
            "UNREAD must not appear as a folder"
        );
    }

    #[test]
    fn user_labels_become_custom_folders() {
        let folders = syncable_labels(&[user("Label_1", "Receipts")]);
        let custom = folders.iter().find(|f| f.role == LabelRole::Custom);
        assert_eq!(custom.map(|f| f.name.as_str()), Some("Receipts"));
    }

    #[test]
    fn a_slash_in_a_user_label_keeps_its_path_but_trims_the_leaf() {
        let folders = syncable_labels(&[user("Label_2", "Work/Clients")]);
        let custom = folders
            .iter()
            .find(|f| f.role == LabelRole::Custom)
            .expect("custom folder");
        assert_eq!(custom.name, "Clients", "the leaf is what the user sees");
        assert_eq!(
            custom.path, "Work/Clients",
            "the full path is kept verbatim; Gmail's '/' is decorative, not a hierarchy"
        );
    }

    #[test]
    fn a_trailing_slash_does_not_produce_an_empty_leaf() {
        let folders = syncable_labels(&[user("Label_3", "Work/")]);
        let custom = folders
            .iter()
            .find(|f| f.role == LabelRole::Custom)
            .expect("custom folder");
        assert!(!custom.name.is_empty(), "an empty label name is unusable");
    }

    /// Without this a mailbox whose label list omits INBOX has no inbox folder at all.
    #[test]
    fn the_inbox_folder_is_synthesised_when_absent() {
        let folders = syncable_labels(&[user("Label_1", "Receipts")]);
        assert!(
            folders.iter().any(|f| f.role == LabelRole::Inbox),
            "a usable mailbox needs an inbox"
        );
    }

    #[test]
    fn a_real_inbox_label_is_not_duplicated() {
        let folders = syncable_labels(&[system(LABEL_INBOX), user("Label_1", "Receipts")]);
        assert_eq!(
            folders
                .iter()
                .filter(|f| f.role == LabelRole::Inbox)
                .count(),
            1
        );
    }

    #[test]
    fn empty_and_oversized_labels_are_skipped() {
        let long = "x".repeat(MAX_LABEL_TEXT + 1);
        let folders = syncable_labels(&[
            user("Label_1", "   "),
            user("Label_2", &long),
            GmailLabel {
                id: String::new(),
                name: "No id".into(),
                label_type: "user".into(),
                messages_unread: None,
                messages_total: None,
            },
        ]);
        assert!(
            folders.iter().all(|f| f.role != LabelRole::Custom),
            "malformed labels must be dropped, not half-created"
        );
    }

    #[test]
    fn unread_and_seen_are_inverses() {
        // `Flag` has no `Unread` variant, so unread is expressed as the absence of `Seen`.
        let unread = labels_to_flags(&[LABEL_UNREAD.into(), LABEL_INBOX.into()]);
        assert!(
            !unread.contains(&vespetrel_core::Flag::Seen),
            "an unread message must not be marked Seen"
        );

        let seen = labels_to_flags(&[LABEL_INBOX.into()]);
        assert!(seen.contains(&vespetrel_core::Flag::Seen));
    }

    #[test]
    fn starred_draft_and_trash_become_flags() {
        let flags = labels_to_flags(&[
            LABEL_STARRED.into(),
            LABEL_DRAFT.into(),
            LABEL_TRASH.into(),
            LABEL_UNREAD.into(),
        ]);
        assert!(flags.contains(&vespetrel_core::Flag::Flagged));
        assert!(flags.contains(&vespetrel_core::Flag::Draft));
        assert!(flags.contains(&vespetrel_core::Flag::Deleted));
        assert!(!flags.contains(&vespetrel_core::Flag::Seen));
    }

    #[test]
    fn marking_seen_removes_the_unread_label() {
        let (add, remove) = flags_to_label_delta(&[vespetrel_core::Flag::Seen], &[]);
        assert!(
            remove.contains(&LABEL_UNREAD.to_string()),
            "marking seen must remove UNREAD, not add it"
        );
        assert!(add.is_empty());
    }

    #[test]
    fn marking_unseen_adds_the_unread_label() {
        let (add, _remove) = flags_to_label_delta(&[], &[vespetrel_core::Flag::Seen]);
        assert!(
            add.contains(&LABEL_UNREAD.to_string()),
            "marking unread must add UNREAD"
        );
    }

    #[test]
    fn starring_and_trashing_map_to_labels() {
        let (add, _) = flags_to_label_delta(
            &[vespetrel_core::Flag::Flagged, vespetrel_core::Flag::Deleted],
            &[],
        );
        assert!(add.contains(&LABEL_STARRED.to_string()));
        assert!(add.contains(&LABEL_TRASH.to_string()));
    }

    #[test]
    fn flag_round_trip_is_stable() {
        use vespetrel_core::Flag;
        let original = vec![Flag::Seen, Flag::Flagged];
        let (add, remove) = flags_to_label_delta(&original, &[]);
        // The applied label set is `(existing + added) - removed`, not their union: UNREAD goes
        // into `remove` precisely because it must no longer be present.
        let applied: Vec<String> = add.into_iter().filter(|l| !remove.contains(l)).collect();
        let round_tripped = labels_to_flags(&applied);
        assert!(
            round_tripped.contains(&Flag::Seen),
            "marking seen then reading back must still be Seen; got {:?}",
            round_tripped
        );
        assert!(round_tripped.contains(&Flag::Flagged));
    }

    #[test]
    fn a_label_in_both_directions_is_dropped_from_the_batch() {
        let (add, remove) = flags_to_label_delta(
            &[vespetrel_core::Flag::Flagged],
            &[vespetrel_core::Flag::Flagged],
        );
        assert!(add.contains(&LABEL_STARRED.to_string()));
        assert!(
            !remove.contains(&LABEL_STARRED.to_string()),
            "adding and removing the same label is a no-op, not two writes"
        );
    }

    #[test]
    fn remote_folders_carry_role_hints_and_no_imap_only_fields() {
        let folders = to_remote_folders(&syncable_labels(&[system(LABEL_INBOX)]));
        let inbox = &folders[0];
        assert_eq!(inbox.role_hint.as_deref(), Some("inbox"));
        assert_eq!(inbox.uid_validity, None, "Gmail has no UIDVALIDITY");
        assert_eq!(inbox.highest_mod_seq, None, "Gmail has no MODSEQ");
    }
}
