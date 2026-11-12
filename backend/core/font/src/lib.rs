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
//! Each bundled face is parsed on its first use and shared thereafter. Latin
//! text does not initialize unrelated weights or the CJK fallback; mixed text
//! initializes the fallback only when its primary face lacks a glyph.
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

pub mod web;
mod opentype;
mod shape;
mod variable;
pub use opentype::{
    char_advances, is_complex, register_features, shape_for_drawing, shape_text, starts_rtl,
    vertical_cell, vertical_glyph, DrawGlyph, FeaturesId, VerticalCell,
};
pub use shape::{is_mark, mirrored, shape_with, vertical_form, visual_order};
pub use variable::{Axis, VarInstance, Variation};

use std::sync::OnceLock;

const FONT_REGULAR: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");
const FONT_ITALIC: &[u8] = include_bytes!("../assets/DejaVuSans-Oblique.ttf");
const FONT_BOLD_ITALIC: &[u8] = include_bytes!("../assets/DejaVuSans-BoldOblique.ttf");
const FONT_CJK_FALLBACK: &[u8] = include_bytes!("../assets/NotoSansTC-Regular.otf");

struct FontSet {
    regular: OnceLock<fontdue::Font>,
    bold: OnceLock<fontdue::Font>,
    italic: OnceLock<fontdue::Font>,
    bold_italic: OnceLock<fontdue::Font>,
    cjk_fallback: OnceLock<fontdue::Font>,
}

fn fonts() -> &'static FontSet {
    static FONTS: FontSet = FontSet {
        regular: OnceLock::new(),
        bold: OnceLock::new(),
        italic: OnceLock::new(),
        bold_italic: OnceLock::new(),
        cjk_fallback: OnceLock::new(),
    };
    &FONTS
}

fn load_font(
    slot: &'static OnceLock<fontdue::Font>,
    bytes: &'static [u8],
) -> &'static fontdue::Font {
    slot.get_or_init(|| {
        #[cfg(test)]
        cold_start::record_parsed_font(bytes);
        fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .expect("bundled font must parse")
    })
}

fn cjk_fallback() -> &'static fontdue::Font {
    load_font(&fonts().cjk_fallback, FONT_CJK_FALLBACK)
}

/// The bundled font matching this bold/italic combination -- ignores
/// which characters will actually be drawn with it; use
/// [`font_for_char`] when a specific character's glyph coverage
/// matters (i.e. whenever the text might not be pure Latin/Cyrillic/
/// Greek, DejaVu Sans's actual coverage).
/// The file of a font BlueIce itself carries, named the way a stylesheet's `local()` names an
/// installed font (the full name or the PostScript name, in any case). The engine has no other
/// fonts to offer a page.
pub fn bundled_font_bytes(name: &str) -> Option<&'static [u8]> {
    let key: String = name
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect();
    Some(match key.as_str() {
        "dejavusans" | "dejavusansbook" | "dejavusansregular" => FONT_REGULAR,
        "dejavusansbold" => FONT_BOLD,
        "dejavusansoblique" | "dejavusansitalic" => FONT_ITALIC,
        "dejavusansboldoblique" | "dejavusansbolditalic" => FONT_BOLD_ITALIC,
        "notosanstc" | "notosanstcregular" => FONT_CJK_FALLBACK,
        _ => return None,
    })
}

/// The file of a bundled font, by the font that draws it (for fonts already loaded).
pub(crate) fn bundled_bytes_of(font: &'static fontdue::Font) -> Option<&'static [u8]> {
    let set = fonts();
    [
        (&set.regular, FONT_REGULAR),
        (&set.bold, FONT_BOLD),
        (&set.italic, FONT_ITALIC),
        (&set.bold_italic, FONT_BOLD_ITALIC),
        (&set.cjk_fallback, FONT_CJK_FALLBACK),
    ]
    .into_iter()
    .find(|(slot, _)| slot.get().is_some_and(|f| std::ptr::eq(f, font)))
    .map(|(_, bytes)| bytes)
}

pub fn font_for(bold: bool, italic: bool) -> &'static fontdue::Font {
    let set = fonts();
    let (slot, bytes) = match (bold, italic) {
        (false, false) => (&set.regular, FONT_REGULAR),
        (true, false) => (&set.bold, FONT_BOLD),
        (false, true) => (&set.italic, FONT_ITALIC),
        (true, true) => (&set.bold_italic, FONT_BOLD_ITALIC),
    };
    load_font(slot, bytes)
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
    cjk_fallback()
}

