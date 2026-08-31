//! Reusable Email Snippets & Templates with Variables §7 Phase 7
use ahash::AHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmailTemplate {
    pub id: String,
    pub name: String,
    pub subject_template: String,
    pub body_template: String,
    pub shortcut: Option<String>,
}

impl EmailTemplate {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        subject: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            subject_template: subject.into(),
            body_template: body.into(),
            shortcut: None,
        }
    }

    /// Substitute `{{variable}}` placeholders with values from variables map
    pub fn render(&self, variables: &AHashMap<String, String>) -> (String, String) {
        let rendered_subject = interpolate_template(&self.subject_template, variables);
        let rendered_body = interpolate_template(&self.body_template, variables);
        (rendered_subject, rendered_body)
    }
}

fn interpolate_template(template: &str, variables: &AHashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, val) in variables {
        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, val);
    }
    result
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TemplateStore {
    pub templates: Vec<EmailTemplate>,
}

impl TemplateStore {
    pub fn new() -> Self {
        let mut store = Self {
            templates: Vec::new(),
        };
        store.register_defaults();
        store
    }

    pub fn register_defaults(&mut self) {
        self.templates = vec![
            EmailTemplate {
                id: "tmpl_intro".into(),
                name: "Quick Introduction".into(),
                subject_template: "Introduction: {{my_name}} <> {{recipient_name}}".into(),
                body_template: "Hi {{recipient_name}},\n\nGreat meeting you! As discussed, here is the information regarding {{project}}.\n\nBest,\n{{my_name}}".into(),
                shortcut: Some(";intro".into()),
            },
            EmailTemplate {
                id: "tmpl_meeting".into(),
                name: "Meeting Follow-up".into(),
                subject_template: "Follow-up: {{meeting_topic}}".into(),
                body_template: "Hi {{recipient_name}},\n\nThanks for your time today discussing {{meeting_topic}}. Looking forward to our next steps.\n\nRegards,\n{{my_name}}".into(),
                shortcut: Some(";followup".into()),
            },
        ];
    }

    pub fn find_by_shortcut(&self, shortcut: &str) -> Option<&EmailTemplate> {
        self.templates
            .iter()
            .find(|t| t.shortcut.as_deref() == Some(shortcut))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_interpolation() {
        let tmpl = EmailTemplate::new(
            "tmpl_1",
            "Pitch",
            "Hello {{name}} from {{company}}",
            "Dear {{name}},\n\nWelcome to {{company}}!",
        );

        let mut vars = AHashMap::new();
        vars.insert("name".to_string(), "Alice".to_string());
        vars.insert("company".to_string(), "Acme Corp".to_string());

        let (subj, body) = tmpl.render(&vars);
        assert_eq!(subj, "Hello Alice from Acme Corp");
        assert_eq!(body, "Dear Alice,\n\nWelcome to Acme Corp!");
    }

    #[test]
    fn test_template_store_shortcuts() {
        let store = TemplateStore::new();
        let tmpl = store.find_by_shortcut(";intro");
        assert!(tmpl.is_some());
        assert_eq!(tmpl.unwrap().id, "tmpl_intro");
    }
}
