// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Vertical writing modes (`writing-mode: vertical-rl | vertical-lr | sideways-lr`).
//!
//! A vertical box is laid out in a turned frame and then turned back. Inside it, `Ctx::writing`
//! makes every style read name the sides the turned frame sees (its `width` is the page's
//! `height`, its `left` the page's top, ...), so the whole block and inline algorithm runs
//! unchanged with an inline size equal to the box's height; the result is then rotated into the
//! page's frame. A box's own margin, border, padding and background stay in the page's frame,
//! laid out by the box's parent as for any block.
//!
//! In the turned frame a line is a row; in the page it is a column. Blocks stack from the right
//! (`vertical-rl`) or the left (`vertical-lr`, `sideways-lr`); inside a line, the side letters
//! grow toward is the page's right, except in `sideways-lr`, where the text is turned the other
//! way, runs up the page and its letters grow toward the left. Text runs are cut where their
//! orientation changes: ideographs, kana, hangul and fullwidth forms stand upright, everything
//! else lies on its side (clockwise), as `text-orientation: mixed` says; `upright` and
//! `sideways` force one, and `sideways-*` writing modes are all sideways.
//!
//! A horizontal box inside a vertical one is an orthogonal flow: it is laid out whole in the
//! page's own frame and turned into the vertical box's frame, so turning that frame back leaves
//! it as it was written. Form controls are always such boxes.

use crate::block::{layout_block_level, layout_flow, Constraint, Forced, Margin, PendingAbs, Placed};
use crate::ctx::{Combine, Ctx, Writing};
use crate::fragment::{Fragment, FragmentKind, TextOrient};
use blueice_dom::NodeId;

/// `text` cut into the stretches `text-combine-upright` sets in one em each (`true`) and the
/// stretches that are laid out as usual (`false`).
pub(crate) fn combine_runs(text: &str, rule: Combine) -> Vec<(String, bool)> {
    match rule {
        Combine::All => vec![(text.to_string(), true)],
        Combine::Digits(most) => {
            let mut out: Vec<(String, bool)> = Vec::new();
            let mut chars = text.chars().peekable();
            while let Some(c) = chars.next() {
                if c.is_ascii_digit() {
                    let mut digits = String::from(c);
                    while let Some(d) = chars.next_if(char::is_ascii_digit) {
                        digits.push(d);
                    }
                    let fits = digits.len() as u32 <= most;
                    out.push((digits, fits));
                } else {
                    match out.last_mut() {
                        Some((run, false)) => run.push(c),
                        _ => out.push((c.to_string(), false)),
                    }
                }
            }
            out
        }
    }
}

/// How a writing mode's turned frame maps onto the page.
#[derive(Clone, Copy)]
struct Turn {
    /// Blocks stack from the page's right (the turned frame's top is the page's right).
    block_reverse: bool,
    /// Inside a line, the turned frame's top is the page's right.
    line_reverse: bool,
    /// The inline direction runs up the page.
    inline_reverse: bool,
}

impl Turn {
    fn of(mode: Writing) -> Turn {
        match mode {
            Writing::VerticalRl => Turn { block_reverse: true, line_reverse: true, inline_reverse: false },
            Writing::VerticalLr => Turn { block_reverse: false, line_reverse: true, inline_reverse: false },
            Writing::SidewaysLr => Turn { block_reverse: false, line_reverse: false, inline_reverse: true },
            Writing::Horizontal => Turn { block_reverse: false, line_reverse: false, inline_reverse: false },
        }
    }

    /// Where a point of the turned frame lies in the page, inside a content box `block` across
    /// and `length` down.
    fn point(self, (x, y): (f64, f64), block: f64, length: f64) -> (f64, f64) {
        (
            if self.block_reverse { block - y } else { y },
            if self.inline_reverse { length - x } else { x },
        )
    }
}

/// What laying out a vertical box's content produced, in the page's frame, relative to the
/// box's content box.
pub(crate) struct VerticalFlow {
    pub children: Vec<Fragment>,
    /// The content's height: the box's own when it has one, otherwise the longest line.
    pub height: f64,
    pub pending_abs: Vec<PendingAbs>,
}

/// Lays out `node`'s children as vertical text in a content box `content_width` wide (across the
/// lines) and `content_height` tall if it has a height. `inline_room` is how long a line may be
/// when the height is not set.
pub(crate) fn layout_vertical_flow(
    ctx: &Ctx<'_>,
    node: NodeId,
    mode: Writing,
    content_width: f64,
    content_height: Option<f64>,
    inline_room: f64,
) -> VerticalFlow {
    let room = content_height.unwrap_or(inline_room);
    let previous = ctx.writing.replace(mode);
    let previous_room = ctx.orth_room.replace(content_width);
    let style = ctx.style(node);
    // The box's own layout algorithm runs in the turned frame: its inline size is `room` and, for
    // a flex container, its block size (the turned frame's height) the page width of the box.
    struct Flow {
        children: Vec<Fragment>,
        extent: f64,
        pending_abs: Vec<PendingAbs>,
    }
    let flow = match style.display() {
        crate::ctx::Display::Table | crate::ctx::Display::InlineTable => {
            let table = crate::table::layout_table(
                ctx,
                node,
                &crate::table::TableInput {
                    available: room,
                    specified: None,
                    forced: None,
                    definite_height: Some(content_width),
                },
            );
            Flow { children: table.children, extent: table.width, pending_abs: table.pending_abs }
        }
        crate::ctx::Display::Flex
        | crate::ctx::Display::InlineFlex
        | crate::ctx::Display::Grid
        | crate::ctx::Display::InlineGrid => {
            let input = crate::flex::FlexInput {
                content_width: room,
                definite_height: Some(content_width),
                min_height: style.px("min-height", content_width).unwrap_or(0.0),
                max_height: style.px("max-height", content_width),
            };
            let laid = if matches!(
                style.display(),
                crate::ctx::Display::Grid | crate::ctx::Display::InlineGrid
            ) {
                crate::grid::layout_grid(ctx, node, &input)
            } else {
                crate::flex::layout_flex(ctx, node, &input)
            };
            Flow { children: laid.children, extent: laid.extent, pending_abs: laid.pending_abs }
        }
        _ => {
            let laid = layout_flow(ctx, node, room, None, true, true, &[]);
            Flow { children: laid.children, extent: laid.extent, pending_abs: laid.pending_abs }
        }
    };
    let height = content_height.unwrap_or_else(|| flow.extent.clamp(0.0, room));
    let turn = Turn::of(mode);
    let mut children = flow.children;
    // Where the text of a box with no height of its own starts down the page (the bottom, in
    // `sideways-lr`) is the end of the longest line, so that is the length that is turned about.
    rotate(ctx, &mut children, content_width, height, false, turn);
    ctx.orth_room.set(previous_room);
    ctx.writing.set(previous);
    // Boxes positioned against a containing block are placed later in the page's frame: their
    // static positions turn with everything else.
    let pending_abs = flow
        .pending_abs
        .into_iter()
        .map(|p| {
            let (x, y) = turn.point((p.x, p.y), content_width, height);
            PendingAbs { node: p.node, x, y }
        })
        .collect();
    VerticalFlow { children, height, pending_abs }
}