/// A face that draws characters: a static font, or a variable one at a weight. Copyable, and
/// the same face is the same [`address`](Glyphs::address).
#[derive(Clone, Copy)]
pub enum Glyphs {
    Static(&'static fontdue::Font),
    Variable(&'static VarInstance),
}

impl Glyphs {
    pub fn has_glyph(self, c: char) -> bool {
        match self {
            Glyphs::Static(f) => f.has_glyph(c),
            Glyphs::Variable(v) => v.has_glyph(c),
        }
    }

    pub fn metrics(self, c: char, px: f32) -> fontdue::Metrics {
        match self {
            Glyphs::Static(f) => f.metrics(c, px),
            Glyphs::Variable(v) => v.metrics(c, px),
        }
    }

    /// The glyph with this index in the face, drawn as [`rasterize`](Glyphs::rasterize) draws a
    /// character (for glyphs a shaper chose).
    pub fn rasterize_indexed(self, id: u16, px: f32) -> (fontdue::Metrics, Vec<u8>) {
        match self {
            Glyphs::Static(f) => f.rasterize_indexed(id, px),
            Glyphs::Variable(v) => v.rasterize_indexed(id, px),
        }
    }

    pub fn rasterize(self, c: char, px: f32) -> (fontdue::Metrics, Vec<u8>) {
        match self {
            Glyphs::Static(f) => f.rasterize(c, px),
            Glyphs::Variable(v) => v.rasterize(c, px),
        }
    }

    pub fn horizontal_line_metrics(self, px: f32) -> Option<fontdue::LineMetrics> {
        match self {
            Glyphs::Static(f) => f.horizontal_line_metrics(px),
            Glyphs::Variable(v) => v.line_metrics(px),
        }
    }

    /// An identity for comparing faces.
    pub fn address(self) -> usize {
        match self {
            Glyphs::Static(f) => std::ptr::from_ref(f) as usize,
            Glyphs::Variable(v) => std::ptr::from_ref(v) as usize,
        }
    }
}

/// [`font_for_char`] with a page's own face tried first: `face` if it has a glyph for `c`,
/// otherwise whatever the bundled fonts would use.
pub fn font_for_char_in(
    c: char,
    bold: bool,
    italic: bool,
    face: Option<web::WebFontId>,
) -> Glyphs {
    if let Some(font) = face.and_then(|id| web::font_for(id, c)) {
        return font;
    }
    Glyphs::Static(font_for_char(c, bold, italic))
}

/// [`measure_text_width`] for text drawn with a page's own face (`None`: the bundled fonts).
pub fn measure_text_width_in(
    text: &str,
    font_size_px: f64,
    bold: bool,
    italic: bool,
    face: Option<web::WebFontId>,
) -> f64 {
    measure_text_width_features(text, font_size_px, bold, italic, face, FeaturesId::default())
}

/// [`measure_text_width_in`] with the OpenType features the text asks for. Text is as wide as the
/// glyphs the font's own layout tables make of it: kerning, ligatures, joining forms.
pub fn measure_text_width_features(
    text: &str,
    font_size_px: f64,
    bold: bool,
    italic: bool,
    face: Option<web::WebFontId>,
    features: FeaturesId,
) -> f64 {
    if text.is_empty() {
        return 0.0;
    }
    type Key = (String, u32, bool, bool, Option<u32>, u32);
    static WIDTHS: std::sync::Mutex<Option<std::collections::HashMap<Key, f64>>> =
        std::sync::Mutex::new(None);
    // Widths are asked for again and again as a page is laid out; shaping is not free.
    const MAX_CACHED: usize = 32_768;
    let key: Key = (
        text.to_string(),
        (font_size_px as f32).to_bits(),
        bold,
        italic,
        face.map(web::WebFontId::as_u32),
        features.as_u32(),
    );
    if let Some(known) = WIDTHS.lock().ok().and_then(|m| m.as_ref().and_then(|m| m.get(&key).copied())) {
        return known;
    }
    let width = measure_uncached(text, font_size_px, bold, italic, face, features);
    if let Ok(mut cache) = WIDTHS.lock() {
        let map = cache.get_or_insert_with(Default::default);
        if map.len() >= MAX_CACHED {
            map.clear();
        }
        map.insert(key, width);
    }
    width
}

fn measure_uncached(
    text: &str,
    font_size_px: f64,
    bold: bool,
    italic: bool,
    face: Option<web::WebFontId>,
    features: FeaturesId,
) -> f64 {
    if let Some(glyphs) = shape_text(
        text,
        bold,
        italic,
        face,
        features,
        font_size_px as f32,
        starts_rtl(text),
    ) {
        return glyphs.iter().map(|g| f64::from(g.advance)).sum();
    }
    shape_in(text, bold, italic, face)
        .chars()
        .map(|c| {
            font_for_char_in(c, bold, italic, face)
                .metrics(c, font_size_px as f32)
                .advance_width as f64
        })
        .sum()
}

/// `text` as it is drawn and measured: Arabic letters in their joining forms (see
/// [`shape_with`]), each form used only when the face that would draw it has the glyph.
pub fn shape_in(
    text: &str,
    bold: bool,
    italic: bool,
    face: Option<web::WebFontId>,
) -> std::borrow::Cow<'_, str> {
    shape_with(text, |c| font_for_char_in(c, bold, italic, face).has_glyph(c))
}

/// [`ascent_descent`] of a page's own face (`None`: the bundled fonts).
pub fn ascent_descent_in(
    font_size_px: f64,
    bold: bool,
    italic: bool,
    face: Option<web::WebFontId>,
) -> (f64, f64) {
    match face.and_then(web::get) {
        Some(font) => match font.horizontal_line_metrics(font_size_px as f32) {
            Some(metrics) => (f64::from(metrics.ascent), f64::from(-metrics.descent)),
            None => (font_size_px * 0.93, font_size_px * 0.24),
        },
        None => ascent_descent(font_size_px, bold, italic),
    }
}

/// The real rendered width of `text` at `font_size_px` in the given
/// weight/style -- the sum of each character's advance width from the
/// *same* font [`font_for_char`] would pick to rasterize it with, so a
/// caller measuring with this and a caller rasterizing per character
/// never disagree about how wide a run of text is, even across a
/// script-fallback boundary.
pub fn measure_text_width(text: &str, font_size_px: f64, bold: bool, italic: bool) -> f64 {
    measure_text_width_in(text, font_size_px, bold, italic, None)
}

/// The bundled font's ascent and descent (both positive, in pixels) at
/// `font_size_px`: how far text of that size reaches above and below its
/// baseline. Line boxes use these to place text on a shared baseline.
pub fn ascent_descent(font_size_px: f64, bold: bool, italic: bool) -> (f64, f64) {
    match font_for(bold, italic).horizontal_line_metrics(font_size_px as f32) {
        Some(metrics) => (f64::from(metrics.ascent), f64::from(-metrics.descent)),
        None => (font_size_px * 0.93, font_size_px * 0.24),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascent_and_descent_are_positive_and_scale_with_the_size() {
        let (ascent, descent) = ascent_descent(16.0, false, false);
        assert!(ascent > 10.0 && ascent < 20.0, "{ascent}");
        assert!(descent > 2.0 && descent < 8.0, "{descent}");
        let (double_ascent, double_descent) = ascent_descent(32.0, false, false);
        assert!((double_ascent - 2.0 * ascent).abs() < 0.01);
        assert!((double_descent - 2.0 * descent).abs() < 0.01);
    }

    #[test]
    fn empty_text_measures_to_zero() {
        assert_eq!(measure_text_width("", 16.0, false, false), 0.0);
    }

    #[test]
    fn longer_text_measures_wider() {
        assert!(
            measure_text_width("hello", 16.0, false, false)
                > measure_text_width("hi", 16.0, false, false)
        );
    }

    #[test]
    fn larger_font_size_measures_wider_for_the_same_text() {
        assert!(
            measure_text_width("hello", 32.0, false, false)
                > measure_text_width("hello", 16.0, false, false)
        );
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
        let cjk = cjk_fallback() as *const _;
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
        let cjk = cjk_fallback() as *const _;
        for (bold, italic) in [(true, false), (false, true), (true, true)] {
            assert_eq!(font_for_char('關', bold, italic) as *const _, cjk);
        }
    }

    #[test]
    fn measuring_a_cjk_character_uses_the_fallback_fonts_real_metrics() {
        let expected = cjk_fallback().metrics('關', 16.0).advance_width as f64;
        assert_eq!(measure_text_width("關", 16.0, false, false), expected);
    }

    #[test]
    fn measuring_mixed_script_text_sums_each_characters_own_font() {
        // a run can legitimately mix scripts (an English word inside a
        // Chinese sentence) -- the sum must not silently use one font
        // for the whole string.
        let mixed = measure_text_width("a關", 16.0, false, false);
        let latin = font_for(false, false).metrics('a', 16.0).advance_width as f64;
        let cjk = cjk_fallback().metrics('關', 16.0).advance_width as f64;
        assert_eq!(mixed, latin + cjk);
    }

    #[test]
    fn bold_measures_at_least_as_wide_as_regular_for_the_same_text() {
        // real font data, not an assumption: DejaVu Sans Bold's glyphs
        // are never narrower than Regular's for ordinary latin text.
        assert!(
            measure_text_width("Example Domain", 32.0, true, false)
                >= measure_text_width("Example Domain", 32.0, false, false)
        );
    }

    #[test]
    fn measured_width_matches_summing_font_for_metrics_directly() {
        // pins down the exact "measure and rasterize must agree"
        // property the module docs describe: this function must not
        // silently diverge from `font_for(..).metrics(..)`.
        let expected: f64 = "abc"
            .chars()
            .map(|c| font_for(false, false).metrics(c, 16.0).advance_width as f64)
            .sum();
        assert_eq!(measure_text_width("abc", 16.0, false, false), expected);
    }

    #[test]
    fn latin_text_is_kerned_unless_the_text_turns_kerning_off() {
        let width = |text: &str, features: FeaturesId| {
            measure_text_width_features(text, 32.0, false, false, None, features)
        };
        let separate = width("A", FeaturesId::default()) + width("V", FeaturesId::default());
        let kerned = width("AV", FeaturesId::default());
        assert!(kerned < separate - 0.5, "the pair is tucked together: {kerned} vs {separate}");
        let off = register_features(&[(*b"kern", 0)]);
        assert!((width("AV", off) - separate).abs() < 0.01, "kerning off is the sum of the letters");
        // the same set registered again is the same set
        assert_eq!(register_features(&[(*b"kern", 0)]), off);
        assert_eq!(register_features(&[]), FeaturesId::default());
    }

    #[test]
    fn arabic_is_measured_in_its_joined_forms() {
        // beh beh: initial then final, not two isolated letters
        let joined = measure_text_width("\u{0628}\u{0628}", 16.0, false, false);
        let face = font_for(false, false);
        let forms: f64 = ['\u{FE91}', '\u{FE90}']
            .iter()
            .map(|c| face.metrics(*c, 16.0).advance_width as f64)
            .sum();
        assert_eq!(joined, forms);
        let isolated: f64 = face.metrics('\u{0628}', 16.0).advance_width as f64 * 2.0;
        assert_ne!(joined, isolated);
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

#[cfg(test)]
#[path = "../tests/cold_start/mod.rs"]
mod cold_start;
