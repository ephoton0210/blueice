// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! [`Page`]: the stateful session `blueice-core`'s process loop drives
//! from incoming `blueice_ipc::ClientMessage`s -- navigation, resize,
//! scroll, and click/hit-testing, per
//! `phase-4-human-rendering-path/PLAN.md`'s checklist. Unlike
//! [`crate::render`] (which redoes every pipeline stage from scratch
//! for one fixture assertion), a `Page` keeps its DOM/styles/fragment
//! tree around across calls so resize/scroll/click don't have to
//! re-fetch or re-parse anything that didn't change.
//!
//! Scrolling is deliberately not real viewport-clipped layout --
//! `research/layout.md`'s MVP cut means layout always computes a box's
//! full intrinsic height regardless of viewport size, so "scrolling"
//! here is "rasterize the whole page, then show a vertical slice of
//! it" ([`Page::render_visible`]), not overflow/clipping during layout
//! itself.

use blueice_css::{cascade, ua_stylesheet, ComputedStyle, Origin, Rule};
use blueice_dom::{Document, NodeData, NodeId};
use blueice_layout::{layout, Constraints, Fragment};
use blueice_paint::{paint, Frame};
use blueice_raster::{rasterize, Pixmap};
use std::collections::HashMap;

pub struct Page {
    doc: Document,
    styles: HashMap<NodeId, ComputedStyle>,
    fragment: Fragment,
    ua: Vec<Rule>,
    viewport_width: f64,
    viewport_height: f64,
    scroll_y: f64,
    url: Option<String>,
}

impl Page {
    pub fn new(viewport_width: f64, viewport_height: f64) -> Self {
        Page {
            doc: Document::new(),
            styles: HashMap::new(),
            fragment: Fragment::empty_block(),
            ua: ua_stylesheet(),
            viewport_width,
            viewport_height,
            scroll_y: 0.0,
            url: None,
        }
    }

    fn load_html(&mut self, html: &str) {
        self.doc = blueice_html::parse(html);
        let author = crate::stylesheet::extract_inline_stylesheets(&self.doc);
        self.styles = cascade(&self.doc, &[(Origin::Ua, &self.ua), (Origin::Author, &author)]);
        self.scroll_y = 0.0;
        self.relayout();
    }

    fn relayout(&mut self) {
        self.fragment = layout(&self.doc, self.doc.root(), &self.styles, Constraints { available_width: self.viewport_width });
        let max_scroll = (self.fragment.height - self.viewport_height).max(0.0);
        self.scroll_y = self.scroll_y.min(max_scroll);
    }

    /// Fetches `url` over the network and loads it as the current page.
    pub fn navigate(&mut self, url: &str) -> Result<(), blueice_net::FetchError> {
        let fetched = blueice_net::fetch(url)?;
        self.load_html(&fetched.body);
        self.url = Some(fetched.final_url);
        Ok(())
    }

    /// Loads `html` directly, with no network fetch -- used by tests,
    /// and by `blueice-core` for its initial blank/about page.
    pub fn load_html_str(&mut self, html: &str, url: Option<String>) {
        self.load_html(html);
        self.url = url;
    }

    pub fn resize(&mut self, width: f64, height: f64) {
        self.viewport_width = width;
        self.viewport_height = height;
        self.relayout();
    }

    pub fn scroll_by(&mut self, delta_y: f64) {
        let max_scroll = (self.fragment.height - self.viewport_height).max(0.0);
        self.scroll_y = (self.scroll_y + delta_y).clamp(0.0, max_scroll);
    }

    /// Hit-tests a click at viewport coordinates (already relative to
    /// what's currently visible; the current scroll offset is applied
    /// here to reach content coordinates) against the layout tree,
    /// returning the URL to follow if the click landed on an `<a
    /// href>` (the clicked node itself or any ancestor up to the
    /// nearest one).
    pub fn click(&self, x: f64, y: f64) -> Option<String> {
        let content_y = y + self.scroll_y;
        let node = hit_test(&self.fragment, x, content_y)?;
        nearest_link_href(&self.doc, node)
    }

    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    pub fn viewport_size(&self) -> (f64, f64) {
        (self.viewport_width, self.viewport_height)
    }

    pub fn scroll_y(&self) -> f64 {
        self.scroll_y
    }

    pub fn render(&self) -> Frame {
        paint(&self.fragment, &self.styles)
    }

    /// Rasterizes the full page, then crops to the current viewport at
    /// the current scroll offset -- see module docs for why this is
    /// how "scrolling" works without real overflow-clipped layout.
    pub fn render_visible(&self) -> Pixmap {
        let full = rasterize(&self.render());
        crop(&full, self.scroll_y, self.viewport_width, self.viewport_height)
    }
}

fn crop(pixmap: &Pixmap, top: f64, width: f64, height: f64) -> Pixmap {
    let top = top.round().max(0.0) as u32;
    let w = (width.round().max(0.0) as u32).min(pixmap.width.max(1));
    let h = height.round().max(0.0) as u32;
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for row in 0..h {
        let src_row = top + row;
        if src_row < pixmap.height {
            let start = ((src_row * pixmap.width) * 4) as usize;
            let end = start + (w as usize * 4);
            pixels.extend_from_slice(&pixmap.pixels[start..end.min(pixmap.pixels.len())]);
            pixels.resize((row as usize + 1) * w as usize * 4, 255);
        } else {
            pixels.extend(std::iter::repeat_n(255u8, w as usize * 4));
        }
    }
    Pixmap { width: w, height: h, pixels }
}

