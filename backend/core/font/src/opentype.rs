// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! OpenType layout for the scripts that need it: a run of Arabic or Hebrew text is shaped with
//! the font's own `GSUB` and `GPOS` tables (joining forms, ligatures, kerning, marks placed on
//! their letters), and an upright character of a vertical line is looked up in the font's
//! `vert`/`vrt2` substitutions. The shaping is `rustybuzz`'s, a port of HarfBuzz.
//!
//! Text that is not in those scripts keeps the per-character measuring and drawing everything
//! else uses, so Latin text does not change by a pixel. A face a page brings as a variable font
//! is not shaped here (its advances come from the instance being drawn); such text falls back to
//! the joining forms of [`crate::shape_with`].

use crate::{font_for_char_in, web::WebFontId, Glyphs};
use rustybuzz::{Direction, Face, UnicodeBuffer};
use std::collections::HashMap;
use std::sync::Mutex;

/// A set of OpenType feature settings a box's text asks for (`font-kerning`, `font-variant-*`,
/// `font-feature-settings`), registered once and named by a small number the paint list can carry.
/// The default is no settings: the font's own defaults (kerning and the common ligatures on).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct FeaturesId(u32);

impl FeaturesId {
    pub fn as_u32(self) -> u32 {
        self.0
    }

    pub fn from_u32(id: u32) -> Self {
        FeaturesId(id)
    }
}

type Setting = ([u8; 4], u32);

#[derive(Default)]
struct FeatureSets {
    sets: Vec<Vec<Setting>>,
    by_content: HashMap<Vec<Setting>, u32>,
}

/// Feature sets registered over the life of the process.
const MAX_FEATURE_SETS: usize = 4096;

fn feature_sets() -> &'static Mutex<FeatureSets> {
    static SETS: std::sync::OnceLock<Mutex<FeatureSets>> = std::sync::OnceLock::new();
    SETS.get_or_init(|| Mutex::new(FeatureSets::default()))
}

/// Registers `settings` (tag and value, a later setting of a tag winning) and names them. No
/// settings, or past the bound, is the default.
pub fn register_features(settings: &[Setting]) -> FeaturesId {
    if settings.is_empty() {
        return FeaturesId::default();
    }
    let Ok(mut known) = feature_sets().lock() else {
        return FeaturesId::default();
    };
    if let Some(&id) = known.by_content.get(settings) {
        return FeaturesId(id);
    }
    if known.sets.len() >= MAX_FEATURE_SETS {
        return FeaturesId::default();
    }
    // Id 0 is the default, so ids start at 1.
    let id = known.sets.len() as u32 + 1;
    known.sets.push(settings.to_vec());
    known.by_content.insert(settings.to_vec(), id);
    FeaturesId(id)
}

