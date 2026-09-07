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
//!
//! **CJK fallback** (`phase-14-i18n-localization/PLAN.md`): DejaVu Sans
//! carries essentially no CJK glyph coverage, which a rendered zh-TW
//! page surfaced directly (translated text laid out correctly --
//! `blueice-layout` measured real advance widths -- but painted as
//! blank space, since `fontdue::Font::rasterize` silently draws
//! nothing for a codepoint the font has no glyph for). Bundles Noto
//! Sans TC Regular (SIL Open Font License, `assets/NotoSansTC-
//! LICENSE.txt`) as a second, fallback-only face: [`font_for_char`]
//! checks the requested weight/style's DejaVu face first and only
//! consults the CJK face for a codepoint DejaVu can't cover. Only one
//! CJK weight is bundled -- there is no bold/italic Traditional
//! Chinese face here, so a bold heading's Chinese text still falls
//! back to the single regular-weight CJK face rather than going blank
//! again; this is a deliberate, documented simplification (visually
//! regular-weight CJK inside a bold heading), not a second missing-
//! glyph bug reappearing one level down.

use std::sync::OnceLock;

const FONT_REGULAR: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");
const FONT_ITALIC: &[u8] = include_bytes!("../assets/DejaVuSans-Oblique.ttf");
const FONT_BOLD_ITALIC: &[u8] = include_bytes!("../assets/DejaVuSans-BoldOblique.ttf");
const FONT_CJK_FALLBACK: &[u8] = include_bytes!("../assets/NotoSansTC-Regular.otf");

struct FontSet {
    regular: fontdue::Font,
    bold: fontdue::Font,
    italic: fontdue::Font,
    bold_italic: fontdue::Font,
    cjk_fallback: fontdue::Font,
}

fn fonts() -> &'static FontSet {
    static FONTS: OnceLock<FontSet> = OnceLock::new();
    FONTS.get_or_init(|| {
        let load = |bytes| fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).expect("bundled font must parse");
        FontSet {
            regular: load(FONT_REGULAR),
            bold: load(FONT_BOLD),
            italic: load(FONT_ITALIC),
            bold_italic: load(FONT_BOLD_ITALIC),
            cjk_fallback: load(FONT_CJK_FALLBACK),
        }
    })
}

/// The bundled font matching this bold/italic combination -- ignores
/// which characters will actually be drawn with it; use
/// [`font_for_char`] when a specific character's glyph coverage
/// matters (i.e. whenever the text might not be pure Latin/Cyrillic/
/// Greek, DejaVu Sans's actual coverage).
pub fn font_for(bold: bool, italic: bool) -> &'static fontdue::Font {
    let set = fonts();
    match (bold, italic) {
        (false, false) => &set.regular,
        (true, false) => &set.bold,
        (false, true) => &set.italic,
        (true, true) => &set.bold_italic,
    }
}

/// The font to draw `c` with at this weight/style: the matching DejaVu
/// face if it has a glyph for `c`, otherwise the bundled CJK fallback
/// face (see the module docs for why that fallback ignores
/// bold/italic). Every caller that walks a string character-by-
/// character to measure or rasterize it (`measure_text_width` here,
/// `blueice-raster`'s glyph loop) must resolve the font per character
/// through this function, not once per run with [`font_for`] -- a run
/// can legitimately mix scripts (e.g. an English link inside a
/// Chinese sentence).
pub fn font_for_char(c: char, bold: bool, italic: bool) -> &'static fontdue::Font {
    let primary = font_for(bold, italic);
    if primary.has_glyph(c) {
        return primary;
    }
    &fonts().cjk_fallback
}

/// The real rendered width of `text` at `font_size_px` in the given
/// weight/style -- the sum of each character's advance width from the
/// *same* font [`font_for_char`] would pick to rasterize it with, so a
/// caller measuring with this and a caller rasterizing per character
/// never disagree about how wide a run of text is, even across a
/// script-fallback boundary.
pub fn measure_text_width(text: &str, font_size_px: f64, bold: bool, italic: bool) -> f64 {
    text.chars().map(|c| font_for_char(c, bold, italic).metrics(c, font_size_px as f32).advance_width as f64).sum()
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
    fn dejavu_has_no_glyph_for_a_cjk_character_confirming_the_fallback_is_load_bearing() {
        // pins down *why* font_for_char exists -- if this ever starts
        // failing (a future DejaVu update adds CJK coverage, say), the
        // fallback logic below is still correct, just no longer doing
        // anything for this particular character.
        assert!(!font_for(false, false).has_glyph('關'));
    }

    #[test]
    fn font_for_char_falls_back_to_the_cjk_face_for_a_character_dejavu_lacks() {
        let chosen = font_for_char('關', false, false) as *const _;
        let cjk = &fonts().cjk_fallback as *const _;
        assert_eq!(chosen, cjk);
    }

    #[test]
    fn font_for_char_stays_on_the_primary_face_for_an_ordinary_latin_character() {
        let chosen = font_for_char('a', false, false) as *const _;
        let regular = font_for(false, false) as *const _;
        assert_eq!(chosen, regular);
    }

    #[test]
    fn font_for_char_ignores_bold_italic_when_falling_back_to_cjk() {
        // no bold/italic CJK face is bundled (module docs) -- every
        // weight/style combination must resolve a CJK character to the
        // exact same single fallback face, not go blank for e.g. bold.
        let cjk = &fonts().cjk_fallback as *const _;
        for (bold, italic) in [(true, false), (false, true), (true, true)] {
            assert_eq!(font_for_char('關', bold, italic) as *const _, cjk);
        }
    }

    #[test]
    fn measuring_a_cjk_character_uses_the_fallback_fonts_real_metrics() {
        let expected = fonts().cjk_fallback.metrics('關', 16.0).advance_width as f64;
        assert_eq!(measure_text_width("關", 16.0, false, false), expected);
    }

    #[test]
    fn measuring_mixed_script_text_sums_each_characters_own_font() {
        // a run can legitimately mix scripts (an English word inside a
        // Chinese sentence) -- the sum must not silently use one font
        // for the whole string.
        let mixed = measure_text_width("a關", 16.0, false, false);
        let latin = font_for(false, false).metrics('a', 16.0).advance_width as f64;
        let cjk = fonts().cjk_fallback.metrics('關', 16.0).advance_width as f64;
        assert_eq!(mixed, latin + cjk);
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
