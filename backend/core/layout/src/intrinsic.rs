// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Preferred widths for shrink-to-fit boxes (absolutely positioned boxes,
//! inline-blocks, floats, flex items): the width a box takes with unlimited
//! room (max-content) and the narrowest it can be without overflowing
//! (min-content), found by laying the box out at those two widths and reading
//! how far its content reaches. Results are memoised per node for the pass.

use crate::block::{layout_block_level, Constraint, Forced};
use crate::ctx::Ctx;
use blueice_dom::NodeId;

/// Room enough that nothing wraps.
pub(crate) const UNBOUNDED: f64 = 1.0e6;

/// `(min-content, max-content)` border-box widths of `node`'s *content*, ignoring
/// its own `width`: what flexbox needs for an item's automatic minimum size and
/// `flex-basis: content`.
pub(crate) fn content_widths(ctx: &Ctx<'_>, node: NodeId) -> (f64, f64) {
    if let Some(known) = ctx.content_widths.borrow().get(&node) {
        return *known;
    }
    let result = content_widths_uncached(ctx, node);
    ctx.content_widths.borrow_mut().insert(node, result);
    result
}

/// The width along the line, in a vertical frame, of a box that is written in another one: its
/// height across the page when it is as wide there as it asks to be (or the room across the page
/// the vertical box leaves). Both preferred widths are that one size.
fn orthogonal_extent(ctx: &Ctx<'_>, node: NodeId) -> f64 {
    let outer = ctx.writing.replace(crate::ctx::Writing::Horizontal);
    let room = ctx.orth_room.get();
    let (min, max) = preferred_widths(ctx, node);
    let width = max.min(room.max(min));
    let placed = layout_block_level(
        ctx,
        node,
        &Constraint { cb_width: room, cb_height: None, root: false },
        Forced { width: Some(width), height: None },
    );
    ctx.writing.set(outer);
    placed.fragment.height
}

/// Whether `node` is written in a different mode from the frame being laid out.
fn is_orthogonal(ctx: &Ctx<'_>, node: NodeId) -> bool {
    ctx.writing.get().is_vertical() && ctx.style(node).writing_mode() != ctx.writing.get()
}

/// A vertical box in a horizontal frame: it is as wide as its lines stacked.
fn vertical_root(ctx: &Ctx<'_>, node: NodeId) -> Option<crate::ctx::Writing> {
    let mode = ctx.style(node).writing_mode();
    (mode.is_vertical() && !ctx.writing.get().is_vertical()).then_some(mode)
}

fn content_widths_uncached(ctx: &Ctx<'_>, node: NodeId) -> (f64, f64) {
    if let Some(mode) = vertical_root(ctx, node) {
        let style = ctx.style(node);
        let (padding, border) = (style.padding(0.0), style.border(0.0));
        let width = crate::vertical::block_extent(ctx, node, mode) + padding.horizontal() + border.horizontal();
        return (width, width);
    }
    if is_orthogonal(ctx, node) {
        let style = ctx.style(node);
        // The content box's height across the page, not counting the box's own padding.
        let outer = ctx.writing.replace(crate::ctx::Writing::Horizontal);
        let (padding, border) = (style.padding(0.0), style.border(0.0));
        let pb_v = padding.vertical() + border.vertical();
        ctx.writing.set(outer);
        let content = (orthogonal_extent(ctx, node) - pb_v).max(0.0);
        // In the turned frame the box's own padding along the line is the page's vertical one.
        return (content + pb_v, content + pb_v);
    }
    let style = ctx.style(node);
    let padding = style.padding(0.0);
    let border = style.border(0.0);
    let pb_h = padding.horizontal() + border.horizontal();
    if let Some(w) = drop_down_width(ctx, node, &style).or_else(|| text_field_width(ctx, node, &style)) {
        return (w + pb_h, w + pb_h);
    }
    // A picture has no content to measure: its size is both widths.
    if ctx.tag(node).is_some_and(crate::block::is_replaced_tag) {
        if let Some((w, _)) = crate::inline::replaced_size(ctx, node, &style, f64::NAN) {
            return (w + pb_h, w + pb_h);
        }
    }
    let measure = |available: f64| -> f64 {
        let c = Constraint {
            cb_width: available,
            cb_height: None,
            root: false,
        };
        let placed = layout_block_level(
            ctx,
            node,
            &c,
            Forced {
                width: Some(available),
                height: None,
            },
        );
        // A box's content width is a `LayoutUnit`: text measured to a float is rounded up to
        // the next 1/64 pixel, as WebKit does.
        (placed.content_extent * 64.0 - 1e-6).ceil() / 64.0 + pb_h
    };
    let max = measure(UNBOUNDED);
    (measure(0.0).min(max), max)
}

