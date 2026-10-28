// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Block layout, per `research/layout.md` §4: a single-pass, top-down walk
//! that stacks block-level children in the block direction (collapsing their
//! vertical margins) and groups consecutive inline-level children into
//! anonymous line-box runs.
//!
//! Lengths resolve through [`crate::ctx::Style`], so `rem`, viewport units,
//! `calc()` and percentages work everywhere; `auto` side margins centre or
//! push a box; `min-`/`max-` width and height clamp; `position: relative`
//! offsets a box without moving its siblings; absolutely positioned boxes
//! are collected on the way up and placed against their containing block
//! (the nearest positioned ancestor, or the initial containing block) once
//! its size is known.

use crate::ctx::{Ctx, Display, Position, Style};
use crate::fragment::{Fragment, FragmentKind};
use crate::text::{default_line_height, StyleMap};
use blueice_css::Value;
use blueice_dom::{Document, NodeData, NodeId};

/// A margin that can collapse: the largest positive and most negative parts
/// contributing to it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Margin {
    pos: f64,
    neg: f64,
}

impl Margin {
    pub fn of(value: f64) -> Margin {
        Margin {
            pos: value.max(0.0),
            neg: value.min(0.0),
        }
    }
    pub fn join(self, other: Margin) -> Margin {
        Margin {
            pos: self.pos.max(other.pos),
            neg: self.neg.min(other.neg),
        }
    }
    pub fn value(self) -> f64 {
        self.pos + self.neg
    }
}

/// What a block child needs to know about the box it sits in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Constraint {
    /// The containing block's content width: what percentages and `auto`
    /// margins resolve against.
    pub cb_width: f64,
    /// The containing block's content height when it is definite.
    pub cb_height: Option<f64>,
    /// The root element's box.
    pub root: bool,
}

/// An absolutely positioned box waiting for its containing block.
#[derive(Debug, Clone)]
pub(crate) struct PendingAbs {
    pub node: NodeId,
    /// Static position, in the coordinates of the fragment currently holding
    /// the pending box (its border-box origin).
    pub x: f64,
    pub y: f64,
}

/// A laid-out block box plus what its parent needs to place it.
pub(crate) struct Placed {
    pub fragment: Fragment,
    pub margin_top: Margin,
    pub margin_bottom: Margin,
    pub margin_left: f64,
    pub margin_right: f64,
    pub pending_abs: Vec<PendingAbs>,
    /// No height, padding or border: its margins collapse through it.
    pub collapses_through: bool,
    /// The border-box width this box would take given unlimited room: its
    /// specified width, or its content plus padding and border (for
    /// shrink-to-fit sizing).
    pub preferred_width: f64,
    /// The farthest right edge of its in-flow content from its content-box
    /// origin.
    pub content_extent: f64,
}

/// Whether the box establishes a new block formatting context: its margins do
/// not collapse with its children's.
fn is_bfc_root(style: &Style<'_>, display: Display, root: bool) -> bool {
    root || style.float().is_some()
        || style.is_out_of_flow()
        || style.clips_overflow()
        || !matches!(
            display,
            Display::Block | Display::ListItem | Display::Inline
        )
}

/// What an in-flow child of a block container is.
enum Item {
    Block(NodeId),
    Inline(NodeId),
    Abs(NodeId),
}

pub(crate) fn is_replaced_tag(tag: &str) -> bool {
    matches!(
        tag,
        "img" | "svg" | "canvas" | "video" | "iframe" | "embed" | "object"
    )
}

/// Whether an inline element has a block-level descendant reachable through
/// inline boxes alone: such an element splits its line, which this engine
/// renders by letting the element's children stand in for it.
fn has_block_descendant(ctx: &Ctx<'_>, node: NodeId) -> bool {
    ctx.doc.children(node).any(|child| {
        let Some(tag) = ctx.tag(child) else {
            return false;
        };
        let style = ctx.style(child);
        let display = style.display();
        if display == Display::None || style.is_out_of_flow() || style.float().is_some() {
            false
        } else if display == Display::Contents
            || (display == Display::Inline && !is_replaced_tag(tag))
        {
            has_block_descendant(ctx, child)
        } else {
            !display.is_atomic_inline() && !is_replaced_tag(tag)
        }
    })
}

/// The children of `node` as layout sees them: `display: none` removed,
/// `display: contents` replaced by its own children.
fn flow_items(ctx: &Ctx<'_>, node: NodeId, out: &mut Vec<Item>) {
    for child in ctx.doc.children(node) {
        match ctx.doc.data(child) {
            NodeData::Text { .. } => out.push(Item::Inline(child)),
            NodeData::Document => {}
            NodeData::Element { tag_name, .. } => {
                let style = ctx.style(child);
                let display = style.display();
                if display == Display::None || display == Display::TableColumn {
                    continue;
                }
                if style.is_out_of_flow() {
                    out.push(Item::Abs(child));
                } else if display == Display::Contents {
                    flow_items(ctx, child, out);
                } else if style.float().is_some() {
                    // Floats are laid out as blocks for now.
                    out.push(Item::Block(child));
                } else if display == Display::Inline && !is_replaced_tag(tag_name) {
                    if has_block_descendant(ctx, child) {
                        flow_items(ctx, child, out);
                    } else {
                        out.push(Item::Inline(child));
                    }
                } else if display.is_atomic_inline()
                    || is_replaced_tag(tag_name) && display == Display::Inline
                {
                    out.push(Item::Inline(child));
                } else {
                    out.push(Item::Block(child));
                }
            }
        }
    }
}

struct Flow {
    children: Vec<Fragment>,
    /// Height of the in-flow content, including margins that did not pass
    /// through to the parent.
    height: f64,
    top_margin: Margin,
    bottom_margin: Margin,
    pending_abs: Vec<PendingAbs>,
    extent: f64,
    /// Nothing in flow took any height.
    empty: bool,
}

