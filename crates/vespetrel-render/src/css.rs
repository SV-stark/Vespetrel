//! Clean-room CSS parser and cascade resolver for HTML email layout.
//!
//! Designed specifically for email HTML:
//! - Parses `<style>` blocks and inline `style="..."` declarations.
//! - Rejects dangerous constructs (`@import`, `expression()`, `javascript:`, `url(...)`).
//! - Maps legacy HTML presentational attributes (`bgcolor`, `align`, `valign`, `width`,
//!   `height`, `border`, `cellpadding`, `cellspacing`, `color`) into `ComputedStyle`.
//! - Resolves selector specificity `(id, class, tag)` and inheritance cleanly.

use std::collections::HashMap;

/// RGBA color in sRGB byte space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CssColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl CssColor {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn is_transparent(self) -> bool {
        self.a == 0
    }

    /// Convert to normalized `[r, g, b, a]` floats in `0.0..=1.0`.
    pub fn to_f32_array(self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }
}

/// CSS length / dimension value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CssLength {
    Px(f32),
    Percent(f32),
    Auto,
}

/// Four-sided edge values (top, right, bottom, left).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EdgeValues {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl EdgeValues {
    pub const fn all(v: f32) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayMode {
    #[default]
    Inline,
    Block,
    InlineBlock,
    Flex,
    Table,
    TableRow,
    TableCell,
    ListItem,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerticalAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderSide {
    pub width: f32,
    pub color: Option<CssColor>,
}

/// Resolved visual & box style for an email DOM element.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: DisplayMode,
    pub visibility_hidden: bool,
    pub color: Option<CssColor>,
    pub background_color: Option<CssColor>,
    pub font_size: f32,
    pub font_weight: u16,
    pub italic: bool,
    pub underline: bool,
    pub line_through: bool,
    pub monospace: bool,
    pub text_align: Option<TextAlign>,
    pub vertical_align: VerticalAlign,
    pub line_height: Option<f32>,
    pub width: Option<CssLength>,
    pub max_width: Option<CssLength>,
    pub min_width: Option<CssLength>,
    pub height: Option<CssLength>,
    pub padding: EdgeValues,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub margin_left: Option<CssLength>,
    pub margin_right: Option<CssLength>,
    pub border_top: Option<BorderSide>,
    pub border_right: Option<BorderSide>,
    pub border_bottom: Option<BorderSide>,
    pub border_left: Option<BorderSide>,
    pub border_radius: f32,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            display: DisplayMode::Inline,
            visibility_hidden: false,
            color: None,
            background_color: None,
            font_size: 14.0,
            font_weight: 400,
            italic: false,
            underline: false,
            line_through: false,
            monospace: false,
            text_align: None,
            vertical_align: VerticalAlign::Top,
            line_height: None,
            width: None,
            max_width: None,
            min_width: None,
            height: None,
            padding: EdgeValues::default(),
            margin_top: 0.0,
            margin_bottom: 0.0,
            margin_left: None,
            margin_right: None,
            border_top: None,
            border_right: None,
            border_bottom: None,
            border_left: None,
            border_radius: 0.0,
        }
    }
}

impl ComputedStyle {
    /// Create a child style inheriting only CSS-inheritable properties from `parent`.
    pub fn inherit_from(parent: &Self, default_display: DisplayMode) -> Self {
        Self {
            display: default_display,
            visibility_hidden: parent.visibility_hidden,
            color: parent.color,
            font_size: parent.font_size,
            font_weight: parent.font_weight,
            italic: parent.italic,
            underline: parent.underline,
            line_through: parent.line_through,
            monospace: parent.monospace,
            text_align: parent.text_align,
            line_height: parent.line_height,
            ..Self::default()
        }
    }

    pub fn is_bold(&self) -> bool {
        self.font_weight >= 600
    }

    pub fn is_horizontally_centered(&self) -> bool {
        matches!(self.margin_left, Some(CssLength::Auto))
            && matches!(self.margin_right, Some(CssLength::Auto))
    }

    pub fn set_all_borders(&mut self, side: Option<BorderSide>) {
        self.border_top = side;
        self.border_right = side;
        self.border_bottom = side;
        self.border_left = side;
    }
}

/// A parsed CSS declaration (`property: value [!important]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub property: String,
    pub value: String,
    pub important: bool,
}

/// A single compound selector segment (e.g. `td.header#main`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleSelector {
    pub tag: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
}