/// Whether `c` stands upright in a vertical line under `text-orientation: mixed`.
fn upright_in_mixed(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF | 0x2E80..=0x2FDF | 0x3000..=0x303F | 0x3040..=0x30FF | 0x3100..=0x312F
        | 0x3130..=0x318F | 0x31A0..=0x31FF | 0x3200..=0x4DBF | 0x4E00..=0x9FFF | 0xA960..=0xA97F
        | 0xAC00..=0xD7FF | 0xF900..=0xFAFF | 0xFE10..=0xFE1F | 0xFE30..=0xFE6F | 0xFF00..=0xFFEF
        | 0x1F000..=0x1FAFF | 0x20000..=0x2FA1F)
}

/// How long upright `text` is down a vertical line: each character takes the font's own vertical
/// advance (`vmtx`), or its horizontal one where the font cannot say.
pub(crate) fn upright_length(
    text: &str,
    size: f64,
    bold: bool,
    italic: bool,
    face: Option<blueice_font::web::WebFontId>,
    features: blueice_font::FeaturesId,
) -> f64 {
    // The font's vertical layout for the run as a whole: pair kerning and proportional widths.
    if let Some(glyphs) = blueice_font::vertical_glyphs(text, bold, italic, face, features, size as f32) {
        return glyphs.iter().map(|g| f64::from(g.advance)).sum();
    }
    text.chars()
        .map(|c| match blueice_font::font_for_char_in(c, bold, italic, face) {
            blueice_font::Glyphs::Static(font) => blueice_font::vertical_cell(font, c)
                .map_or_else(
                    || blueice_font::measure_text_width_in(&c.to_string(), size, bold, italic, face),
                    |cell| f64::from(cell.advance) * size,
                ),
            blueice_font::Glyphs::Variable(_) => {
                blueice_font::measure_text_width_in(&c.to_string(), size, bold, italic, face)
            }
        })
        .sum()
}

/// The orientation a character takes in a vertical line.
pub(crate) fn orient_of(c: char, text_orientation: Option<&str>) -> TextOrient {
    match text_orientation {
        Some("upright") => TextOrient::Upright,
        Some("sideways" | "sideways-right") => TextOrient::Sideways,
        _ if upright_in_mixed(c) => TextOrient::Upright,
        _ => TextOrient::Sideways,
    }
}

/// The room a child's content box has in the turned frame: across the lines, and along them.
fn content_room(ctx: &Ctx<'_>, child: &Fragment) -> (f64, f64) {
    let style = ctx.style_of_fragment(child);
    let (pad, border) = (style.padding(0.0), style.border(0.0));
    (
        (child.height - pad.top - pad.bottom - border.top - border.bottom).max(0.0),
        (child.width - pad.left - pad.right - border.left - border.right).max(0.0),
    )
}

/// Turns `children` (laid out in the turned frame, positions relative to their parent's content
/// box) into the page's frame. The parent's content box is `block` thick across the lines and
/// `length` long down them; `line` says it is a line box, inside which the frame is turned the
/// way lines are, not the way blocks are.
fn rotate(ctx: &Ctx<'_>, children: &mut Vec<Fragment>, block: f64, length: f64, line: bool, turn: Turn) {
    let reverse = if line { turn.line_reverse } else { turn.block_reverse };
    let mut out = Vec::with_capacity(children.len());
    for mut child in std::mem::take(children) {
        let across = if reverse { block - (child.y + child.height) } else { child.y };
        let along = if turn.inline_reverse { length - (child.x + child.width) } else { child.x };
        let (run, thick) = (child.width, child.height);
        // What is inside the child is turned relative to the child's own content box.
        let (inner_block, inner_length, inner_line) = match &child.kind {
            FragmentKind::Line => (child.height, child.width, true),
            _ => {
                let (b, l) = content_room(ctx, &child);
                (b, l, false)
            }
        };
        if child.turned_back {
            // Laid out in the page's own frame by a horizontal box and turned for the layout
            // around it; this turns it back.
            child.turned_back = false;
        } else if let FragmentKind::Text(text) = &child.kind {
            let text = text.clone();
            out.extend(split_runs(ctx, &child, &text, (along, run, thick, across), turn));
            continue;
        }
        rotate(ctx, &mut child.children, inner_block, inner_length, inner_line, turn);
        child.x = across;
        child.y = along;
        child.width = thick;
        child.height = run;
        child.baseline = None;
        out.push(child);
    }
    *children = out;
}