#[allow(clippy::too_many_arguments, unused_assignments)]
fn layout_flow(
    ctx: &Ctx<'_>,
    node: NodeId,
    content_width: f64,
    content_height: Option<f64>,
    collapse_top: bool,
    collapse_bottom: bool,
) -> Flow {
    let mut items = Vec::new();
    flow_items(ctx, node, &mut items);

    let mut flow = Flow {
        children: Vec::new(),
        height: 0.0,
        top_margin: Margin::default(),
        bottom_margin: Margin::default(),
        pending_abs: Vec::new(),
        extent: 0.0,
        empty: true,
    };
    let mut cursor_y = 0.0;
    let mut pending = Margin::default();
    // Until something takes space, a first child's top margin may still pass
    // through to this box's own top margin.
    let mut top_open = true;
    let mut inline_run: Vec<NodeId> = Vec::new();

    let mut no_lines_yet = true;
    macro_rules! flush_inline {
        () => {
            if !inline_run.is_empty() {
                let run =
                    crate::inline::layout_run(ctx, node, &inline_run, content_width, no_lines_yet);
                inline_run.clear();
                let start = cursor_y + if top_open { 0.0 } else { pending.value() };
                flow.pending_abs
                    .extend(shift_pending(run.pending_abs, 0.0, start));
                if !run.lines.is_empty() {
                    if top_open && collapse_top {
                        flow.top_margin = flow.top_margin.join(pending);
                    } else {
                        cursor_y += pending.value();
                    }
                    pending = Margin::default();
                    top_open = false;
                    no_lines_yet = false;
                    flow.empty = false;
                    flow.extent = flow.extent.max(run.extent);
                    for mut line in run.lines {
                        line.y += cursor_y;
                        flow.children.push(line);
                    }
                    cursor_y += run.height;
                }
            }
        };
    }

    for item in items {
        match item {
            Item::Inline(child) => inline_run.push(child),
            Item::Abs(child) => {
                flow.pending_abs.push(PendingAbs {
                    node: child,
                    x: 0.0,
                    y: cursor_y + if top_open { 0.0 } else { pending.value() },
                });
            }
            Item::Block(child) => {
                flush_inline!();
                let constraint = Constraint {
                    cb_width: content_width,
                    cb_height: content_height,
                    root: false,
                };
                let mut placed = layout_child_block(ctx, child, &constraint);
                let child_style = ctx.style(child);
                if placed.collapses_through {
                    pending = pending.join(placed.margin_top).join(placed.margin_bottom);
                    let mut fragment = placed.fragment;
                    fragment.x = placed.margin_left;
                    fragment.y = cursor_y + if top_open { 0.0 } else { pending.value() };
                    apply_relative(ctx, &child_style, &mut fragment, &constraint);
                    flow.pending_abs.extend(shift_pending(
                        placed.pending_abs,
                        fragment.x,
                        fragment.y,
                    ));
                    flow.children.push(fragment);
                    continue;
                }
                if top_open && collapse_top {
                    flow.top_margin = flow.top_margin.join(pending).join(placed.margin_top);
                } else {
                    cursor_y += pending.join(placed.margin_top).value();
                }
                top_open = false;
                no_lines_yet = false;
                flow.empty = false;
                let mut fragment = std::mem::replace(&mut placed.fragment, Fragment::empty_block());
                fragment.x = placed.margin_left;
                fragment.y = cursor_y;
                flow.extent = flow
                    .extent
                    .max(placed.margin_left + placed.preferred_width + placed.margin_right);
                cursor_y += fragment.height;
                pending = placed.margin_bottom;
                apply_relative(ctx, &child_style, &mut fragment, &constraint);
                flow.pending_abs
                    .extend(shift_pending(placed.pending_abs, fragment.x, fragment.y));
                flow.children.push(fragment);
            }
        }
    }
    flush_inline!();

    if collapse_bottom && !top_open {
        flow.bottom_margin = pending;
    } else if top_open && collapse_top && collapse_bottom {
        // Nothing but collapsed-through margins: they all pass through.
        flow.top_margin = flow.top_margin.join(pending);
    } else {
        cursor_y += pending.value();
    }
    flow.height = cursor_y;
    flow
}

fn shift_pending(pending: Vec<PendingAbs>, dx: f64, dy: f64) -> Vec<PendingAbs> {
    pending
        .into_iter()
        .map(|p| PendingAbs {
            node: p.node,
            x: p.x + dx,
            y: p.y + dy,
        })
        .collect()
}

/// Lays out one block-level child according to its `display`.
pub(crate) fn layout_child_block(ctx: &Ctx<'_>, node: NodeId, c: &Constraint) -> Placed {
    layout_block_level(ctx, node, c, None)
}

/// The offsets `position: relative` applies, in pixels.
fn relative_offset(style: &Style<'_>, c: &Constraint) -> (f64, f64) {
    let horizontal_base = c.cb_width;
    let vertical_base = c.cb_height.unwrap_or(f64::NAN);
    let left = style.px("left", horizontal_base);
    let right = style.px("right", horizontal_base);
    let top = style.px("top", vertical_base);
    let bottom = style.px("bottom", vertical_base);
    (
        left.or(right.map(|r| -r)).unwrap_or(0.0),
        top.or(bottom.map(|b| -b)).unwrap_or(0.0),
    )
}

/// Moves a `position: relative` box by its offsets; siblings do not move.
fn apply_relative(ctx: &Ctx<'_>, style: &Style<'_>, fragment: &mut Fragment, c: &Constraint) {
    let _ = ctx;
    if style.position() == Position::Relative {
        let (dx, dy) = relative_offset(style, c);
        fragment.x += dx;
        fragment.y += dy;
    }
}

/// The `Fragment` flags every laid-out element box carries.
fn mark(style: &Style<'_>, fragment: &mut Fragment) {
    let position = style.position();
    fragment.positioned = position != Position::Static;
    fragment.fixed = position == Position::Fixed;
    fragment.z_index = if fragment.positioned {
        style.z_index()
    } else {
        None
    };
    fragment.clips = style.clips_overflow();
    fragment.hidden = !style.is_visible();
}

