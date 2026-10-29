// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Flexbox (CSS Flexible Box Layout 1): `display: flex` / `inline-flex`
//! containers lay their in-flow children out as flex items along a main axis
//! (`flex-direction`), in one line or several (`flex-wrap`), sharing out the
//! free space by `flex-grow`/`flex-shrink` against each item's `flex-basis`
//! with min/max clamping (the spec's iterative freezing), then aligning items
//! across the line (`align-items`/`align-self`, baseline included) and
//! distributing leftover main space (`justify-content`, auto margins), with
//! `gap`, `order`, and `align-content` for wrapped lines.
//!
//! Sizes here are border-box, the way items are laid out (`Forced`).

use crate::block::{layout_block_level, Constraint, Forced, PendingAbs};
use crate::ctx::{Ctx, Display, Style};
use crate::fragment::{Fragment, FragmentKind};
use crate::inline::first_baseline;
use blueice_css::Value;
use blueice_dom::{NodeData, NodeId};

/// Room enough that nothing wraps when measuring content.
const UNBOUNDED: f64 = 1.0e6;

pub(crate) struct FlexInput {
    pub content_width: f64,
    /// The container's content-box height when it is definite.
    pub definite_height: Option<f64>,
    /// `min-height`/`max-height` as content-box heights.
    pub min_height: f64,
    pub max_height: Option<f64>,
}

pub(crate) struct FlexLayout {
    pub children: Vec<Fragment>,
    /// The container's content height when its own height is auto.
    pub height: f64,
    /// Farthest right edge of the items.
    pub extent: f64,
    pub pending_abs: Vec<PendingAbs>,
}

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Start,
    End,
    Center,
    Baseline,
    Stretch,
}

fn parse_align(word: &str) -> Align {
    match word {
        "flex-start" | "start" | "self-start" | "left" => Align::Start,
        "flex-end" | "end" | "self-end" | "right" => Align::End,
        "center" => Align::Center,
        "baseline" | "first baseline" | "last baseline" => Align::Baseline,
        _ => Align::Stretch,
    }
}

pub(crate) enum Source {
    Element(NodeId),
    /// Consecutive text children, wrapped in an anonymous item.
    Text(Vec<NodeId>),
}

struct Item {
    source: Source,
    order: i32,
    grow: f64,
    shrink: f64,
    /// Outer margins `[top, right, bottom, left]`; `None` is `auto`.
    margin: [Option<f64>; 4],
    pb_h: f64,
    pb_v: f64,
    align: Align,
    // main-axis border-box sizes
    base: f64,
    hypothetical: f64,
    min: f64,
    max: f64,
    used: f64,
    /// Cross-axis border-box size.
    cross: f64,
    /// Whether the cross size comes from the item's own style (not stretchable).
    cross_definite: bool,
    fragment: Fragment,
    pending: Vec<PendingAbs>,
    baseline: Option<f64>,
    // placement
    main_pos: f64,
    cross_pos: f64,
}

impl Item {
    fn margin_main(&self, row: bool, start: bool) -> Option<f64> {
        let index = match (row, start) {
            (true, true) => 3,
            (true, false) => 1,
            (false, true) => 0,
            (false, false) => 2,
        };
        self.margin[index]
    }

    fn margin_cross(&self, row: bool, start: bool) -> Option<f64> {
        self.margin_main(!row, start)
    }

    fn outer_main(&self, row: bool, size: f64) -> f64 {
        size + self.margin_main(row, true).unwrap_or(0.0)
            + self.margin_main(row, false).unwrap_or(0.0)
    }

    fn outer_cross(&self, row: bool) -> f64 {
        self.cross
            + self.margin_cross(row, true).unwrap_or(0.0)
            + self.margin_cross(row, false).unwrap_or(0.0)
    }
}

struct Ctxs<'a, 'b> {
    ctx: &'a Ctx<'b>,
    container: NodeId,
    content_width: f64,
    definite_height: Option<f64>,
    row: bool,
}

impl Ctxs<'_, '_> {
    fn constraint(&self) -> Constraint {
        Constraint {
            cb_width: self.content_width,
            cb_height: self.definite_height,
            root: false,
        }
    }

    /// Lays `item` out with the given border-box main (and optional cross)
    /// size, returning its fragment.
    fn lay_out(&self, item: &mut Item, main: f64, cross: Option<f64>) {
        let (width, height) = if self.row {
            (Some(main), cross)
        } else {
            (cross, Some(main))
        };
        match &item.source {
            Source::Element(node) => {
                let placed = layout_block_level(
                    self.ctx,
                    *node,
                    &self.constraint(),
                    Forced { width, height },
                );
                item.baseline = first_baseline(&placed.fragment);
                item.fragment = placed.fragment;
                item.pending = placed.pending_abs;
            }
            Source::Text(nodes) => {
                let w = width.unwrap_or(self.content_width);
                let run = crate::inline::layout_run(self.ctx, self.container, nodes, w, false);
                let h = height.unwrap_or(run.height);
                item.baseline = run.lines.first().and_then(|l| l.baseline.map(|b| l.y + b));
                item.pending = run.pending_abs;
                item.fragment = Fragment {
                    node: None,
                    kind: FragmentKind::Block,
                    width: w,
                    height: h,
                    children: run.lines,
                    ..Fragment::empty_block()
                };
            }
        }
    }

    /// `(min-content, max-content)` border-box widths of the item.
    fn content_widths(&self, item: &Item) -> (f64, f64) {
        match &item.source {
            Source::Element(node) => crate::intrinsic::content_widths(self.ctx, *node),
            Source::Text(nodes) => {
                let widest = |w: f64| {
                    crate::inline::layout_run(self.ctx, self.container, nodes, w, false).extent
                };
                (widest(0.0), widest(UNBOUNDED))
            }
        }
    }

