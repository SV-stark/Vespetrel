use vespetrel_core::message::{Address, ComposedAttachment, ComposedMessage};

pub struct ComposeState {
    pub draft: ComposedMessage,
    /// Contact autocomplete chips for To, Cc, Bcc
    pub to_chips: Vec<Address>,
    pub cc_chips: Vec<Address>,
    pub bcc_chips: Vec<Address>,
    pub is_encrypted: bool,
    pub is_signed: bool,
    pub is_markdown_mode: bool,
}

impl ComposeState {
    pub fn new(from: Address) -> Self {
        Self {
            draft: ComposedMessage {
                from,
                to: Vec::new(),
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: String::new(),
                body_text: String::new(),
                body_html: None,
                in_reply_to: None,
                references: Vec::new(),
                attachments: Vec::new(),
            },
            to_chips: Vec::new(),
            cc_chips: Vec::new(),
            bcc_chips: Vec::new(),
            is_encrypted: false,
            is_signed: false,
            is_markdown_mode: false,
        }
    }

    pub fn set_subject(&mut self, s: impl Into<String>) {
        self.draft.subject = s.into();
    }

    pub fn set_body(&mut self, text: impl Into<String>, html: Option<String>) {
        self.draft.body_text = text.into();
        self.draft.body_html = html;
    }

    pub fn add_recipient(&mut self, addr: Address) {
        self.to_chips.push(addr.clone());
        self.draft.to.push(addr);
    }

    pub fn remove_recipient(&mut self, email: &str) {
        self.to_chips.retain(|a| a.email != email);
        self.draft.to.retain(|a| a.email != email);
    }

    pub fn add_cc(&mut self, addr: Address) {
        self.cc_chips.push(addr.clone());
        self.draft.cc.push(addr);
    }

    pub fn add_bcc(&mut self, addr: Address) {
        self.bcc_chips.push(addr.clone());
        self.draft.bcc.push(addr);
    }

    pub fn add_attachment(
        &mut self,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        data: Vec<u8>,
    ) {
        self.draft.attachments.push(ComposedAttachment {
            filename: filename.into(),
            content_type: content_type.into(),
            data,
        });
    }

    pub fn remove_attachment(&mut self, index: usize) {
        if index < self.draft.attachments.len() {
            self.draft.attachments.remove(index);
        }
    }

    pub fn total_attachments_size(&self) -> usize {
        self.draft.attachments.iter().map(|a| a.data.len()).sum()
    }

    pub fn generate_preview_html(&self) -> String {
        if let Some(html) = &self.draft.body_html {
            html.clone()
        } else if self.is_markdown_mode {
            // Render basic markdown formatting
            format!(
                "<div class=\"markdown-body\">{}</div>",
                self.draft.body_text
            )
        } else {
            format!("<pre>{}</pre>", self.draft.body_text)
        }
    }

    pub fn validate_for_sending(&self) -> Result<(), String> {
        if self.draft.to.is_empty() && self.draft.cc.is_empty() && self.draft.bcc.is_empty() {
            return Err("Please specify at least one recipient".into());
        }
        if self.draft.from.email.trim().is_empty() {
            return Err("Sender address cannot be empty".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compose_state() {
        let from = Address {
            name: Some("Me".into()),
            email: "me@example.com".into(),
        };
        let mut compose = ComposeState::new(from);
        assert!(compose.validate_for_sending().is_err());

        compose.add_recipient(Address {
            name: Some("Recipient".into()),
            email: "recip@example.com".into(),
        });
        compose.set_subject("Hello");
        compose.set_body("World", None);

        assert!(compose.validate_for_sending().is_ok());
        assert_eq!(compose.to_chips.len(), 1);

        compose.add_attachment("test.txt", "text/plain", vec![1, 2, 3, 4]);
        assert_eq!(compose.total_attachments_size(), 4);
        compose.remove_attachment(0);
        assert_eq!(compose.total_attachments_size(), 0);
    }
}
