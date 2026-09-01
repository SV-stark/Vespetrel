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
        let snapped = snap_range_to_char_boundaries(&self.text, selection);
        if snapped.is_empty() {
            return;
        }
        if let Some(pos) = self
            .spans
            .iter()
            .position(|s| s.range == snapped && s.style == style)
        {
            self.spans.remove(pos);
        } else {
            self.spans.push(TextSpan {
                range: snapped,
                style,
                link_url: None,
            });
        }
    }

    /// Attach hyperlink to selected text [Ctrl+K]
    pub fn add_link(&mut self, selection: Range<usize>, url: impl Into<String>) {
        let snapped = snap_range_to_char_boundaries(&self.text, selection);
        if snapped.is_empty() {
            return;
        }
        let clean_url = sanitize_link_url(&url.into());
        self.spans.push(TextSpan {
            range: snapped,
            style: InlineStyle::Underline,
            link_url: Some(clean_url),
        });
    }

    /// TipTap-style markdown inline input rule parser
    /// Converts `**bold**`, `*italic*`, `_italic_`, `# heading`, `- list`, `1. list`, ` ``` ` into spans and blocks
    pub fn parse_markdown(markdown: &str) -> Self {
        let mut doc = Self::default();
        let mut current_offset = 0;
        let mut in_code_block = false;
        let mut code_lang = None;

        for line in markdown.lines() {
            let line_trimmed = line.trim();
            if line_trimmed.starts_with("```") {
                if in_code_block {
                    in_code_block = false;
                    code_lang = None;
                } else {
                    in_code_block = true;
                    code_lang = line_trimmed
                        .strip_prefix("```")
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                }
                continue;
            }

            let (block_kind, text_content) = if in_code_block {
                (BlockKind::CodeBlock(code_lang.clone()), line)
            } else if let Some(h3) = line_trimmed.strip_prefix("### ") {
                (BlockKind::Heading(3), h3)
            } else if let Some(h2) = line_trimmed.strip_prefix("## ") {
                (BlockKind::Heading(2), h2)
            } else if let Some(h1) = line_trimmed.strip_prefix("# ") {
                (BlockKind::Heading(1), h1)
            } else if let Some(bullet) = line_trimmed
                .strip_prefix("- ")
                .or_else(|| line_trimmed.strip_prefix("* "))
            {
                (BlockKind::BulletList, bullet)
            } else if let Some(dot_idx) = line_trimmed.find(". ")
                && line_trimmed[..dot_idx].chars().all(|c| c.is_ascii_digit())
                && let Ok(num) = line_trimmed[..dot_idx].parse::<usize>()
            {
                (BlockKind::NumberedList(num), &line_trimmed[dot_idx + 2..])
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
            let mut in_bullet_list = false;
            let mut in_numbered_list = false;

            for (range, kind) in &self.blocks {
                let formatted_inner = render_inline_spans(&self.text, range.clone(), &self.spans);

                // Handle list transitions
                match kind {
                    BlockKind::BulletList => {
                        if in_numbered_list {
                            html.push_str("</ol>\n");
                            in_numbered_list = false;
                        }
                        if !in_bullet_list {
                            html.push_str("<ul style=\"margin: 6px 0; padding-left: 20px;\">\n");
                            in_bullet_list = true;
                        }
                    }
                    BlockKind::NumberedList(_) => {
                        if in_bullet_list {
                            html.push_str("</ul>\n");
                            in_bullet_list = false;
                        }
                        if !in_numbered_list {
                            html.push_str("<ol style=\"margin: 6px 0; padding-left: 20px;\">\n");
                            in_numbered_list = true;
                        }
                    }
                    _ => {
                        if in_bullet_list {
                            html.push_str("</ul>\n");
                            in_bullet_list = false;
                        }
                        if in_numbered_list {
                            html.push_str("</ol>\n");
                            in_numbered_list = false;
                        }
                    }
                }

                match kind {
                    BlockKind::Heading(1) => {
                        html.push_str(&format!("<h1 style=\"font-size: 1.5rem; font-weight: 700; margin: 16px 0 8px;\">{formatted_inner}</h1>\n"));
                    }
                    BlockKind::Heading(2) => {
                        html.push_str(&format!("<h2 style=\"font-size: 1.25rem; font-weight: 600; margin: 14px 0 6px;\">{formatted_inner}</h2>\n"));
                    }
                    BlockKind::Heading(_) => {
                        html.push_str(&format!("<h3 style=\"font-size: 1.1rem; font-weight: 600; margin: 12px 0 4px;\">{formatted_inner}</h3>\n"));
                    }
                    BlockKind::BulletList => {
                        html.push_str(&format!(
                            "<li style=\"margin: 4px 0;\">{formatted_inner}</li>\n"
                        ));
                    }
                    BlockKind::NumberedList(num) => {
                        html.push_str(&format!(
                            "<li value=\"{num}\" style=\"margin: 4px 0;\">{formatted_inner}</li>\n"
                        ));
                    }
                    BlockKind::Blockquote => {
                        html.push_str(&format!("<blockquote style=\"border-left: 3px solid #d4d4d8; padding-left: 12px; margin: 8px 0; color: #71717a;\">{formatted_inner}</blockquote>\n"));
                    }
                    BlockKind::CodeBlock(_) => {
                        html.push_str(&format!("<pre style=\"background: #f4f4f5; padding: 12px; border-radius: 6px;\"><code>{formatted_inner}</code></pre>\n"));
                    }
                    BlockKind::Paragraph => {
                        if range.is_empty() {
                            html.push_str("<p><br/></p>\n");
                        } else {
                            html.push_str(&format!(
                                "<p style=\"margin: 6px 0;\">{formatted_inner}</p>\n"
                            ));
                        }
                    }
                }
            }

            if in_bullet_list {
                html.push_str("</ul>\n");
            }
            if in_numbered_list {
                html.push_str("</ol>\n");
            }
        }

        html.push_str("</div>");
        html
    }

    pub fn to_plain_text(&self) -> &str {
        &self.text
    }
}