/// Lays out `node` as a block-level box inside the containing block `c`.
/// `forced_width` is a border-box width imposed from outside (a stretched
/// flex item); otherwise the width comes from `width`/`auto`.
pub(crate) fn layout_block_level(
    ctx: &Ctx<'_>,
    node: NodeId,
    c: &Constraint,
    forced_width: Option<f64>,
) -> Placed {
    let style = ctx.style(node);
    let display = style.display();
    let cbw = c.cb_width;
    let padding = style.padding(cbw);
    let border = style.border(cbw);
    let pb_h = padding.horizontal() + border.horizontal();
    let pb_v = padding.vertical() + border.vertical();
    let border_box = style.is_border_box();
    let to_content_w = |v: f64| {
        if border_box {
            (v - pb_h).max(0.0)
        } else {
            v.max(0.0)
        }
    };
    let to_content_h = |v: f64| {
        if border_box {
            (v - pb_v).max(0.0)
        } else {
            v.max(0.0)
        }
    };

    // ---- horizontal: width and side margins ----
    let auto_left = style.is_auto("margin-left");
    let auto_right = style.is_auto("margin-right");
    let mut margin_left = if auto_left {
        0.0
    } else {
        style.side("margin-left", cbw)
    };
    let mut margin_right = if auto_right {
        0.0
    } else {
        style.side("margin-right", cbw)
    };
    let min_width = style.px("min-width", cbw).map(to_content_w).unwrap_or(0.0);
    let max_width = style.px("max-width", cbw).map(to_content_w);
    let clamp_width = |w: f64| {
        let w = max_width.map_or(w, |m| w.min(m));
        w.max(min_width)
    };
    let specified_width = match forced_width {
        Some(w) => Some((w - pb_h).max(0.0)),
        None => style.px("width", cbw).map(to_content_w),
    };
    let (content_width, width_determined) = match specified_width {
        Some(w) => (clamp_width(w), true),
        None => {
            let available = (cbw - margin_left - margin_right - pb_h).max(0.0);
            let clamped = clamp_width(available);
            (clamped, clamped != available)
        }
    };
    if width_determined && forced_width.is_none() {
        let free = cbw - content_width - pb_h - margin_left - margin_right;
        match (auto_left, auto_right) {
            (true, true) => {
                margin_left = (free / 2.0).max(0.0);
                margin_right = free.max(0.0) - margin_left;
            }
            (true, false) => margin_left = free.max(0.0),
            (false, true) => margin_right = free.max(0.0),
            (false, false) => {}
        }
    }
    let margin_top = style.side("margin-top", cbw);
    let margin_bottom = style.side("margin-bottom", cbw);

    // ---- vertical: height ----
    let height_base = c.cb_height.unwrap_or(f64::NAN);
    let specified_height = style.px("height", height_base).map(to_content_h);
    let min_height = style
        .px("min-height", height_base)
        .map(to_content_h)
        .unwrap_or(0.0);
    let max_height = style.px("max-height", height_base).map(to_content_h);
    let clamp_height = |h: f64| {
        let h = max_height.map_or(h, |m| h.min(m));
        h.max(min_height)
    };
    let child_cb_height = specified_height.map(clamp_height);

    let bfc = is_bfc_root(&style, display, c.root);
    let collapse_top = !bfc && padding.top == 0.0 && border.top == 0.0;
    let collapse_bottom = !bfc
        && padding.bottom == 0.0
        && border.bottom == 0.0
        && specified_height.is_none()
        && min_height == 0.0;

    let native = crate::native_text::control_text(ctx.doc, node);
    let mut form = crate::native_text::form_control(ctx.doc, node);
    let is_native = native.is_some() || form.is_some();

    let (
        mut children,
        mut content_flow_height,
        top_pass,
        bottom_pass,
        mut pending_abs,
        extent,
        empty,
    );
    if is_native {
        children = Vec::new();
        content_flow_height = 0.0;
        top_pass = Margin::default();
        bottom_pass = Margin::default();
        pending_abs = Vec::new();
        extent = 0.0;
        empty = false;
    } else {
        let flow = layout_flow(
            ctx,
            node,
            content_width,
            child_cb_height,
            collapse_top,
            collapse_bottom,
        );
        children = flow.children;
        content_flow_height = flow.height;
        top_pass = flow.top_margin;
        bottom_pass = flow.bottom_margin;
        pending_abs = flow.pending_abs;
        extent = flow.extent;
        empty = flow.empty;
    }

    let font_size = style.font_size();
    if let Some(crate::NativeForm::SelectList {
        options,
        first,
        row_height,
        ..
    }) = &mut form
    {
        *row_height = default_line_height(font_size).max(20.0);
        *first = options
            .iter()
            .position(|option| option.selected)
            .unwrap_or(0)
            .saturating_sub(crate::select_display_size(ctx.doc, node).min(1000) as usize - 1);
        content_flow_height =
            *row_height * crate::select_display_size(ctx.doc, node).min(1000) as f64;
    } else if matches!(form, Some(crate::NativeForm::Select(_))) {
        content_flow_height = default_line_height(font_size).max(20.0);
    }
    let option_placeholder = |node: NodeId| Fragment {
        node: Some(node),
        ..Fragment::empty_block()
    };
    if let Some(crate::NativeForm::SelectList { options, .. }) = &form {
        children.extend(options.iter().map(|option| option_placeholder(option.node)));
    } else if matches!(form, Some(crate::NativeForm::Select(_))) {
        children.extend(
            crate::select_options(ctx.doc, node)
                .iter()
                .map(|option| option_placeholder(option.node)),
        );
    }
    if let Some(value) = native {
        let line_height = style
            .style
            .and_then(|s| s.line_height.as_ref())
            .and_then(|line_height| line_height_for(&style, line_height))
            .unwrap_or_else(|| default_line_height(font_size));
        let lines = crate::native_text::lines(
            &value,
            node,
            content_width,
            font_size,
            line_height,
            style.style.is_some_and(|s| s.is_bold()),
            style.style.is_some_and(|s| s.is_italic()),
        );
        content_flow_height += lines.len() as f64 * line_height;
        children.extend(lines);
    }

    // ---- final box ----
    let intrinsic_height = content_flow_height;
    let content_height = clamp_height(specified_height.unwrap_or(intrinsic_height));
    let origin_x = border.left + padding.left;
    let origin_y = border.top + padding.top;
    for f in &mut children {
        f.x += origin_x;
        f.y += origin_y;
    }
    for p in &mut pending_abs {
        p.x += origin_x;
        p.y += origin_y;
    }

    let collapses_through = !bfc
        && !is_native
        && empty
        && specified_height.is_none()
        && min_height == 0.0
        && padding.vertical() == 0.0
        && border.vertical() == 0.0
        && children.is_empty()
        && content_flow_height == 0.0;
    let mut fragment = Fragment {
        node: Some(node),
        kind: if is_native {
            FragmentKind::NativeControl {
                content_x: origin_x,
                content_y: origin_y,
                content_width,
                content_height,
                form,
            }
        } else {
            FragmentKind::Block
        },
        x: 0.0,
        y: 0.0,
        width: content_width + pb_h,
        height: content_height + pb_v,
        children,
        ..Fragment::empty_block()
    };
    mark(&style, &mut fragment);
    crate::update_select_option_bounds(&mut fragment);

    // Absolutely positioned descendants whose containing block is this box.
    let is_container = style.position() != Position::Static || c.root;
    let mut escaping = Vec::new();
    if is_container {
        let (container_x, container_y, container_w, container_h) = if c.root {
            (
                0.0,
                0.0,
                ctx.viewport_width.max(fragment.width),
                ctx.viewport_height.max(fragment.height),
            )
        } else {
            (
                border.left,
                border.top,
                content_width + padding.horizontal(),
                content_height + padding.vertical(),
            )
        };
        let mut placed_abs = Vec::new();
        for pending in std::mem::take(&mut pending_abs) {
            let fixed = ctx.style(pending.node).position() == Position::Fixed;
            if fixed && !c.root {
                escaping.push(pending);
            } else {
                let (vx, vy, vw, vh) = if fixed {
                    (0.0, 0.0, ctx.viewport_width, ctx.viewport_height)
                } else {
                    (container_x, container_y, container_w, container_h)
                };
                placed_abs.push(layout_abs(ctx, &pending, (vx, vy, vw, vh)));
            }
        }
        fragment.children.extend(placed_abs);
    } else {
        escaping = pending_abs;
    }

    let pass_top = if collapse_top {
        Margin::of(margin_top).join(top_pass)
    } else {
        Margin::of(margin_top)
    };
    let pass_bottom = if collapse_bottom {
        Margin::of(margin_bottom).join(bottom_pass)
    } else {
        Margin::of(margin_bottom)
    };
    let preferred_width = if specified_width.is_some() {
        content_width + pb_h
    } else {
        extent.max(0.0) + pb_h
    };
    Placed {
        fragment,
        margin_top: pass_top,
        margin_bottom: pass_bottom,
        margin_left,
        margin_right,
        pending_abs: escaping,
        collapses_through,
        preferred_width,
        content_extent: extent,
    }
}

