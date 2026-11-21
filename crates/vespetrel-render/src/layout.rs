//! Clean-room HTML5 email layout IR builder.
//!
//! Converts email HTML into a structured, style-resolved `EmailDocument` tree
//! suitable for native GPU rendering without an embedded browser/webview.
//!
//! Pipeline:
//! 1. `html::rewrite_html` (`lol_html`) strips active tags, tracking pixels, and
//!    blocks remote images / cleans tracking URLs / flags phishing links.
//! 2. `html5ever` parses the rewritten HTML into an `RcDom`.
//! 3. `<style>` blocks are parsed by `css::Stylesheet` (safe visual properties only).
//! 4. The DOM is walked with CSS cascade + HTML presentational attributes to emit
//!    a clean `EmailDocument` of containers, tables, rich-text runs, CTA buttons,
//!    images, lists, and rules.

use std::collections::HashMap;
use std::default::Default;

use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use crate::css::{
    ComputedStyle, CssColor, DisplayMode, ElementMeta, Stylesheet, TextAlign, compute_style,
};
use crate::html::{RewriteOptions, SanitizeOptions, rewrite_html};

/// Link metadata attached to an inline text run or CTA button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkTarget {
    pub href: String,
    pub phishing_risk: bool,
}

/// A styled inline text run inside a `RichText` block.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRunNode {
    pub text: String,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub line_through: bool,
    pub monospace: bool,
    pub color: Option<CssColor>,
    pub background_color: Option<CssColor>,
    pub link: Option<LinkTarget>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableCellNode {
    pub style: ComputedStyle,
    pub colspan: u16,
    pub rowspan: u16,
    pub children: Vec<LayoutNode>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableRowNode {
    pub style: ComputedStyle,
    pub cells: Vec<TableCellNode>,
}

/// A layout node in the resolved email document tree.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutNode {
    Container {
        style: ComputedStyle,
        children: Vec<LayoutNode>,
    },
    Table {
        style: ComputedStyle,
        cellpadding: f32,
        cellspacing: f32,
        rows: Vec<TableRowNode>,
    },
    RichText {
        style: ComputedStyle,
        align: TextAlign,
        runs: Vec<TextRunNode>,
    },
    ButtonLink {
        style: ComputedStyle,
        label: String,
        target: LinkTarget,
    },
    Image {
        style: ComputedStyle,
        src: Option<String>,
        blocked_src: Option<String>,
        alt: String,
        width: Option<f32>,
        height: Option<f32>,
        is_cid: bool,
    },
    ListItem {
        style: ComputedStyle,
        marker: String,
        children: Vec<LayoutNode>,
    },
    Rule {
        style: ComputedStyle,
    },
}

/// Root resolved email layout document.
#[derive(Debug, Clone, PartialEq)]
pub struct EmailDocument {
    pub body_style: ComputedStyle,
    pub nodes: Vec<LayoutNode>,
    pub blocked_image_count: usize,
}

impl EmailDocument {
    /// Extract a readable plain-text summary of the laid-out document.
    pub fn plain_text_preview(&self) -> String {
        let mut out = String::new();
        collect_preview_text(&self.nodes, &mut out);
        out.trim().to_string()
    }
}

