// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Layout: DOM + cascaded styles -> a box/fragment tree.
//!
//! Not yet implemented. See
//! `development/browser_core/research/layout.md` for the target
//! architecture: a persistent DOM-linked box type plus a freshly-built
//! immutable fragment tree per layout pass (Blink LayoutNG-style),
//! preferred over Gecko's mutable frame graph because the latter fights
//! Rust's ownership model. Algorithm selection should be a single
//! `match` on `display`, with block/inline first and flex/grid as later
//! arms rather than a rewrite.

use blueice_css::Stylesheet;
use blueice_dom::{Document, NodeId};

/// The laid-out result for a subtree. Not yet a real type.
pub struct Fragment;

pub fn layout(_doc: &Document, _root: NodeId, _styles: &Stylesheet) -> Fragment {
    todo!("layout -- see research/layout.md")
}
