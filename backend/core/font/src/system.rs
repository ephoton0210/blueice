// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The fonts the operating system itself ships, offered to a page as the families a stylesheet
//! names (`Helvetica`, `Arial`, `Times New Roman`, `serif`, ...).
//!
//! A browser that draws `font-family: sans-serif` with the system's Helvetica and one that draws
//! it with DejaVu Sans disagree about every width and every line height, so a page can only look
//! the same in both if the same fonts are used. The files are read from the system's own font
//! folders at the moment a page asks for them and are never copied into BlueIce. Off by default
//! (the bundled fonts keep every test and every platform deterministic); the `blueice-core`
//! binary turns it on. Where a file is missing, the family is simply not available and the
//! bundled font stands in, as before.

use crate::web::{self, WebFontId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Lets pages use the system's fonts (`false`: only the bundled ones and the page's own).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Whether pages may use the system's fonts.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// One family on disk: the files that hold its faces.
struct Family {
    files: &'static [&'static str],
    /// Apple's own Helvetica, Times and Courier carry no line gap in their tables, yet CoreText
    /// spaces their lines by 0.15 em more than ascent plus descent (measured against Safari for
    /// sizes 9 to 30).
    legacy: bool,
}

/// The family a name asks for, and where the system keeps it. The generic families are what
/// Safari draws them with by default.
fn family_files(name: &str) -> Option<Family> {
    let files: &'static [&'static str] = match name {
        "helvetica" | "sans-serif" => &["/System/Library/Fonts/Helvetica.ttc"],
        "system-ui" | "-apple-system" | "blinkmacsystemfont" | "ui-sans-serif" | "san francisco" => {
            &["/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/SFNSItalic.ttf"]
        }
        "helvetica neue" => &["/System/Library/Fonts/HelveticaNeue.ttc"],
        "arial" => &[
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
            "/System/Library/Fonts/Supplemental/Arial Italic.ttf",
            "/System/Library/Fonts/Supplemental/Arial Bold Italic.ttf",
        ],
        "times" | "serif" | "ui-serif" => &["/System/Library/Fonts/Times.ttc"],
        "times new roman" => &[
            "/System/Library/Fonts/Supplemental/Times New Roman.ttf",
            "/System/Library/Fonts/Supplemental/Times New Roman Bold.ttf",
            "/System/Library/Fonts/Supplemental/Times New Roman Italic.ttf",
            "/System/Library/Fonts/Supplemental/Times New Roman Bold Italic.ttf",
        ],
        "courier" | "monospace" | "ui-monospace" => &["/System/Library/Fonts/Courier.ttc"],
        "courier new" => &[
            "/System/Library/Fonts/Supplemental/Courier New.ttf",
            "/System/Library/Fonts/Supplemental/Courier New Bold.ttf",
            "/System/Library/Fonts/Supplemental/Courier New Italic.ttf",
            "/System/Library/Fonts/Supplemental/Courier New Bold Italic.ttf",
        ],
        "menlo" => &["/System/Library/Fonts/Menlo.ttc"],
        "georgia" => &[
            "/System/Library/Fonts/Supplemental/Georgia.ttf",
            "/System/Library/Fonts/Supplemental/Georgia Bold.ttf",
            "/System/Library/Fonts/Supplemental/Georgia Italic.ttf",
            "/System/Library/Fonts/Supplemental/Georgia Bold Italic.ttf",
        ],
        "verdana" => &[
            "/System/Library/Fonts/Supplemental/Verdana.ttf",
            "/System/Library/Fonts/Supplemental/Verdana Bold.ttf",
            "/System/Library/Fonts/Supplemental/Verdana Italic.ttf",
            "/System/Library/Fonts/Supplemental/Verdana Bold Italic.ttf",
        ],
        _ => return None,
    };
    let legacy = matches!(name, "helvetica" | "sans-serif" | "times" | "serif" | "ui-serif" | "courier" | "monospace" | "ui-monospace");
    Some(Family { files, legacy })
}

/// A face found in a file: its weight and slant, and the bytes of it as a plain font file.
struct Face {
    weight: u16,
    italic: bool,
    sfnt: Vec<u8>,
}

fn be16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

/// The table directory of the font whose offset table starts at `base`: `(tag, offset, length)`.
fn tables(d: &[u8], base: usize) -> Option<Vec<([u8; 4], usize, usize)>> {
    let count = usize::from(be16(d, base + 4)?);
    (0..count)
        .map(|i| {
            let e = base + 12 + 16 * i;
            Some((d.get(e..e + 4)?.try_into().ok()?, be32(d, e + 8)? as usize, be32(d, e + 12)? as usize))
        })
        .collect()
}

