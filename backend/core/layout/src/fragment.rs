// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The layout output tree, per `research/layout.md` §4: an immutable,
//! freshly-built value tree (Blink LayoutNG's `PhysicalFragment` role),
//! not a mutable frame graph -- there are no back-pointers, no
//! `Rc`/`RefCell`, just an ordinary owned `Vec<Fragment>` tree rebuilt
//! each layout pass. Each fragment optionally carries the stable
//! `NodeId` of the DOM node it renders (per plan §1, that's what lets
//! paint/hit-testing/the AI representation reference back to the DOM
//! without a live pointer); anonymous boxes (line boxes) carry `None`.

use blueice_dom::NodeId;

#[derive(Debug, Clone, PartialEq)]
pub enum FragmentKind {
    /// A block-level box (block flow, or a block-generating box
    /// standing in for a not-yet-implemented algorithm like flex --
    /// see `phase-2-mvp-scope/PLAN.md`).
    Block,
    /// An anonymous line box wrapping one line's worth of inline
    /// content -- has no DOM node of its own.
    Line,
    /// A run of text within a line, in a single style context.
    Text(String),
}

/// One box in the output tree. `x`/`y` are relative to the parent
/// fragment's content-box origin (i.e. already offset past the
/// parent's own padding/border); `width`/`height` are this fragment's
/// own border-box size. Margins are not part of a fragment's box at
/// all -- they're pure spacing the parent accounts for when placing a
/// child, matching how `research/layout.md` describes keeping the
/// fragment tree a plain value tree with no extra bookkeeping fields
/// for something that's purely a positioning input.
#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    pub node: Option<NodeId>,
    pub kind: FragmentKind,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub children: Vec<Fragment>,
}

impl Fragment {
    pub fn empty_block() -> Self {
        Fragment { node: None, kind: FragmentKind::Block, x: 0.0, y: 0.0, width: 0.0, height: 0.0, children: Vec::new() }
    }
}

/// Layout's single input constraint for the MVP algorithm --
/// `research/layout.md` §4 calls a generic constraints-in/fragment-out
/// contract the highest-leverage architectural decision available
/// (it's what let flex/grid/table plug into LayoutNG without touching
/// the block algorithm), so this is deliberately a struct with room to
/// grow (block-size constraints, writing mode, ...) rather than a bare
/// `f64` parameter, even though only `available_width` exists yet.
#[derive(Debug, Clone, Copy)]
pub struct Constraints {
    pub available_width: f64,
}