/// `system-ui` (San Francisco) average character width in pixels at a font size, measured in
/// Safari: its optical size changes the advances, so it is not a fixed fraction of the size.
const SYSTEM_UI_AVERAGE: [(f64, f64); 13] = [
    (9.0, 5.8359375), (10.0, 6.41640625), (11.0, 6.99375), (12.0, 7.55859375), (13.0, 8.1125),
    (13.333, 8.30078125), (14.0, 8.66796875), (15.0, 9.2140625), (16.0, 9.765625), (17.0, 10.2765625),
    (18.0, 10.87968750), (20.0, 12.03828125), (24.0, 14.72734375),
];

fn system_ui_average(px: f64) -> f64 {
    let table = &SYSTEM_UI_AVERAGE;
    if px <= table[0].0 {
        return table[0].1 / table[0].0 * px;
    }
    for pair in table.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        if px <= x1 {
            return y0 + (y1 - y0) * (px - x0) / (x1 - x0);
        }
    }
    let (x, y) = table[table.len() - 1];
    y / x * px
}

/// The families WebKit does not trust the font's own average character width for when it sizes a
/// text field (matched on the name the page wrote): there the field is `size` average characters
/// plus 2px, with no allowance for the widest glyph.
const UNTRUSTED_AVERAGE_FAMILIES: [&str; 34] = [
    "american typewriter", "arial hebrew", "chalkboard", "cochin", "corsiva hebrew", "courier",
    "euphemia ucas", "geneva", "gill sans", "hei", "helvetica", "hoefler text", "inaimathi", "kai",
    "lucida grande", "marker felt", "monaco", "mshtakan", "new peninsula", "osaka", "raanana",
    "stheiti", "symbol", "times", "apple braille", "apple ligothic", "apple lisung", "apple symbols",
    "applegothic", "applemyungjo", "#gungseo", "#headlinea", "#pcmyungjo", "#pilgi",
];

/// The content width of a text field with no width of its own: `size` (20 by default)
/// characters, as WebKit counts them. In a font it trusts, that is `size` average characters
/// rounded up, plus the room by which the widest glyph exceeds the average, plus 2px; in the
/// other families the average is the font's real one and only the 2px is added.
pub(crate) fn text_field_width(ctx: &Ctx<'_>, node: NodeId, style: &crate::ctx::Style<'_>) -> Option<f64> {
    if ctx.tag(node) != Some("input")
        || blueice_css::is_pseudo_node(node)
        || style.raw("width").is_some_and(|_| !style.is_auto("width"))
    {
        return None;
    }
    let kind = ctx.attr(node, "type").map(str::to_ascii_lowercase);
    if !matches!(kind.as_deref(), None | Some("" | "text" | "search" | "email" | "url" | "tel" | "password")) {
        return None;
    }
    let chars = ctx
        .attr(node, "size")
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(20);
    let n = f64::from(chars);
    let px = style.font_size();
    let family = style
        .style
        .and_then(|s| s.font_family.as_deref())
        .map_or_else(|| String::from("system-ui"), |list| {
            list.split(',').next().unwrap_or("").trim().trim_matches(['"', '\'']).to_ascii_lowercase()
        });
    let ceil64 = |v: f64| (v * 64.0 - 1e-6).ceil() / 64.0;
    let system = matches!(family.as_str(), "system-ui" | "-apple-system" | "blinkmacsystemfont" | "ui-sans-serif" | "san francisco");
    if system {
        return Some(ceil64(n * system_ui_average(px)) + 2.0);
    }
    if UNTRUSTED_AVERAGE_FAMILIES.contains(&family.as_str()) {
        let average = match family.as_str() {
            "helvetica" => 0.55615 * px,
            "times" => 0.5 * px,
            "courier" => 0.6 * px,
            _ => blueice_font::control_char_widths(style.style.is_some_and(|s| s.is_bold()), style.style.is_some_and(|s| s.is_italic()), style.web_face(), px)
                .map_or(0.6 * px, |(avg, _)| avg),
        };
        return Some(ceil64(n * average) + 2.0);
    }
    match blueice_font::control_char_widths(style.style.is_some_and(|s| s.is_bold()), style.style.is_some_and(|s| s.is_italic()), style.web_face(), px) {
        Some((avg, max)) => Some(ceil64(n * avg) + (max - avg).max(0.0) + 2.0),
        None => Some(ceil64(n * system_ui_average(px)) + 2.0),
    }
}

