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

use crate::downloads_page::{DownloadsSource, DownloadsView, downloads_html, is_downloads_url};
use blueice_css::{ComputedStyle, Origin, Rule, cascade, ua_stylesheet};
use blueice_dom::{Document, NodeData, NodeId};
use blueice_ipc::{AiSnapshot, NodeAction};
use blueice_layout::{Constraints, Fragment, layout};
use blueice_paint::{Color, Frame, PaintCommand, Rect, paint};
use blueice_raster::{Pixmap, rasterize};
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;

pub struct Page {
    doc: Document,
    styles: HashMap<NodeId, ComputedStyle>,
    fragment: Fragment,
    ua: Vec<Rule>,
    viewport_width: f64,
    viewport_height: f64,
    scroll_y: f64,
    url: Option<String>,
    hovered: Option<NodeId>,
    focused: Option<NodeId>,
    highlighted: Option<NodeId>,
    /// The most recent raster frame for this one tab. Another tab rendering
    /// must not invalidate this tab's frame/representation pairing.
    frame_generation: u64,
    /// Where `about:downloads` reads its list from; `None` (the default)
    /// renders the "service is not running" page.
    downloads: Option<Arc<DownloadsSource>>,
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
            hovered: None,
            focused: None,
            highlighted: None,
            frame_generation: 0,
            downloads: None,
        }
    }

    fn load_html(&mut self, html: &str) {
        // `parse_continuing_from` (not `parse`) so this replacement
        // document's NodeIds never collide with -- or get numerically
        // confused with -- the document it's replacing. A client that
        // caches a NodeId from before this navigation and acts on it
        // afterward must get a safe "doesn't exist" (`Page::act` already
        // checks `doc.contains`), never a silently-misdirected action on
        // an unrelated node that happens to have been assigned the same
        // recycled ID (plan §1's stable-ID-across-mutations requirement).
        self.doc = blueice_html::parse_continuing_from(html, self.doc.next_node_id());
        let author = crate::stylesheet::extract_inline_stylesheets(&self.doc);
        self.styles = cascade(
            &self.doc,
            &[(Origin::Ua, &self.ua), (Origin::Author, &author)],
        );
        self.scroll_y = 0.0;
        // A fresh document invalidates every NodeId a prior interaction
        // might have recorded -- holding onto a stale ID here would let
        // a late-arriving ActOn/Highlight silently act on a node from
        // the *previous* page.
        self.hovered = None;
        self.focused = None;
        self.highlighted = None;
        self.relayout();
    }

    fn relayout(&mut self) {
        self.fragment = layout(
            &self.doc,
            self.doc.root(),
            &self.styles,
            Constraints {
                available_width: self.viewport_width,
            },
        );
        let max_scroll = (self.fragment.height - self.viewport_height).max(0.0);
        self.scroll_y = self.scroll_y.min(max_scroll);
    }

    /// Fetches `url` over the network and loads it as the current
    /// page -- except for the handful of built-in `about:` pages
    /// ([`built_in_page`]), which never hit the network at all.
    pub fn navigate(&mut self, url: &str) -> Result<(), blueice_net::FetchError> {
        if self.load_built_in(url) {
            return Ok(());
        }
        let fetched = blueice_net::fetch(url)?;
        self.load_html(&fetched.body);
        self.url = Some(fetched.final_url);
        Ok(())
    }

    /// Where `about:downloads` gets its list (see
    /// [`DownloadsSource`]); every tab of one `core` shares one source.
    pub fn set_downloads_source(&mut self, source: Option<Arc<DownloadsSource>>) {
        self.downloads = source;
    }

    pub fn downloads_source(&self) -> Option<&Arc<DownloadsSource>> {
        self.downloads.as_ref()
    }

    /// Loads the built-in page for `url` if it is one (`about:blank`,
    /// `about:credits`, `about:downloads`), returning whether it was --
    /// never touching the network. The downloads page reads the downloads
    /// process over its socket, quickly and with a hard time bound (a
    /// hung process must not stall the session), and falls back to the
    /// "service is not running" page rather than failing the navigation.
    pub(crate) fn load_built_in(&mut self, url: &str) -> bool {
        let html = if let Some(html) = built_in_page(url) {
            html
        } else if is_downloads_url(url) {
            let locale = crate::credits::locale_from_url(url);
            match self.downloads.as_ref().map(|source| source.fetch_quick()) {
                Some(Ok(transfers)) => {
                    downloads_html(&DownloadsView::Transfers(&transfers), locale)
                }
                _ => downloads_html(&DownloadsView::Unavailable, locale),
            }
        } else {
            return false;
        };
        self.load_html(&html);
        self.url = Some(url.to_string());
        true
    }

    /// Replaces the current document with `html` *keeping the scroll
    /// position* (clamped to what still exists) -- for a live-updating
    /// built-in page such as `about:downloads`, where a reader halfway down
    /// a long list must not be thrown back to the top every half second.
    /// The URL is left as it is.
    pub(crate) fn refresh_html(&mut self, html: &str) {
        let scroll = self.scroll_y;
        self.load_html(html);
        let max_scroll = (self.fragment.height - self.viewport_height).max(0.0);
        self.scroll_y = scroll.min(max_scroll);
    }

    /// Loads `html` directly, with no network fetch -- used by tests,
    /// and by `blueice-core` for its initial blank/about page.
    pub fn load_html_str(&mut self, html: &str, url: Option<String>) {
        self.load_html(html);
        self.url = url;
    }

    /// The gated counterpart to [`Page::navigate`]'s network-fetching
    /// half, per `phase-7-local-ai/PLAN.md`'s "Wiring design": `session.
    /// rs`'s background thread does the actual gatekeeper round trips
    /// and the fetch itself (never touching `Page` state, since it
    /// doesn't run on the main thread); once that's all cleared, this
    /// applies the already-fetched `html` -- parse/cascade/layout only,
    /// no network -- exactly like [`Page::load_html_str`] does, plus
    /// recording `url`. Takes `_clearance` purely for its compile-time
    /// effect (see [`crate::gatekeeper_client::GatekeeperClearance`]'s
    /// own docs): there is no public, non-gated way to reach this
    /// method from outside the crate, so skipping the gate for a real
    /// (non-built-in, non-test) navigation is a compile error, not a
    /// runtime convention a differently-written caller could omit.
    pub(crate) fn apply_fetched(
        &mut self,
        _clearance: crate::gatekeeper_client::GatekeeperClearance,
        url: &str,
        html: &str,
    ) {
        self.load_html(html);
        self.url = Some(url.to_string());
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
    /// nearest one). Relative links are resolved against the current
    /// page URL when it is an absolute, hierarchical URL.
    pub fn click(&self, x: f64, y: f64) -> Option<String> {
        let content_y = y + self.scroll_y;
        let node = hit_test(&self.fragment, x, content_y)?;
        nearest_link_href(&self.doc, node).map(|href| self.resolve_link_href(href))
    }

    /// Hit-tests a pointer move the same way [`Page::click`] hit-tests
    /// a click, becoming the single source of truth for "what's
    /// hovered" -- see `phase-1-ai-representation-layer/PLAN.md` §4.
    /// Moving off all content clears the hover state, same as a real
    /// pointer leaving the window's content area.
    pub fn hover_at(&mut self, x: f64, y: f64) {
        let content_y = y + self.scroll_y;
        self.hovered = hit_test(&self.fragment, x, content_y);
    }

    /// Sets or clears (`None`) the highlighted node -- rendered as an
    /// outline derived fresh from that node's current bounds on every
    /// [`Page::render`] call, per the AI-to-human sync direction
    /// `phase-1-ai-representation-layer/PLAN.md` §4 describes.
    pub fn set_highlight(&mut self, id: Option<NodeId>) {
        self.highlighted = id;
    }

    /// Applies `action` to the element addressed by `id` -- see
    /// [`NodeAction`]'s own docs for what each variant does. Returns
    /// the URL to navigate to when `action` is [`NodeAction::Click`]
    /// on a link, same shape as [`Page::click`]'s return value (and the
    /// same relative-link resolution), so a caller drives both the same
    /// way. A stale or unknown `id` (e.g. from before the last
    /// navigation) is silently a no-op, not an error -- the same
    /// tolerance [`Page::click`] already has for a point that hits
    /// nothing.
    pub fn act(&mut self, id: NodeId, action: NodeAction) -> Option<String> {
        if !self.doc.contains(id) {
            return None;
        }
        match action {
            NodeAction::Click => {
                nearest_link_href(&self.doc, id).map(|href| self.resolve_link_href(href))
            }
            NodeAction::Focus => {
                self.focused = Some(id);
                None
            }
            NodeAction::SetValue(value) => {
                if let NodeData::Element { attributes, .. } = self.doc.data_mut(id) {
                    match attributes.iter_mut().find(|(k, _)| k == "value") {
                        Some((_, existing)) => *existing = value,
                        None => attributes.push(("value".to_string(), value)),
                    }
                }
                // `load_html` (the only other path that mutates `doc`)
                // always relayouts afterward; this in-place mutation was
                // missed. Inert today since nothing in blueice-layout/
                // blueice-paint reads an element's `value` attribute
                // yet, but the render pass and AI snapshot would
                // otherwise silently go stale relative to `dom_dump()`
                // (which reads `doc` live) the moment layout starts
                // rendering input values.
                self.relayout();
                None
            }
            NodeAction::ScrollIntoView => {
                if let Some(bounds) = find_fragment_bounds(&self.fragment, id, 0.0, 0.0) {
                    let max_scroll = (self.fragment.height - self.viewport_height).max(0.0);
                    self.scroll_y = bounds.y.clamp(0.0, max_scroll);
                }
                None
            }
        }
    }

    /// Resolves an anchor's raw `href` using the current document URL.
    /// Test-only pages and built-in pages may have no usable hierarchical
    /// base; in that case preserve the raw target, so the session's normal
    /// scheme validation reports an invalid target rather than silently
    /// inventing a destination.
    fn resolve_link_href(&self, href: String) -> String {
        self.url
            .as_deref()
            .and_then(|base| Url::parse(base).ok())
            .and_then(|base| base.join(&href).ok())
            .map(|url| url.to_string())
            .unwrap_or(href)
    }

    /// A snapshot of the AI-facing representation
    /// (`phase-1-ai-representation-layer/PLAN.md`'s schema,
    /// implemented per `phase-5-ai-representation-output/PLAN.md`),
    /// extracted from this exact `Page` state -- `generation` is
    /// supplied by the caller (`session.rs`'s own frame-generation
    /// counter) so a snapshot and the `FrameReady` sent alongside it
    /// can share the same number, which is what makes "same render
    /// pass" a checkable property rather than an assertion.
    pub fn snapshot(&self, generation: u64, tab_id: u64) -> AiSnapshot {
        crate::ai_snapshot::build(self, generation, tab_id)
    }

    /// The full DOM tree, in `blueice_dom::dump`'s canonical text
    /// format -- unlike [`Page::snapshot`], nothing is filtered out
    /// (no semantic-role requirement, no `display:none` exclusion),
    /// since a structural comparison against a real browser's DOM
    /// (the Chromium differential-testing harness, `TEST_PLAN.md`)
    /// needs the whole tree, not the AI-facing subset of it.
    pub fn dom_dump(&self) -> String {
        blueice_dom::dump(&self.doc)
    }

    /// Advances this tab's render-pass generation. `session` calls this
    /// immediately before writing the corresponding frame.
    pub(crate) fn advance_frame_generation(&mut self) -> u64 {
        self.frame_generation = self
            .frame_generation
            .checked_add(1)
            .expect("a page cannot render more than u64::MAX frames");
        self.frame_generation
    }

    /// The generation of this tab's most recently written frame, or zero
    /// before it has rendered one.
    pub(crate) fn frame_generation(&self) -> u64 {
        self.frame_generation
    }

    pub(crate) fn doc(&self) -> &Document {
        &self.doc
    }

    pub(crate) fn fragment(&self) -> &Fragment {
        &self.fragment
    }

    pub(crate) fn styles(&self) -> &HashMap<NodeId, ComputedStyle> {
        &self.styles
    }

    pub(crate) fn hovered(&self) -> Option<NodeId> {
        self.hovered
    }

    pub(crate) fn focused(&self) -> Option<NodeId> {
        self.focused
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
        let mut frame = paint(&self.fragment, &self.styles);
        if let Some(id) = self.highlighted {
            if let Some(bounds) = find_fragment_bounds(&self.fragment, id, 0.0, 0.0) {
                frame.commands.extend(highlight_border_commands(bounds));
            }
        }
        frame
    }

    /// Rasterizes the full page, then crops to the current viewport at
    /// the current scroll offset -- see module docs for why this is
    /// how "scrolling" works without real overflow-clipped layout.
    pub fn render_visible(&self) -> Pixmap {
        let full = rasterize(&self.render());
        crop(
            &full,
            self.scroll_y,
            self.viewport_width,
            self.viewport_height,
        )
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
    Pixmap {
        width: w,
        height: h,
        pixels,
    }
}

/// Finds `id`'s own fragment and resolves its bounds to document-
/// content coordinates (accumulating each ancestor's offset the same
/// way `blueice-paint`'s own tree walk does -- a `Fragment`'s `x`/`y`
/// are relative to its parent, not absolute) -- `None` if `id` has no
/// fragment at all, which is exactly right for a `display:none`
/// subtree (dropped entirely by layout, per `research/layout.md`) or a
/// stale ID from before the last navigation. Shared by
/// [`Page::render`]'s highlight overlay, [`Page::act`]'s
/// `ScrollIntoView`, and `ai_snapshot`'s bounds extraction, so the
/// three never disagree about where a node actually is.
pub(crate) fn find_fragment_bounds(
    fragment: &Fragment,
    id: NodeId,
    offset_x: f64,
    offset_y: f64,
) -> Option<blueice_ipc::Bounds> {
    let x = offset_x + fragment.x;
    let y = offset_y + fragment.y;
    if fragment.node == Some(id) {
        return Some(blueice_ipc::Bounds {
            x,
            y,
            width: fragment.width,
            height: fragment.height,
        });
    }
    fragment
        .children
        .iter()
        .find_map(|child| find_fragment_bounds(child, id, x, y))
}

const HIGHLIGHT_COLOR: Color = Color::Rgba(255, 149, 0, 255);
const HIGHLIGHT_THICKNESS: f64 = 2.0;

/// A four-edge outline around `bounds`, built from ordinary
/// `PaintCommand::BorderEdge`s rather than a new paint-command variant
/// -- an AI-requested highlight is an interaction-layer concept
/// `Page` owns, not a CSS box-model feature `blueice-paint` needs to
/// know about.
fn highlight_border_commands(bounds: blueice_ipc::Bounds) -> Vec<PaintCommand> {
    let blueice_ipc::Bounds {
        x,
        y,
        width,
        height,
    } = bounds;
    let t = HIGHLIGHT_THICKNESS;
    vec![
        PaintCommand::BorderEdge {
            rect: Rect {
                x,
                y,
                width,
                height: t,
            },
            color: HIGHLIGHT_COLOR,
        },
        PaintCommand::BorderEdge {
            rect: Rect {
                x: x + width - t,
                y,
                width: t,
                height,
            },
            color: HIGHLIGHT_COLOR,
        },
        PaintCommand::BorderEdge {
            rect: Rect {
                x,
                y: y + height - t,
                width,
                height: t,
            },
            color: HIGHLIGHT_COLOR,
        },
        PaintCommand::BorderEdge {
            rect: Rect {
                x,
                y,
                width: t,
                height,
            },
            color: HIGHLIGHT_COLOR,
        },
    ]
}

fn hit_test(fragment: &Fragment, x: f64, y: f64) -> Option<NodeId> {
    hit_test_rec(fragment, x, y, 0.0, 0.0)
}

fn hit_test_rec(
    fragment: &Fragment,
    x: f64,
    y: f64,
    offset_x: f64,
    offset_y: f64,
) -> Option<NodeId> {
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
        if let NodeData::Element {
            tag_name,
            attributes,
        } = doc.data(node)
        {
            if tag_name == "a" {
                if let Some((_, href)) = attributes.iter().find(|(k, _)| k == "href") {
                    return Some(href.clone());
                }
            }
        }
        node = doc.parent(node)?;
    }
}

