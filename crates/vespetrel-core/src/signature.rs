//! Rich HTML Signatures Engine with Visual Template Presets & Per-Account Profiles
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureTemplate {
    Modern,
    Minimal,
    Corporate,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SignatureProfile {
    pub full_name: String,
    pub job_title: Option<String>,
    pub company: Option<String>,
    pub email: String,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub avatar_url: Option<String>,
    pub pronouns: Option<String>,
    pub disclaimer: Option<String>,
    pub social_links: Vec<(String, String)>, // (Network, URL)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub raw_html: String,
    pub plain_text: Option<String>,
    pub is_default: bool,
    pub include_in_replies: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Signature {
    pub fn new(
        account_id: impl Into<String>,
        name: impl Into<String>,
        raw_html: impl Into<String>,
        plain_text: Option<String>,
        is_default: bool,
        include_in_replies: bool,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            account_id: account_id.into(),
            name: name.into(),
            raw_html: raw_html.into(),
            plain_text,
            is_default,
            include_in_replies,
            created_at: now,
            updated_at: now,
        }
    }

    /// Generate an HTML signature from structured profile using preset templates
    pub fn from_template(
        account_id: impl Into<String>,
        name: impl Into<String>,
        profile: &SignatureProfile,
        template: SignatureTemplate,
        is_default: bool,
        include_in_replies: bool,
    ) -> Self {
        let raw_html = match template {
            SignatureTemplate::Modern => Self::render_modern(profile),
            SignatureTemplate::Minimal => Self::render_minimal(profile),
            SignatureTemplate::Corporate => Self::render_corporate(profile),
        };
        let plain_text = Self::render_plain_text(profile);

        Self::new(
            account_id,
            name,
            raw_html,
            Some(plain_text),
            is_default,
            include_in_replies,
        )
    }

    fn render_modern(p: &SignatureProfile) -> String {
        let mut html = String::from(
            "<div class=\"vespetrel-signature\" style=\"font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; font-size: 13px; line-height: 1.5; color: #3f3f46; margin-top: 20px; padding-top: 14px; border-top: 1px solid #e4e4e7;\">\n",
        );
        html.push_str("  <table cellpadding=\"0\" cellspacing=\"0\" border=\"0\" style=\"border-collapse: collapse;\">\n");
        html.push_str("    <tr>\n");

        if let Some(avatar) = &p.avatar_url {
            html.push_str(&format!(
                "      <td style=\"vertical-align: top; padding-right: 16px;\"><img src=\"{}\" width=\"56\" height=\"56\" style=\"border-radius: 50%; object-fit: cover; display: block; border: 1px solid #d4d4d8;\" alt=\"Avatar\" /></td>\n",
                sanitize_url(avatar)
            ));
        }

        html.push_str("      <td style=\"vertical-align: top;\">\n");
        let name_str = html_escape(&p.full_name);
        let pronouns_str = p
            .pronouns
            .as_ref()
            .map(|pr| format!(" <span style=\"font-size: 11px; color: #a1a1aa; font-weight: normal;\">({})</span>", html_escape(pr)))
            .unwrap_or_default();

        html.push_str(&format!("        <div style=\"font-size: 14px; font-weight: 600; color: #18181b;\">{name_str}{pronouns_str}</div>\n"));

        let mut subtitle = Vec::new();
        if let Some(t) = &p.job_title {
            subtitle.push(html_escape(t));
        }
        if let Some(c) = &p.company {
            subtitle.push(html_escape(c));
        }
        if !subtitle.is_empty() {
            html.push_str(&format!("        <div style=\"color: #71717a; font-size: 12px; margin-bottom: 6px;\">{}</div>\n", subtitle.join(" &bull; ")));
        }

        let mut contact_items = Vec::new();
        if let Some(ph) = &p.phone {
            contact_items.push(format!("<span>📞 {}</span>", html_escape(ph)));
        }
        contact_items.push(format!(
            "<a href=\"mailto:{}\" style=\"color: #2563eb; text-decoration: none;\">✉️ {}</a>",
            html_escape(&p.email),
            html_escape(&p.email)
        ));
        if let Some(web) = &p.website {
            contact_items.push(format!(
                "<a href=\"{}\" style=\"color: #2563eb; text-decoration: none;\">🌐 {}</a>",
                sanitize_url(web),
                html_escape(web)
            ));
        }

        html.push_str(&format!(
            "        <div style=\"font-size: 12px; color: #52525b;\">{}</div>\n",
            contact_items.join(" <span style=\"color: #d4d4d8;\">|</span> ")
        ));

        if !p.social_links.is_empty() {
            let links: Vec<String> = p.social_links.iter().map(|(net, url)| {
                format!("<a href=\"{}\" style=\"color: #4f46e5; text-decoration: none; font-size: 11px; margin-right: 8px; font-weight: 500;\">{}</a>", sanitize_url(url), html_escape(net))
            }).collect();
            html.push_str(&format!(
                "        <div style=\"margin-top: 6px;\">{}</div>\n",
                links.join(" ")
            ));
        }

        html.push_str("      </td>\n");
        html.push_str("    </tr>\n");
        html.push_str("  </table>\n");
        html.push_str("</div>");
        html
    }

    fn render_minimal(p: &SignatureProfile) -> String {
        let mut html = String::from(
            "<div class=\"vespetrel-signature\" style=\"font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; font-size: 12px; line-height: 1.4; color: #52525b; margin-top: 16px; padding-top: 10px; border-top: 1px solid #e4e4e7;\">\n",
        );
        let name_str = html_escape(&p.full_name);
        html.push_str(&format!(
            "  <div style=\"font-weight: 600; color: #18181b;\">{name_str}</div>\n"
        ));
        if let Some(title) = &p.job_title {
            let comp = p.company.as_deref().unwrap_or("");
            let sep = if comp.is_empty() { "" } else { " — " };
            html.push_str(&format!(
                "  <div>{}{sep}{}</div>\n",
                html_escape(title),
                html_escape(comp)
            ));
        }
        html.push_str(&format!("  <div><a href=\"mailto:{}\" style=\"color: #2563eb; text-decoration: none;\">{}</a></div>\n", html_escape(&p.email), html_escape(&p.email)));
        html.push_str("</div>");
        html
    }

    fn render_corporate(p: &SignatureProfile) -> String {
        let mut html = Self::render_modern(p);
        if let Some(disc) = &p.disclaimer {
            html.push_str(&format!(
                "\n<div style=\"font-size: 10px; color: #a1a1aa; line-height: 1.4; margin-top: 10px; max-width: 500px;\">{}</div>",
                html_escape(disc)
            ));
        }
        html
    }

    fn render_plain_text(p: &SignatureProfile) -> String {
        let mut text = String::from("-- \n");
        text.push_str(&p.full_name);
        if let Some(pr) = &p.pronouns {
            text.push_str(&format!(" ({pr})"));
        }
        text.push('\n');

        if let Some(t) = &p.job_title {
            text.push_str(t);
            if let Some(c) = &p.company {
                text.push_str(&format!(" | {c}"));
            }
            text.push('\n');
        }

        text.push_str(&p.email);
        if let Some(ph) = &p.phone {
            text.push_str(&format!(" | {ph}"));
        }
        if let Some(web) = &p.website {
            text.push_str(&format!(" | {web}"));
        }
        text.push('\n');

        if let Some(disc) = &p.disclaimer {
            text.push('\n');
            text.push_str(disc);
            text.push('\n');
        }

        text
    }
}

fn sanitize_url(url: &str) -> String {
    let trimmed = url.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:")
    {
        html_escape(trimmed)
    } else {
        "#".into()
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[derive(Debug, Clone, Default)]
pub struct SignatureStore {
    signatures: Vec<Signature>,
}

impl SignatureStore {
    pub fn new() -> Self {
        Self {
            signatures: Vec::new(),
        }
    }

    pub fn add(&mut self, sig: Signature) {
        if sig.is_default {
            for s in &mut self.signatures {
                if s.account_id == sig.account_id {
                    s.is_default = false;
                }
            }
        }
        self.signatures.push(sig);
    }

    pub fn get_default(&self, account_id: &str) -> Option<&Signature> {
        self.signatures
            .iter()
            .find(|s| (s.account_id == account_id || s.account_id == "*") && s.is_default)
            .or_else(|| {
                self.signatures
                    .iter()
                    .find(|s| s.account_id == account_id || s.account_id == "*")
            })
    }

    pub fn list_for_account(&self, account_id: &str) -> Vec<&Signature> {
        self.signatures
            .iter()
            .filter(|s| s.account_id == account_id || s.account_id == "*")
            .collect()
    }

    pub fn remove(&mut self, sig_id: &str) -> bool {
        if let Some(pos) = self.signatures.iter().position(|s| s.id == sig_id) {
            self.signatures.remove(pos);
            true
        } else {
            false
        }
    }

    /// Inject signature into composed email bodies (HTML and Plain Text)
    pub fn apply_signature(
        &self,
        account_id: &str,
        body_html: &str,
        body_text: &str,
        is_reply: bool,
    ) -> (String, String) {
        let sig = self.get_default(account_id);
        if let Some(sig) = sig {
            if is_reply && !sig.include_in_replies {
                return (body_html.to_string(), body_text.to_string());
            }

            let new_html = if body_html.contains("class=\"vespetrel-signature\"") {
                body_html.to_string()
            } else {
                format!("{body_html}\n{}", sig.raw_html)
            };

            let new_text = if let Some(plain) = &sig.plain_text {
                if body_text.contains("-- \n") {
                    body_text.to_string()
                } else {
                    format!("{body_text}\n\n{plain}")
                }
            } else {
                body_text.to_string()
            };

            (new_html, new_text)
        } else {
            (body_html.to_string(), body_text.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_template_generation() {
        let profile = SignatureProfile {
            full_name: "Suyash Stark".into(),
            job_title: Some("Staff Engineer".into()),
            company: Some("Vespetrel Core".into()),
            email: "suyash@vespetrel.org".into(),
            phone: Some("+1 (555) 019-2831".into()),
            website: Some("https://vespetrel.org".into()),
            avatar_url: Some("https://vespetrel.org/avatar.png".into()),
            pronouns: Some("he/him".into()),
            disclaimer: Some("Confidential communication".into()),
            social_links: vec![("GitHub".into(), "https://github.com/SV-stark".into())],
        };

        let sig = Signature::from_template(
            "acc_1",
            "Work Signature",
            &profile,
            SignatureTemplate::Modern,
            true,
            true,
        );

        assert_eq!(sig.name, "Work Signature");
        assert!(sig.is_default);
        assert!(sig.raw_html.contains("Suyash Stark"));
        assert!(sig.raw_html.contains("Staff Engineer"));
        assert!(sig.raw_html.contains("Vespetrel Core"));
        assert!(sig.raw_html.contains("he/him"));
        assert!(sig.raw_html.contains("vespetrel.org/avatar.png"));
        assert!(
            sig.plain_text
                .as_ref()
                .unwrap()
                .contains("-- \nSuyash Stark")
        );
    }

    #[test]
    fn test_signature_store_and_injection() {
        let mut store = SignatureStore::new();

        let sig1 = Signature::new(
            "acc_work",
            "Default Work",
            "<div class=\"vespetrel-signature\">Best, <strong>Alice</strong></div>",
            Some("-- \nBest, Alice".into()),
            true,
            false, // don't include in reply
        );
        store.add(sig1);

        let body_html = "<p>Here is the quarterly report.</p>";
        let body_text = "Here is the quarterly report.";

        // New mail (is_reply = false)
        let (out_html, out_text) = store.apply_signature("acc_work", body_html, body_text, false);
        assert!(out_html.contains("class=\"vespetrel-signature\""));
        assert!(out_text.contains("-- \nBest, Alice"));

        // Reply mail (is_reply = true) -> Should not inject since include_in_replies = false
        let (reply_html, reply_text) =
            store.apply_signature("acc_work", body_html, body_text, true);
        assert_eq!(reply_html, body_html);
        assert_eq!(reply_text, body_text);
    }
}
