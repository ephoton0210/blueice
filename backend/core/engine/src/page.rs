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

use crate::assistant_page::{assistant_html, is_assistant_url, AssistantPanel};
use crate::downloads_page::{downloads_html, is_downloads_url, DownloadsSource, DownloadsView};
use crate::gatekeeper_settings_page::{
    gatekeeper_settings_html, is_gatekeeper_settings_url, GatekeeperSettingsSource,
    GatekeeperSettingsView, SettingsNotice,
};
use blueice_css::{cascade, ua_stylesheet, ComputedStyle, Origin, Rule};
use blueice_dom::{Document, NodeData, NodeId};
use blueice_ipc::gatekeeper::GatekeeperSettingsChange;
use blueice_ipc::{AiSnapshot, NodeAction};
use blueice_layout::{layout, Constraints, Fragment};
use blueice_paint::{paint, Color, Frame, PaintCommand, Rect};
use blueice_raster::{rasterize, Pixmap};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use url::Url;

// A child-visible handle must never numerically alias a node in another
// document, even when both DOM allocators assign the same internal NodeId.
static NEXT_SCRIPT_NODE_HANDLE: AtomicU64 = AtomicU64::new(1);

pub struct Page {
    pub(crate) submission_pending: bool,
    pub(crate) post_expired: bool,
    pub(crate) last_navigation: Option<crate::navigation_request::BrowserNavigation>,
    doc: Document,
    styles: HashMap<NodeId, ComputedStyle>,
    fragment: Fragment,
    ua: Vec<Rule>,
    viewport_width: f64,
    viewport_height: f64,
    pub(crate) display_viewport: Option<blueice_ipc::viewport::DisplayViewport>,
    pub(crate) page_zoom: f64,
    pub(crate) display_preferences: Option<blueice_ipc::display::DisplayPreferences>,
    scroll_y: f64,
    url: Option<String>,
    network_response: Option<blueice_ipc::extension::NetworkResponseInfo>,
    network_trace: Option<blueice_ipc::extension::NetworkTraceInfo>,
    /// Monotonically changes whenever this navigable context receives a
    /// replacement document. Page-script realms use it to distinguish a
    /// same-origin navigation from the document that preceded it.
    document_generation: u64,
    /// Only handles minted for this exact document can resolve to its nodes.
    script_nodes: HashMap<u64, NodeId>,
    script_node_handles: HashMap<NodeId, u64>,
    hovered: Option<NodeId>,
    focused: Option<NodeId>,
    native_editor: Option<native_editing::EditorSession>,
    native_focus_generation: u64,
    native_focus_exit: Option<blueice_ipc::input::FocusDirection>,
    native_focus_start: Option<NodeId>,
    /// Core-owned defaults survive live native/extension value changes. This
    /// private state never enters accessibility or script IPC snapshots.
    native_form_defaults: HashMap<NodeId, native_forms::ControlDefault>,
    highlighted: Option<NodeId>,
    find: find::FindSession,
    /// The most recent raster frame for this one tab. Another tab rendering
    /// must not invalidate this tab's frame/representation pairing.
    frame_generation: u64,
    /// Where `about:downloads` reads its list from; `None` (the default)
    /// renders the "service is not running" page.
    downloads: Option<Arc<DownloadsSource>>,
    /// Where `about:settings` reads and updates the actual, private Phase 7
    /// gatekeeper policy. It is optional so isolated parser/render tests do
    /// not need a process, in which case the page says it is unavailable.
    gatekeeper_settings: Option<Arc<GatekeeperSettingsSource>>,
    settings_notice: Option<SettingsNotice>,
    /// Where `about:assistant` reads the shared list of assistant results;
    /// `None` (isolated tests) renders the "not configured" page.
    assistant_panel: Option<Arc<AssistantPanel>>,
    /// Live-translation substitutions for the current document, with the
    /// retained originals (`phase-7-local-ai/PLAN.md`). Empty when the page
    /// was not translated.
    translation: crate::translation::TranslationState,
}

impl Page {
    pub fn new(viewport_width: f64, viewport_height: f64) -> Self {
        Self::new_continuing_from(viewport_width, viewport_height, 0)
    }

