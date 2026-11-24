// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Matching a page's decoded pictures to the nodes that show them, after each
//! cascade: `<img>` by its `src`, an inline `<svg>` by rendering its markup,
//! and CSS `background-image: url()` by the address as written. Replaced
//! elements also learn their natural size here, which layout reads through the
//! `-blueice-intrinsic-size` property.

use super::Page;
use crate::images::{img_source, render_svg_source, svg_markup, svg_natural_size, LoadedImage};
use blueice_css::Value;
use blueice_dom::{NodeData, NodeId};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Renders kept per page before the cache starts over.
const SVG_CACHE_LIMIT: usize = 256;

/// The colour a masked box's `background-color` resolves to (`currentColor` is the text colour),
/// as straight RGBA; `None` when it has no background colour, so the mask shows nothing to cut.
fn background_tint(style: &blueice_css::ComputedStyle) -> Option<[u8; 4]> {
    use blueice_css::Color;
    match style.other.get("background-color")? {
        Value::Color(Color::Rgba(r, g, b, a)) => Some([*r, *g, *b, *a]),
        Value::Color(Color::CurrentColor) => match style.color {
            Color::Rgba(r, g, b, a) => Some([r, g, b, a]),
            Color::CurrentColor => None,
        },
        _ => None,
    }
}

/// `mask`'s pixels coloured `tint`: the mask's alpha times the colour's own.
fn tinted(mask: &blueice_image::RasterImage, tint: [u8; 4]) -> blueice_image::RasterImage {
    let mut rgba = Vec::with_capacity(mask.rgba.len());
    for px in mask.rgba.chunks_exact(4) {
        let alpha = (u32::from(px[3]) * u32::from(tint[3]) + 127) / 255;
        rgba.extend_from_slice(&[tint[0], tint[1], tint[2], alpha as u8]);
    }
    blueice_image::RasterImage::new(mask.width, mask.height, rgba).with_natural(mask.natural())
}

impl Page {
    /// Pictures that styles name as `data:` URLs need no fetching, so those the page's load never
    /// asked for (styles that arrived later: a script's, ResourceLoader's) are decoded when they
    /// are first wanted.
    fn decode_data_urls_in_styles(&mut self) {
        const MAX_DATA_PICTURES: usize = 256;
        let mut wanted: Vec<String> = Vec::new();
        for style in self.styles.values() {
            for property in ["background-image", "mask-image"] {
                if let Some(Value::Unparsed(tokens)) = style.other.get(property) {
                    for layer in blueice_css::background_layers(tokens) {
                        if let Some(url) = blueice_css::url_of(layer) {
                            let url = url.trim();
                            if url.starts_with("data:") && !self.linked_images.contains_key(url) {
                                wanted.push(url.to_string());
                            }
                        }
                    }
                }
            }
        }
        wanted.sort();
        wanted.dedup();
        for url in wanted {
            if self.linked_images.len() >= MAX_DATA_PICTURES {
                break;
            }
            let loaded = crate::images::data_url_bytes(&url)
                .as_deref()
                .and_then(crate::images::decode_loaded);
            if let Some(loaded) = loaded {
                self.linked_images.insert(url, Arc::new(loaded));
            }
        }
    }