    /// The item's natural height at the given border-box width.
    fn natural_height(&self, item: &Item, width: f64) -> f64 {
        match &item.source {
            Source::Element(node) => {
                layout_block_level(
                    self.ctx,
                    *node,
                    &self.constraint(),
                    Forced {
                        width: Some(width),
                        height: None,
                    },
                )
                .fragment
                .height
            }
            Source::Text(nodes) => {
                crate::inline::layout_run(self.ctx, self.container, nodes, width, false).height
            }
        }
    }
}

pub(crate) fn collect_items(
    ctx: &Ctx<'_>,
    node: NodeId,
    items: &mut Vec<Source>,
    abs: &mut Vec<NodeId>,
    text: &mut Vec<NodeId>,
) {
    let flush = |items: &mut Vec<Source>, text: &mut Vec<NodeId>| {
        if !text.is_empty() {
            items.push(Source::Text(std::mem::take(text)));
        }
    };
    for child in ctx.doc.children(node) {
        match ctx.doc.data(child) {
            NodeData::Text { data } => {
                if data.chars().any(|c| !c.is_whitespace()) || !text.is_empty() {
                    text.push(child);
                }
            }
            NodeData::Document => {}
            NodeData::Element { .. } => {
                let style = ctx.style(child);
                let display = style.display();
                if display == Display::None || display == Display::TableColumn {
                    continue;
                }
                if style.is_out_of_flow() {
                    abs.push(child);
                } else if display == Display::Contents {
                    collect_items(ctx, child, items, abs, text);
                } else {
                    flush(items, text);
                    items.push(Source::Element(child));
                }
            }
        }
    }
    flush(items, text);
}

fn number(style: &Style<'_>, property: &str, default: f64) -> f64 {
    match style.raw(property) {
        Some(Value::Number(n)) => *n,
        _ => default,
    }
}

/// A resolved `flex-basis`-like size in border-box pixels.
fn border_box(style: &Style<'_>, value: f64, pb: f64) -> f64 {
    if style.is_border_box() {
        value.max(pb)
    } else {
        value + pb
    }
}

