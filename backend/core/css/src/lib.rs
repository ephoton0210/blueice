// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CSS parsing and cascade.
//!
//! Not yet implemented. See
//! `development/browser_core/research/css-cascade.md` for the target
//! architecture: port Stylo's packed-specificity/cascade-origin design
//! as a lightweight from-scratch implementation rather than vendoring
//! the `style` crate directly (its `TElement` trait alone has 82 methods
//! to implement against a new DOM).

/// A parsed, cascaded stylesheet. Not yet a real type.
pub struct Stylesheet;

pub fn parse(_input: &str) -> Stylesheet {
    todo!("CSS parser/cascade -- see research/css-cascade.md")
}
