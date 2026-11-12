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

/// One glyph to draw, in the order it is drawn left to right.
#[derive(Clone, Copy)]
pub struct DrawGlyph {
    /// The font that has it.
    pub font: &'static fontdue::Font,
    /// Its index in that font.
    pub id: u16,
    /// How far the pen moves after it, in pixels.
    pub advance: f32,
    /// Where it is drawn from the pen: right, and up.
    pub dx: f32,
    pub dy: f32,
}

/// Whether `c` is in a script whose text is shaped with the font's layout tables.
fn is_complex_char(c: char) -> bool {
    matches!(c as u32,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
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

fn face_of(font: &'static fontdue::Font) -> Option<&'static Face<'static>> {
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

/// `text` as glyphs, in the order they are drawn. `rtl` says the text runs right to left, in which
/// case the glyphs come out already in screen order. `None` when the text is not in a script that
/// needs shaping or a face that draws part of it cannot be shaped.
pub fn shape_for_drawing(
    text: &str,
    bold: bool,
    italic: bool,
    face: Option<WebFontId>,
    px: f32,
    rtl: bool,
) -> Option<Vec<DrawGlyph>> {
    if !is_complex(text) {
        return None;
    }
    // Runs of characters one font draws.
    let mut runs: Vec<(&'static fontdue::Font, usize, usize)> = Vec::new();
    for (offset, c) in text.char_indices() {
        let font = match font_for_char_in(c, bold, italic, face) {
            Glyphs::Static(font) => font,
            Glyphs::Variable(_) => return None,
        };
        match runs.last_mut() {
            Some((f, _, end)) if std::ptr::eq(*f, font) => *end = offset + c.len_utf8(),
            _ => runs.push((font, offset, offset + c.len_utf8())),
        }
    }
    let mut out = Vec::with_capacity(text.len());
    let mut shaped_runs: Vec<Vec<DrawGlyph>> = Vec::with_capacity(runs.len());
    for (font, start, end) in runs {
        let hb = face_of(font)?;
        let run = &text[start..end];
        // A script is shaped in its own direction, which gives the glyphs in screen order for
        // that direction; asked for the other way round, they are turned around.
        let natural_rtl = starts_rtl(run);
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(run);
        buffer.set_direction(if natural_rtl { Direction::RightToLeft } else { Direction::LeftToRight });
        buffer.guess_segment_properties();
        let glyphs = rustybuzz::shape(hb, &[], buffer);
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
        if natural_rtl != rtl {
            shaped.reverse();
        }
        shaped_runs.push(shaped);
    }
    // A right-to-left line is drawn from its last run to its first.
    if rtl {
        shaped_runs.reverse();
    }
    out.extend(shaped_runs.into_iter().flatten());
    Some(out)
}

/// How far each character of `text` advances the pen once shaped: a ligature's width is its
/// first character's and the rest have none, a mark has none. `None` where [`shape_for_drawing`]
/// would be `None`. For finding where each character is in a shaped run.
pub fn char_advances(
    text: &str,
    bold: bool,
    italic: bool,
    face: Option<WebFontId>,
    px: f32,
    _rtl: bool,
) -> Option<Vec<f64>> {
    if !is_complex(text) {
        return None;
    }
    let mut out = vec![0.0; text.chars().count()];
    let mut char_of_byte: HashMap<usize, usize> = HashMap::new();
    for (index, (offset, _)) in text.char_indices().enumerate() {
        char_of_byte.insert(offset, index);
    }
    let mut start = 0;
    let mut current: Option<&'static fontdue::Font> = None;
    let mut runs: Vec<(&'static fontdue::Font, usize, usize)> = Vec::new();
    for (offset, c) in text.char_indices() {
        let font = match font_for_char_in(c, bold, italic, face) {
            Glyphs::Static(font) => font,
            Glyphs::Variable(_) => return None,
        };
        if current.is_some_and(|f| !std::ptr::eq(f, font)) {
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
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(run);
        buffer.set_direction(if starts_rtl(run) { Direction::RightToLeft } else { Direction::LeftToRight });
        buffer.guess_segment_properties();
        let glyphs = rustybuzz::shape(hb, &[], buffer);
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

/// The glyph a font draws `c` with in a vertical line, when its `vert` or `vrt2` feature has one:
/// the corner brackets that turn to face down the line, the small kana and stops that move to
/// the upper right of their cell, the dashes that stand on end. `None` for a character those
/// features leave alone, or a font that cannot be shaped.
pub fn vertical_glyph(font: &'static fontdue::Font, c: char) -> Option<u16> {
    let hb = face_of(font)?;
    let plain = hb.glyph_index(c)?.0;
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(&c.to_string());
    buffer.set_direction(Direction::TopToBottom);
    buffer.guess_segment_properties();
    let glyphs = rustybuzz::shape(hb, &[], buffer);
    let info = glyphs.glyph_infos();
    (info.len() == 1 && info[0].glyph_id as u16 != plain).then(|| info[0].glyph_id as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> &'static fontdue::Font {
        crate::font_for(false, false)
    }

    fn glyph_of(c: char) -> u16 {
        face_of(font()).unwrap().glyph_index(c).unwrap().0
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
        assert!(shape_for_drawing("Hello", false, false, None, 16.0, false).is_none());
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
        let per_char: f64 = char_advances(text, false, false, None, 20.0, true).unwrap().iter().sum();
        assert!((f64::from(glyph_sum) - per_char).abs() < 0.01);
        let advances = char_advances(text, false, false, None, 20.0, true).unwrap();
        assert_eq!(advances.len(), 4);
        assert_eq!(advances[2], 0.0, "the alef is inside the lam-alef ligature");
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