fn features_of(id: FeaturesId) -> Vec<rustybuzz::Feature> {
    if id.0 == 0 {
        return Vec::new();
    }
    let Ok(known) = feature_sets().lock() else {
        return Vec::new();
    };
    known
        .sets
        .get(id.0 as usize - 1)
        .map(|set| {
            set.iter()
                .map(|(tag, value)| {
                    rustybuzz::Feature::new(rustybuzz::ttf_parser::Tag::from_bytes(tag), *value, ..)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One glyph to draw, in the order it is drawn left to right.
#[derive(Clone, Copy)]
pub struct DrawGlyph {
    /// The face that has it.
    pub font: Glyphs,
    /// Its index in that font.
    pub id: u16,
    /// How far the pen moves after it, in pixels.
    pub advance: f32,
    /// Where it is drawn from the pen: right, and up.
    pub dx: f32,
    pub dy: f32,
}

/// Whether `c` is in a script whose text is shaped with the font's layout tables: the right-to-left
/// scripts, the Indic and Southeast Asian ones, Mongolian, and the Arabic presentation forms.
fn is_complex_char(c: char) -> bool {
    matches!(c as u32,
        0x0590..=0x0DFF | 0x0E00..=0x0FFF | 0x1000..=0x109F | 0x1780..=0x17FF | 0x1800..=0x18AF
        | 0x1900..=0x1AAF | 0xA800..=0xABFF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}

/// Whether `c` reads right to left.
fn is_rtl_char(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF)
}

/// Whether `text` holds anything [`shape_for_drawing`] shapes.
pub fn is_complex(text: &str) -> bool {
    text.chars().any(is_complex_char)
}

/// Whether the first letter of `text` that reads one way or the other reads right to left.
pub fn starts_rtl(text: &str) -> bool {
    text.chars().find(|c| c.is_alphabetic()).is_some_and(is_rtl_char)
}

/// The file of a bundled or registered static font.
fn bytes_of(font: &'static fontdue::Font) -> Option<&'static [u8]> {
    crate::bundled_bytes_of(font).or_else(|| crate::web::bytes_of(font))
}

/// A parsed face for shaping, kept for the life of the process.
struct Shaper(&'static Face<'static>);

// A face is read-only data over a file that lives forever.
unsafe impl Send for Shaper {}
unsafe impl Sync for Shaper {}

/// The face the shaper reads for `glyphs`: a static font's, or a variable one's at its axis values.
fn face_of(glyphs: Glyphs) -> Option<&'static Face<'static>> {
    match glyphs {
        Glyphs::Static(font) => static_face_of(font),
        Glyphs::Variable(instance) => instance.shaper(),
    }
}

fn static_face_of(font: &'static fontdue::Font) -> Option<&'static Face<'static>> {
    static FACES: Mutex<Option<HashMap<usize, Option<&'static Shaper>>>> = Mutex::new(None);
    let key = std::ptr::from_ref(font) as usize;
    if let Some(known) = FACES.lock().ok()?.as_ref().and_then(|m| m.get(&key)) {
        return known.map(|s| s.0);
    }
    // Parsed outside the lock; it is the slow part.
    let parsed = bytes_of(font)
        .and_then(|bytes| Face::from_slice(bytes, 0))
        .map(|face| &*Box::leak(Box::new(Shaper(Box::leak(Box::new(face))))));
    FACES.lock().ok()?.get_or_insert_with(HashMap::new).insert(key, parsed);
    parsed.map(|s| s.0)
}

/// `text` as glyphs, in the order they are drawn, with the font's default features: see
/// [`shape_text`].
pub fn shape_for_drawing(
    text: &str,
    bold: bool,
    italic: bool,
    face: Option<WebFontId>,
    px: f32,
    rtl: bool,
) -> Option<Vec<DrawGlyph>> {
    shape_text(text, bold, italic, face, FeaturesId::default(), px, rtl)
}

/// The direction a run is shaped in. A script is shaped in its own direction, and a run asked
/// to read right to left is shaped so (which mirrors its brackets); the glyphs come out in screen
/// order for that direction, and are turned around if the other was asked for.
fn run_direction(run: &str, rtl: bool) -> (Direction, bool) {
    let shape_rtl = rtl || starts_rtl(run);
    (
        if shape_rtl { Direction::RightToLeft } else { Direction::LeftToRight },
        shape_rtl != rtl,
    )
}

/// `text` as glyphs, in the order they are drawn, shaped with the font's own layout tables
/// (joining, ligatures, kerning, marks, contextual forms of every script) and the `features` the
/// box asked for. `rtl` says the text runs right to left, in which case the glyphs come out in
/// screen order. `None` when a face that draws part of it cannot be shaped.
pub fn shape_text(
    text: &str,
    bold: bool,
    italic: bool,
    face: Option<WebFontId>,
    features: FeaturesId,
    px: f32,
    rtl: bool,
) -> Option<Vec<DrawGlyph>> {
    if text.is_empty() {
        return None;
    }
    let settings = features_of(features);
    // Runs of characters one font draws.
    let mut runs: Vec<(Glyphs, usize, usize)> = Vec::new();
    for (offset, c) in text.char_indices() {
        let font = font_for_char_in(c, bold, italic, face);
        match runs.last_mut() {
            Some((f, _, end)) if f.address() == font.address() => *end = offset + c.len_utf8(),
            _ => runs.push((font, offset, offset + c.len_utf8())),
        }
    }
    let mut shaped_runs: Vec<Vec<DrawGlyph>> = Vec::with_capacity(runs.len());
    for (font, start, end) in runs {
        let hb = face_of(font)?;
        let run = &text[start..end];
        let (direction, turn) = run_direction(run, rtl);
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(run);
        buffer.set_direction(direction);
        buffer.guess_segment_properties();
        let glyphs = rustybuzz::shape(hb, &settings, buffer);
        let scale = px / hb.units_per_em() as f32;
        let mut shaped: Vec<DrawGlyph> = glyphs
            .glyph_infos()
            .iter()
            .zip(glyphs.glyph_positions())
            .map(|(info, pos)| DrawGlyph {
                font,
                id: info.glyph_id as u16,
                advance: pos.x_advance as f32 * scale,
                dx: pos.x_offset as f32 * scale,
                dy: pos.y_offset as f32 * scale,
            })
            .collect();
        if turn {
            shaped.reverse();
        }
        shaped_runs.push(shaped);
    }
    // A right-to-left line is drawn from its last run to its first.
    if rtl {
        shaped_runs.reverse();
    }
    Some(shaped_runs.into_iter().flatten().collect())
}

/// How far each character of `text` advances the pen once shaped: a ligature's width is its
/// first character's and the rest have none, a mark has none. `None` where [`shape_for_drawing`]
/// would be `None`. For finding where each character is in a shaped run.
pub fn char_advances(
    text: &str,
    bold: bool,
    italic: bool,
    face: Option<WebFontId>,
    features: FeaturesId,
    px: f32,
    rtl: bool,
) -> Option<Vec<f64>> {
    if text.is_empty() {
        return None;
    }
    let settings = features_of(features);
    let mut out = vec![0.0; text.chars().count()];
    let mut char_of_byte: HashMap<usize, usize> = HashMap::new();
    for (index, (offset, _)) in text.char_indices().enumerate() {
        char_of_byte.insert(offset, index);
    }
    let mut start = 0;
    let mut current: Option<Glyphs> = None;
    let mut runs: Vec<(Glyphs, usize, usize)> = Vec::new();
    for (offset, c) in text.char_indices() {
        let font = font_for_char_in(c, bold, italic, face);
        if current.is_some_and(|f| f.address() != font.address()) {
            runs.push((current.unwrap(), start, offset));
            start = offset;
        }
        current = Some(font);
    }
    if let Some(font) = current {
        runs.push((font, start, text.len()));
    }
    for (font, start, end) in runs {
        let hb = face_of(font)?;
        let run = &text[start..end];
        let (direction, _) = run_direction(run, rtl);
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(run);
        buffer.set_direction(direction);
        buffer.guess_segment_properties();
        let glyphs = rustybuzz::shape(hb, &settings, buffer);
        let scale = f64::from(px) / f64::from(hb.units_per_em());
        for (info, pos) in glyphs.glyph_infos().iter().zip(glyphs.glyph_positions()) {
            // Clusters are byte offsets into the run.
            if let Some(&index) = char_of_byte.get(&(start + info.cluster as usize)) {
                out[index] += f64::from(pos.x_advance) * scale;
            }
        }
    }
    Some(out)
}

/// How a font sets one character upright in a vertical line, in the font's units per em: the
/// glyph (its `vert`/`vrt2` alternate where it has one), how far the pen moves down the line, and
/// where the glyph is drawn from the pen, which sits at the top of the cell on the line's axis.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct VerticalCell {
    pub id: u16,
    /// Whether `id` is a vertical alternate rather than the glyph the character has anyway.
    pub alternate: bool,
    /// How far down the line the pen moves, as a fraction of the em.
    pub advance: f32,
    /// How far right of the line's axis the glyph is drawn from, and how far below the top of the
    /// cell its baseline is, as fractions of the em.
    pub dx: f32,
    pub dy: f32,
}

/// The vertical cell of `c` in `font`, from the font's own vertical layout (`vmtx`, `VORG`,
/// `vert`/`vrt2`, `vkrn`). `None` for a font that cannot be shaped.
pub fn vertical_cell(font: &'static fontdue::Font, c: char) -> Option<VerticalCell> {
    type Cells = HashMap<(usize, char), Option<VerticalCell>>;
    static CELLS: Mutex<Option<Cells>> = Mutex::new(None);
    let key = (std::ptr::from_ref(font) as usize, c);
    if let Some(known) = CELLS.lock().ok()?.as_ref().and_then(|m| m.get(&key)) {
        return *known;
    }
    let cell = (|| {
        let hb = static_face_of(font)?;
        let plain = hb.glyph_index(c)?.0;
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(&c.to_string());
        buffer.set_direction(Direction::TopToBottom);
        buffer.guess_segment_properties();
        let glyphs = rustybuzz::shape(hb, &[], buffer);
        let (info, pos) = (glyphs.glyph_infos(), glyphs.glyph_positions());
        if info.len() != 1 {
            return None;
        }
        let em = hb.units_per_em() as f32;
        Some(VerticalCell {
            id: info[0].glyph_id as u16,
            alternate: info[0].glyph_id as u16 != plain,
            // HarfBuzz counts down the line as negative, and a glyph drawn below the pen as a
            // negative offset.
            advance: -(pos[0].y_advance as f32) / em,
            dx: pos[0].x_offset as f32 / em,
            dy: -(pos[0].y_offset as f32) / em,
        })
    })();
    CELLS.lock().ok()?.get_or_insert_with(HashMap::new).insert(key, cell);
    cell
}

/// The glyph a font draws `c` with in a vertical line, when its `vert` or `vrt2` feature has one.
pub fn vertical_glyph(font: &'static fontdue::Font, c: char) -> Option<u16> {
    vertical_cell(font, c).filter(|cell| cell.alternate).map(|cell| cell.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> &'static fontdue::Font {
        crate::font_for(false, false)
    }

    fn glyph_of(c: char) -> u16 {
        static_face_of(font()).unwrap().glyph_index(c).unwrap().0
    }

    fn ids(text: &str, rtl: bool) -> Vec<u16> {
        shape_for_drawing(text, false, false, None, 16.0, rtl)
            .unwrap()
            .iter()
            .map(|g| g.id)
            .collect()
    }

    #[test]
    fn only_arabic_and_hebrew_text_is_shaped_here() {
        assert!(!is_complex("Hello, 你好"));
        assert!(is_complex("abc \u{5d0}"));
        assert!(is_complex("\u{628}"));
        assert!(is_complex("\u{915}"), "Devanagari");
    }

    #[test]
    fn joined_arabic_letters_take_the_fonts_own_joining_forms() {
        // beh beh beh, right to left: final, medial, initial on screen
        let shaped = ids("\u{628}\u{628}\u{628}", true);
        assert_eq!(shaped, [glyph_of('\u{FE90}'), glyph_of('\u{FE92}'), glyph_of('\u{FE91}')]);
        // asked for left to right it is the same glyphs: the script decides how they join
        let mut logical = ids("\u{628}\u{628}\u{628}", false);
        logical.sort_unstable();
        let mut expected = shaped.clone();
        expected.sort_unstable();
        assert_eq!(logical, expected);
    }

    #[test]
    fn lam_alef_is_one_glyph() {
        assert_eq!(ids("\u{644}\u{627}", true), [glyph_of('\u{FEFB}')]);
    }

    #[test]
    fn a_mark_is_placed_on_its_letter_and_takes_no_room_of_its_own() {
        let shaped = shape_for_drawing("\u{628}\u{64E}", false, false, None, 32.0, true).unwrap();
        assert_eq!(shaped.len(), 2);
        // on screen the mark's glyph comes first (the letter is on its right) or second; find it
        let mark = shaped.iter().find(|g| g.advance == 0.0).expect("a zero-width mark");
        assert!(mark.dx != 0.0 || mark.dy != 0.0, "moved onto the letter: {} {}", mark.dx, mark.dy);
        let letter = shaped.iter().find(|g| g.advance > 0.0).unwrap();
        assert!(letter.advance > 5.0);
    }

    #[test]
    fn widths_agree_between_shaping_and_per_character_advances() {
        let text = "\u{628}\u{644}\u{627}\u{633}";
        let glyph_sum: f32 = shape_for_drawing(text, false, false, None, 20.0, true)
            .unwrap()
            .iter()
            .map(|g| g.advance)
            .sum();
        let per_char: f64 = char_advances(text, false, false, None, FeaturesId::default(), 20.0, true).unwrap().iter().sum();
        assert!((f64::from(glyph_sum) - per_char).abs() < 0.01);
        let advances = char_advances(text, false, false, None, FeaturesId::default(), 20.0, true).unwrap();
        assert_eq!(advances.len(), 4);
        assert_eq!(advances[2], 0.0, "the alef is inside the lam-alef ligature");
    }

    #[test]
    fn a_vertical_cell_comes_from_the_fonts_own_vertical_layout() {
        let cjk = crate::font_for_char('\u{65E5}', false, false);
        let cell = vertical_cell(cjk, '\u{65E5}').unwrap();
        assert!(!cell.alternate);
        assert!((cell.advance - 1.0).abs() < 0.001, "an ideograph is one em tall: {}", cell.advance);
        assert!((cell.dx + 0.5).abs() < 0.001, "centred on the line's axis: {}", cell.dx);
        assert!(cell.dy > 0.7 && cell.dy < 1.0, "its baseline is most of an em below the top: {}", cell.dy);
        let bracket = vertical_cell(cjk, '\u{300C}').unwrap();
        assert!(bracket.alternate);
    }

    #[test]
    fn a_vertical_alternate_comes_from_the_fonts_vert_feature() {
        let cjk = crate::font_for_char('\u{300C}', false, false);
        let up = vertical_glyph(cjk, '\u{300C}');
        assert!(up.is_some(), "the corner bracket has a vertical form");
        assert_ne!(up, cjk.lookup_glyph_index('\u{300C}').into());
        assert_eq!(vertical_glyph(cjk, '\u{65E5}'), None, "an ideograph stays as it is");
    }
}
