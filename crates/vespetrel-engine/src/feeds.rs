//! News & RSS/Atom Feed Reader Engine §7 Phase 5
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use vespetrel_core::MessageSummary;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedSubscription {
    pub id: String,
    pub title: String,
    pub feed_url: String,
    pub site_url: Option<String>,
    pub last_polled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedItem {
    pub guid: String,
    pub title: String,
    pub link: String,
    pub author: Option<String>,
    pub summary: Option<String>,
    pub published_at: DateTime<Utc>,
}

impl FeedItem {
    pub fn to_message_summary(&self) -> MessageSummary {
        MessageSummary {
            id: format!("feed:{}", self.guid),
            thread_id: None,
            subject: Some(self.title.clone()),
            from_address: self.author.clone().unwrap_or_else(|| "rss@feeds".into()),
            from_name: self.author.clone(),
            snippet: self.summary.clone(),
            sent_at: self.published_at,
            is_read: false,
            is_flagged: false,
            has_attachments: false,
        }
    }
}

/// Parse RSS 2.0 or Atom XML feed content
pub fn parse_feed_xml(xml: &str) -> Vec<FeedItem> {
    let mut items = Vec::new();
    let is_atom = xml.contains("<feed") || xml.contains("<entry");

    if is_atom {
        // Parse Atom entries
        for entry_block in xml.split("<entry>").skip(1) {
            if let Some(item) = entry_block
                .split("</entry>")
                .next()
                .and_then(parse_atom_entry)
            {
                items.push(item);
            }
        }
    } else {
        // Parse RSS 2.0 items
        for item_block in xml.split("<item>").skip(1) {
            if let Some(item) = item_block.split("</item>").next().and_then(parse_rss_item) {
                items.push(item);
            }
        }
    }

    items
}

fn parse_rss_item(block: &str) -> Option<FeedItem> {
    let title = extract_tag_content(block, "title")?;
    let link = extract_tag_content(block, "link").unwrap_or_default();
    let guid = extract_tag_content(block, "guid").unwrap_or_else(|| link.clone());
    let author =
        extract_tag_content(block, "author").or_else(|| extract_tag_content(block, "dc:creator"));
    let summary = extract_tag_content(block, "description");

    Some(FeedItem {
        guid,
        title,
        link,
        author,
        summary,
        published_at: Utc::now(),
    })
}

fn parse_atom_entry(block: &str) -> Option<FeedItem> {
    let title = extract_tag_content(block, "title")?;
    let guid = extract_tag_content(block, "id").unwrap_or_default();
    let summary =
        extract_tag_content(block, "summary").or_else(|| extract_tag_content(block, "content"));
    let author = extract_tag_content(block, "name");

    Some(FeedItem {
        guid: if guid.is_empty() { title.clone() } else { guid },
        title,
        link: String::new(),
        author,
        summary,
        published_at: Utc::now(),
    })
}

fn extract_tag_content(xml: &str, tag: &str) -> Option<String> {
    let open_tag = format!("<{tag}>");
    let close_tag = format!("</{tag}>");

    let (_, rest) = xml.split_once(&open_tag)?;
    let (content, _) = rest.split_once(&close_tag)?;

    // Strip CDATA if present
    let clean = if content.starts_with("<![CDATA[") && content.ends_with("]]>") {
        content
            .trim_start_matches("<![CDATA[")
            .trim_end_matches("]]>")
    } else {
        content
    };

    Some(clean.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rss_2_0_feed() {
        let rss_xml = r#"
        <rss version="2.0">
            <channel>
                <title>Rust Blog</title>
                <item>
                    <title>Announcing Rust 1.85</title>
                    <link>https://blog.rust-lang.org/2026/02/20/Rust-1.85.0.html</link>
                    <description><![CDATA[The Rust team is happy to announce a new version.]]></description>
                    <guid>https://blog.rust-lang.org/2026/02/20/Rust-1.85.0.html</guid>
                    <author>The Rust Core Team</author>
                </item>
            </channel>
        </rss>
        "#;

        let items = parse_feed_xml(rss_xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Announcing Rust 1.85");
        assert_eq!(items[0].author.as_deref(), Some("The Rust Core Team"));
        assert!(
            items[0]
                .summary
                .as_deref()
                .unwrap()
                .contains("Rust team is happy")
        );

        let msg = items[0].to_message_summary();
        assert_eq!(msg.subject.as_deref(), Some("Announcing Rust 1.85"));
    }

    #[test]
    fn test_parse_atom_feed() {
        let atom_xml = r#"
        <feed xmlns="http://www.w3.org/2005/Atom">
            <title>Example Feed</title>
            <entry>
                <title>Atom Entry 1</title>
                <id>urn:uuid:1225c695-cfb8-4ebb-aaaa-80da344efa6a</id>
                <summary>Atom summary test</summary>
            </entry>
        </feed>
        "#;

        let items = parse_feed_xml(atom_xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Atom Entry 1");
        assert_eq!(
            items[0].guid,
            "urn:uuid:1225c695-cfb8-4ebb-aaaa-80da344efa6a"
        );
    }
}
