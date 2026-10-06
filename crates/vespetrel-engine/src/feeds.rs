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
    // A real XML scanner, not `str::find`. Feed payloads are attacker-controlled: an item's
    // `<description>` may legally contain CDATA holding markup, so a substring search for
    // `<title` happily returns markup living *inside another element's text*.
    let is_atom = match find_start_tag(xml, 0, "") {
        Some((_, tag)) => match local_name(&tag.name) {
            "feed" => true,
            "rss" | "rdf" => false,
            // Unrecognisable root (bare fragment). Fall back to sniffing.
            _ => {
                let lower = xml.to_ascii_lowercase();
                lower.contains("<feed") || lower.contains("<entry")
            }
        },
        None => return Vec::new(),
    };

    let container = if is_atom { "entry" } else { "item" };
    let mut items = Vec::new();
    let mut cursor = 0usize;
    while let Some((start, _)) = find_start_tag(xml, cursor, container) {
        cursor = start + 1;
        let Some(raw) = element_raw_text(xml, start) else {
            continue;
        };
        if let Some(item) = if is_atom {
            parse_atom_entry(raw)
        } else {
            parse_rss_item(raw)
        } {
            items.push(item);
        }
    }

    items
}

fn parse_rss_item(block: &str) -> Option<FeedItem> {
    let title = extract_tag_content(block, "title")?;
    let link = extract_atom_link(block).unwrap_or_default();
    let guid = extract_tag_content(block, "guid").unwrap_or_else(|| link.clone());
    let author = extract_tag_content(block, "author")
        .or_else(|| extract_tag_content(block, "dc:creator"))
        .filter(|a| !a.is_empty());
    let summary = extract_tag_content(block, "description")
        .or_else(|| extract_tag_content(block, "content:encoded"));
    let published_at = extract_tag_content(block, "pubDate")
        .or_else(|| extract_tag_content(block, "dc:date"))
        .and_then(|d| {
            DateTime::parse_from_rfc2822(&d)
                .ok()
                .or_else(|| DateTime::parse_from_rfc3339(&d).ok())
                .map(|dt| dt.with_timezone(&Utc))
        })
        .unwrap_or_else(Utc::now);

    Some(FeedItem {
        guid,
        title,
        link,
        author,
        summary,
        published_at,
    })
}

fn parse_atom_entry(block: &str) -> Option<FeedItem> {
    let title = extract_tag_content(block, "title")?;
    let guid = extract_tag_content(block, "id").unwrap_or_default();
    let summary =
        extract_tag_content(block, "summary").or_else(|| extract_tag_content(block, "content"));
    let author = extract_tag_content(block, "name").filter(|a| !a.is_empty());
    let link = extract_atom_link(block).unwrap_or_default();
    let published_at = extract_tag_content(block, "published")
        .or_else(|| extract_tag_content(block, "updated"))
        .and_then(|d| {
            DateTime::parse_from_rfc3339(&d)
                .ok()
                .or_else(|| DateTime::parse_from_rfc2822(&d).ok())
                .map(|dt| dt.with_timezone(&Utc))
        })
        .unwrap_or_else(Utc::now);

    Some(FeedItem {
        guid: if guid.is_empty() { title.clone() } else { guid },
        title,
        link,
        author,
        summary,
        published_at,
    })
}

fn extract_atom_link(xml: &str) -> Option<String> {
    // Preference order:
    //   1. `rel="alternate"` href (Atom entries carry the article URL only as an attribute).
    //   2. The `<link>` element's text (RSS 2.0). Checked before any unqualified href so the
    //      `rel="self"` autodiscovery link that RSS 2.0 feeds sprinkle around cannot win.
    //   3. Any href, e.g. a lone `<atom:link>` item link.
    let mut any_href: Option<String> = None;
    let mut cursor = 0usize;
    while let Some((start, tag)) = find_start_tag(xml, cursor, "link") {
        cursor = start + 1;
        if let Some(href) = tag.attr("href") {
            let rel = tag.attr("rel").unwrap_or_else(|| "alternate".to_string());
            if rel.eq_ignore_ascii_case("alternate") {
                return Some(href);
            }
            any_href.get_or_insert(href);
        }
    }
    extract_tag_content(xml, "link").or(any_href)
}

/// One parsed XML start tag.
struct StartTag {
    /// Lowercased element name, namespace prefix retained (`atom:link`).
    name: String,
    /// Raw attribute text between the name and the closing `>`.
    attrs: String,
    /// Byte offset just past the tag's `>`.
    end: usize,
    self_closing: bool,
}

