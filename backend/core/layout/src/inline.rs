// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Inline formatting: a run of inline-level content (text, inline elements,
//! atomic inline boxes such as `inline-block` and replaced elements, `<br>`)
//! becomes a stack of line boxes.
//!
//! Text is cut into words and collapsible spaces following `white-space`;
//! lines break greedily at spaces and between CJK characters; each line sits
//! on one baseline shared by text of every size and by atomic boxes (with
//! `vertical-align`), its height the tallest ascent plus the deepest descent
//! including the container's strut; `text-align` and `text-indent` place the
//! content on the line.

use crate::block::{layout_block_level, Constraint, Forced, PendingAbs};
use crate::ctx::{Ctx, Display, Style};
use crate::float::{self, FloatRect};
use crate::fragment::{Fragment, FragmentKind};
use blueice_dom::{NodeData, NodeId};

/// One piece of an inline run, before line breaking.
enum Piece {
    Word {
        text: String,
        width: f64,
        style_node: NodeId,
        /// A break is allowed between the previous piece and this one even
        /// without a space (CJK text).
        can_break_before: bool,
        wrap: bool,
    },
    Space {
        width: f64,
        style_node: NodeId,
        wrap: bool,
    },
    /// Horizontal padding, border or margin of an inline box: unbreakable.
    Gap(f64),
    /// The same for an inline box that paints something of its own (a
    /// background, border, radius or shadow): `outside` of `width` is margin,
    /// the rest is the box's own border and padding.
    Edge {
        node: NodeId,
        open: bool,
        width: f64,
        outside: f64,
    },
    Atomic(Box<Atomic>),
    Break,
}

/// An atomic inline: laid out as a block-level box and placed whole on a line.
struct Atomic {
    fragment: Fragment,
    margin_left: f64,
    margin_right: f64,
    margin_top: f64,
    margin_bottom: f64,
    /// Distance from the top of the border box to the baseline it aligns by.
    baseline: f64,
    align: Align,
    pending: Vec<PendingAbs>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Align {
    /// Raised above the baseline by this much (negative: lowered).
    Baseline(f64),
    Top,
    Bottom,
}

/// What laying out a run produced.
pub(crate) struct InlineResult {
    /// Line fragments, `y` from the top of the run.
    pub lines: Vec<Fragment>,
    pub height: f64,
    pub pending_abs: Vec<PendingAbs>,
    /// The farthest right edge any line reaches.
    pub extent: f64,
}

struct State {
    /// The last piece was a collapsible space (or the run just began), so
    /// another collapsible space is dropped.
    last_space: bool,
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2FDF | 0x3000..=0x303F | 0x3040..=0x30FF | 0x31F0..=0x31FF
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF
        | 0x20000..=0x2FA1F)
}

/// Punctuation that must not start a line.
fn no_line_start(c: char) -> bool {
    matches!(
        c,
        '，' | '。'
            | '、'
            | '；'
            | '：'
            | '！'
            | '？'
            | '）'
            | '」'
            | '』'
            | '》'
            | '〉'
            | '】'
            | '〕'
            | '…'
    )
}

fn is_collapsible_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

fn transform_text(text: &str, transform: Option<&str>) -> String {
    match transform {
        Some("uppercase") => text.to_uppercase(),
        Some("lowercase") => text.to_lowercase(),
        Some("capitalize") => {
            let mut out = String::with_capacity(text.len());
            let mut start = true;
            for c in text.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
                start = c.is_whitespace();
            }
            out
        }
        _ => text.to_string(),
    }
}

struct TextProps {
    font_size: f64,
    bold: bool,
    italic: bool,
    letter_spacing: f64,
    word_spacing: f64,
}

impl TextProps {
    fn of(style: &Style<'_>) -> TextProps {
        TextProps {
            font_size: style.font_size(),
            bold: style.style.is_some_and(|s| s.is_bold()),
            italic: style.style.is_some_and(|s| s.is_italic()),
            letter_spacing: style.px("letter-spacing", f64::NAN).unwrap_or(0.0),
            word_spacing: style.px("word-spacing", f64::NAN).unwrap_or(0.0),
        }
    }

    fn width(&self, text: &str) -> f64 {
        blueice_font::measure_text_width(text, self.font_size, self.bold, self.italic)
            + self.letter_spacing * text.chars().count() as f64
    }

    fn space(&self) -> f64 {
        self.width(" ") + self.word_spacing
    }
}

fn push_text(
    ctx: &Ctx<'_>,
    data: &str,
    style_node: NodeId,
    pieces: &mut Vec<Piece>,
    state: &mut State,
) {
    let style = ctx.style(style_node);
    let white_space = style.keyword("white-space").unwrap_or("normal");
    let (collapse, newlines, wrap) = match white_space {
        "nowrap" => (true, false, false),
        "pre" => (false, true, false),
        "pre-wrap" | "break-spaces" => (false, true, true),
        "pre-line" => (true, true, true),
        _ => (true, false, true),
    };
    let text = transform_text(data, style.keyword("text-transform"));
    let props = TextProps::of(&style);
    let mut word = String::new();
    let mut previous_cjk = false;
    let flush = |word: &mut String, pieces: &mut Vec<Piece>, can_break: bool| {
        if !word.is_empty() {
            let width = props.width(word);
            pieces.push(Piece::Word {
                text: std::mem::take(word),
                width,
                style_node,
                can_break_before: can_break,
                wrap,
            });
        }
    };
    let mut break_before_next_word = false;
    for c in text.chars() {
        if c == '\u{200b}' || c == '\u{feff}' || c == '\u{ad}' {
            continue;
        }
        if c == '\n' && newlines {
            flush(&mut word, pieces, break_before_next_word);
            break_before_next_word = false;
            pieces.push(Piece::Break);
            state.last_space = false;
            previous_cjk = false;
        } else if is_collapsible_space(c) {
            flush(&mut word, pieces, break_before_next_word);
            break_before_next_word = false;
            previous_cjk = false;
            if collapse {
                if !state.last_space {
                    pieces.push(Piece::Space {
                        width: props.space(),
                        style_node,
                        wrap,
                    });
                    state.last_space = true;
                }
            } else {
                let columns = if c == '\t' { 8.0 } else { 1.0 };
                pieces.push(Piece::Space {
                    width: props.space() * columns,
                    style_node,
                    wrap,
                });
                state.last_space = false;
            }
        } else {
            state.last_space = false;
            let cjk = is_cjk(c);
            // Break between CJK characters (not before closing punctuation).
            let boundary = wrap && (cjk || previous_cjk) && !word.is_empty() && !no_line_start(c);
            if boundary || (cjk && !word.is_empty()) {
                flush(&mut word, pieces, break_before_next_word);
                break_before_next_word = boundary;
            }
            word.push(c);
            previous_cjk = cjk;
        }
    }
    flush(&mut word, pieces, break_before_next_word);
}