fn collect_preview_text(nodes: &[LayoutNode], out: &mut String) {
    for node in nodes {
        match node {
            LayoutNode::Container { children, .. } => {
                collect_preview_text(children, out);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            LayoutNode::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_preview_text(&cell.children, out);
                        if !out.ends_with(' ') && !out.ends_with('\n') {
                            out.push(' ');
                        }
                    }
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
            }
            LayoutNode::RichText { runs, .. } => {
                for r in runs {
                    out.push_str(&r.text);
                }
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            LayoutNode::ButtonLink { label, .. } => {
                out.push('[');
                out.push_str(label);
                out.push_str("]\n");
            }
            LayoutNode::Image { alt, .. } => {
                if !alt.is_empty() {
                    out.push('[');
                    out.push_str(alt);
                    out.push_str("]\n");
                }
            }
            LayoutNode::ListItem {
                marker, children, ..
            } => {
                out.push_str(marker);
                out.push(' ');
                collect_preview_text(children, out);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            LayoutNode::Rule { .. } => out.push_str("---\n"),
        }
    }
}

/// Parse, sanitize, cascade CSS, and build an `EmailDocument` layout tree.
pub fn parse_email_html(html: &str, opts: &SanitizeOptions) -> anyhow::Result<EmailDocument> {
    let rewritten = rewrite_html(html, &opts.rewrite)?;
    let dom = parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut rewritten.as_bytes())?;

    let mut stylesheet = Stylesheet::default();
    collect_stylesheets(&dom.document, &mut stylesheet);

    let root_style = ComputedStyle {
        display: DisplayMode::Block,
        font_size: 14.0,
        font_weight: 400,
        ..Default::default()
    };

    let mut ctx = BuildCtx {
        stylesheet: &stylesheet,
        opts: &opts.rewrite,
        blocked_image_count: 0,
    };

    let mut body_style = root_style.clone();
    let nodes;
    let mut ancestors: Vec<AncestorEntry> = Vec::new();

    if let Some(body_handle) = find_element(&dom.document, "body") {
        let attrs = extract_attrs(&body_handle);
        let id = attrs.get("id").map(String::as_str);
        let classes: Vec<&str> = split_classes(attrs.get("class"));
        body_style = compute_style(
            "body",
            id,
            &classes,
            &attrs,
            &[],
            &root_style,
            &stylesheet,
            DisplayMode::Block,
        );
        ancestors.push((
            "body".to_string(),
            id.map(str::to_string),
            split_classes_owned(attrs.get("class")),
        ));
        nodes = build_block_children(&body_handle, &body_style, &mut ancestors, &mut ctx, false);
    } else {
        nodes = build_block_children(&dom.document, &root_style, &mut ancestors, &mut ctx, false);
    }

    Ok(EmailDocument {
        body_style,
        nodes,
        blocked_image_count: ctx.blocked_image_count,
    })
}

struct BuildCtx<'a> {
    stylesheet: &'a Stylesheet,
    opts: &'a RewriteOptions,
    blocked_image_count: usize,
}

type AncestorEntry = (String, Option<String>, Vec<String>);

