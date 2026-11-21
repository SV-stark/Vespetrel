//! Clean-room GPUI native renderer for `vespetrel_render::layout::EmailDocument`.
//!
//! Maps the pre-computed, sanitized `EmailDocument` layout IR to native GPUI
//! elements (`div`, `InteractiveText` + `StyledText`, native table flex rows,
//! CTA buttons, and blocked-image placeholders) using `cx.theme()` semantic
//! tokens as fallbacks when email CSS does not specify explicit colors.

use std::ops::Range;
use std::sync::Arc;

use base64::Engine as _;
use gpui_kit::component::theme::{ActiveTheme, ThemeMode};
use gpui_kit::gpui::{
    AnyElement, App, Div, ElementId, Font, FontStyle, FontWeight, Hsla, Image, ImageFormat,
    ImageSource, InteractiveElement, InteractiveText, IntoElement, ObjectFit, ParentElement, Rgba,
    SharedString, StatefulInteractiveElement, StrikethroughStyle, Styled, StyledImage, StyledText,
    TextRun, UnderlineStyle, div, img, px,
};
use vespetrel_render::css::{ComputedStyle, CssColor, CssLength, TextAlign, VerticalAlign};
use vespetrel_render::layout::{EmailDocument, LayoutNode, LinkTarget, TableCellNode, TextRunNode};

use crate::views::message_viewer::AttachmentInfo;

/// Render a pre-laid-out `EmailDocument` into a GPUI `AnyElement`.
pub fn render_email_document(
    doc: &EmailDocument,
    attachments: &[AttachmentInfo],
    cx: &App,
) -> AnyElement {
    let mut counter = 0usize;
    let mut root = div().w_full().flex().flex_col();
    let has_explicit_bg = doc
        .body_style
        .background_color
        .is_some_and(|c| !c.is_transparent());
    apply_box_style(&mut root, &doc.body_style, cx);

    for node in &doc.nodes {
        root = root.child(render_node(
            node,
            &mut counter,
            attachments,
            has_explicit_bg,
            cx,
        ));
    }
    root.into_any_element()
}

fn css_to_hsla(c: CssColor) -> Hsla {
    let [r, g, b, a] = c.to_f32_array();
    Rgba { r, g, b, a }.into()
}

fn adapt_fg_for_theme(c: CssColor, has_explicit_bg: bool, cx: &App) -> Hsla {
    if !has_explicit_bg && cx.theme().mode == ThemeMode::Dark {
        let [r, g, b, a] = c.to_f32_array();
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        if luma < 0.28 {
            // Boost dark text on transparent background so it stays readable in dark mode
            let boost = (0.88 - luma).clamp(0.45, 0.85);
            return Rgba {
                r: (r + boost).min(0.94),
                g: (g + boost).min(0.94),
                b: (b + boost).min(0.96),
                a,
            }
            .into();
        }
    }
    css_to_hsla(c)
}

