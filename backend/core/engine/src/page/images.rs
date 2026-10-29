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

impl Page {
    pub(super) fn rebuild_images(&mut self) {
        let mut images = blueice_paint::ImageMap::default();
        let mut sizes: Vec<(NodeId, (f64, f64))> = Vec::new();
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
                    if let Some(loaded) =
                        blueice_css::url_of(tokens).and_then(|u| self.linked_images.get(u.trim()))
                    {
                        images.background.insert(node, loaded.image.clone());
                    }
                }
            }
            for child in self.doc.children(node) {
                stack.push((child, inside));
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
        self.images = images;
    }

    fn render_inline_svg(&mut self, node: NodeId, natural: (f64, f64)) -> Option<Arc<LoadedImage>> {
        let markup = svg_markup(&self.doc, &self.styles, node);
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
        self.svg_renders.insert(key, rendered.clone());
        rendered
    }
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
        let mut page = Page::new(width, 900.0);
        page.pending_linked_styles = styles;
        page.pending_linked_images = linked_images;
        let laid_out = std::time::Instant::now();
        page.load_html_str(&fetched.body, Some(fetched.final_url.clone()));
        eprintln!("styled and laid out in {:?}", laid_out.elapsed());
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