/// An absolutely positioned box placed against `cb` (x, y, width, height in
/// the coordinates of the fragment that contains it).
fn layout_abs(ctx: &Ctx<'_>, pending: &PendingAbs, cb: (f64, f64, f64, f64)) -> Fragment {
    let (cb_x, cb_y, cb_w, cb_h) = cb;
    let style = ctx.style(pending.node);
    let padding = style.padding(cb_w);
    let border = style.border(cb_w);
    let pb_h = padding.horizontal() + border.horizontal();
    let pb_v = padding.vertical() + border.vertical();
    let border_box = style.is_border_box();
    let to_w = |v: f64| {
        if border_box {
            (v - pb_h).max(0.0)
        } else {
            v.max(0.0)
        }
    };
    let to_h = |v: f64| {
        if border_box {
            (v - pb_v).max(0.0)
        } else {
            v.max(0.0)
        }
    };

    let left = style.px("left", cb_w);
    let right = style.px("right", cb_w);
    let top = style.px("top", cb_h);
    let bottom = style.px("bottom", cb_h);
    let auto_ml = style.is_auto("margin-left");
    let auto_mr = style.is_auto("margin-right");
    let mut ml = if auto_ml {
        0.0
    } else {
        style.side("margin-left", cb_w)
    };
    let mut mr = if auto_mr {
        0.0
    } else {
        style.side("margin-right", cb_w)
    };
    let mt = style.side("margin-top", cb_w);
    let mb = style.side("margin-bottom", cb_w);

    let min_w = style.px("min-width", cb_w).map(to_w).unwrap_or(0.0);
    let max_w = style.px("max-width", cb_w).map(to_w);
    let clamp_w = |w: f64| max_w.map_or(w, |m| w.min(m)).max(min_w);
    let specified_w = style.px("width", cb_w).map(to_w);
    let content_width = match (specified_w, left, right) {
        (Some(w), _, _) => clamp_w(w),
        (None, Some(l), Some(r)) => clamp_w((cb_w - l - r - ml - mr - pb_h).max(0.0)),
        _ => {
            // Shrink-to-fit: the preferred width, but no wider than what is
            // left of the containing block.
            let available =
                (cb_w - left.unwrap_or(0.0) - right.unwrap_or(0.0) - ml - mr - pb_h).max(0.0);
            let (min_content, max_content) = crate::intrinsic::preferred_widths(ctx, pending.node);
            clamp_w(max_content.min(available.max(min_content)) - pb_h).max(0.0)
        }
    };
    if let (true, Some(l), Some(r)) = (specified_w.is_some(), left, right) {
        let free = cb_w - l - r - content_width - pb_h - ml - mr;
        match (auto_ml, auto_mr) {
            (true, true) => {
                ml = (free / 2.0).max(0.0);
                mr = free.max(0.0) - ml;
            }
            (true, false) => ml = free.max(0.0),
            (false, true) => mr = free.max(0.0),
            _ => {}
        }
    }

    let specified_h = style.px("height", cb_h).map(to_h);
    let min_h = style.px("min-height", cb_h).map(to_h).unwrap_or(0.0);
    let max_h = style.px("max-height", cb_h).map(to_h);
    let clamp_h = |h: f64| max_h.map_or(h, |m| h.min(m)).max(min_h);
    let stretched_h = match (specified_h, top, bottom) {
        (None, Some(t), Some(b)) => Some(to_h((cb_h - t - b - mt - mb).max(0.0)).max(0.0)),
        _ => None,
    };
    let forced_height = specified_h.or(stretched_h);

    let constraint = Constraint {
        cb_width: cb_w,
        cb_height: Some(cb_h),
        root: false,
    };
    // Laid out as a block of the computed width; `Display` is blockified.
    let mut placed = layout_block_level(ctx, pending.node, &constraint, Some(content_width + pb_h));
    let mut fragment = placed.fragment;
    if let Some(h) = forced_height {
        fragment.height = clamp_h(h) + pb_v;
    } else {
        fragment.height = clamp_h((fragment.height - pb_v).max(0.0)) + pb_v;
    }
    let width = fragment.width;
    let height = fragment.height;
    fragment.x = match (left, right) {
        (Some(l), _) => cb_x + l + ml,
        (None, Some(r)) => cb_x + cb_w - r - mr - width,
        (None, None) => pending.x + ml,
    };
    fragment.y = match (top, bottom) {
        (Some(t), _) => cb_y + t + mt,
        (None, Some(b)) => cb_y + cb_h - b - mb - height,
        (None, None) => pending.y + mt,
    };
    fragment.positioned = true;
    fragment.fixed = style.position() == Position::Fixed;
    fragment.z_index = style.z_index();
    // An abspos box's own descendants escape only if they are fixed.
    let _ = &mut placed.pending_abs;
    fragment
}