fn with_ancestors<R>(ancestors: &[AncestorEntry], f: impl FnOnce(&[ElementMeta<'_>]) -> R) -> R {
    let metas: Vec<ElementMeta<'_>> = ancestors
        .iter()
        .map(|(tag, id, classes)| {
            // Note: classes_refs borrows from `classes`
            let _ = (tag, id, classes);
            ElementMeta {
                tag: tag.as_str(),
                id: id.as_deref(),
                classes: &[],
            }
        })
        .collect();
    // Build class slice storage alongside
    let class_slices: Vec<Vec<&str>> = ancestors
        .iter()
        .map(|(_, _, c)| c.iter().map(String::as_str).collect())
        .collect();
    let metas: Vec<ElementMeta<'_>> = metas
        .into_iter()
        .zip(class_slices.iter())
        .map(|(mut m, cs)| {
            m.classes = cs.as_slice();
            m
        })
        .collect();
    f(&metas)
}

fn collect_stylesheets(node: &Handle, out: &mut Stylesheet) {
    if let NodeData::Element { name, .. } = &node.data
        && name.local.as_ref().eq_ignore_ascii_case("style")
    {
        let mut css_text = String::new();
        for child in node.children.borrow().iter() {
            if let NodeData::Text { contents } = &child.data {
                css_text.push_str(&contents.borrow());
            }
        }
        if !css_text.trim().is_empty() {
            out.merge(Stylesheet::parse(&css_text));
        }
        return;
    }
    for child in node.children.borrow().iter() {
        collect_stylesheets(child, out);
    }
}

fn find_element(node: &Handle, target_tag: &str) -> Option<Handle> {
    if let NodeData::Element { name, .. } = &node.data
        && name.local.as_ref().eq_ignore_ascii_case(target_tag)
    {
        return Some(node.clone());
    }
    for child in node.children.borrow().iter() {
        if let Some(found) = find_element(child, target_tag) {
            return Some(found);
        }
    }
    None
}

fn extract_attrs(node: &Handle) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let NodeData::Element { attrs, .. } = &node.data {
        for attr in attrs.borrow().iter() {
            map.insert(
                attr.name.local.as_ref().to_ascii_lowercase(),
                attr.value.to_string(),
            );
        }
    }
    map
}

fn split_classes(class_attr: Option<&String>) -> Vec<&str> {
    class_attr
        .map(|s| s.split_whitespace().collect())
        .unwrap_or_default()
}

fn split_classes_owned(class_attr: Option<&String>) -> Vec<String> {
    class_attr
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

fn default_display_for_tag(tag: &str) -> DisplayMode {
    match tag {
        "html" | "body" | "div" | "section" | "article" | "header" | "footer" | "main" | "nav"
        | "aside" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "pre" | "ul"
        | "ol" | "dl" | "dt" | "dd" | "center" | "figure" | "figcaption" | "address" | "hr" => {
            DisplayMode::Block
        }
        "li" => DisplayMode::ListItem,
        "table" => DisplayMode::Table,
        "tr" => DisplayMode::TableRow,
        "td" | "th" => DisplayMode::TableCell,
        _ => DisplayMode::Inline,
    }
}

fn is_skipped_tag(tag: &str) -> bool {
    matches!(
        tag,
        "head" | "script" | "style" | "noscript" | "template" | "title" | "meta" | "link" | "svg"
    )
}

fn is_block_tag(tag: &str, style: &ComputedStyle) -> bool {
    if matches!(
        style.display,
        DisplayMode::Block
            | DisplayMode::Flex
            | DisplayMode::Table
            | DisplayMode::ListItem
            | DisplayMode::None
    ) {
        return true;
    }
    matches!(
        tag,
        "div"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "main"
            | "nav"
            | "aside"
            | "p"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "pre"
            | "ul"
            | "ol"
            | "li"
            | "table"
            | "hr"
            | "center"
            | "img"
    )
}

fn apply_semantic_tag_defaults(tag: &str, style: &mut ComputedStyle, has_explicit_font_size: bool) {
    match tag {
        "b" | "strong" | "th" => style.font_weight = 700,
        "i" | "em" | "cite" => style.italic = true,
        "u" | "ins" => style.underline = true,
        "s" | "strike" | "del" => style.line_through = true,
        "pre" => {
            style.monospace = true;
            if style.padding.top == 0.0 && style.padding.left == 0.0 {
                style.padding = crate::css::EdgeValues::all(10.0);
            }
            if style.border_radius == 0.0 {
                style.border_radius = 6.0;
            }
        }
        "code" | "kbd" | "tt" | "samp" => style.monospace = true,
        "center" => style.text_align = Some(TextAlign::Center),
        "h1" => {
            style.font_weight = 700;
            if !has_explicit_font_size {
                style.font_size = 24.0;
            }
            if style.margin_top == 0.0 {
                style.margin_top = 12.0;
            }
            if style.margin_bottom == 0.0 {
                style.margin_bottom = 8.0;
            }
        }
        "h2" => {
            style.font_weight = 700;
            if !has_explicit_font_size {
                style.font_size = 20.0;
            }
            if style.margin_top == 0.0 {
                style.margin_top = 10.0;
            }
            if style.margin_bottom == 0.0 {
                style.margin_bottom = 6.0;
            }
        }
        "h3" => {
            style.font_weight = 700;
            if !has_explicit_font_size {
                style.font_size = 17.0;
            }
            if style.margin_top == 0.0 {
                style.margin_top = 8.0;
            }
            if style.margin_bottom == 0.0 {
                style.margin_bottom = 4.0;
            }
        }
        "h4" | "h5" | "h6" => {
            style.font_weight = 700;
            if !has_explicit_font_size {
                style.font_size = 15.0;
            }
        }
        "p" => {
            if style.margin_bottom == 0.0 {
                style.margin_bottom = 8.0;
            }
        }
        "blockquote" => {
            if style.padding.left == 0.0 {
                style.padding.left = 12.0;
            }
            if style.border_left.is_none() {
                style.border_left = Some(crate::css::BorderSide {
                    width: 3.0,
                    color: Some(CssColor::rgb(161, 161, 170)),
                });
            }
            if style.margin_bottom == 0.0 {
                style.margin_bottom = 8.0;
            }
        }
        _ => {}
    }
}

fn compute_element_style(
    node: &Handle,
    tag: &str,
    attrs: &HashMap<String, String>,
    ancestors: &[AncestorEntry],
    parent_style: &ComputedStyle,
    stylesheet: &Stylesheet,
) -> ComputedStyle {
    let _ = node;
    let id = attrs.get("id").map(String::as_str);
    let classes = split_classes(attrs.get("class"));
    let default_disp = default_display_for_tag(tag);
    let mut style = with_ancestors(ancestors, |metas| {
        compute_style(
            tag,
            id,
            &classes,
            attrs,
            metas,
            parent_style,
            stylesheet,
            default_disp,
        )
    });
    let has_explicit_fs = attrs
        .get("style")
        .is_some_and(|s| s.to_ascii_lowercase().contains("font-size"));
    apply_semantic_tag_defaults(tag, &mut style, has_explicit_fs);
    style
}

fn build_block_children(
    parent: &Handle,
    parent_style: &ComputedStyle,
    ancestors: &mut Vec<AncestorEntry>,
    ctx: &mut BuildCtx<'_>,
    preserve_ws: bool,
) -> Vec<LayoutNode> {
    let mut out = Vec::new();
    let mut pending_runs: Vec<TextRunNode> = Vec::new();

    let flush_runs = |runs: &mut Vec<TextRunNode>, out: &mut Vec<LayoutNode>| {
        trim_edge_whitespace(runs);
        if !runs.is_empty() {
            out.push(LayoutNode::RichText {
                style: parent_style.clone(),
                align: parent_style.text_align.unwrap_or(TextAlign::Left),
                runs: std::mem::take(runs),
            });
        }
    };

    for child in parent.children.borrow().iter() {
        match &child.data {
            NodeData::Text { contents } => {
                let raw = contents.borrow();
                let text = if preserve_ws {
                    raw.to_string()
                } else {
                    collapse_whitespace(&raw)
                };
                if !text.is_empty() {
                    push_text_run(&mut pending_runs, text, parent_style, None);
                }
            }
            NodeData::Element { name, .. } => {
                let tag = name.local.as_ref().to_ascii_lowercase();
                if is_skipped_tag(&tag) {
                    continue;
                }
                let attrs = extract_attrs(child);
                let style = compute_element_style(
                    child,
                    &tag,
                    &attrs,
                    ancestors,
                    parent_style,
                    ctx.stylesheet,
                );
                if style.display == DisplayMode::None || style.visibility_hidden {
                    continue;
                }

                if tag == "br" {
                    push_text_run(&mut pending_runs, "\n".to_string(), parent_style, None);
                    continue;
                }

                if tag == "hr" {
                    flush_runs(&mut pending_runs, &mut out);
                    out.push(LayoutNode::Rule { style });
                    continue;
                }

                if tag == "img" {
                    flush_runs(&mut pending_runs, &mut out);
                    if let Some(img_node) = build_image_node(&attrs, style, ctx) {
                        out.push(img_node);
                    }
                    continue;
                }

                // CTA button-like <a> (inline-block/block with background or border)
                if tag == "a"
                    && matches!(style.display, DisplayMode::InlineBlock | DisplayMode::Block)
                    && (style.background_color.is_some()
                        || style.border_top.is_some()
                        || style.border_bottom.is_some())
                    && let Some(target) = extract_link_target(&attrs)
                {
                    let label = collect_plain_text(child).trim().to_string();
                    if !label.is_empty() {
                        flush_runs(&mut pending_runs, &mut out);
                        out.push(LayoutNode::ButtonLink {
                            style,
                            label,
                            target,
                        });
                        continue;
                    }
                }

                if tag == "table" {
                    flush_runs(&mut pending_runs, &mut out);
                    let id = attrs.get("id").cloned();
                    let classes = split_classes_owned(attrs.get("class"));
                    ancestors.push((tag, id, classes));
                    let table = build_table_node(child, &attrs, style, ancestors, ctx);
                    ancestors.pop();
                    out.push(table);
                    continue;
                }

                if tag == "ul" || tag == "ol" {
                    flush_runs(&mut pending_runs, &mut out);
                    let id = attrs.get("id").cloned();
                    let classes = split_classes_owned(attrs.get("class"));
                    ancestors.push((tag.clone(), id, classes));
                    let items = build_list_items(child, tag == "ol", &style, ancestors, ctx);
                    ancestors.pop();
                    out.extend(items);
                    continue;
                }

                if is_block_tag(&tag, &style) {
                    flush_runs(&mut pending_runs, &mut out);
                    let id = attrs.get("id").cloned();
                    let classes = split_classes_owned(attrs.get("class"));
                    let child_preserve_ws = preserve_ws || tag == "pre";
                    ancestors.push((tag, id, classes));
                    let children =
                        build_block_children(child, &style, ancestors, ctx, child_preserve_ws);
                    ancestors.pop();
                    if !children.is_empty() || style.background_color.is_some() {
                        out.push(LayoutNode::Container { style, children });
                    }
                } else {
                    // Inline element: collect into pending_runs
                    let link = if tag == "a" {
                        extract_link_target(&attrs)
                    } else {
                        None
                    };
                    let id = attrs.get("id").cloned();
                    let classes = split_classes_owned(attrs.get("class"));
                    ancestors.push((tag.clone(), id, classes));
                    collect_inline_runs(
                        child,
                        &style,
                        link.as_ref(),
                        ancestors,
                        ctx,
                        preserve_ws || tag == "code",
                        &mut pending_runs,
                        &mut out,
                    );
                    ancestors.pop();
                }
            }
            _ => {}
        }
    }

    flush_runs(&mut pending_runs, &mut out);
    out
}

#[allow(clippy::too_many_arguments)]
fn collect_inline_runs(
    node: &Handle,
    style: &ComputedStyle,
    active_link: Option<&LinkTarget>,
    ancestors: &mut Vec<AncestorEntry>,
    ctx: &mut BuildCtx<'_>,
    preserve_ws: bool,
    runs: &mut Vec<TextRunNode>,
    block_out: &mut Vec<LayoutNode>,
) {
    for child in node.children.borrow().iter() {
        match &child.data {
            NodeData::Text { contents } => {
                let raw = contents.borrow();
                let text = if preserve_ws {
                    raw.to_string()
                } else {
                    collapse_whitespace(&raw)
                };
                if !text.is_empty() {
                    push_text_run(runs, text, style, active_link.cloned());
                }
            }
            NodeData::Element { name, .. } => {
                let tag = name.local.as_ref().to_ascii_lowercase();
                if is_skipped_tag(&tag) {
                    continue;
                }
                let attrs = extract_attrs(child);
                let child_style =
                    compute_element_style(child, &tag, &attrs, ancestors, style, ctx.stylesheet);
                if child_style.display == DisplayMode::None || child_style.visibility_hidden {
                    continue;
                }
                if tag == "br" {
                    push_text_run(runs, "\n".to_string(), style, active_link.cloned());
                    continue;
                }
                if tag == "img" {
                    if let Some(img_node) = build_image_node(&attrs, child_style, ctx) {
                        if !runs.is_empty() {
                            trim_edge_whitespace(runs);
                            if !runs.is_empty() {
                                block_out.push(LayoutNode::RichText {
                                    style: style.clone(),
                                    align: style.text_align.unwrap_or(TextAlign::Left),
                                    runs: std::mem::take(runs),
                                });
                            }
                        }
                        block_out.push(img_node);
                    }
                    continue;
                }

                let link = if tag == "a" {
                    extract_link_target(&attrs).or_else(|| active_link.cloned())
                } else {
                    active_link.cloned()
                };
                let id = attrs.get("id").cloned();
                let classes = split_classes_owned(attrs.get("class"));
                ancestors.push((tag.clone(), id, classes));
                collect_inline_runs(
                    child,
                    &child_style,
                    link.as_ref(),
                    ancestors,
                    ctx,
                    preserve_ws || tag == "code",
                    runs,
                    block_out,
                );
                ancestors.pop();
            }
            _ => {}
        }
    }
}

fn build_table_node(
    table: &Handle,
    table_attrs: &HashMap<String, String>,
    style: ComputedStyle,
    ancestors: &mut Vec<AncestorEntry>,
    ctx: &mut BuildCtx<'_>,
) -> LayoutNode {
    let cellpadding = table_attrs
        .get("cellpadding")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(0.0)
        .clamp(0.0, 48.0);
    let cellspacing = table_attrs
        .get("cellspacing")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(0.0)
        .clamp(0.0, 24.0);

    let mut rows = Vec::new();
    collect_table_rows(table, &style, cellpadding, ancestors, ctx, &mut rows);

    LayoutNode::Table {
        style,
        cellpadding,
        cellspacing,
        rows,
    }
}

fn collect_table_rows(
    node: &Handle,
    table_style: &ComputedStyle,
    cellpadding: f32,
    ancestors: &mut Vec<AncestorEntry>,
    ctx: &mut BuildCtx<'_>,
    rows_out: &mut Vec<TableRowNode>,
) {
    for child in node.children.borrow().iter() {
        let NodeData::Element { name, .. } = &child.data else {
            continue;
        };
        let tag = name.local.as_ref().to_ascii_lowercase();
        if matches!(tag.as_str(), "thead" | "tbody" | "tfoot") {
            let attrs = extract_attrs(child);
            let sec_style =
                compute_element_style(child, &tag, &attrs, ancestors, table_style, ctx.stylesheet);
            if sec_style.display == DisplayMode::None {
                continue;
            }
            let id = attrs.get("id").cloned();
            let classes = split_classes_owned(attrs.get("class"));
            ancestors.push((tag, id, classes));
            collect_table_rows(child, &sec_style, cellpadding, ancestors, ctx, rows_out);
            ancestors.pop();
        } else if tag == "tr" {
            let attrs = extract_attrs(child);
            let row_style =
                compute_element_style(child, &tag, &attrs, ancestors, table_style, ctx.stylesheet);
            if row_style.display == DisplayMode::None || row_style.visibility_hidden {
                continue;
            }
            let id = attrs.get("id").cloned();
            let classes = split_classes_owned(attrs.get("class"));
            ancestors.push((tag, id, classes));
            let mut cells = Vec::new();
            for cell_handle in child.children.borrow().iter() {
                let NodeData::Element {
                    name: cell_name, ..
                } = &cell_handle.data
                else {
                    continue;
                };
                let ctag = cell_name.local.as_ref().to_ascii_lowercase();
                if ctag != "td" && ctag != "th" {
                    continue;
                }
                let cattrs = extract_attrs(cell_handle);
                let mut cell_style = compute_element_style(
                    cell_handle,
                    &ctag,
                    &cattrs,
                    ancestors,
                    &row_style,
                    ctx.stylesheet,
                );
                if cell_style.display == DisplayMode::None || cell_style.visibility_hidden {
                    continue;
                }
                if cellpadding > 0.0
                    && cell_style.padding.top == 0.0
                    && cell_style.padding.right == 0.0
                    && cell_style.padding.bottom == 0.0
                    && cell_style.padding.left == 0.0
                {
                    cell_style.padding = crate::css::EdgeValues::all(cellpadding);
                }
                let colspan = cattrs
                    .get("colspan")
                    .and_then(|v| v.trim().parse::<u16>().ok())
                    .unwrap_or(1)
                    .max(1);
                let rowspan = cattrs
                    .get("rowspan")
                    .and_then(|v| v.trim().parse::<u16>().ok())
                    .unwrap_or(1)
                    .max(1);
                let cid = cattrs.get("id").cloned();
                let cclasses = split_classes_owned(cattrs.get("class"));
                ancestors.push((ctag, cid, cclasses));
                let children =
                    build_block_children(cell_handle, &cell_style, ancestors, ctx, false);
                ancestors.pop();
                cells.push(TableCellNode {
                    style: cell_style,
                    colspan,
                    rowspan,
                    children,
                });
            }
            ancestors.pop();
            if !cells.is_empty() {
                rows_out.push(TableRowNode {
                    style: row_style,
                    cells,
                });
            }
        }
    }
}

fn build_list_items(
    list: &Handle,
    ordered: bool,
    list_style: &ComputedStyle,
    ancestors: &mut Vec<AncestorEntry>,
    ctx: &mut BuildCtx<'_>,
) -> Vec<LayoutNode> {
    let mut items = Vec::new();
    let mut idx = 1usize;
    for child in list.children.borrow().iter() {
        let NodeData::Element { name, .. } = &child.data else {
            continue;
        };
        let tag = name.local.as_ref().to_ascii_lowercase();
        if tag != "li" {
            continue;
        }
        let attrs = extract_attrs(child);
        let item_style =
            compute_element_style(child, &tag, &attrs, ancestors, list_style, ctx.stylesheet);
        if item_style.display == DisplayMode::None || item_style.visibility_hidden {
            continue;
        }
        let marker = if ordered {
            let m = format!("{idx}.");
            idx += 1;
            m
        } else {
            "•".to_string()
        };
        let id = attrs.get("id").cloned();
        let classes = split_classes_owned(attrs.get("class"));
        ancestors.push((tag, id, classes));
        let children = build_block_children(child, &item_style, ancestors, ctx, false);
        ancestors.pop();
        items.push(LayoutNode::ListItem {
            style: item_style,
            marker,
            children,
        });
    }
    items
}

fn build_image_node(
    attrs: &HashMap<String, String>,
    style: ComputedStyle,
    ctx: &mut BuildCtx<'_>,
) -> Option<LayoutNode> {
    let width = match style.width {
        Some(crate::css::CssLength::Px(px)) => Some(px),
        _ => attrs
            .get("width")
            .and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok()),
    };
    let height = match style.height {
        Some(crate::css::CssLength::Px(px)) => Some(px),
        _ => attrs
            .get("height")
            .and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok()),
    };

    // Skip 1x1 / 0px tracking pixels
    if width.is_some_and(|w| w <= 1.0) || height.is_some_and(|h| h <= 1.0) {
        return None;
    }

    let src = attrs.get("src").cloned();
    let blocked_src = attrs.get("data-blocked-src").cloned();
    if blocked_src.is_some() {
        ctx.blocked_image_count += 1;
    }
    let is_cid = src
        .as_deref()
        .is_some_and(|s| s.starts_with("blob://") || s.starts_with("cid:"));
    let alt = attrs.get("alt").cloned().unwrap_or_default();

    // Only keep safe non-remote src if blocking is enabled
    let safe_src = src.filter(|s| {
        !(ctx.opts.block_remote_images
            && (s.starts_with("http://") || s.starts_with("https://") || s.starts_with("//")))
    });

    if safe_src.is_none() && blocked_src.is_none() && alt.trim().is_empty() {
        return None;
    }

    Some(LayoutNode::Image {
        style,
        src: safe_src,
        blocked_src,
        alt,
        width,
        height,
        is_cid,
    })
}

