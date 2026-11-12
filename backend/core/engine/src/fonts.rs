// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The fonts a page brings (`@font-face`), fetched with the rest of its subresources and
//! decoded into faces (`blueice_font::web`), and the choice of which face an element's text uses.
//!
//! A font is optional content, like an image: one that cannot be fetched or decoded leaves the
//! page's text in the next family of its list, and in the end the bundled fonts. Fonts are
//! fetched before the page is shown (`font-display: swap` has nothing to swap to), behind the same
//! per-hop review as every other subresource.

use crate::stylesheet::LinkedStyles;
use blueice_css::{FontFaceRule, FontSource};
use blueice_font::web::{WebFontId, MAX_FONT_BYTES};
use std::collections::HashMap;
use url::Url;

/// How much font a page may ask for.
#[derive(Clone, Copy)]
pub(crate) struct FontLimits {
    /// Faces fetched for one page.
    pub faces: usize,
    pub redirects: usize,
}

pub(crate) const DEFAULT_FONT_LIMITS: FontLimits = FontLimits {
    faces: 24,
    redirects: 3,
};

/// One loaded face and the part of a family it covers.
#[derive(Debug, Clone)]
struct CatalogFace {
    weight_min: u16,
    weight_max: u16,
    italic: bool,
    slant: Option<(i16, i16)>,
    /// The widths it covers, as `font-stretch` percentages in tenths.
    stretch_min: u16,
    stretch_max: u16,
    ranges: Vec<(u32, u32)>,
    id: WebFontId,
    /// The font file, kept so the scripts' process can decode its own copy.
    bytes: std::sync::Arc<Vec<u8>>,
}

/// A page's faces by lowercased family name.
#[derive(Debug, Clone, Default)]
pub(crate) struct FontCatalog {
    families: HashMap<String, Vec<CatalogFace>>,
    script_faces: usize,
    generation: u64,
}

/// A face a script made and added to the document (`new FontFace`, `document.fonts.add`): the
/// bytes it was given, which the page decodes into its own registry, and the descriptors that
/// say what part of a family it is.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScriptFont {
    pub family: String,
    pub weight_min: u16,
    pub weight_max: u16,
    pub italic: bool,
    /// The angles an oblique face leans through, in tenths of a degree (see `FontFaceRule::slant`).
    pub slant: Option<(i16, i16)>,
    /// The widths it covers, `font-stretch` percentages in tenths.
    pub stretch_min: u16,
    pub stretch_max: u16,
    pub ranges: Vec<(u32, u32)>,
    pub bytes: Vec<u8>,
}

/// Faces scripts may add to one page.
pub(crate) const MAX_SCRIPT_FONTS: usize = 64;

/// Generic family names and the families of the system's own fonts: a list that reaches one of
/// these before any of the page's families is satisfied by the system font, not by a face that
/// comes later in the list.
const SYSTEM_FAMILIES: &[&str] = &[
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "emoji",
    "math",
    "fangsong",
    "-apple-system",
    "blinkmacsystemfont",
];

impl FontCatalog {
    pub(crate) fn is_empty(&self) -> bool {
        self.families.is_empty()
    }

    /// What text with this `font-family` list (lowercased, comma separated), weight and style is
    /// drawn with: for each family the page has faces for, the faces of its best-matching style
    /// and weight, those declared last first; the families after the first join the set so they
    /// cover what the earlier ones do not. A system or generic family ends the list (the system
    /// font draws the rest), and a list the page has no face for is `None`.
    #[cfg(test)]
    pub(crate) fn face_for(&self, families: &str, weight: u16, italic: bool) -> Option<WebFontId> {
        self.face_and_synthesis(families, weight, italic).0
    }

    /// The face and synthesis for a weight and style alone.
    #[cfg(test)]
    pub(crate) fn face_and_synthesis(
        &self,
        families: &str,
        weight: u16,
        italic: bool,
    ) -> (Option<WebFontId>, u8) {
        self.face_for_request(families, &FaceRequest::new(weight, italic))
    }

    /// [`face_for`](Self::face_for), and which of bold (1) and italic (2) the page's primary
    /// face lacks, so the text is made bold or slanted by drawing instead.
    pub(crate) fn face_for_request(&self, families: &str, req: &FaceRequest) -> (Option<WebFontId>, u8) {
        let (weight, italic) = (req.weight, req.slant != Slant::Upright);
        let mut synthesis = None;
        let mut members: Vec<blueice_font::web::VariedMember> = Vec::new();
        for family in families.split(',') {
            let family = family.trim();
            if let Some(faces) = self.families.get(family) {
                if synthesis.is_none() {
                    synthesis = best_of(faces, weight, italic).map(|best| {
                        let leans = blueice_font::web::slants_by_axis(best.id);
                        u8::from(weight >= 600 && best.weight_max < 600)
                            | (u8::from(italic && !best.italic && !leans) << 1)
                    });
                }
                members.extend(best_faces(faces, req));
            } else if SYSTEM_FAMILIES.contains(&family) {
                break;
            }
        }
        (blueice_font::web::register_set_varied(&members), synthesis.unwrap_or(0))
    }

    /// Adds a face a script supplied. False when it does not decode, or the page already has as
    /// many script faces as it may.
    pub(crate) fn add_script_font(&mut self, font: &ScriptFont) -> bool {
        let family = font.family.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_ascii_lowercase();
        if family.is_empty() || self.script_faces >= 2 * MAX_SCRIPT_FONTS {
            return false;
        }
        let Some(id) = blueice_font::web::register(&font.bytes) else {
            return false;
        };
        self.script_faces += 1;
        let bytes = std::sync::Arc::new(font.bytes.clone());
        self.add(
            &family,
            CatalogFace {
                weight_min: font.weight_min,
                weight_max: font.weight_max,
                italic: font.italic,
                slant: font.slant,
                stretch_min: font.stretch_min,
                stretch_max: font.stretch_max,
                ranges: font.ranges.clone(),
                id,
                bytes,
            },
        );
        true
    }

    /// Bumped by every face added: a holder of a copy knows when it is out of date.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// Every face, as the scripts' process is told of them.
    pub(crate) fn script_view(&self) -> Vec<ScriptFont> {
        let mut out = Vec::new();
        for (family, faces) in &self.families {
            for f in faces {
                out.push(ScriptFont {
                    family: family.clone(),
                    weight_min: f.weight_min,
                    weight_max: f.weight_max,
                    italic: f.italic,
                    slant: f.slant,
                    stretch_min: f.stretch_min,
                    stretch_max: f.stretch_max,
                    ranges: f.ranges.clone(),
                    bytes: f.bytes.as_ref().clone(),
                });
            }
        }
        out
    }

    /// The catalog a [`script_view`](Self::script_view) describes.
    pub(crate) fn from_script_view(view: &[ScriptFont]) -> FontCatalog {
        let mut catalog = FontCatalog::default();
        for font in view {
            catalog.add_script_font(font);
        }
        catalog
    }

    fn add(&mut self, family: &str, face: CatalogFace) {
        self.generation += 1;
        self.families.entry(family.to_string()).or_default().push(face);
    }
}

/// How far `weight` is from what the face covers.
fn weight_distance(face: &CatalogFace, weight: u16) -> u16 {
    if weight < face.weight_min {
        face.weight_min - weight
    } else {
        weight.saturating_sub(face.weight_max)
    }
}

/// CSS font matching, simplified: the style first, then the nearest weight (the heavier for a
/// bold request between equally near ones). Every face of the family with that style and weight
/// range is returned, the one declared last first, with the code points it covers.
fn best_of(faces: &[CatalogFace], weight: u16, italic: bool) -> Option<&CatalogFace> {
    faces.iter().min_by_key(|f| {
        (
            u8::from(f.italic != italic),
            weight_distance(f, weight),
            if weight > 400 { u16::MAX - f.weight_max } else { f.weight_min },
        )
    })
}

fn best_faces(faces: &[CatalogFace], req: &FaceRequest) -> Vec<blueice_font::web::VariedMember> {
    let Some(best) = best_of(faces, req.weight, req.slant != Slant::Upright) else {
        return Vec::new();
    };
    faces
        .iter()
        .rev()
        .filter(|f| (f.italic, f.weight_min, f.weight_max) == (best.italic, best.weight_min, best.weight_max))
        .map(|f| (f.id, f.ranges.clone(), req.variation((f.weight_min, f.weight_max), (f.stretch_min, f.stretch_max), f.slant)))
        .collect()
}

/// How a face leans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Slant {
    Upright,
    /// `font-style: italic`.
    Italic,
    /// `font-style: oblique <angle>`: the angle in tenths of a degree (leaning right is positive).
    Oblique(i16),
}

/// What a text asks of the face it is drawn in: everything that can move a variable face's axes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FaceRequest {
    pub weight: u16,
    pub slant: Slant,
    /// `font-stretch` as a percentage in tenths.
    pub stretch: u16,
    /// The font size in tenths of a pixel, for the optical size.
    pub size: u16,
    /// Whether the optical size follows the font size (`font-optical-sizing: auto`).
    pub optical: bool,
    /// `font-variation-settings`: axis tag and value in thousandths.
    pub settings: Vec<(String, i64)>,
}

impl FaceRequest {
    pub(crate) fn new(weight: u16, italic: bool) -> FaceRequest {
        FaceRequest {
            weight,
            slant: if italic { Slant::Italic } else { Slant::Upright },
            stretch: 1000,
            size: 0,
            optical: false,
            settings: Vec::new(),
        }
    }