impl SimpleSelector {
    fn specificity(&self) -> (u16, u16, u16) {
        (
            u16::from(self.id.is_some()),
            self.classes.len() as u16,
            u16::from(self.tag.is_some()),
        )
    }

    pub fn matches(&self, tag: &str, id: Option<&str>, classes: &[&str]) -> bool {
        if let Some(ref req_tag) = self.tag
            && req_tag != "*"
            && !req_tag.eq_ignore_ascii_case(tag)
        {
            return false;
        }
        if let Some(ref req_id) = self.id {
            match id {
                Some(el_id) if el_id == req_id => {}
                _ => return false,
            }
        }
        for req_cls in &self.classes {
            if !classes.iter().any(|c| c == req_cls) {
                return false;
            }
        }
        true
    }
}

/// A selector chain (e.g. `table.wrapper td.content`), stored right-to-left or left-to-right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    /// Sequence of simple selectors from ancestor to target, with `direct_child` flag
    /// indicating whether `>` combinator was used before this segment.
    pub parts: Vec<(SimpleSelector, bool)>,
}

impl Selector {
    pub fn specificity(&self) -> (u16, u16, u16) {
        self.parts.iter().fold((0, 0, 0), |acc, (seg, _)| {
            let s = seg.specificity();
            (acc.0 + s.0, acc.1 + s.1, acc.2 + s.2)
        })
    }

    /// Match against an element and its ancestor stack (`ancestors[0]` = root, `ancestors.last()` = direct parent).
    pub fn matches_element(
        &self,
        tag: &str,
        id: Option<&str>,
        classes: &[&str],
        ancestors: &[ElementMeta<'_>],
    ) -> bool {
        let Some((target, direct_child)) = self.parts.last() else {
            return false;
        };
        if !target.matches(tag, id, classes) {
            return false;
        }
        if self.parts.len() == 1 {
            return true;
        }

        let mut anc_idx = ancestors.len();
        let mut need_direct = *direct_child;
        for (seg, seg_direct) in self.parts[..self.parts.len() - 1].iter().rev() {
            if need_direct {
                if anc_idx == 0 {
                    return false;
                }
                anc_idx -= 1;
                let anc = &ancestors[anc_idx];
                if !seg.matches(anc.tag, anc.id, anc.classes) {
                    return false;
                }
            } else {
                let mut found = false;
                while anc_idx > 0 {
                    anc_idx -= 1;
                    let anc = &ancestors[anc_idx];
                    if seg.matches(anc.tag, anc.id, anc.classes) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return false;
                }
            }
            need_direct = *seg_direct;
        }
        true
    }
}

/// Lightweight element metadata passed during selector matching.
#[derive(Debug, Clone, Copy)]
pub struct ElementMeta<'a> {
    pub tag: &'a str,
    pub id: Option<&'a str>,
    pub classes: &'a [&'a str],
}

#[derive(Debug, Clone)]
pub struct CssRule {
    pub selector: Selector,
    pub declarations: Vec<Declaration>,
    pub order: usize,
}

/// Parsed stylesheet from `<style>` blocks in an email.
#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    pub rules: Vec<CssRule>,
}

impl Stylesheet {
    /// Parse a CSS stylesheet string, discarding comments and `@`-rules safely.
    pub fn parse(css: &str) -> Self {
        let stripped = strip_css_comments(css);
        let mut rules = Vec::new();
        let mut order = 0usize;
        let bytes = stripped.as_bytes();
        let mut i = 0usize;

        while i < bytes.len() {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }

            // Skip @-rules safely (e.g. @media, @font-face, @import, @keyframes)
            if bytes[i] == b'@' {
                i = skip_at_rule(&stripped, i);
                continue;
            }

            let Some(open_brace) = stripped[i..].find('{') else {
                break;
            };
            let selector_str = stripped[i..i + open_brace].trim();
            let body_start = i + open_brace + 1;
            let Some(close_brace) = find_matching_brace(&stripped, body_start) else {
                break;
            };
            let decl_str = &stripped[body_start..close_brace];
            let declarations = parse_declarations(decl_str);

            if !declarations.is_empty() {
                for raw_sel in selector_str.split(',') {
                    if let Some(selector) = parse_selector(raw_sel.trim()) {
                        rules.push(CssRule {
                            selector,
                            declarations: declarations.clone(),
                            order,
                        });
                        order += 1;
                    }
                }
            }
            i = close_brace + 1;
        }

