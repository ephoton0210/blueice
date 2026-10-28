// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Text measurement and greedy line-breaking for inline content.
//!
//! Word widths are measured with `blueice-font`'s real, bundled DejaVu
//! Sans metrics -- the same font `blueice-raster` rasterizes with, so
//! layout's line-breaking geometry and the eventual rendered pixels
//! agree about how wide a word is. This used to be a flat
//! `font_size_px * 0.6`-per-character approximation with no font data
//! at all; that approximation visibly undershot real (especially bold)
//! glyph widths once actual rendering existed to check it against --
//! e.g. an `<h1>` heading's words overlapping when viewed in the
//! reference frontend -- so it was replaced rather than re-tuned again.
//! Real ascent/descent-aware baseline placement and shaping (kerning,
//! ligatures, bidi/script segmentation) are still out of scope; only
//! per-character advance-width summing is real.
//!
//! Line-breaking itself is word-level greedy fill-then-wrap (accumulate
//! words onto a line until the next one would overflow, then start a
//! new line) -- `research/layout.md` §4's "naive algorithm", with no
//! hyphenation, no mid-word breaking on overflow, and no bidi/script
//! segmentation.

use blueice_css::ComputedStyle;
use blueice_dom::NodeId;
use std::collections::HashMap;

pub type StyleMap = HashMap<NodeId, ComputedStyle>;

/// The initial `line-height: normal` value real browsers use absent an
/// explicit one: roughly 1.2x the font size.
pub fn default_line_height(font_size_px: f64) -> f64 {
    font_size_px * 1.2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_line_height_is_larger_than_font_size() {
        assert!(default_line_height(16.0) > 16.0);
    }
}