fn line_height_for(style: &Style<'_>, value: &Value) -> Option<f64> {
    let font_size = style.font_size();
    match value {
        Value::Number(n) => Some(n * font_size),
        Value::Keyword(k) if k == "normal" => None,
        other => other.to_px(&style.length_context(font_size)),
    }
}

/// Compatibility entry point: lays out `node` at `available_width` against a
/// default viewport. Used by tests; real callers go through [`layout_root`].
#[cfg(test)]
pub(crate) fn layout_block(
    doc: &Document,
    node: NodeId,
    styles: &StyleMap,
    available_width: f64,
) -> Fragment {
    let ctx = Ctx {
        doc,
        styles,
        viewport_width: available_width,
        viewport_height: 720.0,
        root_font_size: 16.0,
        intrinsic: Default::default(),
    };
    let c = Constraint {
        cb_width: available_width,
        cb_height: None,
        root: false,
    };
    layout_block_level(&ctx, node, &c, None).fragment
}

/// Lays out the root element against the viewport.
pub(crate) fn layout_root(
    doc: &Document,
    node: NodeId,
    styles: &StyleMap,
    viewport_width: f64,
    viewport_height: f64,
) -> Fragment {
    let root_font_size = styles.get(&node).map_or(16.0, |s| s.font_size_px);
    let ctx = Ctx {
        doc,
        styles,
        viewport_width,
        viewport_height,
        root_font_size,
        intrinsic: Default::default(),
    };
    let c = Constraint {
        cb_width: viewport_width,
        cb_height: Some(viewport_height),
        root: true,
    };
    layout_block_level(&ctx, node, &c, None).fragment
}