        Self { rules }
    }

    pub fn merge(&mut self, other: Self) {
        let offset = self.rules.len();
        for mut r in other.rules {
            r.order += offset;
            self.rules.push(r);
        }
    }
}

fn strip_css_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        if let Some(end) = rest[start + 2..].find("*/") {
            rest = &rest[start + 2 + end + 2..];
        } else {
            return out;
        }
    }
    out.push_str(rest);
    out
}

fn skip_at_rule(s: &str, start: usize) -> usize {
    let bytes = s.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b';' => return i + 1,
            b'{' => {
                if let Some(end) = find_matching_brace(s, i + 1) {
                    return end + 1;
                }
                return bytes.len();
            }
            _ => i += 1,
        }
    }
    bytes.len()
}

fn find_matching_brace(s: &str, after_open: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 1usize;
    let mut i = after_open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_selector(raw: &str) -> Option<Selector> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // Reject pseudo-elements/complex pseudo-selectors we don't model (except :link/:visited)
    let mut parts = Vec::new();
    let mut next_direct = false;

    for token in raw.split_whitespace() {
        if token == ">" {
            next_direct = true;
            continue;
        }
        if token == "+" || token == "~" {
            return None;
        }
        let seg = parse_simple_selector(token)?;
        parts.push((seg, next_direct));
        next_direct = false;
    }

    if parts.is_empty() {
        None
    } else {
        Some(Selector { parts })
    }
}

fn parse_simple_selector(token: &str) -> Option<SimpleSelector> {
    // Strip benign pseudo-classes like :link, :visited; reject :hover/:before/:after
    let base = if let Some(idx) = token.find(':') {
        let pseudo = &token[idx..];
        if pseudo.eq_ignore_ascii_case(":link") || pseudo.eq_ignore_ascii_case(":visited") {
            &token[..idx]
        } else {
            return None;
        }
    } else {
        token
    };
    if base.is_empty() {
        return None;
    }

    let mut tag: Option<String> = None;
    let mut id: Option<String> = None;
    let mut classes = Vec::new();

    let mut cur = base;
    if !cur.starts_with('.') && !cur.starts_with('#') {
        let end = cur.find(['.', '#']).unwrap_or(cur.len());
        let t = cur[..end].trim();
        if !t.is_empty() {
            tag = Some(t.to_ascii_lowercase());
        }
        cur = &cur[end..];
    }

    while !cur.is_empty() {
        let prefix = cur.as_bytes()[0];
        let rest = &cur[1..];
        let end = rest.find(['.', '#']).unwrap_or(rest.len());
        let name = rest[..end].trim();
        if name.is_empty() {
            return None;
        }
        match prefix {
            b'#' => id = Some(name.to_string()),
            b'.' => classes.push(name.to_string()),
            _ => return None,
        }
        cur = &rest[end..];
    }

    Some(SimpleSelector { tag, id, classes })
}

fn is_dangerous_css_value(val: &str) -> bool {
    let lower = val.to_ascii_lowercase();
    lower.contains("url(")
        || lower.contains("expression(")
        || lower.contains("javascript:")
        || lower.contains("@import")
        || lower.contains("-moz-binding")
        || lower.contains("behavior:")
}

/// Parse inline `style="..."` or rule body declarations.
pub fn parse_declarations(raw: &str) -> Vec<Declaration> {
    let mut out = Vec::new();
    for item in raw.split(';') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let Some((prop, val)) = item.split_once(':') else {
            continue;
        };
        let prop = prop.trim().to_ascii_lowercase();
        let mut val = val.trim();
        if prop.is_empty() || val.is_empty() || is_dangerous_css_value(val) {
            continue;
        }
        let mut important = false;
        if let Some(stripped) = val.strip_suffix("!important") {
            important = true;
            val = stripped.trim();
        } else if val.to_ascii_lowercase().ends_with("!important") {
            important = true;
            val = val[..val.len() - "!important".len()].trim();
        }
        if !val.is_empty() {
            out.push(Declaration {
                property: prop,
                value: val.to_string(),
                important,
            });
        }
    }
    out
}

type MatchedRule<'a> = ((u16, u16, u16), usize, &'a Vec<Declaration>);

