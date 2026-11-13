// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Images of a page whose HTML has cleared both gatekeeper stages: the
//! `<img>` elements' pictures and the `url()`s of CSS backgrounds, fetched on
//! the navigation's background thread behind the same per-hop URL review as
//! stylesheets, then decoded (`blueice-image`). An image is optional content:
//! anything that fails -- review, network, status, size, format -- leaves that
//! picture out and the page keeps its layout.

use crate::stylesheet::LinkedStyles;
use blueice_image::RasterImage;
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;

/// A decoded picture and the size it has by itself (an SVG is drawn at twice
/// that, so scaling it down stays sharp).
#[derive(Debug)]
pub struct LoadedImage {
    pub image: Arc<RasterImage>,
    pub natural: (f64, f64),
    /// For an SVG file made of `<symbol>`s (an icon sprite): its text, which inline SVGs'
    /// `<use href="file.svg#id">` draw from.
    pub sprite: Option<Arc<str>>,
}

/// Decoded images by the address as written in the page (an `src`, a CSS
/// `url()`, or a whole `data:` URL).
pub type LinkedImages = HashMap<String, Arc<LoadedImage>>;

#[derive(Clone, Copy)]
pub(crate) struct ImageLimits {
    pub images: usize,
    pub bytes_each: usize,
    pub bytes_total: usize,
    pub redirects: usize,
}

pub(crate) const DEFAULT_IMAGE_LIMITS: ImageLimits = ImageLimits {
    images: 160,
    bytes_each: 8 * 1024 * 1024,
    bytes_total: 24 * 1024 * 1024,
    redirects: 3,
};

/// Decodes fetched bytes. An SVG is rendered at twice its own size.
pub(crate) fn decode_loaded(bytes: &[u8]) -> Option<LoadedImage> {
    match blueice_image::sniff(bytes)? {
        blueice_image::Format::Svg => {
            let text = std::str::from_utf8(bytes).ok()?;
            // A file of `<symbol>`s with no drawing of its own is an icon sprite: kept as text.
            let sprite = text.contains("<symbol").then(|| Arc::<str>::from(text));
            let drawn = blueice_image::svg_size(text)
                .and_then(|(w, h)| render_svg_source(text, (w, h)));
            match (drawn, sprite) {
                (Some(loaded), sprite) => Some(LoadedImage { sprite, ..loaded }),
                (None, Some(sprite)) => Some(LoadedImage {
                    image: Arc::new(RasterImage::new(1, 1, vec![0; 4])),
                    natural: (0.0, 0.0),
                    sprite: Some(sprite),
                }),
                (None, None) => None,
            }
        }
        _ => {
            let image = blueice_image::decode(bytes)?;
            let natural = (f64::from(image.width), f64::from(image.height));
            Some(LoadedImage {
                image: Arc::new(image),
                natural,
                sprite: None,
            })
        }
    }
}

/// An SVG document drawn at twice `natural` (within the decoder's limits).
pub(crate) fn render_svg_source(text: &str, natural: (f64, f64)) -> Option<LoadedImage> {
    let scale = if natural.0.max(natural.1) * 2.0 <= 2048.0 {
        2.0
    } else {
        1.0
    };
    let size = (
        (natural.0 * scale).ceil().max(1.0) as u32,
        (natural.1 * scale).ceil().max(1.0) as u32,
    );
    let image = blueice_image::render_svg(text, Some(size))?.with_natural(natural);
    Some(LoadedImage {
        image: Arc::new(image),
        natural,
        sprite: None,
    })
}

/// The bytes of a `data:` URL (base64 or percent-encoded), if `url` is one.
pub(crate) fn data_url_bytes(url: &str) -> Option<Vec<u8>> {
    let rest = url.trim().strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    if meta.to_ascii_lowercase().ends_with(";base64") {
        base64_decode(data)
    } else {
        Some(percent_decode(data))
    }
}

fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Every `url(...)` address in CSS text, as written.
pub(crate) fn css_urls(css: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = css.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("url(") {
        let start = from + at + 4;
        let rest = &css[start..];
        let rest_trim = rest.trim_start();
        let (value, consumed) = match rest_trim.chars().next() {
            Some(q @ ('"' | '\'')) => {
                let inner = &rest_trim[1..];
                match inner.find(q) {
                    Some(end) => (
                        inner[..end].to_string(),
                        rest.len() - rest_trim.len() + 1 + end + 1,
                    ),
                    None => break,
                }
            }
            _ => match rest.find(')') {
                Some(end) => (rest[..end].trim().to_string(), end),
                None => break,
            },
        };
        if !value.is_empty() {
            out.push(value);
        }
        from = start + consumed.min(rest.len());
    }
    out
}

/// What a page's elements are called: the ids, classes and tag names its rules can match.
#[derive(Default)]
struct DocumentKeys {
    ids: std::collections::HashSet<String>,
    classes: std::collections::HashSet<String>,
    tags: std::collections::HashSet<String>,
}

impl DocumentKeys {
    fn of(doc: &blueice_dom::Document) -> Self {
        let mut keys = DocumentKeys::default();
        let mut stack = vec![doc.root()];
        while let Some(node) = stack.pop() {
            if let blueice_dom::NodeData::Element { tag_name, attributes } = doc.data(node) {
                keys.tags.insert(tag_name.to_ascii_lowercase());
                for (name, value) in attributes {
                    match name.as_str() {
                        "id" => {
                            keys.ids.insert(value.clone());
                        }
                        "class" => keys.classes.extend(value.split_whitespace().map(str::to_string)),
                        _ => {}
                    }
                }
            }
            stack.extend(doc.children(node));
        }
        keys
    }

    /// Whether a selector's most selective rightmost part is something the page has (a rule with
    /// a pseudo-class or a pseudo-element still counts: it may apply later).
    fn has(&self, key: blueice_css::SelectorKey<'_>) -> bool {
        use blueice_css::SelectorKey;
        match key {
            SelectorKey::Id(id) => self.ids.contains(id),
            SelectorKey::Class(class) => self.classes.contains(class),
            SelectorKey::Type(tag) => self.tags.contains(&tag.to_ascii_lowercase()),
            SelectorKey::Universal => true,
        }
    }
}