fn apply_box_style(el: &mut Div, style: &ComputedStyle, cx: &App) {
    if let Some(bg) = style.background_color
        && !bg.is_transparent()
    {
        el.style().background = Some(css_to_hsla(bg).into());
    }

    if let Some(CssLength::Px(w)) = style.width {
        el.style().size.width = Some(px(w).into());
    } else if let Some(CssLength::Percent(pct)) = style.width
        && pct >= 99.0
    {
        el.style().size.width = Some(gpui_kit::gpui::relative(1.0).into());
    }

    if let Some(CssLength::Px(mw)) = style.max_width {
        el.style().max_size.width = Some(px(mw).into());
    }
    if let Some(CssLength::Px(min_w)) = style.min_width {
        el.style().min_size.width = Some(px(min_w).into());
    }

    if style.padding.top > 0.0 {
        el.style().padding.top = Some(px(style.padding.top).into());
    }
    if style.padding.right > 0.0 {
        el.style().padding.right = Some(px(style.padding.right).into());
    }
    if style.padding.bottom > 0.0 {
        el.style().padding.bottom = Some(px(style.padding.bottom).into());
    }
    if style.padding.left > 0.0 {
        el.style().padding.left = Some(px(style.padding.left).into());
    }

    if style.margin_top > 0.0 {
        el.style().margin.top = Some(px(style.margin_top).into());
    }
    if style.margin_bottom > 0.0 {
        el.style().margin.bottom = Some(px(style.margin_bottom).into());
    }

    let default_border: Hsla = cx.theme().border;
    if let Some(b) = style.border_top {
        el.style().border_widths.top = Some(px(b.width).into());
        el.style().border_color = Some(b.color.map(css_to_hsla).unwrap_or(default_border));
    }
    if let Some(b) = style.border_right {
        el.style().border_widths.right = Some(px(b.width).into());
        el.style().border_color = Some(b.color.map(css_to_hsla).unwrap_or(default_border));
    }
    if let Some(b) = style.border_bottom {
        el.style().border_widths.bottom = Some(px(b.width).into());
        el.style().border_color = Some(b.color.map(css_to_hsla).unwrap_or(default_border));
    }
    if let Some(b) = style.border_left {
        el.style().border_widths.left = Some(px(b.width).into());
        el.style().border_color = Some(b.color.map(css_to_hsla).unwrap_or(default_border));
    }

    if style.border_radius > 0.0 {
        let r = px(style.border_radius);
        el.style().corner_radii.top_left = Some(r.into());
        el.style().corner_radii.top_right = Some(r.into());
        el.style().corner_radii.bottom_left = Some(r.into());
        el.style().corner_radii.bottom_right = Some(r.into());
    }
}

fn wrap_if_centered(inner: Div, style: &ComputedStyle) -> Div {
    if style.is_horizontally_centered()
        || matches!(style.text_align, Some(TextAlign::Center))
            && matches!(style.width, Some(CssLength::Px(_)))
    {
        div().w_full().flex().justify_center().child(inner)
    } else {
        inner
    }
}

fn render_node(
    node: &LayoutNode,
    counter: &mut usize,
    attachments: &[AttachmentInfo],
    has_explicit_bg: bool,
    cx: &App,
) -> AnyElement {
    match node {
        LayoutNode::Container { style, children } => {
            let child_bg =
                has_explicit_bg || style.background_color.is_some_and(|c| !c.is_transparent());
            let mut box_div = div().flex().flex_col();
            apply_box_style(&mut box_div, style, cx);
            for child in children {
                box_div = box_div.child(render_node(child, counter, attachments, child_bg, cx));
            }
            wrap_if_centered(box_div, style).into_any_element()
        }
        LayoutNode::Table {
            style,
            cellspacing,
            rows,
            ..
        } => {
            let table_bg =
                has_explicit_bg || style.background_color.is_some_and(|c| !c.is_transparent());
            let mut table_div = div().flex().flex_col();
            if *cellspacing > 0.0 {
                table_div = table_div.gap(px(*cellspacing));
            }
            apply_box_style(&mut table_div, style, cx);

            for row in rows {
                let row_bg = table_bg
                    || row
                        .style
                        .background_color
                        .is_some_and(|c| !c.is_transparent());
                let mut row_div = div().w_full().flex().flex_row();
                if *cellspacing > 0.0 {
                    row_div = row_div.gap(px(*cellspacing));
                }
                apply_box_style(&mut row_div, &row.style, cx);
                for cell in &row.cells {
                    row_div =
                        row_div.child(render_table_cell(cell, counter, attachments, row_bg, cx));
                }
                table_div = table_div.child(row_div);
            }

            wrap_if_centered(table_div, style).into_any_element()
        }
        LayoutNode::RichText { style, align, runs } => {
            let rt_bg =
                has_explicit_bg || style.background_color.is_some_and(|c| !c.is_transparent());
            render_rich_text(style, *align, runs, counter, rt_bg, cx)
        }
        LayoutNode::ButtonLink {
            style,
            label,
            target,
        } => render_button_link(style, label, target, counter, cx),
        LayoutNode::Image {
            style,
            src,
            blocked_src,
            alt,
            width,
            height,
            is_cid,
        } => render_image_placeholder(
            style,
            src.as_deref(),
            blocked_src.as_deref(),
            alt,
            *width,
            *height,
            *is_cid,
            attachments,
            cx,
        ),
        LayoutNode::ListItem {
            style,
            marker,
            children,
        } => {
            let item_bg =
                has_explicit_bg || style.background_color.is_some_and(|c| !c.is_transparent());
            let mut row = div().w_full().flex().flex_row().items_start().gap(px(8.0));
            apply_box_style(&mut row, style, cx);
            let marker_color = style
                .color
                .map(|c| adapt_fg_for_theme(c, item_bg, cx))
                .unwrap_or(cx.theme().muted_foreground);
            row = row.child(
                div()
                    .text_sm()
                    .text_color(marker_color)
                    .child(marker.clone()),
            );
            let mut content = div().flex_1().min_w(px(0.0)).flex().flex_col();
            for c in children {
                content = content.child(render_node(c, counter, attachments, item_bg, cx));
            }
            row.child(content).into_any_element()
        }
        LayoutNode::Rule { style } => {
            let color = style
                .border_top
                .and_then(|b| b.color)
                .map(css_to_hsla)
                .unwrap_or(cx.theme().border);
            div()
                .w_full()
                .my(px(8.0))
                .border_t_1()
                .border_color(color)
                .into_any_element()
        }
    }
}