/// Snaps arbitrary byte range to nearest valid UTF-8 character boundaries within string
pub fn snap_range_to_char_boundaries(text: &str, range: Range<usize>) -> Range<usize> {
    if text.is_empty() {
        return 0..0;
    }
    let len = text.len();
    let start_clamped = range.start.min(len);
    let end_clamped = range.end.min(len);

    let start = if text.is_char_boundary(start_clamped) {
        start_clamped
    } else {
        (0..=start_clamped)
            .rev()
            .find(|&i| text.is_char_boundary(i))
            .unwrap_or(0)
    };

    let end = if text.is_char_boundary(end_clamped) {
        end_clamped
    } else {
        (end_clamped..=len)
            .find(|&i| text.is_char_boundary(i))
            .unwrap_or(len)
    };

    start..end.max(start)
}

fn parse_inline_markdown(line: &str, base_offset: usize) -> (String, Vec<TextSpan>) {
    let mut plain = String::new();
    let mut spans = Vec::new();
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '*' && chars.peek() == Some(&'*') {
            chars.next(); // skip second *
            let mut bold_content = String::new();
            let mut closed = false;
            while let Some(bc) = chars.next() {
                if bc == '*' && chars.peek() == Some(&'*') {
                    chars.next();
                    closed = true;
                    break;
                }
                bold_content.push(bc);
            }
            if closed {
                let start = base_offset + plain.len();
                let end = start + bold_content.len();
                plain.push_str(&bold_content);
                spans.push(TextSpan {
                    range: start..end,
                    style: InlineStyle::Bold,
                    link_url: None,
                });
            } else {
                plain.push_str("**");
                plain.push_str(&bold_content);
            }
        } else if c == '*' || c == '_' {
            let delimiter = c;
            let mut italic_content = String::new();
            let mut closed = false;
            while let Some(ic) = chars.next() {
                if ic == delimiter {
                    closed = true;
                    break;
                }
                italic_content.push(ic);
            }
            if closed && !italic_content.is_empty() {
                let start = base_offset + plain.len();
                let end = start + italic_content.len();
                plain.push_str(&italic_content);
                spans.push(TextSpan {
                    range: start..end,
                    style: InlineStyle::Italic,
                    link_url: None,
                });
            } else {
                plain.push(delimiter);
                plain.push_str(&italic_content);
            }
        } else if c == '`' {
            let mut code_content = String::new();
            let mut closed = false;
            for cc in chars.by_ref() {
                if cc == '`' {
                    closed = true;
                    break;
                }
                code_content.push(cc);
            }

            if closed {
                let start = base_offset + plain.len();
                let end = start + code_content.len();
                plain.push_str(&code_content);
                spans.push(TextSpan {
                    range: start..end,
                    style: InlineStyle::InlineCode,
                    link_url: None,
                });
            } else {
                plain.push('`');
                plain.push_str(&code_content);
            }
        } else if c == '[' {
            let mut text = String::new();
            let mut closed_bracket = false;
            for tc in chars.by_ref() {
                if tc == ']' {
                    closed_bracket = true;
                    break;
                }
                text.push(tc);
            }
            if closed_bracket && chars.peek() == Some(&'(') {
                chars.next();
                let mut url = String::new();
                let mut closed_paren = false;
                for uc in chars.by_ref() {
                    if uc == ')' {
                        closed_paren = true;
                        break;
                    }
                    url.push(uc);
                }
                if closed_paren {
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
                    plain.push_str("](");
                    plain.push_str(&url);
                }
            } else {
                plain.push('[');
                plain.push_str(&text);
                if closed_bracket {
                    plain.push(']');
                }
            }
        } else {
            plain.push(c);
        }
    }

    (plain, spans)
}