/// The content width of a drop-down (`<select>`) with no width of its own: its longest option's
/// label plus room for the arrow.
pub(crate) fn drop_down_width(ctx: &Ctx<'_>, node: NodeId, style: &crate::ctx::Style<'_>) -> Option<f64> {
    if ctx.tag(node) != Some("select")
        || blueice_css::is_pseudo_node(node)
        || style.raw("width").is_some_and(|_| !style.is_auto("width"))
    {
        return None;
    }
    let (bold, italic) = (
        style.style.is_some_and(|s| s.is_bold()),
        style.style.is_some_and(|s| s.is_italic()),
    );
    let widest = crate::native_text::select_options(ctx.doc, node)
        .iter()
        .map(|option| blueice_font::measure_text_width_in(&option.label, style.font_size(), bold, italic, style.web_face()))
        .fold(0.0f64, f64::max);
    // Room for the arrow, unless the page draws its own (`appearance: none`).
    let appearance_none = style
        .raw("appearance")
        .or_else(|| style.raw("-webkit-appearance"))
        .is_some_and(|v| matches!(v, blueice_css::Value::Keyword(k) if k == "none"));
    Some(widest + if appearance_none { 0.0 } else { 24.0 })
}

/// `(min-content, max-content)` border-box widths of `node`, margins excluded.
pub(crate) fn preferred_widths(ctx: &Ctx<'_>, node: NodeId) -> (f64, f64) {
    if let Some(known) = ctx.intrinsic.borrow().get(&node) {
        return *known;
    }
    if is_orthogonal(ctx, node) {
        let extent = orthogonal_extent(ctx, node);
        ctx.intrinsic.borrow_mut().insert(node, (extent, extent));
        return (extent, extent);
    }
    if let Some(mode) = vertical_root(ctx, node) {
        // An explicit width is still its own preferred width; otherwise the lines decide.
        let style = ctx.style(node);
        let (padding, border) = (style.padding(0.0), style.border(0.0));
        let pb_h = padding.horizontal() + border.horizontal();
        let width = match style.px("width", f64::NAN) {
            Some(w) => if style.is_border_box() { w } else { w + pb_h },
            None => crate::vertical::block_extent(ctx, node, mode) + pb_h,
        };
        ctx.intrinsic.borrow_mut().insert(node, (width, width));
        return (width, width);
    }
    let style = ctx.style(node);
    let padding = style.padding(0.0);
    let border = style.border(0.0);
    let pb_h = padding.horizontal() + border.horizontal();
    let measure = |available: f64| -> f64 {
        let c = Constraint {
            cb_width: available,
            cb_height: None,
            root: false,
        };
        let placed = layout_block_level(
            ctx,
            node,
            &c,
            Forced {
                width: Some(available),
                height: None,
            },
        );
        // A box's content width is a `LayoutUnit`: text measured to a float is rounded up to
        // the next 1/64 pixel, as WebKit does.
        (placed.content_extent * 64.0 - 1e-6).ceil() / 64.0 + pb_h
    };
    // An explicit pixel width is both preferred widths.
    let explicit =
        style
            .px("width", f64::NAN)
            .map(|w| if style.is_border_box() { w } else { w + pb_h });
    let drop_down = drop_down_width(ctx, node, &style).or_else(|| text_field_width(ctx, node, &style)).map(|w| w + pb_h);
    let result = match explicit.or(drop_down) {
        Some(w) => (w, w),
        None => {
            let max = measure(UNBOUNDED);
            let min = measure(0.0).min(max);
            (min, max)
        }
    };
    ctx.intrinsic.borrow_mut().insert(node, result);
    result
}