/// Intrinsic size of a replaced element, from CSS then attributes.
pub(crate) fn replaced_size(
    ctx: &Ctx<'_>,
    node: NodeId,
    style: &Style<'_>,
    cb_width: f64,
) -> Option<(f64, f64)> {
    let tag = ctx.tag(node)?;
    let default = match tag {
        "img" | "image" => (0.0, 0.0),
        "svg" | "canvas" | "iframe" | "video" | "embed" | "object" => (300.0, 150.0),
        _ => return None,
    };
    let NodeData::Element { attributes, .. } = ctx.node(node) else {
        return None;
    };
    let attr = |name: &str| {
        attributes
            .iter()
            .find(|(k, _)| k == name)
            .and_then(|(_, v)| v.trim().trim_end_matches("px").parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
    };
    let (attr_w, attr_h) = (attr("width"), attr("height"));
    // What the engine learned from the image itself, when it has loaded.
    let intrinsic = match style.raw("-blueice-intrinsic-size") {
        Some(blueice_css::Value::List(parts)) => match parts.as_slice() {
            [blueice_css::Value::Number(w), blueice_css::Value::Number(h)] if *w > 0.0 && *h > 0.0 => {
                Some((*w, *h))
            }
            _ => None,
        },
        _ => None,
    };
    let natural = intrinsic.unwrap_or(default);
    // An explicit CSS `auto` overrides the attribute (a presentational hint).
    let css_width = style.px("width", cb_width);
    let css_height = style.px("height", f64::NAN);
    let width = css_width.or(attr_w.filter(|_| !style.is_auto("width")));
    let height = css_height.or(attr_h.filter(|_| !style.is_auto("height")));
    // A single known dimension keeps the aspect ratio of the attributes, or of
    // the image itself.
    let ratio = match (attr_w, attr_h) {
        (Some(w), Some(h)) if h > 0.0 && w > 0.0 => Some(w / h),
        _ => intrinsic.map(|(w, h)| w / h),
    };
    let (width, height) = match (width, height) {
        (Some(w), Some(h)) => (w, Some(h)),
        (Some(w), None) => (w, ratio.map(|r| w / r)),
        (None, Some(h)) => (ratio.map_or(natural.0, |r| h * r), Some(h)),
        (None, None) => (natural.0, Some(natural.1)),
    };
    let clamped = style
        .px("max-width", cb_width)
        .map_or(width, |m| width.min(m))
        .max(style.px("min-width", cb_width).unwrap_or(0.0));
    let height = match height {
        // A width squeezed by `max-width` takes the height with it unless the
        // height was set outright.
        Some(h) if clamped != width && css_height.is_none() && ratio.is_some() => {
            let _ = h;
            clamped / ratio.unwrap_or(1.0)
        }
        Some(h) => h,
        None => natural.1,
    };
    Some((clamped, height))
}

fn vertical_align(style: &Style<'_>, line_height: f64) -> Align {
    let font_size = style.font_size();
    match style.raw("vertical-align") {
        Some(blueice_css::Value::Keyword(k)) => match k.as_str() {
            "top" | "text-top" => Align::Top,
            "bottom" | "text-bottom" => Align::Bottom,
            "sub" => Align::Baseline(-font_size * 0.2),
            "super" => Align::Baseline(font_size * 0.33),
            "middle" => Align::Baseline(font_size * 0.25),
            _ => Align::Baseline(0.0),
        },
        Some(blueice_css::Value::Percentage(p)) => Align::Baseline(line_height * p / 100.0),
        Some(other) => Align::Baseline(
            other
                .to_px(&style.length_context(line_height))
                .unwrap_or(0.0),
        ),
        None => Align::Baseline(0.0),
    }
}

/// An atomic inline built from `node`, or `None` when it is not one.
fn atomic_for(ctx: &Ctx<'_>, node: NodeId, cb_width: f64) -> Atomic {
    let style = ctx.style(node);
    let padding = style.padding(cb_width);
    let border = style.border(cb_width);
    let pb_h = padding.horizontal() + border.horizontal();
    let margin = |name: &str| {
        if style.is_auto(name) {
            0.0
        } else {
            style.side(name, cb_width)
        }
    };
    let (margin_left, margin_right) = (margin("margin-left"), margin("margin-right"));
    let (margin_top, margin_bottom) = (margin("margin-top"), margin("margin-bottom"));
    let constraint = Constraint {
        cb_width,
        cb_height: None,
        root: false,
    };
    let mut pending = Vec::new();
    let mut fragment;
    let mut line_baseline = None;
    if let Some((width, height)) = replaced_size(ctx, node, &style, cb_width) {
        fragment = Fragment {
            node: Some(node),
            width: width + pb_h,
            height: height + padding.vertical() + border.vertical(),
            ..Fragment::empty_block()
        };
        fragment.positioned = style.position() != crate::ctx::Position::Static;
        fragment.hidden = !style.is_visible();
    } else {
        let specified =
            style
                .px("width", cb_width)
                .map(|w| if style.is_border_box() { w } else { w + pb_h });
        let width = specified.unwrap_or_else(|| {
            let (min_content, max_content) = crate::intrinsic::preferred_widths(ctx, node);
            let available = (cb_width - margin_left - margin_right).max(0.0);
            max_content.min(available.max(min_content))
        });
        let placed = layout_block_level(
            ctx,
            node,
            &constraint,
            Forced {
                width: Some(width),
                height: None,
            },
        );
        pending = placed.pending_abs;
        line_baseline = last_baseline(&placed.fragment);
        fragment = placed.fragment;
    }
    let baseline = match line_baseline {
        Some(b) if !style.clips_overflow() => b,
        _ => fragment.height + margin_bottom,
    };
    let own_line_height = fragment.height + margin_top + margin_bottom;
    Atomic {
        margin_left,
        margin_right,
        margin_top,
        margin_bottom,
        baseline,
        align: vertical_align(&style, own_line_height),
        fragment,
        pending,
    }
}

/// The baseline of the first line box inside `fragment`, from its top edge.
pub(crate) fn first_baseline(fragment: &Fragment) -> Option<f64> {
    for child in &fragment.children {
        if matches!(child.kind, FragmentKind::Line) {
            if let Some(b) = child.baseline {
                return Some(child.y + b);
            }
        }
        if matches!(child.kind, FragmentKind::Block) && !child.positioned {
            if let Some(b) = first_baseline(child) {
                return Some(child.y + b);
            }
        }
    }
    None
}

/// The baseline of the last line box inside `fragment`, from its top edge.
fn last_baseline(fragment: &Fragment) -> Option<f64> {
    for child in fragment.children.iter().rev() {
        if matches!(child.kind, FragmentKind::Line) {
            if let Some(b) = child.baseline {
                return Some(child.y + b);
            }
        }
        if matches!(child.kind, FragmentKind::Block) && !child.positioned {
            if let Some(b) = last_baseline(child) {
                return Some(child.y + b);
            }
        }
    }
    None
}

/// Whether an inline-level node should be treated as one atomic box.
fn is_atomic(ctx: &Ctx<'_>, node: NodeId, style: &Style<'_>) -> bool {
    style.display().is_atomic_inline()
        || matches!(
            ctx.tag(node),
            Some("img" | "svg" | "canvas" | "video" | "iframe" | "embed" | "object")
        )
        || !matches!(
            style.display(),
            Display::Inline | Display::None | Display::Contents
        )
}

fn collect(
    ctx: &Ctx<'_>,
    node: NodeId,
    cb_width: f64,
    pieces: &mut Vec<Piece>,
    abs: &mut Vec<PendingAbs>,
    state: &mut State,
) {
    match ctx.node(node) {
        NodeData::Text { data } => {
            if let Some(parent) = ctx.parent(node) {
                push_text(ctx, data, parent, pieces, state);
            }
        }
        NodeData::Document => {}
        NodeData::Element { tag_name, .. } => {
            let style = ctx.style(node);
            let display = style.display();
            if display == Display::None || display == Display::TableColumn {
                return;
            }
            if style.is_out_of_flow() {
                abs.push(PendingAbs {
                    node,
                    x: 0.0,
                    y: 0.0,
                });
                return;
            }
            if tag_name == "br" {
                pieces.push(Piece::Break);
                state.last_space = false;
                return;
            }
            if display == Display::Contents {
                for child in ctx.children(node) {
                    collect(ctx, child, cb_width, pieces, abs, state);
                }
                return;
            }
            if is_atomic(ctx, node, &style) {
                pieces.push(Piece::Atomic(Box::new(atomic_for(ctx, node, cb_width))));
                state.last_space = false;
                return;
            }
            // A plain inline box: its horizontal margin, border and padding
            // take room before and after its content.
            let padding = style.padding(cb_width);
            let border = style.border(cb_width);
            let side = |name: &str| {
                if style.is_auto(name) {
                    0.0
                } else {
                    style.side(name, cb_width)
                }
            };
            let before = side("margin-left") + border.left + padding.left;
            let after = side("margin-right") + border.right + padding.right;
            let paints = paints_own_box(&style);
            if paints {
                pieces.push(Piece::Edge {
                    node,
                    open: true,
                    width: before,
                    outside: side("margin-left"),
                });
            } else if before != 0.0 {
                pieces.push(Piece::Gap(before));
            }
            for child in ctx.children(node) {
                collect(ctx, child, cb_width, pieces, abs, state);
            }
            if paints {
                pieces.push(Piece::Edge {
                    node,
                    open: false,
                    width: after,
                    outside: side("margin-right"),
                });
            } else if after != 0.0 {
                pieces.push(Piece::Gap(after));
            }
        }
    }
}

/// Whether an inline box has anything of its own to paint.
fn paints_own_box(style: &Style<'_>) -> bool {
    let has = |name: &str| {
        style.raw(name).is_some_and(
            |v| !matches!(v, blueice_css::Value::Keyword(k) if k == "none" || k == "transparent"),
        )
    };
    has("background-color")
        || has("background-image")
        || has("box-shadow")
        || ["top", "right", "bottom", "left"].iter().any(|side| {
            has(&format!("border-{side}-style"))
                && style.border(0.0).horizontal() + style.border(0.0).vertical() > 0.0
        })
}

/// `line-height` in pixels for text in `style`: a number scales the font
/// size, a length or percentage is used as is, and `normal` is 1.2 em.
fn line_height_of(style: &Style<'_>) -> f64 {
    let font_size = style.font_size();
    match style.style.and_then(|s| s.line_height.as_ref()) {
        Some(blueice_css::Value::Number(n)) => n * font_size,
        Some(blueice_css::Value::Keyword(_)) | None => crate::text::default_line_height(font_size),
        Some(other) => other
            .to_px(&style.length_context(font_size))
            .unwrap_or_else(|| crate::text::default_line_height(font_size)),
    }
}

/// How far text of `style` reaches above and below its baseline, with the
/// line-height's half-leading split evenly around the font's own extent.
fn text_extent(style: &Style<'_>) -> (f64, f64) {
    let line_height = line_height_of(style);
    let (ascent, descent) = blueice_font::ascent_descent(
        style.font_size(),
        style.style.is_some_and(|s| s.is_bold()),
        style.style.is_some_and(|s| s.is_italic()),
    );
    let half_leading = (line_height - (ascent + descent)) / 2.0;
    (ascent + half_leading, descent + half_leading)
}

struct Chunk {
    /// Collapsible space before the chunk (dropped at a line start).
    lead: f64,
    items: Vec<usize>,
    width: f64,
    forced_break_after: bool,
}

fn atomic_width(a: &Atomic) -> f64 {
    a.margin_left + a.fragment.width + a.margin_right
}

fn into_chunks(pieces: &[Piece]) -> Vec<Chunk> {
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut current: Option<Chunk> = None;
    let mut lead = 0.0;
    let mut need_new = false;
    for (i, piece) in pieces.iter().enumerate() {
        match piece {
            Piece::Space { width, wrap, .. } => {
                if *wrap {
                    lead += width;
                    need_new = true;
                } else if let Some(chunk) = current.as_mut() {
                    chunk.items.push(i);
                    chunk.width += width;
                } else {
                    current = Some(Chunk {
                        lead: std::mem::take(&mut lead),
                        items: vec![i],
                        width: *width,
                        forced_break_after: false,
                    });
                }
            }
            Piece::Word {
                width,
                can_break_before,
                wrap,
                ..
            } => {
                let start_new = current.is_none() || need_new || (*can_break_before && *wrap);
                if start_new {
                    chunks.extend(current.take());
                    current = Some(Chunk {
                        lead: std::mem::take(&mut lead),
                        items: vec![i],
                        width: *width,
                        forced_break_after: false,
                    });
                } else if let Some(chunk) = current.as_mut() {
                    chunk.items.push(i);
                    chunk.width += width;
                }
                need_new = false;
            }
            Piece::Gap(width) | Piece::Edge { width, .. } => {
                if let Some(chunk) = current.as_mut().filter(|_| !need_new) {
                    chunk.items.push(i);
                    chunk.width += width;
                } else {
                    chunks.extend(current.take());
                    current = Some(Chunk {
                        lead: std::mem::take(&mut lead),
                        items: vec![i],
                        width: *width,
                        forced_break_after: false,
                    });
                    need_new = false;
                }
            }
            Piece::Atomic(atomic) => {
                chunks.extend(current.take());
                current = Some(Chunk {
                    lead: std::mem::take(&mut lead),
                    items: vec![i],
                    width: atomic_width(atomic),
                    forced_break_after: false,
                });
                // A break is allowed after an atomic box.
                need_new = true;
            }
            Piece::Break => {
                let mut chunk = current.take().unwrap_or(Chunk {
                    lead: 0.0,
                    items: Vec::new(),
                    width: 0.0,
                    forced_break_after: false,
                });
                chunk.forced_break_after = true;
                chunks.push(chunk);
                lead = 0.0;
                need_new = false;
            }
        }
    }
    chunks.extend(current);
    chunks
}

enum LineItem {
    Piece(usize),
    Space(f64),
}

struct Line {
    items: Vec<LineItem>,
    width: f64,
    forced_end: bool,
    /// Inset from the left edge and width left by floats.
    left: f64,
    avail: f64,
    /// Where a line pushed below a float starts, from the run's top.
    start_y: Option<f64>,
}

impl Line {
    fn new(room: Room, start_y: Option<f64>) -> Line {
        Line {
            items: Vec::new(),
            width: 0.0,
            forced_end: false,
            left: room.left,
            avail: room.avail,
            start_y,
        }
    }
}

/// The room a line has at some height: how far floats push its left edge in,
/// how wide it then is, and where the next float ends below that height.
#[derive(Clone, Copy)]
struct Room {
    left: f64,
    avail: f64,
    next: Option<f64>,
}

/// Floats a run of inline content flows around; `top` is where the run starts
/// in the same coordinates as the floats.
pub(crate) struct FloatView<'a> {
    pub floats: &'a [FloatRect],
    pub top: f64,
}

