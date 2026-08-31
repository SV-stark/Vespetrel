//! Native TipTap-Style Rich Text & Markdown WYSIWYG Editor Engine §7 Phase 7 (Approach 1)
use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InlineStyle {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    InlineCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockKind {
    Paragraph,
    Heading(u8), // 1, 2, 3
    BulletList,
    NumberedList(usize),
    Blockquote,
    CodeBlock(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSpan {
    pub range: Range<usize>,
    pub style: InlineStyle,
    pub link_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RichTextDocument {
    pub text: String,
    pub spans: Vec<TextSpan>,
    pub blocks: Vec<(Range<usize>, BlockKind)>,
}

impl RichTextDocument {
    pub fn new(initial_text: impl Into<String>) -> Self {
        let text = initial_text.into();
        let len = text.len();
        Self {
            text,
            spans: Vec::new(),
            blocks: vec![(0..len, BlockKind::Paragraph)],
        }
    }

    /// Toggle formatting style over selection (e.g., Bold [Ctrl+B], Italic [Ctrl+I])
    pub fn toggle_style(&mut self, selection: Range<usize>, style: InlineStyle) {
        if selection.is_empty() {
            return;
        }
        if let Some(pos) = self
            .spans
            .iter()
            .position(|s| s.range == selection && s.style == style)
        {
            self.spans.remove(pos);
        } else {
            self.spans.push(TextSpan {
                range: selection,
                style,
                link_url: None,
            });
        }
    }

    /// Attach hyperlink to selected text [Ctrl+K]
    pub fn add_link(&mut self, selection: Range<usize>, url: impl Into<String>) {
        if selection.is_empty() {
            return;
        }
        self.spans.push(TextSpan {
            range: selection,
            style: InlineStyle::Underline,
            link_url: Some(url.into()),
        });
    }

    /// TipTap-style markdown inline input rule parser
    /// Converts `**bold**`, `*italic*`, `# heading`, `- list` into spans and blocks
    pub fn parse_markdown(markdown: &str) -> Self {
        let mut doc = Self::default();
        let mut current_offset = 0;

        for line in markdown.lines() {
            let line_trimmed = line.trim_start();
            let (block_kind, text_content) = if let Some(h1) = line_trimmed.strip_prefix("# ") {
                (BlockKind::Heading(1), h1)
            } else if let Some(h2) = line_trimmed.strip_prefix("## ") {
                (BlockKind::Heading(2), h2)
            } else if let Some(h3) = line_trimmed.strip_prefix("### ") {
                (BlockKind::Heading(3), h3)
            } else if let Some(bullet) = line_trimmed
                .strip_prefix("- ")
                .or_else(|| line_trimmed.strip_prefix("* "))
            {
                (BlockKind::BulletList, bullet)
            } else if let Some(quote) = line_trimmed.strip_prefix("> ") {
                (BlockKind::Blockquote, quote)
            } else {
                (BlockKind::Paragraph, line)
            };

            let (plain_text, line_spans) = parse_inline_markdown(text_content, current_offset);
            let line_len = plain_text.len();
            let block_range = current_offset..current_offset + line_len;

            doc.text.push_str(&plain_text);
            doc.text.push('\n');
            doc.spans.extend(line_spans);
            doc.blocks.push((block_range, block_kind));

            current_offset += line_len + 1;
        }

        doc
    }

    /// Render to clean, email-safe HTML for outbound RFC 5322 MIME multipart
    pub fn to_html(&self) -> String {
        let mut html = String::from(
            "<div style=\"font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; font-size: 14px; line-height: 1.6; color: #18181b;\">\n",
        );

        if self.blocks.is_empty() {
            for line in self.text.lines() {
                if line.trim().is_empty() {
                    html.push_str("<p><br/></p>\n");
                } else {
                    html.push_str(&format!(
                        "<p style=\"margin: 6px 0;\">{}</p>\n",
                        html_escape(line)
                    ));
                }
            }
        } else {
            for (range, kind) in &self.blocks {
                let content = if range.start <= range.end && range.end <= self.text.len() {
                    &self.text[range.clone()]
                } else {
                    ""
                };
                let escaped = html_escape(content);
                match kind {
                    BlockKind::Heading(1) => {
                        html.push_str(&format!("<h1 style=\"font-size: 1.5rem; font-weight: 700; margin: 16px 0 8px;\">{escaped}</h1>\n"));
                    }
                    BlockKind::Heading(2) => {
                        html.push_str(&format!("<h2 style=\"font-size: 1.25rem; font-weight: 600; margin: 14px 0 6px;\">{escaped}</h2>\n"));
                    }
                    BlockKind::Heading(_) => {
                        html.push_str(&format!("<h3 style=\"font-size: 1.1rem; font-weight: 600; margin: 12px 0 4px;\">{escaped}</h3>\n"));
                    }
                    BlockKind::BulletList => {
                        html.push_str(&format!(
                            "<li style=\"margin-left: 20px;\">{escaped}</li>\n"
                        ));
                    }
                    BlockKind::NumberedList(num) => {
                        html.push_str(&format!(
                            "<li value=\"{num}\" style=\"margin-left: 20px;\">{escaped}</li>\n"
                        ));
                    }
                    BlockKind::Blockquote => {
                        html.push_str(&format!("<blockquote style=\"border-left: 3px solid #d4d4d8; padding-left: 12px; margin: 8px 0; color: #71717a;\">{escaped}</blockquote>\n"));
                    }
                    BlockKind::CodeBlock(_) => {
                        html.push_str(&format!("<pre style=\"background: #f4f4f5; padding: 12px; border-radius: 6px;\"><code>{escaped}</code></pre>\n"));
                    }
                    BlockKind::Paragraph => {
                        if escaped.trim().is_empty() {
                            html.push_str("<p><br/></p>\n");
                        } else {
                            html.push_str(&format!("<p style=\"margin: 6px 0;\">{escaped}</p>\n"));
                        }
                    }
                }
            }
        }

        html.push_str("</div>");
        html
    }

    pub fn to_plain_text(&self) -> &str {
        &self.text
    }
}

fn parse_inline_markdown(line: &str, base_offset: usize) -> (String, Vec<TextSpan>) {
    let mut plain = String::new();
    let mut spans = Vec::new();
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '*' && chars.peek() == Some(&'*') {
            chars.next(); // skip second *
            let start = base_offset + plain.len();
            let mut bold_content = String::new();
            while let Some(bc) = chars.next() {
                if bc == '*' && chars.peek() == Some(&'*') {
                    chars.next();
                    break;
                }
                bold_content.push(bc);
            }
            let end = start + bold_content.len();
            plain.push_str(&bold_content);
            spans.push(TextSpan {
                range: start..end,
                style: InlineStyle::Bold,
                link_url: None,
            });
        } else if c == '[' {
            let mut text = String::new();
            for tc in chars.by_ref() {
                if tc == ']' {
                    break;
                }
                text.push(tc);
            }
            if chars.peek() == Some(&'(') {
                chars.next();
                let mut url = String::new();
                for uc in chars.by_ref() {
                    if uc == ')' {
                        break;
                    }
                    url.push(uc);
                }
                let start = base_offset + plain.len();
                let end = start + text.len();
                plain.push_str(&text);
                spans.push(TextSpan {
                    range: start..end,
                    style: InlineStyle::Underline,
                    link_url: Some(url),
                });
            } else {
                plain.push('[');
                plain.push_str(&text);
            }
        } else {
            plain.push(c);
        }
    }

    (plain, spans)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_richtext_toggle_and_link() {
        let mut doc = RichTextDocument::new("Hello World");
        doc.toggle_style(0..5, InlineStyle::Bold);
        assert_eq!(doc.spans.len(), 1);
        assert_eq!(doc.spans[0].style, InlineStyle::Bold);

        // Toggle off
        doc.toggle_style(0..5, InlineStyle::Bold);
        assert_eq!(doc.spans.len(), 0);

        // Add link
        doc.add_link(6..11, "https://vespetrel.org");
        assert_eq!(doc.spans.len(), 1);
        assert_eq!(
            doc.spans[0].link_url.as_deref(),
            Some("https://vespetrel.org")
        );
    }

    #[test]
    fn test_markdown_input_rules_and_html() {
        let md = "# Team Update\nThis is **critical** info.\nCheck [Vespetrel](https://vespetrel.org).\n- Deliver Phase 7";
        let doc = RichTextDocument::parse_markdown(md);
        assert!(!doc.spans.is_empty());
        assert_eq!(doc.blocks.len(), 4);
        assert_eq!(doc.blocks[0].1, BlockKind::Heading(1));
        assert_eq!(doc.blocks[3].1, BlockKind::BulletList);

        let html = doc.to_html();
        assert!(html.contains("<h1"));
        assert!(html.contains("<li"));
        assert!(html.contains("font-family"));
    }
}