fn extract_link_target(attrs: &HashMap<String, String>) -> Option<LinkTarget> {
    let href = attrs.get("href")?.trim();
    if href.is_empty() {
        return None;
    }
    let lower = href.to_ascii_lowercase();
    if lower.starts_with("javascript:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("data:")
    {
        return None;
    }
    let phishing_risk = attrs
        .get("data-phishing-risk")
        .is_some_and(|v| v == "flagged");
    Some(LinkTarget {
        href: href.to_string(),
        phishing_risk,
    })
}

fn collect_plain_text(node: &Handle) -> String {
    let mut out = String::new();
    match &node.data {
        NodeData::Text { contents } => out.push_str(&contents.borrow()),
        NodeData::Element { .. } => {
            for child in node.children.borrow().iter() {
                out.push_str(&collect_plain_text(child));
            }
        }
        _ => {}
    }
    out
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !last_ws {
                out.push(' ');
                last_ws = true;
            }
        } else {
            out.push(ch);
            last_ws = false;
        }
    }
    out
}

fn push_text_run(
    runs: &mut Vec<TextRunNode>,
    text: String,
    style: &ComputedStyle,
    link: Option<LinkTarget>,
) {
    if text.is_empty() {
        return;
    }
    let bold = style.is_bold();
    let underline = style.underline || link.is_some();
    if let Some(last) = runs.last_mut()
        && last.bold == bold
        && last.italic == style.italic
        && last.underline == underline
        && last.line_through == style.line_through
        && last.monospace == style.monospace
        && (last.font_size - style.font_size).abs() < 0.01
        && last.color == style.color
        && last.background_color == style.background_color
        && last.link == link
    {
        // Avoid double spaces across adjacent inline boundaries
        if last.text.ends_with(' ') && text.starts_with(' ') {
            last.text.push_str(text.trim_start_matches(' '));
        } else {
            last.text.push_str(&text);
        }
        return;
    }

    runs.push(TextRunNode {
        text,
        font_size: style.font_size,
        bold,
        italic: style.italic,
        underline,
        line_through: style.line_through,
        monospace: style.monospace,
        color: style.color,
        background_color: style.background_color,
        link,
    });
}