fn break_lines(
    pieces: &[Piece],
    first_indent: f64,
    room: &dyn Fn(f64) -> Room,
    est_height: f64,
) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut y_est = 0.0f64;
    let mut line = Line::new(room(0.0), None);
    for chunk in into_chunks(pieces) {
        // A line with nothing on it drops below floats that leave it too
        // narrow for the next word.
        macro_rules! skip_floats {
            () => {
                if line.items.is_empty() {
                    let indent = if lines.is_empty() { first_indent } else { 0.0 };
                    loop {
                        let r = room(y_est);
                        match r.next {
                            Some(next) if chunk.width > r.avail - indent + 0.01 && next > y_est => {
                                y_est = next;
                                line = Line::new(room(y_est), Some(y_est));
                            }
                            _ => break,
                        }
                    }
                }
            };
        }
        skip_floats!();
        let first = lines.is_empty();
        let limit = line.avail - if first { first_indent } else { 0.0 };
        let has_content = !line.items.is_empty();
        let fits = !has_content || line.width + chunk.lead + chunk.width <= limit + 0.01;
        if !fits {
            y_est += est_height;
            let next = Line::new(room(y_est), None);
            lines.push(std::mem::replace(&mut line, next));
            skip_floats!();
        }
        if !line.items.is_empty() && chunk.lead > 0.0 {
            line.items.push(LineItem::Space(chunk.lead));
            line.width += chunk.lead;
        }
        for i in &chunk.items {
            line.items.push(LineItem::Piece(*i));
        }
        line.width += chunk.width;
        if chunk.forced_break_after {
            line.forced_end = true;
            y_est += est_height;
            let next = Line::new(room(y_est), None);
            lines.push(std::mem::replace(&mut line, next));
        }
    }
    if !line.items.is_empty() {
        lines.push(line);
    }
    lines
}

