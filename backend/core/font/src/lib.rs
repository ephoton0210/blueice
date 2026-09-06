// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The one place BlueIce loads and measures its bundled fonts --
//! shared by `blueice-layout` (line-breaking needs real word widths)
//! and `blueice-raster` (rasterizing needs the same font data those
//! widths were measured against). Splitting this out of `blueice-
//! raster` fixes a real bug found by actually looking at a rendered
//! page (`https://example.com` through the full `core`/`frontend`
//! pipeline): layout used to measure text with a flat `font_size_px *
//! 0.6`-per-character approximation while raster rendered with real
//! DejaVu Sans glyphs, so bold headings (real glyphs measurably wider
//! than the approximation, `research/layout.md`'s text.rs module
//! previously documented this gap explicitly) visually overlapped the
//! next word -- two different, disagreeing notions of "how wide is
//! this text" for the same text. There is now exactly one: whatever
//! `measure_text_width` returns is exactly what `font_for(..)
//! .rasterize(..)`'s cumulative advance will draw, because both call
//! into the same loaded `fontdue::Font`.
//!
//! Bundles the DejaVu Sans family (Regular/Bold/Oblique/BoldOblique,
//! Bitstream Vera-derived permissive license, `assets/DejaVuSans-
//! LICENSE.txt`) -- credited on the Help/About/Credits screen
//! alongside Gecko/Chromium per Phase 0/4's BSD-3-Clause obligation.

use std::sync::OnceLock;

const FONT_REGULAR: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");
const FONT_ITALIC: &[u8] = include_bytes!("../assets/DejaVuSans-Oblique.ttf");
const FONT_BOLD_ITALIC: &[u8] = include_bytes!("../assets/DejaVuSans-BoldOblique.ttf");

struct FontSet {
    regular: fontdue::Font,
    bold: fontdue::Font,
    italic: fontdue::Font,
    bold_italic: fontdue::Font,
}

fn fonts() -> &'static FontSet {
    static FONTS: OnceLock<FontSet> = OnceLock::new();
    FONTS.get_or_init(|| {
        let load = |bytes| fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).expect("bundled font must parse");
        FontSet { regular: load(FONT_REGULAR), bold: load(FONT_BOLD), italic: load(FONT_ITALIC), bold_italic: load(FONT_BOLD_ITALIC) }
    })
}

/// The bundled font matching this bold/italic combination.
pub fn font_for(bold: bool, italic: bool) -> &'static fontdue::Font {
    let set = fonts();
    match (bold, italic) {
        (false, false) => &set.regular,
        (true, false) => &set.bold,
        (false, true) => &set.italic,
        (true, true) => &set.bold_italic,
    }
}

/// The real rendered width of `text` at `font_size_px` in the given
/// weight/style -- the sum of each character's advance width from the
/// *same* font `font_for` returns, so a caller measuring with this and
/// a caller rasterizing with `font_for(..).rasterize(..)` never
/// disagree about how wide a run of text is.
pub fn measure_text_width(text: &str, font_size_px: f64, bold: bool, italic: bool) -> f64 {
    let font = font_for(bold, italic);
    text.chars().map(|c| font.metrics(c, font_size_px as f32).advance_width as f64).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_measures_to_zero() {
        assert_eq!(measure_text_width("", 16.0, false, false), 0.0);
    }

    #[test]
    fn longer_text_measures_wider() {
        assert!(measure_text_width("hello", 16.0, false, false) > measure_text_width("hi", 16.0, false, false));
    }

    #[test]
    fn larger_font_size_measures_wider_for_the_same_text() {
        assert!(measure_text_width("hello", 32.0, false, false) > measure_text_width("hello", 16.0, false, false));
    }

    #[test]
    fn bold_measures_at_least_as_wide_as_regular_for_the_same_text() {
        // real font data, not an assumption: DejaVu Sans Bold's glyphs
        // are never narrower than Regular's for ordinary latin text.
        assert!(measure_text_width("Example Domain", 32.0, true, false) >= measure_text_width("Example Domain", 32.0, false, false));
    }

    #[test]
    fn measured_width_matches_summing_font_for_metrics_directly() {
        // pins down the exact "measure and rasterize must agree"
        // property the module docs describe: this function must not
        // silently diverge from `font_for(..).metrics(..)`.
        let expected: f64 = "abc".chars().map(|c| font_for(false, false).metrics(c, 16.0).advance_width as f64).sum();
        assert_eq!(measure_text_width("abc", 16.0, false, false), expected);
    }

    #[test]
    fn font_for_selects_the_matching_weight_and_style() {
        // distinct font files -- the four combinations must not all
        // silently resolve to the same loaded font.
        let regular = font_for(false, false) as *const _;
        let bold = font_for(true, false) as *const _;
        let italic = font_for(false, true) as *const _;
        let bold_italic = font_for(true, true) as *const _;
        assert_ne!(regular, bold);
        assert_ne!(regular, italic);
        assert_ne!(regular, bold_italic);
        assert_ne!(bold, italic);
        assert_ne!(bold, bold_italic);
        assert_ne!(italic, bold_italic);
    }
}
