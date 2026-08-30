use vespetrel_core::message::{Address, ComposedMessage};

pub struct ComposeState {
    pub draft: ComposedMessage,
    /// Contact autocomplete chips
    pub to_chips: Vec<Address>,
}

impl ComposeState {
    pub fn new(from: Address) -> Self {
        Self {
            draft: ComposedMessage {
                from: from.clone(),
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
        }
    }

    pub fn set_subject(&mut self, s: impl Into<String>) { self.draft.subject = s.into(); }
    pub fn set_body(&mut self, text: impl Into<String>, html: Option<String>) {
        self.draft.body_text = text.into();
        self.draft.body_html = html;
    }

    pub fn add_recipient(&mut self, addr: Address) {
        self.to_chips.push(addr.clone());
        self.draft.to.push(addr);
    }
}
