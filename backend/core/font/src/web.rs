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

/// The largest font file taken from a page, as sent.
pub const MAX_FONT_BYTES: usize = 8 * 1024 * 1024;
/// The largest font once decompressed.
const MAX_DECODED_BYTES: usize = 24 * 1024 * 1024;
/// Faces registered over the life of the process.
const MAX_FACES: usize = 256;
/// Decoded bytes registered over the life of the process.
const MAX_TOTAL_BYTES: usize = 96 * 1024 * 1024;

/// A registered face.
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

#[derive(Default)]
struct Registry {
    by_content: HashMap<u64, u32>,
    faces: Vec<&'static fontdue::Font>,
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
    let font = fontdue::Font::from_bytes(sfnt, fontdue::FontSettings::default()).ok()?;
    let mut known = registry().lock().ok()?;
    if let Some(&id) = known.by_content.get(&key) {
        return Some(WebFontId(id));
    }
    if known.faces.len() >= MAX_FACES || known.bytes + size > MAX_TOTAL_BYTES {
        return None;
    }
    let id = u32::try_from(known.faces.len()).ok()?;
    known.faces.push(Box::leak(Box::new(font)));
    known.by_content.insert(key, id);
    known.bytes += size;
    Some(WebFontId(id))
}

/// The face registered under `id`.
pub fn get(id: WebFontId) -> Option<&'static fontdue::Font> {
    registry().lock().ok()?.faces.get(id.0 as usize).copied()
}