/// The HTML for `url`, for the small set of `about:` URLs `navigate`
/// serves locally instead of fetching over the network -- `None` for
/// any other URL (including unrecognized `about:` ones, which aren't
/// treated as built-in pages here). Owned `String`, not `&'static
/// str`: the credits page is generated per request from `blueice-i18n`
/// at whatever locale the URL's `?lang=` parameter asks for
/// (`credits::locale_from_url`), not a single fixed literal.
pub(crate) fn built_in_page(url: &str) -> Option<String> {
    if url == "about:blank" {
        return Some(String::new());
    }
    if url == crate::credits::CREDITS_URL
        || url.starts_with(&format!("{}?", crate::credits::CREDITS_URL))
    {
        return Some(crate::credits::credits_html(
            crate::credits::locale_from_url(url),
        ));
    }
    None
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
        assert!(
            page.render()
                .commands
                .iter()
                .any(|c| matches!(c, PaintCommand::Text { text, .. } if text == "hi"))
        );
    }

    #[test]
    fn navigate_to_about_credits_loads_the_built_in_credits_page_without_network() {
        let mut page = Page::new(320.0, 200.0);
        page.navigate("about:credits").unwrap();
        assert_eq!(page.url(), Some("about:credits"));
        let text = all_text(&page.render());
        assert!(
            text.contains("Chromium"),
            "must reproduce the Chromium BSD-3-Clause notice: {text}"
        );
        assert!(text.contains("Gecko"), "must credit Gecko: {text}");
        assert!(
            text.contains("DejaVu"),
            "must credit the bundled DejaVu font: {text}"
        );
    }

    #[test]
    fn navigate_to_about_credits_with_a_lang_parameter_loads_the_localized_credits_page() {
        let mut page = Page::new(320.0, 200.0);
        page.navigate("about:credits?lang=zh-TW").unwrap();
        assert_eq!(page.url(), Some("about:credits?lang=zh-TW"));
        let text = all_text(&page.render());
        assert!(
            text.contains("關於"),
            "must render the localized page: {text}"
        );
    }

    #[test]
    fn navigating_never_reuses_a_nodeid_from_the_previous_document() {
        // Regression: `NodeIdAllocator` used to live on `Document`
        // itself, restarting at 0 every time `load_html` built a fresh
        // `Document` -- so a client that cached a NodeId before this
        // navigation and acted on it afterward could get silently
        // redirected to whatever unrelated node the recycled ID now
        // happened to belong to, instead of a safe "doesn't exist"
        // (plan §1's stable-ID-across-mutations requirement).
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str("<p>first</p>", None);
        let stale_id = page.doc().root();
        let first_next_id = page.doc().next_node_id();

        page.load_html_str("<p>second</p>", None);

        assert!(
            !page.doc().contains(stale_id),
            "a NodeId real in the previous document must not resolve to anything in the new one"
        );
        assert!(
            page.doc().next_node_id() >= first_next_id,
            "the new document's allocator must continue from where the old one left off, not restart at 0"
        );
    }

    fn find_by_tag(doc: &Document, root: NodeId, tag: &str) -> Option<NodeId> {
        if let NodeData::Element { tag_name, .. } = doc.data(root) {
            if tag_name == tag {
                return Some(root);
            }
        }
        doc.children(root).find_map(|c| find_by_tag(doc, c, tag))
    }

    #[test]
    fn act_set_value_updates_the_value_attribute() {
        // No existing test exercised `NodeAction::SetValue` at all. Its
        // handler was also found to skip the `relayout()` call every
        // other `doc`-mutating path makes (harmless today since nothing
        // in blueice-layout/blueice-paint reads an input's value yet, so
        // there's no *observable* effect to assert on here beyond "it
        // doesn't panic" -- but the call is now in place for whenever
        // layout does start reading it).
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(r#"<input type="text">"#, None);
        let input_id = find_by_tag(page.doc(), page.doc().root(), "input").unwrap();

        page.act(input_id, NodeAction::SetValue("hello".to_string()));

        let NodeData::Element { attributes, .. } = page.doc().data(input_id) else {
            panic!("expected an element")
        };
        assert!(attributes.contains(&("value".to_string(), "hello".to_string())));
    }

    #[test]
    fn navigate_to_about_blank_loads_an_empty_page_without_network() {
        let mut page = Page::new(320.0, 200.0);
        page.navigate("about:blank").unwrap();
        assert_eq!(page.url(), Some("about:blank"));
        assert!(page.render().commands.is_empty());
    }

    #[test]
    fn dom_dump_matches_blueice_doms_own_dump_of_the_same_document() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(r#"<div class="x"><p>hi</p></div>"#, None);
        assert_eq!(page.dom_dump(), blueice_dom::dump(page.doc()));
        assert!(page.dom_dump().contains("<div>"));
    }

    #[test]
    fn dom_dump_includes_nodes_the_ai_snapshot_would_exclude() {
        // a bare <div> has no semantic role, so Page::snapshot excludes
        // it entirely -- dom_dump must not apply that filter.
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(r#"<div style="background-color: red;">x</div>"#, None);
        assert!(page.dom_dump().contains("<div>"));
        assert!(page.snapshot(0, 1).nodes.is_empty());
    }

    fn all_text(frame: &Frame) -> String {
        frame
            .commands
            .iter()
            .filter_map(|c| match c {
                PaintCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
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
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        });
        let mut page = Page::new(320.0, 200.0);
        page.navigate(&format!("http://{addr}")).unwrap();
        assert!(
            page.render()
                .commands
                .iter()
                .any(|c| matches!(c, PaintCommand::Text { text, .. } if text == "fetched"))
        );
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
        assert!(
            narrow_lines > wide_lines,
            "a much narrower viewport must wrap onto more lines"
        );
    }

    #[test]
    fn scroll_clamps_to_the_content_range() {
        let mut page = Page::new(100.0, 20.0);
        page.load_html_str("<div style=\"height: 500px;\"></div>", None);
        page.scroll_by(-100.0);
        assert_eq!(page.scroll_y(), 0.0, "cannot scroll above the top");
        page.scroll_by(10_000.0);
        assert!(
            page.scroll_y() > 0.0 && page.scroll_y() <= 500.0,
            "cannot scroll past the bottom of the content"
        );
    }

    #[test]
    fn click_on_a_link_returns_its_href() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(r#"<a href="https://example.com/next">click me</a>"#, None);
        // the link is the only content, at the top-left of the page
        assert_eq!(
            page.click(2.0, 2.0),
            Some("https://example.com/next".to_string())
        );
    }

    #[test]
    fn click_on_nested_content_inside_a_link_still_finds_the_href() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(r#"<a href="/x"><b>bold link text</b></a>"#, None);
        assert_eq!(page.click(2.0, 2.0), Some("/x".to_string()));
    }

    #[test]
    fn clicking_a_relative_link_resolves_it_against_the_current_page_url() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(
            r#"<a href="../next?q=blue#section">next</a>"#,
            Some("https://example.com/guide/start/index.html?old=query".to_string()),
        );

        assert_eq!(
            page.click(2.0, 2.0),
            Some("https://example.com/guide/next?q=blue#section".to_string())
        );
    }

    #[test]
    fn acting_on_a_relative_link_uses_the_same_resolution_as_a_pointer_click() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(
            r#"<a href="/downloads/file.zip">file</a>"#,
            Some("https://example.com/guide/start".to_string()),
        );
        let link_id = page.snapshot(0, 1).nodes[0].id;

        assert_eq!(
            page.act(NodeId::from_u64(link_id), NodeAction::Click),
            Some("https://example.com/downloads/file.zip".to_string())
        );
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
        page.load_html_str(
            "<div style=\"height: 500px; background-color: red;\"></div>",
            None,
        );
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
        assert_eq!(
            top.get_pixel(0, 0),
            [255, 0, 0, 255],
            "scrolled to top, red div is visible"
        );

        page.scroll_by(10.0);
        let scrolled = page.render_visible();
        assert_eq!(
            scrolled.get_pixel(0, 0),
            [0, 0, 255, 255],
            "scrolled down 10px, blue div is now visible"
        );
    }

    // ---- about:downloads ------------------------------------------------

    use crate::downloads_page::DownloadsSource;
    use crate::downloads_page::test_support::{Scratch, fake_downloads};
    use blueice_ipc::downloads::{TransferInfo, TransferState};
    use std::sync::Arc;

    fn transfer(id: u64, name: &str, state: TransferState) -> TransferInfo {
        TransferInfo {
            id,
            url: format!("https://example.com/{name}"),
            dest_path: format!("/d/{name}"),
            state,
            total_bytes: Some(1000),
            completed_bytes: 400,
            ..TransferInfo::default()
        }
    }

    fn page_reading(socket: std::path::PathBuf) -> Page {
        let mut page = Page::new(400.0, 300.0);
        page.set_downloads_source(Some(Arc::new(DownloadsSource::without_spawner(socket))));
        page
    }

    #[test]
    fn navigating_to_about_downloads_renders_the_live_list_without_a_network_fetch() {
        let dir = Scratch::new("page-live");
        let _server = fake_downloads(
            &dir.socket(),
            vec![
                transfer(1, "alpha.iso", TransferState::Active),
                transfer(2, "beta.zip", TransferState::Completed),
            ],
            false,
            blueice_ipc::downloads::DOWNLOADS_PROTOCOL_VERSION,
        );
        let mut page = page_reading(dir.socket());

        page.navigate("about:downloads")
            .expect("a built-in page never fails to navigate");
        assert_eq!(page.url(), Some("about:downloads"));
        let dump = page.dom_dump();
        assert!(
            dump.contains("alpha.iso") && dump.contains("beta.zip"),
            "{dump}"
        );
        assert!(
            dump.contains("Downloading") && dump.contains("Completed"),
            "{dump}"
        );
    }

    #[test]
    fn about_downloads_says_the_service_is_not_running_when_there_is_no_source_or_it_is_unreachable()
     {
        let mut without = Page::new(400.0, 300.0);
        without.navigate("about:downloads").unwrap();
        assert!(
            without
                .dom_dump()
                .contains("The downloads service is not running"),
            "{}",
            without.dom_dump()
        );

        let dir = Scratch::new("page-dead");
        let mut unreachable = page_reading(dir.socket()); // nothing listens there
        unreachable.navigate("about:downloads").unwrap();
        assert!(
            unreachable
                .dom_dump()
                .contains("The downloads service is not running")
        );
        assert!(
            !unreachable.dom_dump().contains("No downloads yet."),
            "unreachable is not the same as empty"
        );
    }

    #[test]
    fn about_downloads_honors_a_lang_parameter() {
        let dir = Scratch::new("page-lang");
        let _server = fake_downloads(
            &dir.socket(),
            vec![transfer(1, "alpha.iso", TransferState::Active)],
            false,
            blueice_ipc::downloads::DOWNLOADS_PROTOCOL_VERSION,
        );
        let mut page = page_reading(dir.socket());
        page.navigate("about:downloads?lang=zh-TW").unwrap();
        assert_eq!(page.url(), Some("about:downloads?lang=zh-TW"));
        assert!(page.dom_dump().contains("下載中"), "{}", page.dom_dump());
    }

    #[test]
    fn refreshing_keeps_the_scroll_position_a_navigation_would_reset() {
        let long: String = (0..60).map(|i| format!("<p>line {i}</p>")).collect();
        let mut page = Page::new(400.0, 100.0);
        page.load_html_str(&long, Some("about:downloads".to_string()));
        page.scroll_by(200.0);
        assert_eq!(page.scroll_y(), 200.0);

        page.refresh_html(&long);
        assert_eq!(
            page.scroll_y(),
            200.0,
            "a live refresh must not throw a reader back to the top"
        );
        page.load_html_str(&long, None);
        assert_eq!(
            page.scroll_y(),
            0.0,
            "whereas a real navigation does start at the top"
        );
    }

    #[test]
    fn refreshing_to_shorter_content_clamps_the_scroll_to_what_still_exists() {
        let long: String = (0..60).map(|i| format!("<p>line {i}</p>")).collect();
        let mut page = Page::new(400.0, 100.0);
        page.load_html_str(&long, Some("about:downloads".to_string()));
        page.scroll_by(500.0);
        let before = page.scroll_y();
        assert!(before > 100.0);
        page.refresh_html("<p>just one line</p>");
        assert_eq!(page.scroll_y(), 0.0, "nothing left to scroll to");
    }
}