pub(crate) fn layout_flex(ctx: &Ctx<'_>, container: NodeId, input: &FlexInput) -> FlexLayout {
    let cstyle = ctx.style(container);
    let direction = cstyle.keyword("flex-direction").unwrap_or("row");
    let row = matches!(direction, "row" | "row-reverse");
    let reverse = matches!(direction, "row-reverse" | "column-reverse");
    let wrap_mode = cstyle.keyword("flex-wrap").unwrap_or("nowrap");
    let wrap = wrap_mode != "nowrap";
    let wrap_reverse = wrap_mode == "wrap-reverse";
    let content_width = input.content_width;
    let container_main = if row {
        Some(content_width)
    } else {
        input.definite_height
    };
    let container_cross = if row {
        input.definite_height
    } else {
        Some(content_width)
    };
    let gap_of = |property: &str, base: Option<f64>| {
        cstyle
            .px(property, base.unwrap_or(f64::NAN))
            .unwrap_or(0.0)
            .max(0.0)
    };
    let gap_main = gap_of(if row { "column-gap" } else { "row-gap" }, container_main);
    let gap_cross = gap_of(if row { "row-gap" } else { "column-gap" }, container_cross);
    let justify = cstyle
        .keyword("justify-content")
        .unwrap_or("normal")
        .to_string();
    let align_items = cstyle
        .keyword("align-items")
        .map(parse_align)
        .unwrap_or(Align::Stretch);
    let align_content = cstyle
        .keyword("align-content")
        .unwrap_or("normal")
        .to_string();

    let helper = Ctxs {
        ctx,
        container,
        content_width,
        definite_height: input.definite_height,
        row,
    };

    let mut sources = Vec::new();
    let mut abs_nodes = Vec::new();
    collect_items(
        ctx,
        container,
        &mut sources,
        &mut abs_nodes,
        &mut Vec::new(),
    );
    let mut result = FlexLayout {
        children: Vec::new(),
        height: 0.0,
        extent: 0.0,
        pending_abs: abs_nodes
            .into_iter()
            .map(|node| PendingAbs {
                node,
                x: 0.0,
                y: 0.0,
            })
            .collect(),
    };
    if sources.is_empty() {
        result.height = input.definite_height.unwrap_or(0.0);
        return result;
    }

    // ---- build items with flex base sizes ----
    let mut items: Vec<Item> = Vec::new();
    for source in sources {
        let (style_node, anonymous) = match &source {
            Source::Element(node) => (Some(*node), false),
            Source::Text(_) => (None, true),
        };
        let style = style_node.map(|n| ctx.style(n));
        let mut item = Item {
            source,
            order: 0,
            grow: 0.0,
            shrink: 1.0,
            margin: [Some(0.0); 4],
            pb_h: 0.0,
            pb_v: 0.0,
            align: align_items,
            base: 0.0,
            hypothetical: 0.0,
            min: 0.0,
            max: f64::INFINITY,
            used: 0.0,
            cross: 0.0,
            cross_definite: false,
            fragment: Fragment::empty_block(),
            pending: Vec::new(),
            baseline: None,
            main_pos: 0.0,
            cross_pos: 0.0,
        };
        if let Some(style) = &style {
            item.order = number(style, "order", 0.0) as i32;
            item.grow = number(style, "flex-grow", 0.0).max(0.0);
            item.shrink = number(style, "flex-shrink", 1.0).max(0.0);
            let m = |name: &str| {
                if style.is_auto(name) {
                    None
                } else {
                    Some(style.side(name, content_width))
                }
            };
            item.margin = [
                m("margin-top"),
                m("margin-right"),
                m("margin-bottom"),
                m("margin-left"),
            ];
            let padding = style.padding(content_width);
            let border = style.border(content_width);
            item.pb_h = padding.horizontal() + border.horizontal();
            item.pb_v = padding.vertical() + border.vertical();
            if let Some(word) = style.keyword("align-self") {
                if word != "auto" {
                    item.align = parse_align(word);
                }
            }
        }
        let _ = anonymous;
        items.push(item);
    }
    items.sort_by_key(|i| i.order);
    let count = items.len();

    // Cross size for column items must be known before their natural height.
    for item in items.iter_mut() {
        let (pb_main, pb_cross) = if row {
            (item.pb_h, item.pb_v)
        } else {
            (item.pb_v, item.pb_h)
        };
        let style = match &item.source {
            Source::Element(node) => Some(ctx.style(*node)),
            Source::Text(_) => None,
        };
        let main_prop = if row { "width" } else { "height" };
        let cross_prop = if row { "height" } else { "width" };
        let specified_main = style.as_ref().and_then(|s| {
            s.px(main_prop, container_main.unwrap_or(f64::NAN))
                .map(|v| border_box(s, v, pb_main))
        });
        let specified_cross = style.as_ref().and_then(|s| {
            s.px(cross_prop, container_cross.unwrap_or(f64::NAN))
                .map(|v| border_box(s, v, pb_cross))
        });
        item.cross_definite = specified_cross.is_some();

        // Column items need a width to measure their height against.
        let column_width = if row {
            0.0
        } else {
            specified_cross.unwrap_or_else(|| {
                let margins = item.margin[1].unwrap_or(0.0) + item.margin[3].unwrap_or(0.0);
                if item.align == Align::Stretch {
                    (content_width - margins).max(0.0)
                } else {
                    let (min_c, max_c) = helper.content_widths(item);
                    max_c.min((content_width - margins).max(min_c))
                }
            })
        };

        // flex-basis
        let basis_value = style.as_ref().and_then(|s| s.raw("flex-basis"));
        let content_size = |item: &Item| -> f64 {
            if row {
                helper.content_widths(item).1
            } else {
                helper.natural_height(item, column_width)
            }
        };
        let base = match basis_value {
            Some(Value::Keyword(k)) if k == "content" => content_size(item),
            Some(Value::Keyword(k)) if k == "auto" => match specified_main {
                Some(m) => m,
                None => content_size(item),
            },
            Some(value) => match (style.as_ref(), container_main) {
                (Some(s), Some(main)) => value
                    .to_px(&s.length_context(main))
                    .map(|v| border_box(s, v, pb_main))
                    .unwrap_or_else(|| content_size(item)),
                (Some(s), None) => value
                    .to_px(&s.length_context(f64::NAN))
                    .map(|v| border_box(s, v, pb_main))
                    .unwrap_or_else(|| content_size(item)),
                _ => content_size(item),
            },
            None => match specified_main {
                Some(m) => m,
                None => content_size(item),
            },
        };

        // min / max (border-box)
        let min_prop = if row { "min-width" } else { "min-height" };
        let max_prop = if row { "max-width" } else { "max-height" };
        let max = style
            .as_ref()
            .and_then(|s| {
                s.px(max_prop, container_main.unwrap_or(f64::NAN))
                    .map(|v| border_box(s, v, pb_main))
            })
            .unwrap_or(f64::INFINITY);
        let explicit_min = style.as_ref().and_then(|s| {
            s.px(min_prop, container_main.unwrap_or(f64::NAN))
                .map(|v| border_box(s, v, pb_main))
        });
        let scroll_container = style.as_ref().is_some_and(|s| s.clips_overflow());
        let min = match explicit_min {
            Some(m) => m,
            None if scroll_container => 0.0,
            None => {
                // The automatic minimum: the content size, or the specified
                // size when that is smaller.
                let content_min = if row {
                    helper.content_widths(item).0
                } else {
                    helper.natural_height(item, column_width)
                };
                match specified_main {
                    Some(m) => content_min.min(m),
                    None => content_min,
                }
                .min(max)
            }
        };
        item.base = base;
        item.min = min.min(max.max(0.0)).max(0.0);
        item.max = max;
        item.hypothetical = base.min(item.max).max(item.min);
        item.used = item.hypothetical;
        if !row {
            item.cross = column_width;
        }
    }

    // ---- lines ----
    let available_main = container_main;
    let mut lines: Vec<Vec<usize>> = Vec::new();
    if !wrap || available_main.is_none() {
        lines.push((0..count).collect());
    } else {
        let limit = available_main.unwrap_or(UNBOUNDED);
        let mut current: Vec<usize> = Vec::new();
        let mut used = 0.0;
        for (i, item) in items.iter().enumerate() {
            let outer = item.outer_main(row, item.hypothetical);
            let gap = if current.is_empty() { 0.0 } else { gap_main };
            if !current.is_empty() && used + gap + outer > limit + 0.01 {
                lines.push(std::mem::take(&mut current));
                used = 0.0;
            }
            used += if current.is_empty() { 0.0 } else { gap_main } + outer;
            current.push(i);
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }

    // ---- resolve flexible lengths, per line ----
    for line in &lines {
        let gaps = gap_main * (line.len().saturating_sub(1)) as f64;
        let outer_hypothetical: f64 = line
            .iter()
            .map(|&i| items[i].outer_main(row, items[i].hypothetical))
            .sum::<f64>()
            + gaps;
        let target = available_main.unwrap_or(outer_hypothetical);
        let growing = outer_hypothetical < target;
        let mut frozen = vec![false; line.len()];
        for (slot, &i) in line.iter().enumerate() {
            let item = &items[i];
            let factor = if growing { item.grow } else { item.shrink };
            let inflexible = factor == 0.0
                || (growing && item.base > item.hypothetical)
                || (!growing && item.base < item.hypothetical);
            if inflexible {
                frozen[slot] = true;
                items[i].used = item.hypothetical;
            } else {
                items[i].used = item.base;
            }
        }
        let initial_free: f64 = target
            - line
                .iter()
                .enumerate()
                .map(|(slot, &i)| {
                    let size = if frozen[slot] {
                        items[i].used
                    } else {
                        items[i].base
                    };
                    items[i].outer_main(row, size)
                })
                .sum::<f64>()
            - gaps;
        for _ in 0..(line.len() + 1) {
            if frozen.iter().all(|f| *f) {
                break;
            }
            let remaining: f64 = target
                - line
                    .iter()
                    .enumerate()
                    .map(|(slot, &i)| {
                        let size = if frozen[slot] {
                            items[i].used
                        } else {
                            items[i].base
                        };
                        items[i].outer_main(row, size)
                    })
                    .sum::<f64>()
                - gaps;
            let unfrozen_factor: f64 = line
                .iter()
                .enumerate()
                .filter(|(slot, _)| !frozen[*slot])
                .map(|(_, &i)| {
                    if growing {
                        items[i].grow
                    } else {
                        items[i].shrink
                    }
                })
                .sum();
            let mut free = remaining;
            if unfrozen_factor < 1.0 && growing {
                let scaled = initial_free * unfrozen_factor;
                if scaled.abs() < free.abs() {
                    free = scaled;
                }
            }
            if free != 0.0 {
                if growing {
                    for (slot, &i) in line.iter().enumerate() {
                        if !frozen[slot] && unfrozen_factor > 0.0 {
                            items[i].used = items[i].base + free * items[i].grow / unfrozen_factor;
                        }
                    }
                } else {
                    let scaled_sum: f64 = line
                        .iter()
                        .enumerate()
                        .filter(|(slot, _)| !frozen[*slot])
                        .map(|(_, &i)| items[i].shrink * items[i].base)
                        .sum();
                    for (slot, &i) in line.iter().enumerate() {
                        if !frozen[slot] && scaled_sum > 0.0 {
                            let share = items[i].shrink * items[i].base / scaled_sum;
                            items[i].used = items[i].base + free * share;
                        }
                    }
                }
            }
            // Clamp and freeze by the sign of the total violation.
            let mut violation = 0.0;
            let mut clamped_to = vec![0.0; line.len()];
            for (slot, &i) in line.iter().enumerate() {
                if frozen[slot] {
                    continue;
                }
                let unclamped = items[i].used;
                let clamped = unclamped.min(items[i].max).max(items[i].min);
                violation += clamped - unclamped;
                clamped_to[slot] = clamped;
            }
            for (slot, &i) in line.iter().enumerate() {
                if frozen[slot] {
                    continue;
                }
                let unclamped = items[i].used;
                let clamped = clamped_to[slot];
                let freeze = if violation == 0.0 {
                    true
                } else if violation > 0.0 {
                    clamped > unclamped
                } else {
                    clamped < unclamped
                };
                if freeze {
                    items[i].used = clamped;
                    frozen[slot] = true;
                }
            }
        }
        for &i in line {
            items[i].used = items[i].used.max(0.0);
        }
    }

    // ---- cross sizes ----
    let mut line_cross: Vec<f64> = Vec::new();
    for line in &lines {
        let mut tallest = 0.0f64;
        for &i in line {
            let used = items[i].used;
            let cross_fixed = if items[i].cross_definite {
                let node = match &items[i].source {
                    Source::Element(n) => Some(*n),
                    Source::Text(_) => None,
                };
                node.and_then(|n| {
                    let s = ctx.style(n);
                    let pb = if row { items[i].pb_v } else { items[i].pb_h };
                    let prop = if row { "height" } else { "width" };
                    s.px(prop, container_cross.unwrap_or(f64::NAN))
                        .map(|v| border_box(&s, v, pb))
                })
            } else {
                None
            };
            helper.lay_out(&mut items[i], used, cross_fixed);
            items[i].cross = if row {
                items[i].fragment.height
            } else {
                items[i].fragment.width
            };
            tallest = tallest.max(items[i].outer_cross(row));
        }
        line_cross.push(tallest);
    }
    // A single-line container with a definite cross size is exactly that tall.
    let single_line = !wrap;
    if single_line {
        if let Some(cross) = container_cross {
            if let Some(first) = line_cross.first_mut() {
                *first = cross;
            }
        }
    }

    // ---- align-content: place lines on the cross axis ----
    let gaps_cross = gap_cross * (lines.len().saturating_sub(1)) as f64;
    let lines_total: f64 = line_cross.iter().sum::<f64>() + gaps_cross;
    let cross_space = container_cross;
    let mut line_offset = vec![0.0; lines.len()];
    let mut extra_between = 0.0;
    let mut start_offset = 0.0;
    if let Some(space) = cross_space {
        if !single_line {
            let free = space - lines_total;
            match align_content.as_str() {
                "flex-start" | "start" => {}
                "flex-end" | "end" => start_offset = free.max(0.0),
                "center" => start_offset = free.max(0.0) / 2.0,
                "space-between" if lines.len() > 1 && free > 0.0 => {
                    extra_between = free / (lines.len() - 1) as f64
                }
                "space-around" if free > 0.0 => {
                    extra_between = free / lines.len() as f64;
                    start_offset = extra_between / 2.0;
                }
                "space-evenly" if free > 0.0 => {
                    extra_between = free / (lines.len() + 1) as f64;
                    start_offset = extra_between;
                }
                // normal / stretch: lines share the free space equally
                _ if free > 0.0
                    && !matches!(
                        align_content.as_str(),
                        "space-between" | "space-around" | "space-evenly"
                    ) =>
                {
                    let share = free / lines.len() as f64;
                    for c in line_cross.iter_mut() {
                        *c += share;
                    }
                }
                _ => {}
            }
        }
    }
    let mut cursor = start_offset;
    for (index, cross) in line_cross.iter().enumerate() {
        line_offset[index] = cursor;
        cursor += cross + gap_cross + extra_between;
    }
    let total_cross = {
        let used: f64 = line_cross.iter().sum::<f64>() + gaps_cross;
        used + start_offset
    };
    if wrap_reverse {
        let total: f64 = line_cross.iter().sum::<f64>() + gaps_cross;
        let mut cursor = start_offset;
        for index in (0..lines.len()).rev() {
            line_offset[index] = cursor;
            cursor += line_cross[index] + gap_cross + extra_between;
        }
        let _ = total;
    }

    // ---- per line: cross alignment and main placement ----
    let used_main_total = container_main;
    for (index, line) in lines.iter().enumerate() {
        let cross_size = line_cross[index];
        // baseline alignment shares one baseline across the line
        let baseline_above = line
            .iter()
            .filter(|&&i| items[i].align == Align::Baseline && (row))
            .map(|&i| {
                items[i].margin_cross(row, true).unwrap_or(0.0)
                    + items[i].baseline.unwrap_or(items[i].cross)
            })
            .fold(0.0f64, f64::max);
        for &i in line {
            // stretch
            if items[i].align == Align::Stretch && !items[i].cross_definite {
                let margins = items[i].margin_cross(row, true).unwrap_or(0.0)
                    + items[i].margin_cross(row, false).unwrap_or(0.0);
                let stretched = (cross_size - margins).max(0.0);
                if (stretched - items[i].cross).abs() > 0.01 && stretched > items[i].cross {
                    let used = items[i].used;
                    helper.lay_out(&mut items[i], used, Some(stretched));
                    items[i].cross = stretched;
                }
            }
            let outer = items[i].outer_cross(row);
            let start_auto = items[i].margin_cross(row, true).is_none();
            let end_auto = items[i].margin_cross(row, false).is_none();
            let free = cross_size - outer;
            let margin_start = items[i].margin_cross(row, true).unwrap_or(0.0);
            let offset = if start_auto || end_auto {
                match (start_auto, end_auto) {
                    (true, true) => free.max(0.0) / 2.0,
                    (true, false) => free.max(0.0),
                    _ => 0.0,
                }
            } else {
                match items[i].align {
                    Align::Start | Align::Stretch => 0.0,
                    Align::End => free,
                    Align::Center => free / 2.0,
                    Align::Baseline if row => {
                        baseline_above - margin_start - items[i].baseline.unwrap_or(items[i].cross)
                    }
                    Align::Baseline => 0.0,
                }
            };
            items[i].cross_pos = line_offset[index] + offset + margin_start;
        }

        // main axis
        let free_main = match used_main_total {
            Some(total) => {
                total
                    - line
                        .iter()
                        .map(|&i| items[i].outer_main(row, items[i].used))
                        .sum::<f64>()
                    - gap_main * (line.len().saturating_sub(1)) as f64
            }
            None => 0.0,
        };
        let auto_margins: usize = line
            .iter()
            .map(|&i| {
                usize::from(items[i].margin_main(row, true).is_none())
                    + usize::from(items[i].margin_main(row, false).is_none())
            })
            .sum();
        let mut lead = 0.0;
        let mut between = gap_main;
        let mut auto_share = 0.0;
        if auto_margins > 0 && free_main > 0.0 {
            auto_share = free_main / auto_margins as f64;
        } else if free_main > 0.0 {
            let n = line.len() as f64;
            match justify.as_str() {
                "flex-end" | "end" | "right" => lead = free_main,
                "center" => lead = free_main / 2.0,
                "space-between" if line.len() > 1 => between += free_main / (n - 1.0),
                "space-around" => {
                    between += free_main / n;
                    lead = free_main / n / 2.0;
                }
                "space-evenly" => {
                    between += free_main / (n + 1.0);
                    lead = free_main / (n + 1.0);
                }
                _ => {}
            }
        } else if free_main < 0.0 && matches!(justify.as_str(), "center") {
            lead = free_main / 2.0;
        }
        let mut cursor = lead;
        for &i in line {
            let start_margin = items[i].margin_main(row, true).unwrap_or(auto_share);
            let end_margin = items[i].margin_main(row, false).unwrap_or(auto_share);
            items[i].main_pos = cursor + start_margin;
            cursor += start_margin + items[i].used + end_margin + between;
        }
        if reverse {
            if let Some(total) = used_main_total {
                for &i in line {
                    items[i].main_pos = total - items[i].main_pos - items[i].used;
                }
            }
        }
    }

    // ---- fragments ----
    let mut max_right = 0.0f64;
    let mut max_bottom = 0.0f64;
    for item in items {
        let mut fragment = item.fragment;
        let (x, y) = if row {
            (item.main_pos, item.cross_pos)
        } else {
            (item.cross_pos, item.main_pos)
        };
        fragment.x = x;
        fragment.y = y;
        max_right = max_right.max(x + fragment.width + item.margin[1].unwrap_or(0.0));
        max_bottom = max_bottom.max(y + fragment.height + item.margin[2].unwrap_or(0.0));
        for p in item.pending {
            result.pending_abs.push(PendingAbs {
                node: p.node,
                x: p.x + x,
                y: p.y + y,
            });
        }
        result.children.push(fragment);
    }
    result.extent = max_right;
    let natural = if row {
        total_cross.max(0.0)
    } else {
        max_bottom
    };
    result.height = match input.definite_height {
        Some(h) => h,
        None => natural
            .max(input.min_height)
            .min(input.max_height.unwrap_or(f64::INFINITY)),
    };
    result
}

#[cfg(test)]
mod tests {
    use crate::block::layout_block;
    use crate::fragment::Fragment;
    use blueice_css::{cascade, ua_stylesheet, Origin};
    use blueice_dom::{NodeData, NodeId};

    fn find(doc: &blueice_dom::Document, from: NodeId, tag: &str) -> Option<NodeId> {
        if matches!(doc.data(from), NodeData::Element { tag_name, .. } if tag_name == tag) {
            return Some(from);
        }
        doc.children(from).find_map(|c| find(doc, c, tag))
    }

    /// The first child (the flex container) of the body of `html` at `width`.
    fn flex(html: &str, css: &str, width: f64) -> Fragment {
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = blueice_css::parse(&format!("html,body,div,p,ul,ol,li,h1,h2,h3,h4,h5,h6,pre,blockquote,figure,dl,dd,menu,fieldset,hr,section,address{{margin:0;padding:0}} body {{ overflow: hidden; }} {css}")).rules;
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        let body = find(&doc, doc.root(), "body").unwrap();
        layout_block(&doc, body, &styles, width).children.remove(0)
    }

    fn xs(f: &Fragment) -> Vec<f64> {
        f.children.iter().map(|c| round(c.x)).collect()
    }
    fn ys(f: &Fragment) -> Vec<f64> {
        f.children.iter().map(|c| round(c.y)).collect()
    }
    fn ws(f: &Fragment) -> Vec<f64> {
        f.children.iter().map(|c| round(c.width)).collect()
    }
    fn hs(f: &Fragment) -> Vec<f64> {
        f.children.iter().map(|c| round(c.height)).collect()
    }
    fn round(v: f64) -> f64 {
        (v * 100.0).round() / 100.0
    }

    const ITEMS: &str = "<div class=f><i class=a></i><i class=b></i><i class=c></i></div>";
    const BASE: &str = ".f { display: flex; } i { display: block; height: 20px; } ";

    fn css(extra: &str) -> String {
        format!("{BASE}{extra}")
    }

    #[test]
    fn a_row_places_items_end_to_end_at_their_widths() {
        let f = flex(
            ITEMS,
            &css(".a{width:50px} .b{width:100px} .c{width:30px}"),
            400.0,
        );
        assert_eq!(xs(&f), vec![0.0, 50.0, 150.0]);
        assert_eq!(ws(&f), vec![50.0, 100.0, 30.0]);
        assert_eq!(
            f.width, 400.0,
            "a block-level flex container fills its parent"
        );
        assert_eq!(f.height, 20.0);
    }

    #[test]
    fn flex_grow_shares_free_space_in_proportion() {
        let f = flex(ITEMS, &css(".a{flex:1} .b{flex:2} .c{width:100px}"), 400.0);
        // 300px free over grow 1 and 2 from a zero basis.
        assert_eq!(ws(&f), vec![100.0, 200.0, 100.0]);
        let f = flex(
            ITEMS,
            &css("i{width:100px} .a{flex-grow:1} .b{flex-grow:1}"),
            400.0,
        );
        assert_eq!(
            ws(&f),
            vec![150.0, 150.0, 100.0],
            "grow adds to the base size"
        );
    }

    #[test]
    fn flex_shrink_takes_space_in_proportion_to_size() {
        let f = flex(ITEMS, &css("i{width:150px}"), 300.0);
        assert_eq!(ws(&f), vec![100.0, 100.0, 100.0]);
        let f = flex(
            ITEMS,
            &css(".a{width:300px} .b{width:100px} .c{width:100px;flex-shrink:0}"),
            400.0,
        );
        // 100px over; a and b shrink 3:1 -> 75 and 25.
        assert_eq!(ws(&f), vec![225.0, 75.0, 100.0]);
    }

    #[test]
    fn min_and_max_sizes_freeze_items_and_redistribute() {
        let f = flex(ITEMS, &css("i{width:150px} .b{min-width:130px}"), 200.0);
        // b would shrink to 66.7 but is held at 130; the rest absorb the difference.
        assert_eq!(round(f.children[1].width), 130.0);
        assert_eq!(
            round(f.children[0].width + f.children[1].width + f.children[2].width),
            200.0
        );
        let f = flex(ITEMS, &css("i{flex:1} .b{max-width:50px}"), 300.0);
        assert_eq!(
            ws(&f),
            vec![125.0, 50.0, 125.0],
            "capped item's share goes to the others"
        );
    }

    #[test]
    fn automatic_minimum_size_stops_text_items_shrinking_below_their_content() {
        let f = flex(
            "<div class=f><div class=a>unbreakable</div><div class=b>x</div></div>",
            ".f { display: flex; width: 30px; } .a, .b { flex: 0 1 100px; }",
            400.0,
        );
        let word = blueice_font::measure_text_width("unbreakable", 16.0, false, false);
        assert!(
            f.children[0].width >= word - 0.01,
            "kept at its min-content: {}",
            f.children[0].width
        );
        assert!(f.children[1].width < 100.0, "the other item did shrink");
    }

    #[test]
    fn justify_content_distributes_leftover_main_space() {
        let widths = ".a,.b,.c{width:50px}";
        let at = |j: &str| {
            xs(&flex(
                ITEMS,
                &css(&format!("{widths} .f{{justify-content:{j}}}")),
                350.0,
            ))
        };
        assert_eq!(at("flex-start"), vec![0.0, 50.0, 100.0]);
        assert_eq!(at("flex-end"), vec![200.0, 250.0, 300.0]);
        assert_eq!(at("center"), vec![100.0, 150.0, 200.0]);
        assert_eq!(at("space-between"), vec![0.0, 150.0, 300.0]);
        assert_eq!(at("space-around"), vec![33.33, 150.0, 266.67]);
        assert_eq!(at("space-evenly"), vec![50.0, 150.0, 250.0]);
    }

    #[test]
    fn align_items_places_items_across_the_line() {
        let tall = ".a{height:80px} .b{height:20px} .c{height:50px}";
        let at = |a: &str| {
            let f = flex(ITEMS, &css(&format!("{tall} .f{{align-items:{a}}}")), 300.0);
            (ys(&f), hs(&f))
        };
        assert_eq!(
            at("flex-start"),
            (vec![0.0, 0.0, 0.0], vec![80.0, 20.0, 50.0])
        );
        assert_eq!(
            at("flex-end"),
            (vec![0.0, 60.0, 30.0], vec![80.0, 20.0, 50.0])
        );
        assert_eq!(
            at("center"),
            (vec![0.0, 30.0, 15.0], vec![80.0, 20.0, 50.0])
        );
        let stretched = flex(
            "<div class=f><div class=a>a</div><div class=b style='height:50px'>b</div></div>",
            ".f { display: flex; }",
            300.0,
        );
        assert_eq!(
            hs(&stretched),
            vec![50.0, 50.0],
            "auto-height items stretch to the line; fixed ones do not"
        );
        let self_override = flex(
            ITEMS,
            &css(&format!(
                "{tall} .f{{align-items:flex-start}} .b{{align-self:flex-end}}"
            )),
            300.0,
        );
        assert_eq!(ys(&self_override), vec![0.0, 60.0, 0.0]);
    }

    #[test]
    fn baseline_alignment_lines_text_up() {
        let f = flex(
            "<div class=f><div class=a style='font-size:40px'>A</div><div class=b>b</div></div>",
            ".f { display: flex; align-items: baseline; }",
            300.0,
        );
        let a = &f.children[0];
        let b = &f.children[1];
        let baseline =
            |item: &Fragment| item.y + item.children[0].y + item.children[0].baseline.unwrap();
        assert!(
            (baseline(a) - baseline(b)).abs() < 0.01,
            "{} vs {}",
            baseline(a),
            baseline(b)
        );
        assert!(
            b.y > a.y,
            "the smaller text sits lower to meet the big one's baseline"
        );
    }

    #[test]
    fn column_direction_flexes_heights_and_stretches_widths() {
        let f = flex(
            ITEMS,
            &css(".f{flex-direction:column;height:300px} i{height:auto} .a{flex:1} .b{flex:2} .c{height:60px}"),
            200.0,
        );
        // 240px free (300 - 60) split 1:2.
        assert_eq!(hs(&f), vec![80.0, 160.0, 60.0]);
        assert_eq!(ys(&f), vec![0.0, 80.0, 240.0]);
        assert_eq!(
            ws(&f),
            vec![200.0, 200.0, 200.0],
            "stretched across the container"
        );
        let auto = flex(
            ITEMS,
            &css(".f{flex-direction:column} .a{height:10px} .b{height:20px} .c{height:30px}"),
            200.0,
        );
        assert_eq!(
            auto.height, 60.0,
            "an auto-height column is as tall as its items"
        );
        let reversed = flex(ITEMS, &css(".f{flex-direction:column-reverse;height:100px} .a{height:10px} .b{height:20px} .c{height:30px}"), 200.0);
        assert_eq!(ys(&reversed), vec![90.0, 70.0, 40.0]);
    }

    #[test]
    fn wrapping_starts_new_lines_and_gap_spaces_items() {
        let f = flex(
            ITEMS,
            &css(".f{flex-wrap:wrap;width:100px} i{width:40px}"),
            400.0,
        );
        assert_eq!(
            (xs(&f), ys(&f)),
            (vec![0.0, 40.0, 0.0], vec![0.0, 0.0, 20.0])
        );
        assert_eq!(f.height, 40.0);
        let gapped = flex(
            ITEMS,
            &css(".f{flex-wrap:wrap;width:100px;gap:10px 5px} i{width:40px}"),
            400.0,
        );
        assert_eq!(
            (xs(&gapped), ys(&gapped)),
            (vec![0.0, 45.0, 0.0], vec![0.0, 0.0, 30.0])
        );
        let nowrap = flex(
            ITEMS,
            &css(".f{width:100px;column-gap:10px} i{width:20px}"),
            400.0,
        );
        assert_eq!(xs(&nowrap), vec![0.0, 30.0, 60.0]);
        let reverse = flex(
            ITEMS,
            &css(".f{flex-wrap:wrap-reverse;width:100px} i{width:40px}"),
            400.0,
        );
        assert_eq!(
            ys(&reverse),
            vec![20.0, 20.0, 0.0],
            "later lines come first"
        );
    }

    #[test]
    fn align_content_spreads_wrapped_lines() {
        let at = |a: &str| {
            ys(&flex(ITEMS, &css(&format!(".f{{flex-wrap:wrap;width:100px;height:100px;align-content:{a}}} i{{width:60px}}")), 400.0))
        };
        assert_eq!(at("flex-start"), vec![0.0, 20.0, 40.0]);
        assert_eq!(at("flex-end"), vec![40.0, 60.0, 80.0]);
        assert_eq!(at("center"), vec![20.0, 40.0, 60.0]);
        assert_eq!(at("space-between"), vec![0.0, 40.0, 80.0]);
        assert_eq!(at("stretch"), vec![0.0, 33.33, 66.67]);
    }

    #[test]
    fn order_and_reverse_change_visual_order_only() {
        let f = flex(
            ITEMS,
            &css("i{width:30px} .a{order:3} .b{order:1} .c{order:2}"),
            400.0,
        );
        let node_order: Vec<_> = f.children.iter().map(|c| c.node).collect();
        assert_eq!(xs(&f), vec![0.0, 30.0, 60.0]);
        let doc_order = flex(ITEMS, &css("i{width:30px}"), 400.0);
        assert_ne!(
            node_order,
            doc_order
                .children
                .iter()
                .map(|c| c.node)
                .collect::<Vec<_>>()
        );
        let rev = flex(
            ITEMS,
            &css(".f{flex-direction:row-reverse} i{width:30px}"),
            400.0,
        );
        assert_eq!(xs(&rev), vec![370.0, 340.0, 310.0]);
    }

    #[test]
    fn auto_margins_absorb_free_space() {
        let f = flex(ITEMS, &css("i{width:50px} .b{margin-left:auto}"), 400.0);
        assert_eq!(xs(&f), vec![0.0, 300.0, 350.0], "b and c pushed to the end");
        let centred = flex(
            ITEMS,
            &css(".f{height:100px} i{width:50px;height:20px} .a{margin:auto}"),
            400.0,
        );
        assert_eq!(
            centred.children[0].x, 125.0,
            "250px of free space split over its two auto margins"
        );
        assert_eq!(centred.children[0].y, 40.0, "and on the cross axis");
        let cross = flex(ITEMS, &css(".f{height:100px;align-items:flex-start} i{width:50px;height:20px} .a{margin-top:auto}"), 400.0);
        assert_eq!(cross.children[0].y, 80.0);
    }

    #[test]
    fn flex_basis_and_percentages_resolve_against_the_container() {
        let f = flex(ITEMS, &css("i{flex:0 0 25%}"), 400.0);
        assert_eq!(ws(&f), vec![100.0, 100.0, 100.0]);
        let basis = flex(ITEMS, &css("i{flex-basis:30px;width:200px}"), 400.0);
        assert_eq!(ws(&basis), vec![30.0, 30.0, 30.0], "flex-basis beats width");
        let content = flex(
            "<div class=f><div class=a>abc</div></div>",
            ".f{display:flex} .a{flex-basis:content;width:500px}",
            400.0,
        );
        let abc = blueice_font::measure_text_width("abc", 16.0, false, false);
        assert!((content.children[0].width - abc).abs() < 0.01);
    }

    #[test]
    fn text_directly_in_a_flex_container_becomes_an_anonymous_item() {
        let f = flex(
            "<div class=f>hello <b>x</b></div>",
            ".f{display:flex}",
            400.0,
        );
        assert_eq!(
            f.children.len(),
            2,
            "an anonymous item for the text, and the element"
        );
        let hello = blueice_font::measure_text_width("hello", 16.0, false, false);
        assert!(
            (f.children[0].width - hello).abs() < 0.5,
            "{}",
            f.children[0].width
        );
        assert!(f.children[1].x >= f.children[0].width);
    }

    #[test]
    fn container_sizes_follow_their_items() {
        let row = flex(ITEMS, &css("i{width:10px} .b{height:70px}"), 400.0);
        assert_eq!(
            row.height, 70.0,
            "the tallest item sets an auto row's height"
        );
        let fixed = flex(
            ITEMS,
            &css(".f{height:200px} i{width:10px;height:auto}"),
            400.0,
        );
        assert_eq!(fixed.height, 200.0);
        assert_eq!(
            hs(&fixed),
            vec![200.0, 200.0, 200.0],
            "items stretch to a definite height"
        );
        let min = flex(ITEMS, &css(".f{min-height:90px} i{width:10px}"), 400.0);
        assert_eq!(min.height, 90.0);
    }

    #[test]
    fn nested_flex_containers_and_inline_flex_shrink_to_fit() {
        let f = flex(
            "<div class=f><div class=g><i class=a></i><i class=b></i></div><i class=c></i></div>",
            &css(".g{display:flex;flex:1} i{width:30px}"),
            400.0,
        );
        assert_eq!(round(f.children[0].width), 370.0);
        assert_eq!(xs(&f.children[0]), vec![0.0, 30.0]);
        let inline = flex(
            "<div class=w><span class=f><i class=a></i><i class=b></i></span></div>",
            &css(".f{display:inline-flex} i{width:30px;height:10px}"),
            400.0,
        );
        let span = &inline.children[0];
        assert_eq!(
            span.width, 60.0,
            "an inline-flex container is as wide as its items"
        );
    }

    #[test]
    fn absolutely_positioned_children_leave_the_flex_flow() {
        let f = flex(
            "<div class=f><i class=a></i><i class=x></i><i class=c></i></div>",
            &css(".f{position:relative} i{width:40px} .x{position:absolute;top:5px;left:7px;width:10px;height:10px}"),
            400.0,
        );
        let flowing: Vec<_> = f.children.iter().filter(|c| !c.positioned).collect();
        assert_eq!(flowing.len(), 2);
        assert_eq!(round(flowing[1].x), 40.0, "c follows a directly");
        assert!(f
            .children
            .iter()
            .any(|c| c.positioned && c.x == 7.0 && c.y == 5.0));
    }

    #[test]
    fn webkit_box_is_a_flex_container() {
        let f = flex(ITEMS, &css(".f{display:-webkit-box} i{width:20px}"), 400.0);
        assert_eq!(xs(&f), vec![0.0, 20.0, 40.0]);
    }
}
