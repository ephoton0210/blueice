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

/// The full name of the font whose offset table starts at `base` (Windows platform, UTF-16).
fn full_name(d: &[u8], dir: &[([u8; 4], usize, usize)]) -> String {
    let Some(&(_, offset, length)) = dir.iter().find(|(t, _, _)| t == b"name") else {
        return String::new();
    };
    let Some(table) = d.get(offset..offset.saturating_add(length)) else {
        return String::new();
    };
    let count = usize::from(be16(table, 2).unwrap_or(0));
    let strings = usize::from(be16(table, 4).unwrap_or(0));
    // The English name (language 0x409) if there is one, else the first.
    let mut first = None;
    for i in 0..count {
        let r = 6 + 12 * i;
        let (platform, language, id, len, at) = (
            be16(table, r).unwrap_or(0),
            be16(table, r + 4).unwrap_or(0),
            be16(table, r + 6).unwrap_or(0),
            usize::from(be16(table, r + 8).unwrap_or(0)),
            usize::from(be16(table, r + 10).unwrap_or(0)),
        );
        if platform == 3 && id == 4 {
            if let Some(raw) = table.get(strings + at..strings + at + len) {
                let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                let name = String::from_utf16_lossy(&units);
                if language == 0x409 {
                    return name;
                }
                first.get_or_insert(name);
            }
        }
    }
    first.unwrap_or_default()
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

/// A face's weight, slant (`OS/2`, else `head`'s style bits) and full name, read without copying
/// it: a collection of dozens of faces is looked through before one is taken out.
fn face_info(d: &[u8], base: usize) -> Option<(u16, bool, String)> {
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
    Some((weight, italic, full_name(d, &dir)))
}

/// The upright face of a collection whose full name contains `part`, at weight 400.
fn face_named(d: &[u8], part: &str) -> Option<Face> {
    if d.get(..4) != Some(b"ttcf") {
        return None;
    }
    let count = be32(d, 8)?.min(256) as usize;
    (0..count).find_map(|i| {
        let base = be32(d, 12 + 4 * i)? as usize;
        let (weight, italic, name) = face_info(d, base)?;
        (weight == 400 && !italic && name.contains(part)).then(|| face_at(d, base)).flatten()
    })
}

/// The font whose offset table starts at `base`, as a standalone font file, with its weight and
/// slant.
fn face_at(d: &[u8], base: usize) -> Option<Face> {
    let dir = tables(d, base)?;
    let (weight, italic, _) = face_info(d, base)?;
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

/// Whether the system's San Francisco can be drawn: its `h` has an outline.
fn system_font_draws() -> bool {
    static DRAWS: OnceLock<bool> = OnceLock::new();
    *DRAWS.get_or_init(|| {
        struct Nothing;
        impl ttf_parser::OutlineBuilder for Nothing {
            fn move_to(&mut self, _: f32, _: f32) {}
            fn line_to(&mut self, _: f32, _: f32) {}
            fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
            fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
            fn close(&mut self) {}
        }
        let Ok(data) = std::fs::read("/System/Library/Fonts/SFNS.ttf") else {
            return false;
        };
        ttf_parser::Face::parse(&data, 0)
            .ok()
            .and_then(|face| face.glyph_index('h').and_then(|id| face.outline_glyph(id, &mut Nothing)))
            .is_some()
    })
}

/// The face of the named family that best matches a weight and slant: the same slant first, then
/// the nearest weight (a heavier one for a bold request, as CSS asks).
fn face_of(name: &str, weight: u16, italic: bool, size_tenths: u16) -> Option<WebFontId> {
    let faces = faces_of(name);
    // The system's own face is one variable font whose weight and optical size are asked for.
    if matches!(name, "system-ui" | "-apple-system" | "blinkmacsystemfont" | "ui-sans-serif" | "san francisco") {
        // Recent macOS keeps the outlines of San Francisco in a table no font library here reads
        // (its glyphs come out empty), so Helvetica Neue stands in for it.
        if !system_font_draws() {
            return face_of("helvetica neue", weight, italic, size_tenths);
        }
        let (face, _) = faces.iter().filter(|(f, _)| f.italic == italic).chain(faces.iter()).next().map(|f| (&f.0, &f.1))?;
        let id = web::register_system(&face.sfnt)?;
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
        let id = web::register_system(&best.0.sfnt)?;
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
pub fn face_for(
    families: Option<&str>,
    weight: u16,
    italic: bool,
    size_tenths: u16,
    lang: Option<&str>,
) -> Option<WebFontId> {
    if !enabled() {
        return None;
    }
    let Some(families) = families else {
        return face_of("times", weight, italic, size_tenths);
    };
    families.split(',').find_map(|family| {
        let family = family.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_ascii_lowercase();
        // Safari draws the generic sans-serif of a Chinese page with PingFang, Latin letters too.
        if family == "sans-serif" {
            if let Some(variant) = pingfang_for(lang) {
                if let Some(id) = pingfang(variant) {
                    return Some(id);
                }
            }
        }
        face_of(&family, weight, italic, size_tenths)
    })
}

/// Whether a character is one the bundled Latin fonts cannot draw and the system's CJK face
/// should: Han, kana, hangul, the CJK symbols and the fullwidth forms.
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x9FFF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x2FA1F)
}

/// A PingFang face of the system (`variant`: "PingFang TC", "PingFang SC", "PingFang HK"), which
/// macOS keeps in a downloadable font asset; found once. `None` where it is not installed.
fn pingfang(variant: &str) -> Option<WebFontId> {
    static FACES: OnceLock<Mutex<HashMap<String, Option<WebFontId>>>> = OnceLock::new();
    let cache = FACES.get_or_init(Default::default);
    if let Some(found) = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(variant) {
        return *found;
    }
    let found = (|| {
        for dir in std::fs::read_dir("/System/Library/AssetsV2").ok()?.flatten() {
            if !dir.file_name().to_string_lossy().starts_with("com_apple_MobileAsset_Font") {
                continue;
            }
            let Ok(versions) = std::fs::read_dir(dir.path()) else { continue };
            for version in versions.flatten() {
                let Ok(bytes) = std::fs::read(version.path().join("AssetData/PingFang.ttc")) else { continue };
                if let Some(face) = face_named(&bytes, variant) {
                    return web::register_system(&face.sfnt);
                }
            }
        }
        None
    })();
    cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(variant.to_string(), found);
    found
}

/// The PingFang variant a language's generic sans-serif is drawn in.
fn pingfang_for(lang: Option<&str>) -> Option<&'static str> {
    let lang = lang?.to_ascii_lowercase();
    if lang == "zh-hk" || lang == "zh-mo" {
        Some("PingFang HK")
    } else if lang == "zh-tw" || lang.starts_with("zh-hant") {
        Some("PingFang TC")
    } else if lang == "zh" || lang.starts_with("zh-cn") || lang.starts_with("zh-hans") || lang.starts_with("zh-sg") {
        Some("PingFang SC")
    } else {
        None
    }
}

fn cjk_face() -> Option<WebFontId> {
    pingfang("PingFang TC")
}

/// The system face that draws `c` when the page's own face cannot: PingFang for CJK text. `None`
/// when system fonts are off, `c` is not CJK or the face lacks it.
pub fn fallback_glyphs(c: char) -> Option<crate::Glyphs> {
    if !enabled() || !is_cjk(c) {
        return None;
    }
    let glyphs = web::get(cjk_face()?)?;
    glyphs.has_glyph(c).then_some(glyphs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_by_default_and_when_on_cjk_text_comes_from_the_systems_face() {
        // One test, because the switch is the process's.
        assert!(!enabled());
        assert!(face_for(Some("Helvetica"), 400, false, 160, None).is_none());
        assert!(fallback_glyphs('關').is_none(), "off: the bundled CJK face draws it");
        set_enabled(true);
        let found = fallback_glyphs('關');
        let latin = fallback_glyphs('a');
        set_enabled(false);
        assert!(latin.is_none(), "only CJK is a fallback");
        if cjk_face().is_some() {
            assert!(found.is_some_and(|g| g.has_glyph('關')), "PingFang draws Han");
        }
    }

    #[test]
    fn system_ui_text_has_glyphs_to_draw() {
        // macOS's San Francisco may have no outlines a font library can read; whatever stands in
        // for `system-ui` must draw, or every control's text disappears.
        if !std::path::Path::new("/System/Library/Fonts/Helvetica.ttc").exists() {
            return; // not macOS
        }
        set_enabled(true);
        let id = face_for(Some("system-ui"), 400, false, 130, None);
        set_enabled(false);
        let glyphs = web::get(id.expect("a face")).expect("glyphs");
        let (metrics, bitmap) = glyphs.rasterize('h', 16.0);
        assert!(metrics.width > 0 && bitmap.iter().any(|&b| b > 0), "{metrics:?}");
    }

    #[test]
    fn a_chinese_pages_sans_serif_is_pingfang_with_its_latin_letters() {
        // Safari: `sans-serif` on a zh-TW page is PingFang TC; "Google" is 46.28px at 14px there.
        if pingfang("PingFang TC").is_none() {
            return; // not macOS, or the font asset is not downloaded
        }
        set_enabled(true);
        let chinese = face_for(Some("sans-serif"), 400, false, 140, Some("zh-TW"));
        let plain = face_for(Some("sans-serif"), 400, false, 140, Some("en"));
        set_enabled(false);
        assert_ne!(chinese, plain, "a Chinese page gets PingFang, an English one Helvetica");
        let width = crate::measure_text_width_in("Google", 14.0, false, false, chinese);
        assert!((width - 46.28).abs() < 0.2, "{width}");
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