/// Compute final style for an element from inherited parent style, HTML attributes,
/// matching stylesheet rules, and inline `style="..."`.
#[allow(clippy::too_many_arguments)]
pub fn compute_style(
    tag: &str,
    id: Option<&str>,
    classes: &[&str],
    attrs: &HashMap<String, String>,
    ancestors: &[ElementMeta<'_>],
    parent_style: &ComputedStyle,
    stylesheet: &Stylesheet,
    default_display: DisplayMode,
) -> ComputedStyle {
    let mut style = ComputedStyle::inherit_from(parent_style, default_display);

    // 1. HTML presentational attributes (lowest specificity)
    apply_presentational_attrs(&mut style, tag, attrs, parent_style.font_size);

    // 2. Matching stylesheet rules sorted by (specificity, source order)
    let mut matched: Vec<MatchedRule<'_>> = stylesheet
        .rules
        .iter()
        .filter(|r| r.selector.matches_element(tag, id, classes, ancestors))
        .map(|r| (r.selector.specificity(), r.order, &r.declarations))
        .collect();
    matched.sort_by_key(|(spec, order, _)| (*spec, *order));

    let inline_decls = attrs
        .get("style")
        .map(|s| parse_declarations(s))
        .unwrap_or_default();

    // Normal declarations from rules, then normal inline, then !important rules, then !important inline
    for (_, _, decls) in &matched {
        for d in decls.iter().filter(|d| !d.important) {
            apply_declaration(&mut style, d, parent_style.font_size);
        }
    }
    for d in inline_decls.iter().filter(|d| !d.important) {
        apply_declaration(&mut style, d, parent_style.font_size);
    }
    for (_, _, decls) in &matched {
        for d in decls.iter().filter(|d| d.important) {
            apply_declaration(&mut style, d, parent_style.font_size);
        }
    }
    for d in inline_decls.iter().filter(|d| d.important) {
        apply_declaration(&mut style, d, parent_style.font_size);
    }

    style
}

fn apply_presentational_attrs(
    style: &mut ComputedStyle,
    tag: &str,
    attrs: &HashMap<String, String>,
    base_font: f32,
) {
    if let Some(bgcolor) = attrs.get("bgcolor")
        && let Some(c) = parse_color(bgcolor)
    {
        style.background_color = Some(c);
    }
    if tag == "font"
        && let Some(color) = attrs.get("color")
        && let Some(c) = parse_color(color)
    {
        style.color = Some(c);
    }
    if let Some(align) = attrs.get("align") {
        match align.trim().to_ascii_lowercase().as_str() {
            "left" => style.text_align = Some(TextAlign::Left),
            "center" | "middle" => style.text_align = Some(TextAlign::Center),
            "right" => style.text_align = Some(TextAlign::Right),
            "justify" => style.text_align = Some(TextAlign::Justify),
            _ => {}
        }
    }
    if let Some(valign) = attrs.get("valign") {
        match valign.trim().to_ascii_lowercase().as_str() {
            "top" => style.vertical_align = VerticalAlign::Top,
            "middle" | "center" => style.vertical_align = VerticalAlign::Middle,
            "bottom" => style.vertical_align = VerticalAlign::Bottom,
            _ => {}
        }
    }
    if let Some(w) = attrs.get("width")
        && let Some(len) = parse_length(w, base_font)
    {
        style.width = Some(len);
    }
    if let Some(h) = attrs.get("height")
        && let Some(len) = parse_length(h, base_font)
    {
        style.height = Some(len);
    }
    if tag == "table"
        && let Some(border) = attrs.get("border")
        && let Ok(w) = border.trim().parse::<f32>()
        && w > 0.0
    {
        style.set_all_borders(Some(BorderSide {
            width: w.min(8.0),
            color: Some(CssColor::rgb(200, 200, 200)),
        }));
    }
    if attrs.get("hidden").is_some()
        || attrs
            .get("aria-hidden")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    {
        style.display = DisplayMode::None;
    }
}

fn apply_declaration(style: &mut ComputedStyle, decl: &Declaration, base_font: f32) {
    let val = decl.value.trim();
    let lower = val.to_ascii_lowercase();

    match decl.property.as_str() {
        "display" => {
            style.display = match lower.as_str() {
                "none" => DisplayMode::None,
                "block" => DisplayMode::Block,
                "inline" => DisplayMode::Inline,
                "inline-block" => DisplayMode::InlineBlock,
                "flex" | "inline-flex" => DisplayMode::Flex,
                "table" => DisplayMode::Table,
                "table-row" => DisplayMode::TableRow,
                "table-cell" => DisplayMode::TableCell,
                "list-item" => DisplayMode::ListItem,
                _ => style.display,
            };
        }
        "visibility" => {
            style.visibility_hidden = matches!(lower.as_str(), "hidden" | "collapse");
        }
        "mso-hide" if lower == "all" => {
            style.display = DisplayMode::None;
        }
        "color" => {
            if let Some(c) = parse_color(val) {
                style.color = Some(c);
            }
        }
        "background-color" => {
            if let Some(c) = parse_color(val) {
                style.background_color = Some(c);
            }
        }
        "background" => {
            // Extract first color token from `background` shorthand if present
            for token in val.split_whitespace() {
                if let Some(c) = parse_color(token) {
                    style.background_color = Some(c);
                    break;
                }
            }
        }
        "font-size" => {
            if let Some(px) = parse_font_size(val, base_font) {
                style.font_size = px.clamp(8.0, 64.0);
            }
        }
        "font-weight" => match lower.as_str() {
            "bold" | "bolder" => style.font_weight = 700,
            "normal" | "lighter" => style.font_weight = 400,
            _ => {
                if let Ok(w) = lower.parse::<u16>() {
                    style.font_weight = w.clamp(100, 900);
                }
            }
        },
        "font-style" => {
            style.italic = matches!(lower.as_str(), "italic" | "oblique");
        }
        "font-family" => {
            style.monospace = lower.contains("monospace")
                || lower.contains("consolas")
                || lower.contains("courier")
                || lower.contains("menlo")
                || lower.contains("monaco");
        }
        "text-decoration" | "text-decoration-line" => {
            if lower == "none" {
                style.underline = false;
                style.line_through = false;
            } else {
                if lower.contains("underline") {
                    style.underline = true;
                }
                if lower.contains("line-through") {
                    style.line_through = true;
                }
            }
        }
        "text-align" => {
            style.text_align = match lower.as_str() {
                "left" | "start" => Some(TextAlign::Left),
                "center" => Some(TextAlign::Center),
                "right" | "end" => Some(TextAlign::Right),
                "justify" => Some(TextAlign::Justify),
                _ => style.text_align,
            };
        }
        "vertical-align" => {
            style.vertical_align = match lower.as_str() {
                "top" => VerticalAlign::Top,
                "middle" => VerticalAlign::Middle,
                "bottom" => VerticalAlign::Bottom,
                _ => style.vertical_align,
            };
        }
        "line-height" => {
            if let Some(lh) = parse_line_height(val, style.font_size) {
                style.line_height = Some(lh);
            }
        }
        "width" => {
            if let Some(len) = parse_length(val, base_font) {
                style.width = Some(len);
            }
        }
        "max-width" => {
            if let Some(len) = parse_length(val, base_font) {
                style.max_width = Some(len);
            }
        }
        "min-width" => {
            if let Some(len) = parse_length(val, base_font) {
                style.min_width = Some(len);
            }
        }
        "height" => {
            if let Some(len) = parse_length(val, base_font) {
                style.height = Some(len);
            }
        }
        "padding" => {
            if let Some(edges) = parse_edge_lengths(val, base_font) {
                style.padding = edges;
            }
        }
        "padding-top" => {
            if let Some(CssLength::Px(px)) = parse_length(val, base_font) {
                style.padding.top = px.max(0.0);
            }
        }
        "padding-right" => {
            if let Some(CssLength::Px(px)) = parse_length(val, base_font) {
                style.padding.right = px.max(0.0);
            }
        }
        "padding-bottom" => {
            if let Some(CssLength::Px(px)) = parse_length(val, base_font) {
                style.padding.bottom = px.max(0.0);
            }
        }
        "padding-left" => {
            if let Some(CssLength::Px(px)) = parse_length(val, base_font) {
                style.padding.left = px.max(0.0);
            }
        }
        "margin" => {
            apply_margin_shorthand(style, val, base_font);
        }
        "margin-top" => {
            if let Some(CssLength::Px(px)) = parse_length(val, base_font) {
                style.margin_top = px.max(0.0);
            }
        }
        "margin-bottom" => {
            if let Some(CssLength::Px(px)) = parse_length(val, base_font) {
                style.margin_bottom = px.max(0.0);
            }
        }
        "margin-left" => {
            if let Some(len) = parse_length(val, base_font) {
                style.margin_left = Some(len);
            }
        }
        "margin-right" => {
            if let Some(len) = parse_length(val, base_font) {
                style.margin_right = Some(len);
            }
        }
        "border" => {
            style.set_all_borders(parse_border_side(val, base_font));
        }
        "border-top" => style.border_top = parse_border_side(val, base_font),
        "border-right" => style.border_right = parse_border_side(val, base_font),
        "border-bottom" => style.border_bottom = parse_border_side(val, base_font),
        "border-left" => style.border_left = parse_border_side(val, base_font),
        "border-radius" => {
            if let Some(first) = val.split_whitespace().next()
                && let Some(CssLength::Px(px)) = parse_length(first, base_font)
            {
                style.border_radius = px.clamp(0.0, 64.0);
            }
        }
        _ => {}
    }
}

fn apply_margin_shorthand(style: &mut ComputedStyle, val: &str, base_font: f32) {
    let parts: Vec<CssLength> = val
        .split_whitespace()
        .filter_map(|t| parse_length(t, base_font))
        .collect();
    let (top, right, bottom, left) = match parts.as_slice() {
        [a] => (*a, *a, *a, *a),
        [v, h] => (*v, *h, *v, *h),
        [t, h, b] => (*t, *h, *b, *h),
        [t, r, b, l, ..] => (*t, *r, *b, *l),
        [] => return,
    };
    if let CssLength::Px(px) = top {
        style.margin_top = px.max(0.0);
    }
    if let CssLength::Px(px) = bottom {
        style.margin_bottom = px.max(0.0);
    }
    style.margin_left = Some(left);
    style.margin_right = Some(right);
}

fn parse_edge_lengths(val: &str, base_font: f32) -> Option<EdgeValues> {
    let nums: Vec<f32> = val
        .split_whitespace()
        .filter_map(|t| match parse_length(t, base_font)? {
            CssLength::Px(px) => Some(px.max(0.0)),
            _ => Some(0.0),
        })
        .collect();
    Some(match nums.as_slice() {
        [a] => EdgeValues::all(*a),
        [v, h] => EdgeValues {
            top: *v,
            right: *h,
            bottom: *v,
            left: *h,
        },
        [t, h, b] => EdgeValues {
            top: *t,
            right: *h,
            bottom: *b,
            left: *h,
        },
        [t, r, b, l, ..] => EdgeValues {
            top: *t,
            right: *r,
            bottom: *b,
            left: *l,
        },
        [] => return None,
    })
}

fn parse_border_side(val: &str, base_font: f32) -> Option<BorderSide> {
    let lower = val.trim().to_ascii_lowercase();
    if lower == "none" || lower == "0" || lower == "0px" {
        return None;
    }
    let mut width = 1.0f32;
    let mut color = None;
    for tok in val.split_whitespace() {
        let t_low = tok.to_ascii_lowercase();
        if matches!(
            t_low.as_str(),
            "solid" | "dashed" | "dotted" | "double" | "groove" | "ridge"
        ) {
            continue;
        }
        if t_low == "none" || t_low == "hidden" {
            return None;
        }
        if let Some(CssLength::Px(px)) = parse_length(tok, base_font) {
            width = px.clamp(0.5, 12.0);
        } else if let Some(c) = parse_color(tok) {
            color = Some(c);
        }
    }
    Some(BorderSide { width, color })
}

fn parse_font_size(val: &str, base_font: f32) -> Option<f32> {
    let lower = val.trim().to_ascii_lowercase();
    match lower.as_str() {
        "xx-small" => return Some(9.0),
        "x-small" => return Some(10.0),
        "small" => return Some(12.0),
        "medium" => return Some(14.0),
        "large" => return Some(16.0),
        "x-large" => return Some(20.0),
        "xx-large" => return Some(24.0),
        _ => {}
    }
    match parse_length(val, base_font)? {
        CssLength::Px(px) => Some(px),
        CssLength::Percent(pct) => Some(base_font * (pct / 100.0)),
        CssLength::Auto => None,
    }
}

fn parse_line_height(val: &str, font_size: f32) -> Option<f32> {
    let trimmed = val.trim();
    if let Ok(unitless) = trimmed.parse::<f32>() {
        return Some((unitless * font_size).clamp(10.0, 96.0));
    }
    match parse_length(trimmed, font_size)? {
        CssLength::Px(px) => Some(px.clamp(10.0, 96.0)),
        CssLength::Percent(pct) => Some((font_size * pct / 100.0).clamp(10.0, 96.0)),
        CssLength::Auto => None,
    }
}

pub fn parse_length(val: &str, base_font: f32) -> Option<CssLength> {
    let s = val.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    if s == "auto" {
        return Some(CssLength::Auto);
    }
    if let Some(num) = s.strip_suffix('%') {
        return num.trim().parse::<f32>().ok().map(CssLength::Percent);
    }
    if let Some(num) = s.strip_suffix("px") {
        return num.trim().parse::<f32>().ok().map(CssLength::Px);
    }
    if let Some(num) = s.strip_suffix("pt") {
        return num
            .trim()
            .parse::<f32>()
            .ok()
            .map(|pt| CssLength::Px(pt * 4.0 / 3.0));
    }
    if let Some(num) = s.strip_suffix("rem") {
        return num
            .trim()
            .parse::<f32>()
            .ok()
            .map(|r| CssLength::Px(r * 14.0));
    }
    if let Some(num) = s.strip_suffix("em") {
        return num
            .trim()
            .parse::<f32>()
            .ok()
            .map(|e| CssLength::Px(e * base_font));
    }
    s.parse::<f32>().ok().map(CssLength::Px)
}

pub fn parse_color(val: &str) -> Option<CssColor> {
    let s = val.trim();
    if s.is_empty() {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    if lower == "transparent" {
        return Some(CssColor::rgba(0, 0, 0, 0));
    }
    if let Some(hex) = lower.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    if let Some(inner) = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))
        .and_then(|r| r.strip_suffix(')'))
    {
        return parse_rgb_func(inner);
    }
    parse_named_color(&lower)
}