    pub(super) fn rebuild_images(&mut self) {
        self.decode_data_urls_in_styles();
        let mut images = blueice_paint::ImageMap::default();
        let mut sizes: Vec<(NodeId, (f64, f64))> = Vec::new();
        let mut masked: Vec<NodeId> = Vec::new();
        let mut stack = vec![(self.doc.root(), false)];
        while let Some((node, in_svg)) = stack.pop() {
            let mut inside = in_svg;
            if let NodeData::Element {
                tag_name,
                attributes,
            } = self.doc.data(node)
            {
                if tag_name == "img" {
                    if let Some(loaded) =
                        img_source(attributes).and_then(|s| self.linked_images.get(&s))
                    {
                        images.content.insert(node, loaded.image.clone());
                        sizes.push((node, loaded.natural));
                    }
                } else if tag_name == "canvas" {
                    let dim = |name: &str, default: f64| {
                        attributes
                            .iter()
                            .find(|(k, _)| k == name)
                            .and_then(|(_, v)| v.trim().parse::<f64>().ok())
                            .filter(|n| *n >= 0.0)
                            .unwrap_or(default)
                    };
                    sizes.push((node, (dim("width", 300.0), dim("height", 150.0))));
                    if let Some(loaded) = self.canvas_pictures.get(&node) {
                        images.content.insert(node, loaded.image.clone());
                    }
                } else if tag_name == "svg" && !in_svg {
                    inside = true;
                    if let Some(natural) = svg_natural_size(attributes) {
                        if let Some(loaded) = self.render_inline_svg(node, natural) {
                            images.content.insert(node, loaded.image.clone());
                            sizes.push((node, loaded.natural));
                        }
                    }
                }
                if let Some(Value::Unparsed(tokens)) = self
                    .styles
                    .get(&node)
                    .and_then(|s| s.other.get("background-image"))
                {
                    // Of a comma-separated list of layers, the first picture that loaded.
                    if let Some(loaded) = blueice_css::background_layers(tokens)
                        .into_iter()
                        .find_map(|layer| {
                            blueice_css::url_of(layer).and_then(|u| self.linked_images.get(u.trim()))
                        })
                    {
                        images.background.insert(node, loaded.image.clone());
                    }
                }
            }
            // `mask-image`: the picture's alpha cuts the background colour into its shape, which is
            // how icon fonts' replacements are drawn. The tinted picture is drawn as the
            // background (sized, placed and repeated by the `mask-*` properties).
            if let Some(style) = self.styles.get(&node) {
                if let Some(Value::Unparsed(tokens)) = style.other.get("mask-image") {
                    if let Some(loaded) = blueice_css::background_layers(tokens)
                        .into_iter()
                        .find_map(|layer| {
                            blueice_css::url_of(layer).and_then(|u| self.linked_images.get(u.trim()))
                        })
                    {
                        if let Some(tint) = background_tint(style) {
                            images.background.insert(node, Arc::new(tinted(&loaded.image, tint)));
                            masked.push(node);
                        }
                    }
                }
            }
            for child in self.doc.children(node) {
                stack.push((child, inside));
            }
        }
        // The boxes made for `::before` and `::after` have pictures of their own too.
        let pseudo: Vec<NodeId> = self.styles.keys().copied().filter(|&id| blueice_css::is_pseudo_node(id)).collect();
        for node in pseudo {
            let Some(style) = self.styles.get(&node) else { continue };
            if let Some(Value::Unparsed(tokens)) = style.other.get("background-image") {
                if let Some(loaded) = blueice_css::background_layers(tokens).into_iter().find_map(|layer| {
                    blueice_css::url_of(layer).and_then(|u| self.linked_images.get(u.trim()))
                }) {
                    images.background.insert(node, loaded.image.clone());
                }
            }
            if let Some(Value::Unparsed(tokens)) = style.other.get("mask-image") {
                if let Some(loaded) = blueice_css::background_layers(tokens).into_iter().find_map(|layer| {
                    blueice_css::url_of(layer).and_then(|u| self.linked_images.get(u.trim()))
                }) {
                    if let Some(tint) = background_tint(style) {
                        images.background.insert(node, Arc::new(tinted(&loaded.image, tint)));
                        masked.push(node);
                    }
                }
            }
        }
        for node in masked {
            if let Some(style) = self.styles.get_mut(&node) {
                style.other.insert("-blueice-masked".to_string(), Value::Number(1.0));
            }
        }
        for (node, (w, h)) in sizes {
            if let Some(style) = self.styles.get_mut(&node) {
                style.other.insert(
                    "-blueice-intrinsic-size".to_string(),
                    Value::List(vec![Value::Number(w), Value::Number(h)]),
                );
            }
        }
        // Boxes with more than one background layer: each picture by its place in the list.
        for (node, style) in &self.styles {
            let Some(Value::List(layers)) = style.other.get("-blueice-background-layers") else { continue };
            for (index, layer) in layers.iter().enumerate() {
                let Value::List(parts) = layer else { continue };
                let Some(Value::Unparsed(tokens)) = parts.first() else { continue };
                let loaded = blueice_css::background_layers(tokens)
                    .into_iter()
                    .find_map(|l| blueice_css::url_of(l).and_then(|u| self.linked_images.get(u.trim())));
                if let Some(loaded) = loaded {
                    images.layers.insert((*node, index), loaded.image.clone());
                }
            }
        }
        self.images = images;
        let lang = document_language(&self.doc);
        crate::fonts::assign_faces(&self.linked_fonts, &mut self.styles, lang.as_deref());
    }

    /// Repoints canvases at their latest pictures without a restyle.
    pub(super) fn refresh_canvas_images(&mut self) {
        for (node, loaded) in &self.canvas_pictures {
            self.images.content.insert(*node, loaded.image.clone());
        }
    }

    fn render_inline_svg(&mut self, node: NodeId, natural: (f64, f64)) -> Option<Arc<LoadedImage>> {
        let markup = svg_markup(&self.doc, &self.styles, node, &self.linked_images);
        let mut hasher = DefaultHasher::new();
        markup.hash(&mut hasher);
        let key = hasher.finish();
        if let Some(known) = self.svg_renders.get(&key) {
            return known.clone();
        }
        if self.svg_renders.len() >= SVG_CACHE_LIMIT {
            self.svg_renders.clear();
        }
        let rendered = render_svg_source(&markup, natural).map(Arc::new);
        if rendered.is_none() && std::env::var_os("BI_DIAG").is_some() {
            eprintln!("inline svg not drawn ({} bytes): {}", markup.len(), &markup[..markup.len().min(400)]);
        }
        self.svg_renders.insert(key, rendered.clone());
        rendered
    }
}