/// A text fragment of the turned frame as the runs it is made of in the page's frame: one
/// fragment for each stretch of characters that share an orientation, one below the other.
/// `(along, run, thick, across)` is where the fragment starts down the page, how long it is, how
/// wide its column is and where that starts across the page.
fn split_runs(
    ctx: &Ctx<'_>,
    f: &Fragment,
    text: &str,
    (along, run, thick, across): (f64, f64, f64, f64),
    turn: Turn,
) -> Vec<Fragment> {
    let style = f.node.map(|n| ctx.style(n));
    // `sideways-lr` writes everything turned the other way, one run to the line.
    let ccw = turn.inline_reverse;
    let mode_keyword = style
        .as_ref()
        .and_then(|s| s.style.and_then(|st| st.other.get("writing-mode")))
        .and_then(|v| match v {
            blueice_css::Value::Keyword(k) => Some(k.as_str()),
            _ => None,
        });
    let forced_sideways = mode_keyword.is_some_and(|k| k.starts_with("sideways"));
    let orientation = if forced_sideways {
        Some("sideways")
    } else {
        style.as_ref().and_then(|s| s.keyword("text-orientation"))
    };
    let combine = style.as_ref().and_then(|s| s.combine_upright());
    let mut runs: Vec<(String, TextOrient)> = Vec::new();
    if ccw {
        runs.push((text.to_string(), TextOrient::SidewaysCcw));
    } else {
        let pieces = match combine {
            Some(rule) => combine_runs(text, rule),
            None => vec![(text.to_string(), false)],
        };
        for (piece, combined) in pieces {
            if combined {
                runs.push((piece, TextOrient::Combined));
                continue;
            }
            for c in piece.chars() {
                let o = orient_of(c, orientation);
                match runs.last_mut() {
                    Some((run, last)) if *last == o => run.push(c),
                    _ => runs.push((c.to_string(), o)),
                }
            }
        }
    }
    let em = style.as_ref().map_or(16.0, |s| s.font_size());
    let measure = |run: &str, orient: TextOrient| match &style {
        _ if orient == TextOrient::Combined => em,
        Some(s) if orient == TextOrient::Upright => upright_length(
            run,
            s.font_size(),
            s.style.is_some_and(|st| st.is_bold()),
            s.style.is_some_and(|st| st.is_italic()),
            s.web_face(),
            s.style
                .map(|st| blueice_font::register_features(&blueice_css::font_features(st)))
                .unwrap_or_default(),
        ),
        Some(s) => blueice_font::measure_text_width_in(
            run,
            s.font_size(),
            s.style.is_some_and(|st| st.is_bold()),
            s.style.is_some_and(|st| st.is_italic()),
            s.web_face(),
        ),
        None => blueice_font::measure_text_width(run, 16.0, false, false),
    };
    let count = runs.len();
    // Down the page from `along`; up from its far end when the text runs up.
    let mut cursor = if ccw { along + run } else { along };
    runs.into_iter()
        .map(|(text, orient)| {
            // A single run is the whole fragment, so its length is exactly the layout's.
            let w = if count == 1 { run } else { measure(&text, orient) };
            let y = if ccw { cursor - w } else { cursor };
            cursor = if ccw { y } else { cursor + w };
            Fragment {
                kind: FragmentKind::Text(text),
                x: across,
                y,
                width: thick,
                height: w,
                orient,
                rtl: false,
                children: Vec::new(),
                ..f.clone()
            }
        })
        .collect()
}

/// How thick a vertical box would be with all the room along its lines it asks for: the page
/// width its stacked lines take, which is its shrink-to-fit width. `content` leaves out the box's
/// own padding and border.
pub(crate) fn block_extent(ctx: &Ctx<'_>, node: NodeId, mode: Writing) -> f64 {
    let style = ctx.style(node);
    let (padding, border) = (style.padding(0.0), style.border(0.0));
    let pb_v = padding.vertical() + border.vertical();
    let room = style
        .px("height", ctx.viewport_height)
        .map_or((ctx.viewport_height - pb_v).max(1.0), |h| {
            if style.is_border_box() { (h - pb_v).max(0.0) } else { h }
        });
    let previous = ctx.writing.replace(mode);
    let previous_room = ctx.orth_room.replace(ctx.orth_room.get());
    let flow = layout_flow(ctx, node, room, None, true, true, &[]);
    ctx.orth_room.set(previous_room);
    ctx.writing.set(previous);
    flow.height
}

/// A horizontal box inside a vertical one. It is laid out whole in the page's own frame, at the
/// width its own style asks for or the room the vertical box leaves across the page, and then
/// turned into the frame the vertical box is laid out in, so that turning that frame back sets
/// it right again.
pub(crate) fn layout_orthogonal(ctx: &Ctx<'_>, node: NodeId, c: &Constraint, forced: Forced) -> Placed {
    let outer = ctx.writing.get();
    let turn = Turn::of(outer);
    ctx.writing.set(Writing::Horizontal);
    let inner = Constraint {
        cb_width: ctx.orth_room.get(),
        cb_height: Some(c.cb_width),
        root: false,
    };
    let mut placed = layout_block_level(
        ctx,
        node,
        &inner,
        Forced { width: forced.height, height: forced.width },
    );
    let (page_width, page_height) = (placed.fragment.width, placed.fragment.height);
    let style = ctx.style_of_fragment(&placed.fragment);
    let (pad, border) = (style.padding(0.0), style.border(0.0));
    let block = (page_width - pad.left - pad.right - border.left - border.right).max(0.0);
    let length = (page_height - pad.top - pad.bottom - border.top - border.bottom).max(0.0);
    placed.fragment.width = page_height;
    placed.fragment.height = page_width;
    unrotate(ctx, &mut placed.fragment.children, block, length, false, turn);
    ctx.writing.set(outer);
    // The page's margins on the sides the turned frame names differently.
    let (left, right) = (placed.margin_left, placed.margin_right);
    let (top, bottom) = (placed.margin_top.value(), placed.margin_bottom.value());
    let (turned_left, turned_right) = if turn.inline_reverse { (bottom, top) } else { (top, bottom) };
    let (turned_top, turned_bottom) = match (turn.block_reverse, turn.inline_reverse) {
        (true, _) => (right, left),
        (false, true) => (left, right),
        (false, false) => (left, right),
    };
    placed.margin_left = turned_left;
    placed.margin_right = turned_right;
    placed.margin_top = Margin::of(turned_top);
    placed.margin_bottom = Margin::of(turned_bottom);
    placed.pending_abs = placed
        .pending_abs
        .into_iter()
        .map(|p| PendingAbs {
            node: p.node,
            x: if turn.inline_reverse { page_height - p.y } else { p.y },
            y: if turn.block_reverse { page_width - p.x } else { p.x },
        })
        .collect();
    placed.preferred_width = placed.fragment.width;
    placed.content_extent = placed.fragment.width;
    placed.shift = 0.0;
    placed
}