/// Apply the list's scroll position and clip option hit/AX bounds to its content.
pub fn update_select_option_bounds(fragment: &mut Fragment) {
    let FragmentKind::NativeControl {
        content_x,
        content_y,
        content_width,
        content_height,
        form:
            Some(crate::NativeForm::SelectList {
                options,
                first,
                row_height,
                ..
            }),
    } = &fragment.kind
    else {
        return;
    };
    for (index, child) in fragment.children.iter_mut().enumerate() {
        if options.get(index).map(|option| option.node) != child.node {
            continue;
        }
        let top = (index as f64 - *first as f64) * *row_height;
        let bottom = (top + *row_height).min(*content_height);
        child.x = *content_x;
        child.y = *content_y + top.max(0.0);
        child.width = if bottom > top.max(0.0) {
            *content_width
        } else {
            0.0
        };
        child.height = (bottom - top.max(0.0)).max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_css::{cascade, ua_stylesheet, Origin};
    use std::collections::HashMap;

    fn find_by_tag(doc: &Document, root: NodeId, tag: &str) -> Option<NodeId> {
        if let NodeData::Element { tag_name, .. } = doc.data(root) {
            if tag_name == tag {
                return Some(root);
            }
        }
        doc.children(root).find_map(|c| find_by_tag(doc, c, tag))
    }

    fn styled(doc: &Document, html: &str, css: &str) -> (Document, StyleMap) {
        let _ = doc;
        let doc = blueice_html::parse(html);
        let ua = ua_stylesheet();
        let author = blueice_css::parse(css).rules;
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        (doc, styles)
    }

    fn body_fragment(html: &str, css: &str, available_width: f64) -> Fragment {
        let placeholder = Document::new();
        let (doc, styles) = styled(&placeholder, html, css);
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        layout_block(&doc, body, &styles, available_width)
    }

    #[test]
    fn plain_block_fills_available_width_with_zero_box_model() {
        let f = body_fragment("<div>x</div>", "", 800.0);
        let div = &f.children[0];
        assert_eq!(div.width, 800.0);
        assert_eq!(div.x, 0.0);
    }

    #[test]
    fn explicit_width_and_px_margin_are_respected() {
        // `overflow` makes body a block formatting context, so the div's margin
        // stays inside it instead of collapsing through (see the collapsing tests).
        let f = body_fragment(
            "<div>x</div>",
            "body { overflow: hidden; } div { width: 200px; margin: 10px; }",
            800.0,
        );
        let div = &f.children[0];
        assert_eq!(div.width, 200.0);
        assert_eq!(div.x, 10.0, "left margin offsets the box");
        assert_eq!(div.y, 10.0, "top margin offsets the box");
    }

    #[test]
    fn percentage_width_resolves_against_containing_block() {
        let f = body_fragment("<div>x</div>", "div { width: 50%; }", 800.0);
        assert_eq!(f.children[0].width, 400.0);
    }

    #[test]
    fn em_margin_resolves_against_the_elements_own_font_size() {
        let f = body_fragment(
            "<div>x</div>",
            "body { overflow: hidden; } div { font-size: 20px; margin-top: 2em; }",
            800.0,
        );
        assert_eq!(f.children[0].y, 40.0);
    }

    #[test]
    fn box_sizing_border_box_subtracts_padding_and_border_from_specified_width() {
        let f = body_fragment(
            "<div>x</div>",
            "div { width: 200px; padding: 10px; border-left-width: 5px; border-right-width: 5px; box-sizing: border-box; }",
            800.0,
        );
        assert_eq!(
            f.children[0].width, 200.0,
            "border-box width is the total, not content + extra"
        );
    }

    #[test]
    fn content_box_sizing_adds_padding_and_border_on_top_of_specified_width() {
        let f = body_fragment(
            "<div>x</div>",
            "div { width: 200px; padding: 10px; }",
            800.0,
        );
        assert_eq!(f.children[0].width, 220.0);
    }

    #[test]
    fn two_block_children_stack_vertically_by_height_plus_margins() {
        let f = body_fragment(
            "<div>a</div><div>b</div>",
            "div { height: 50px; margin-bottom: 10px; }",
            800.0,
        );
        assert_eq!(f.children.len(), 2);
        assert_eq!(f.children[0].y, 0.0);
        assert_eq!(
            f.children[1].y, 60.0,
            "second div starts after first's height + margin-bottom"
        );
    }

    // ---- interaction tests: added by a dedicated post-implementation
    // test-review pass (per TEST_PLAN.md's Definition of Done) -- each
    // of these combines two features whose own single-feature tests
    // above don't actually exercise together.

    #[test]
    fn padding_offsets_a_nested_block_child_not_just_inline_content() {
        // every existing padding/content-origin test above only checks
        // it against inline text; a block *child* needs the same
        // content_origin_x/y offset applied to it.
        let f = body_fragment("<div><p>x</p></div>", "div { padding: 10px; }", 800.0);
        let div = &f.children[0];
        let p = &div.children[0];
        assert_eq!(p.x, 10.0);
        assert_eq!(p.y, 10.0);
    }

    #[test]
    fn inline_run_flushes_before_a_following_block_sibling() {
        let f = body_fragment("<div>text<p>block</p></div>", "", 800.0);
        let div = &f.children[0];
        assert_eq!(
            div.children.len(),
            2,
            "one line fragment, then the p's block fragment"
        );
        let line = &div.children[0];
        assert_eq!(line.kind, FragmentKind::Line);
        let p = &div.children[1];
        assert_eq!(p.kind, FragmentKind::Block);
        assert_eq!(
            p.y,
            line.y + line.height,
            "p starts immediately after the flushed line, not overlapping it"
        );
    }

    #[test]
    fn inline_run_also_flushes_correctly_when_it_comes_after_a_block_sibling() {
        let f = body_fragment("<p>block</p>text", "", 800.0);
        let p = &f.children[0];
        assert_eq!(p.kind, FragmentKind::Block);
        let line = &f.children[1];
        assert_eq!(line.kind, FragmentKind::Line);
        assert_eq!(
            line.y,
            p.y + p.height,
            "trailing inline text starts after the preceding block, not at y=0"
        );
    }

    #[test]
    fn display_none_removes_the_element_and_its_subtree() {
        let f = body_fragment(
            "<div>a</div><div class=\"gone\"><span>x</span></div><div>b</div>",
            ".gone { display: none; }",
            800.0,
        );
        assert_eq!(
            f.children.len(),
            2,
            "the display:none div contributes no fragment at all"
        );
    }

    #[test]
    fn inline_text_produces_a_line_fragment_with_correct_word_positions() {
        let f = body_fragment("<p>hello world</p>", "", 800.0);
        let p = &f.children[0];
        assert_eq!(p.children.len(), 1, "one line, both words fit");
        let line = &p.children[0];
        assert_eq!(line.kind, FragmentKind::Line);
        assert_eq!(line.children.len(), 2);
        assert_eq!(
            line.children[0].kind,
            FragmentKind::Text("hello".to_string())
        );
        assert_eq!(
            line.children[1].kind,
            FragmentKind::Text("world".to_string())
        );
        assert!(
            line.children[1].x > line.children[0].x + line.children[0].width,
            "world starts after hello plus a space gap"
        );
    }

    #[test]
    fn narrow_container_wraps_inline_content_onto_multiple_lines() {
        let f = body_fragment("<p>aaaa bbbb cccc</p>", "", 40.0);
        let p = &f.children[0];
        assert!(
            p.children.len() > 1,
            "text must wrap across multiple lines in a narrow container"
        );
        for line in &p.children {
            assert_eq!(line.kind, FragmentKind::Line);
        }
    }

    #[test]
    fn nested_inline_element_words_are_tagged_with_their_own_node() {
        let placeholder = Document::new();
        let (doc, styles) = styled(&placeholder, "<p>a <b>bold</b> c</p>", "");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        let f = layout_block(&doc, body, &styles, 800.0);
        let p = &f.children[0];
        let b_node = find_by_tag(&doc, doc.root(), "b").unwrap();

        let line = &p.children[0];
        let bold_word = line
            .children
            .iter()
            .find(|w| w.kind == FragmentKind::Text("bold".to_string()))
            .unwrap();
        assert_eq!(
            bold_word.node,
            Some(b_node),
            "the word's style node is <b>, not <p>"
        );
    }

    #[test]
    fn empty_element_produces_a_zero_height_block() {
        let f = body_fragment("<div></div>", "", 800.0);
        assert_eq!(f.children[0].height, 0.0);
    }

    #[test]
    fn explicit_height_overrides_intrinsic_content_height() {
        let f = body_fragment("<div>hi</div>", "div { height: 500px; }", 800.0);
        assert_eq!(f.children[0].height, 500.0);
    }

    #[test]
    fn missing_style_falls_back_to_sane_defaults_without_panicking() {
        let mut doc = Document::new();
        let root = doc.root();
        let div = doc.create_node(NodeData::Element {
            tag_name: "div".to_string(),
            attributes: vec![],
        });
        doc.append_child(root, div);
        let styles: StyleMap = HashMap::new();
        let f = layout_block(&doc, div, &styles, 800.0);
        assert_eq!(f.width, 800.0);
    }

    #[test]
    fn whitespace_only_text_between_block_siblings_contributes_no_phantom_line() {
        let f = body_fragment("<div>a</div>\n  <div>b</div>", "", 800.0);
        assert_eq!(
            f.children.len(),
            2,
            "no extra Line fragment from the whitespace text node"
        );
    }

    #[test]
    fn line_height_number_multiplies_font_size() {
        let f = body_fragment("<p>hi</p>", "p { line-height: 2; }", 800.0);
        let line = &f.children[0].children[0];
        assert_eq!(line.height, 32.0);
    }

    // ---- margin collapsing, auto margins, min/max, units, positioning ----

    /// The body's fragment with `body { overflow: hidden }` so the body is a
    /// block formatting context and its children's margins stay measurable.
    fn contained(html: &str, css: &str) -> Fragment {
        body_fragment(html, &format!("body {{ overflow: hidden; }} {css}"), 800.0)
    }

    fn child_y(f: &Fragment) -> Vec<f64> {
        f.children.iter().map(|c| c.y).collect()
    }

    #[test]
    fn adjacent_sibling_margins_collapse_to_the_larger() {
        let f = contained(
            "<div>a</div><div>b</div>",
            "div { height: 20px; margin-top: 10px; margin-bottom: 10px; }",
        );
        assert_eq!(
            child_y(&f),
            vec![10.0, 40.0],
            "10 + 20 + max(10,10) + ... not 10+20+10+10"
        );
        let f = contained(
            "<div>a</div><div>b</div>",
            "div { height: 20px; } div:first-child { margin-bottom: 10px; } div + div { margin-top: 30px; }",
        );
        assert_eq!(
            child_y(&f),
            vec![0.0, 50.0],
            "the 30px margin wins over the 10px one"
        );
    }

    #[test]
    fn negative_margins_collapse_by_summing_the_extremes() {
        let f = contained(
            "<div>a</div><div>b</div>",
            "div { height: 20px; } div:first-child { margin-bottom: 20px; } div + div { margin-top: -5px; }",
        );
        assert_eq!(
            child_y(&f)[1],
            35.0,
            "20 + (-5) = 15 gap after the 20px box"
        );
        let f = contained(
            "<div>a</div><div>b</div>",
            "div { height: 20px; } div:first-child { margin-bottom: -5px; } div + div { margin-top: -8px; }",
        );
        assert_eq!(
            child_y(&f)[1],
            12.0,
            "two negatives collapse to the more negative"
        );
    }

    #[test]
    fn a_first_childs_margin_collapses_through_its_parent_unless_padding_blocks_it() {
        let f = contained(
            "<div class=o><div class=i>x</div></div>",
            ".i { margin-top: 20px; height: 10px; }",
        );
        assert_eq!(
            f.children[0].y, 20.0,
            "the parent is pushed down by the child's margin"
        );
        assert_eq!(
            f.children[0].children[0].y, 0.0,
            "and the child sits at the parent's top"
        );
        let f = contained(
            "<div class=o><div class=i>x</div></div>",
            ".o { padding-top: 1px; } .i { margin-top: 20px; height: 10px; }",
        );
        assert_eq!(f.children[0].y, 0.0);
        assert_eq!(
            f.children[0].children[0].y, 21.0,
            "padding keeps the margin inside"
        );
        let f = contained(
            "<div class=o><div class=i>x</div></div>",
            ".o { overflow: hidden; } .i { margin-top: 20px; height: 10px; }",
        );
        assert_eq!(
            f.children[0].children[0].y, 20.0,
            "a BFC root does not collapse with its child"
        );
    }

    #[test]
    fn a_last_childs_bottom_margin_collapses_through_an_auto_height_parent() {
        let f = contained(
            "<div class=o><div class=i>x</div></div><div class=n>y</div>",
            ".i { height: 10px; margin-bottom: 20px; } .n { height: 5px; }",
        );
        assert_eq!(
            f.children[0].height, 10.0,
            "the margin is not part of the parent's height"
        );
        assert_eq!(
            f.children[1].y, 30.0,
            "it separates the parent from the next sibling"
        );
        let f = contained(
            "<div class=o><div class=i>x</div></div><div class=n>y</div>",
            ".o { height: 40px; } .i { height: 10px; margin-bottom: 20px; } .n { height: 5px; }",
        );
        assert_eq!(
            f.children[1].y, 40.0,
            "a fixed height keeps the margin inside"
        );
    }

    #[test]
    fn empty_blocks_let_margins_collapse_through_them() {
        let f = contained(
            "<div class=a></div><div class=e></div><div class=b></div>",
            ".a { height: 10px; margin-bottom: 10px; } .e { margin-top: 30px; margin-bottom: 5px; } .b { height: 10px; margin-top: 20px; }",
        );
        assert_eq!(f.children[2].y, 40.0, "10 + max(10,30,5,20) = 40");
    }

    #[test]
    fn auto_side_margins_centre_or_push_a_box() {
        let f = contained("<div>x</div>", "div { width: 200px; margin: 0 auto; }");
        assert_eq!(f.children[0].x, 300.0);
        let f = contained("<div>x</div>", "div { width: 200px; margin-left: auto; }");
        assert_eq!(f.children[0].x, 600.0);
        let f = contained("<div>x</div>", "div { width: 200px; margin-right: auto; }");
        assert_eq!(f.children[0].x, 0.0);
        let f = contained("<div>x</div>", "div { margin: 0 auto; }");
        assert_eq!(
            (f.children[0].x, f.children[0].width),
            (0.0, 800.0),
            "auto width fills"
        );
        let f = contained("<div>x</div>", "div { max-width: 300px; margin: 0 auto; }");
        assert_eq!((f.children[0].x, f.children[0].width), (250.0, 300.0));
    }

    #[test]
    fn min_and_max_sizes_clamp_the_used_size() {
        let w = |css: &str| contained("<div>x</div>", css).children[0].width;
        assert_eq!(w("div { width: 50%; max-width: 300px; }"), 300.0);
        assert_eq!(w("div { width: 100px; min-width: 500px; }"), 500.0);
        assert_eq!(
            w("div { max-width: 100px; min-width: 200px; }"),
            200.0,
            "min beats max"
        );
        assert_eq!(
            w("div { width: 100px; padding: 10px; max-width: 100px; box-sizing: border-box; }"),
            100.0
        );
        let h = |css: &str| contained("<div>x</div>", css).children[0].height;
        assert_eq!(h("div { height: 10px; min-height: 40px; }"), 40.0);
        assert_eq!(h("div { height: 100px; max-height: 40px; }"), 40.0);
        assert_eq!(h("div { min-height: 100vh; }"), 720.0);
        assert_eq!(
            h("div { min-height: 50%; }"),
            19.2,
            "a percentage min-height with an auto-height parent is ignored"
        );
    }

    #[test]
    fn rem_viewport_units_and_calc_are_lengths_everywhere() {
        let f = contained(
            "<div>x</div>",
            "div { width: calc(100% - 2rem); margin-top: 1rem; height: 10vh; }",
        );
        let div = &f.children[0];
        assert_eq!(div.width, 768.0);
        assert_eq!(div.y, 16.0);
        assert_eq!(div.height, 72.0, "10vh of the 720px test viewport");
        let f = contained(
            "<div>x</div>",
            "div { width: 50vw; padding: 1em; font-size: 10px; }",
        );
        assert_eq!(f.children[0].width, 400.0 + 20.0);
    }

    #[test]
    fn percentage_heights_need_a_definite_parent_height() {
        let f = contained(
            "<div class=o><div class=i>x</div></div>",
            ".o { height: 200px; } .i { height: 50%; }",
        );
        assert_eq!(f.children[0].children[0].height, 100.0);
        let f = contained(
            "<div class=o><div class=i>x</div></div>",
            ".i { height: 50%; }",
        );
        assert_eq!(
            f.children[0].children[0].height, 19.2,
            "auto parent: height behaves as auto"
        );
    }

    #[test]
    fn border_styles_decide_whether_a_border_takes_space() {
        let f = contained("<div>x</div>", "div { border: 4px solid red; }");
        assert_eq!(f.children[0].width, 800.0);
        assert_eq!(f.children[0].height, 19.2 + 8.0);
        let f = contained(
            "<div>x</div>",
            "div { border-width: 4px; border-style: none; }",
        );
        assert_eq!(f.children[0].height, 19.2, "no style, no border");
    }

    #[test]
    fn relative_positioning_offsets_the_box_without_moving_siblings() {
        let f = contained(
            "<div class=a>a</div><div class=b>b</div>",
            ".a { position: relative; top: 5px; left: 7px; height: 10px; } .b { height: 10px; }",
        );
        assert_eq!((f.children[0].x, f.children[0].y), (7.0, 5.0));
        assert_eq!(
            f.children[1].y, 10.0,
            "the sibling is where it would have been"
        );
        assert!(f.children[0].positioned && !f.children[1].positioned);
        let f = contained(
            "<div class=a>a</div>",
            ".a { position: relative; bottom: 3px; right: 4px; top: auto; left: auto; height: 10px; }",
        );
        assert_eq!(
            (f.children[0].x, f.children[0].y),
            (-4.0, -3.0),
            "right/bottom push the other way"
        );
    }

    #[test]
    fn absolute_boxes_are_placed_against_the_positioned_ancestor() {
        let f = contained(
            "<div class=c><div class=x>x</div></div><div class=after>y</div>",
            ".c { position: relative; width: 200px; height: 100px; margin: 7px; } \
             .x { position: absolute; top: 10px; left: 20px; width: 50px; height: 30px; } \
             .after { height: 5px; }",
        );
        let container = &f.children[0];
        let abs = container.children.last().unwrap();
        assert_eq!(
            (abs.x, abs.y, abs.width, abs.height),
            (20.0, 10.0, 50.0, 30.0)
        );
        assert!(abs.positioned);
        assert_eq!(
            f.children[1].y,
            7.0 + 100.0 + 7.0,
            "the absolute box took no space"
        );
    }

    #[test]
    fn absolute_boxes_anchor_to_any_edge_and_may_stretch() {
        let place = |css: &str| {
            let f = contained(
                "<div class=c><div class=x>x</div></div>",
                &format!(".c {{ position: relative; width: 200px; height: 100px; padding: 5px; border: 3px solid red; }} .x {{ position: absolute; {css} }}"),
            );
            let abs = f.children[0].children.last().unwrap().clone();
            (abs.x, abs.y, abs.width, abs.height)
        };
        // The containing block is the padding box: inside the 3px border.
        assert_eq!(
            place("right: 0; bottom: 0; width: 50px; height: 30px;"),
            (3.0 + 210.0 - 50.0, 3.0 + 110.0 - 30.0, 50.0, 30.0)
        );
        assert_eq!(
            place("left: 0; right: 0; top: 0; bottom: 0;"),
            (3.0, 3.0, 210.0, 110.0)
        );
        assert_eq!(
            place("left: 10px; width: 50px; height: 5px; margin-left: 4px;"),
            (17.0, 8.0, 50.0, 5.0)
        );
        assert_eq!(
            place("left: 0; right: 0; width: 100px; height: 10px; margin: 0 auto;").0,
            3.0 + 55.0
        );
    }

    #[test]
    fn an_absolute_box_with_no_positioned_ancestor_uses_the_initial_containing_block() {
        let doc = blueice_html::parse("<div style='height:50px'>a</div><p class=x>x</p>");
        let ua = ua_stylesheet();
        let author = blueice_css::parse(
            ".x { position: absolute; right: 0; bottom: 0; width: 100px; height: 10px; }",
        )
        .rules;
        let styles = cascade(&doc, &[(Origin::Ua, &ua), (Origin::Author, &author)]);
        let root = crate::layout(
            &doc,
            doc.root(),
            &styles,
            crate::Constraints {
                available_width: 800.0,
                viewport_height: 600.0,
            },
        );
        let abs = root.children.last().unwrap();
        assert!(abs.positioned, "the box was placed in the root fragment");
        assert_eq!(
            (abs.x, abs.y),
            (700.0, 590.0),
            "against the 800x600 initial containing block"
        );
        let body = &root.children[0];
        assert!(
            body.children.iter().all(|c| !c.positioned),
            "and left the body"
        );
    }

    #[test]
    fn z_index_overflow_and_visibility_are_recorded_on_the_fragment() {
        let f = contained(
            "<div class=a>a</div><div class=b>b</div><div class=c>c</div>",
            ".a { position: relative; z-index: 3; } .b { overflow: hidden; } .c { visibility: hidden; }",
        );
        assert_eq!(f.children[0].z_index, Some(3));
        assert!(f.children[1].clips && !f.children[0].clips);
        assert!(f.children[2].hidden && !f.children[0].hidden);
        let plain = contained("<div class=a>a</div>", ".a { z-index: 3; }");
        assert_eq!(
            plain.children[0].z_index, None,
            "z-index applies to positioned boxes only"
        );
    }

    #[test]
    fn display_contents_replaces_the_element_with_its_children() {
        let f = contained(
            "<div class=w><p>a</p><p>b</p></div>",
            ".w { display: contents; padding: 50px; } p { height: 10px; }",
        );
        assert_eq!(
            f.children.len(),
            2,
            "no wrapper box, and its padding is not applied"
        );
        assert_eq!(child_y(&f), vec![0.0, 10.0]);
    }
}