/// The shift `text-align` gives a line of `width` in `available` room.
fn align_offset(text_align: &str, width: f64, available: f64) -> f64 {
    let free = (available - width).max(0.0);
    match text_align {
        "right" | "end" | "-webkit-right" | "-moz-right" => free,
        "center" | "-webkit-center" | "-moz-center" => free / 2.0,
        _ => 0.0,
    }
}

fn piece_style<'a>(ctx: &'a Ctx<'a>, piece: &Piece) -> Option<Style<'a>> {
    match piece {
        Piece::Word { style_node, .. } | Piece::Space { style_node, .. } => {
            Some(ctx.style(*style_node))
        }
        _ => None,
    }
}

/// Lays out one run of inline-level `nodes` inside `container`.
pub(crate) fn layout_run(
    ctx: &Ctx<'_>,
    container: NodeId,
    nodes: &[NodeId],
    content_width: f64,
    first_in_block: bool,
) -> InlineResult {
    layout_run_around(ctx, container, nodes, content_width, first_in_block, None)
}

/// [`layout_run`] with lines narrowed (and, when a word does not fit beside
/// them, lowered) around `floats`.
pub(crate) fn layout_run_around(
    ctx: &Ctx<'_>,
    container: NodeId,
    nodes: &[NodeId],
    content_width: f64,
    first_in_block: bool,
    floats: Option<&FloatView<'_>>,
) -> InlineResult {
    let mut pieces = Vec::new();
    let mut abs = Vec::new();
    let mut state = State { last_space: true };
    for node in nodes {
        collect(ctx, *node, content_width, &mut pieces, &mut abs, &mut state);
    }
    let mut result = InlineResult {
        lines: Vec::new(),
        height: 0.0,
        pending_abs: abs,
        extent: 0.0,
    };
    if pieces.is_empty() {
        return result;
    }

    let cstyle = ctx.style(container);
    let text_align = cstyle
        .style
        .and_then(|s| s.text_align.as_deref())
        .unwrap_or("left")
        .to_string();
    let indent = if first_in_block {
        cstyle.px("text-indent", content_width).unwrap_or(0.0)
    } else {
        0.0
    };
    let (strut_above, strut_below) = text_extent(&cstyle);
    let room = |y: f64| match floats {
        Some(view) => {
            let top = view.top + y;
            let (left, right) = float::free_span(
                view.floats,
                top,
                top + strut_above + strut_below,
                content_width,
            );
            Room {
                left,
                avail: right - left,
                next: float::next_edge(view.floats, top).map(|e| e - view.top),
            }
        }
        None => Room {
            left: 0.0,
            avail: content_width,
            next: None,
        },
    };
    let lines = break_lines(&pieces, indent, &room, strut_above + strut_below);
    let line_count = lines.len();
    let mut y = 0.0f64;
    // Inline boxes still open across lines, with where each starts on the line.
    let mut open: Vec<(NodeId, f64, usize)> = Vec::new();
    let mut opened = 0usize;

    for (line_index, line) in lines.iter().enumerate() {
        let is_first = line_index == 0;
        let line_indent = if is_first { indent } else { 0.0 };
        let available = line.avail - line_indent;
        if let Some(start) = line.start_y {
            y = y.max(start);
        }

        // ---- vertical metrics ----
        let mut max_above = strut_above;
        let mut max_below = strut_below;
        let mut tall = 0.0f64;
        let mut metrics: Vec<Option<(f64, f64, Align)>> = Vec::new();
        for item in &line.items {
            let LineItem::Piece(i) = item else {
                metrics.push(None);
                continue;
            };
            let entry = match &pieces[*i] {
                Piece::Atomic(a) => {
                    let above = a.margin_top + a.baseline;
                    let below = (a.margin_top + a.fragment.height + a.margin_bottom) - above;
                    Some((above, below, a.align))
                }
                piece => piece_style(ctx, piece).map(|style| {
                    let (above, below) = text_extent(&style);
                    let align = match piece {
                        Piece::Word { style_node, .. } | Piece::Space { style_node, .. }
                            if *style_node != container =>
                        {
                            vertical_align(&style, above + below)
                        }
                        _ => Align::Baseline(0.0),
                    };
                    (above, below, align)
                }),
            };
            if let Some((above, below, align)) = entry {
                match align {
                    Align::Baseline(shift) => {
                        max_above = max_above.max(above + shift);
                        max_below = max_below.max(below - shift);
                    }
                    Align::Top | Align::Bottom => tall = tall.max(above + below),
                }
            }
            metrics.push(entry);
        }
        let line_height = (max_above + max_below).max(tall);
        let baseline = max_above;

        // ---- horizontal placement and fragments ----
        let extra_per_space =
            if text_align == "justify" && !line.forced_end && line_index + 1 < line_count {
                let spaces = line
                    .items
                    .iter()
                    .filter(|i| matches!(i, LineItem::Space(_)))
                    .count();
                if spaces > 0 {
                    ((available - line.width).max(0.0)) / spaces as f64
                } else {
                    0.0
                }
            } else {
                0.0
            };
        let used_width = line.width
            + extra_per_space
                * line
                    .items
                    .iter()
                    .filter(|i| matches!(i, LineItem::Space(_)))
                    .count() as f64;
        let offset = line.left + line_indent + align_offset(&text_align, line.width, available);
        let mut x = 0.0;
        let mut children: Vec<Fragment> = Vec::new();
        let mut decorations: Vec<(usize, NodeId, f64, f64)> = Vec::new();
        for entry in open.iter_mut() {
            entry.1 = 0.0;
        }
        let mut run_text: Option<(String, f64, f64, NodeId, f64, f64)> = None; // text, x, width, node, y, height
        let flush = |run: &mut Option<(String, f64, f64, NodeId, f64, f64)>,
                     children: &mut Vec<Fragment>| {
            if let Some((text, rx, rw, node, ry, rh)) = run.take() {
                let hidden = !ctx.style(node).is_visible();
                children.push(Fragment {
                    node: Some(node),
                    kind: FragmentKind::Text(text),
                    x: rx,
                    y: ry,
                    width: rw,
                    height: rh,
                    hidden,
                    ..Fragment::empty_block()
                });
            }
        };
        for (slot, item) in line.items.iter().enumerate() {
            match item {
                LineItem::Space(width) => {
                    flush(&mut run_text, &mut children);
                    x += width + extra_per_space;
                }
                LineItem::Piece(i) => match &pieces[*i] {
                    Piece::Word {
                        text,
                        width,
                        style_node,
                        ..
                    } => {
                        let (above, below, align) = metrics[slot].unwrap_or((
                            strut_above,
                            strut_below,
                            Align::Baseline(0.0),
                        ));
                        let top = match align {
                            Align::Baseline(shift) => baseline - shift - above,
                            Align::Top => 0.0,
                            Align::Bottom => line_height - (above + below),
                        };
                        match run_text.as_mut() {
                            Some((run, _, rw, node, _, _)) if *node == *style_node => {
                                run.push_str(text);
                                *rw += width;
                            }
                            _ => {
                                flush(&mut run_text, &mut children);
                                run_text = Some((
                                    text.clone(),
                                    x,
                                    *width,
                                    *style_node,
                                    top,
                                    above + below,
                                ));
                            }
                        }
                        x += width;
                    }
                    Piece::Space { width, .. } => {
                        flush(&mut run_text, &mut children);
                        x += width;
                    }
                    Piece::Gap(width) => {
                        flush(&mut run_text, &mut children);
                        x += width;
                    }
                    Piece::Edge {
                        node,
                        open: opens,
                        width,
                        outside,
                    } => {
                        flush(&mut run_text, &mut children);
                        if *opens {
                            open.push((*node, x + outside, opened));
                            opened += 1;
                        } else if let Some(at) = open.iter().rposition(|(n, _, _)| n == node) {
                            let (n, start, order) = open.remove(at);
                            decorations.push((order, n, start, x + width - outside));
                        }
                        x += width;
                    }
                    Piece::Atomic(a) => {
                        flush(&mut run_text, &mut children);
                        let mut fragment = a.fragment.clone();
                        fragment.x = x + a.margin_left;
                        fragment.y = match a.align {
                            Align::Baseline(shift) => baseline - shift - a.baseline,
                            Align::Top => a.margin_top,
                            Align::Bottom => line_height - a.margin_bottom - a.fragment.height,
                        };
                        for p in &a.pending {
                            result.pending_abs.push(PendingAbs {
                                node: p.node,
                                x: p.x + fragment.x + offset,
                                y: p.y + fragment.y + y,
                            });
                        }
                        x += atomic_width(a);
                        children.push(fragment);
                    }
                    Piece::Break => {}
                },
            }
        }
        flush(&mut run_text, &mut children);
        for (n, start, order) in &open {
            decorations.push((*order, *n, *start, x));
        }
        decorations.sort_by_key(|d| d.0);
        let boxes: Vec<Fragment> = decorations
            .into_iter()
            .map(|(_, node, x0, x1)| {
                let style = ctx.style(node);
                let padding = style.padding(content_width);
                let border = style.border(content_width);
                let (ascent, descent) = blueice_font::ascent_descent(
                    style.font_size(),
                    style.style.is_some_and(|s| s.is_bold()),
                    style.style.is_some_and(|s| s.is_italic()),
                );
                Fragment {
                    node: Some(node),
                    kind: FragmentKind::Block,
                    x: x0,
                    y: baseline - ascent - padding.top - border.top,
                    width: (x1 - x0).max(0.0),
                    height: ascent + descent + padding.vertical() + border.vertical(),
                    hidden: !style.is_visible(),
                    ..Fragment::empty_block()
                }
            })
            .collect();
        children.splice(0..0, boxes);

        result.extent = result.extent.max(offset + used_width.max(x));
        result.lines.push(Fragment {
            node: None,
            kind: FragmentKind::Line,
            x: offset,
            y,
            width: x,
            height: line_height,
            children,
            baseline: Some(baseline),
            ..Fragment::empty_block()
        });
        y += line_height;
    }
    result.height = y;
    result
}