fn render_table_cell(
    cell: &TableCellNode,
    counter: &mut usize,
    attachments: &[AttachmentInfo],
    has_explicit_bg: bool,
    cx: &App,
) -> AnyElement {
    let cell_bg = has_explicit_bg
        || cell
            .style
            .background_color
            .is_some_and(|c| !c.is_transparent());
    let mut cell_div = div().flex().flex_col().min_w(px(0.0));
    match cell.style.width {
        Some(CssLength::Px(w)) => {
            cell_div = cell_div.w(px(w));
        }
        _ => {
            cell_div = cell_div.flex_1();
            if cell.colspan > 1 {
                cell_div.style().flex_grow = Some(cell.colspan as f32);
            }
        }
    }
    match cell.style.vertical_align {
        VerticalAlign::Top => cell_div = cell_div.justify_start(),
        VerticalAlign::Middle => cell_div = cell_div.justify_center(),
        VerticalAlign::Bottom => cell_div = cell_div.justify_end(),
    }
    if matches!(cell.style.text_align, Some(TextAlign::Center)) {
        cell_div = cell_div.items_center();
    } else if matches!(cell.style.text_align, Some(TextAlign::Right)) {
        cell_div = cell_div.items_end();
    }
    apply_box_style(&mut cell_div, &cell.style, cx);

    for child in &cell.children {
        cell_div = cell_div.child(render_node(child, counter, attachments, cell_bg, cx));
    }
    cell_div.into_any_element()
}

