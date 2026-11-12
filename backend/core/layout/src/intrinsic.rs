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
        placed.content_extent + pb_h
    };
    let max = measure(UNBOUNDED);
    (measure(0.0).min(max), max)
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
        placed.content_extent + pb_h
    };
    // An explicit pixel width is both preferred widths.
    let explicit =
        style
            .px("width", f64::NAN)
            .map(|w| if style.is_border_box() { w } else { w + pb_h });
    let result = match explicit {
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