impl StartTag {
    /// Value of `attr`, either quote style accepted, entities decoded.
    fn attr(&self, attr: &str) -> Option<String> {
        let lower_attrs = self.attrs.to_ascii_lowercase();
        let mut from = 0usize;
        while let Some(rel) = lower_attrs[from..].find(attr) {
            let at = from + rel;
            from = at + attr.len();
            // The attribute name must end here, otherwise `href` also matches `xhref`.
            let name_ends = at == 0
                || lower_attrs[..at]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_ascii_whitespace());
            if !name_ends {
                continue;
            }
            let mut rest = lower_attrs[from..].trim_start();
            let Some(stripped) = rest.strip_prefix('=') else {
                continue;
            };
            rest = stripped.trim_start();
            let quote = rest.chars().next()?;
            if quote != '"' && quote != '\'' {
                continue;
            }
            let value = &rest[quote.len_utf8()..];
            let end = value.find(quote)?;
            return Some(decode_xml_entities(&value[..end]));
        }
        None
    }
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Advance past any non-element markup at `pos` (comment, CDATA, DOCTYPE, processing
/// instruction), returning `pos` unchanged when there is none.
fn skip_non_element(xml: &str, mut pos: usize) -> usize {
    loop {
        if pos >= xml.len() {
            return pos;
        }
        let rest = &xml[pos..];
        let next = if let Some(body) = rest.strip_prefix("<!--") {
            4 + body.find("-->").map_or(body.len(), |i| i + 3)
        } else if let Some(body) = rest.strip_prefix("<![CDATA[") {
            9 + body.find("]]>").map_or(body.len(), |i| i + 3)
        } else if let Some(body) = rest.strip_prefix("<?") {
            2 + body.find("?>").map_or(body.len(), |i| i + 2)
        } else if let Some(body) = rest.strip_prefix("<!") {
            2 + body.find('>').map_or(body.len(), |i| i + 1)
        } else {
            return pos;
        };
        pos += next;
    }
}

/// How a requested element name is matched against a tag in the document.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NameMatch {
    /// Any tag; used to discover the document root.
    Any,
    /// Qualified name must be identical: `link` does not match `atom:link`.
    Exact,
    /// Namespace prefix ignored: `link` matches `atom:link`.
    LocalName,
}

/// Find the next start tag at or after `from` whose name matches.
fn find_start_tag_matching(
    xml: &str,
    from: usize,
    name: &str,
    mode: NameMatch,
) -> Option<(usize, StartTag)> {
    let bytes = xml.as_bytes();
    let mut pos = from.min(xml.len());
    while pos < bytes.len() {
        if bytes[pos] != b'<' {
            pos += 1;
            continue;
        }
        // Markup inside a comment or CDATA section is content, not structure.
        let skipped = skip_non_element(xml, pos);
        if skipped != pos {
            pos = skipped;
            continue;
        }
        let after = &xml[pos + 1..];
        let after = after.strip_prefix('/').unwrap_or(after);
        let name_end = after
            .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
            .unwrap_or(after.len());
        if name_end == 0 {
            // A bare `<` is literal text, not a tag.
            pos += 1;
            continue;
        }
        let tag_name = after[..name_end].to_ascii_lowercase();
        let gt = xml[pos + 1 + name_end..]
            .find('>')
            .map(|i| pos + 1 + name_end + i)?;
        let self_closing = bytes[gt - 1] == b'/' && gt > pos + 1 + name_end;
        let tag = StartTag {
            name: tag_name,
            attrs: xml[pos + 1 + name_end..if self_closing { gt - 1 } else { gt }]
                .trim()
                .to_string(),
            end: gt + 1,
            self_closing,
        };
        let hit = match mode {
            NameMatch::Any => true,
            NameMatch::Exact => tag.name == name,
            NameMatch::LocalName => local_name(&tag.name) == name,
        };
        if hit {
            return Some((pos, tag));
        }
        pos = tag.end;
    }
    None
}

fn find_start_tag(xml: &str, from: usize, name: &str) -> Option<(usize, StartTag)> {
    let mode = if name.is_empty() {
        NameMatch::Any
    } else {
        NameMatch::LocalName
    };
    find_start_tag_matching(xml, from, name, mode)
}

/// Raw inner text of the element whose start tag begins at `tag_start`.
///
/// Tracks nesting of same-named elements so a `<title>` inside `<title>`, or an `<item>`
/// inside a CDATA-free `<item>`, does not terminate the scan early.
fn element_raw_text(xml: &str, tag_start: usize) -> Option<&str> {
    let (_, tag) = find_start_tag(xml, tag_start, "")?;
    if tag.self_closing {
        return Some("");
    }
    let bytes = xml.as_bytes();
    let mut depth = 1usize;
    let mut pos = tag.end;
    while pos < bytes.len() {
        if bytes[pos] != b'<' {
            pos += 1;
            continue;
        }
        let skipped = skip_non_element(xml, pos);
        if skipped != pos {
            pos = skipped;
            continue;
        }
        let after = &xml[pos + 1..];
        if after.len() < 2 {
            return None;
        }
        let (closing, rest) = match after.strip_prefix('/') {
            Some(r) => (true, r),
            None => (false, after),
        };
        let name_end = rest
            .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
            .unwrap_or(rest.len());
        if name_end == 0 {
            pos += 1;
            continue;
        }
        if rest[..name_end].to_ascii_lowercase() != tag.name {
            pos += 1;
            continue;
        }
        let gt = xml[pos + 1 + name_end..]
            .find('>')
            .map(|i| pos + 1 + name_end + i)?;
        if closing {
            depth -= 1;
            if depth == 0 {
                return Some(&xml[tag.end..pos]);
            }
        } else if bytes[gt - 1] != b'/' {
            depth += 1;
        }
        pos = gt + 1;
    }
    None
}