fn trim_edge_whitespace(runs: &mut Vec<TextRunNode>) {
    while let Some(first) = runs.first_mut() {
        let trimmed = first.text.trim_start_matches(' ');
        if trimmed.is_empty() {
            runs.remove(0);
        } else {
            if trimmed.len() != first.text.len() {
                first.text = trimmed.to_string();
            }
            break;
        }
    }
    while let Some(last) = runs.last_mut() {
        let trimmed = last.text.trim_end_matches(' ');
        if trimmed.is_empty() {
            runs.pop();
        } else {
            if trimmed.len() != last.text.len() {
                last.text = trimmed.to_string();
            }
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_newsletter_table_and_cta_button() {
        let html = r#"
        <html>
        <head>
            <style>
                .wrapper { background-color: #f4f4f5; padding: 24px; }
                .card { background-color: #ffffff; max-width: 600px; margin: 0 auto; border-radius: 8px; }
                .cta { display: inline-block; background-color: #2563eb; color: #ffffff; padding: 12px 24px; border-radius: 6px; }
                .preheader { display: none; }
            </style>
        </head>
        <body>
            <div class="preheader">Hidden preview text</div>
            <table class="wrapper" width="100%" cellpadding="16">
                <tr>
                    <td align="center">
                        <table class="card" width="600">
                            <tr>
                                <td>
                                    <h1>Welcome to Vespetrel</h1>
                                    <p>Hello <b>World</b>! <a href="https://example.com?utm_source=mail">Visit</a></p>
                                    <a class="cta" href="https://example.com/verify">Verify Account</a>
                                    <img src="https://cdn.example.com/banner.png" alt="Banner" width="600" height="200">
                                </td>
                            </tr>
                        </table>
                    </td>
                </tr>
            </table>
        </body>
        </html>
        "#;

        let doc = parse_email_html(html, &SanitizeOptions::default()).unwrap();
        assert_eq!(doc.blocked_image_count, 1);

        let preview = doc.plain_text_preview();
        assert!(!preview.contains("Hidden preview text"));
        assert!(preview.contains("Welcome to Vespetrel"));
        assert!(preview.contains("Hello World!"));
        assert!(preview.contains("[Verify Account]"));
        assert!(preview.contains("[Banner]"));
    }

    #[test]
    fn strips_scripts_and_javascript_links() {
        let html = r#"
            <script>alert(1)</script>
            <p><a href="javascript:alert(2)">Bad</a> and <a href="https://good.example">Good</a></p>
        "#;
        let doc = parse_email_html(html, &SanitizeOptions::default()).unwrap();
        let preview = doc.plain_text_preview();
        assert!(!preview.contains("alert"));
        assert!(preview.contains("Bad and Good"));
        let runs = match &doc.nodes[0] {
            LayoutNode::Container { children, .. } => match &children[0] {
                LayoutNode::RichText { runs, .. } => runs,
                other => panic!("expected RichText inside Container, got {other:?}"),
            },
            LayoutNode::RichText { runs, .. } => runs,
            other => panic!("expected Container/RichText node, got {other:?}"),
        };
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "Bad and ");
        assert!(runs[0].link.is_none());
        assert_eq!(runs[1].text, "Good");
        assert_eq!(
            runs[1].link.as_ref().map(|l| l.href.as_str()),
            Some("https://good.example")
        );
    }
}
