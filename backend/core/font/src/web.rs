// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fonts a page brings with it (`@font-face`): WOFF2, WOFF and plain TrueType/OpenType files
//! decoded into faces the rest of the engine names by a small id.
//!
//! A registered face lives as long as the process: ids are handed to layout and paint as plain
//! numbers, and the same file registered again (another tab, a reload) is the same face. The
//! number and size of faces is bounded so a page cannot grow the process without limit; past the
//! bound a file is simply not registered and its text falls back to the bundled fonts.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

pub use crate::variable::Axis;
use crate::variable::{axes, VarInstance, Variation};
use crate::Glyphs;

/// The largest font file taken from a page, as sent.
pub const MAX_FONT_BYTES: usize = 8 * 1024 * 1024;
/// The largest font once decompressed.
const MAX_DECODED_BYTES: usize = 24 * 1024 * 1024;
/// Faces registered over the life of the process.
const MAX_FACES: usize = 256;
/// Decoded bytes registered over the life of the process.
const MAX_TOTAL_BYTES: usize = 96 * 1024 * 1024;

/// What text is drawn with: one registered face, or a set of faces each covering part of the
/// code points (a family split by `unicode-range`, and the families after it in a `font-family`
/// list). One face is the set of that face alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WebFontId(u32);

impl WebFontId {
    /// The id as a number, for places that can only carry numbers (a computed style).
    pub fn as_u32(self) -> u32 {
        self.0
    }

    pub fn from_u32(id: u32) -> Self {
        WebFontId(id)
    }
}

/// One member of a set: a face (an index into the registry's faces) and the code points it
/// covers (empty: all of them).
#[derive(Clone, PartialEq, Eq, Hash)]
struct Member {
    face: u32,
    ranges: Vec<(u32, u32)>,
    /// How a variable face is drawn (already resolved against its axes; none for a static face).
    var: Variation,
}

/// A variable face's file and its axes.
type VariableFile = (&'static [u8], &'static [Axis]);

/// One member of a varied set: a face, the code points it covers, and how it is drawn.
pub type VariedMember = (WebFontId, Vec<(u32, u32)>, Variation);

/// A registered face. A variable one keeps its file, to be drawn at any weight of its axis.
struct FaceEntry {
    font: &'static fontdue::Font,
    variable: Option<VariableFile>,
}

impl Member {
    fn covers(&self, c: char) -> bool {
        self.ranges.is_empty() || self.ranges.iter().any(|&(lo, hi)| lo <= c as u32 && c as u32 <= hi)
    }
}