#[cfg(test)]
mod tests {
    use crate::block::layout_block;
    use crate::fragment::{Fragment, FragmentKind};
    use blueice_css::{cascade, ua_stylesheet, Origin};
    use blueice_dom::{NodeData, NodeId};

    fn find(doc: &blueice_dom::Document, from: NodeId, tag: &str) -> Option<NodeId> {
        if matches!(doc.data(from), NodeData::Element { tag_name, .. } if tag_name == tag) {
            return Some(from);
        }
        doc.children(from).find_map(|c| find(doc, c, tag))
    }

    /// The body fragment of `html` laid out at `width` with `css` on top of the UA sheet.
    fn body(html: &str, css: &str, width: f64) -> Fragment {
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = blueice_css::parse(&format!("html,body,div,p,ul,ol,li,h1,h2,h3,h4,h5,h6,pre,blockquote,figure,dl,dd,menu,fieldset,hr,section,address{{margin:0;padding:0}} {css}")).rules;
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        let body = find(&doc, doc.root(), "body").unwrap();
        layout_block(&doc, body, &styles, width)
    }

    fn texts(line: &Fragment) -> Vec<(String, f64)> {
        line.children
            .iter()
            .filter_map(|c| match &c.kind {
                FragmentKind::Text(t) => Some((t.clone(), c.x)),
                _ => None,
            })
            .collect()
    }