fn parse_hex_color(hex: &str) -> Option<CssColor> {
    let b = hex.as_bytes();
    match b.len() {
        3 => {
            let r = hex_nibble(b[0])? * 17;
            let g = hex_nibble(b[1])? * 17;
            let bl = hex_nibble(b[2])? * 17;
            Some(CssColor::rgb(r, g, bl))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let bl = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(CssColor::rgb(r, g, bl))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let bl = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(CssColor::rgba(r, g, bl, a))
        }
        _ => None,
    }
}

fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn parse_rgb_func(inner: &str) -> Option<CssColor> {
    let normalized = inner.replace('/', " ");
    let parts: Vec<&str> = if normalized.contains(',') {
        normalized.split(',').map(|s| s.trim()).collect()
    } else {
        normalized.split_whitespace().collect()
    };
    if parts.len() < 3 {
        return None;
    }
    let parse_chan = |s: &str| -> Option<u8> {
        if let Some(p) = s.strip_suffix('%') {
            let v = p.trim().parse::<f32>().ok()?;
            Some(((v.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u8)
        } else {
            let v = s.parse::<f32>().ok()?;
            Some(v.clamp(0.0, 255.0).round() as u8)
        }
    };
    let r = parse_chan(parts[0])?;
    let g = parse_chan(parts[1])?;
    let b = parse_chan(parts[2])?;
    let a = if let Some(alpha_str) = parts.get(3) {
        if let Some(p) = alpha_str.strip_suffix('%') {
            let v = p.trim().parse::<f32>().ok()?;
            ((v.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u8
        } else {
            let v = alpha_str.parse::<f32>().ok()?;
            (v.clamp(0.0, 1.0) * 255.0).round() as u8
        }
    } else {
        255
    };
    Some(CssColor::rgba(r, g, b, a))
}

fn parse_named_color(name: &str) -> Option<CssColor> {
    Some(match name {
        "black" => CssColor::rgb(0, 0, 0),
        "white" => CssColor::rgb(255, 255, 255),
        "gray" | "grey" => CssColor::rgb(128, 128, 128),
        "darkgray" | "darkgrey" => CssColor::rgb(169, 169, 169),
        "lightgray" | "lightgrey" => CssColor::rgb(211, 211, 211),
        "dimgray" | "dimgrey" => CssColor::rgb(105, 105, 105),
        "silver" => CssColor::rgb(192, 192, 192),
        "whitesmoke" => CssColor::rgb(245, 245, 245),
        "snow" => CssColor::rgb(255, 250, 250),
        "ivory" => CssColor::rgb(255, 255, 240),
        "beige" => CssColor::rgb(245, 245, 220),
        "linen" => CssColor::rgb(250, 240, 230),
        "aliceblue" => CssColor::rgb(240, 248, 255),
        "red" => CssColor::rgb(255, 0, 0),
        "crimson" => CssColor::rgb(220, 20, 60),
        "firebrick" => CssColor::rgb(178, 34, 34),
        "maroon" => CssColor::rgb(128, 0, 0),
        "coral" => CssColor::rgb(255, 127, 80),
        "orange" => CssColor::rgb(255, 165, 0),
        "gold" => CssColor::rgb(255, 215, 0),
        "yellow" => CssColor::rgb(255, 255, 0),
        "olive" => CssColor::rgb(128, 128, 0),
        "green" => CssColor::rgb(0, 128, 0),
        "lime" => CssColor::rgb(0, 255, 0),
        "forestgreen" => CssColor::rgb(34, 139, 34),
        "seagreen" => CssColor::rgb(46, 139, 87),
        "teal" => CssColor::rgb(0, 128, 128),
        "aqua" | "cyan" => CssColor::rgb(0, 255, 255),
        "blue" => CssColor::rgb(0, 0, 255),
        "navy" => CssColor::rgb(0, 0, 128),
        "midnightblue" => CssColor::rgb(25, 25, 112),
        "royalblue" => CssColor::rgb(65, 105, 225),
        "dodgerblue" => CssColor::rgb(30, 144, 255),
        "steelblue" => CssColor::rgb(70, 130, 180),
        "skyblue" => CssColor::rgb(135, 206, 235),
        "slategray" | "slategrey" => CssColor::rgb(112, 128, 144),
        "purple" => CssColor::rgb(128, 0, 128),
        "indigo" => CssColor::rgb(75, 0, 130),
        "rebeccapurple" => CssColor::rgb(102, 51, 153),
        "violet" => CssColor::rgb(238, 130, 238),
        "magenta" | "fuchsia" => CssColor::rgb(255, 0, 255),
        "pink" => CssColor::rgb(255, 192, 203),
        "brown" => CssColor::rgb(165, 42, 42),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stylesheet_and_specificity() {
        let css = r#"
            /* header comment */
            @media screen { .ignored { color: red; } }
            body { color: #111827; font-size: 15px; }
            .btn { background-color: #2563eb; color: white; padding: 10px 20px; border-radius: 6px; }
            table.card td.cell { padding: 12px; }
        "#;
        let sheet = Stylesheet::parse(css);
        assert_eq!(sheet.rules.len(), 3);
        assert_eq!(sheet.rules[1].selector.specificity(), (0, 1, 0));
        assert_eq!(sheet.rules[2].selector.specificity(), (0, 2, 2));
    }

    #[test]
    fn blocks_dangerous_css_values() {
        let decls = parse_declarations(
            "background: url(https://evil.com/track.png); color: #123456; width: expression(alert(1));",
        );
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].property, "color");
        assert_eq!(decls[0].value, "#123456");
    }

    #[test]
    fn cascades_inheritance_rules_and_inline() {
        let sheet = Stylesheet::parse("p.lead { color: #333333; font-size: 18px; }");
        let parent = ComputedStyle {
            color: Some(CssColor::rgb(0, 0, 0)),
            font_size: 14.0,
            ..Default::default()
        };
        let mut attrs = HashMap::new();
        attrs.insert(
            "style".to_string(),
            "color: #ff0000; margin: 0 auto;".to_string(),
        );

        let computed = compute_style(
            "p",
            None,
            &["lead"],
            &attrs,
            &[],
            &parent,
            &sheet,
            DisplayMode::Block,
        );
        assert_eq!(computed.color, Some(CssColor::rgb(255, 0, 0)));
        assert_eq!(computed.font_size, 18.0);
        assert!(computed.is_horizontally_centered());
    }

    #[test]
    fn parses_colors_and_lengths() {
        assert_eq!(parse_color("#fff"), Some(CssColor::rgb(255, 255, 255)));
        assert_eq!(
            parse_color("rgb(10, 20, 30)"),
            Some(CssColor::rgb(10, 20, 30))
        );
        assert_eq!(
            parse_color("rgba(255, 0, 0, 0.5)"),
            Some(CssColor::rgba(255, 0, 0, 128))
        );
        assert_eq!(parse_length("12pt", 14.0), Some(CssLength::Px(16.0)));
        assert_eq!(parse_length("50%", 14.0), Some(CssLength::Percent(50.0)));
        assert_eq!(parse_length("auto", 14.0), Some(CssLength::Auto));
    }
}