#[derive(Default)]
struct Registry {
    /// Faces by content, to the set that holds that face alone.
    by_content: HashMap<u64, u32>,
    faces: Vec<FaceEntry>,
    /// Variable faces at a weight, made when first drawn.
    instances: HashMap<(u32, Variation), &'static VarInstance>,
    sets: Vec<Vec<Member>>,
    by_members: HashMap<Vec<Member>, u32>,
    bytes: usize,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: std::sync::OnceLock<Mutex<Registry>> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

/// The sfnt bytes of a WOFF2, WOFF or plain font file.
fn to_sfnt(bytes: &[u8]) -> Option<Vec<u8>> {
    let sfnt = match bytes.get(..4)? {
        b"wOF2" => wuff::decompress_woff2(bytes).ok()?,
        b"wOFF" => wuff::decompress_woff1(bytes).ok()?,
        // TrueType, OpenType with CFF outlines, and Apple's `true`; a collection is not a font
        // face a page can name.
        [0, 1, 0, 0] | b"OTTO" | b"true" => bytes.to_vec(),
        _ => return None,
    };
    (sfnt.len() <= MAX_DECODED_BYTES).then_some(sfnt)
}

/// Decodes `bytes` and registers the face. `None` for a file that is not a usable font, is too
/// big, or would take the process past its bounds. Never panics on damaged input.
pub fn register(bytes: &[u8]) -> Option<WebFontId> {
    if bytes.is_empty() || bytes.len() > MAX_FONT_BYTES {
        return None;
    }
    std::panic::catch_unwind(|| register_unguarded(bytes)).ok().flatten()
}

fn register_unguarded(bytes: &[u8]) -> Option<WebFontId> {
    let sfnt = to_sfnt(bytes)?;
    let mut hasher = DefaultHasher::new();
    sfnt.hash(&mut hasher);
    let key = hasher.finish();
    {
        let known = registry().lock().ok()?;
        if let Some(&id) = known.by_content.get(&key) {
            return Some(WebFontId(id));
        }
        if known.faces.len() >= MAX_FACES || known.bytes + sfnt.len() > MAX_TOTAL_BYTES {
            return None;
        }
    }
    // Parsed outside the lock: it is the slow part.
    let size = sfnt.len();
    let found = axes(&sfnt);
    let variable = (!found.is_empty()).then(|| {
        (&*Box::leak(sfnt.clone().into_boxed_slice()), &*Box::leak(found.into_boxed_slice()))
    });
    let font = fontdue::Font::from_bytes(sfnt, fontdue::FontSettings::default()).ok()?;
    let mut known = registry().lock().ok()?;
    if let Some(&id) = known.by_content.get(&key) {
        return Some(WebFontId(id));
    }
    if known.faces.len() >= MAX_FACES || known.bytes + size > MAX_TOTAL_BYTES {
        return None;
    }
    let face = u32::try_from(known.faces.len()).ok()?;
    known.faces.push(FaceEntry { font: Box::leak(Box::new(font)), variable });
    let members = vec![Member { face, ranges: Vec::new(), var: Variation::NONE }];
    let id = u32::try_from(known.sets.len()).ok()?;
    known.sets.push(members.clone());
    known.by_members.insert(members, id);
    known.by_content.insert(key, id);
    known.bytes += size;
    Some(WebFontId(id))
}

/// Sets registered over the life of the process.
const MAX_SETS: usize = 4096;

/// Registers the set of `members` in priority order (the first member that covers a character
/// and has a glyph for it draws it): each is a registered face and the code point ranges it
/// covers, none meaning all. Equal sets are one set; a set of one member with no range is that
/// member's own id. `None` for no members or past the bound.
pub fn register_set(members: &[(WebFontId, Vec<(u32, u32)>)]) -> Option<WebFontId> {
    let varied: Vec<_> = members.iter().map(|(id, r)| (*id, r.clone(), Variation::NONE)).collect();
    register_set_varied(&varied)
}

/// [`register_set`] with how each member is drawn when it is a variable face (clamped to the
/// face's axes; axes it lacks, and every axis of a static face, are ignored).
pub fn register_set_varied(members: &[VariedMember]) -> Option<WebFontId> {
    if members.is_empty() {
        return None;
    }
    let mut known = registry().lock().ok()?;
    let mut resolved = Vec::with_capacity(members.len());
    for (id, ranges, asked) in members {
        // A member is a face; if the id given is itself a set, its first member is the face.
        let face = known.sets.get(id.0 as usize)?.first()?.face;
        let mut ranges = ranges.clone();
        ranges.sort_unstable();
        let var = match known.faces.get(face as usize)?.variable {
            Some((_, found)) => asked.resolve(found),
            None => Variation::NONE,
        };
        resolved.push(Member { face, ranges, var });
    }
    if let [only] = resolved.as_slice() {
        if only.ranges.is_empty() && only.var.is_none() {
            return known
                .by_members
                .get(&resolved)
                .copied()
                .map(WebFontId);
        }
    }
    if let Some(&id) = known.by_members.get(&resolved) {
        return Some(WebFontId(id));
    }
    if known.sets.len() >= MAX_SETS {
        return None;
    }
    let id = u32::try_from(known.sets.len()).ok()?;
    known.sets.push(resolved.clone());
    known.by_members.insert(resolved, id);
    Some(WebFontId(id))
}

/// The drawable of `member` (a variable face at its weight, made on first use).
fn glyphs_of(known: &mut Registry, member: &Member) -> Option<Glyphs> {
    let entry = known.faces.get(member.face as usize)?;
    let (font, variable) = (entry.font, entry.variable);
    match variable {
        Some((data, _)) if !member.var.is_none() => {
            if let Some(&instance) = known.instances.get(&(member.face, member.var)) {
                return Some(Glyphs::Variable(instance));
            }
            if known.instances.len() >= MAX_INSTANCES {
                return Some(Glyphs::Static(font));
            }
            match VarInstance::new(data, &member.var) {
                Some(instance) => {
                    let instance: &'static VarInstance = Box::leak(Box::new(instance));
                    known.instances.insert((member.face, member.var), instance);
                    Some(Glyphs::Variable(instance))
                }
                None => Some(Glyphs::Static(font)),
            }
        }
        _ => Some(Glyphs::Static(font)),
    }
}

/// Variable faces at one setting, made over the life of the process.
const MAX_INSTANCES: usize = 512;

/// The driven axes of a registered face (`id` being a face or a set: the first member's face).
pub fn axes_of(id: WebFontId) -> Vec<Axis> {
    let Ok(known) = registry().lock() else { return Vec::new() };
    known
        .sets
        .get(id.0 as usize)
        .and_then(|m| m.first())
        .and_then(|m| known.faces.get(m.face as usize))
        .and_then(|f| f.variable)
        .map_or(Vec::new(), |(_, found)| found.to_vec())
}

/// Whether the face leans by an axis (`slnt` or `ital`), so a page's italic needs no drawing trick.
pub fn slants_by_axis(id: WebFontId) -> bool {
    axes_of(id).iter().any(|a| &a.tag == b"slnt" || &a.tag == b"ital")
}

/// The face a set is measured by for line metrics: the first member that covers ordinary Latin
/// text, else the first member.
pub fn get(id: WebFontId) -> Option<Glyphs> {
    let mut known = registry().lock().ok()?;
    let members = known.sets.get(id.0 as usize)?;
    let primary = members.iter().find(|m| m.covers('a')).or_else(|| members.first())?.clone();
    glyphs_of(&mut known, &primary)
}

/// The face of set `id` that draws `c`: the first member covering `c` that has a glyph for it.
pub fn font_for(id: WebFontId, c: char) -> Option<Glyphs> {
    let mut known = registry().lock().ok()?;
    let member = known
        .sets
        .get(id.0 as usize)?
        .iter()
        .filter(|m| m.covers(c))
        .find(|m| known.faces.get(m.face as usize).is_some_and(|f| f.font.has_glyph(c)))?
        .clone();
    glyphs_of(&mut known, &member)
}