    /// Creates a page whose first DOM node starts at `next_node_id`. This is
    /// crate-visible because [`crate::TabManager`] can retain complete pages
    /// when its optional history-snapshot policy is enabled: a replacement
    /// page in one tab must continue after the highest ID in every retained
    /// snapshot, just as [`Self::load_html`] already continues after the page
    /// it replaces. Otherwise a stale node ID from a snapshot in that tab's
    /// Back/Forward cache could be misdirected at an unrelated document.
    pub(crate) fn new_continuing_from(
        viewport_width: f64,
        viewport_height: f64,
        next_node_id: u64,
    ) -> Self {
        Page {
            doc: Document::new_continuing_from(next_node_id),
            styles: HashMap::new(),
            fragment: Fragment::empty_block(),
            ua: ua_stylesheet(),
            viewport_width,
            viewport_height,
            display_viewport: None,
            page_zoom: 1.0,
            display_preferences: None,
            scroll_y: 0.0,
            url: None,
            network_response: None,
            network_trace: None,
            document_generation: 0,
            script_nodes: HashMap::new(),
            script_node_handles: HashMap::new(),
            hovered: None,
            focused: None,
            native_editor: None,
            native_focus_generation: 0,
            native_focus_exit: None,
            native_focus_start: None,
            native_form_defaults: HashMap::new(),
            last_navigation: None,
            post_expired: false,
            submission_pending: false,
            highlighted: None,
            find: find::FindSession::default(),
            frame_generation: 0,
            downloads: None,
            gatekeeper_settings: None,
            settings_notice: None,
            assistant_panel: None,
            translation: crate::translation::TranslationState::default(),
        }
    }

    fn load_html(&mut self, html: &str) {
        self.load_html_translating(html, None);
    }

    /// Parses `html`, substitutes `translations` for its translatable text
    /// (see [`crate::translation`]) when given and well-formed, then styles and
    /// lays out. The substitution sits between parse and cascade so layout
    /// measures the text that is actually shown; a wrong-length answer leaves
    /// the original page (fail-open).
    fn load_html_translating(&mut self, html: &str, translations: Option<&[String]>) {
        // `parse_continuing_from` (not `parse`) so this replacement
        // document's NodeIds never collide with -- or get numerically
        // confused with -- the document it's replacing. A client that
        // caches a NodeId from before this navigation and acts on it
        // afterward must get a safe "doesn't exist" (`Page::act` already
        // checks `doc.contains`), never a silently-misdirected action on
        // an unrelated node that happens to have been assigned the same
        // recycled ID (plan §1's stable-ID-across-mutations requirement).
        self.doc = blueice_html::parse_continuing_from(html, self.doc.next_node_id());
        self.translation = translations
            .and_then(|translations| {
                crate::translation::apply_translation(&mut self.doc, translations)
            })
            .unwrap_or_default();
        self.network_response = None;
        self.network_trace = None;
        self.native_form_defaults.clear();
        self.last_navigation = None;
        self.post_expired = false;
        self.submission_pending = false;
        self.document_generation = self.document_generation.wrapping_add(1);
        self.script_nodes.clear();
        self.script_node_handles.clear();
        self.scroll_y = 0.0;
        // A fresh document invalidates every NodeId a prior interaction
        // might have recorded -- holding onto a stale ID here would let
        // a late-arriving ActOn/Highlight silently act on a node from
        // the *previous* page.
        self.hovered = None;
        self.focused = None;
        self.native_editor = None;
        self.native_focus_exit = None;
        self.native_focus_start = None;
        self.highlighted = None;
        self.find = find::FindSession::default();
        self.restyle_and_relayout();
    }

    /// Re-cascades the current DOM then reflows it. Script mutation must use
    /// this rather than [`Self::relayout`] alone because a node created after
    /// navigation has no entry in the prior `styles` map; layout would leave
    /// script DOM state and the rendered frame out of sync.
    fn restyle_and_relayout(&mut self) {
        self.recascade();
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
        self.adjust_native_editor_scroll();
        let max_scroll = (self.fragment.height - self.viewport_height).max(0.0);
        self.scroll_y = self.scroll_y.min(max_scroll);
        self.rebuild_find();
    }

    fn recascade(&mut self) {
        self.remember_native_form_defaults();
        let author = crate::stylesheet::extract_inline_stylesheets_with_environment(
            &self.doc,
            &self.media_environment(),
        );
        self.styles = cascade(
            &self.doc,
            &[(Origin::Ua, &self.ua), (Origin::Author, &author)],
        );
    }

