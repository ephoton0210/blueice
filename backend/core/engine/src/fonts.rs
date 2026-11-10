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
    pub(crate) fn face_for(&self, families: &str, weight: u16, italic: bool) -> Option<WebFontId> {
        self.face_and_synthesis(families, weight, italic).0
    }

    /// [`face_for`](Self::face_for), and which of bold (1) and italic (2) the page's primary
    /// face lacks, so the text is made bold or slanted by drawing instead.
    pub(crate) fn face_and_synthesis(
        &self,
        families: &str,
        weight: u16,
        italic: bool,
    ) -> (Option<WebFontId>, u8) {
        let mut synthesis = None;
        let mut members: Vec<blueice_font::web::WeightedMember> = Vec::new();
        for family in families.split(',') {
            let family = family.trim();
            if let Some(faces) = self.families.get(family) {
                if synthesis.is_none() {
                    synthesis = best_of(faces, weight, italic).map(|best| {
                        u8::from(weight >= 600 && best.weight_max < 600) | (u8::from(italic && !best.italic) << 1)
                    });
                }
                members.extend(best_faces(faces, weight, italic));
            } else if SYSTEM_FAMILIES.contains(&family) {
                break;
            }
        }
        (blueice_font::web::register_set_weighted(&members), synthesis.unwrap_or(0))
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

fn best_faces(faces: &[CatalogFace], weight: u16, italic: bool) -> Vec<blueice_font::web::WeightedMember> {
    let Some(best) = best_of(faces, weight, italic) else {
        return Vec::new();
    };
    faces
        .iter()
        .rev()
        .filter(|f| (f.italic, f.weight_min, f.weight_max) == (best.italic, best.weight_min, best.weight_max))
        .map(|f| (f.id, f.ranges.clone(), weight.clamp(f.weight_min, f.weight_max)))
        .collect()
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
    let mut chosen: HashMap<(String, u16, bool), (Option<WebFontId>, u8)> = HashMap::new();
    for style in styles.values_mut() {
        let Some(families) = style.font_family.as_deref() else {
            style.other.remove("-blueice-web-font");
            style.other.remove("-blueice-web-synth");
            continue;
        };
        let key = (families.to_string(), weight_of(style), style.is_italic());
        let (face, synthesis) = *chosen
            .entry(key.clone())
            .or_insert_with(|| catalog.face_and_synthesis(&key.0, key.1, key.2));
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
    fn canvas_text_can_use_a_face_the_pages_stylesheet_declared() {
        let html = format!(
            "<style>@font-face {{ font-family: Sheet; src: url({}) format('woff2'); }}</style>\
             <body><canvas id=c width=80 height=30></canvas><script>\
             const g = document.getElementById('c').getContext('2d');\
             g.font = '20px Sheet'; const own = g.measureText('Hello').width;\
             g.font = '20px sans-serif'; const plain = g.measureText('Hello').width;\
             console.log('SHEET ' + (own !== plain) + ' ' + document.fonts.size);\
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
        assert!(log.contains("SHEET true 0"), "{log}");
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