fn sanitize_link_url(url: &str) -> String {
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

fn render_inline_spans(full_text: &str, block_range: Range<usize>, spans: &[TextSpan]) -> String {
    let start = block_range.start;
    let end = block_range.end;
    if start >= end
        || end > full_text.len()
        || !full_text.is_char_boundary(start)
        || !full_text.is_char_boundary(end)
    {
        return html_escape(&full_text[start.min(full_text.len())..end.min(full_text.len())]);
    }

    let block_text = &full_text[start..end];
    let mut relevant_spans: Vec<&TextSpan> = spans
        .iter()
        .filter(|s| s.range.start >= start && s.range.end <= end && s.range.start < s.range.end)
        .collect();

    if relevant_spans.is_empty() {
        return html_escape(block_text);
    }

    relevant_spans.sort_by_key(|s| s.range.start);

    let mut result = String::new();
    let mut curr = start;

    for span in relevant_spans {
        let span_start = span.range.start.clamp(start, end);
        let span_end = span.range.end.clamp(start, end);

        if span_start > curr
            && full_text.is_char_boundary(curr)
            && full_text.is_char_boundary(span_start)
        {
            result.push_str(&html_escape(&full_text[curr..span_start]));
        }

        if full_text.is_char_boundary(span_start) && full_text.is_char_boundary(span_end) {
            let inner = html_escape(&full_text[span_start..span_end]);
            let formatted = match (&span.style, &span.link_url) {
                (_, Some(url)) => {
                    let safe_url = sanitize_link_url(url);
                    format!(
                        "<a href=\"{safe_url}\" target=\"_blank\" rel=\"noopener noreferrer\" style=\"color: #2563eb; text-decoration: underline;\">{inner}</a>"
                    )
                }
                (InlineStyle::Bold, None) => format!("<strong>{inner}</strong>"),
                (InlineStyle::Italic, None) => format!("<em>{inner}</em>"),
                (InlineStyle::Underline, None) => format!("<u>{inner}</u>"),
                (InlineStyle::Strikethrough, None) => format!("<del>{inner}</del>"),
                (InlineStyle::InlineCode, None) => format!(
                    "<code style=\"background: #f4f4f5; padding: 2px 4px; border-radius: 4px;\">{inner}</code>"
                ),
            };
            result.push_str(&formatted);
            curr = span_end;
        }
    }

    if curr < end && full_text.is_char_boundary(curr) && full_text.is_char_boundary(end) {
        result.push_str(&html_escape(&full_text[curr..end]));
    }

    result
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
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
        let md = "# Team Update\nThis is **critical** info.\nCheck [Vespetrel](https://vespetrel.org).\n- Deliver Phase 7\n- Second bullet";
        let doc = RichTextDocument::parse_markdown(md);
        assert!(!doc.spans.is_empty());
        assert_eq!(doc.blocks.len(), 5);
        assert_eq!(doc.blocks[0].1, BlockKind::Heading(1));
        assert_eq!(doc.blocks[3].1, BlockKind::BulletList);

        let html = doc.to_html();
        assert!(html.contains("<h1"));
        assert!(html.contains("<ul"));
        assert!(html.contains("</ul>"));
        assert!(html.contains("<li"));
        assert!(html.contains("font-family"));
        assert!(html.contains("<strong>critical</strong>"));
        assert!(html.contains("<a href=\"https://vespetrel.org\""));
    }

    #[test]
    fn test_utf8_char_boundary_snapping() {
        let mut doc = RichTextDocument::new("Héllo Wörld");
        // 'é' is at byte 1..3, snapping 0..2 or 0..5 should never panic or slice invalid UTF-8
        doc.toggle_style(0..2, InlineStyle::Bold);
        assert_eq!(doc.spans.len(), 1);
        let html = doc.to_html();
        assert!(html.contains("<strong>Hé</strong>"));
    }
}