    /// Test-only direct navigation for exercising the fetch-to-document path.
    ///
    /// Production navigation goes through `session`, which obtains
    /// gatekeeper clearance before fetching a non-built-in URL. Keeping this
    /// helper out of non-test builds means external `Page` users cannot bypass
    /// that boundary by calling a convenient network method directly.
    #[cfg(test)]
    pub(crate) fn navigate(&mut self, url: &str) -> Result<(), blueice_net::FetchError> {
        if self.load_built_in(url) {
            return Ok(());
        }
        let fetched = blueice_net::fetch(url)?;
        self.load_html(&fetched.body);
        self.network_response = Some(blueice_ipc::extension::NetworkResponseInfo {
            method: "GET".to_string(),
            final_url: fetched.final_url.clone(),
            status: fetched.status,
            content_type: fetched.content_type,
        });
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

    /// Where `about:settings` gets and updates the gatekeeper's complete
    /// policy view. Every tab shares one source through [`crate::TabManager`].
    pub fn set_gatekeeper_settings_source(
        &mut self,
        source: Option<Arc<GatekeeperSettingsSource>>,
    ) {
        self.gatekeeper_settings = source;
    }

    /// Where `about:assistant` reads the assistant's results. Every tab of one
    /// `core` shares one panel through [`crate::TabManager`].
    pub fn set_assistant_panel(&mut self, panel: Option<Arc<AssistantPanel>>) {
        self.assistant_panel = panel;
    }

    /// `about:assistant`'s HTML for `url` from the shared panel's current state.
    fn assistant_panel_html(&self, url: &str) -> String {
        let (entries, available, settings_file) = match &self.assistant_panel {
            Some(panel) => (panel.entries(), panel.is_available(), panel.settings_file()),
            None => (Vec::new(), false, None),
        };
        // Read on every render, so an edit to the file shows without a restart
        // (it still takes effect only when the launcher next starts).
        let settings = crate::assistant_page::read_settings_view(settings_file.as_deref());
        assistant_html(
            &entries,
            available,
            &settings,
            settings_file.as_deref(),
            crate::credits::locale_from_url(url),
        )
    }

    /// Re-renders this page from the panel's current state, keeping the scroll
    /// position, when (and only when) it is showing `about:assistant`.
    /// Returns whether it did, so the caller knows a fresh frame is due.
    pub(crate) fn refresh_assistant_panel(&mut self) -> bool {
        let Some(url) = self.url.clone().filter(|url| is_assistant_url(url)) else {
            return false;
        };
        let html = self.assistant_panel_html(&url);
        self.refresh_html(&html);
        true
    }

    pub fn gatekeeper_settings_source(&self) -> Option<&Arc<GatekeeperSettingsSource>> {
        self.gatekeeper_settings.as_ref()
    }

    /// Loads the built-in page for `url` if it is one (`about:blank`,
    /// `about:credits`, `about:downloads`, `about:settings`, `about:assistant`), returning whether it was --
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
        } else if is_assistant_url(url) {
            self.assistant_panel_html(url)
        } else if is_gatekeeper_settings_url(url) {
            let locale = crate::credits::locale_from_url(url);
            let notice = self.settings_notice.take();
            let view = match self
                .gatekeeper_settings
                .as_ref()
                .map(|source| source.fetch())
            {
                Some(Ok(settings)) => GatekeeperSettingsView::Settings(settings),
                _ => GatekeeperSettingsView::Unavailable,
            };
            gatekeeper_settings_html(&view, locale, notice)
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

    /// The gated navigation completion path, per `phase-7-local-ai/PLAN.md`'s
    /// "Wiring design": `session.
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
    ///
    /// Also substitutes the assistant's translation of the page's
    /// translatable text, if any was obtained in time. The clearance covers the
    /// *original* HTML; a translation can only be applied on top of a cleared
    /// page, never instead of one.
    pub(crate) fn apply_fetched_translated(
        &mut self,
        _clearance: crate::gatekeeper_client::GatekeeperClearance,
        url: &str,
        html: &str,
        translations: Option<&[String]>,
    ) {
        self.load_html_translating(html, translations);
        self.url = Some(url.to_string());
    }

    /// Loads `html` with `translations` applied, like [`Self::load_html_str`]
    /// plus a translation. Ungated exactly as `load_html_str` is (tests and
    /// trusted built-in content).
    pub fn load_html_translated(
        &mut self,
        html: &str,
        url: Option<String>,
        translations: &[String],
    ) {
        self.load_html_translating(html, Some(translations));
        self.url = url;
    }

    /// Whether the current document has any substituted text to toggle.
    pub fn has_translation(&self) -> bool {
        self.translation.has_translation()
    }

    /// Whether the translation (rather than the original) is on screen.
    pub fn translation_shown(&self) -> bool {
        self.translation.is_shown()
    }

    /// Shows the translation (`true`) or the retained original (`false`) and
    /// relayouts. Returns whether anything changed, so callers only publish a
    /// new frame when there is one.
    pub fn set_translation_shown(&mut self, shown: bool) -> bool {
        let changed = crate::translation::set_shown(&mut self.doc, &mut self.translation, shown);
        if changed {
            self.restyle_and_relayout();
        }
        changed
    }

    /// The page's shown prose as plain text, at most what the assistant
    /// accepts in one request (`crate::page_text`).
    pub fn visible_text(&self) -> String {
        crate::page_text::visible_text(
            &self.doc,
            &self.styles,
            blueice_ipc::assistant::MAX_REQUEST_TEXT_BYTES,
        )
    }

    /// The original text of a translated text node, for the AI representation.
    pub(crate) fn original_text(&self, node: NodeId) -> Option<&str> {
        self.translation.original_of(&self.doc, node)
    }

    pub(crate) fn set_network_response(
        &mut self,
        response: blueice_ipc::extension::NetworkResponseInfo,
    ) {
        self.network_response = Some(response);
    }

    pub(crate) fn network_response(&self) -> Option<&blueice_ipc::extension::NetworkResponseInfo> {
        self.network_response.as_ref()
    }

    pub(crate) fn set_network_trace(&mut self, trace: blueice_ipc::extension::NetworkTraceInfo) {
        self.network_trace = Some(trace);
    }

    pub(crate) fn network_trace(&self) -> Option<&blueice_ipc::extension::NetworkTraceInfo> {
        self.network_trace.as_ref()
    }

    pub fn resize(&mut self, width: f64, height: f64) {
        if let Some(display) = &mut self.display_viewport {
            display.width = width;
            display.height = height;
        }
        self.viewport_width = width / self.page_zoom;
        self.viewport_height = height / self.page_zoom;
        self.restyle_and_relayout();
    }

    pub(crate) fn configure_display(&mut self, display: blueice_ipc::viewport::DisplayViewport) {
        self.display_viewport = Some(display);
        self.resize(display.width, display.height);
    }

    fn media_environment(&self) -> blueice_css::MediaEnvironment {
        let preferences = self.display_preferences.unwrap_or_default();
        blueice_css::MediaEnvironment {
            print: false,
            width: self.viewport_width,
            height: self.viewport_height,
            resolution: self
                .display_viewport
                .map_or(1.0, |v| v.backing_scale.unwrap_or(v.device_scale))
                * self.page_zoom,
            dark: preferences.dark,
            high_contrast: preferences.high_contrast,
            reduced_motion: preferences.reduced_motion,
        }
    }

    pub(crate) fn set_display_preferences(
        &mut self,
        preferences: blueice_ipc::display::DisplayPreferences,
    ) {
        if self.display_preferences != Some(preferences) {
            self.display_preferences = Some(preferences);
            self.restyle_and_relayout();
        }
    }

    pub(crate) fn display_preferences_state(
        &self,
        source: u64,
        tab_id: u64,
    ) -> blueice_ipc::display::DisplayPreferencesState {
        blueice_ipc::display::DisplayPreferencesState {
            tab_id,
            frame_source: source,
            frame_generation: self.frame_generation,
            preferences: self.display_preferences.unwrap_or_default(),
        }
    }

    pub(crate) fn set_page_zoom(&mut self, zoom: f64) {
        let display = self
            .display_viewport
            .unwrap_or(blueice_ipc::viewport::DisplayViewport {
                width: self.viewport_width,
                height: self.viewport_height,
                device_scale: 1.0,
                backing_scale: None,
            });
        self.page_zoom = zoom;
        self.configure_display(display);
        self.scroll_by(0.0);
    }

    pub(crate) fn viewport_state(
        &self,
        source: u64,
        tab_id: u64,
    ) -> blueice_ipc::viewport::ViewportState {
        let display = self
            .display_viewport
            .unwrap_or(blueice_ipc::viewport::DisplayViewport {
                width: self.viewport_width,
                height: self.viewport_height,
                device_scale: 1.0,
                backing_scale: None,
            });
        let (pixel_width, pixel_height) = display.pixel_size();
        blueice_ipc::viewport::ViewportState {
            tab_id,
            frame_source: source,
            frame_generation: self.frame_generation,
            width: display.width,
            height: display.height,
            device_scale: display.device_scale,
            backing_scale: display.backing_scale,
            zoom: self.page_zoom,
            css_width: self.viewport_width,
            css_height: self.viewport_height,
            pixel_width,
            pixel_height,
        }
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
        let node = self.click_target(x, y)?;
        nearest_link_href(&self.doc, node).map(|href| self.resolve_link_href(href))
    }

    /// Returns the current DOM target for a pointer click. Core obtains this
    /// before it invokes BlueJS listeners, so a listener may mutate/remove the
    /// target while the browser still retains the pre-dispatch default action
    /// (for example, a link navigation) to apply unless it is prevented.
    pub fn click_target(&self, x: f64, y: f64) -> Option<NodeId> {
        let content_y = y + self.scroll_y;
        hit_test(&self.fragment, x, content_y)
    }

    /// Applies the native default focus action for a pointer click. Only an
    /// enabled text input can become the editing target; every other click
    /// clears a previous text-input focus. Keeping this state in the core
    /// means the reference frontend never has to turn keyboard events into a
    /// guessed DOM node ID.
    pub(crate) fn supports_native_text_input(&self, node: NodeId) -> bool {
        self.native_focusable(node) && native_editing::supports_native_input(&self.doc, node)
    }

    #[cfg(test)]
    pub(crate) fn focus_text_input_at(&mut self, target: Option<NodeId>) -> bool {
        let focused = target.and_then(|node| nearest_supported_text_input(&self.doc, node));
        if self.focused == focused {
            return false;
        }
        self.focused = focused;
        true
    }

    /// The live element receiving an owner-originated click. Text fragments
    /// hit-test to text nodes, but the first event profile exposes element
    /// listeners, so route to their nearest element ancestor.
    pub(crate) fn click_event_target(&self, x: f64, y: f64) -> Option<NodeId> {
        let node = hit_test(&self.fragment, x, y + self.scroll_y)?;
        self.event_element_target(node)
    }

    pub(crate) fn event_element_target(&self, mut node: NodeId) -> Option<NodeId> {
        if !self.doc.contains(node) {
            return None;
        }
        loop {
            if matches!(self.doc.data(node), NodeData::Element { .. }) {
                return Some(node);
            }
            node = self.doc.parent(node)?;
        }
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
                if self.focused != Some(id) {
                    self.native_focus_generation = self.native_focus_generation.wrapping_add(1);
                    self.native_editor = None;
                    self.native_focus_exit = None;
                }
                self.focused = Some(id);
                self.native_focus_start = Some(id);
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
                // always relayouts afterward. The input-value fragment and
                // accessibility snapshot both read this core-owned attribute,
                // so this render pass is the synchronization point the human
                // and MCP observers share after a value change.
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

    /// Applies a control activated from the built-in `about:settings` page.
    /// This is intentionally not a generic DOM-to-privileged bridge: only the
    /// fixed data attributes emitted by `gatekeeper_settings_html` are
    /// recognized, their input is revalidated by the gatekeeper process, and
    /// no action can disable a compiled rule or workflow step.
    /// Returns `None` for an ordinary page control, and `Some` when this was
    /// one of the fixed settings controls. A rejected update is still a
    /// handled control: the refreshed page presents a localized rejection
    /// notice, while the caller can preserve its ordinary frame lifecycle.
    pub(crate) fn apply_gatekeeper_settings_control(
        &mut self,
        id: NodeId,
    ) -> Option<Result<(), String>> {
        if !self.url.as_deref().is_some_and(is_gatekeeper_settings_url) {
            return None;
        }
        let change = self.gatekeeper_settings_change_for(id)?;
        let locale = self
            .url
            .as_deref()
            .map(crate::credits::locale_from_url)
            .unwrap_or(blueice_i18n::DEFAULT_LOCALE);
        let Some(source) = self.gatekeeper_settings.as_ref() else {
            self.settings_notice = Some(SettingsNotice::Rejected(
                "the settings service is unavailable".to_string(),
            ));
            let html = gatekeeper_settings_html(
                &GatekeeperSettingsView::Unavailable,
                locale,
                self.settings_notice.take(),
            );
            self.load_html(&html);
            return Some(Err(
                "the gatekeeper settings source is unavailable".to_string()
            ));
        };
        let (view, notice) = match source.update(change) {
            Ok(settings) => (
                GatekeeperSettingsView::Settings(settings),
                SettingsNotice::Saved,
            ),
            Err(error) => {
                let view = source
                    .fetch()
                    .map(GatekeeperSettingsView::Settings)
                    .unwrap_or(GatekeeperSettingsView::Unavailable);
                let html = gatekeeper_settings_html(
                    &view,
                    locale,
                    Some(SettingsNotice::Rejected(error.clone())),
                );
                self.load_html(&html);
                return Some(Err(error));
            }
        };
        let html = gatekeeper_settings_html(&view, locale, Some(notice));
        self.load_html(&html);
        Some(Ok(()))
    }

    fn gatekeeper_settings_change_for(&self, mut node: NodeId) -> Option<GatekeeperSettingsChange> {
        let action = loop {
            if let Some(value) = element_attribute(&self.doc, node, "data-gatekeeper-action") {
                break value.to_string();
            }
            node = self.doc.parent(node)?;
        };
        match action.as_str() {
            "configure-model" => {
                let read_input = |id| {
                    let input = find_element_by_id(&self.doc, self.doc.root(), id)?;
                    element_attribute(&self.doc, input, "value").map(str::to_string)
                };
                Some(GatekeeperSettingsChange::ConfigureLocalModel {
                    provider: read_input("gatekeeper-model-provider")?,
                    base_url: read_input("gatekeeper-model-base")?,
                    model: read_input("gatekeeper-model-name")?,
                })
            }
            "disable-model" => Some(GatekeeperSettingsChange::DisableLocalModel),
            "add-host" => {
                let input =
                    find_element_by_id(&self.doc, self.doc.root(), "gatekeeper-custom-host")?;
                Some(GatekeeperSettingsChange::AddBlockedHost {
                    host: element_attribute(&self.doc, input, "value")?.to_string(),
                })
            }
            "remove-host" => Some(GatekeeperSettingsChange::RemoveBlockedHost {
                host: element_attribute(&self.doc, node, "data-gatekeeper-host")?.to_string(),
            }),
            "add-phrase" => {
                let input =
                    find_element_by_id(&self.doc, self.doc.root(), "gatekeeper-custom-phrase")?;
                Some(GatekeeperSettingsChange::AddBlockedPhrase {
                    phrase: element_attribute(&self.doc, input, "value")?.to_string(),
                })
            }
            "remove-phrase" => Some(GatekeeperSettingsChange::RemoveBlockedPhrase {
                phrase: element_attribute(&self.doc, node, "data-gatekeeper-phrase")?.to_string(),
            }),
            "add-extension" => {
                let input =
                    find_element_by_id(&self.doc, self.doc.root(), "gatekeeper-custom-extension")?;
                Some(GatekeeperSettingsChange::AddBlockedDownloadExtension {
                    extension: element_attribute(&self.doc, input, "value")?.to_string(),
                })
            }
            "remove-extension" => Some(GatekeeperSettingsChange::RemoveBlockedDownloadExtension {
                extension: element_attribute(&self.doc, node, "data-gatekeeper-extension")?
                    .to_string(),
            }),
            "add-popup-phrase" => {
                let input = find_element_by_id(
                    &self.doc,
                    self.doc.root(),
                    "gatekeeper-custom-popup-phrase",
                )?;
                Some(GatekeeperSettingsChange::AddBlockedPopupPhrase {
                    phrase: element_attribute(&self.doc, input, "value")?.to_string(),
                })
            }
            "remove-popup-phrase" => Some(GatekeeperSettingsChange::RemoveBlockedPopupPhrase {
                phrase: element_attribute(&self.doc, node, "data-gatekeeper-popup-phrase")?
                    .to_string(),
            }),
            _ => None,
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

    /// Resolves the first element whose literal `id` attribute matches the
    /// page-script request. This is crate-visible only: script authority must
    /// enter through the core-owned IPC dispatcher rather than letting an
    /// arbitrary caller mutate a page's DOM directly.
    pub(crate) fn script_get_element_by_id(&self, id: &str) -> Option<NodeId> {
        find_element_by_id(&self.doc, self.doc.root(), id)
    }

    /// Mints one stable child-private handle for a node in this document.
    /// Handles are process-unique, but resolve only in the page that minted
    /// them; they are not DOM `NodeId`s or page JavaScript values.
    pub(crate) fn script_handle_for_node(&mut self, node: NodeId) -> u64 {
        assert!(
            self.doc.contains(node),
            "cannot mint a handle for a foreign DOM node"
        );
        if let Some(handle) = self.script_node_handles.get(&node) {
            return *handle;
        }
        let handle = NEXT_SCRIPT_NODE_HANDLE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .expect("script node handle space exhausted");
        self.script_nodes.insert(handle, node);
        self.script_node_handles.insert(node, handle);
        handle
    }

    /// Converts a core hit-test NodeId into the same child-private handle
    /// used by script IPC, if the hit still belongs to this live document.
    #[cfg(unix)]
    pub(crate) fn script_handle_for_raw_node(&mut self, raw: u64) -> Option<u64> {
        let node = NodeId::from_u64(raw);
        self.doc
            .contains(node)
            .then(|| self.script_handle_for_node(node))
    }

    /// Checks whether a child-private node handle still belongs to this
    /// document. Detached nodes remain valid until their subtree is removed.
    pub(crate) fn script_validate_node(&self, node: u64) -> Result<(), String> {
        self.script_node(node).map(|_| ())
    }

    /// Creates a detached element for the page-script IPC surface.
    pub(crate) fn script_create_element(&mut self, tag_name: String) -> Result<NodeId, String> {
        if tag_name.is_empty() {
            return Err("element tag name must not be empty".to_string());
        }
        Ok(self.doc.create_node(NodeData::Element {
            tag_name,
            attributes: Vec::new(),
        }))
    }

    /// Creates a detached text node for the page-script IPC surface.
    pub(crate) fn script_create_text_node(&mut self, data: String) -> NodeId {
        self.doc.create_node(NodeData::Text { data })
    }

    /// Appends a detached child after validating that both raw IPC handles
    /// belong to this document and cannot produce a malformed DOM tree.
    pub(crate) fn script_append_child(&mut self, parent: u64, child: u64) -> Result<(), String> {
        let parent = self.script_node(parent)?;
        let child = self.script_node(child)?;
        if parent == child {
            return Err("a node cannot be appended to itself".to_string());
        }
        if child == self.doc.root() {
            return Err("the document root cannot be appended".to_string());
        }
        if matches!(self.doc.data(parent), NodeData::Text { .. }) {
            return Err("a text node cannot have children".to_string());
        }
        if self.doc.parent(child).is_some() {
            return Err("the child node is already attached".to_string());
        }
        self.doc.append_child(parent, child);
        self.native_form_script_text_change(parent);
        // Newly created elements have no computed style until they join this
        // document. Recompute author/UA styles before layout and paint.
        self.recascade();
        self.relayout();
        Ok(())
    }

    /// Returns the recursive text content for one live page-script node.
    pub(crate) fn script_text_content(&self, node: u64) -> Result<String, String> {
        Ok(node_text_content(&self.doc, self.script_node(node)?))
    }

    /// Returns a snapshot of the current document's recursive text content for
    /// the first read-only BlueJS page binding. The value is copied into the
    /// realm callback at binding installation, so it grants neither a DOM
    /// reference nor a cross-document handle to the VM.
    pub(crate) fn script_document_text_content(&self) -> String {
        node_text_content(&self.doc, self.doc.root())
    }

    /// Implements the narrow page-script `textContent` setter. Existing child
    /// subtrees are removed before a non-empty replacement text node is
    /// attached, matching the DOM operation's observable tree replacement.
    pub(crate) fn script_set_text_content(
        &mut self,
        node: u64,
        value: String,
    ) -> Result<(), String> {
        let node = self.script_node(node)?;
        if let NodeData::Text { data } = self.doc.data_mut(node) {
            *data = value;
        } else {
            let children = self.doc.children(node).collect::<Vec<_>>();
            for child in children {
                self.doc.remove_subtree(child);
            }
            if !value.is_empty() {
                let text = self.doc.create_node(NodeData::Text { data: value });
                self.doc.append_child(node, text);
            }
        }
        self.native_form_script_text_change(node);
        self.relayout();
        Ok(())
    }

    fn script_node(&self, raw: u64) -> Result<NodeId, String> {
        self.script_nodes
            .get(&raw)
            .copied()
            .filter(|node| self.doc.contains(*node))
            .ok_or_else(|| "unknown script node handle".to_string())
    }

    /// Replaces an element's children with one text node (or updates a Text
    /// node in place) for core-owned form edits, then restyles and relayouts.
    fn set_node_text_content(&mut self, node: NodeId, value: String) -> Result<(), String> {
        if !self.doc.contains(node) {
            return Err("node does not belong to this document".to_string());
        }
        if let NodeData::Text { data } = self.doc.data_mut(node) {
            *data = value;
        } else {
            let children = self.doc.children(node).collect::<Vec<_>>();
            for child in children {
                self.doc.remove_subtree(child);
            }
            let text = self.doc.create_node(NodeData::Text { data: value });
            self.doc.append_child(node, text);
        }
        self.restyle_and_relayout();
        Ok(())
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

    /// Makes this page the successor of `previous` within one tab: its frame
    /// generation continues from the departing page's (so a frontend that only
    /// accepts newer frames never drops the first frame of a new document), and
    /// its document generation is strictly greater (so a page-script realm bound
    /// to the departed document can never alias this one).
    pub(crate) fn continue_tab_generations_from(&mut self, previous: &Page) {
        self.frame_generation = previous.frame_generation;
        self.document_generation = previous.document_generation.wrapping_add(1);
        self.display_viewport = previous.display_viewport;
        self.page_zoom = previous.page_zoom;
        let media_changed = self.display_preferences != previous.display_preferences;
        self.display_preferences = previous.display_preferences;
        if self.viewport_width != previous.viewport_width
            || self.viewport_height != previous.viewport_height
            || media_changed
        {
            self.viewport_width = previous.viewport_width;
            self.viewport_height = previous.viewport_height;
            self.restyle_and_relayout();
        }
    }

    /// The generation of this tab's most recently written frame, or zero
    /// before it has rendered one.
    pub(crate) fn frame_generation(&self) -> u64 {
        self.frame_generation
    }

    pub(crate) fn doc(&self) -> &Document {
        &self.doc
    }

    /// The first node ID a subsequent replacement document would have to use
    /// to avoid reusing any ID currently present in this page.
    pub(crate) fn next_node_id(&self) -> u64 {
        self.doc.next_node_id()
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

    /// Returns this document's explicitly opted-in BlueTS script declarations
    /// in source order. The result has no loader, origin, capability, or
    /// execution authority; an external `src` remains only a declaration for
    /// a future authorized page-script loader.
    pub fn blue_ts_script_declarations(&self) -> Vec<crate::script::BlueTsPageScriptDeclaration> {
        crate::script::discover_blue_ts_page_scripts(&self.doc)
    }

    /// Returns supported standard JavaScript declarations in source order.
    /// Like the BlueTS accessor, this is observation only: an external `src`
    /// remains inert until a core-owned host authorizes a closed source graph.
    pub fn blue_js_script_declarations(&self) -> Vec<crate::script::BlueJsPageScriptDeclaration> {
        crate::script::discover_blue_js_page_scripts(&self.doc)
    }

    /// Returns every supported page-script declaration under one document-order
    /// sequence. Only the launcher-supervised shared BlueTS/JavaScript host
    /// consumes this inventory; it remains an observation API with no source
    /// loader, compiler-profile, DOM, or execution authority.
    pub fn combined_page_script_declarations(
        &self,
    ) -> Vec<crate::script::CombinedPageScriptDeclaration> {
        crate::script::discover_combined_page_scripts(&self.doc)
    }

    /// The identity of the currently loaded document within this page.
    ///
    /// This is intentionally crate-visible: it is a core lifecycle token, not
    /// web-observable state. It lets the script host invalidate a realm even
    /// when a navigation keeps the same origin.
    pub(crate) fn document_generation(&self) -> u64 {
        self.document_generation
    }

    /// Native control values retain whitespace. Accessible names use a
    /// different, collapsed text algorithm; protected values never cross it.
    pub(crate) fn native_control_public_value(&self, node: NodeId) -> Option<String> {
        let NodeData::Element { tag_name, .. } = self.doc.data(node) else {
            return None;
        };
        if tag_name == "textarea" {
            return Some(node_text_content(&self.doc, node));
        }
        if tag_name == "select" {
            return self.native_select_value(node);
        }
        if tag_name != "input" {
            return None;
        }
        let kind = element_attribute(&self.doc, node, "type")
            .unwrap_or("text")
            .to_ascii_lowercase();
        if kind == "range" {
            return blueice_layout::input_range_values(&self.doc, node).map(|(_, _, value)| {
                // Preserve a valid in-range source spelling, including exact
                // integers beyond f64 precision. Only defaults/clamps replace it.
                element_attribute(&self.doc, node, "value")
                    .filter(|raw| {
                        raw.parse::<f64>()
                            .ok()
                            .is_some_and(|parsed| parsed.is_finite() && parsed == value)
                    })
                    .map_or_else(|| value.to_string(), str::to_string)
            });
        }
        if matches!(kind.as_str(), "text" | "search" | "email" | "url" | "tel") {
            element_attribute(&self.doc, node, "value").map(str::to_string)
        } else {
            None
        }
    }

    pub fn viewport_size(&self) -> (f64, f64) {
        (self.viewport_width, self.viewport_height)
    }

    pub fn scroll_y(&self) -> f64 {
        self.scroll_y
    }

    pub fn render(&self) -> Frame {
        let mut frame = paint(&self.fragment, &self.styles);
        self.paint_native_editor(&mut frame);
        self.paint_native_focus(&mut frame);
        self.paint_find(&mut frame);
        if let Some(id) = self.highlighted {
            if let Some(bounds) = find_fragment_bounds(&self.fragment, id, 0.0, 0.0) {
                frame.commands.extend(highlight_border_commands(bounds));
            }
        }
        frame
    }

    /// Native display sessions rasterize a bounded viewport directly at their
    /// backing-density/page-zoom transform. Legacy sessions rasterize the full
    /// page and crop, retaining their original one-pixel-per-CSS-pixel output.
    pub fn render_visible(&self) -> Pixmap {
        if let Some(display) = self.display_viewport {
            let (pixel_width, pixel_height) = display.pixel_size();
            return blueice_raster::rasterize_viewport(
                &self.render(),
                self.scroll_y,
                pixel_width,
                pixel_height,
                display.device_scale * self.page_zoom,
            )
            .expect("validated display viewport has bounded physical dimensions");
        }
        let full = rasterize(&self.render());
        crop(
            &full,
            self.scroll_y,
            self.viewport_width,
            self.viewport_height,
        )
    }
}

mod context_menu;
mod dom_helpers;
mod dom_write;
mod find;
mod form_submission;
mod native_editing;
mod native_forms;
mod native_interaction;
pub(crate) mod printing;
use dom_helpers::*;

#[cfg(test)]
mod tests;

pub(crate) use dom_helpers::find_fragment_bounds;