    fn word_width(text: &str) -> f64 {
        blueice_font::measure_text_width(text, 16.0, false, false)
    }

    fn space() -> f64 {
        word_width(" ")
    }

    #[test]
    fn whitespace_collapses_and_edges_are_trimmed() {
        let f = body("<p>  a \n\t b   c  </p>", "", 800.0);
        let line = &f.children[0].children[0];
        let t = texts(line);
        assert_eq!(
            t.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert!(
            (t[1].1 - (word_width("a") + space())).abs() < 0.01,
            "exactly one space"
        );
        assert_eq!(t[0].1, 0.0, "no leading space");
    }

    #[test]
    fn adjacent_inline_elements_do_not_gain_a_space() {
        let f = body("<p><b>bo</b>ld <i>x</i></p>", "", 800.0);
        let line = &f.children[0].children[0];
        let t = texts(line);
        assert_eq!(t.len(), 3);
        let bold_bo = blueice_font::measure_text_width("bo", 16.0, true, false);
        assert!((t[1].1 - bold_bo).abs() < 0.01, "ld sits flush against bo");
        assert_eq!(
            f.children[0].children[0].children[0]
                .node
                .map(|n| n.as_u64() > 0),
            Some(true)
        );
    }

    #[test]
    fn a_space_collapses_across_an_element_boundary() {
        let f = body("<p>a <span> b</span></p>", "", 800.0);
        let t = texts(&f.children[0].children[0]);
        assert_eq!(t.len(), 2);
        assert!(
            (t[1].1 - (word_width("a") + space())).abs() < 0.01,
            "one space, not two"
        );
    }

    #[test]
    fn br_forces_a_new_line_and_a_double_br_leaves_a_blank_one() {
        let f = body("<p>a<br>b<br><br>c</p>", "", 800.0);
        let lines = &f.children[0].children;
        assert_eq!(lines.len(), 4, "a, b, blank, c");
        assert!(lines[2].children.is_empty());
        assert_eq!(lines[3].y, 3.0 * 19.2);
    }

    #[test]
    fn text_align_moves_lines_and_justify_fills_all_but_the_last_line() {
        let w = 200.0;
        let f = body("<p>hi</p>", "p { text-align: right; }", w);
        let line = &f.children[0].children[0];
        assert!((line.x - (w - word_width("hi"))).abs() < 0.01);
        let f = body("<p>hi</p>", "p { text-align: center; }", w);
        assert!((f.children[0].children[0].x - (w - word_width("hi")) / 2.0).abs() < 0.01);
        let f = body(
            "<p>aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll</p>",
            "p { text-align: justify; }",
            120.0,
        );
        let lines = &f.children[0].children;
        assert!(lines.len() > 1);
        let first_right = {
            let last = lines[0].children.last().unwrap();
            last.x + last.width
        };
        assert!(
            (first_right - 120.0).abs() < 0.5,
            "justified line reaches the right edge: {first_right}"
        );
        let last = lines.last().unwrap();
        let end = last.children.last().unwrap();
        assert!(
            end.x + end.width < 120.0 - 1.0,
            "the last line stays ragged"
        );
    }

    #[test]
    fn narrow_containers_wrap_and_long_words_overflow() {
        let f = body("<p>aaa bbb ccc</p>", "", word_width("aaa bbb") + 1.0);
        assert_eq!(
            f.children[0].children.len(),
            2,
            "two words fit, the third wraps"
        );
        let f = body("<p>aaaaaaaaaaaaaaaaaaaaaaaa bb</p>", "", 20.0);
        let lines = &f.children[0].children;
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].width > 20.0,
            "an unbreakable word overflows its line"
        );
    }

    #[test]
    fn white_space_nowrap_pre_pre_wrap_and_pre_line() {
        let long = "aaa bbb ccc ddd";
        let f = body(
            &format!("<p>{long}</p>"),
            "p { white-space: nowrap; }",
            40.0,
        );
        assert_eq!(f.children[0].children.len(), 1, "nowrap never wraps");
        let f = body("<p>a  b\nc</p>", "p { white-space: pre; }", 800.0);
        let lines = &f.children[0].children;
        assert_eq!(lines.len(), 2, "the newline breaks the line");
        let t = texts(&lines[0]);
        assert!(
            (t[1].1 - (word_width("a") + 2.0 * space())).abs() < 0.01,
            "two spaces kept"
        );
        let f = body(
            &format!("<p>{long}</p>"),
            "p { white-space: pre-wrap; }",
            60.0,
        );
        assert!(f.children[0].children.len() > 1, "pre-wrap still wraps");
        let f = body("<p>a  b\nc</p>", "p { white-space: pre-line; }", 800.0);
        let lines = &f.children[0].children;
        assert_eq!(lines.len(), 2);
        let t = texts(&lines[0]);
        assert!(
            (t[1].1 - (word_width("a") + space())).abs() < 0.01,
            "spaces collapse, newlines do not"
        );
    }

    #[test]
    fn text_nodes_are_tagged_with_their_own_element_and_font() {
        let f = body(
            "<p>a <b>big</b> <span style='font-size:32px'>B</span></p>",
            "",
            800.0,
        );
        let line = &f.children[0].children[0];
        assert_eq!(line.children.len(), 3);
        assert!(
            line.height > 19.2,
            "the 32px span makes the line taller: {}",
            line.height
        );
        let nodes: Vec<_> = line.children.iter().map(|c| c.node).collect();
        assert!(nodes[0] != nodes[1] && nodes[1] != nodes[2]);
        let b = &line.children[2];
        assert!(b.width > 16.0 * 0.5, "measured at 32px");
    }

    #[test]
    fn display_none_inline_content_never_appears() {
        let f = body(
            "<p>a<span style='display:none'>HIDDEN</span>b<style>.q{}</style></p>",
            "",
            800.0,
        );
        let line = &f.children[0].children[0];
        let all: String = texts(line).into_iter().map(|(t, _)| t).collect();
        assert_eq!(all, "ab".to_string().replace("ab", "a") + "b", "{all}");
        assert!(!all.contains("HIDDEN") && !all.contains(".q"));
    }

    #[test]
    fn text_transform_changes_what_is_laid_out() {
        let f = body(
            "<p>hello world</p>",
            "p { text-transform: uppercase; }",
            800.0,
        );
        let t = texts(&f.children[0].children[0]);
        assert_eq!(t[0].0, "HELLO");
        let f = body(
            "<p>hello world</p>",
            "p { text-transform: capitalize; }",
            800.0,
        );
        assert_eq!(texts(&f.children[0].children[0])[1].0, "World");
    }

    #[test]
    fn letter_and_word_spacing_widen_text() {
        let plain = body("<p>ab cd</p>", "", 800.0);
        let spaced = body(
            "<p>ab cd</p>",
            "p { letter-spacing: 2px; word-spacing: 5px; }",
            800.0,
        );
        let pw = &plain.children[0].children[0].children;
        let sw = &spaced.children[0].children[0].children;
        assert!((sw[0].width - (pw[0].width + 4.0)).abs() < 0.01);
        assert!(
            (sw[1].x - (pw[1].x + 4.0 + 2.0 * 0.0 + 5.0 + 0.0 + 2.0 * 1.0)).abs() < 0.5,
            "{} vs {}",
            sw[1].x,
            pw[1].x
        );
    }

    #[test]
    fn cjk_text_breaks_between_characters_but_not_before_closing_punctuation() {
        let f = body("<p>一二三四五六七八九十</p>", "", 16.0 * 3.5);
        let lines = &f.children[0].children;
        assert!(lines.len() >= 3, "no spaces, yet it wraps: {}", lines.len());
        let f = body("<p>一二三，四五</p>", "", 16.0 * 3.2);
        let lines = &f.children[0].children;
        let second_first = texts(&lines[1])[0].0.clone();
        assert!(
            !second_first.starts_with('，'),
            "a comma never starts a line: {second_first}"
        );
        let f = body("<p>一二三</p>", "", 800.0);
        assert_eq!(
            f.children[0].children[0].children.len(),
            1,
            "one fragment for unbroken CJK text"
        );
    }

    #[test]
    fn text_indent_shifts_only_the_first_line() {
        let f = body(
            "<p>aaa bbb ccc ddd eee</p>",
            "p { text-indent: 30px; }",
            80.0,
        );
        let lines = &f.children[0].children;
        assert!(lines.len() > 1);
        assert_eq!(texts(&lines[0])[0].1, 0.0);
        assert_eq!(lines[0].x, 30.0);
        assert_eq!(lines[1].x, 0.0);
    }

    #[test]
    fn inline_boxes_take_room_for_their_horizontal_padding_and_margin() {
        let plain = body("<p><span>a</span>b</p>", "", 800.0);
        let padded = body(
            "<p><span style='padding:0 10px;margin-right:5px'>a</span>b</p>",
            "",
            800.0,
        );
        let p = texts(&plain.children[0].children[0]);
        let q = texts(&padded.children[0].children[0]);
        assert!(
            (q[0].1 - 10.0).abs() < 0.01,
            "left padding precedes the text"
        );
        assert!(
            (q[1].1 - (p[1].1 + 25.0)).abs() < 0.01,
            "padding both sides plus the margin"
        );
    }

    #[test]
    fn inline_block_is_an_atomic_box_that_shrinks_to_fit_and_sits_on_the_line() {
        let f = body(
            "<p>x<span style='display:inline-block;padding:3px'>abc</span>y</p>",
            "",
            800.0,
        );
        let line = &f.children[0].children[0];
        let atomic = line
            .children
            .iter()
            .find(|c| matches!(c.kind, FragmentKind::Block))
            .unwrap();
        assert!(
            (atomic.width - (word_width("abc") + 6.0)).abs() < 0.01,
            "shrink-to-fit: {}",
            atomic.width
        );
        let x = line
            .children
            .iter()
            .find(|c| matches!(&c.kind, FragmentKind::Text(t) if t == "x"))
            .unwrap();
        let y = line
            .children
            .iter()
            .find(|c| matches!(&c.kind, FragmentKind::Text(t) if t == "y"))
            .unwrap();
        assert!((atomic.x - x.width).abs() < 0.01, "flush after x");
        assert!(
            (y.x - (atomic.x + atomic.width)).abs() < 0.01,
            "y follows the box"
        );
        // The box's own text baseline equals the surrounding text baseline.
        let inner_line = &atomic.children[0];
        let baseline_in_box = atomic.y + inner_line.y + inner_line.baseline.unwrap();
        assert!((baseline_in_box - line.baseline.unwrap()).abs() < 0.01);
    }

    #[test]
    fn inline_blocks_wrap_when_they_do_not_fit() {
        let f = body(
            "<p><span style='display:inline-block;width:70px'>a</span><span style='display:inline-block;width:70px'>b</span></p>",
            "",
            100.0,
        );
        assert_eq!(f.children[0].children.len(), 2, "one box per line");
        let wide = body(
            "<p><span style='display:inline-block;width:70px'>a</span><span style='display:inline-block;width:70px'>b</span></p>",
            "",
            200.0,
        );
        assert_eq!(wide.children[0].children.len(), 1);
    }

    #[test]
    fn vertical_align_top_and_baseline_shift_atomic_boxes() {
        let css = "i { display: inline-block; width: 10px; height: 50px; }";
        let base = body("<p>a<i></i></p>", css, 800.0);
        let line = &base.children[0].children[0];
        assert!(
            line.height >= 50.0,
            "the box sets the line height: {}",
            line.height
        );
        let atomic = line
            .children
            .iter()
            .find(|c| matches!(c.kind, FragmentKind::Block))
            .unwrap();
        assert!(
            (atomic.y + atomic.height - line.baseline.unwrap()).abs() < 0.01,
            "bottom edge on the baseline"
        );
        let top = body("<p>a<i style='vertical-align:top'></i></p>", css, 800.0);
        let atomic = top.children[0].children[0]
            .children
            .iter()
            .find(|c| matches!(c.kind, FragmentKind::Block))
            .unwrap();
        assert_eq!(atomic.y, 0.0, "top aligned");
        let raised = body(
            "<p>a<span style='vertical-align:super'>b</span></p>",
            "",
            800.0,
        );
        let line = &raised.children[0].children[0];
        assert!(
            line.height > 19.2,
            "a raised span grows the line: {}",
            line.height
        );
    }

    #[test]
    fn replaced_elements_take_their_attribute_or_css_size() {
        let f = body("<p>a<img width=30 height=20>b</p>", "", 800.0);
        let line = &f.children[0].children[0];
        let img = line
            .children
            .iter()
            .find(|c| matches!(c.kind, FragmentKind::Block))
            .unwrap();
        assert_eq!((img.width, img.height), (30.0, 20.0));
        let f = body(
            "<p><img width=30 height=20 style='width:60px'></p>",
            "",
            800.0,
        );
        let img = &f.children[0].children[0].children[0];
        assert_eq!(
            (img.width, img.height),
            (60.0, 20.0),
            "CSS wins over the attribute"
        );
        let f = body("<p><svg width='24' height='24'></svg></p>", "", 800.0);
        assert_eq!(f.children[0].children[0].children[0].width, 24.0);
        let f = body(
            "<p><img width=40 height=20 style='width:80px;height:auto'></p>",
            "",
            800.0,
        );
        assert_eq!(
            f.children[0].children[0].children[0].height, 40.0,
            "auto keeps the aspect ratio"
        );
    }

    #[test]
    fn an_inline_element_holding_a_block_splits_into_block_and_text() {
        let f = body(
            "<section>a<span><div style='height:10px'>x</div>y</span></section>",
            "",
            800.0,
        );
        let kinds: Vec<_> = f.children[0]
            .children
            .iter()
            .map(|c| matches!(c.kind, FragmentKind::Line))
            .collect();
        assert_eq!(kinds, vec![true, false, true], "line, block, line");
    }

    #[test]
    fn the_container_strut_gives_empty_looking_lines_their_height() {
        let f = body("<div style='font-size:40px'>a</div>", "", 800.0);
        assert_eq!(f.children[0].height, 48.0);
        let f = body("<div style='line-height:2'>a</div>", "", 800.0);
        assert_eq!(f.children[0].height, 32.0);
        let f = body("<div style='line-height:30px'>a<br>b</div>", "", 800.0);
        assert_eq!(f.children[0].height, 60.0);
    }

    #[test]
    fn hidden_text_is_marked_but_keeps_its_place() {
        let f = body(
            "<p>a<span style='visibility:hidden'>b</span>c</p>",
            "",
            800.0,
        );
        let line = &f.children[0].children[0];
        assert_eq!(line.children.len(), 3);
        assert!(!line.children[0].hidden && line.children[1].hidden && !line.children[2].hidden);
    }

    #[test]
    fn a_painted_inline_box_becomes_a_fragment_behind_its_text() {
        let f = body(
            "<p>a <span style='background:red;padding:2px 5px'>bc</span> d</p>",
            "",
            800.0,
        );
        let line = &f.children[0].children[0];
        let first = &line.children[0];
        assert!(
            matches!(first.kind, FragmentKind::Block) && first.node.is_some(),
            "the box comes before the text it sits behind"
        );
        let bc = blueice_font::measure_text_width("bc", 16.0, false, false);
        assert!(
            (first.width - (bc + 10.0)).abs() < 0.01,
            "content plus padding"
        );
        let a = blueice_font::measure_text_width("a ", 16.0, false, false);
        assert!((first.x - a).abs() < 0.01, "starts where the span does");
    }

    #[test]
    fn an_unpainted_inline_box_adds_no_fragment() {
        let f = body("<p>a <span style='padding:0 5px'>bc</span></p>", "", 800.0);
        let line = &f.children[0].children[0];
        assert!(line
            .children
            .iter()
            .all(|c| matches!(c.kind, FragmentKind::Text(_))));
    }

    #[test]
    fn a_painted_inline_box_split_across_lines_has_a_box_per_line() {
        let f = body(
            "<p><span style='background:red'>one two three four five six seven</span></p>",
            "p { width: 80px; }",
            800.0,
        );
        let boxes = f.children[0]
            .children
            .iter()
            .filter(|l| {
                matches!(
                    l.children.first().map(|c| &c.kind),
                    Some(FragmentKind::Block)
                )
            })
            .count();
        assert!(
            boxes >= 3,
            "each line carries its piece of the box, got {boxes}"
        );
    }

    fn generated_text_of(line: &Fragment) -> Vec<String> {
        line.children
            .iter()
            .filter_map(|c| match &c.kind {
                FragmentKind::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn before_and_after_content_becomes_text_in_the_line() {
        let f = body(
            "<p>mid</p>",
            "p::before { content: \"[\"; } p::after { content: \"]\"; }",
            800.0,
        );
        let line = &f.children[0].children[0];
        let joined: String = generated_text_of(line).concat();
        assert_eq!(joined, "[mid]");
        let first = &line.children[0];
        assert_eq!(first.node, None, "no DOM node stands behind generated text");
        assert!(first.style_node.is_some());
    }

    #[test]
    fn attr_content_reads_the_owners_attribute() {
        let f = body(
            "<a href=\"http://x\">link</a>",
            "a::after { content: \" (\" attr(href) \")\"; }",
            800.0,
        );
        assert_eq!(
            generated_text_of(&f.children[0]),
            vec!["link".to_string(), "(http://x)".to_string()]
        );
    }

    #[test]
    fn an_empty_block_pseudo_element_is_a_box_with_its_own_size() {
        let f = body(
            "<div>text</div>",
            "div::after { content: \"\"; display: block; width: 30px; height: 7px; background: red; }",
            800.0,
        );
        let div = &f.children[0];
        let boxed = div.children.last().unwrap();
        assert_eq!((boxed.width, boxed.height), (30.0, 7.0));
        assert!(boxed.node.is_none() && boxed.style_node.is_some());
        assert_eq!(boxed.y, div.children[0].height, "after the text line");
    }

    #[test]
    fn content_none_and_missing_content_generate_nothing() {
        let f = body(
            "<p>x</p>",
            "p::before { content: none; } p::after { color: red; }",
            800.0,
        );
        assert_eq!(
            generated_text_of(&f.children[0].children[0]),
            vec!["x".to_string()]
        );
    }

    #[test]
    fn a_clearfix_pseudo_element_contains_floats_in_its_parent() {
        let f = body(
            "<div class=c><div style='float:left;width:10px;height:40px'></div></div>",
            ".c::after { content: \"\"; display: table; clear: both; }",
            800.0,
        );
        assert_eq!(f.children[0].height, 40.0);
    }

    fn img_size(html: &str, css: &str, width: f64) -> (f64, f64) {
        let f = body(html, &format!("img {{ -blueice-intrinsic-size: 200 100; }} {css}"), width);
        let atom = f.children[0].children[0]
            .children
            .iter()
            .find(|c| c.node.is_some())
            .unwrap();
        (atom.width, atom.height)
    }

    #[test]
    fn an_image_takes_its_own_size_when_nothing_else_says() {
        assert_eq!(img_size("<p><img></p>", "", 800.0), (200.0, 100.0));
    }

    #[test]
    fn one_given_dimension_scales_the_other_by_the_images_ratio() {
        assert_eq!(img_size("<p><img width=100></p>", "", 800.0), (100.0, 50.0));
        assert_eq!(img_size("<p><img height=20></p>", "", 800.0), (40.0, 20.0));
        assert_eq!(
            img_size("<p><img></p>", "img { width: 50px; height: auto; }", 800.0),
            (50.0, 25.0)
        );
    }

    #[test]
    fn both_given_dimensions_win() {
        assert_eq!(img_size("<p><img width=30 height=30></p>", "", 800.0), (30.0, 30.0));
    }

    #[test]
    fn max_width_shrinks_an_image_in_proportion() {
        assert_eq!(
            img_size("<p><img></p>", "img { max-width: 100px; height: auto; }", 800.0),
            (100.0, 50.0)
        );
        assert_eq!(
            img_size("<p><img></p>", "img { max-width: 100%; }", 150.0),
            (150.0, 75.0)
        );
    }
}