fn hit_test(fragment: &Fragment, x: f64, y: f64) -> Option<NodeId> {
    hit_test_rec(fragment, x, y, 0.0, 0.0)
}

fn hit_test_rec(fragment: &Fragment, x: f64, y: f64, offset_x: f64, offset_y: f64) -> Option<NodeId> {
    let fx = offset_x + fragment.x;
    let fy = offset_y + fragment.y;
    if x < fx || y < fy || x > fx + fragment.width || y > fy + fragment.height {
        return None;
    }
    for child in &fragment.children {
        if let Some(hit) = hit_test_rec(child, x, y, fx, fy) {
            return Some(hit);
        }
    }
    fragment.node
}

fn nearest_link_href(doc: &Document, mut node: NodeId) -> Option<String> {
    loop {
        if let NodeData::Element { tag_name, attributes } = doc.data(node) {
            if tag_name == "a" {
                if let Some((_, href)) = attributes.iter().find(|(k, _)| k == "href") {
                    return Some(href.clone());
                }
            }
        }
        node = doc.parent(node)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_paint::PaintCommand;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn new_page_is_blank_with_no_url() {
        let page = Page::new(320.0, 200.0);
        assert_eq!(page.url(), None);
        assert!(page.render().commands.is_empty());
    }

    #[test]
    fn load_html_str_sets_url_and_renders_content() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str("<p>hi</p>", Some("about:blank".to_string()));
        assert_eq!(page.url(), Some("about:blank"));
        assert!(page.render().commands.iter().any(|c| matches!(c, PaintCommand::Text { text, .. } if text == "hi")));
    }

    #[test]
    fn navigate_fetches_over_the_network_and_loads_the_body() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let body = "<p>fetched</p>";
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
        });
        let mut page = Page::new(320.0, 200.0);
        page.navigate(&format!("http://{addr}")).unwrap();
        assert!(page.render().commands.iter().any(|c| matches!(c, PaintCommand::Text { text, .. } if text == "fetched")));
    }

    fn distinct_line_count(frame: &Frame) -> usize {
        let mut ys: Vec<i64> = frame
            .commands
            .iter()
            .filter_map(|c| match c {
                PaintCommand::Text { y, .. } => Some(y.round() as i64),
                _ => None,
            })
            .collect();
        ys.sort_unstable();
        ys.dedup();
        ys.len()
    }

    #[test]
    fn resize_relayouts_at_the_new_width() {
        let mut page = Page::new(1000.0, 200.0);
        page.load_html_str("<p>aaaa bbbb cccc dddd eeee</p>", None);
        let wide_lines = distinct_line_count(&page.render());
        page.resize(50.0, 200.0);
        let narrow_lines = distinct_line_count(&page.render());
        assert!(narrow_lines > wide_lines, "a much narrower viewport must wrap onto more lines");
    }

    #[test]
    fn scroll_clamps_to_the_content_range() {
        let mut page = Page::new(100.0, 20.0);
        page.load_html_str("<div style=\"height: 500px;\"></div>", None);
        page.scroll_by(-100.0);
        assert_eq!(page.scroll_y(), 0.0, "cannot scroll above the top");
        page.scroll_by(10_000.0);
        assert!(page.scroll_y() > 0.0 && page.scroll_y() <= 500.0, "cannot scroll past the bottom of the content");
    }

    #[test]
    fn click_on_a_link_returns_its_href() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(r#"<a href="https://example.com/next">click me</a>"#, None);
        // the link is the only content, at the top-left of the page
        assert_eq!(page.click(2.0, 2.0), Some("https://example.com/next".to_string()));
    }

    #[test]
    fn click_on_nested_content_inside_a_link_still_finds_the_href() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(r#"<a href="/x"><b>bold link text</b></a>"#, None);
        assert_eq!(page.click(2.0, 2.0), Some("/x".to_string()));
    }

    #[test]
    fn click_outside_any_link_returns_none() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str("<p>no links here</p>", None);
        assert_eq!(page.click(2.0, 2.0), None);
    }

    #[test]
    fn click_past_the_end_of_the_content_returns_none() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(r#"<a href="/x">hi</a>"#, None);
        assert_eq!(page.click(300.0, 190.0), None);
    }

    #[test]
    fn render_visible_crops_to_the_viewport_size() {
        let mut page = Page::new(50.0, 30.0);
        page.load_html_str("<div style=\"height: 500px; background-color: red;\"></div>", None);
        let visible = page.render_visible();
        assert_eq!(visible.width, 50);
        assert_eq!(visible.height, 30);
    }

    #[test]
    fn render_visible_shows_content_at_the_current_scroll_offset() {
        let mut page = Page::new(20.0, 10.0);
        page.load_html_str(
            "<div style=\"height: 10px; background-color: red;\"></div><div style=\"height: 10px; background-color: blue;\"></div>",
            None,
        );
        let top = page.render_visible();
        assert_eq!(top.get_pixel(0, 0), [255, 0, 0, 255], "scrolled to top, red div is visible");

        page.scroll_by(10.0);
        let scrolled = page.render_visible();
        assert_eq!(scrolled.get_pixel(0, 0), [0, 0, 255, 255], "scrolled down 10px, blue div is now visible");
    }
}