    /// The axis values this asks of a face whose `@font-face` declares these weights and widths.
    pub(crate) fn variation(&self, weights: (u16, u16), stretches: (u16, u16), slants: Option<(i16, i16)>) -> blueice_font::Variation {
        let held = |angle: i16| slants.map_or(angle, |(lo, hi)| angle.clamp(lo, hi));
        let mut v = blueice_font::Variation {
            wght: Some(self.weight.clamp(weights.0, weights.1)),
            wdth: Some(self.stretch.clamp(stretches.0, stretches.1)),
            ..blueice_font::Variation::NONE
        };
        match self.slant {
            Slant::Upright => {}
            Slant::Italic => {
                v.ital = Some(true);
                v.slnt = Some(-held(140));
            }
            Slant::Oblique(angle) => v.slnt = Some(-held(angle)),
        }
        if self.optical && self.size > 0 {
            v.opsz = Some(self.size);
        }
        for (tag, value) in &self.settings {
            let tenths = (*value as f64 / 100.0).round();
            match tag.as_str() {
                "wght" => v.wght = Some((*value as f64 / 1000.0).round().clamp(1.0, 1000.0) as u16),
                "wdth" => v.wdth = Some(tenths.clamp(1.0, 10000.0) as u16),
                "slnt" => v.slnt = Some(tenths.clamp(-900.0, 900.0) as i16),
                "opsz" => v.opsz = Some(tenths.clamp(1.0, 10000.0) as u16),
                "ital" => v.ital = Some(*value >= 500),
                other => {
                    if let Ok(tag) = <[u8; 4]>::try_from(other.as_bytes()) {
                        v = v.with_axis(tag, (*value).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32);
                    }
                }
            }
        }
        v
    }
}

/// The stylesheet texts of a page with the address each resolves its `url()`s against.
fn sheets(
    doc: &blueice_dom::Document,
    linked: &LinkedStyles,
    document_base: &Url,
) -> Vec<(String, Url)> {
    use blueice_dom::{NodeData, NodeId};
    fn walk(doc: &blueice_dom::Document, node: NodeId, base: &Url, out: &mut Vec<(String, Url)>) {
        if let NodeData::Element { tag_name, .. } = doc.data(node) {
            if tag_name == "style" {
                let mut css = String::new();
                for child in doc.children(node) {
                    if let NodeData::Text { data } = doc.data(child) {
                        css.push_str(data);
                    }
                }
                out.push((css, base.clone()));
            }
        }
        for child in doc.children(node) {
            walk(doc, child, base, out);
        }
    }
    let mut out = Vec::new();
    walk(doc, doc.root(), document_base, &mut out);
    let mut hrefs: Vec<&String> = linked.keys().collect();
    hrefs.sort();
    for href in hrefs {
        let base = document_base.join(href).unwrap_or_else(|_| document_base.clone());
        out.push((linked[href].clone(), base));
    }
    out
}

/// Whether a `format()` hint names something decodable.
fn usable_format(source: &FontSource) -> bool {
    source.format.as_deref().is_none_or(|f| {
        matches!(
            f,
            "woff2" | "woff" | "truetype" | "opentype" | "ttf" | "otf" | "sfnt" | "woff2-variations"
                | "woff-variations" | "truetype-variations" | "opentype-variations"
        )
    })
}

fn fetch_bytes(
    start: &str,
    review: &(dyn Fn(&str) -> bool + Sync),
    redirects: usize,
) -> Option<Vec<u8>> {
    if start.starts_with("data:") {
        let bytes = crate::images::data_url_bytes(start)?;
        return (bytes.len() <= MAX_FONT_BYTES).then_some(bytes);
    }
    let mut url = start.to_string();
    for _ in 0..=redirects {
        if !matches!(Url::parse(&url).ok()?.scheme(), "http" | "https") || !review(&url) {
            return None;
        }
        match blueice_net::fetch_subresource_hop(&url, MAX_FONT_BYTES).ok()? {
            blueice_net::SubresourceHop::Redirect { location, .. } => url = location,
            blueice_net::SubresourceHop::Response { status, body, .. } => {
                return (200..300).contains(&status).then_some(body);
            }
            blueice_net::SubresourceHop::TooLarge => return None,
        }
    }
    None
}

/// The first source of `face` that fetches and decodes.
fn load(
    face: &FontFaceRule,
    base: &Url,
    review: &(dyn Fn(&str) -> bool + Sync),
    redirects: usize,
) -> Option<(WebFontId, std::sync::Arc<Vec<u8>>)> {
    for source in face.sources.iter().filter(|s| usable_format(s)) {
        if let Some(name) = &source.local {
            // A font the page asks for by its installed name: the ones BlueIce carries.
            if let Some(bytes) = blueice_font::bundled_font_bytes(name) {
                if let Some(id) = blueice_font::web::register(bytes) {
                    return Some((id, std::sync::Arc::new(bytes.to_vec())));
                }
            }
            continue;
        }
        let url = if source.url.trim().starts_with("data:") {
            source.url.trim().to_string()
        } else {
            match base.join(source.url.trim()) {
                Ok(url) => url.to_string(),
                Err(_) => continue,
            }
        };
        if let Some(bytes) = fetch_bytes(&url, review, redirects) {
            if let Some(id) = blueice_font::web::register(&bytes) {
                return Some((id, std::sync::Arc::new(bytes)));
            }
        }
    }
    None
}

/// Fetches and decodes the fonts `html` and its fetched stylesheets ask for. `review` answers
/// whether one absolute URL may be requested.
pub(crate) fn fetch_linked_fonts(
    html: &str,
    linked: &LinkedStyles,
    document_url: &str,
    review: &(dyn Fn(&str) -> bool + Sync),
    limits: FontLimits,
) -> FontCatalog {
    let doc = blueice_html::parse(html);
    let Some(base) = crate::linked_styles::base_url(&doc, document_url) else {
        return FontCatalog::default();
    };
    let mut wanted: Vec<(FontFaceRule, Url)> = Vec::new();
    for (css, sheet_base) in sheets(&doc, linked, &base) {
        for face in blueice_css::font_faces(&css) {
            if wanted.len() < limits.faces {
                wanted.push((face, sheet_base.clone()));
            }
        }
    }
    let diag = std::env::var_os("BI_DIAG").is_some();
    if diag {
        eprintln!("FONTS {} @font-face rules wanted", wanted.len());
    }
    let loaded: Vec<Option<(WebFontId, std::sync::Arc<Vec<u8>>)>> = std::thread::scope(|scope| {
        let workers: Vec<_> = wanted
            .iter()
            .map(|(face, base)| scope.spawn(move || load(face, base, review, limits.redirects)))
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().ok().flatten())
            .collect()
    });
    let mut catalog = FontCatalog::default();
    for ((face, _), id) in wanted.into_iter().zip(loaded) {
        if diag {
            let first = face.sources.first().map_or("-", |s| s.url.as_str());
            eprintln!("FONTS {} {}-{} -> {} ({first})", face.family, face.weight_min, face.weight_max, id.as_ref().map_or("not loaded".to_string(), |i| format!("face {}", i.0.as_u32())));
        }
        let Some((id, bytes)) = id else { continue };
        catalog.add(
            &face.family,
            CatalogFace {
                weight_min: face.weight_min,
                weight_max: face.weight_max,
                italic: face.italic,
                slant: face.slant,
                stretch_min: face.stretch_min,
                stretch_max: face.stretch_max,
                ranges: face.unicode_ranges,
                id,
                bytes,
            },
        );
    }
    catalog
}

/// The numeric weight of a computed `font-weight`.
fn weight_of(style: &blueice_css::ComputedStyle) -> u16 {
    use blueice_css::Value;
    match style.other.get("font-weight") {
        Some(Value::Number(n)) => n.clamp(1.0, 1000.0) as u16,
        Some(Value::Keyword(k)) if k == "bold" || k == "bolder" => 700,
        Some(Value::Keyword(k)) if k == "lighter" => 300,
        _ => 400,
    }
}

/// `font-stretch` as a percentage in tenths.
fn stretch_of(style: &blueice_css::ComputedStyle) -> u16 {
    use blueice_css::Value;
    let percent = match style.other.get("font-stretch") {
        Some(Value::Percentage(p)) => *p,
        Some(Value::Keyword(k)) => match k.as_str() {
            "ultra-condensed" => 50.0,
            "extra-condensed" => 62.5,
            "condensed" => 75.0,
            "semi-condensed" => 87.5,
            "semi-expanded" => 112.5,
            "expanded" => 125.0,
            "extra-expanded" => 150.0,
            "ultra-expanded" => 200.0,
            _ => 100.0,
        },
        _ => 100.0,
    };
    (percent * 10.0).round().clamp(10.0, 10000.0) as u16
}

/// Which of bold (1) and italic (2) the style lets the engine make by drawing
/// (`font-synthesis`, `-weight`, `-style`).
fn synthesis_allowed(style: &blueice_css::ComputedStyle) -> u8 {
    use blueice_css::{Token, Value};
    let (mut weight, mut slant) = (true, true);
    match style.other.get("font-synthesis") {
        Some(Value::Keyword(k)) => match k.as_str() {
            "none" | "small-caps" => (weight, slant) = (false, false),
            "weight" => slant = false,
            "style" => weight = false,
            _ => {}
        },
        Some(Value::Unparsed(tokens)) => {
            let has = |word: &str| tokens.iter().any(|t| matches!(t, Token::Ident(i) if i.eq_ignore_ascii_case(word)));
            (weight, slant) = (has("weight"), has("style"));
        }
        _ => {}
    }
    let off = |name: &str| matches!(style.other.get(name), Some(Value::Keyword(k)) if k == "none");
    u8::from(weight && !off("font-synthesis-weight")) | (u8::from(slant && !off("font-synthesis-style")) << 1)
}

/// What a computed style asks of its face.
fn request_of(style: &blueice_css::ComputedStyle) -> FaceRequest {
    use blueice_css::{Token, Value};
    let mut req = FaceRequest::new(weight_of(style), false);
    req.slant = match (style.font_style.as_deref(), style.other.get("-blueice-oblique-angle")) {
        (Some("oblique"), Some(Value::Number(a))) => Slant::Oblique((a * 10.0).round().clamp(-900.0, 900.0) as i16),
        (Some("oblique"), _) => Slant::Oblique(140),
        (Some("italic"), _) => Slant::Italic,
        _ => Slant::Upright,
    };
    req.stretch = stretch_of(style);
    req.size = (style.font_size_px * 10.0).round().clamp(1.0, 10000.0) as u16;
    req.optical = !matches!(style.other.get("font-optical-sizing"), Some(Value::Keyword(k)) if k == "none");
    if let Some(Value::Unparsed(tokens)) = style.other.get("font-variation-settings") {
        let mut tag: Option<String> = None;
        for token in tokens {
            match (token, &tag) {
                (Token::QuotedString(t), None) if t.len() == 4 => tag = Some(t.clone()),
                (Token::Number(n), Some(t)) => {
                    req.settings.push((t.clone(), (n * 1000.0).round() as i64));
                    tag = None;
                }
                (Token::Whitespace, _) => {}
                (Token::Comma, _) => tag = None,
                _ => {}
            }
        }
        req.settings.truncate(16);
    }
    req
}