fn render_rich_text(
    style: &ComputedStyle,
    align: TextAlign,
    runs: &[TextRunNode],
    counter: &mut usize,
    has_explicit_bg: bool,
    cx: &App,
) -> AnyElement {
    let id_num = *counter;
    *counter += 1;

    let mut full_text = String::new();
    let mut gpui_runs = Vec::with_capacity(runs.len());
    let mut clickable_ranges: Vec<Range<usize>> = Vec::new();
    let mut click_targets: Vec<LinkTarget> = Vec::new();

    let default_fg = cx.theme().foreground;
    let link_fg = cx.theme().link;
    let danger_fg = cx.theme().danger;

    let mut dominant_font_size = 14.0f32;
    for r in runs {
        dominant_font_size = dominant_font_size.max(r.font_size);
        let start = full_text.len();
        full_text.push_str(&r.text);
        let end = full_text.len();
        let len = end - start;
        if len == 0 {
            continue;
        }

        let color = if let Some(link) = &r.link {
            clickable_ranges.push(start..end);
            click_targets.push(link.clone());
            if link.phishing_risk {
                danger_fg
            } else {
                r.color
                    .map(|c| adapt_fg_for_theme(c, has_explicit_bg, cx))
                    .unwrap_or(link_fg)
            }
        } else {
            r.color
                .map(|c| adapt_fg_for_theme(c, has_explicit_bg, cx))
                .unwrap_or(default_fg)
        };

        let mut font = Font {
            family: if r.monospace {
                SharedString::from("Consolas")
            } else {
                SharedString::from(".SystemUIFont")
            },
            features: Default::default(),
            fallbacks: None,
            weight: if r.bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            },
            style: if r.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
        };
        let _ = &mut font;

        let underline = if r.underline || r.link.is_some() {
            Some(UnderlineStyle {
                thickness: px(1.0),
                color: Some(color),
                wavy: r.link.as_ref().is_some_and(|l| l.phishing_risk),
            })
        } else {
            None
        };

        let strikethrough = if r.line_through {
            Some(StrikethroughStyle {
                thickness: px(1.0),
                color: Some(color),
            })
        } else {
            None
        };

        gpui_runs.push(TextRun {
            len,
            font,
            color,
            background_color: r.background_color.map(css_to_hsla),
            underline,
            strikethrough,
        });
    }

    let styled = StyledText::new(SharedString::from(full_text)).with_runs(gpui_runs);
    let interactive =
        InteractiveText::new(ElementId::Name(format!("email-rt-{id_num}").into()), styled)
            .on_click(clickable_ranges, move |range_ix, _window, cx| {
                if let Some(target) = click_targets.get(range_ix) {
                    cx.open_url(&target.href);
                }
            });

    let mut wrapper = div().w_full().text_size(px(dominant_font_size));
    match align {
        TextAlign::Center => wrapper = wrapper.text_center(),
        TextAlign::Right => wrapper = wrapper.text_right(),
        _ => wrapper = wrapper.text_left(),
    }
    apply_box_style(&mut wrapper, style, cx);
    wrapper.child(interactive).into_any_element()
}

fn render_button_link(
    style: &ComputedStyle,
    label: &str,
    target: &LinkTarget,
    counter: &mut usize,
    cx: &App,
) -> AnyElement {
    let id_num = *counter;
    *counter += 1;

    let bg = style
        .background_color
        .filter(|c| !c.is_transparent())
        .map(css_to_hsla)
        .unwrap_or(cx.theme().primary);
    let fg = style
        .color
        .map(css_to_hsla)
        .unwrap_or(cx.theme().primary_foreground);
    let radius = if style.border_radius > 0.0 {
        style.border_radius
    } else {
        6.0
    };
    let pad_v = if style.padding.top > 0.0 {
        style.padding.top
    } else {
        8.0
    };
    let pad_h = if style.padding.left > 0.0 {
        style.padding.left
    } else {
        16.0
    };

    let href = target.href.clone();
    let display_label = if target.phishing_risk {
        format!("⚠ {label}")
    } else {
        label.to_string()
    };

    let btn = div()
        .id(ElementId::Name(format!("email-cta-{id_num}").into()))
        .flex()
        .items_center()
        .justify_center()
        .bg(bg)
        .text_color(fg)
        .text_size(px(style.font_size))
        .font_weight(if style.is_bold() {
            FontWeight::BOLD
        } else {
            FontWeight::MEDIUM
        })
        .px(px(pad_h))
        .py(px(pad_v))
        .rounded(px(radius))
        .cursor_pointer()
        .on_click(move |_ev, _window, cx| {
            cx.open_url(&href);
        })
        .child(display_label);

    wrap_if_centered(div().my(px(4.0)).child(btn), style).into_any_element()
}

const MAX_INLINE_IMAGE_BYTES: usize = 4 * 1024 * 1024;