/// The inverse of [`rotate`]: `children`, in the page's frame and relative to their parent's
/// content box that is `block` wide and `length` tall, put into the turned frame.
fn unrotate(ctx: &Ctx<'_>, children: &mut [Fragment], block: f64, length: f64, line: bool, turn: Turn) {
    let reverse = if line { turn.line_reverse } else { turn.block_reverse };
    for child in children.iter_mut() {
        let (x, y, w, h) = (child.x, child.y, child.width, child.height);
        let (inner_block, inner_length, inner_line) = match &child.kind {
            FragmentKind::Line => (w, h, true),
            _ => {
                let style = ctx.style_of_fragment(child);
                let (pad, border) = (style.padding(0.0), style.border(0.0));
                (
                    (w - pad.left - pad.right - border.left - border.right).max(0.0),
                    (h - pad.top - pad.bottom - border.top - border.bottom).max(0.0),
                    false,
                )
            }
        };
        unrotate(ctx, &mut child.children, inner_block, inner_length, inner_line, turn);
        child.y = if reverse { block - (x + w) } else { x };
        child.x = if turn.inline_reverse { length - (y + h) } else { y };
        child.width = h;
        child.height = w;
        child.baseline = None;
        if matches!(child.kind, FragmentKind::Text(_)) {
            child.turned_back = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::block::layout_block;
    use crate::fragment::{Fragment, FragmentKind, TextOrient};
    use blueice_css::{cascade, ua_stylesheet, Origin};
    use blueice_dom::{NodeData, NodeId};

    fn find(doc: &blueice_dom::Document, from: NodeId, tag: &str) -> Option<NodeId> {
        if matches!(doc.data(from), NodeData::Element { tag_name, .. } if tag_name == tag) {
            return Some(from);
        }
        doc.children(from).find_map(|c| find(doc, c, tag))
    }

    /// The body of `html` laid out 400px wide (viewport 720px tall), with the UA sheet and `css`.
    fn body(html: &str, css: &str) -> Fragment {
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = blueice_css::parse(&format!(
            "html,body,div,p{{margin:0;padding:0}} body{{font-size:16px}} {css}"
        ))
        .rules;
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        let body = find(&doc, doc.root(), "body").unwrap();
        layout_block(&doc, body, &styles, 400.0)
    }

    /// Every text fragment under `f` as `(text, orient, x, y, width, height)` in the coordinates
    /// of the root's content box.
    fn texts(f: &Fragment) -> Vec<(String, TextOrient, f64, f64, f64, f64)> {
        fn walk(
            f: &Fragment,
            (ox, oy): (f64, f64),
            out: &mut Vec<(String, TextOrient, f64, f64, f64, f64)>,
        ) {
            let (x, y) = (ox + f.x, oy + f.y);
            if let FragmentKind::Text(t) = &f.kind {
                out.push((t.clone(), f.orient, x, y, f.width, f.height));
            }
            for c in &f.children {
                walk(c, (x, y), out);
            }
        }
        let mut out = Vec::new();
        for c in &f.children {
            walk(c, (0.0, 0.0), &mut out);
        }
        out
    }

    fn div(f: &Fragment) -> &Fragment {
        &f.children[0]
    }

    const LATIN: &str = "aaa bbb ccc ddd eee fff";

    #[test]
    fn a_vertical_box_keeps_its_physical_size_and_its_text_runs_down() {
        let f = body(
            &format!("<div>{LATIN}</div>"),
            "div { writing-mode: vertical-rl; width: 100px; height: 300px }",
        );
        let d = div(&f);
        assert_eq!((d.width, d.height), (100.0, 300.0));
        let t = texts(d);
        assert!(!t.is_empty());
        for (_, orient, x, y, w, h) in &t {
            assert_eq!(*orient, TextOrient::Sideways, "Latin lies on its side");
            assert!(*x >= 0.0 && x + w <= 100.0 + 0.01, "inside the box across: {x} {w}");
            assert!(*y >= 0.0 && y + h <= 300.0 + 0.01, "inside the box along: {y} {h}");
        }
        // "aaa bbb ccc ddd eee fff" is about 150px of text, so it is one column of six words
        assert_eq!(t.len(), 6);
        assert!(t.iter().all(|r| r.2 == t[0].2), "all in one column");
        assert_eq!(t[0].3, 0.0, "starts at the top");
        assert!(t.windows(2).all(|w| w[1].3 > w[0].3), "each word is below the one before");
    }

    #[test]
    fn a_text_run_is_as_long_down_the_page_as_it_is_wide_across_a_horizontal_page() {
        let vertical = body("<div>hello</div>", "div { writing-mode: vertical-rl; width: 60px; height: 200px }");
        let horizontal = body("<div>hello</div>", "div { width: 200px }");
        let v = &texts(div(&vertical))[0];
        let h = &texts(div(&horizontal))[0];
        assert!((v.5 - h.4).abs() < 0.01, "length down == length across: {} vs {}", v.5, h.4);
        assert!((v.4 - h.5).abs() < 0.01, "column width == line height: {} vs {}", v.4, h.5);
    }

    #[test]
    fn vertical_rl_stacks_lines_from_the_right_and_vertical_lr_from_the_left() {
        let css = |mode: &str| {
            format!("div {{ writing-mode: {mode}; width: 200px; height: 60px; line-height: 20px }}")
        };
        let lines = |mode: &str| {
            let f = body(&format!("<div>{LATIN}</div>"), &css(mode));
            let first = texts(div(&f)); // in reading order
            let mut columns: Vec<f64> = first.iter().map(|t| t.2).collect();
            columns.dedup(); // the words of one line share a column
            assert!(columns.len() >= 3, "{first:?}");
            columns
        };
        let rl = lines("vertical-rl");
        assert!(rl.windows(2).all(|w| w[0] > w[1]), "each line is left of the one before: {rl:?}");
        assert!(rl[0] + 20.0 > 199.0, "the first line is at the right edge");
        let lr = lines("vertical-lr");
        assert!(lr.windows(2).all(|w| w[0] < w[1]), "each line is right of the one before: {lr:?}");
        assert!(lr[0] < 1.0, "the first line is at the left edge");
    }

    #[test]
    fn ideographs_stand_upright_and_latin_lies_down_in_the_same_line() {
        let f = body(
            "<div>\u{65e5}\u{672c}\u{8a9e}abc</div>",
            "div { writing-mode: vertical-rl; width: 60px; height: 300px }",
        );
        let t = texts(div(&f));
        assert_eq!(t.len(), 2, "{t:?}");
        assert_eq!((t[0].0.as_str(), t[0].1), ("\u{65e5}\u{672c}\u{8a9e}", TextOrient::Upright));
        assert_eq!((t[1].0.as_str(), t[1].1), ("abc", TextOrient::Sideways));
        assert_eq!(t[0].3, 0.0);
        assert!((t[1].3 - (t[0].3 + t[0].5)).abs() < 0.01, "abc follows 日本語 down the line");
        // an upright run is three characters tall, one em each
        assert!((t[0].5 - 48.0).abs() < 1.0, "{}", t[0].5);
    }

    #[test]
    fn text_orientation_upright_stands_latin_up_too() {
        let f = body(
            "<div>abc</div>",
            "div { writing-mode: vertical-rl; text-orientation: upright; width: 60px; height: 300px }",
        );
        assert_eq!(texts(div(&f))[0].1, TextOrient::Upright);
    }

    #[test]
    fn an_auto_height_vertical_box_is_as_tall_as_its_longest_line() {
        let f = body("<div>hello</div>", "div { writing-mode: vertical-rl; width: 60px }");
        let d = div(&f);
        let run = texts(d)[0].5;
        assert_eq!(d.width, 60.0);
        assert!((d.height - run).abs() < 0.01, "height {} is the line's length {run}", d.height);
    }

    #[test]
    fn lines_break_at_the_height_of_the_box() {
        let f = body(
            "<div>\u{65e5}\u{672c}\u{8a9e}\u{65e5}\u{672c}\u{8a9e}\u{65e5}\u{672c}\u{8a9e}\u{65e5}\u{672c}\u{8a9e}</div>",
            "div { writing-mode: vertical-rl; width: 200px; height: 100px }",
        );
        let t = texts(div(&f));
        assert!(t.len() >= 2, "twelve characters do not fit in 100px: {t:?}");
        assert!(t.iter().all(|r| r.3 + r.5 <= 100.01), "no line is longer than the box");
    }

    #[test]
    fn padding_follows_the_physical_sides() {
        // padding-top is at the start of each line in a vertical box
        let f = body(
            "<div>x</div>",
            "div { writing-mode: vertical-rl; width: 100px; height: 200px; padding: 10px 0 0 0; box-sizing: border-box }",
        );
        let t = &texts(div(&f))[0];
        assert_eq!(t.3, 10.0, "the text starts below the top padding");
        // padding-right is at the start of the block direction: the first column is inset by it
        let f = body(
            "<div>x</div>",
            "div { writing-mode: vertical-rl; width: 100px; height: 200px; padding: 0 30px 0 0; box-sizing: border-box }",
        );
        let t = &texts(div(&f))[0];
        assert!(t.2 + t.4 <= 70.01, "the column ends 30px short of the right edge: {} + {}", t.2, t.4);
    }

    /// Every fragment under `f` as `(node tag or kind, x, y, width, height)` in the coordinates
    /// of `f`'s content box.
    fn boxes(f: &Fragment) -> Vec<(f64, f64, f64, f64)> {
        fn walk(f: &Fragment, (ox, oy): (f64, f64), out: &mut Vec<(f64, f64, f64, f64)>) {
            let (x, y) = (ox + f.x, oy + f.y);
            out.push((x, y, f.width, f.height));
            for c in &f.children {
                walk(c, (x, y), out);
            }
        }
        let mut out = Vec::new();
        for c in &f.children {
            walk(c, (0.0, 0.0), &mut out);
        }
        out
    }

    const VBOX: &str = "div.v { writing-mode: vertical-rl; width: 300px; height: 240px }";

    #[test]
    fn a_replaced_element_in_a_vertical_box_keeps_its_page_size() {
        let f = body("<div class=v>a<img width=40 height=20>b</div>", VBOX);
        let found = boxes(div(&f)).into_iter().any(|b| (b.2, b.3) == (40.0, 20.0));
        assert!(found, "an image 40 wide and 20 tall is 40 wide and 20 tall on the page");
    }

    #[test]
    fn a_horizontal_inline_block_in_a_vertical_line_keeps_its_text_horizontal() {
        let f = body(
            "<div class=v>\u{65e5}<span style='display:inline-block;writing-mode:horizontal-tb;width:80px'>abc def</span></div>",
            VBOX,
        );
        let t = texts(div(&f));
        let horizontal: Vec<_> = t.iter().filter(|r| r.0 == "abc" || r.0 == "def").collect();
        assert_eq!(horizontal.len(), 2, "{t:?}");
        assert!(horizontal.iter().all(|r| r.1 == TextOrient::Horizontal));
        // "abc" is as wide across the page as it is when written horizontally
        let flat = body("<div>abc</div>", "div { width: 200px }");
        let w = texts(div(&flat))[0].4;
        assert!((horizontal[0].4 - w).abs() < 0.01, "{} vs {w}", horizontal[0].4);
        assert!(horizontal[0].5 < 40.0, "and one line high, not as long as the word");
    }

    #[test]
    fn a_horizontal_block_in_a_vertical_box_flows_across_the_page() {
        let f = body(
            "<div class=v><p style='writing-mode:horizontal-tb;width:120px'>aaa bbb ccc ddd eee fff</p></div>",
            VBOX,
        );
        let t = texts(div(&f));
        assert!(t.len() >= 6 && t.iter().all(|r| r.1 == TextOrient::Horizontal), "{t:?}");
        // wrapped in 120px: several rows, each starting at the same x, stacked downward
        let mut rows: Vec<f64> = t.iter().map(|r| r.3).collect();
        rows.dedup();
        assert!(rows.len() >= 2, "more than one row: {rows:?}");
        assert!(rows.windows(2).all(|w| w[1] > w[0]), "rows go down the page: {rows:?}");
        assert!(t.iter().all(|r| r.2 + r.4 <= 300.01), "inside the box");
        let right = t.iter().map(|r| r.2 + r.4).fold(0.0, f64::max);
        assert!((100.0..=300.01).contains(&right));
    }

    #[test]
    fn an_absolutely_positioned_box_in_a_vertical_box_uses_page_offsets() {
        let f = body(
            "<div class=v style='position:relative'>text<span style='position:absolute;left:10px;top:20px;width:30px;height:35px;background:red'></span></div>",
            VBOX,
        );
        let found = boxes(div(&f)).into_iter().any(|b| b == (10.0, 20.0, 30.0, 35.0));
        assert!(found, "{:?}", boxes(div(&f)));
    }

    #[test]
    fn a_float_in_a_vertical_box_goes_to_the_line_left_which_is_the_top() {
        let f = body(
            "<div class=v><div style='float:left;width:50px;height:30px'></div>text</div>",
            VBOX,
        );
        let float = boxes(div(&f)).into_iter().find(|b| (b.2, b.3) == (50.0, 30.0)).expect("the float");
        assert_eq!(float.1, 0.0, "at the top of the box");
        assert!(float.0 + 50.0 > 299.0, "in the first column, which is at the right");
    }

    #[test]
    fn a_table_in_a_vertical_box_runs_its_columns_down_the_page() {
        let f = body("<div class=v><table><tr><td>aa</td><td>bb</td></tr></table></div>", VBOX);
        let t = texts(div(&f));
        let (a, b) = (&t[0], &t[1]);
        assert_eq!((a.0.as_str(), b.0.as_str()), ("aa", "bb"));
        assert!(b.3 > a.3 + 10.0, "the second cell is below the first: {a:?} {b:?}");
        assert!((a.2 - b.2).abs() < 1.0, "in the same column of the page");
    }

    #[test]
    fn a_flex_row_in_a_vertical_box_runs_down_the_page() {
        let f = body(
            "<div class=v><div style='display:flex'><i style='display:block;width:20px;height:30px'></i><i style='display:block;width:20px;height:30px'></i></div></div>",
            VBOX,
        );
        let items: Vec<_> = boxes(div(&f)).into_iter().filter(|b| (b.2, b.3) == (20.0, 30.0)).collect();
        assert_eq!(items.len(), 2, "{items:?}");
        assert!(items[1].1 > items[0].1 + 19.0, "the second item is below the first: {items:?}");
    }

    #[test]
    fn sideways_lr_runs_up_the_page_and_stacks_its_lines_from_the_left() {
        let css = "div { writing-mode: sideways-lr; width: 200px; height: 80px; line-height: 20px }";
        let f = body(&format!("<div>{LATIN}</div>"), css);
        let t = texts(div(&f));
        assert!(t.iter().all(|r| r.1 == TextOrient::SidewaysCcw), "{t:?}");
        // each line starts at the bottom of the box and its words climb
        let first_line: Vec<_> = t.iter().filter(|r| r.2 == t[0].2).collect();
        assert!(first_line.len() >= 2);
        assert!(first_line.windows(2).all(|w| w[1].3 < w[0].3), "each word is above the one before");
        let bottom = first_line[0].3 + first_line[0].5;
        assert!((bottom - 80.0).abs() < 0.01, "the line starts at the bottom: {bottom}");
        let mut columns: Vec<f64> = t.iter().map(|r| r.2).collect();
        columns.dedup();
        assert!(columns.len() >= 2 && columns.windows(2).all(|w| w[1] > w[0]), "lines go rightward: {columns:?}");
        assert!(columns[0] < 1.0, "the first line is at the left edge");
    }

    #[test]
    fn sideways_lr_takes_its_inline_start_from_the_bottom_padding() {
        let f = body(
            "<div>x</div>",
            "div { writing-mode: sideways-lr; width: 100px; height: 200px; padding: 0 0 15px 0; box-sizing: border-box }",
        );
        let t = &texts(div(&f))[0];
        assert!((t.3 + t.5 - 185.0).abs() < 0.01, "starts 15px above the bottom: {}", t.3 + t.5);
    }

    #[test]
    fn a_horizontal_box_in_a_sideways_lr_box_is_still_upright_on_the_page() {
        let f = body(
            "<div style='writing-mode:sideways-lr;width:300px;height:200px'><p style='writing-mode:horizontal-tb;width:100px'>abc def</p></div>",
            "",
        );
        let t = texts(div(&f));
        assert!(t.iter().all(|r| r.1 == TextOrient::Horizontal), "{t:?}");
        let flat = body("<div>abc</div>", "div { width: 200px }");
        assert!((t[0].4 - texts(div(&flat))[0].4).abs() < 0.01, "as wide as it is written");
    }

    #[test]
    fn text_combine_upright_sets_digits_upright_in_one_em() {
        let f = body(
            "<div>\u{5e73}\u{6210}<span style='text-combine-upright:all'>2026</span></div>",
            "div { writing-mode: vertical-rl; width: 60px; height: 300px }",
        );
        let t = texts(div(&f));
        let year = t.iter().find(|r| r.0 == "2026").expect("the digits");
        assert_eq!(year.1, TextOrient::Combined);
        assert!((year.5 - 16.0).abs() < 0.01, "one em long, not four digits: {}", year.5);
        // and the line is as long as 平成 plus one em
        let era = t.iter().find(|r| r.0 == "\u{5e73}\u{6210}").unwrap();
        assert!((year.3 - (era.3 + era.5)).abs() < 0.01);
    }

    #[test]
    fn combine_all_takes_the_whole_text_of_the_element_into_one_cell() {
        let f = body(
            "<div>\u{5e73}<span style='text-combine-upright:all'>12 <b>34</b></span>\u{5e74}</div>",
            "div { writing-mode: vertical-rl; width: 60px; height: 300px }",
        );
        let t = texts(div(&f));
        let combined: Vec<_> = t.iter().filter(|r| r.1 == TextOrient::Combined).collect();
        assert_eq!(combined.len(), 1, "one cell for the whole element: {t:?}");
        assert_eq!(combined[0].0, "12 34");
        assert!((combined[0].5 - 16.0).abs() < 0.01, "one em long");
        let year = t.iter().find(|r| r.0 == "\u{5e74}").unwrap();
        assert!((year.3 - (combined[0].3 + 16.0)).abs() < 0.01, "the next character follows the cell directly");
    }

    #[test]
    fn digits_n_combines_runs_of_up_to_n_digits_and_leaves_longer_ones_sideways() {
        let css = "div { writing-mode: vertical-rl; width: 60px; height: 300px; text-combine-upright: digits 2 }";
        let f = body("<div>\u{7b2c}12\u{6708}</div>", css);
        let t = texts(div(&f));
        let runs: Vec<(&str, TextOrient)> = t.iter().map(|r| (r.0.as_str(), r.1)).collect();
        assert_eq!(
            runs,
            [("\u{7b2c}", TextOrient::Upright), ("12", TextOrient::Combined), ("\u{6708}", TextOrient::Upright)]
        );
        let f = body("<div>\u{7b2c}123\u{6708}</div>", css);
        let t = texts(div(&f));
        assert!(t.iter().any(|r| r.0 == "123" && r.1 == TextOrient::Sideways), "{t:?}");
        // a single digit is combined too, which stands it upright
        let f = body("<div>\u{7b2c}5\u{6708}</div>", css);
        assert!(texts(div(&f)).iter().any(|r| r.0 == "5" && r.1 == TextOrient::Combined));
    }

    #[test]
    fn without_text_combine_upright_digits_lie_down() {
        let f = body("<div>12</div>", "div { writing-mode: vertical-rl; width: 60px; height: 300px }");
        assert_eq!(texts(div(&f))[0].1, TextOrient::Sideways);
    }

    #[test]
    fn a_form_control_in_a_vertical_line_keeps_its_page_size_and_the_line_goes_on() {
        let f = body(
            "<div class=v>\u{65e5}<input value=\"abc\" style=\"width:60px\">\u{672c}</div>",
            VBOX,
        );
        let flat = body("<div style='width:400px'><input value=\"abc\" style=\"width:60px\"></div>", "");
        let control = |f: &Fragment| -> (f64, f64) {
            fn find(f: &Fragment) -> Option<(f64, f64)> {
                if matches!(f.kind, FragmentKind::NativeControl { .. }) {
                    return Some((f.width, f.height));
                }
                f.children.iter().find_map(find)
            }
            find(f).expect("the control")
        };
        let (v, h) = (control(&f), control(&flat));
        assert!((v.0 - h.0).abs() < 0.5 && (v.1 - h.1).abs() < 0.5, "{v:?} vs {h:?}");
        let t = texts(div(&f));
        assert!(t.iter().any(|r| r.0 == "\u{672c}"), "the text after the control is still there: {t:?}");
        let (before, after) = (
            t.iter().find(|r| r.0 == "\u{65e5}").unwrap(),
            t.iter().find(|r| r.0 == "\u{672c}").unwrap(),
        );
        // on a vertical line a horizontal box takes room along the line by its height; its width
        // reaches across the column
        let gap = after.3 - (before.3 + before.5);
        assert!(gap >= h.1 - 0.5 && gap < h.1 + 12.0, "the control takes its {} of height: {t:?}", h.1);
        assert!(h.0 <= v.0 + 0.5, "and still its {} across", h.0);
    }

    #[test]
    fn a_vertical_rl_box_in_a_vertical_lr_box_stacks_its_own_lines_from_its_own_right() {
        let inner = "<p style='writing-mode:vertical-rl;width:150px;height:120px'>aaa bbb ccc ddd eee fff ggg hhh</p>";
        let f = body(
            &format!("<div style='writing-mode:vertical-lr;width:300px;height:200px'>{inner}</div>"),
            "",
        );
        let t = texts(div(&f));
        let first = &t[0];
        assert!(first.2 + first.4 > 149.0, "the first column is at the right of the 150px box: {first:?}");
        let mut columns: Vec<f64> = t.iter().map(|r| r.2).collect();
        columns.dedup();
        assert!(columns.len() >= 2 && columns.windows(2).all(|w| w[1] < w[0]), "later lines go left: {columns:?}");
        // and the box itself sits at the left of the outer one, where lr puts its first block
        let p = &div(&f).children[0];
        assert_eq!(p.x, 0.0);
        // the other way round
        let inner = "<p style='writing-mode:vertical-lr;width:150px;height:120px'>aaa bbb ccc ddd eee fff ggg hhh</p>";
        let f = body(
            &format!("<div style='writing-mode:vertical-rl;width:300px;height:200px'>{inner}</div>"),
            "",
        );
        let t = texts(div(&f));
        let first = &t[0];
        assert!(first.2 >= 150.0 - 0.01 && first.2 < 170.0, "the lr box is at the right, its first column at its left: {first:?}");
        let mut columns: Vec<f64> = t.iter().map(|r| r.2).collect();
        columns.dedup();
        assert!(columns.windows(2).all(|w| w[1] > w[0]), "its lines go right: {columns:?}");
    }

    #[test]
    fn horizontal_flex_items_in_a_vertical_flex_container_are_sized_by_their_page_height() {
        let item = "<p style='writing-mode:horizontal-tb;margin:0;width:100px'>aaa bbb ccc ddd eee fff</p>";
        let f = body(
            &format!("<div class=v style='display:flex;align-items:flex-start'>{item}{item}</div>"),
            VBOX,
        );
        let t = texts(div(&f));
        // each item wraps in its own 100px width: some rows high; the second follows the first
        let first_rows: Vec<f64> = t.iter().filter(|r| r.0 == "aaa").map(|r| r.3).collect();
        assert_eq!(first_rows.len(), 2, "{t:?}");
        let (a, b) = (first_rows[0], first_rows[1]);
        let item_height = {
            let flat = body(&format!("<div style='width:400px'>{item}</div>"), "");
            div(&flat).children[0].height
        };
        assert!(
            (b - a - item_height).abs() < 0.5,
            "the second item starts one item height ({item_height}) below the first: {a} {b}"
        );
        // and each item is as wide across the page as it is written (100px)
        let widths: Vec<f64> = boxes(div(&f)).iter().filter(|b| (b.2 - 100.0).abs() < 0.01).map(|b| b.2).collect();
        assert!(widths.len() >= 2, "{:?}", boxes(div(&f)));
    }

    #[test]
    fn a_vertical_table_or_grid_container_runs_its_columns_down_the_page() {
        let table = body(
            "<table style='writing-mode:vertical-rl;width:200px;height:240px'><tr><td>aa</td><td>bb</td></tr></table>",
            "",
        );
        let t = texts(div(&table));
        let (a, b) = (&t[0], &t[1]);
        assert!(b.3 > a.3 + 10.0 && (a.2 - b.2).abs() < 1.0, "{a:?} {b:?}");
        let grid = body(
            "<div style='display:grid;grid-template-columns:50px 60px;writing-mode:vertical-rl;width:200px;height:240px'><i style='display:block;height:20px'>a</i><i style='display:block;height:20px'>b</i></div>",
            "",
        );
        let t = texts(div(&grid));
        let (a, b) = (&t[0], &t[1]);
        assert!(b.3 >= a.3 + 49.0, "the second column of the grid starts below the first: {a:?} {b:?}");
    }

    #[test]
    fn a_floated_vertical_box_with_no_width_is_as_wide_as_its_columns() {
        let text = "aaa bbb ccc ddd eee fff ggg hhh";
        let css = "float: left; writing-mode: vertical-rl; height: 100px; line-height: 20px";
        let f = body(&format!("<div style='{css}'>{text}</div><p>after</p>"), "");
        let float = &f.children[0];
        let mut columns: Vec<f64> = texts(float).iter().map(|r| r.2).collect();
        columns.dedup();
        assert!(columns.len() >= 3, "wraps into several columns: {columns:?}");
        assert!(
            (float.width - 20.0 * columns.len() as f64).abs() < 0.01,
            "{} wide for {} columns of 20px",
            float.width,
            columns.len()
        );
        // the same box as an inline-block, and with padding
        let f = body(
            &format!("<span style='display:inline-block;writing-mode:vertical-rl;height:100px;line-height:20px;padding:0 5px'>{text}</span>"),
            "",
        );
        let line = &f.children[0];
        let inline = line
            .children
            .iter()
            .find(|c| c.kind == FragmentKind::Block && !c.children.is_empty())
            .expect("the inline-block");
        let mut columns: Vec<f64> = texts(inline).iter().map(|r| r.2).collect();
        columns.dedup();
        assert!(
            (inline.width - (20.0 * columns.len() as f64 + 10.0)).abs() < 0.01,
            "{} wide for {columns:?}: {:?}",
            inline.width,
            texts(&f.children[0])
        );
    }

    #[test]
    fn proportional_widths_shorten_upright_punctuation_in_a_vertical_line() {
        let run = |css: &str| {
            let f = body(
                "<div>\u{300c}\u{3042}\u{3002}\u{300d}</div>",
                &format!("div {{ writing-mode: vertical-rl; width: 60px; height: 300px; text-spacing-trim: space-all; {css} }}"),
            );
            texts(div(&f))[0].5
        };
        let plain = run("");
        assert!((plain - 64.0).abs() < 0.5, "four 16px cells: {plain}");
        let proportional = run("font-variant-east-asian: proportional-width");
        assert!(proportional < plain - 2.0, "{proportional} vs {plain}");
        assert!((run("font-feature-settings: \"vpal\" 1") - proportional).abs() < 0.01);
    }

    #[test]
    fn adjacent_closing_marks_are_trimmed_in_a_vertical_line_with_vhal() {
        let run = |css: &str| {
            let f = body(
                "<div>\u{3042}\u{3002}\u{300d}</div>",
                &format!("div {{ writing-mode: vertical-rl; width: 60px; height: 300px; {css} }}"),
            );
            texts(div(&f)).iter().map(|r| r.5).sum::<f64>()
        };
        assert!(run("") < run("text-spacing-trim: space-all") - 2.0);
    }

    #[test]
    fn horizontal_content_around_a_vertical_box_is_untouched() {
        let f = body(
            "<p>before</p><div>x</div><p>after</p>",
            "div { writing-mode: vertical-rl; width: 50px; height: 80px }",
        );
        assert_eq!(f.children[0].y, 0.0);
        assert_eq!(f.children[1].y, f.children[0].height);
        assert_eq!(f.children[2].y, f.children[1].y + 80.0);
        assert!(texts(&f.children[0]).iter().all(|t| t.1 == TextOrient::Horizontal));
    }
}