/// Sets, on every styled element whose `font-family` the page has a face for, the face its text
/// is drawn with (`-blueice-web-font`: layout and paint read it), and clears it where there is
/// none. The elements are those of `styles`; `catalog` is the page's.
pub(crate) fn assign_faces(
    catalog: &FontCatalog,
    styles: &mut HashMap<blueice_dom::NodeId, blueice_css::ComputedStyle>,
) {
    use blueice_css::Value;
    if catalog.is_empty() {
        return;
    }
    let mut chosen: HashMap<(String, FaceRequest), (Option<WebFontId>, u8)> = HashMap::new();
    for style in styles.values_mut() {
        let Some(families) = style.font_family.as_deref() else {
            style.other.remove("-blueice-web-font");
            style.other.remove("-blueice-web-synth");
            continue;
        };
        let key = (families.to_string(), request_of(style));
        let (face, synthesis) = *chosen
            .entry(key.clone())
            .or_insert_with(|| catalog.face_for_request(&key.0, &key.1));
        let synthesis = synthesis & synthesis_allowed(style);
        match face {
            Some(id) => {
                style
                    .other
                    .insert("-blueice-web-font".to_string(), Value::Number(f64::from(id.as_u32())));
                if synthesis == 0 {
                    style.other.remove("-blueice-web-synth");
                } else {
                    style
                        .other
                        .insert("-blueice-web-synth".to_string(), Value::Number(f64::from(synthesis)));
                }
            }
            None => {
                style.other.remove("-blueice-web-font");
                style.other.remove("-blueice-web-synth");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const REGULAR: &[u8] = include_bytes!("../../font/tests/data/roboto-latin-400.woff2");
    const BOLD: &[u8] = include_bytes!("../../font/tests/data/roboto-latin-700.woff2");
    const WOFF: &[u8] = include_bytes!("../../font/tests/data/roboto-latin-400.woff");

    fn base64(bytes: &[u8]) -> String {
        const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = (u32::from(chunk[0]) << 16)
                | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
                | u32::from(*chunk.get(2).unwrap_or(&0));
            out.push(TABLE[(n >> 18) as usize & 63] as char);
            out.push(TABLE[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
            out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
        }
        out
    }

    fn data_url(bytes: &[u8], mime: &str) -> String {
        format!("data:{mime};base64,{}", base64(bytes))
    }

    fn catalog(html: &str) -> FontCatalog {
        fetch_linked_fonts(
            html,
            &LinkedStyles::new(),
            "https://example.com/p",
            &|_| true,
            DEFAULT_FONT_LIMITS,
        )
    }

    fn id_of(bytes: &[u8]) -> WebFontId {
        blueice_font::web::register(bytes).unwrap()
    }

    #[test]
    fn faces_in_a_pages_style_elements_are_loaded_and_chosen_by_family_weight_and_style() {
        let html = format!(
            "<style>\
             @font-face {{ font-family: 'Web Test'; src: url({}) format('woff2'); font-weight: 400; }}\
             @font-face {{ font-family: 'Web Test'; src: url({}) format('woff2'); font-weight: 700; }}\
             </style><p>x</p>",
            data_url(REGULAR, "font/woff2"),
            data_url(BOLD, "font/woff2")
        );
        let c = catalog(&html);
        let (regular, bold) = (id_of(REGULAR), id_of(BOLD));
        assert_eq!(c.face_for("web test", 400, false), Some(regular));
        assert_eq!(c.face_for("web test", 700, false), Some(bold));
        assert_eq!(c.face_for("web test", 500, false), Some(regular), "nearest weight");
        assert_eq!(c.face_for("web test", 600, false), Some(bold), "a tie goes to the heavier for a bold request");
        assert_eq!(c.face_for("web test", 400, true), Some(regular), "no italic: the upright one");
        // The first family the page has a face for; unknown ones are passed over.
        assert_eq!(c.face_for("helvetica neue,web test,sans-serif", 400, false), Some(regular));
        // A system family first is satisfied by the system.
        assert_eq!(c.face_for("system-ui,web test", 400, false), None);
        assert_eq!(c.face_for("sans-serif", 400, false), None);
        assert_eq!(c.face_for("nope", 400, false), None);
    }

    #[test]
    fn a_local_source_is_a_bundled_font_by_name_and_other_names_fall_through() {
        let html = format!(
            "<style>@font-face {{ font-family: Loc; src: local('Arial'), local('DejaVu Sans Bold'); }}\
             @font-face {{ font-family: Pick; src: local('Missing Font'), url({}) format('woff2'); }}\
             @font-face {{ font-family: None; src: local('Missing Font'); }}</style>",
            data_url(REGULAR, "font/woff2")
        );
        let c = catalog(&html);
        let loc = c.face_for("loc", 400, false).expect("local() of a bundled font loads");
        let bold = blueice_font::web::register(blueice_font::bundled_font_bytes("DejaVuSans-Bold").unwrap()).unwrap();
        assert_eq!(font_of(loc), font_of(bold), "the same file");
        assert!(c.face_for("pick", 400, false).is_some(), "an unknown local() name falls to the url");
        assert!(c.face_for("none", 400, false).is_none(), "nothing to load");
    }

    #[test]
    fn a_face_that_cannot_be_used_falls_through_to_the_next_source() {
        let html = format!(
            "<style>@font-face {{ font-family: a; src: url(x.eot?#i) format('embedded-opentype'), \
             url({}) format('woff2'), url({}) format('woff'); }}</style>",
            data_url(b"not a font", "font/woff2"),
            data_url(WOFF, "font/woff"),
        );
        let c = catalog(&html);
        assert_eq!(c.face_for("a", 400, false), Some(id_of(WOFF)));
    }

    #[test]
    fn a_face_the_review_refuses_is_not_fetched_and_the_rest_still_load() {
        let seen = AtomicUsize::new(0);
        let html = "<style>@font-face { font-family: a; src: url(https://bad.test/a.woff2); }\
                    @font-face { font-family: b; src: url(https://good.test/b.woff2); }</style>";
        let c = fetch_linked_fonts(
            html,
            &LinkedStyles::new(),
            "https://example.com/p",
            &|url| {
                seen.fetch_add(1, Ordering::SeqCst);
                !url.contains("bad.test")
            },
            DEFAULT_FONT_LIMITS,
        );
        assert!(seen.load(Ordering::SeqCst) >= 1, "the review was asked");
        assert!(c.face_for("a", 400, false).is_none());
    }

    #[test]
    fn relative_urls_resolve_against_the_sheet_that_names_them() {
        let mut linked = LinkedStyles::new();
        linked.insert(
            "/css/site.css".to_string(),
            "@font-face { font-family: a; src: url(../fonts/a.woff2); }".to_string(),
        );
        let urls = std::sync::Mutex::new(Vec::new());
        let _ = fetch_linked_fonts(
            "<p>x</p>",
            &linked,
            "https://example.com/dir/page",
            &|url| {
                urls.lock().unwrap().push(url.to_string());
                false
            },
            DEFAULT_FONT_LIMITS,
        );
        assert_eq!(*urls.lock().unwrap(), ["https://example.com/fonts/a.woff2"]);
    }

    #[test]
    fn a_page_cannot_ask_for_more_faces_than_the_limit() {
        let mut css = String::new();
        for i in 0..40 {
            css.push_str(&format!(
                "@font-face {{ font-family: f{i}; src: url(https://e.test/{i}.woff2); }}"
            ));
        }
        let asked = AtomicUsize::new(0);
        let _ = fetch_linked_fonts(
            &format!("<style>{css}</style>"),
            &LinkedStyles::new(),
            "https://example.com/p",
            &|_| {
                asked.fetch_add(1, Ordering::SeqCst);
                false
            },
            FontLimits { faces: 5, redirects: 1 },
        );
        assert_eq!(asked.load(Ordering::SeqCst), 5);
    }

    /// The address of the face that draws `ch` for text in `families` (faces are compared by
    /// identity).
    fn drawn_by(c: &FontCatalog, families: &str, ch: char) -> usize {
        let font = blueice_font::font_for_char_in(ch, false, false, c.face_for(families, 400, false));
        font.address()
    }

    fn font_of(id: WebFontId) -> usize {
        blueice_font::web::get(id).unwrap().address()
    }

    fn bundled(ch: char) -> usize {
        blueice_font::Glyphs::Static(blueice_font::font_for_char(ch, false, false)).address()
    }

    #[test]
    fn a_family_split_by_unicode_range_draws_each_character_with_the_face_that_covers_it() {
        let html = format!(
            "<style>\
             @font-face {{ font-family: s; src: url({}); unicode-range: U+0041-005A; }}\
             @font-face {{ font-family: s; src: url({}); unicode-range: U+0061-007A; }}</style>",
            data_url(BOLD, "font/woff2"),
            data_url(REGULAR, "font/woff2")
        );
        let c = catalog(&html);
        assert_eq!(drawn_by(&c, "s", 'A'), font_of(id_of(BOLD)));
        assert_eq!(drawn_by(&c, "s", 'a'), font_of(id_of(REGULAR)));
        assert_eq!(drawn_by(&c, "s", '7'), bundled('7'), "covered by no face: the bundled font");
    }

    #[test]
    fn where_ranges_overlap_the_face_declared_last_draws_the_character() {
        let html = format!(
            "<style>\
             @font-face {{ font-family: s; src: url({}); }}\
             @font-face {{ font-family: s; src: url({}); unicode-range: U+0041-005A; }}</style>",
            data_url(REGULAR, "font/woff2"),
            data_url(BOLD, "font/woff2")
        );
        let c = catalog(&html);
        assert_eq!(drawn_by(&c, "s", 'A'), font_of(id_of(BOLD)));
        assert_eq!(drawn_by(&c, "s", 'a'), font_of(id_of(REGULAR)));
    }

    #[test]
    fn the_families_after_the_first_cover_what_it_does_not_until_a_system_family() {
        let html = format!(
            "<style>\
             @font-face {{ font-family: fa; src: url({}); unicode-range: U+0041-005A; }}\
             @font-face {{ font-family: fb; src: url({}); }}</style>",
            data_url(BOLD, "font/woff2"),
            data_url(REGULAR, "font/woff2")
        );
        let c = catalog(&html);
        assert_eq!(drawn_by(&c, "fa,fb", 'A'), font_of(id_of(BOLD)));
        assert_eq!(drawn_by(&c, "fa,fb", 'a'), font_of(id_of(REGULAR)), "fb covers what fa does not");
        // A system family in between: the later web family is not reached.
        assert_eq!(drawn_by(&c, "fa,sans-serif,fb", 'a'), bundled('a'));
        // A family alone is its own face.
        assert_eq!(c.face_for("fb", 400, false), Some(id_of(REGULAR)));
    }

    #[test]
    fn faces_of_other_weights_stay_out_of_the_set_of_the_best_weight() {
        let html = format!(
            "<style>\
             @font-face {{ font-family: w; src: url({}); font-weight: 400; unicode-range: U+0041-005A; }}\
             @font-face {{ font-family: w; src: url({}); font-weight: 700; unicode-range: U+0061-007A; }}</style>",
            data_url(REGULAR, "font/woff2"),
            data_url(BOLD, "font/woff2")
        );
        let c = catalog(&html);
        let at400 = c.face_for("w", 400, false);
        let addr = |ch| blueice_font::font_for_char_in(ch, false, false, at400).address();
        assert_eq!(addr('A'), font_of(id_of(REGULAR)));
        assert_eq!(addr('a'), bundled('a'), "the 700 face is not part of the 400 set");
    }

    fn page_with_fonts(html: &str) -> crate::Page {
        let mut page = crate::Page::new(400.0, 300.0);
        page.pending_linked_fonts = catalog(html);
        page.load_html_str(html, Some("https://example.com/p".to_string()));
        page
    }

    fn runs(page: &crate::Page) -> Vec<(String, Option<u32>, f64)> {
        page.render()
            .commands
            .iter()
            .filter_map(|c| match c {
                blueice_paint::PaintCommand::Text { text, face, x, .. } => {
                    Some((text.clone(), *face, *x))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_page_draws_and_measures_its_text_in_the_fonts_it_brought() {
        let html = format!(
            "<style>\
             @font-face {{ font-family: 'Web Test'; src: url({}) format('woff2'); font-weight: 400; }}\
             @font-face {{ font-family: 'Web Test'; src: url({}) format('woff2'); font-weight: 700; }}\
             body {{ margin: 0 }} p {{ margin: 0; font-family: 'Web Test', sans-serif }}\
             .fallback {{ font-family: Nope, 'Web Test' }} .sys {{ font-family: sans-serif }}\
             </style><body><p>Hello world</p><p style='font-weight:700'>Heavy</p>\
             <p class=fallback>Third</p><p class=sys>Plain</p></body>",
            data_url(REGULAR, "font/woff2"),
            data_url(BOLD, "font/woff2")
        );
        let page = page_with_fonts(&html);
        let (regular, bold) = (id_of(REGULAR).as_u32(), id_of(BOLD).as_u32());
        let got = runs(&page);
        let face_of = |word: &str| got.iter().find(|(t, ..)| t == word).map(|r| r.1);
        assert_eq!(face_of("Hello"), Some(Some(regular)));
        assert_eq!(face_of("Heavy"), Some(Some(bold)));
        assert_eq!(face_of("Third"), Some(Some(regular)), "the second family of the list");
        assert_eq!(face_of("Plain"), Some(None), "a system family: the bundled font");
        // And laid out with the face's own widths: the second word starts after the first
        // word and a space of that face.
        let x_of = |word: &str| got.iter().find(|(t, ..)| t == word).map(|r| r.2).unwrap();
        let own = |t: &str| blueice_font::measure_text_width_in(t, 16.0, false, false, Some(id_of(REGULAR)));
        assert!((x_of("world") - x_of("Hello") - own("Hello") - own(" ")).abs() < 0.01);
    }

    #[test]
    fn a_page_lays_out_text_character_by_character_across_the_faces_of_a_split_family() {
        let html = format!(
            "<style>\
             @font-face {{ font-family: s; src: url({}); unicode-range: U+0041-005A; }}\
             @font-face {{ font-family: s; src: url({}); unicode-range: U+0061-007A, U+0020; }}\
             body {{ margin: 0 }} p {{ margin: 0; font-family: s }}</style><body><p>Ab cd</p></body>",
            data_url(BOLD, "font/woff2"),
            data_url(REGULAR, "font/woff2")
        );
        let page = page_with_fonts(&html);
        let set = page.linked_fonts.face_for("s", 400, false);
        assert!(set.is_some());
        let got = runs(&page);
        assert_eq!(got.iter().map(|r| r.1).collect::<Vec<_>>(), [set.map(WebFontId::as_u32); 2]);
        let x_of = |word: &str| got.iter().find(|(t, ..)| t == word).map(|r| r.2).unwrap();
        let w = |t: &str| blueice_font::measure_text_width_in(t, 16.0, false, false, set);
        assert!((x_of("cd") - (w("Ab") + w(" "))).abs() < 0.01, "{}", x_of("cd"));
        // The mixed word is wider than the same letters in either single face.
        let single = |id| blueice_font::measure_text_width_in("Ab", 16.0, false, false, Some(id));
        let mixed = w("Ab");
        assert!(mixed != single(id_of(BOLD)) && mixed != single(id_of(REGULAR)));
    }

    #[test]
    fn a_page_without_fonts_is_untouched_and_text_follows_a_changed_family() {
        let html = format!(
            "<style>@font-face {{ font-family: w; src: url({}) format('woff2'); }}\
             body {{ margin: 0 }} p {{ font-family: sans-serif }}</style><body><p id=a>abc</p>\
             <script>window.swap = () => {{ document.getElementById('a').style.fontFamily = 'w'; }}</script></body>",
            data_url(REGULAR, "font/woff2")
        );
        let mut page = crate::Page::new(400.0, 300.0);
        page.set_web_scripts(true);
        page.pending_linked_fonts = catalog(&html);
        page.load_html_str(&html, Some("https://example.com/p".to_string()));
        assert_eq!(runs(&page)[0].1, None);
        page.web_eval("window.swap()");
        page.settle();
        assert_eq!(runs(&page)[0].1, Some(id_of(REGULAR).as_u32()), "restyled to the page's face");
        let plain = crate::Page::new(400.0, 300.0);
        assert!(plain.linked_fonts.is_empty());
    }

    fn script_page(html: &str) -> crate::Page {
        let mut page = crate::Page::new(400.0, 300.0);
        page.set_web_scripts(true);
        page.load_html_str(html, Some("https://example.com/p".to_string()));
        page.settle();
        page
    }

    fn run_scripts(page: &mut crate::Page) {
        for _ in 0..80 {
            page.tick_web();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn a_font_face_a_script_loads_and_adds_to_the_document_is_used_by_the_page() {
        let html = format!(
            "<style>body {{ margin: 0 }} p {{ margin: 0; font-family: Dyn, sans-serif }}</style>\
             <body><p>Hello</p><script>\
             const f = new FontFace('Dyn', 'url({})', {{ weight: '400' }});\
             console.log('STATUS0 ' + f.status);\
             f.load().then((g) => {{\
               document.fonts.add(g);\
               console.log('STATUS1 ' + f.status + ' ' + (g === f) + ' ' + document.fonts.has(f) + ' ' + document.fonts.check('16px Dyn'));\
             }});\
             </script></body>",
            data_url(REGULAR, "font/woff2")
        );
        let mut page = script_page(&html);
        assert_eq!(runs(&page)[0].1, None, "before the font loads: the bundled one");
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        assert!(log.contains("STATUS0 unloaded"), "{log}");
        assert!(log.contains("STATUS1 loaded true true true"), "{log}");
        let face = runs(&page)[0].1;
        assert!(face.is_some(), "the text is drawn in the loaded face: {log}");
        assert_eq!(face, page.linked_fonts.face_for("dyn", 400, false).map(WebFontId::as_u32));
    }

    #[test]
    fn a_face_from_bytes_waits_for_the_document_and_a_bad_one_rejects() {
        let html = format!(
            "<style>body {{ margin: 0 }} p {{ margin: 0; font-family: Raw }}</style>\
             <body><p>Hi</p><script>\
             const bin = atob('{}'); const bytes = new Uint8Array(bin.length);\
             for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);\
             const f = new FontFace('Raw', bytes.buffer);\
             console.log('LOADED_EARLY ' + f.status);\
             f.loaded.then(() => console.log('LOADED ' + f.status));\
             document.fonts.add(f);\
             new FontFace('Bad', new Uint8Array([1, 2, 3, 4]).buffer).load()\
               .then(() => console.log('BAD ok'), (e) => console.log('BAD rejected ' + e.name));\
             </script></body>",
            base64(REGULAR)
        );
        let mut page = script_page(&html);
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        assert!(log.contains("LOADED_EARLY loaded"), "binary data is available at once: {log}");
        assert!(log.contains("LOADED loaded"), "{log}");
        assert!(log.contains("BAD rejected SyntaxError"), "{log}");
        assert!(runs(&page)[0].1.is_some(), "{log}");
    }

    #[test]
    fn a_face_that_is_not_added_to_the_document_does_not_style_the_page() {
        let html = format!(
            "<style>body {{ margin: 0 }} p {{ margin: 0; font-family: Lone }}</style>\
             <body><p>Hi</p><script>\
             new FontFace('Lone', 'url({})').load().then(() => console.log('DONE'));\
             </script></body>",
            data_url(REGULAR, "font/woff2")
        );
        let mut page = script_page(&html);
        run_scripts(&mut page);
        assert!(page.web_console().join("\n").contains("DONE"));
        assert_eq!(runs(&page)[0].1, None);
    }

    #[test]
    fn document_fonts_load_loads_the_added_faces_of_a_family_and_check_says_when_they_are_ready() {
        let html = format!(
            "<style>body {{ margin: 0 }} p {{ margin: 0; font-family: 'Lazy Face' }}</style>\
             <body><p>Hi</p><script>\
             document.fonts.add(new FontFace('Lazy Face', 'url({})'));\
             console.log('CHECK0 ' + document.fonts.check('italic 16px \"Lazy Face\", serif') + ' ' + document.fonts.check('16px Other'));\
             document.fonts.load('16px \"Lazy Face\"').then((faces) =>\
               console.log('CHECK1 ' + faces.length + ' ' + document.fonts.check('16px \"Lazy Face\"') + ' ' + document.fonts.status));\
             </script></body>",
            data_url(REGULAR, "font/woff2")
        );
        let mut page = script_page(&html);
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        assert!(log.contains("CHECK0 false true"), "{log}");
        assert!(log.contains("CHECK1 1 true loaded"), "{log}");
        assert!(runs(&page)[0].1.is_some(), "{log}");
    }

    #[test]
    fn canvas_text_is_drawn_and_measured_in_a_face_the_script_added() {
        let html = format!(
            "<body><canvas id=c width=80 height=30></canvas><script>\
             const f = new FontFace('Dyn', 'url({})');\
             f.load().then(() => {{\
               document.fonts.add(f);\
               const g = document.getElementById('c').getContext('2d');\
               g.font = '20px Dyn';\
               const own = g.measureText('Hello').width;\
               g.font = '20px sans-serif';\
               const bundled = g.measureText('Hello').width;\
               g.font = '20px Dyn'; g.fillStyle = '#ff0000'; g.fillText('Hello', 2, 22);\
               const d = g.getImageData(0, 0, 80, 30).data; let ink = 0;\
               for (let i = 3; i < d.length; i += 4) if (d[i] > 0) ink++;\
               console.log('CANVAS ' + (own !== bundled) + ' ' + (ink > 20));\
             }});\
             </script></body>",
            data_url(REGULAR, "font/woff2")
        );
        let mut page = script_page(&html);
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        assert!(log.contains("CANVAS true true"), "{log}");
    }

    #[test]
    fn ready_and_the_loading_events_follow_the_faces_a_script_has_loading() {
        let html = format!(
            "<body><p>Hi</p><script>\
             const log = (m) => console.log(m);\
             log('STATUS0 ' + document.fonts.status);\
             document.fonts.ready.then(() => log('READY0'));\
             document.fonts.addEventListener('loading', () => log('EV loading ' + document.fonts.status));\
             document.fonts.onloadingdone = (e) => log('EV done ' + e.fontfaces.length + ' ' + document.fonts.status);\
             document.fonts.addEventListener('loadingerror', (e) => log('EV error ' + e.fontfaces.length));\
             const f = new FontFace('Evt', 'url({})');\
             document.fonts.add(f);\
             f.load();\
             log('STATUS1 ' + document.fonts.status);\
             const r = document.fonts.ready;\
             r.then(() => {{\
               log('READY1 ' + document.fonts.status);\
               const bad = new FontFace('Broken', 'url(data:font/woff2;base64,AAAA)');\
               document.fonts.add(bad); bad.load();\
             }});\
             </script></body>",
            data_url(REGULAR, "font/woff2")
        );
        let mut page = script_page(&html);
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        assert!(log.contains("STATUS0 loaded"), "{log}");
        assert!(log.contains("READY0"), "ready is resolved when nothing is loading: {log}");
        assert!(log.contains("EV loading loading"), "{log}");
        assert!(log.contains("STATUS1 loading"), "{log}");
        assert!(log.contains("EV done 1 loaded"), "{log}");
        assert!(log.contains("READY1 loaded"), "{log}");
        assert!(log.contains("EV error 1"), "a face that fails fires loadingerror: {log}");
        let at = |needle: &str| log.find(needle).unwrap_or(usize::MAX);
        assert!(at("EV loading") < at("EV done") && at("EV done") < at("READY1"), "{log}");
    }

    const OPEN_SANS: &[u8] = include_bytes!("../../font/tests/data/opensans-latin-wdth.woff2");
    const INTER_OPSZ: &[u8] = include_bytes!("../../font/tests/data/inter-latin-opsz.woff2");

    fn run_width(css_face: &str, family_css: &str, file: &[u8]) -> f64 {
        let html = format!(
            "<style>@font-face {{ font-family: V; src: url({}) format('woff2'); font-weight: 100 900; {css_face} }}\
             body {{ margin: 0 }} p {{ margin: 0; font-family: V; {family_css} }}</style><body><p>Hamburgefonstiv<span>|</span></p></body>",
            data_url(file, "font/woff2")
        );
        // The second run starts where the first ends.
        runs(&page_with_fonts(&html))[1].2
    }

    #[test]
    fn font_stretch_moves_a_width_axis_within_the_range_the_face_declares() {
        let wide = run_width("font-stretch: 75% 100%", "font-stretch: 100%", OPEN_SANS);
        let narrow = run_width("font-stretch: 75% 100%", "font-stretch: 75%", OPEN_SANS);
        assert!(narrow < wide * 0.9, "{narrow} vs {wide}");
        let keyword = run_width("font-stretch: 75% 100%", "font-stretch: condensed", OPEN_SANS);
        assert!((keyword - narrow).abs() < 0.01, "condensed is 75%");
        let undeclared = run_width("", "font-stretch: 75%", OPEN_SANS);
        assert!((undeclared - wide).abs() < 0.01, "a face that declares no range stays at its normal width");
        let clamped = run_width("font-stretch: 75% 100%", "font-stretch: 50%", OPEN_SANS);
        assert!((clamped - narrow).abs() < 0.01, "past the declared range is its end");
    }

    #[test]
    fn font_variation_settings_reach_the_axes_and_win_over_the_properties() {
        let base = run_width("font-stretch: 75% 100%", "", OPEN_SANS);
        let set = run_width("font-stretch: 75% 100%", "font-variation-settings: 'wdth' 75", OPEN_SANS);
        assert!(set < base * 0.9, "{set} vs {base}");
        let beats = run_width("font-stretch: 75% 100%", "font-stretch: 100%; font-variation-settings: \"wdth\" 75", OPEN_SANS);
        assert!((beats - set).abs() < 0.01);
        let weight = run_width("", "font-variation-settings: \"wght\" 900", OPEN_SANS);
        let regular = run_width("", "", OPEN_SANS);
        assert!(weight > regular, "the weight axis too: {weight} vs {regular}");
        let unknown = run_width("", "font-variation-settings: \"ZZZZ\" 5", OPEN_SANS);
        assert!((unknown - regular).abs() < 0.01, "an axis the face lacks does nothing");
    }

    #[test]
    fn optical_size_follows_the_font_size_unless_it_is_switched_off() {
        let at = |css: &str| run_width("", css, INTER_OPSZ);
        // Same text at two sizes: the ratio of widths differs from the ratio of sizes only
        // because the face is drawn with another optical size.
        let (small, large) = (at("font-size: 14px"), at("font-size: 28px"));
        let (small_off, large_off) = (
            at("font-size: 14px; font-optical-sizing: none"),
            at("font-size: 28px; font-optical-sizing: none"),
        );
        assert!((large_off / small_off - 2.0).abs() < 0.01, "off: widths scale with size");
        assert!((large / small - 2.0).abs() > 0.01, "on: a large size takes the display cut");
    }

    #[test]
    fn a_text_request_becomes_the_variation_the_face_is_drawn_with() {
        use blueice_font::Variation;
        let mut req = FaceRequest::new(650, false);
        req.stretch = 875;
        req.size = 160;
        req.optical = true;
        let v = req.variation((600, 700), (750, 1000), None);
        assert_eq!(v, Variation { wght: Some(650), wdth: Some(875), opsz: Some(160), ..Variation::NONE });
        req.optical = false;
        req.stretch = 1250;
        assert_eq!(req.variation((600, 700), (750, 1000), None).wdth, Some(1000), "clamped to the declared range");
        assert_eq!(req.variation((600, 700), (750, 1000), None).opsz, None);
        // Slants: an angle leans the other way on the axis; the italic keyword also asks the
        // italic switch, with the usual 14 degrees if the face only slants.
        req.slant = Slant::Oblique(100);
        let v = req.variation((400, 400), (1000, 1000), None);
        assert_eq!((v.slnt, v.ital), (Some(-100), None));
        req.slant = Slant::Italic;
        let v = req.variation((400, 400), (1000, 1000), None);
        assert_eq!((v.slnt, v.ital), (Some(-140), Some(true)));
        req.slant = Slant::Upright;
        let v = req.variation((400, 400), (1000, 1000), None);
        assert_eq!((v.slnt, v.ital), (None, None));
        // A face that declares the angles it leans through holds the request to them.
        req.slant = Slant::Oblique(200);
        assert_eq!(req.variation((400, 400), (1000, 1000), Some((50, 150))).slnt, Some(-150), "past the far end");
        req.slant = Slant::Oblique(10);
        assert_eq!(req.variation((400, 400), (1000, 1000), Some((50, 150))).slnt, Some(-50), "short of the near end");
        req.slant = Slant::Oblique(100);
        assert_eq!(req.variation((400, 400), (1000, 1000), Some((50, 150))).slnt, Some(-100), "inside it");
        req.slant = Slant::Italic;
        assert_eq!(req.variation((400, 400), (1000, 1000), Some((50, 100))).slnt, Some(-100), "the italic's 14 degrees too");
        req.slant = Slant::Upright;
        // The page's settings come last and are not held to the declared ranges.
        req.settings = vec![("wdth".into(), 60_000), ("slnt".into(), -5_000), ("ital".into(), 1_000), ("wght".into(), 905_400)];
        let v = req.variation((400, 400), (1000, 1000), None);
        assert_eq!((v.wdth, v.slnt, v.ital, v.wght), (Some(600), Some(-50), Some(true), Some(905)));
        // Any other four-letter axis is passed on in thousandths, at most four of them.
        req.settings = vec![("GRAD".into(), 150_000), ("XTRA".into(), 468_500), ("GRAD".into(), -25_000)];
        let v = req.variation((400, 400), (1000, 1000), None);
        assert_eq!(v.custom, [Some((*b"GRAD", -25_000)), Some((*b"XTRA", 468_500)), None, None], "the later value wins");
        req.settings = ["AAAA", "BBBB", "CCCC", "DDDD", "EEEE"].iter().map(|t| (t.to_string(), 1_000)).collect();
        assert_eq!(req.variation((400, 400), (1000, 1000), None).custom.iter().flatten().count(), 4);
    }

    #[test]
    fn a_page_asking_for_an_axis_a_face_does_not_have_draws_it_unchanged() {
        let plain = run_width("", "", OPEN_SANS);
        let asked = run_width("", "font-variation-settings: \"GRAD\" 100, \"SOFT\" 50", OPEN_SANS);
        assert!((plain - asked).abs() < 0.01);
    }

    #[test]
    fn document_fonts_lists_the_angles_of_an_oblique_stylesheet_face() {
        let html = format!(
            "<style>@font-face {{ font-family: A; font-style: oblique 5deg 15deg; src: url({0}) format('woff2'); }}\
             @font-face {{ font-family: B; font-style: italic; src: url({0}) format('woff2'); }}\
             @font-face {{ font-family: C; font-style: oblique; src: url({0}) format('woff2'); }}</style>\
             <body><script>console.log('STYLES ' + [...document.fonts].map((f) => f.family + '=' + f.style).sort().join(';'));</script></body>",
            data_url(OPEN_SANS, "font/woff2")
        );
        let mut page = crate::Page::new(400.0, 300.0);
        page.set_web_scripts(true);
        page.pending_linked_fonts = catalog(&html);
        page.load_html_str(&html, Some("https://example.com/p".to_string()));
        page.settle();
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        assert!(log.contains("STYLES a=oblique 5deg 15deg;b=italic;c=oblique 14deg"), "{log}");
    }

    #[test]
    fn a_font_face_a_script_makes_can_declare_its_stretch() {
        let html = format!(
            "<style>body {{ margin: 0 }} p {{ margin: 0; font-family: Dyn; font-stretch: 75% }}</style>\
             <body><p>Hamburgefonstiv<span>|</span></p><script>\
             const f = new FontFace('Dyn', 'url({})', {{ weight: '300 800', stretch: '75% 100%' }});\
             document.fonts.add(f); f.load();\
             </script></body>",
            data_url(OPEN_SANS, "font/woff2")
        );
        let mut page = script_page(&html);
        run_scripts(&mut page);
        let narrow = runs(&page)[1].2;
        let wide = run_width("font-stretch: 75% 100%", "font-stretch: 100%", OPEN_SANS);
        assert!(narrow < wide * 0.9, "{narrow} vs {wide}");
    }

    #[test]
    fn a_script_strokes_rectangles_and_measures_the_ink_of_text() {
        let html = "<body><canvas id=c width=40 height=40></canvas><script>\
             const g = document.getElementById('c').getContext('2d');\
             g.strokeStyle = '#0000ff'; g.lineWidth = 4; g.strokeRect(10, 10, 20, 20);\
             const d = g.getImageData(0, 0, 40, 40).data;\
             const a = (x, y) => d[(y * 40 + x) * 4 + 3];\
             console.log('RECT ' + a(10, 10) + ' ' + a(8, 20) + ' ' + a(11, 20) + ' ' + a(20, 20) + ' ' + a(5, 20) + ' ' + a(20, 33));\
             g.font = '40px sans-serif'; const m = g.measureText('Hg');\
             console.log('BOX ' + (m.actualBoundingBoxAscent > 20) + ' ' + (m.actualBoundingBoxDescent > 5) + ' ' +\
               (m.actualBoundingBoxRight > 20) + ' ' + (m.fontBoundingBoxAscent > m.actualBoundingBoxAscent) + ' ' + (m.width > 40));\
             </script></body>";
        let mut page = script_page(html);
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        assert!(log.contains("RECT 255 255 255 0 0 0"), "the line covers 8..12 on each side, not the inside nor the outside: {log}");
        assert!(log.contains("BOX true true true true true"), "{log}");
    }

    #[test]
    fn canvas_text_takes_kerning_spacing_and_the_other_font_properties() {
        let html = "<body><canvas id=c width=100 height=40></canvas><script>\
             const g = document.getElementById('c').getContext('2d');\
             g.font = '32px sans-serif';\
             const w = (t) => g.measureText(t).width;\
             const kerned = w('AVAV');\
             g.fontKerning = 'none'; const flat = w('AVAV');\
             g.fontKerning = 'normal'; const back = w('AVAV');\
             g.fontKerning = 'bogus'; const kept = g.fontKerning;\
             g.fontKerning = 'auto';\
             g.letterSpacing = '4px'; const spaced = w('abcd'); g.letterSpacing = '0px'; const plain = w('abcd');\
             g.wordSpacing = '10px'; const words = w('a b c'); g.wordSpacing = '0px'; const nowords = w('a b c');\
             g.textRendering = 'optimizeSpeed'; const fast = w('AVAV'); g.textRendering = 'auto';\
             g.fontVariantCaps = 'small-caps'; const caps = g.fontVariantCaps; g.fontVariantCaps = 'bogus';\
             console.log('KERN ' + (kerned < flat - 1) + ' ' + (back === kerned) + ' ' + kept + ' ' + (Math.abs(fast - flat) < 0.01));\
             console.log('SPACE ' + Math.round(spaced - plain) + ' ' + Math.round(words - nowords));\
             console.log('CAPS ' + caps + ' ' + g.fontVariantCaps + ' ' + g.letterSpacing + ' ' + g.wordSpacing + ' ' + g.textRendering);\
             g.save(); g.fontKerning = 'none'; g.restore(); console.log('RESTORED ' + g.fontKerning);\
             </script></body>";
        let mut page = script_page(html);
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        assert!(log.contains("KERN true true normal true"), "{log}");
        assert!(log.contains("SPACE 16 20"), "four letters and two spaces: {log}");
        assert!(log.contains("CAPS small-caps small-caps 0px 0px auto"), "{log}");
        assert!(log.contains("RESTORED auto"), "{log}");
    }

    #[test]
    fn a_script_fills_and_strokes_paths() {
        let html = "<body><canvas id=c width=100 height=100></canvas><script>\
             const g = document.getElementById('c').getContext('2d');\
             const px = (x, y) => { const d = g.getImageData(x, y, 1, 1).data; return d[3] === 0 ? '-' : d[0] + ',' + d[1] + ',' + d[2]; };\
             const out = [];\
             g.fillStyle = '#ff0000'; g.beginPath(); g.moveTo(10, 10); g.lineTo(50, 10); g.lineTo(10, 50); g.closePath(); g.fill();\
             out.push('TRI ' + px(15, 15) + ' ' + px(45, 45));\
             g.clearRect(0, 0, 100, 100); g.fillStyle = '#00ff00'; g.beginPath(); g.arc(50, 50, 20, 0, Math.PI * 2); g.fill();\
             out.push('ARC ' + px(50, 50) + ' ' + px(50, 33) + ' ' + px(50, 25) + ' ' + px(69, 50) + ' ' + px(72, 50));\
             g.clearRect(0, 0, 100, 100); g.fillStyle = '#0000ff'; g.beginPath(); g.rect(10, 10, 80, 80); g.rect(30, 30, 40, 40); g.fill('evenodd');\
             out.push('EO ' + px(20, 20) + ' ' + px(50, 50));\
             g.clearRect(0, 0, 100, 100); g.beginPath(); g.rect(10, 10, 80, 80); g.rect(30, 30, 40, 40); g.fill();\
             out.push('NZ ' + px(50, 50));\
             g.clearRect(0, 0, 100, 100); g.strokeStyle = '#ff00ff'; g.lineWidth = 10; g.beginPath(); g.moveTo(10, 50); g.lineTo(90, 50); g.stroke();\
             out.push('LINE ' + px(50, 46) + ' ' + px(50, 54) + ' ' + px(50, 40) + ' ' + px(5, 50));\
             g.clearRect(0, 0, 100, 100); g.lineCap = 'round'; g.beginPath(); g.moveTo(10, 50); g.lineTo(90, 50); g.stroke();\
             out.push('CAP ' + px(7, 50));\
             g.clearRect(0, 0, 100, 100); g.fillStyle = '#123456'; g.beginPath(); g.moveTo(10, 90); g.bezierCurveTo(10, 10, 90, 10, 90, 90); g.closePath(); g.fill();\
             out.push('BEZ ' + px(50, 70) + ' ' + px(50, 20));\
             g.clearRect(0, 0, 100, 100); g.beginPath(); g.roundRect(10, 10, 80, 80, 30); g.fill();\
             out.push('RR ' + px(50, 50) + ' ' + px(11, 11) + ' ' + px(30, 12));\
             g.clearRect(0, 0, 100, 100); g.beginPath(); g.moveTo(10, 10); g.arcTo(90, 10, 90, 90, 20); g.lineTo(90, 90); g.lineWidth = 4; g.stroke();\
             out.push('ARCTO ' + px(50, 10) + ' ' + px(90, 50) + ' ' + px(87, 13));\
             g.beginPath(); g.rect(10, 10, 20, 20);\
             out.push('IN ' + g.isPointInPath(20, 20) + ' ' + g.isPointInPath(40, 40));\
             g.clearRect(0, 0, 100, 100); g.save(); g.translate(50, 50); g.scale(2, 1); g.fillStyle = '#ff0000'; g.beginPath(); g.arc(0, 0, 10, 0, 7); g.fill(); g.restore();\
             out.push('XF ' + px(65, 50) + ' ' + px(50, 62) + ' ' + px(50, 55));\
             try { g.arc(0, 0, -1, 0, 1); } catch (e) { out.push('NEG ' + e.name); }\
             console.log(out.join(' | '));\
             </script></body>";
        let mut page = script_page(html);
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        let has = |s: &str| assert!(log.contains(s), "{s} in {log}");
        has("TRI 255,0,0 -");
        has("ARC 0,255,0 0,255,0 - 0,255,0 -");
        has("EO 0,0,255 -");
        has("NZ 0,0,255");
        has("LINE 255,0,255 255,0,255 - -");
        has("CAP 255,0,255");
        has("BEZ 18,52,86 -");
        has("RR 18,52,86 - 18,52,86");
        has("ARCTO 255,0,255 255,0,255 -");
        has("IN true false");
        has("XF 255,0,0 - 255,0,0");
        has("NEG IndexSizeError");
    }

    #[test]
    fn a_script_dashes_clips_and_reuses_paths() {
        let html = "<body><canvas id=c width=100 height=100></canvas><script>\
             const g = document.getElementById('c').getContext('2d');\
             const px = (x, y) => { const d = g.getImageData(x, y, 1, 1).data; return d[3] === 0 ? '-' : d[0] + ',' + d[1] + ',' + d[2]; };\
             const out = [];\
             g.strokeStyle = '#ff0000'; g.lineWidth = 4; g.setLineDash([10, 10]);\
             out.push('GET ' + g.getLineDash().join('/'));\
             g.beginPath(); g.moveTo(0, 50); g.lineTo(100, 50); g.stroke();\
             out.push('DASH ' + px(5, 50) + ' ' + px(15, 50) + ' ' + px(25, 50));\
             g.lineDashOffset = 10; g.clearRect(0, 0, 100, 100); g.stroke();\
             out.push('OFF ' + px(5, 50) + ' ' + px(15, 50));\
             g.setLineDash([3]); out.push('ODD ' + g.getLineDash().join('/'));\
             g.setLineDash([-1, 2]); out.push('BAD ' + g.getLineDash().join('/'));\
             g.setLineDash([]); g.lineDashOffset = 0; g.clearRect(0, 0, 100, 100); g.stroke();\
             out.push('SOLID ' + px(5, 50) + ' ' + px(15, 50));\
             g.clearRect(0, 0, 100, 100); g.save(); g.beginPath(); g.arc(50, 50, 20, 0, Math.PI * 2); g.clip();\
             g.fillStyle = '#0000ff'; g.fillRect(0, 0, 100, 100);\
             out.push('CLIP ' + px(50, 50) + ' ' + px(5, 5) + ' ' + px(50, 31) + ' ' + px(50, 28));\
             g.restore(); g.fillStyle = '#00ff00'; g.fillRect(0, 0, 10, 10);\
             out.push('RESTORED ' + px(5, 5));\
             const sq = new Path2D(); sq.rect(10, 10, 30, 30);\
             g.clearRect(0, 0, 100, 100); g.fillStyle = '#123456'; g.fill(sq);\
             out.push('P2D ' + px(20, 20) + ' ' + px(50, 50) + ' ' + g.isPointInPath(sq, 20, 20) + ' ' + g.isPointInPath(sq, 60, 60));\
             const both = new Path2D(sq); both.addPath(new Path2D('M60 60 h20 v20 h-20 z'));\
             g.clearRect(0, 0, 100, 100); g.fill(both);\
             out.push('ADD ' + px(20, 20) + ' ' + px(70, 70) + ' ' + px(50, 50));\
             g.clearRect(0, 0, 100, 100); g.strokeStyle = '#ff00ff'; g.lineWidth = 6; g.stroke(new Path2D('M10 10 L90 10 L90 90'));\
             out.push('SVGSTROKE ' + px(50, 10) + ' ' + px(90, 50) + ' ' + px(50, 50));\
             g.clearRect(0, 0, 100, 100); g.fillStyle = '#ff0000'; g.fill(new Path2D('M20 50 A30 30 0 0 1 80 50 Z'));\
             out.push('SVGARC ' + px(50, 40) + ' ' + px(50, 60) + ' ' + px(50, 15));\
             g.clearRect(0, 0, 100, 100); g.save(); g.beginPath(); g.rect(0, 0, 50, 100); g.clip(); g.fill(new Path2D('M0 0 H100 V100 H0 Z'));\
             out.push('CLIPPATH ' + px(25, 50) + ' ' + px(75, 50)); g.restore();\
             g.clearRect(0, 0, 100, 100); g.fillStyle = '#ff0000'; g.beginPath(); g.roundRect(10, 10, 80, 80, [30, 0, 30, 0]); g.fill();\
             out.push('RR4 ' + px(11, 11) + ' ' + px(88, 11) + ' ' + px(88, 88) + ' ' + px(11, 88));\
             g.clearRect(0, 0, 100, 100); g.beginPath(); g.roundRect(10, 10, 80, 40, { x: 40, y: 10 }); g.fill();\
             out.push('RRE ' + px(11, 11) + ' ' + px(50, 11) + ' ' + px(11, 30));\
             try { g.roundRect(0, 0, 10, 10, -1); } catch (e) { out.push('RRNEG ' + e.name); }\
             console.log(out.join(' | '));\
             </script></body>";
        let mut page = script_page(html);
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        let has = |s: &str| assert!(log.contains(s), "{s} in {log}");
        has("GET 10/10");
        has("DASH 255,0,0 - 255,0,0");
        has("OFF - 255,0,0");
        has("ODD 3/3 ");
        has("BAD 3/3 ");
        has("SOLID 255,0,0 255,0,0");
        has("CLIP 0,0,255 - 0,0,255 -");
        has("RESTORED 0,255,0");
        has("P2D 18,52,86 - true false");
        has("ADD 18,52,86 18,52,86 -");
        has("SVGSTROKE 255,0,255 255,0,255 -");
        has("SVGARC 255,0,0 - -");
        has("CLIPPATH 255,0,0 -");
        has("RR4 - 255,0,0 - 255,0,0");
        has("RRE - 255,0,0 255,0,0");
        has("RRNEG RangeError");
    }

    #[test]
    fn a_script_paints_with_conic_gradients_and_patterns() {
        let html = "<body><canvas id=c width=40 height=40></canvas><canvas id=t width=2 height=2></canvas><script>\
             const g = document.getElementById('c').getContext('2d');\
             const px = (x, y) => { const d = g.getImageData(x, y, 1, 1).data; return d[3] === 0 ? '-' : d[0] + ',' + d[1] + ',' + d[2]; };\
             const out = [];\
             const k = g.createConicGradient(0, 20, 20); k.addColorStop(0, '#ff0000'); k.addColorStop(0.5, '#ff0000'); k.addColorStop(0.5, '#0000ff'); k.addColorStop(1, '#0000ff');\
             g.fillStyle = k; g.fillRect(0, 0, 40, 40);\
             out.push('CONIC ' + px(20, 35) + ' ' + px(20, 5));\
             let bad = ''; try { g.createConicGradient(NaN, 0, 0); } catch (e) { bad = e.name; } out.push('CONICBAD ' + bad);\
             const t = document.getElementById('t').getContext('2d');\
             t.fillStyle = '#ff0000'; t.fillRect(0, 0, 1, 2); t.fillStyle = '#00ff00'; t.fillRect(1, 0, 1, 2);\
             const pat = g.createPattern(document.getElementById('t'), 'repeat');\
             g.clearRect(0, 0, 40, 40); g.fillStyle = pat; g.fillRect(0, 0, 40, 40);\
             out.push('PAT ' + px(0, 0) + ' ' + px(1, 1) + ' ' + px(38, 3) + ' ' + px(39, 3));\
             g.clearRect(0, 0, 40, 40); g.fillStyle = g.createPattern(document.getElementById('t'), 'no-repeat'); g.fillRect(0, 0, 40, 40);\
             out.push('NOREP ' + px(1, 1) + ' ' + px(5, 5));\
             out.push('PATNULL ' + (g.createPattern(document.getElementById('t'), null) !== null) + ' ' + g.createPattern({}, 'repeat'));\
             try { g.createPattern(document.getElementById('t'), 'sideways'); } catch (e) { out.push('PATBAD ' + e.name); }\
             g.clearRect(0, 0, 40, 40); g.strokeStyle = pat; g.lineWidth = 6; g.beginPath(); g.moveTo(0, 20); g.lineTo(40, 20); g.stroke();\
             out.push('PATSTROKE ' + px(10, 20));\
             console.log(out.join(' | '));\
             </script></body>";
        let mut page = script_page(html);
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        let has = |s: &str| assert!(log.contains(s), "{s} in {log}");
        has("CONIC 255,0,0 0,0,255");
        has("CONICBAD TypeError");
        has("PAT 255,0,0 0,255,0 255,0,0 0,255,0");
        has("NOREP 0,255,0 -");
        has("PATNULL true null");
        has("PATBAD SyntaxError");
        has("PATSTROKE 255,0,0");
    }

    #[test]
    fn a_script_font_face_can_name_an_installed_font_with_local() {
        let html = "<body><script>\
             const out = [];\
             const ok = new FontFace('LocalFace', 'local(\"DejaVu Sans Bold\")', { weight: '700' });\
             const bad = new FontFace('Missing', 'local(\"No Such Font\")');\
             const mixed = new FontFace('Mixed', 'local(\"No Such Font\"), local(DejaVuSans)');\
             const both = new FontFace('Both', 'local(\"No Such Font\"), url(\"data:font/ttf;base64,AAAA\")');\
             out.push('START ' + ok.status);\
             Promise.all([ok.load(), mixed.load()]).then(() => {\
               document.fonts.add(ok);\
               out.push('OK ' + ok.status + ' ' + mixed.status + ' ' + document.fonts.check('16px LocalFace') + ' ' + document.fonts.size);\
               return bad.load().catch((e) => out.push('BAD ' + bad.status + ' ' + e.name));\
             }).then(() => both.load().catch((e) => out.push('BOTH ' + e.name)))\
               .then(() => console.log(out.join(' | ')));\
             </script></body>";
        let mut page = script_page(html);
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        assert!(log.contains("START unloaded | OK loaded loaded true 1 | BAD error NetworkError | BOTH NetworkError"), "{log}");
    }

    #[test]
    fn canvas_direction_decides_which_side_start_and_end_mean() {
        let html = "<body><canvas id=c width=100 height=30></canvas><script>\
             const g = document.getElementById('c').getContext('2d');\
             g.font = '16px sans-serif';\
             const ink = () => { const d = g.getImageData(0, 0, 100, 30).data; let lo = 100, hi = -1;\
               for (let y = 0; y < 30; y++) for (let x = 0; x < 100; x++) if (d[(y * 100 + x) * 4 + 3] > 0) { lo = Math.min(lo, x); hi = Math.max(hi, x); }\
               g.clearRect(0, 0, 100, 30); return (lo + hi) / 2 > 50 ? 'right' : 'left'; };\
             const out = ['DEFAULT ' + g.direction];\
             g.textAlign = 'start'; g.fillText('HH', 50, 20); out.push('LTR-START ' + ink());\
             g.textAlign = 'end'; g.fillText('HH', 50, 20); out.push('LTR-END ' + ink());\
             g.direction = 'rtl'; out.push('SET ' + g.direction);\
             g.textAlign = 'start'; g.fillText('HH', 50, 20); out.push('RTL-START ' + ink());\
             g.textAlign = 'end'; g.fillText('HH', 50, 20); out.push('RTL-END ' + ink());\
             g.textAlign = 'left'; g.fillText('HH', 50, 20); out.push('RTL-LEFT ' + ink());\
             g.direction = 'sideways'; out.push('KEEP ' + g.direction);\
             g.direction = 'inherit'; out.push('INHERIT ' + g.direction);\
             g.save(); g.direction = 'ltr'; g.restore(); out.push('RESTORED ' + g.direction);\
             console.log(out.join(' | '));\
             </script></body>";
        let mut page = script_page(html);
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        let want = "DEFAULT ltr | LTR-START right | LTR-END left | SET rtl | RTL-START left | RTL-END right | RTL-LEFT right | KEEP rtl | INHERIT inherit | RESTORED inherit";
        assert!(log.contains(want), "{log}");
    }

    #[test]
    fn a_script_paints_canvas_text_with_gradients_and_outlines() {
        let html = "<body><canvas id=c width=120 height=60></canvas><script>\
             const g = document.getElementById('c').getContext('2d');\
             const grad = g.createLinearGradient(0, 0, 120, 0);\
             grad.addColorStop(0, '#ff0000'); grad.addColorStop(1, '#0000ff');\
             let bad = ''; try { grad.addColorStop(2, 'red'); } catch (e) { bad += e.name + ' '; }\
             try { grad.addColorStop(0.5, 'not a colour'); } catch (e) { bad += e.name; }\
             g.font = '40px sans-serif'; g.fillStyle = grad; g.fillText('HHHH', 0, 45);\
             const row = g.getImageData(0, 0, 120, 60).data;\
             let first = null, last = null;\
             for (let x = 0; x < 120; x++) for (let y = 0; y < 60; y++) { const i = (y * 120 + x) * 4;\
               if (row[i + 3] === 255) { if (!first) first = [row[i], row[i + 2]]; last = [row[i], row[i + 2]]; } }\
             console.log('GRAD ' + (first[0] > first[1]) + ' ' + (last[1] > last[0]) + ' ' + bad + ' ' + (g.fillStyle === grad));\
             g.clearRect(0, 0, 120, 60); g.strokeStyle = '#00ff00'; g.lineWidth = 3; g.strokeText('O', 10, 50);\
             const d = g.getImageData(0, 0, 120, 60).data; let green = 0;\
             for (let i = 0; i < d.length; i += 4) if (d[i + 1] === 255 && d[i + 3] === 255) green++;\
             console.log('STROKE ' + (green > 20));\
             </script></body>";
        let mut page = script_page(html);
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        assert!(log.contains("GRAD true true IndexSizeError SyntaxError true"), "{log}");
        assert!(log.contains("STROKE true"), "{log}");
    }

    #[test]
    fn canvas_text_can_use_a_face_the_pages_stylesheet_declared() {
        let html = format!(
            "<style>@font-face {{ font-family: Sheet; font-stretch: 75% 100%; src: url({}) format('woff2'); }}</style>\
             <body><canvas id=c width=80 height=30></canvas><script>\
             const g = document.getElementById('c').getContext('2d');\
             g.font = '20px Sheet'; const own = g.measureText('Hello').width;\
             g.font = '20px sans-serif'; const plain = g.measureText('Hello').width;\
             console.log('SHEET ' + (own !== plain) + ' ' + document.fonts.size);\
             const [sf] = [...document.fonts]; console.log('SHEETFACE ' + sf.family + ' ' + sf.status + ' ' + sf.weight + ' ' + sf.stretch + ' ' + document.fonts.check('16px Sheet'));\
             </script></body>",
            data_url(REGULAR, "font/woff2")
        );
        let mut page = crate::Page::new(400.0, 300.0);
        page.set_web_scripts(true);
        page.pending_linked_fonts = catalog(&html);
        page.load_html_str(&html, Some("https://example.com/p".to_string()));
        page.settle();
        run_scripts(&mut page);
        let log = page.web_console().join("\n");
        assert!(log.contains("SHEET true 1"), "{log}");
        assert!(log.contains("SHEETFACE sheet loaded 400 75% 100% true"), "{log}");
    }

    #[test]
    fn a_missing_bold_or_italic_is_to_be_made_by_drawing() {
        let html = format!(
            "<style>@font-face {{ font-family: only; src: url({}); font-weight: 400; }}\
             @font-face {{ font-family: both; src: url({}); font-weight: 400; }}\
             @font-face {{ font-family: both; src: url({}); font-weight: 700; }}</style>",
            data_url(REGULAR, "font/woff2"),
            data_url(REGULAR, "font/woff2"),
            data_url(BOLD, "font/woff2")
        );
        let c = catalog(&html);
        let synth = |f: &str, w, i| c.face_and_synthesis(f, w, i).1;
        assert_eq!(synth("only", 400, false), 0);
        assert_eq!(synth("only", 700, false), 1);
        assert_eq!(synth("only", 400, true), 2);
        assert_eq!(synth("only", 700, true), 3);
        assert_eq!(synth("both", 700, false), 0, "a real bold face exists");
        assert_eq!(synth("both", 400, true), 2);
        assert_eq!(synth("sans-serif", 700, false), 0, "no page face: nothing to synthesize");
    }

    #[test]
    fn font_synthesis_can_forbid_the_drawing_tricks() {
        let html = format!(
            "<style>@font-face {{ font-family: only; src: url({}); }}\
             p {{ font-family: only; font-weight: bold; font-style: italic }}\
             .none {{ font-synthesis: none }} .w {{ font-synthesis: weight }} .s {{ font-synthesis: style }}\
             .nw {{ font-synthesis-weight: none }} .ns {{ font-synthesis-style: none }}</style>\
             <body><p id=a>a</p><p id=b class=none>b</p><p id=c class=w>c</p><p id=d class=s>d</p>\
             <p id=e class=nw>e</p><p id=f class=ns>f</p><div class=none><p id=g>g</p></div></body>",
            data_url(REGULAR, "font/woff2")
        );
        let doc = blueice_html::parse(&html);
        let ua = blueice_css::ua_stylesheet();
        let author = blueice_css::parse(&html[html.find("<style>").unwrap() + 7..html.find("</style>").unwrap()]).rules;
        let mut styles = blueice_css::cascade(&doc, &[(blueice_css::Origin::Ua, &ua), (blueice_css::Origin::Author, &author)]);
        assign_faces(&catalog(&html), &mut styles);
        let synth = |id: &str| {
            let node = styles
                .keys()
                .copied()
                .find(|n| {
                    matches!(doc.data(*n), blueice_dom::NodeData::Element { attributes, .. }
                        if attributes.iter().any(|(k, v)| k == "id" && v == id))
                })
                .unwrap();
            match styles[&node].other.get("-blueice-web-synth") {
                Some(blueice_css::Value::Number(n)) => *n as u8,
                _ => 0,
            }
        };
        assert_eq!(
            ["a", "b", "c", "d", "e", "f", "g"].map(synth),
            [3, 0, 1, 2, 2, 1, 0],
            "default, none, weight, style, no weight, no style, inherited none"
        );
    }

    #[test]
    fn a_variable_face_is_drawn_at_each_elements_weight_and_needs_no_synthetic_bold() {
        let inter = include_bytes!("../../font/tests/data/inter-latin-var.woff2");
        let html = format!(
            "<style>@font-face {{ font-family: Var; src: url({}) format('woff2'); font-weight: 100 900; }}\
             body {{ margin: 0 }} p {{ margin: 0; font-family: Var; font-size: 32px }}\
             .l {{ font-weight: 200 }} .h {{ font-weight: 800 }}</style>\
             <body><p class=l>Hamburgefonstiv</p><p class=h>Hamburgefonstiv</p></body>",
            data_url(inter, "font/woff2")
        );
        let page = page_with_fonts(&html);
        let got = runs(&page);
        let (light, heavy) = (got[0].1.unwrap(), got[1].1.unwrap());
        assert_ne!(light, heavy, "one file, two weights, two faces");
        let w = |id: u32| blueice_font::measure_text_width_in("Hamburgefonstiv", 32.0, false, false, Some(WebFontId::from_u32(id)));
        assert!(w(light) < w(heavy));
        let c = &page.linked_fonts;
        assert_eq!(c.face_and_synthesis("var", 800, false).1, 0, "the axis reaches 800");
        let cmds: Vec<_> = page.render().commands.into_iter().filter_map(|c| match c {
            blueice_paint::PaintCommand::Text { bold, .. } => Some(bold), _ => None }).collect();
        assert_eq!(cmds, [false, false]);
    }
}
