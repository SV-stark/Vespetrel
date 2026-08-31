//! Contact Address Book View §6 & §7 Phase 3
use std::collections::BTreeMap;
use vespetrel_core::Contact;

#[derive(Debug, Clone)]
pub struct ContactsView {
    pub contacts: Vec<Contact>,
    pub search_query: String,
    pub selected_contact_id: Option<String>,
}

impl ContactsView {
    pub fn new(contacts: Vec<Contact>) -> Self {
        Self {
            contacts,
            search_query: String::new(),
            selected_contact_id: None,
        }
    }

    pub fn set_search(&mut self, query: impl Into<String>) {
        self.search_query = query.into().trim().to_string();
    }

    pub fn filtered_contacts(&self) -> Vec<&Contact> {
        let q_bytes = self.search_query.as_bytes();
        if q_bytes.is_empty() {
            self.contacts.iter().collect()
        } else {
            self.contacts
                .iter()
                .filter(|c| {
                    crate::views::message_list::contains_ignore_case_ascii(
                        c.email.as_bytes(),
                        q_bytes,
                    ) || c
                        .display_name
                        .as_deref()
                        .map(|n| {
                            crate::views::message_list::contains_ignore_case_ascii(
                                n.as_bytes(),
                                q_bytes,
                            )
                        })
                        .unwrap_or(false)
                })
                .collect()
        }
    }

    /// Group contacts alphabetically by first letter of display name (or email)
    pub fn grouped_alphabetically(&self) -> BTreeMap<char, Vec<&Contact>> {
        let mut groups: BTreeMap<char, Vec<&Contact>> = BTreeMap::new();
        for c in self.filtered_contacts() {
            let ch = c
                .display_name
                .as_deref()
                .and_then(|n| n.chars().next())
                .unwrap_or_else(|| c.email.chars().next().unwrap_or('#'))
                .to_ascii_uppercase();
            groups.entry(ch).or_default().push(c);
        }
        groups
    }

    /// Autocomplete helper for compose recipient chips
    pub fn autocomplete(&self, prefix: &str) -> Vec<&Contact> {
        let p_bytes = prefix.trim().as_bytes();
        if p_bytes.is_empty() {
            return Vec::new();
        }
        self.contacts
            .iter()
            .filter(|c| {
                let email_match = c.email.len() >= p_bytes.len()
                    && c.email.as_bytes()[..p_bytes.len()].eq_ignore_ascii_case(p_bytes);
                let name_match = c
                    .display_name
                    .as_deref()
                    .map(|n| {
                        n.len() >= p_bytes.len()
                            && n.as_bytes()[..p_bytes.len()].eq_ignore_ascii_case(p_bytes)
                    })
                    .unwrap_or(false);
                email_match || name_match
            })
            .take(10)
            .collect()
    }
}

impl Default for ContactsView {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contacts_view_grouping_and_autocomplete() {
        let c1 = Contact {
            id: "1".into(),
            remote_id: None,
            display_name: Some("Alice Smith".into()),
            email: "alice@example.com".into(),
            vcard_data: None,
        };
        let c2 = Contact {
            id: "2".into(),
            remote_id: None,
            display_name: Some("Bob Jones".into()),
            email: "bob@example.com".into(),
            vcard_data: None,
        };
        let c3 = Contact {
            id: "3".into(),
            remote_id: None,
            display_name: Some("Albert Einstein".into()),
            email: "albert@example.com".into(),
            vcard_data: None,
        };

        let view = ContactsView::new(vec![c1, c2, c3]);
        let groups = view.grouped_alphabetically();
        assert_eq!(groups.get(&'A').map(|v| v.len()), Some(2));
        assert_eq!(groups.get(&'B').map(|v| v.len()), Some(1));

        let auto = view.autocomplete("alb");
        assert_eq!(auto.len(), 1);
        assert_eq!(auto[0].email, "albert@example.com");
    }
}