/// The addresses in the declarations of `css`'s rules whose selectors can match something in the
/// page: an icon set's sheet lists hundreds of pictures, of which a page uses a few.
fn used_css_urls(css: &str, keys: &DocumentKeys) -> Vec<String> {
    use blueice_css::{Token, Value};
    let mut out = Vec::new();
    for rule in blueice_css::parse(css).rules {
        if !rule.selectors.iter().any(|s| keys.has(s.key())) {
            continue;
        }
        for declaration in &rule.declarations {
            let Value::Unparsed(tokens) = &declaration.value else { continue };
            let mut i = 0;
            while i < tokens.len() {
                match &tokens[i] {
                    Token::Url(u) => out.push(u.clone()),
                    Token::Ident(name) if name.eq_ignore_ascii_case("url") => {
                        if let (Some(Token::LeftParen), Some(Token::QuotedString(u))) =
                            (tokens.get(i + 1), tokens.get(i + 2))
                        {
                            out.push(u.clone());
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
        }
    }
    out
}

/// Records one reference found while walking a page.
type AddReference<'a> = dyn FnMut(&str, &Url, &mut Vec<(String, String)>) + 'a;

/// What the page asks for: `(as written, resolved)` pairs.
pub(crate) fn image_references(
    doc: &blueice_dom::Document,
    linked: &LinkedStyles,
    document_base: &Url,
    limit: usize,
) -> Vec<(String, String)> {
    use blueice_dom::{NodeData, NodeId};
    let mut found: Vec<(String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |raw: &str, base: &Url, found: &mut Vec<(String, String)>| {
        let raw = raw.trim();
        if raw.is_empty() || !seen.insert(raw.to_string()) {
            return;
        }
        let resolved = if raw.starts_with("data:") {
            Some(raw.to_string())
        } else {
            base.join(raw).ok().map(|u| u.to_string())
        };
        if let Some(resolved) = resolved {
            found.push((raw.to_string(), resolved));
        }
    };
    fn walk(
        doc: &blueice_dom::Document,
        node: NodeId,
        base: &Url,
        add: &mut AddReference<'_>,
        found: &mut Vec<(String, String)>,
    ) {
        if let NodeData::Element {
            tag_name,
            attributes,
        } = doc.data(node)
        {
            let attr = |name: &str| {
                attributes
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.as_str())
            };
            if tag_name == "use" {
                // An icon sprite: the file before the `#`; a same-document reference needs none.
                let reference = attr("href").or_else(|| attr("xlink:href"));
                if let Some(file) = reference
                    .and_then(|r| r.split('#').next())
                    .filter(|f| !f.trim().is_empty() && !f.trim().starts_with("data:"))
                {
                    add(file, base, found);
                }
            }
            if tag_name == "img" {
                match attr("src").filter(|s| !s.trim().is_empty()) {
                    Some(src) => add(src, base, found),
                    None => {
                        if let Some(first) = attr("srcset")
                            .and_then(|s| s.split(',').next())
                            .and_then(|c| c.split_whitespace().next())
                        {
                            add(first, base, found);
                        }
                    }
                }
            }
            if let Some(style) = attr("style") {
                for url in css_urls(style) {
                    add(&url, base, found);
                }
            }
            if tag_name == "style" {
                for child in doc.children(node) {
                    if let NodeData::Text { data } = doc.data(child) {
                        for url in css_urls(data) {
                            add(&url, base, found);
                        }
                    }
                }
            }
        }
        for child in doc.children(node) {
            walk(doc, child, base, add, found);
        }
    }
    walk(doc, doc.root(), document_base, &mut add, &mut found);
    // The pictures of rules the page can use come first, so a limit cuts the unused ones.
    let keys = DocumentKeys::of(doc);
    for (href, css) in linked {
        let sheet_base = document_base
            .join(href)
            .unwrap_or_else(|_| document_base.clone());
        for url in used_css_urls(css, &keys) {
            add(&url, &sheet_base, &mut found);
        }
    }
    for (href, css) in linked {
        let sheet_base = document_base
            .join(href)
            .unwrap_or_else(|_| document_base.clone());
        for url in css_urls(css) {
            add(&url, &sheet_base, &mut found);
        }
    }
    found.truncate(limit);
    found
}

/// Fetches and decodes the images `html` (with its fetched stylesheets) asks
/// for. `review` answers whether one absolute URL may be requested.
pub(crate) fn fetch_linked_images(
    html: &str,
    linked: &LinkedStyles,
    document_url: &str,
    review: &(dyn Fn(&str) -> bool + Sync),
    limits: ImageLimits,
) -> LinkedImages {
    let doc = blueice_html::parse(html);
    let Some(base) = crate::linked_styles::base_url(&doc, document_url) else {
        return LinkedImages::new();
    };
    let wanted = image_references(&doc, linked, &base, limits.images);
    let decoded: Vec<Option<LoadedImage>> = std::thread::scope(|scope| {
        let workers: Vec<_> = wanted
            .iter()
            .map(|(_, url)| scope.spawn(move || fetch_one(url, review, limits)))
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().ok().flatten())
            .collect()
    });
    let mut total = 0usize;
    let mut images = LinkedImages::new();
    for ((raw, _), image) in wanted.into_iter().zip(decoded) {
        let Some(image) = image else { continue };
        total += image.image.rgba.len();
        if total > limits.bytes_total * 2 {
            break;
        }
        images.insert(raw, Arc::new(image));
    }
    images
}

fn fetch_one(
    start: &str,
    review: &(dyn Fn(&str) -> bool + Sync),
    limits: ImageLimits,
) -> Option<LoadedImage> {
    if start.starts_with("data:") {
        let bytes = data_url_bytes(start)?;
        return (bytes.len() <= limits.bytes_each)
            .then(|| decode_loaded(&bytes))
            .flatten();
    }
    let mut url = start.to_string();
    for _ in 0..=limits.redirects {
        if !matches!(Url::parse(&url).ok()?.scheme(), "http" | "https") || !review(&url) {
            return None;
        }
        match blueice_net::fetch_subresource_hop(&url, limits.bytes_each).ok()? {
            blueice_net::SubresourceHop::Redirect { location, .. } => url = location,
            blueice_net::SubresourceHop::Response { status, body, .. } => {
                return (200..300)
                    .contains(&status)
                    .then(|| decode_loaded(&body))
                    .flatten();
            }
            blueice_net::SubresourceHop::TooLarge => return None,
        }
    }
    None
}

/// The address an `<img>` shows: its `src`, else the first `srcset` candidate.
pub(crate) fn img_source(attributes: &[(String, String)]) -> Option<String> {
    let attr = |name: &str| {
        attributes
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.trim())
    };
    attr("src")
        .filter(|s| !s.is_empty())
        .or_else(|| {
            attr("srcset")
                .and_then(|s| s.split(',').next())
                .and_then(|c| c.split_whitespace().next())
        })
        .map(str::to_string)
}

fn escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

fn css_value(value: &blueice_css::Value) -> Option<String> {
    use blueice_css::{Color, Value};
    Some(match value {
        Value::Color(Color::Rgba(r, g, b, a)) => {
            format!("rgba({r},{g},{b},{})", f64::from(*a) / 255.0)
        }
        Value::Color(Color::CurrentColor) => "currentColor".to_string(),
        Value::Keyword(k) => k.clone(),
        Value::Number(n) => n.to_string(),
        Value::Length(l) => format!(
            "{}px",
            l.to_px(&blueice_css::LengthContext {
                font_size: 16.0,
                root_font_size: 16.0,
                viewport_width: 1280.0,
                viewport_height: 720.0,
                percent_base: f64::NAN,
            })
        ),
        Value::Percentage(p) => format!("{p}%"),
        _ => return None,
    })
}

/// Presentation properties the page's CSS may have set on an SVG element.
const SVG_STYLE_PROPERTIES: [&str; 9] = [
    "fill",
    "stroke",
    "stroke-width",
    "opacity",
    "fill-opacity",
    "stroke-opacity",
    "stop-color",
    "stop-opacity",
    "display",
];

/// `href="file.svg#id"` resolved against the fetched sprites: the id and the `<symbol>` text
/// that defines it. `None` for a same-document reference or a sprite that was not fetched.
fn sprite_symbol(href: &str, sprites: &LinkedImages) -> Option<(String, String)> {
    let (file, id) = href.trim().split_once('#')?;
    let text = sprites.get(file.trim())?.sprite.as_deref()?;
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || "-_:.".contains(c)) {
        return None;
    }
    let at = ["id=\"", "id='"]
        .iter()
        .find_map(|quote| {
            let needle = format!("{quote}{id}{}", &quote[quote.len() - 1..]);
            text.find(&needle)
        })?;
    let start = text[..at].rfind("<symbol")?;
    if text[start..at].contains('>') {
        return None;
    }
    let end = at + text[at..].find("</symbol>")? + "</symbol>".len();
    Some((id.to_string(), text[start..end].to_string()))
}

/// The SVG properties that inherit down the tree.
const INHERITED_SVG_PROPERTIES: [&str; 5] =
    ["fill", "stroke", "stroke-width", "fill-opacity", "stroke-opacity"];

/// The inline `<svg>` at `node` as a standalone document. The page's CSS
/// reaches it through the computed values: `currentColor` is the svg's text
/// colour, and the presentation properties above are written out as a `style`
/// (ahead of the element's own, which keeps winning).
pub(crate) fn svg_markup(
    doc: &blueice_dom::Document,
    styles: &HashMap<blueice_dom::NodeId, blueice_css::ComputedStyle>,
    node: blueice_dom::NodeId,
    sprites: &LinkedImages,
) -> String {
    use blueice_dom::NodeData;
    #[allow(clippy::too_many_arguments)]
    fn write(
        doc: &blueice_dom::Document,
        styles: &HashMap<blueice_dom::NodeId, blueice_css::ComputedStyle>,
        sprites: &LinkedImages,
        used: &mut Vec<(String, String)>,
        node: blueice_dom::NodeId,
        root: bool,
        parent: Option<blueice_dom::NodeId>,
        out: &mut String,
    ) {
        match doc.data(node) {
            NodeData::Text { data } => escape(data, out),
            NodeData::Element {
                tag_name,
                attributes,
            } => {
                out.push('<');
                out.push_str(tag_name);
                let mut computed = String::new();
                if let Some(style) = styles.get(&node) {
                    if root {
                        if let Some(c) = css_value(&blueice_css::Value::Color(style.color)) {
                            computed.push_str(&format!("color:{c};"));
                        }
                    }
                    let parent_style = parent.and_then(|p| styles.get(&p));
                    for property in SVG_STYLE_PROPERTIES {
                        let value = style.other.get(property);
                        // A value the element only inherits is left to inherit: written as this
                        // element's own `style` it would beat its presentation attributes.
                        if INHERITED_SVG_PROPERTIES.contains(&property)
                            && parent_style.is_some_and(|p| p.other.get(property) == value)
                        {
                            continue;
                        }
                        if let Some(v) = value.and_then(css_value) {
                            computed.push_str(&format!("{property}:{v};"));
                        }
                    }
                }
                let mut wrote_style = false;
                for (name, value) in attributes {
                    out.push(' ');
                    out.push_str(name);
                    out.push_str("=\"");
                    if name == "style" {
                        escape(&computed, out);
                        wrote_style = true;
                    }
                    let local = (tag_name == "use" && (name == "href" || name == "xlink:href"))
                        .then(|| sprite_symbol(value, sprites))
                        .flatten();
                    match local {
                        // An icon from a sprite file: the symbol is carried inside this drawing.
                        Some((id, symbol)) => {
                            if !used.iter().any(|(known, _)| *known == id) {
                                used.push((id.clone(), symbol));
                            }
                            escape(&format!("#{id}"), out);
                        }
                        None => escape(value, out),
                    }
                    out.push('"');
                }
                if !wrote_style && !computed.is_empty() {
                    out.push_str(" style=\"");
                    escape(&computed, out);
                    out.push('"');
                }
                if root {
                    if !attributes.iter().any(|(k, _)| k == "xmlns") {
                        out.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
                    }
                    if !attributes.iter().any(|(k, _)| k == "xmlns:xlink") {
                        out.push_str(" xmlns:xlink=\"http://www.w3.org/1999/xlink\"");
                    }
                }
                out.push('>');
                for child in doc.children(node) {
                    write(doc, styles, sprites, used, child, false, Some(node), out);
                }
                if root && !used.is_empty() {
                    out.push_str("<defs>");
                    for (_, symbol) in used.iter() {
                        out.push_str(symbol);
                    }
                    out.push_str("</defs>");
                }
                out.push_str("</");
                out.push_str(tag_name);
                out.push('>');
            }
            NodeData::Document => {}
        }
    }
    let mut out = String::new();
    write(doc, styles, sprites, &mut Vec::new(), node, true, None, &mut out);
    out
}

/// An inline SVG's own size: `width`/`height` in pixels, else its `viewBox`.
pub(crate) fn svg_natural_size(attributes: &[(String, String)]) -> Option<(f64, f64)> {
    let number = |name: &str| {
        attributes
            .iter()
            .find(|(k, _)| k == name)
            .and_then(|(_, v)| {
                let v = v.trim();
                (!v.ends_with('%')).then(|| v.trim_end_matches("px").parse::<f64>().ok())?
            })
            .filter(|v| v.is_finite() && *v > 0.0)
    };
    let view_box = attributes
        .iter()
        .find(|(k, _)| k == "viewBox")
        .and_then(|(_, v)| {
            let n: Vec<f64> = v
                .split([' ', ','])
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.parse().ok())
                .collect();
            (n.len() == 4 && n[2] > 0.0 && n[3] > 0.0).then_some((n[2], n[3]))
        });
    match (number("width"), number("height"), view_box) {
        (Some(w), Some(h), _) => Some((w, h)),
        (Some(w), None, Some((vw, vh))) => Some((w, w * vh / vw)),
        (None, Some(h), Some((vw, vh))) => Some((h * vw / vh, h)),
        (None, None, Some(view)) => Some(view),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_urls_are_found_quoted_and_not() {
        assert_eq!(
            css_urls(
                "a{background:url(a.png)} b{background-image:url( \"b c.png\" ) , URL('d.gif')}"
            ),
            vec!["a.png", "b c.png", "d.gif"]
        );
        assert!(css_urls("a{color:red}").is_empty());
        assert!(css_urls("a{background:url(").is_empty());
    }

    #[test]
    fn data_urls_decode_base64_and_percent_encoding() {
        assert_eq!(
            data_url_bytes("data:text/plain;base64,aGk="),
            Some(b"hi".to_vec())
        );
        assert_eq!(data_url_bytes("data:,a%20b"), Some(b"a b".to_vec()));
        assert_eq!(
            data_url_bytes("data:image/svg+xml,%3Csvg%3E"),
            Some(b"<svg>".to_vec())
        );
        assert_eq!(data_url_bytes("http://x"), None);
        assert_eq!(data_url_bytes("data:;base64,@@"), None);
    }

    const SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='5'><rect width='10' height='5' fill='red'/></svg>";

    #[test]
    fn an_svg_is_drawn_at_twice_its_size_but_keeps_its_natural_size() {
        let loaded = decode_loaded(SVG.as_bytes()).unwrap();
        assert_eq!(loaded.natural, (10.0, 5.0));
        assert_eq!((loaded.image.width, loaded.image.height), (20, 10));
    }

    #[test]
    fn references_come_from_img_style_attributes_style_tags_and_linked_sheets() {
        let doc = blueice_html::parse(
            "<style>.a{background:url(s.png)}</style><img src='i.png'><div style=\"background:url('d.png')\"></div><img srcset='r.png 2x, big.png 4x'><img src='data:image/png;base64,AAAA'>",
        );
        let base = Url::parse("http://h/dir/page.html").unwrap();
        let mut linked = LinkedStyles::new();
        linked.insert(
            "css/site.css".into(),
            "x{background:url(../img/l.png)}".into(),
        );
        let refs = image_references(&doc, &linked, &base, 99);
        let resolved: HashMap<_, _> = refs.into_iter().collect();
        assert_eq!(resolved["i.png"], "http://h/dir/i.png");
        assert_eq!(resolved["s.png"], "http://h/dir/s.png");
        assert_eq!(resolved["d.png"], "http://h/dir/d.png");
        assert_eq!(resolved["r.png"], "http://h/dir/r.png");
        assert_eq!(
            resolved["../img/l.png"], "http://h/dir/img/l.png",
            "relative to the sheet"
        );
        assert!(resolved.contains_key("data:image/png;base64,AAAA"));
    }

    #[test]
    fn the_limit_caps_how_many_are_requested() {
        let doc = blueice_html::parse("<img src=a><img src=b><img src=c>");
        let base = Url::parse("http://h/").unwrap();
        assert_eq!(
            image_references(&doc, &LinkedStyles::new(), &base, 2).len(),
            2
        );
    }

    #[test]
    fn pictures_of_rules_that_match_the_page_are_requested_first() {
        // A large sheet lists far more pictures (an icon set) than the limit allows; the ones
        // the page uses must be the ones fetched, not the first in the file.
        let doc = blueice_html::parse("<p class=used>x</p><p id=also>y</p>");
        let base = Url::parse("http://h/").unwrap();
        let mut linked = LinkedStyles::new();
        linked.insert(
            "s.css".to_string(),
            ".u1{mask-image:url(a.png)} .u2 .x{background:url(b.png)} \
             .used{-webkit-mask-image:url(c.png);mask-image:url(c.png)} \
             #also{background-image:url(d.png), linear-gradient(red, blue)} \
             .u3{background:url(e.png)}"
                .to_string(),
        );
        let refs = |limit| -> Vec<String> {
            image_references(&doc, &linked, &base, limit).into_iter().map(|r| r.0).collect()
        };
        assert_eq!(refs(2), ["c.png", "d.png"]);
        // the rest still follow, so nothing a sheet asks for is lost under a larger limit
        let all = refs(10);
        assert_eq!(&all[..2], ["c.png", "d.png"]);
        for other in ["a.png", "b.png", "e.png"] {
            assert!(all.contains(&other.to_string()), "{other} in {all:?}");
        }
    }

    fn styled(
        html: &str,
        css: &str,
    ) -> (
        blueice_dom::Document,
        HashMap<blueice_dom::NodeId, blueice_css::ComputedStyle>,
    ) {
        let doc = blueice_html::parse(html);
        let ua = blueice_css::ua_stylesheet();
        let author = blueice_css::parse(css).rules;
        let styles = blueice_css::cascade(
            &doc,
            &[
                (blueice_css::Origin::Ua, &ua),
                (blueice_css::Origin::Author, &author),
            ],
        );
        (doc, styles)
    }

    fn first_svg(
        doc: &blueice_dom::Document,
        from: blueice_dom::NodeId,
    ) -> Option<blueice_dom::NodeId> {
        if matches!(doc.data(from), blueice_dom::NodeData::Element { tag_name, .. } if tag_name == "svg")
        {
            return Some(from);
        }
        doc.children(from).find_map(|c| first_svg(doc, c))
    }

    #[test]
    fn an_inline_svg_serializes_with_namespaces_and_page_colours() {
        let (doc, styles) = styled(
            "<p style='color:#00ff00'><svg viewBox='0 0 4 4'><path d='M0 0h4v4z'/><text>a&amp;b</text></svg></p>",
            "path { fill: #ff0000; }",
        );
        let svg = first_svg(&doc, doc.root()).unwrap();
        let markup = svg_markup(&doc, &styles, svg, &LinkedImages::new());
        assert!(markup.starts_with("<svg viewBox=\"0 0 4 4\""), "{markup}");
        assert!(markup.contains("xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(
            markup.contains("color:rgba(0,255,0,1)"),
            "currentColor comes from the page: {markup}"
        );
        assert!(markup.contains("fill:rgba(255,0,0,1)"), "{markup}");
        assert!(markup.contains("<text>a&amp;b</text>"));
        // And a real renderer accepts it, drawing the page's red.
        let loaded = render_svg_source(&markup, (4.0, 4.0)).unwrap();
        let p = &loaded.image.rgba[((2 * loaded.image.width + 6) * 4) as usize..][..4];
        assert_eq!(p, [255, 0, 0, 255]);
    }

    #[test]
    fn an_inherited_fill_does_not_override_a_childs_own_fill_attribute() {
        // A root `fill: none` reaches every descendant by inheritance; written out as a `style`
        // on each of them it would beat the path's own `fill="..."` and blank the drawing.
        let (doc, styles) = styled(
            "<svg viewBox='0 0 4 4'><g><path d='M0 0h4v4z' fill='#00ff00'/></g></svg>",
            "svg { fill: none; stroke: #0000ff; } path { stroke-width: 0.2px; }",
        );
        let svg = first_svg(&doc, doc.root()).unwrap();
        let markup = svg_markup(&doc, &styles, svg, &LinkedImages::new());
        assert_eq!(
            markup.matches("fill:none").count(),
            1,
            "only the element that declares it writes it: {markup}"
        );
        assert_eq!(markup.matches("stroke:rgba(0,0,255,1)").count(), 1, "{markup}");
        assert!(markup.contains("stroke-width:0.2"), "{markup}");
        let loaded = render_svg_source(&markup, (4.0, 4.0)).unwrap();
        // (3, 1) in a 4x4 drawing at twice its size: inside the triangle, clear of its edges.
        let p = &loaded.image.rgba[((2 * loaded.image.width + 6) * 4) as usize..][..4];
        assert_eq!(p, [0, 255, 0, 255], "the path keeps its own green fill");
    }

    const SPRITE: &str = "<svg xmlns='http://www.w3.org/2000/svg' style='display:none'>\
        <symbol id='dot' viewBox='0 0 20 20'><rect width='20' height='20' fill='#00ff00'/></symbol>\
        <symbol id='other' viewBox='0 0 20 20'><rect width='20' height='20' fill='#ff0000'/></symbol></svg>";

    #[test]
    fn a_sprite_file_without_a_size_is_kept_as_text_for_use_references() {
        let loaded = decode_loaded(SPRITE.as_bytes()).expect("a sprite is kept");
        assert!(loaded.sprite.as_deref().is_some_and(|t| t.contains("id='dot'")));
        // An ordinary picture has no sprite text.
        let plain = decode_loaded(b"<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'><rect width='4' height='4'/></svg>")
            .unwrap();
        assert!(plain.sprite.is_none());
    }

    #[test]
    fn the_files_an_svg_use_refers_to_are_fetched_with_the_images() {
        let doc = blueice_html::parse(
            "<svg><use href='/icons.svg#dot'></use></svg><svg><use xlink:href='sprites/a.svg#x'></use></svg>\
             <svg><use href='#local'></use></svg>",
        );
        let base = Url::parse("https://example.com/page/").unwrap();
        let wanted = image_references(&doc, &LinkedStyles::default(), &base, 10);
        let urls: Vec<_> = wanted.iter().map(|(raw, url)| (raw.as_str(), url.as_str())).collect();
        assert_eq!(
            urls,
            vec![
                ("/icons.svg", "https://example.com/icons.svg"),
                ("sprites/a.svg", "https://example.com/page/sprites/a.svg"),
            ],
            "the same-document reference needs no file"
        );
    }

    #[test]
    fn an_svg_use_of_an_external_sprite_is_drawn_from_the_fetched_symbol() {
        let (doc, styles) = styled(
            "<svg viewBox='0 0 20 20'><use href='/icons.svg#dot'></use></svg>",
            "",
        );
        let svg = first_svg(&doc, doc.root()).unwrap();
        let mut sprites = LinkedImages::new();
        sprites.insert(
            "/icons.svg".to_string(),
            Arc::new(decode_loaded(SPRITE.as_bytes()).unwrap()),
        );
        let markup = svg_markup(&doc, &styles, svg, &sprites);
        assert!(markup.contains("href=\"#dot\""), "{markup}");
        assert!(markup.contains("<symbol id='dot'") || markup.contains("<symbol id=\"dot\""), "{markup}");
        assert!(!markup.contains("id='other'") && !markup.contains("id=\"other\""), "only what is used: {markup}");
        let loaded = render_svg_source(&markup, (20.0, 20.0)).unwrap();
        let p = &loaded.image.rgba[((20 * loaded.image.width + 20) * 4) as usize..][..4];
        assert_eq!(p, [0, 255, 0, 255], "the symbol is drawn");
        // Without the sprite fetched, the reference stays and draws nothing.
        let bare = svg_markup(&doc, &styles, svg, &LinkedImages::new());
        assert!(!bare.contains("<symbol"), "{bare}");
    }

    #[test]
    fn svg_natural_size_reads_attributes_then_the_view_box() {
        let attrs = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        assert_eq!(
            svg_natural_size(&attrs(&[("width", "24px"), ("height", "12")])),
            Some((24.0, 12.0))
        );
        assert_eq!(
            svg_natural_size(&attrs(&[("viewBox", "0 0 20 10")])),
            Some((20.0, 10.0))
        );
        assert_eq!(
            svg_natural_size(&attrs(&[("width", "40"), ("viewBox", "0 0 20 10")])),
            Some((40.0, 20.0))
        );
        assert_eq!(svg_natural_size(&attrs(&[("width", "100%")])), None);
    }

    #[test]
    fn img_source_prefers_src_over_srcset() {
        let attrs = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        assert_eq!(
            img_source(&attrs(&[("src", " a.png "), ("srcset", "b.png 2x")])),
            Some("a.png".into())
        );
        assert_eq!(
            img_source(&attrs(&[("srcset", "b.png 2x, c.png 3x")])),
            Some("b.png".into())
        );
        assert_eq!(img_source(&attrs(&[("src", "")])), None);
    }
}