/// The `lang` of the document element (the page's language, which picks its generic fonts).
fn document_language(doc: &blueice_dom::Document) -> Option<String> {
    doc.children(doc.root()).find_map(|html| match doc.data(html) {
        blueice_dom::NodeData::Element { tag_name, attributes } if tag_name == "html" => attributes
            .iter()
            .find(|(k, _)| k == "lang")
            .map(|(_, v)| v.trim().to_string()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_paint::PaintCommand;

    fn image_commands(page: &Page) -> Vec<(f64, f64, f64, f64)> {
        page.render()
            .commands
            .iter()
            .filter_map(|c| match c {
                PaintCommand::Image { rect, .. } => Some((rect.x, rect.y, rect.width, rect.height)),
                _ => None,
            })
            .collect()
    }

    const RED_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='6'><rect width='10' height='6' fill='#ff0000'/></svg>";

    fn loaded(svg: &str) -> Arc<LoadedImage> {
        Arc::new(crate::images::decode_loaded(svg.as_bytes()).unwrap())
    }

    #[test]
    fn an_inline_svg_is_drawn_at_its_layout_box() {
        let mut page = Page::new(400.0, 300.0);
        page.load_html_str(
            "<body style='margin:0'><svg viewBox='0 0 10 10' width='30' height='30'><rect width='10' height='10' fill='#00ff00'/></svg></body>",
            None,
        );
        assert_eq!(image_commands(&page), vec![(0.0, 0.0, 30.0, 30.0)]);
        let pixels = blueice_raster::rasterize(&page.render());
        let i = ((15 * pixels.width + 15) * 4) as usize;
        assert_eq!(&pixels.pixels[i..i + 4], [0, 255, 0, 255]);
    }

    #[test]
    fn a_logo_svg_with_painted_groups_and_a_hidden_light_variant_draws_its_base() {
        // The shape of Cloudflare's header logo: `fill="none"` on the root, a base group whose
        // paths carry their own fill, and a second group faded out with `opacity: 0`.
        let mut page = Page::new(400.0, 300.0);
        page.load_html_str(
            "<style>.logo{height:26px;width:57px;flex-shrink:0;color:#ff5e1f} .fade{transition:opacity .2s}\
             .light{opacity:0}</style>\
             <body style='margin:0'><a style='display:flex;padding:4px'><svg xmlns='http://www.w3.org/2000/svg' \
             width='66' height='30' viewBox='0 0 100 50' fill='none' class='logo'>\
             <g class='base fade'><path d='M0 0H100V50H0Z' fill='#FF5F08'/></g>\
             <g class='light fade' fill='currentColor'><path d='M0 0H10V10H0Z'/></g></svg></a></body>",
            None,
        );
        let boxes = image_commands(&page);
        assert_eq!(boxes.len(), 1, "the logo is one picture: {boxes:?}");
        assert_eq!((boxes[0].2, boxes[0].3), (57.0, 26.0));
        let pixels = blueice_raster::rasterize(&page.render());
        let at = |x: u32, y: u32| {
            let i = ((y * pixels.width + x) * 4) as usize;
            [pixels.pixels[i], pixels.pixels[i + 1], pixels.pixels[i + 2]]
        };
        // Inside the logo, away from the faded corner: the base group's orange.
        assert_eq!(at((boxes[0].0 + 40.0) as u32, (boxes[0].1 + 18.0) as u32), [0xFF, 0x5F, 0x08]);
    }

    #[test]
    fn an_svg_with_a_height_and_auto_width_keeps_its_view_box_ratio() {
        // The header wordmark: only a `viewBox`, `height: 12px; width: auto`, inside a column
        // flex box that aligns its items to the start, itself inside an inline-flex link.
        for wrapper in [
            "<div>{}</div>",
            "<div style='display:flex;flex-direction:column;align-items:flex-start'>{}</div>",
            "<a style='display:flex;align-items:center'><div style='display:none;flex-direction:column;align-items:flex-start;margin-left:12px'>{}</div></a>",
            "<a style='display:flex;align-items:center'><div style='display:flex;flex-direction:column;align-items:flex-start;margin-left:12px'>{}</div></a>",
        ] {
            let svg = "<svg viewBox='0 0 720 60' style='height:12px;width:auto' fill='none'><path d='M0 0h720v60H0z' fill='#000'/></svg>";
            let html = format!("<body style='margin:0'>{}</body>", wrapper.replace("{}", svg));
            let mut page = Page::new(400.0, 300.0);
            page.load_html_str(&html, None);
            let sizes: Vec<_> = image_commands(&page).iter().map(|r| (r.2, r.3)).collect();
            if wrapper.contains("display:none") {
                assert!(sizes.is_empty(), "{wrapper}: {sizes:?}");
            } else {
                assert_eq!(sizes, vec![(144.0, 12.0)], "{wrapper}");
            }
        }
    }

    #[test]
    fn an_svg_icon_in_a_row_flex_link_takes_its_size_from_height_and_view_box() {
        // `<a class="flex"><span>Products</span><svg class="h-4 w-auto" viewBox="0 0 20 20">`:
        // the icon is a flex item with no content, sized by its picture alone.
        let mut page = Page::new(400.0, 300.0);
        page.load_html_str(
            "<body style='margin:0'><a style='display:flex;align-items:center'><span>Products</span>\
             <svg viewBox='0 0 20 20' style='display:inline-block;height:16px;width:auto;flex-shrink:0;margin-left:6px'>\
             <path d='M0 0h20v20H0z' fill='#000'/></svg></a></body>",
            None,
        );
        let sizes: Vec<_> = image_commands(&page).iter().map(|r| (r.2, r.3)).collect();
        assert_eq!(sizes, vec![(16.0, 16.0)]);
    }

    #[test]
    fn a_tailwind_centering_translate_moves_the_box_by_half_its_own_width() {
        // Tailwind v4's `-translate-x-1/2`: a custom property holds the amount, a second one
        // (`--tw-translate-y`) has no value except its `@property` initial value, and the
        // `translate` property joins them.
        let mut page = Page::new(800.0, 300.0);
        page.load_html_str(
            "<style>\
             @property --tw-translate-x { syntax: '*'; inherits: false; initial-value: 0; }\
             @property --tw-translate-y { syntax: '*'; inherits: false; initial-value: 0; }\
             .l{left:50%} .t{--tw-translate-x:calc(1/2 * -100%);translate:var(--tw-translate-x) var(--tw-translate-y)}\
             .w{position:absolute;width:200px;height:40px} .r{position:relative;height:100px}\
             </style><body style='margin:0'><div class=r>\
             <svg class='w l t' viewBox='0 0 10 10'><rect width='10' height='10'/></svg></div></body>",
            None,
        );
        assert_eq!(image_commands(&page), vec![(300.0, 0.0, 200.0, 40.0)]);
    }

    #[test]
    fn an_svg_without_width_or_height_takes_its_view_box_size() {
        let mut page = Page::new(400.0, 300.0);
        page.load_html_str(
            "<body style='margin:0'><svg viewBox='0 0 24 12'><rect width='24' height='12'/></svg></body>",
            None,
        );
        let sizes: Vec<_> = image_commands(&page).iter().map(|r| (r.2, r.3)).collect();
        assert_eq!(sizes, vec![(24.0, 12.0)]);
    }

    #[test]
    fn an_img_uses_its_fetched_picture_and_natural_size() {
        let mut page = Page::new(400.0, 300.0);
        page.pending_linked_images
            .insert("a.svg".to_string(), loaded(RED_SVG));
        page.load_html_str("<body style='margin:0'><img src='a.svg'></body>", None);
        let sizes: Vec<_> = image_commands(&page).iter().map(|r| (r.2, r.3)).collect();
        assert_eq!(sizes, vec![(10.0, 6.0)]);
    }

    #[test]
    fn an_img_without_its_picture_keeps_its_attributes_and_paints_nothing() {
        let mut page = Page::new(400.0, 300.0);
        page.load_html_str(
            "<body style='margin:0'><img src='missing.png' width='20' height='10'><p id=after>x</p></body>",
            None,
        );
        assert!(image_commands(&page).is_empty());
        assert!(
            page.render()
                .commands
                .iter()
                .any(|c| matches!(c, PaintCommand::Text { y, .. } if *y >= 10.0)),
            "layout still reserves the attribute size"
        );
    }

    #[test]
    fn a_css_background_url_finds_its_picture() {
        let mut page = Page::new(400.0, 300.0);
        page.pending_linked_images
            .insert("bg.svg".to_string(), loaded(RED_SVG));
        page.load_html_str(
            "<style>div{width:30px;height:12px;background:url(bg.svg) no-repeat}</style><body style='margin:0'><div></div></body>",
            None,
        );
        let rects = image_commands(&page);
        assert_eq!(rects.len(), 1);
        assert_eq!((rects[0].2, rects[0].3), (10.0, 6.0));
        // with no position given the picture sits at the top left, not vertically centred
        assert_eq!((rects[0].0, rects[0].1), (0.0, 0.0));
    }

    #[test]
    fn a_picture_in_a_multi_layer_background_still_finds_its_picture() {
        // `background: url(x), linear-gradient(...)` is how pages (Hacker News's vote arrows)
        // give a picture a fallback: the picture is one layer of a comma-separated list.
        for rule in [
            "background-image: url(\"bg.svg\"), linear-gradient(transparent, transparent); background-repeat: no-repeat",
            "background: linear-gradient(transparent, transparent), url(bg.svg) no-repeat",
            "background: url(missing.png), url(bg.svg) no-repeat",
        ] {
            let mut page = Page::new(400.0, 300.0);
            page.pending_linked_images
                .insert("bg.svg".to_string(), loaded(RED_SVG));
            page.load_html_str(
                &format!("<style>div{{width:30px;height:12px;{rule}}}</style><body style='margin:0'><div></div></body>"),
                None,
            );
            assert_eq!(image_commands(&page).len(), 1, "{rule}");
        }
        // `no-repeat` after the gradient belongs to the gradient's layer: the picture's own layer
        // repeats, as it does in Safari (a 10x6 picture tiles 3 x 2 times in 30 x 12).
        let mut page = Page::new(400.0, 300.0);
        page.pending_linked_images
            .insert("bg.svg".to_string(), loaded(RED_SVG));
        page.load_html_str(
            "<style>div{width:30px;height:12px;background: url(bg.svg), linear-gradient(transparent, transparent) no-repeat}</style><body style='margin:0'><div></div></body>",
            None,
        );
        assert_eq!(image_commands(&page).len(), 6);
    }

    #[test]
    fn a_mask_image_shapes_the_background_colour() {
        // Icons are drawn this way: a flat `background-color` seen through a picture's alpha.
        let pixel = |css: &str, x: u32, y: u32| {
            let mut page = Page::new(100.0, 60.0);
            page.pending_linked_images
                .insert("m.svg".to_string(), loaded(RED_SVG));
            page.load_html_str(
                &format!("<style>body{{margin:0}} div{{width:30px;height:30px;background:rgb(0,0,255);{css}}}</style><div></div>"),
                None,
            );
            page.render_visible().get_pixel(x, y)
        };
        let blue = [0, 0, 255, 255];
        let white = [255, 255, 255, 255];
        // no mask: the whole box is blue
        assert_eq!(pixel("", 25, 25), blue);
        for mask in [
            "mask-image: url(m.svg); mask-repeat: no-repeat",
            "-webkit-mask-image: url(m.svg); -webkit-mask-repeat: no-repeat",
            "mask: url(m.svg) no-repeat",
        ] {
            // the 10x6 picture is opaque: blue shows through it, and nowhere else
            assert_eq!(pixel(mask, 3, 3), blue, "{mask}");
            assert_eq!(pixel(mask, 25, 25), white, "{mask}");
        }
        // the mask can be sized and placed like a background
        let sized = "mask-image: url(m.svg); mask-repeat: no-repeat; mask-size: 30px 30px";
        assert_eq!(pixel(sized, 25, 25), blue);
        let centred = "mask-image: url(m.svg); mask-repeat: no-repeat; mask-position: center";
        assert_eq!(pixel(centred, 15, 15), blue);
        assert_eq!(pixel(centred, 3, 3), white);
    }

    #[test]
    fn a_mask_given_as_an_inline_data_url_is_drawn_even_when_nothing_fetched_it() {
        // ResourceLoader's styles arrive after the page loaded: nothing fetched their icons, but a
        // `data:` URL needs no fetching.
        let mut page = Page::new(400.0, 300.0);
        let url = format!("data:image/svg+xml;utf8,{RED_SVG}");
        page.load_html_str(
            &format!("<style>body{{margin:0}} div{{width:30px;height:30px;background:rgb(0,0,255);mask-image:url(\"{url}\");mask-repeat:no-repeat}}</style><div></div>"),
            None,
        );
        let pixel = |x, y| page.render_visible().get_pixel(x, y);
        assert_eq!(pixel(3, 3), [0, 0, 255, 255], "inside the picture the colour shows");
        assert_eq!(pixel(25, 25), [255, 255, 255, 255], "outside it nothing does");
    }

    #[test]
    fn a_pseudo_element_mask_cuts_its_colour_to_the_pictures_shape() {
        // Wikipedia's dropdown arrow is `label::after { content: ""; mask-image: url(...); background: blue }`.
        let mut page = Page::new(400.0, 300.0);
        let url = format!("data:image/svg+xml;utf8,{RED_SVG}");
        page.load_html_str(
            &format!("<style>body{{margin:0}} div::after{{content:'';display:block;width:30px;height:30px;background:rgb(0,0,255);mask-image:url(\"{url}\");mask-repeat:no-repeat}}</style><div></div>"),
            None,
        );
        let pixel = |x, y| page.render_visible().get_pixel(x, y);
        assert_eq!(pixel(3, 3), [0, 0, 255, 255], "inside the picture the colour shows");
        assert_eq!(pixel(25, 25), [255, 255, 255, 255], "outside it nothing does");
        // A plain background picture on a generated box is drawn as well.
        let mut page = Page::new(400.0, 300.0);
        page.load_html_str(
            &format!("<style>body{{margin:0}} div::after{{content:'';display:block;width:30px;height:30px;background:url(\"{url}\") no-repeat}}</style><div></div>"),
            None,
        );
        assert_eq!(page.render_visible().get_pixel(3, 3), [255, 0, 0, 255]);
    }

    #[test]
    fn every_layer_of_a_multi_layer_background_is_painted_with_its_own_placement() {
        // A 10x6 red picture at the top left, a 10x6 blue one at 20px 20px, and a green gradient
        // below both.
        let mut page = Page::new(400.0, 300.0);
        let red = format!("data:image/svg+xml;utf8,{RED_SVG}");
        let blue = format!("data:image/svg+xml;utf8,{}", RED_SVG.replace("#ff0000", "#0000ff"));
        page.load_html_str(
            &format!("<style>body{{margin:0}} div{{width:40px;height:40px;background:url(\"{red}\") no-repeat, url(\"{blue}\") no-repeat 20px 20px, linear-gradient(rgb(0,255,0), rgb(0,255,0))}}</style><div></div>"),
            None,
        );
        let pixel = |x, y| page.render_visible().get_pixel(x, y);
        assert_eq!(pixel(3, 3), [255, 0, 0, 255], "the first layer is on top");
        assert_eq!(pixel(23, 22), [0, 0, 255, 255], "the second layer sits where it was put");
        assert_eq!(pixel(35, 5), [0, 255, 0, 255], "the layer below shows elsewhere");
    }

    #[test]
    fn a_page_that_restyles_keeps_its_images() {
        let mut page = Page::new(400.0, 300.0);
        page.pending_linked_images
            .insert("a.svg".to_string(), loaded(RED_SVG));
        page.load_html_str("<body><img src='a.svg'></body>", None);
        page.resize(500.0, 300.0);
        assert_eq!(image_commands(&page).len(), 1);
    }

    /// Developer tool, not a test: fetches `BI_URL` for real (stylesheets and
    /// images included, with no gatekeeper) and writes what BlueIce paints to
    /// `BI_OUT` (default `/tmp/live.png`) at `BI_WIDTH` (default 1280):
    /// `BI_URL=https://example.com cargo test -p blueice-engine --release --lib
    /// live_page_to_png -- --ignored --nocapture`
    #[test]
    #[ignore = "needs the network; run by hand"]
    fn live_page_to_png() {
        let Ok(url) = std::env::var("BI_URL") else {
            return;
        };
        let out = std::env::var("BI_OUT").unwrap_or_else(|_| "/tmp/live.png".to_string());
        blueice_font::system::set_enabled(std::env::var("BI_SYSFONTS").map_or(true, |v| v != "0"));
        let width: f64 = std::env::var("BI_WIDTH")
            .ok()
            .and_then(|w| w.parse().ok())
            .unwrap_or(1280.0);
        let started = std::time::Instant::now();
        let fetched = blueice_net::fetch(&url).expect("page fetch");
        let review = |_: &str| true;
        let styles = crate::linked_styles::fetch_linked_styles(
            &fetched.body,
            &fetched.final_url,
            &review,
            crate::linked_styles::DEFAULT_SHEET_LIMITS,
        );
        if let Ok(dir) = std::env::var("BI_SAVE") {
            // Developer aid: keep the page and its sheets to read what the layout is given.
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(format!("{dir}/page.html"), &fetched.body);
            for (n, (href, css)) in styles.iter().enumerate() {
                let _ = std::fs::write(format!("{dir}/sheet{n}.css"), format!("/* {href} */\n{css}"));
            }
        }
        let linked_images = crate::images::fetch_linked_images(
            &fetched.body,
            &styles,
            &fetched.final_url,
            &review,
            crate::images::DEFAULT_IMAGE_LIMITS,
        );
        eprintln!(
            "fetched {} sheets and {} images in {:?}",
            styles.len(),
            linked_images.len(),
            started.elapsed()
        );
        let height: f64 = std::env::var("BI_HEIGHT").ok().and_then(|h| h.parse().ok()).unwrap_or(900.0);
        let mut page = Page::new(width, height);
        let scripts = std::env::var("BI_SCRIPTS").is_ok();
        if scripts {
            page.set_web_scripts(true);
            if std::env::var("BI_ASYNC").is_ok() {
                page.set_async_scripts(true);
            }
            page.set_network_review(std::sync::Arc::new(|_| true));
            page.pending_linked_scripts = crate::scripts_fetch::fetch_linked_scripts(
                &fetched.body,
                &fetched.final_url,
                &|_| true,
                crate::scripts_fetch::DEFAULT_SCRIPT_LIMITS,
            );
            page.pending_linked_modules = crate::scripts_fetch::fetch_linked_modules(
                &fetched.body,
                &page.pending_linked_scripts,
                &fetched.final_url,
                &|_| true,
                crate::scripts_fetch::DEFAULT_MODULE_LIMITS,
            );
            if let Ok(needle) = std::env::var("BI_FIND") {
                for (key, text) in &page.pending_linked_modules {
                    if key.contains(&needle) {
                        eprintln!("HAVE {key} ({} bytes)", text.len());
                    }
                    if let Some(at) = text.find(&needle) {
                        let from = at.saturating_sub(60);
                        eprintln!(
                            "REFERENCED in {key}: ...{}...",
                            &text[from..(at + needle.len() + 20).min(text.len())]
                                .replace('\n', " ")
                        );
                    }
                }
            }
            if std::env::var("BI_DIAG").is_ok() {
                for (key, text) in &page.pending_linked_modules {
                    match blueice_bluejs::parse_module(text) {
                        Ok(m) => {
                            if let Err(e) = blueice_bluejs::compile_module(&m) {
                                eprintln!("COMPILE-ERR {key}: {e:?}");
                            }
                            for r in &m.requests {
                                let target = crate::scripts_fetch::resolve_key(key, &r.specifier);
                                if !page.pending_linked_modules.contains_key(&target) {
                                    eprintln!("MISSING {target} (imported by {key})");
                                }
                            }
                        }
                        Err(e) => eprintln!("PARSE-ERR {key}: {e:?}"),
                    }
                }
            }
            if let Ok(dir) = std::env::var("BI_SAVE") {
                let _ = std::fs::create_dir_all(format!("{dir}/scripts"));
                for (n, (src, text)) in page.pending_linked_scripts.iter().enumerate() {
                    let _ = std::fs::write(format!("{dir}/scripts/{n}.js"), format!("// {src}\n{text}"));
                }
                let _ = std::fs::create_dir_all(format!("{dir}/modules"));
                for (n, (key, text)) in page.pending_linked_modules.iter().enumerate() {
                    let _ = std::fs::write(format!("{dir}/modules/{n}.js"), format!("// {key}\n{text}"));
                }
            }
            eprintln!(
                "fetched {} scripts and {} modules",
                page.pending_linked_scripts.len(),
                page.pending_linked_modules.len()
            );
        }
        let font_started = std::time::Instant::now();
        page.pending_linked_fonts = crate::fonts::fetch_linked_fonts(
            &fetched.body,
            &styles,
            &fetched.final_url,
            &|_| true,
            crate::fonts::DEFAULT_FONT_LIMITS,
        );
        eprintln!("fetched fonts in {:?}", font_started.elapsed());
        page.pending_linked_styles = styles;
        page.pending_linked_images = linked_images;
        let laid_out = std::time::Instant::now();
        page.load_html_str(&fetched.body, Some(fetched.final_url.clone()));
        eprintln!("styled, scripted and laid out in {:?}", laid_out.elapsed());
        if scripts {
            // Let the page's timers run for a while, as the session tick would.
            let until = std::time::Instant::now()
                + std::time::Duration::from_secs(
                    std::env::var("BI_WAIT")
                        .ok()
                        .and_then(|w| w.parse().ok())
                        .unwrap_or(3),
                );
            let mut turns = 0;
            while std::time::Instant::now() < until {
                if page.tick_web() {
                    turns += 1;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if std::env::var("BI_DOC_COST").is_ok() {
                let t = std::time::Instant::now();
                let bytes = page.doc.encode();
                let encoded = t.elapsed();
                let t = std::time::Instant::now();
                let back = blueice_dom::Document::decode(&bytes);
                eprintln!(
                    "DOCCOST nodes ~ {} bytes {} encode {:?} decode {:?} ok {}",
                    page.doc.next_node_id(),
                    bytes.len(),
                    encoded,
                    t.elapsed(),
                    back.is_some()
                );
            }
            if let Some(to) = std::env::var("BI_SCROLL_TO")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
            {
                // Scroll straight to one place and let the scripts run there.
                page.scroll_by(to);
                let until = std::time::Instant::now()
                    + std::time::Duration::from_secs(
                        std::env::var("BI_SCROLL_SECS")
                            .ok()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(40),
                    );
                while std::time::Instant::now() < until {
                    page.tick_web();
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
            if std::env::var("BI_SCROLL").is_ok() {
                // Scroll through the page as a reader would, then back to the top.
                let height = page.fragment.height;
                let mut y = 0.0;
                while y < height {
                    page.scroll_by(850.0);
                    for _ in 0..20 {
                        page.tick_web();
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    y += 850.0;
                }
                page.scroll_by(-height);
                for _ in 0..30 {
                    page.tick_web();
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            eprintln!("{turns} timer turns changed the document");
            if let Some(n) = std::env::var("BI_CASCADE_LOOP")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
            {
                // Only the cascade, repeated, so a sampling profiler sees nothing else.
                let started = std::time::Instant::now();
                for _ in 0..n {
                    page.recascade();
                }
                eprintln!("CASCADE x{n}: {:?} each", started.elapsed() / n);
            }
            if let Ok(probe) = std::env::var("BI_EVAL") {
                page.web_eval(&probe);
                // `BI_WAIT_AFTER` seconds more for what the probe started to finish.
                let more: u64 = std::env::var("BI_WAIT_AFTER").ok().and_then(|w| w.parse().ok()).unwrap_or(0);
                let until = std::time::Instant::now() + std::time::Duration::from_secs(more);
                while std::time::Instant::now() < until {
                    page.tick_web();
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            // A probe's own lines (`console.log('EVAL ...')`) and its failures, found among
            // whatever noise the page made.
            for line in page.web_console().iter().filter(|l| {
                l.starts_with("[log] EVAL") || l.starts_with("[error] Uncaught") || l.starts_with("[error] TypeError")
            }) {
                eprintln!("eval: {}", &line[..line.len().min(300)]);
            }
            for line in page.web_console().iter().take(60) {
                eprintln!("console: {}", &line[..line.len().min(300)]);
            }
        }
        if std::env::var("BI_DIAG").is_ok() {
            let mut hidden = 0;
            for (id, style) in &page.styles {
                let zero =
                    matches!(style.other.get("opacity"), Some(Value::Number(n)) if *n == 0.0);
                let vis = matches!(style.other.get("visibility"), Some(Value::Keyword(k)) if k == "hidden");
                if (zero || vis) && !blueice_css::is_pseudo_node(*id) {
                    hidden += 1;
                    if hidden <= 400 {
                        if let NodeData::Element {
                            tag_name,
                            attributes,
                        } = page.doc.data(*id)
                        {
                            let class = attributes
                                .iter()
                                .find(|(k, _)| k == "class")
                                .map(|(_, v)| v.as_str())
                                .unwrap_or("");
                            eprintln!(
                                "hidden({}) <{tag_name} class=\"{}\">",
                                if zero { "opacity" } else { "visibility" },
                                &class[..class.len().min(80)]
                            );
                        }
                    }
                }
            }
            eprintln!("{hidden} elements hidden by opacity:0 or visibility:hidden");
        }
        if let Ok(range) = std::env::var("BI_DUMP") {
            // Developer aid: print every box overlapping `y0,y1`, with its element.
            let mut bounds = range.split(',').filter_map(|v| v.parse::<f64>().ok());
            let (y0, y1) = (bounds.next().unwrap_or(0.0), bounds.next().unwrap_or(900.0));
            // Whole pixels round halves up, as a browser's reported geometry does.
            fn r(v: f64) -> f64 {
                if std::env::var("BI_FRAC").is_ok() { v } else { (v + 1e-6).round() }
            }
            fn walk(page: &Page, f: &blueice_layout::Fragment, depth: usize, ox: f64, oy: f64, y0: f64, y1: f64) {
                let (ax, ay) = (ox + f.x, oy + f.y);
                if ay + f.height >= y0 && ay <= y1 && !matches!(f.kind, blueice_layout::FragmentKind::Text { .. }) {
                    let name = f.node.map_or(String::new(), |n| match page.doc.data(n) {
                        blueice_dom::NodeData::Element { tag_name, attributes } => {
                            let get = |k: &str| {
                                attributes.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str()).unwrap_or("")
                            };
                            format!("<{tag_name} id={} class={}>", get("id"), get("class"))
                        }
                        _ => String::new(),
                    });
                    let style = f.node.and_then(|n| page.styles.get(&n)).map_or(String::new(), |s| {
                        let mut keys: Vec<_> = s
                            .other
                            .iter()
                            .filter(|(k, _)| {
                                k.starts_with("grid")
                                    || k.ends_with("gap")
                                    || std::env::var("BI_KEYS").is_ok_and(|w| w.split(',').any(|x| x == k.as_str()))
                            })
                            .map(|(k, v)| format!("{k}={v:?}"))
                            .collect();
                        keys.sort();
                        format!(" [{} {}]", s.display, keys.join(" "))
                    });
                    // Absolute page coordinates, so they can be set beside a real browser's.
                    eprintln!(
                        "{}{}{} x={:.p$} y={:.p$} w={:.p$} h={:.p$}",
                        "  ".repeat(depth),
                        name,
                        style,
                        r(ax),
                        r(ay),
                        r(f.width),
                        r(f.height),
                        p = if std::env::var("BI_FRAC").is_ok() { 4 } else { 0 }
                    );
                }
                for child in &f.children {
                    walk(page, child, depth + 1, ax, ay, y0, y1);
                }
            }
            walk(&page, &page.fragment, 0, 0.0, 0.0, y0, y1);
        }
        if let Ok(class) = std::env::var("BI_CLASS") {
            // Developer aid: the computed display of every element carrying `class`.
            for (node, style) in page.styles.iter().filter(|(n, _)| !blueice_css::is_pseudo_node(**n)) {
                if let blueice_dom::NodeData::Element { tag_name, attributes } = page.doc.data(*node) {
                    let has = attributes.iter().any(|(k, v)| (k == "class" && v.split_whitespace().any(|c| c == class)) || (k == "id" && v == &class) || class == format!("<{tag_name}>"));
                    if has {
                        eprintln!("CLASS {class}: GO={:.2} <{tag_name}> display={} font={:?} size={} lh={:?} attrs={attributes:?} other={:?}", blueice_font::measure_text_width_in("GO", 16.0, false, false, match style.other.get("-blueice-web-font") { Some(blueice_css::Value::Number(n)) => Some(blueice_font::web::WebFontId::from_u32(*n as u32)), _ => None }), style.display, style.font_family, style.font_size_px, style.line_height, style.other);
                    }
                }
            }
        }
        let pixmap = blueice_raster::rasterize(&page.render());
        eprintln!(
            "{}x{}, {} images painted",
            pixmap.width,
            pixmap.height,
            image_commands(&page).len()
        );
        pixmap
            .save_png(std::path::Path::new(&out))
            .expect("writable PNG path");
    }
}
