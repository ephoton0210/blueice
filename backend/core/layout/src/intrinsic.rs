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
const UNBOUNDED: f64 = 1.0e6;

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

fn content_widths_uncached(ctx: &Ctx<'_>, node: NodeId) -> (f64, f64) {
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
    let max = measure(UNBOUNDED);
    (measure(0.0).min(max), max)
}

/// `(min-content, max-content)` border-box widths of `node`, margins excluded.
pub(crate) fn preferred_widths(ctx: &Ctx<'_>, node: NodeId) -> (f64, f64) {
    if let Some(known) = ctx.intrinsic.borrow().get(&node) {
        return *known;
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