/// The font whose offset table starts at `base`, as a standalone font file, with its weight and
/// slant (`OS/2`, else `head`'s style bits).
fn face_at(d: &[u8], base: usize) -> Option<Face> {
    let dir = tables(d, base)?;
    let table = |tag: &[u8; 4]| dir.iter().find(|(t, _, _)| t == tag).map(|&(_, o, l)| (o, l));
    let (weight, italic) = match table(b"OS/2") {
        Some((o, l)) if l >= 64 => (be16(d, o + 4)?, be16(d, o + 62)? & 1 != 0),
        _ => {
            let (h, _) = table(b"head")?;
            let style = be16(d, h + 44)?;
            (if style & 1 != 0 { 700 } else { 400 }, style & 2 != 0)
        }
    };
    // Rebuilt with offsets that count from the start of the new file.
    let mut out = Vec::new();
    out.extend_from_slice(d.get(base..base + 12)?);
    let at = 12 + 16 * dir.len();
    let mut body = Vec::new();
    for (i, &(tag, offset, length)) in dir.iter().enumerate() {
        let data = d.get(offset..offset.checked_add(length)?)?;
        out.extend_from_slice(&tag);
        out.extend_from_slice(d.get(base + 12 + 16 * i + 4..base + 12 + 16 * i + 8)?);
        out.extend_from_slice(&u32::try_from(at + body.len()).ok()?.to_be_bytes());
        out.extend_from_slice(&u32::try_from(length).ok()?.to_be_bytes());
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
    }
    out.extend_from_slice(&body);
    Some(Face { weight, italic, sfnt: out })
}

/// Every face in a font file, a collection or a single font.
fn faces_in(d: &[u8]) -> Vec<Face> {
    if d.get(..4) == Some(b"ttcf") {
        let count = be32(d, 8).unwrap_or(0).min(64) as usize;
        (0..count)
            .filter_map(|i| face_at(d, be32(d, 12 + 4 * i)? as usize))
            .collect()
    } else {
        face_at(d, 0).into_iter().collect()
    }
}

/// A face, and the id it was registered under once a page asked for it.
type Loaded = (Face, OnceLock<Option<WebFontId>>);

/// The faces of a family, read once.
fn faces_of(name: &str) -> &'static [Loaded] {
    static CACHE: OnceLock<Mutex<HashMap<String, &'static [Loaded]>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut cache = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(found) = cache.get(name) {
        return found;
    }
    let mut faces = Vec::new();
    if let Some(family) = family_files(name) {
        for path in family.files {
            if let Ok(bytes) = std::fs::read(path) {
                faces.extend(faces_in(&bytes));
            }
        }
    }
    let faces: &'static [_] = Box::leak(
        faces
            .into_iter()
            .map(|f| (f, OnceLock::new()))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    cache.insert(name.to_string(), faces);
    faces
}

/// The face of the named family that best matches a weight and slant: the same slant first, then
/// the nearest weight (a heavier one for a bold request, as CSS asks).
fn face_of(name: &str, weight: u16, italic: bool, size_tenths: u16) -> Option<WebFontId> {
    let faces = faces_of(name);
    // The system's own face is one variable font whose weight and optical size are asked for.
    if matches!(name, "system-ui" | "-apple-system" | "blinkmacsystemfont" | "ui-sans-serif" | "san francisco") {
        let (face, _) = faces.iter().filter(|(f, _)| f.italic == italic).chain(faces.iter()).next().map(|f| (&f.0, &f.1))?;
        let id = web::register(&face.sfnt)?;
        let variation = crate::Variation {
            wght: Some(weight.clamp(1, 1000)),
            opsz: Some(size_tenths),
            ..crate::Variation::NONE
        };
        return web::register_set_varied(&[(id, Vec::new(), variation)]);
    }
    let best = faces.iter().min_by_key(|(f, _)| {
        let distance = u32::from(f.weight.abs_diff(weight));
        let lighter_for_bold = u32::from(weight >= 600 && f.weight < weight);
        (u32::from(f.italic != italic), lighter_for_bold, distance)
    })?;
    *best.1.get_or_init(|| {
        let id = web::register(&best.0.sfnt)?;
        if family_files(name).is_some_and(|f| f.legacy) {
            legacy_ids().lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(id.as_u32());
        }
        Some(id)
    })
}

fn legacy_ids() -> &'static Mutex<std::collections::HashSet<u32>> {
    static IDS: OnceLock<Mutex<std::collections::HashSet<u32>>> = OnceLock::new();
    IDS.get_or_init(Default::default)
}

/// The line gap, in em, that CoreText adds to a system face whose own tables carry none.
pub(crate) fn extra_line_gap(face: WebFontId) -> f64 {
    let legacy = legacy_ids().lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(&face.as_u32());
    if legacy { 0.15 } else { 0.0 }
}

/// The system's face for the first family of a `font-family` list that the system has, with the
/// weight and slant asked for. `None` when system fonts are off or the system has none of them.
/// A list with no `font-family` at all is the browser's default family, a serif.
pub fn face_for(families: Option<&str>, weight: u16, italic: bool, size_tenths: u16) -> Option<WebFontId> {
    if !enabled() {
        return None;
    }
    let Some(families) = families else {
        return face_of("times", weight, italic, size_tenths);
    };
    families.split(',').find_map(|family| {
        let family = family.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_ascii_lowercase();
        face_of(&family, weight, italic, size_tenths)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_by_default_so_the_bundled_fonts_stay_the_same_everywhere() {
        assert!(!enabled());
        assert!(face_for(Some("Helvetica"), 400, false, 160).is_none());
    }

    #[test]
    fn a_collection_is_split_into_faces_with_their_weight_and_slant() {
        let Ok(bytes) = std::fs::read("/System/Library/Fonts/Helvetica.ttc") else {
            return; // not macOS
        };
        let faces = faces_in(&bytes);
        assert!(faces.iter().any(|f| f.weight == 700 && !f.italic));
        assert!(faces.iter().any(|f| f.weight == 400 && f.italic));
        for f in &faces {
            assert!(fontdue::Font::from_bytes(f.sfnt.as_slice(), fontdue::FontSettings::default()).is_ok());
        }
    }
}