/// Inner text of the first `<name>` element, CDATA-unwrapped and entity-decoded.
///
/// Matches the qualified name exactly, so a request for `link` never picks up the
/// self-closing `<atom:link rel="self" href="..."/>` that RSS 2.0 feeds sprinkle around.
fn extract_tag_content(xml: &str, name: &str) -> Option<String> {
    let (start, _) = find_start_tag_matching(xml, 0, name, NameMatch::Exact)?;
    let raw = element_raw_text(xml, start)?;
    Some(decode_xml_entities(unwrap_cdata(raw)).trim().to_string())
}

/// Reduce a CDATA section to its payload. Feeds routinely wrap the whole body in one, and
/// the previous code only unwrapped when the text *both* started with `<![CDATA[` and ended
/// with `]]>`, so `<![CDATA[x]]> trailing` kept the literal marker in the rendered summary.
fn unwrap_cdata(text: &str) -> &str {
    let Some(open) = text.find("<![CDATA[") else {
        return text;
    };
    let body = &text[open + "<![CDATA[".len()..];
    match body.find("]]>") {
        Some(close) => &body[..close],
        // Unterminated CDATA: keep the remainder rather than dropping the whole element.
        None => body,
    }
}

/// Decode the XML predefined entities, numeric character references, and the HTML named
/// entities that feeds emit in practice.
///
/// A single left-to-right pass, so `&amp;lt;` decodes to `&lt;` and not to `<`.
fn decode_xml_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            let len = input[i..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&input[i..i + len]);
            i += len;
            continue;
        }
        match decode_reference(&input[i..]) {
            Some((c, consumed)) => {
                out.push(c);
                i += consumed;
            }
            None => {
                out.push('&');
                i += 1;
            }
        }
    }
    out
}

/// Decode one `&...;` reference at the start of `input`, returning the character and the
/// number of bytes consumed.
fn decode_reference(input: &str) -> Option<(char, usize)> {
    let semi = input[1..].find(';')? + 1;
    // No real entity name is this long; bail out on ordinary prose with a semicolon.
    if semi > 12 {
        return None;
    }
    let name = &input[1..semi];
    let decoded = match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(dec) = name.strip_prefix('#') {
                dec.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                named_entity(name)
            }
        }
    }?;
    Some((decoded, semi + 1))
}

fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "nbsp" => '\u{a0}',
        "copy" => '\u{a9}',
        "reg" => '\u{ae}',
        "trade" => '\u{2122}',
        "hellip" => '\u{2026}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201c}',
        "rdquo" => '\u{201d}',
        "bull" => '\u{2022}',
        "middot" => '\u{b7}',
        "laquo" => '\u{ab}',
        "raquo" => '\u{bb}',
        "deg" => '\u{b0}',
        "plusmn" => '\u{b1}',
        "times" => '\u{d7}',
        "divide" => '\u{f7}',
        "euro" => '\u{20ac}',
        "pound" => '\u{a3}',
        "yen" => '\u{a5}',
        "cent" => '\u{a2}',
        "sect" => '\u{a7}',
        "para" => '\u{b6}',
        "dagger" => '\u{2020}',
        "permil" => '\u{2030}',
        "eacute" => '\u{e9}',
        "egrave" => '\u{e8}',
        "agrave" => '\u{e0}',
        "ccedil" => '\u{e7}',
        "uuml" => '\u{fc}',
        "ouml" => '\u{f6}',
        "auml" => '\u{e4}',
        "szlig" => '\u{df}',
        "ntilde" => '\u{f1}',
        "aacute" => '\u{e1}',
        "iacute" => '\u{ed}',
        "oacute" => '\u{f3}',
        "uacute" => '\u{fa}',
        _ => return None,
    })
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

    /// A `<description>` may legally contain CDATA holding markup. The old substring parser
    /// stopped the `<item>` block at the first `</item>` *inside* that CDATA, and
    /// `extract_tag_content` matched the `<title>` buried in it, so the article body was
    /// promoted to the headline and the real title was lost.
    #[test]
    fn test_cdata_markup_does_not_become_the_title() {
        let xml = concat!(
            r#"<rss version="2.0"><channel>"#,
            "<item>",
            "  <title>Real Headline</title>",
            "  <description><![CDATA[<item><title>Spoofed</title></item>]]></description>",
            "  <link>https://example.com/a</link>",
            "</item>",
            "</channel></rss>"
        );
        let items = parse_feed_xml(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Real Headline");
        assert_eq!(items[0].link, "https://example.com/a");
        assert_eq!(
            items[0].summary.as_deref(),
            Some("<item><title>Spoofed</title></item>")
        );
    }

    /// `&amp;` and friends were never decoded, so every feed title rendered as
    /// `Tom &amp; Jerry` and query strings arrived with literal `&amp;`.
    #[test]
    fn test_entities_are_decoded_once() {
        assert_eq!(decode_xml_entities("Tom &amp; Jerry"), "Tom & Jerry");
        assert_eq!(decode_xml_entities("&lt;b&gt;"), "<b>");
        assert_eq!(decode_xml_entities("&quot;q&quot;"), "\"q\"");
        assert_eq!(decode_xml_entities("caf&#233;"), "café");
        assert_eq!(decode_xml_entities("&#x1F600;"), "😀");
        assert_eq!(decode_xml_entities("a&nbsp;b"), "a\u{a0}b");
        // Single left-to-right pass: an escaped ampersand must not re-trigger decoding.
        assert_eq!(decode_xml_entities("&amp;lt;"), "&lt;");
        // Unknown / malformed references are preserved verbatim rather than dropped.
        assert_eq!(decode_xml_entities("&notanentity;"), "&notanentity;");
        assert_eq!(decode_xml_entities("AT&T & R&D"), "AT&T & R&D");
    }

    /// `<item >` and `<item attr="x">` are both legal; the old `split("<item>")` silently
    /// dropped every such item.
    #[test]
    fn test_item_tags_with_attributes_are_parsed() {
        let xml = concat!(
            r#"<rss version="2.0"><channel>"#,
            r#"<item xml:base="https://example.com/">"#,
            "<title>Spaced</title>",
            "</item>",
            r#"<item ><title>Trailing space</title></item>"#,
            "</channel></rss>"
        );
        let items = parse_feed_xml(xml);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Spaced");
        assert_eq!(items[1].title, "Trailing space");
    }

    /// RSS 2.0 feeds carry `<atom:link rel="self" href="..."/>` for feed autodiscovery. The
    /// old `<link` prefix search matched it, so the item's link pointed at the feed URL.
    #[test]
    fn test_atom_self_link_does_not_shadow_item_link() {
        let xml = concat!(
            r#"<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom"><channel>"#,
            r#"<atom:link rel="self" href="https://example.com/feed.xml" type="application/rss+xml"/>"#,
            "<item>",
            r#"<atom:link rel="self" href="https://example.com/feed.xml"/>"#,
            "<title>Post</title>",
            "<link>https://example.com/post</link>",
            "</item>",
            "</channel></rss>"
        );
        let items = parse_feed_xml(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].link, "https://example.com/post");
    }

    /// Atom entries put the link in an attribute, and `rel="self"` must not win over
    /// `rel="alternate"`.
    #[test]
    fn test_atom_link_prefers_alternate() {
        let xml = concat!(
            r#"<feed xmlns="http://www.w3.org/2005/Atom">"#,
            "<entry>",
            "<title>Entry</title>",
            r#"<link rel="self" href="https://example.com/feed/1"/>"#,
            r#"<link rel="alternate" href="https://example.com/article"/>"#,
            "</entry>",
            "</feed>"
        );
        let items = parse_feed_xml(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].link, "https://example.com/article");
    }

    /// The root-element sniff must not be fooled by markup inside a comment, and a comment
    /// must not be able to terminate an item early.
    #[test]
    fn test_comments_are_not_structure() {
        let xml = concat!(
            "<!-- <item><title>Ghost</title></item> -->",
            r#"<rss version="2.0"><channel>"#,
            "<item>",
            "<!-- </item> -->",
            "<title>Real</title>",
            "</item>",
            "</channel></rss>"
        );
        let items = parse_feed_xml(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Real");
    }

    /// A malformed feed must not panic and must not loop.
    #[test]
    fn test_malformed_feed_does_not_panic() {
        for xml in [
            "",
            "<rss",
            "<rss><channel><item><title>Unclosed",
            "<rss><channel><item><![CDATA[unterminated",
            "<item>",
            "<rss><channel><item>text only, no title</item></channel></rss>",
            "<a><<b>></a>",
            "<rss><channel><item><title></title></item></channel></rss>",
        ] {
            let items = parse_feed_xml(xml);
            // The contract is "does not panic or hang", not a specific count.
            assert!(items.len() < 100);
        }
    }
}