fn image_format_from_mime(mime: &str) -> Option<ImageFormat> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" | "image/jpg" => Some(ImageFormat::Jpeg),
        "image/webp" => Some(ImageFormat::Webp),
        "image/gif" => Some(ImageFormat::Gif),
        "image/svg+xml" => Some(ImageFormat::Svg),
        "image/bmp" => Some(ImageFormat::Bmp),
        "image/tiff" => Some(ImageFormat::Tiff),
        "image/x-icon" | "image/vnd.microsoft.icon" => Some(ImageFormat::Ico),
        _ => None,
    }
}

fn decode_data_uri_image(src: &str) -> Option<Arc<Image>> {
    let rest = src.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    if !meta.to_ascii_lowercase().contains(";base64") {
        return None;
    }
    let mime = meta.split(';').next()?;
    let format = image_format_from_mime(mime)?;
    if payload.len() > MAX_INLINE_IMAGE_BYTES * 2 {
        return None;
    }
    let cleaned: String = payload
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .ok()?;
    if bytes.is_empty() || bytes.len() > MAX_INLINE_IMAGE_BYTES {
        return None;
    }
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

fn decode_cid_attachment_image(src: &str, attachments: &[AttachmentInfo]) -> Option<Arc<Image>> {
    let cid = src
        .strip_prefix("blob://")
        .or_else(|| src.strip_prefix("cid:"))?;
    let cid_name = cid.split('@').next().unwrap_or(cid);
    let att = attachments.iter().find(|a| {
        !a.blob_path.is_empty()
            && (a.filename.eq_ignore_ascii_case(cid) || a.filename.eq_ignore_ascii_case(cid_name))
    })?;
    let format = image_format_from_mime(&att.content_type)?;
    let meta = std::fs::metadata(&att.blob_path).ok()?;
    if meta.len() == 0 || meta.len() > MAX_INLINE_IMAGE_BYTES as u64 {
        return None;
    }
    let bytes = std::fs::read(&att.blob_path).ok()?;
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

#[allow(clippy::too_many_arguments)]
fn render_image_placeholder(
    style: &ComputedStyle,
    src: Option<&str>,
    blocked_src: Option<&str>,
    alt: &str,
    width: Option<f32>,
    height: Option<f32>,
    is_cid: bool,
    attachments: &[AttachmentInfo],
    cx: &App,
) -> AnyElement {
    if blocked_src.is_none()
        && let Some(src_str) = src
        && let Some(decoded) = decode_data_uri_image(src_str)
            .or_else(|| decode_cid_attachment_image(src_str, attachments))
    {
        let mut image_el = img(ImageSource::Image(decoded)).object_fit(ObjectFit::Contain);
        if let Some(w) = width {
            image_el = image_el.w(px(w.clamp(16.0, 680.0)));
        } else {
            image_el = image_el.max_w(px(680.0));
        }
        if let Some(h) = height {
            image_el = image_el.h(px(h.clamp(16.0, 600.0)));
        }
        return wrap_if_centered(div().my(px(4.0)).child(image_el), style).into_any_element();
    }

    let mut box_div = div()
        .flex()
        .items_center()
        .justify_center()
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(6.0))
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted)
        .text_xs()
        .text_color(cx.theme().muted_foreground);

    if let Some(w) = width {
        box_div = box_div.max_w(px(w.clamp(48.0, 680.0)));
    }
    if let Some(h) = height {
        box_div = box_div.min_h(px(h.clamp(28.0, 120.0)));
    }

    let caption = if blocked_src.is_some() {
        if alt.trim().is_empty() {
            "Blocked remote image".to_string()
        } else {
            format!("Blocked image: {alt}")
        }
    } else if is_cid {
        if alt.trim().is_empty() {
            format!("Inline attachment ({})", src.unwrap_or("cid"))
        } else {
            format!("Inline image: {alt}")
        }
    } else if !alt.trim().is_empty() {
        alt.to_string()
    } else {
        "Image".to_string()
    };

    wrap_if_centered(box_div.child(caption), style).into_any_element()
}
