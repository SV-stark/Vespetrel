//! Address Book Contact Mailing and Distribution Lists §5 Phase 5
use serde::{Deserialize, Serialize};

use crate::contact::Contact;
use crate::message::Address;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailingList {
    pub id: String,
    pub name: String,
    pub nickname: Option<String>,
    pub description: Option<String>,
    /// List of direct contact IDs or email strings
    pub members: Vec<String>,
}

impl MailingList {
    pub fn new(id: impl Into<String>, name: impl Into<String>, members: Vec<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            nickname: None,
            description: None,
            members,
        }
    }

    pub fn with_nickname(mut self, nickname: impl Into<String>) -> Self {
        self.nickname = Some(nickname.into());
        self
    }
}

/// Address expansion helper for Compose & Send recipients
pub struct MailingListExpander;

impl MailingListExpander {
    /// Expand recipient strings (which can be contacts, mailing list names, nicknames, or raw emails)
    pub fn expand(
        recipients: &[Address],
        contacts: &[Contact],
        lists: &[MailingList],
    ) -> Vec<Address> {
        let mut expanded: Vec<Address> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for recipient in recipients {
            let target = recipient.email.trim();

            // 1. Check if recipient matches a mailing list name or nickname
            if let Some(list) = lists.iter().find(|l| {
                l.name.eq_ignore_ascii_case(target)
                    || l.nickname
                        .as_deref()
                        .is_some_and(|n| n.eq_ignore_ascii_case(target))
            }) {
                for member in &list.members {
                    // Member can be a contact ID or direct email
                    if let Some(contact) = contacts.iter().find(|c| c.id == *member) {
                        if seen.insert(contact.email.to_lowercase()) {
                            expanded.push(Address {
                                name: contact.display_name.clone(),
                                email: contact.email.clone(),
                            });
                        }
                    } else if member.contains('@') && seen.insert(member.to_lowercase()) {
                        expanded.push(Address {
                            name: None,
                            email: member.clone(),
                        });
                    }
                }
            } else if seen.insert(target.to_lowercase()) {
                expanded.push(recipient.clone());
            }
        }

        expanded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mailing_list_expansion() {
        let contacts = vec![
            Contact {
                id: "c1".into(),
                remote_id: None,
                display_name: Some("Alice Smith".into()),
                email: "alice@company.com".into(),
                vcard_data: None,
            },
            Contact {
                id: "c2".into(),
                remote_id: None,
                display_name: Some("Bob Jones".into()),
                email: "bob@company.com".into(),
                vcard_data: None,
            },
        ];

        let dev_team = MailingList::new(
            "list_dev",
            "Dev Team",
            vec!["c1".into(), "c2".into(), "external@contractor.org".into()],
        )
        .with_nickname("devs");

        let lists = vec![dev_team];

        let recipients = vec![Address {
            name: None,
            email: "devs".into(),
        }];

        let result = MailingListExpander::expand(&recipients, &contacts, &lists);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].email, "alice@company.com");
        assert_eq!(result[1].email, "bob@company.com");
        assert_eq!(result[2].email, "external@contractor.org");
    }
}
