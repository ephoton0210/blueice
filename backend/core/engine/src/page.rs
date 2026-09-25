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
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use url::Url;

/// The mutation requested through an element's `classList` proxy. Keeping it
/// typed at the core boundary avoids sending JavaScript method spelling into
/// the DOM owner as an unvalidated free-form operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptClassListOperation {
    Add,
    Remove,
    Toggle,
    Contains,
}

pub struct Page {
    doc: Document,
    styles: HashMap<NodeId, ComputedStyle>,
    fragment: Fragment,
    ua: Vec<Rule>,
    viewport_width: f64,
    viewport_height: f64,
    scroll_y: f64,
    url: Option<String>,
    network_response: Option<blueice_ipc::extension::NetworkResponseInfo>,
    network_trace: Option<blueice_ipc::extension::NetworkTraceInfo>,
    hovered: Option<NodeId>,
    focused: Option<NodeId>,
    highlighted: Option<NodeId>,
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
            scroll_y: 0.0,
            url: None,
            network_response: None,
            network_trace: None,
            hovered: None,
            focused: None,
            highlighted: None,
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
        self.scroll_y = 0.0;
        // A fresh document invalidates every NodeId a prior interaction
        // might have recorded -- holding onto a stale ID here would let
        // a late-arriving ActOn/Highlight silently act on a node from
        // the *previous* page.
        self.hovered = None;
        self.focused = None;
        self.highlighted = None;
        self.restyle_and_relayout();
    }

    /// Re-cascades the current DOM then reflows it. Script mutation must use
    /// this rather than [`Self::relayout`] alone because a node created after
    /// navigation has no entry in the prior `styles` map; layout would leave
    /// script DOM state and the rendered frame out of sync.
    fn restyle_and_relayout(&mut self) {
        let author = crate::stylesheet::extract_inline_stylesheets(&self.doc);
        self.styles = cascade(
            &self.doc,
            &[(Origin::Ua, &self.ua), (Origin::Author, &author)],
        );
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
        self.viewport_width = width;
        self.viewport_height = height;
        self.restyle_and_relayout();
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
    pub(crate) fn focus_text_input_at(&mut self, target: Option<NodeId>) -> bool {
        let focused = target.and_then(|node| nearest_supported_text_input(&self.doc, node));
        if self.focused == focused {
            return false;
        }
        self.focused = focused;
        true
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
            self.settings_notice = Some(SettingsNotice::Rejected("the settings service is unavailable".to_string()));
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
                let html = gatekeeper_settings_html(&view, locale, Some(SettingsNotice::Rejected(error.clone())));
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
                let input = find_element_by_id(&self.doc, self.doc.root(), "gatekeeper-custom-extension")?;
                Some(GatekeeperSettingsChange::AddBlockedDownloadExtension {
                    extension: element_attribute(&self.doc, input, "value")?.to_string(),
                })
            }
            "remove-extension" => Some(GatekeeperSettingsChange::RemoveBlockedDownloadExtension {
                extension: element_attribute(&self.doc, node, "data-gatekeeper-extension")?.to_string(),
            }),
            "add-popup-phrase" => {
                let input = find_element_by_id(&self.doc, self.doc.root(), "gatekeeper-custom-popup-phrase")?;
                Some(GatekeeperSettingsChange::AddBlockedPopupPhrase {
                    phrase: element_attribute(&self.doc, input, "value")?.to_string(),
                })
            }
            "remove-popup-phrase" => Some(GatekeeperSettingsChange::RemoveBlockedPopupPhrase {
                phrase: element_attribute(&self.doc, node, "data-gatekeeper-popup-phrase")?.to_string(),
            }),
            _ => None,
        }
    }

    /// Sets the value of a real, supported native text input for the
    /// extension protocol's versioned `dom:write` operation. Unlike the
    /// broader first-party [`NodeAction::SetValue`] compatibility action,
    /// this external boundary verifies that the target is a live `<input>`
    /// with no `type` or `type=text`; an extension cannot use an AI node ID to
    /// smuggle a value attribute onto arbitrary document content or a
    /// sensitive input type.
    pub(crate) fn set_text_input_value(&mut self, id: NodeId, value: String) -> Result<(), String> {
        if !self.doc.contains(id) {
            return Err(format!("unknown text input node {}", id.as_u64()));
        }
        if !is_supported_text_input(&self.doc, id) {
            return Err(format!(
                "node {} is not a supported text input",
                id.as_u64()
            ));
        }
        let NodeData::Element { attributes, .. } = self.doc.data_mut(id) else {
            unreachable!("a checked input node remains an element");
        };
        match attributes
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case("value"))
        {
            Some((_, existing)) => *existing = value,
            None => attributes.push(("value".to_string(), value)),
        }
        self.relayout();
        Ok(())
    }

    /// Appends user-entered text to the focused supported input. The input
    /// target is selected solely by [`Self::focus_text_input_at`], rather than
    /// supplied by a frontend or extension, so this remains a keyboard path
    /// rather than a general DOM mutation capability.
    pub(crate) fn insert_focused_text(&mut self, text: &str) -> bool {
        let Some(id) = self
            .focused
            .filter(|id| is_supported_text_input(&self.doc, *id))
        else {
            return false;
        };
        if text.is_empty() {
            return false;
        }
        let mut value = element_attribute(&self.doc, id, "value")
            .unwrap_or_default()
            .to_string();
        value.push_str(text);
        self.set_text_input_value(id, value)
            .expect("focused supported text input remains writable");
        true
    }

    /// Removes one Unicode scalar from the focused supported input.
    pub(crate) fn delete_focused_text_backward(&mut self) -> bool {
        let Some(id) = self
            .focused
            .filter(|id| is_supported_text_input(&self.doc, *id))
        else {
            return false;
        };
        let mut value = element_attribute(&self.doc, id, "value")
            .unwrap_or_default()
            .to_string();
        if value.pop().is_none() {
            return false;
        }
        self.set_text_input_value(id, value)
            .expect("focused supported text input remains writable");
        true
    }

    /// Sets the text content of a real, enabled native textarea for the
    /// extension protocol's version-4 `dom:write` operation. This is a
    /// distinct bounded operation from text-input attributes: a textarea's
    /// value is represented by its child text, and extensions cannot use it
    /// to replace arbitrary element content.
    pub(crate) fn set_textarea_value(&mut self, id: NodeId, value: String) -> Result<(), String> {
        if !self.doc.contains(id) {
            return Err(format!("unknown textarea node {}", id.as_u64()));
        }
        let is_supported_textarea = matches!(
            self.doc.data(id),
            NodeData::Element {
                tag_name,
                attributes,
            } if tag_name.eq_ignore_ascii_case("textarea")
                && !attributes
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case("disabled"))
        );
        if !is_supported_textarea {
            return Err(format!(
                "node {} is not an enabled native textarea",
                id.as_u64()
            ));
        }
        self.script_set_text_content(id, value)
    }

    /// Narrow v8 extension mutation: only text inside an ordinary rendered
    /// semantic leaf. Keeping the existing text node avoids removing links,
    /// controls, event targets, or arbitrary subtrees through textContent.
    pub(crate) fn set_visible_leaf_text(&mut self, id: NodeId, value: String) -> Result<(), String> {
        self.validate_visible_text_write(id, &value)?;
        let children = self.doc.children(id).collect::<Vec<_>>();
        let [text_id] = children.as_slice() else {
            return Err("the target must have exactly one text child".to_string());
        };
        if !matches!(self.doc.data(*text_id), NodeData::Text { .. }) {
            return Err("the target contains nested content".to_string());
        }
        if let NodeData::Text { data } = self.doc.data_mut(*text_id) {
            *data = value;
        }
        self.restyle_and_relayout();
        Ok(())
    }

    /// Version 9 deliberately expands textContent-style edits to ordinary
    /// inline formatting inside a rendered heading, paragraph, or list item.
    /// Validation finishes before any descendant is removed. A link, form
    /// control, semantic child, element with an ID or inline event handler,
    /// or hidden/interactive annotation cannot be deleted through this API.
    pub(crate) fn set_visible_text_content(
        &mut self,
        id: NodeId,
        value: String,
    ) -> Result<(), String> {
        self.validate_visible_text_write(id, &value)?;
        let mut pending = self.doc.children(id).collect::<Vec<_>>();
        let mut inspected = 0;
        while let Some(node) = pending.pop() {
            inspected += 1;
            if inspected > 128 {
                return Err("visible text content has too many descendants".to_string());
            }
            match self.doc.data(node) {
                NodeData::Text { .. } => {}
                NodeData::Element {
                    tag_name,
                    attributes,
                } if matches!(
                    tag_name.as_str(),
                    "span" | "strong" | "em" | "b" | "i" | "small" | "code" | "mark" | "u" | "s" | "br"
                ) && !attributes.iter().any(|(name, _)| {
                    let name = name.to_ascii_lowercase();
                    matches!(
                        name.as_str(),
                        "id" | "role" | "tabindex" | "contenteditable" | "hidden" | "aria-hidden"
                    ) || name.starts_with("on")
                }) && !self.styles.get(&node).is_some_and(|style| {
                    style.display.eq_ignore_ascii_case("none") || style.opacity() <= 0.0
                }) => pending.extend(self.doc.children(node)),
                _ => {
                    return Err(
                        "visible text content may replace only ordinary inline formatting"
                            .to_string(),
                    )
                }
            }
        }
        self.script_set_text_content(id, value)
    }

    fn validate_visible_text_write(&self, id: NodeId, value: &str) -> Result<(), String> {
        if value.len() > blueice_ipc::extension::MAX_VISIBLE_LEAF_TEXT_BYTES {
            return Err("visible text exceeds the protocol limit".to_string());
        }
        if value.trim().is_empty() {
            return Err("visible text cannot be empty".to_string());
        }
        if value.chars().any(|ch| ch.is_control() && ch != '\n' && ch != '\t') {
            return Err("visible text contains a control character".to_string());
        }
        if !self
            .url
            .as_deref()
            .is_some_and(|url| url.starts_with("http://") || url.starts_with("https://"))
        {
            return Err("extension text writes require an ordinary web page".to_string());
        }
        if !self.doc.contains(id) {
            return Err(format!("unknown visible text node {}", id.as_u64()));
        }
        let NodeData::Element { tag_name, .. } = self.doc.data(id) else {
            return Err("the target must be a semantic element".to_string());
        };
        if !matches!(
            tag_name.as_str(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "li"
        ) {
            return Err("the target is not a supported semantic text leaf".to_string());
        }
        if element_attribute(&self.doc, id, "aria-label").is_some()
            || element_attribute(&self.doc, id, "title").is_some()
        {
            return Err("the target has a separate accessible name".to_string());
        }
        let mut ancestor = Some(id);
        while let Some(node) = ancestor {
            if element_attribute(&self.doc, node, "hidden").is_some()
                || element_attribute(&self.doc, node, "aria-hidden")
                    .is_some_and(|value| value.trim().eq_ignore_ascii_case("true"))
                || self
                    .styles
                    .get(&node)
                    .is_some_and(|style| style.opacity() <= 0.0)
            {
                return Err("the target is hidden".to_string());
            }
            ancestor = self.doc.parent(node);
        }
        if !find_fragment_bounds(&self.fragment, id, 0.0, 0.0)
            .is_some_and(|bounds| bounds.width > 0.0 && bounds.height > 0.0)
        {
            return Err("the target is not rendered".to_string());
        }
        Ok(())
    }

    /// Sets an integer value on one real, enabled native range input for the
    /// extension protocol's version-7 `dom:write` operation. This is not a
    /// generic numeric attribute setter: core owns the live `min`, `max`, and
    /// `step` checks before it changes the one `value` attribute. The first
    /// increment deliberately accepts only integer range constraints (or the
    /// native 0..=100/step-1 defaults); decimal and `step=any` controls need a
    /// later, separately specified numeric representation.
    pub(crate) fn set_range_input_value(&mut self, id: NodeId, value: i64) -> Result<(), String> {
        if !self.doc.contains(id) {
            return Err(format!("unknown range input node {}", id.as_u64()));
        }
        let (min, max, step) = match self.doc.data(id) {
            NodeData::Element {
                tag_name,
                attributes,
            } if tag_name.eq_ignore_ascii_case("input")
                && attributes.iter().any(|(name, input_type)| {
                    name.eq_ignore_ascii_case("type") && input_type.eq_ignore_ascii_case("range")
                })
                && !attributes
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case("disabled")) =>
            {
                let integer_attribute = |name: &str, default: i64| {
                    attributes
                        .iter()
                        .find(|(attribute, _)| attribute.eq_ignore_ascii_case(name))
                        .map_or(Ok(default), |(_, raw)| raw.parse::<i64>().map_err(|_| ()))
                };
                let min = integer_attribute("min", 0).map_err(|_| {
                    format!("range input node {} has a non-integer min", id.as_u64())
                })?;
                let max = integer_attribute("max", 100).map_err(|_| {
                    format!("range input node {} has a non-integer max", id.as_u64())
                })?;
                let step = integer_attribute("step", 1).map_err(|_| {
                    format!("range input node {} has a non-integer step", id.as_u64())
                })?;
                (min, max, step)
            }
            _ => {
                return Err(format!(
                    "node {} is not an enabled integer range input",
                    id.as_u64()
                ));
            }
        };
        if min > max {
            return Err(format!(
                "range input node {} has min above max",
                id.as_u64()
            ));
        }
        if step <= 0 {
            return Err(format!(
                "range input node {} has a non-positive step",
                id.as_u64()
            ));
        }
        // Do the offset arithmetic in i128: a valid i64 range can span
        // across zero, making `value - min` overflow even though both values
        // are individually valid protocol integers.
        let step_aligned = (i128::from(value) - i128::from(min)) % i128::from(step) == 0;
        if !(min..=max).contains(&value) || !step_aligned {
            return Err(format!(
                "value {value} is outside the integer range constraints for node {}",
                id.as_u64()
            ));
        }
        let NodeData::Element { attributes, .. } = self.doc.data_mut(id) else {
            unreachable!("a checked range input remains an element");
        };
        match attributes
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case("value"))
        {
            Some((_, existing)) => *existing = value.to_string(),
            None => attributes.push(("value".to_string(), value.to_string())),
        }
        self.relayout();
        Ok(())
    }

    /// Sets the checked state of a real, enabled native checkbox for the
    /// extension protocol's version-3 `dom:write` operation. This remains a
    /// bounded semantic operation rather than a generic attribute setter:
    /// radios have group semantics that need their own protocol design, and a
    /// disabled control must remain unavailable to an extension write.
    pub(crate) fn set_checkbox_checked(&mut self, id: NodeId, checked: bool) -> Result<(), String> {
        if !self.doc.contains(id) {
            return Err(format!("unknown checkbox node {}", id.as_u64()));
        }
        let is_supported_checkbox = matches!(
            self.doc.data(id),
            NodeData::Element {
                tag_name,
                attributes,
            } if tag_name.eq_ignore_ascii_case("input")
                && attributes.iter().any(|(name, value)| {
                    name.eq_ignore_ascii_case("type") && value.eq_ignore_ascii_case("checkbox")
                })
                && !attributes.iter().any(|(name, _)| name.eq_ignore_ascii_case("disabled"))
        );
        if !is_supported_checkbox {
            return Err(format!(
                "node {} is not an enabled native checkbox",
                id.as_u64()
            ));
        }
        let NodeData::Element { attributes, .. } = self.doc.data_mut(id) else {
            unreachable!("a checked checkbox node remains an element");
        };
        if checked {
            if !attributes
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("checked"))
            {
                attributes.push(("checked".to_string(), String::new()));
            }
        } else {
            attributes.retain(|(name, _)| !name.eq_ignore_ascii_case("checked"));
        }
        self.relayout();
        Ok(())
    }

    /// Selects one real, enabled native radio input for the extension
    /// protocol's version-5 `dom:write` operation. A radio is deliberately
    /// not treated as a checkbox: core derives its local group from the live
    /// document and clears the group's other members atomically. To avoid
    /// silently approximating the HTML form-owner algorithm, this first
    /// operation accepts only named radios with an ordinary ancestor form (or
    /// no form) and rejects externally form-associated controls.
    pub(crate) fn set_radio_checked(&mut self, id: NodeId) -> Result<(), String> {
        let (name, form_owner) = extension_radio_group_scope(&self.doc, id)?;
        let mut members = Vec::new();
        collect_extension_radio_group_members(
            &self.doc,
            self.doc.root(),
            &name,
            form_owner,
            &mut members,
        );
        if !members.contains(&id) {
            return Err("the live radio group could not be resolved".to_string());
        }

        for member in members {
            let NodeData::Element { attributes, .. } = self.doc.data_mut(member) else {
                unreachable!("a collected radio group member remains an element");
            };
            if member == id {
                if !attributes
                    .iter()
                    .any(|(attribute, _)| attribute.eq_ignore_ascii_case("checked"))
                {
                    attributes.push(("checked".to_string(), String::new()));
                }
            } else {
                attributes.retain(|(attribute, _)| !attribute.eq_ignore_ascii_case("checked"));
            }
        }
        self.relayout();
        Ok(())
    }

    /// Selects one real, enabled `<option>` for the extension protocol's
    /// version-6 `dom:write` operation. Like radio selection, the extension
    /// carries only a stable node ID: core derives the owning live `<select>`
    /// and performs the whole single-select transition atomically. The first
    /// slice intentionally does not approximate multiple-select or disabled
    /// option-group semantics, so those controls are rejected rather than
    /// partially changed.
    pub(crate) fn select_option(&mut self, id: NodeId) -> Result<(), String> {
        let select = extension_select_option_owner(&self.doc, id)?;
        let mut options = Vec::new();
        collect_extension_select_options(&self.doc, select, &mut options);
        if !options.contains(&id) {
            return Err("the live select options could not be resolved".to_string());
        }

        for option in options {
            let NodeData::Element { attributes, .. } = self.doc.data_mut(option) else {
                unreachable!("a collected select option remains an element");
            };
            if option == id {
                if !attributes
                    .iter()
                    .any(|(attribute, _)| attribute.eq_ignore_ascii_case("selected"))
                {
                    attributes.push(("selected".to_string(), String::new()));
                }
            } else {
                attributes.retain(|(attribute, _)| !attribute.eq_ignore_ascii_case("selected"));
            }
        }
        self.relayout();
        Ok(())
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

    /// The script-host-facing `document.getElementById` operation.  It lives
    /// here rather than exposing `Document` to BlueJS so only `core` owns
    /// mutable DOM state across the process boundary.
    pub fn script_get_element_by_id(&self, id: &str) -> Option<NodeId> {
        find_element_by_id(&self.doc, self.doc.root(), id)
    }

    /// Runs the existing CSS selector matcher against the live DOM rather
    /// than growing a second, subtly divergent selector implementation for
    /// JavaScript. Unsupported/invalid selectors are reported to the script
    /// host as a normal runtime error.
    pub fn script_query_selector(&self, selector: &str) -> Result<Option<NodeId>, String> {
        Ok(self.script_query_selector_all(selector)?.into_iter().next())
    }

    pub fn script_query_selector_all(&self, selector: &str) -> Result<Vec<NodeId>, String> {
        let stylesheet = blueice_css::parse(&format!("{selector} {{ color: inherit; }}"));
        let selectors = stylesheet
            .rules
            .first()
            .map(|rule| &rule.selectors)
            .filter(|selectors| !selectors.is_empty())
            .ok_or_else(|| format!("unsupported selector: {selector}"))?;
        let mut nodes = Vec::new();
        collect_matching_nodes(&self.doc, self.doc.root(), selectors, &mut nodes);
        Ok(nodes)
    }

    /// Inline classic-script source in document order. The HTML parser keeps
    /// `<script>` text opaque (as it must); Phase 13's script scheduler uses
    /// this extraction point to hand that already-parsed text to BlueJS rather
    /// than re-parsing HTML or scanning raw response bytes itself.
    pub fn inline_script_sources(&self) -> Vec<String> {
        let mut sources = Vec::new();
        collect_inline_script_sources(&self.doc, self.doc.root(), &mut sources);
        sources
    }

    /// Allocates a detached element for the script host. The caller must use
    /// [`Self::script_append_child`] to attach it, exactly matching DOM's
    /// `document.createElement`/`appendChild` split.
    pub fn script_create_element(&mut self, tag_name: String) -> NodeId {
        self.doc.create_node(NodeData::Element {
            tag_name,
            attributes: Vec::new(),
        })
    }

    pub fn script_create_text_node(&mut self, data: String) -> NodeId {
        self.doc.create_node(NodeData::Text { data })
    }

    /// Appends two IPC-supplied node handles only after validating that they
    /// belong to this document and the child is detached. This turns the DOM
    /// crate's internal panic precondition into a structured script error.
    pub fn script_append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), String> {
        if !self.doc.contains(parent) || !self.doc.contains(child) {
            return Err("script node does not belong to this document".to_string());
        }
        if self.doc.parent(child).is_some() {
            return Err("script appendChild requires a detached child".to_string());
        }
        self.doc.append_child(parent, child);
        self.restyle_and_relayout();
        Ok(())
    }

    pub fn script_insert_before(
        &mut self,
        parent: NodeId,
        child: NodeId,
        reference: Option<NodeId>,
    ) -> Result<(), String> {
        if !self.doc.contains(parent) || !self.doc.contains(child) {
            return Err("script node does not belong to this document".to_string());
        }
        if self.doc.parent(child).is_some() {
            return Err("script insertBefore requires a detached child".to_string());
        }
        if let Some(reference) = reference {
            if !self.doc.contains(reference) || self.doc.parent(reference) != Some(parent) {
                return Err("script insertBefore reference is not a child of parent".to_string());
            }
        }
        self.doc.insert_before(parent, child, reference);
        self.restyle_and_relayout();
        Ok(())
    }

    pub fn script_remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), String> {
        if !self.doc.contains(parent) || !self.doc.contains(child) {
            return Err("script node does not belong to this document".to_string());
        }
        if self.doc.parent(child) != Some(parent) {
            return Err("script removeChild requires a child of parent".to_string());
        }
        self.doc.detach(child);
        self.restyle_and_relayout();
        Ok(())
    }

    pub fn script_remove(&mut self, node: NodeId) -> Result<(), String> {
        if !self.doc.contains(node) {
            return Err("script node does not belong to this document".to_string());
        }
        if self.doc.parent(node).is_none() {
            return Err("script remove requires an attached node".to_string());
        }
        self.doc.detach(node);
        self.restyle_and_relayout();
        Ok(())
    }

    pub fn script_text_content(&self, node: NodeId) -> Result<String, String> {
        if !self.doc.contains(node) {
            return Err("script node does not belong to this document".to_string());
        }
        Ok(node_text_content(&self.doc, node))
    }

    /// Replaces an element's children with one text node (or updates a Text
    /// node in place), then recascades/layouts before the next render pass.
    pub fn script_set_text_content(&mut self, node: NodeId, value: String) -> Result<(), String> {
        if !self.doc.contains(node) {
            return Err("script node does not belong to this document".to_string());
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

    pub fn script_get_attribute(&self, node: NodeId, name: &str) -> Result<Option<String>, String> {
        let attributes = self.script_element_attributes(node)?;
        Ok(attributes
            .iter()
            .find(|(attribute, _)| attribute.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone()))
    }

    pub fn script_set_attribute(
        &mut self,
        node: NodeId,
        name: String,
        value: String,
    ) -> Result<(), String> {
        let attributes = self.script_element_attributes_mut(node)?;
        match attributes
            .iter_mut()
            .find(|(attribute, _)| attribute.eq_ignore_ascii_case(&name))
        {
            Some((_, existing)) => *existing = value,
            None => attributes.push((name, value)),
        }
        self.restyle_and_relayout();
        Ok(())
    }

    pub fn script_remove_attribute(&mut self, node: NodeId, name: &str) -> Result<(), String> {
        let attributes = self.script_element_attributes_mut(node)?;
        attributes.retain(|(attribute, _)| !attribute.eq_ignore_ascii_case(name));
        self.restyle_and_relayout();
        Ok(())
    }

    pub fn script_inner_html(&self, node: NodeId) -> Result<String, String> {
        if !self.doc.contains(node) {
            return Err("script node does not belong to this document".to_string());
        }
        let mut html = String::new();
        for child in self.doc.children(node) {
            serialize_html_node(&self.doc, child, &mut html);
        }
        Ok(html)
    }

    pub fn script_get_style_property(
        &self,
        node: NodeId,
        property: &str,
    ) -> Result<String, String> {
        let style = self
            .script_get_attribute(node, "style")?
            .unwrap_or_default();
        let mut declarations = parse_inline_style(&style);
        Ok(declarations
            .remove(&css_property_name(property))
            .unwrap_or_default())
    }

    pub fn script_set_style_property(
        &mut self,
        node: NodeId,
        property: String,
        value: String,
    ) -> Result<(), String> {
        let mut style = parse_inline_style(
            &self
                .script_get_attribute(node, "style")?
                .unwrap_or_default(),
        );
        let property = css_property_name(&property);
        if value.is_empty() {
            style.remove(&property);
        } else {
            style.insert(property, value);
        }
        let serialized = style
            .into_iter()
            .map(|(property, value)| format!("{property}: {value}"))
            .collect::<Vec<_>>()
            .join("; ");
        if serialized.is_empty() {
            self.script_remove_attribute(node, "style")
        } else {
            self.script_set_attribute(node, "style".to_string(), serialized)
        }
    }

    pub fn script_class_list(
        &mut self,
        node: NodeId,
        operation: ScriptClassListOperation,
        class_name: String,
    ) -> Result<bool, String> {
        let mut classes = self
            .script_get_attribute(node, "class")?
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let present = classes.iter().any(|class| class == &class_name);
        let result = match operation {
            ScriptClassListOperation::Contains => present,
            ScriptClassListOperation::Add => {
                if !present && !class_name.is_empty() {
                    classes.push(class_name);
                }
                true
            }
            ScriptClassListOperation::Remove => {
                classes.retain(|class| class != &class_name);
                false
            }
            ScriptClassListOperation::Toggle => {
                if present {
                    classes.retain(|class| class != &class_name);
                    false
                } else if class_name.is_empty() {
                    false
                } else {
                    classes.push(class_name);
                    true
                }
            }
        };
        if !matches!(operation, ScriptClassListOperation::Contains) {
            if classes.is_empty() {
                self.script_remove_attribute(node, "class")?;
            } else {
                self.script_set_attribute(node, "class".to_string(), classes.join(" "))?;
            }
        }
        Ok(result)
    }

    fn script_element_attributes(&self, node: NodeId) -> Result<&[(String, String)], String> {
        if !self.doc.contains(node) {
            return Err("script node does not belong to this document".to_string());
        }
        match self.doc.data(node) {
            NodeData::Element { attributes, .. } => Ok(attributes),
            _ => Err("script operation requires an element node".to_string()),
        }
    }

    fn script_element_attributes_mut(
        &mut self,
        node: NodeId,
    ) -> Result<&mut Vec<(String, String)>, String> {
        if !self.doc.contains(node) {
            return Err("script node does not belong to this document".to_string());
        }
        match self.doc.data_mut(node) {
            NodeData::Element { attributes, .. } => Ok(attributes),
            _ => Err("script operation requires an element node".to_string()),
        }
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

fn find_element_by_id(doc: &Document, node: NodeId, id: &str) -> Option<NodeId> {
    if let NodeData::Element { attributes, .. } = doc.data(node) {
        if attributes
            .iter()
            .any(|(name, value)| name == "id" && value == id)
        {
            return Some(node);
        }
    }
    doc.children(node)
        .find_map(|child| find_element_by_id(doc, child, id))
}

fn element_attribute<'a>(doc: &'a Document, node: NodeId, name: &str) -> Option<&'a str> {
    let NodeData::Element { attributes, .. } = doc.data(node) else {
        return None;
    };
    attributes
        .iter()
        .find(|(attribute, _)| attribute.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Returns the group identity for one extension-selectable radio. This is
/// intentionally stricter than arbitrary script DOM mutation: the extension
/// provides only a stable node ID, so all group membership comes from the
/// core-owned current document.
fn extension_radio_group_scope(
    doc: &Document,
    id: NodeId,
) -> Result<(String, Option<NodeId>), String> {
    if !doc.contains(id) {
        return Err(format!("unknown radio node {}", id.as_u64()));
    }
    let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(id)
    else {
        return Err(format!(
            "node {} is not an enabled named native radio",
            id.as_u64()
        ));
    };
    let is_radio = tag_name.eq_ignore_ascii_case("input")
        && attributes.iter().any(|(attribute, value)| {
            attribute.eq_ignore_ascii_case("type") && value.eq_ignore_ascii_case("radio")
        });
    let disabled = attributes
        .iter()
        .any(|(attribute, _)| attribute.eq_ignore_ascii_case("disabled"));
    let externally_associated = attributes
        .iter()
        .any(|(attribute, _)| attribute.eq_ignore_ascii_case("form"));
    let name = attributes
        .iter()
        .find(|(attribute, _)| attribute.eq_ignore_ascii_case("name"))
        .map(|(_, value)| value.clone())
        .filter(|value| !value.is_empty());
    if !is_radio || disabled || externally_associated || name.is_none() {
        return Err(format!(
            "node {} is not an enabled named native radio",
            id.as_u64()
        ));
    }
    Ok((
        name.expect("a checked radio name is present"),
        nearest_form_ancestor(doc, id),
    ))
}

fn nearest_form_ancestor(doc: &Document, id: NodeId) -> Option<NodeId> {
    let mut ancestor = doc.parent(id);
    while let Some(node) = ancestor {
        if matches!(doc.data(node), NodeData::Element { tag_name, .. } if tag_name.eq_ignore_ascii_case("form"))
        {
            return Some(node);
        }
        ancestor = doc.parent(node);
    }
    None
}

fn collect_extension_radio_group_members(
    doc: &Document,
    node: NodeId,
    name: &str,
    form_owner: Option<NodeId>,
    members: &mut Vec<NodeId>,
) {
    if let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(node)
    {
        let is_member = tag_name.eq_ignore_ascii_case("input")
            && attributes.iter().any(|(attribute, value)| {
                attribute.eq_ignore_ascii_case("type") && value.eq_ignore_ascii_case("radio")
            })
            && !attributes
                .iter()
                .any(|(attribute, _)| attribute.eq_ignore_ascii_case("form"))
            && attributes
                .iter()
                .any(|(attribute, value)| attribute.eq_ignore_ascii_case("name") && value == name)
            && nearest_form_ancestor(doc, node) == form_owner;
        if is_member {
            members.push(node);
        }
    }
    for child in doc.children(node) {
        collect_extension_radio_group_members(doc, child, name, form_owner, members);
    }
}

/// Resolves the single-select that owns an extension-selectable option. This
/// intentionally models only the safe subset whose semantics core can own
/// exactly: a live, enabled native select without `multiple`, and a live,
/// enabled option not contained by a disabled optgroup. Form association does
/// not alter selection membership, so it is intentionally not part of this
/// local DOM transition.
fn extension_select_option_owner(doc: &Document, id: NodeId) -> Result<NodeId, String> {
    if !doc.contains(id) {
        return Err(format!("unknown option node {}", id.as_u64()));
    }
    let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(id)
    else {
        return Err(format!(
            "node {} is not an enabled single-select option",
            id.as_u64()
        ));
    };
    let is_enabled_option = tag_name.eq_ignore_ascii_case("option")
        && !attributes
            .iter()
            .any(|(attribute, _)| attribute.eq_ignore_ascii_case("disabled"));
    let select = nearest_select_ancestor(doc, id);
    let select_is_enabled_single = select.is_some_and(|select| {
        matches!(
            doc.data(select),
            NodeData::Element {
                tag_name,
                attributes,
            } if tag_name.eq_ignore_ascii_case("select")
                && !attributes.iter().any(|(attribute, _)| {
                    attribute.eq_ignore_ascii_case("disabled")
                        || attribute.eq_ignore_ascii_case("multiple")
                })
        )
    });
    if !is_enabled_option || !select_is_enabled_single || has_disabled_optgroup_ancestor(doc, id) {
        return Err(format!(
            "node {} is not an enabled single-select option",
            id.as_u64()
        ));
    }
    Ok(select.expect("a checked enabled single select is present"))
}

fn nearest_select_ancestor(doc: &Document, id: NodeId) -> Option<NodeId> {
    let mut ancestor = doc.parent(id);
    while let Some(node) = ancestor {
        if matches!(doc.data(node), NodeData::Element { tag_name, .. } if tag_name.eq_ignore_ascii_case("select"))
        {
            return Some(node);
        }
        ancestor = doc.parent(node);
    }
    None
}

fn has_disabled_optgroup_ancestor(doc: &Document, id: NodeId) -> bool {
    let mut ancestor = doc.parent(id);
    while let Some(node) = ancestor {
        if let NodeData::Element {
            tag_name,
            attributes,
        } = doc.data(node)
        {
            if tag_name.eq_ignore_ascii_case("optgroup")
                && attributes
                    .iter()
                    .any(|(attribute, _)| attribute.eq_ignore_ascii_case("disabled"))
            {
                return true;
            }
        }
        ancestor = doc.parent(node);
    }
    false
}

fn collect_extension_select_options(doc: &Document, node: NodeId, options: &mut Vec<NodeId>) {
    if matches!(doc.data(node), NodeData::Element { tag_name, .. } if tag_name.eq_ignore_ascii_case("option"))
    {
        options.push(node);
    }
    for child in doc.children(node) {
        collect_extension_select_options(doc, child, options);
    }
}

fn node_text_content(doc: &Document, node: NodeId) -> String {
    match doc.data(node) {
        NodeData::Text { data } => data.clone(),
        NodeData::Document | NodeData::Element { .. } => doc
            .children(node)
            .map(|child| node_text_content(doc, child))
            .collect(),
    }
}

fn collect_inline_script_sources(doc: &Document, node: NodeId, sources: &mut Vec<String>) {
    if let NodeData::Element { tag_name, .. } = doc.data(node) {
        if tag_name.eq_ignore_ascii_case("script") {
            sources.push(node_text_content(doc, node));
            return;
        }
    }
    for child in doc.children(node) {
        collect_inline_script_sources(doc, child, sources);
    }
}

fn collect_matching_nodes(
    doc: &Document,
    node: NodeId,
    selectors: &[blueice_css::ComplexSelector],
    nodes: &mut Vec<NodeId>,
) {
    if selectors
        .iter()
        .any(|selector| blueice_css::matches(doc, node, selector))
    {
        nodes.push(node);
    }
    for child in doc.children(node) {
        collect_matching_nodes(doc, child, selectors, nodes);
    }
}

fn serialize_html_node(doc: &Document, node: NodeId, html: &mut String) {
    match doc.data(node) {
        NodeData::Document => {
            for child in doc.children(node) {
                serialize_html_node(doc, child, html);
            }
        }
        NodeData::Text { data } => html.push_str(&escape_html_text(data)),
        NodeData::Element {
            tag_name,
            attributes,
        } => {
            html.push('<');
            html.push_str(tag_name);
            for (name, value) in attributes {
                html.push(' ');
                html.push_str(name);
                html.push_str("=\"");
                html.push_str(&escape_html_attribute(value));
                html.push('"');
            }
            html.push('>');
            for child in doc.children(node) {
                serialize_html_node(doc, child, html);
            }
            html.push_str("</");
            html.push_str(tag_name);
            html.push('>');
        }
    }
}

fn escape_html_text(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;")
}

fn escape_html_attribute(value: &str) -> String {
    escape_html_text(value).replace('"', "&quot;")
}

fn parse_inline_style(style: &str) -> BTreeMap<String, String> {
    style
        .split(';')
        .filter_map(|declaration| declaration.split_once(':'))
        .map(|(property, value)| (css_property_name(property.trim()), value.trim().to_string()))
        .filter(|(property, _)| !property.is_empty())
        .collect()
}

fn css_property_name(property: &str) -> String {
    let mut name = String::with_capacity(property.len() + 4);
    for character in property.trim().chars() {
        if character.is_ascii_uppercase() {
            name.push('-');
            name.push(character.to_ascii_lowercase());
        } else {
            name.push(character.to_ascii_lowercase());
        }
    }
    name
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

fn nearest_supported_text_input(doc: &Document, mut node: NodeId) -> Option<NodeId> {
    loop {
        if is_supported_text_input(doc, node) {
            return Some(node);
        }
        node = doc.parent(node)?;
    }
}

fn is_supported_text_input(doc: &Document, id: NodeId) -> bool {
    matches!(
        doc.data(id),
        NodeData::Element {
            tag_name,
            attributes,
        } if tag_name.eq_ignore_ascii_case("input")
            && !attributes
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("disabled"))
            && attributes
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("type"))
                .is_none_or(|(_, input_type)| input_type.eq_ignore_ascii_case("text"))
    )
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
    fn about_assistant_reads_the_shared_panel_and_refreshes_in_place() {
        use crate::assistant_page::{AssistantPanel, PanelKind};
        let panel = Arc::new(AssistantPanel::new());
        panel.set_available(true);
        let mut page = Page::new(320.0, 400.0);
        page.set_assistant_panel(Some(panel.clone()));
        assert!(page.load_built_in("about:assistant"));
        assert_eq!(page.url(), Some("about:assistant"));
        assert!(all_text(&page.render()).contains("Nothing here yet"));

        panel.push(PanelKind::Summary, None, None, Ok("a fresh summary".into()));
        assert!(page.refresh_assistant_panel());
        assert!(all_text(&page.render()).contains("a fresh summary"));
        assert_eq!(page.url(), Some("about:assistant"));
    }

    #[test]
    fn only_a_page_showing_about_assistant_refreshes() {
        let mut page = Page::new(320.0, 400.0);
        page.load_html_str("<p>ordinary</p>", Some("https://example.com/".into()));
        assert!(!page.refresh_assistant_panel());
        assert_eq!(shown_text(&page), ["ordinary"]);
        let mut blank = Page::new(320.0, 400.0);
        assert!(!blank.refresh_assistant_panel());
    }

    #[test]
    fn about_assistant_without_a_panel_says_it_is_not_configured() {
        let mut page = Page::new(320.0, 400.0);
        assert!(page.load_built_in("about:assistant"));
        assert!(all_text(&page.render()).contains("No local assistant is configured"));
    }

    #[test]
    fn about_assistant_honours_the_lang_parameter() {
        let mut page = Page::new(320.0, 400.0);
        assert!(page.load_built_in("about:assistant?lang=zh-TW"));
        assert!(all_text(&page.render()).contains("助理"));
    }

    #[test]
    fn new_page_is_blank_with_no_url() {
        let page = Page::new(320.0, 200.0);
        assert_eq!(page.url(), None);
        assert!(page.render().commands.is_empty());
    }

    fn shown_text(page: &Page) -> Vec<String> {
        page.render()
            .commands
            .iter()
            .filter_map(|c| match c {
                PaintCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_translated_load_paints_the_translation_and_keeps_the_original() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_translated(
            "<p>Hello</p><p>World</p>",
            Some("https://a.example/".into()),
            &["你好".into(), "世界".into()],
        );
        assert_eq!(page.url(), Some("https://a.example/"));
        assert_eq!(shown_text(&page), ["你好", "世界"]);
        assert!(page.has_translation());
        assert!(page.translation_shown());
        let node = crate::translation::translatable_texts(page.doc())[0].node;
        assert_eq!(page.original_text(node), Some("Hello"));
    }

    #[test]
    fn translation_lands_before_layout_so_a_longer_text_reflows() {
        let mut short = Page::new(100.0, 200.0);
        short.load_html_str("<p>hi</p>", None);
        let mut long = Page::new(100.0, 200.0);
        let translation = "supercalifragilistic expialidocious wonderful ".repeat(4);
        long.load_html_translated("<p>hi</p>", None, &[translation]);
        assert!(
            shown_text(&long).len() > shown_text(&short).len(),
            "the long translation must wrap onto more lines: laid out from the translated text, not the original"
        );
    }

    #[test]
    fn toggling_shows_the_original_and_back_and_relayouts() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_translated("<p>Hello</p>", None, &["你好".into()]);
        assert!(page.set_translation_shown(false));
        assert_eq!(shown_text(&page), ["Hello"]);
        assert!(!page.translation_shown());
        assert!(
            !page.set_translation_shown(false),
            "no change, no new frame"
        );
        assert!(page.set_translation_shown(true));
        assert_eq!(shown_text(&page), ["你好"]);
    }

    #[test]
    fn a_wrong_length_translation_leaves_the_original_page() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_translated("<p>a</p><p>b</p>", None, &["only one".into()]);
        assert_eq!(shown_text(&page), ["a", "b"]);
        assert!(!page.has_translation());
        assert!(!page.set_translation_shown(false));
    }

    #[test]
    fn a_new_document_discards_the_previous_translation() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_translated("<p>Hello</p>", None, &["你好".into()]);
        page.load_html_str("<p>Fresh</p>", None);
        assert!(!page.has_translation());
        assert_eq!(shown_text(&page), ["Fresh"]);
    }

    #[test]
    fn an_untranslated_page_has_no_originals() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str("<p>Hello</p>", None);
        let node = crate::translation::translatable_texts(page.doc())[0].node;
        assert_eq!(page.original_text(node), None);
        assert!(!page.translation_shown());
    }

    #[test]
    fn load_html_str_sets_url_and_renders_content() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str("<p>hi</p>", Some("about:blank".to_string()));
        assert_eq!(page.url(), Some("about:blank"));
        assert!(page
            .render()
            .commands
            .iter()
            .any(|c| matches!(c, PaintCommand::Text { text, .. } if text == "hi")));
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

    fn find_by_attribute(doc: &Document, root: NodeId, name: &str, value: &str) -> Option<NodeId> {
        if element_attribute(doc, root, name) == Some(value) {
            return Some(root);
        }
        doc.children(root)
            .find_map(|child| find_by_attribute(doc, child, name, value))
    }

    #[test]
    fn about_settings_shows_and_applies_the_running_gatekeepers_additive_policy() {
        use blueice_ai_gatekeeper::GatekeeperService;
        use std::os::unix::net::UnixListener;
        use std::sync::Arc;
        use std::thread;

        let socket = blueice_ipc::local_socket::default_socket_dir()
            .join(format!("gks-{}", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let service = Arc::new(GatekeeperService::new(None).unwrap());
        let worker = thread::spawn({
            let service = service.clone();
            move || {
                // Ten full settings-page renders can exceed ten seconds in a
                // cold, single-test run even though every socket exchange is
                // bounded separately by the production 300 ms client timeout.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                let mut handled = 0;
                while handled < 10 && std::time::Instant::now() < deadline {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            // The listener is nonblocking only so this worker can
                            // honor its deadline. On macOS an accepted stream may
                            // inherit that mode and race the client's first write.
                            stream.set_nonblocking(false).unwrap();
                            service.handle_connection(&mut stream).unwrap();
                            handled += 1;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(error) => panic!("settings test listener failed: {error}"),
                    }
                }
                assert_eq!(
                    handled, 10,
                    "the page must read and apply every control through the real service"
                );
            }
        });

        let mut page = Page::new(640.0, 480.0);
        page.set_gatekeeper_settings_source(Some(Arc::new(
            GatekeeperSettingsSource::without_default(&socket),
        )));
        page.navigate("about:settings?lang=en").unwrap();
        assert!(page.dom_dump().contains("known-malicious-domain"));
        assert!(page.dom_dump().contains("extension-visible-text-social-engineering"));
        assert!(page.dom_dump().contains("Exact host or dot-boundary subdomain match"));
        assert!(page.dom_dump().contains("If rejected or unavailable"));
        assert!(page.dom_dump().contains("Active review order"));
        assert!(page.dom_dump().contains("Compiled deterministic rule base (required)"));
        assert!(page.dom_dump().contains("Compiled rules at this step"));
        let model_name = find_element_by_id(page.doc(), page.doc().root(), "gatekeeper-model-name").unwrap();
        page.act(model_name, NodeAction::SetValue("local-model".to_string()));
        let configure_model = find_by_attribute(
            page.doc(), page.doc().root(), "data-gatekeeper-action", "configure-model",
        ).unwrap();
        assert_eq!(page.gatekeeper_settings_change_for(configure_model), Some(
            GatekeeperSettingsChange::ConfigureLocalModel {
                provider: "ollama".to_string(),
                base_url: "http://127.0.0.1:11434/v1/".to_string(),
                model: "local-model".to_string(),
            }
        ));
        let input =
            find_element_by_id(page.doc(), page.doc().root(), "gatekeeper-custom-host").unwrap();
        let add = find_by_attribute(
            page.doc(),
            page.doc().root(),
            "data-gatekeeper-action",
            "add-host",
        )
        .unwrap();
        page.act(input, NodeAction::SetValue("tracker.example".to_string()));
        assert!(page.gatekeeper_settings.is_some());
        assert_eq!(
            page.gatekeeper_settings_change_for(add),
            Some(GatekeeperSettingsChange::AddBlockedHost {
                host: "tracker.example".to_string()
            })
        );
        assert_eq!(page.apply_gatekeeper_settings_control(add), Some(Ok(())));
        assert!(page.dom_dump().contains("tracker.example"));
        assert!(page.dom_dump().contains("Your blocked hosts"));
        assert!(page.dom_dump().contains("Gatekeeper settings saved"));
        let phrase_input = find_element_by_id(
            page.doc(),
            page.doc().root(),
            "gatekeeper-custom-phrase",
        ).unwrap();
        let add_phrase = find_by_attribute(
            page.doc(),
            page.doc().root(),
            "data-gatekeeper-action",
            "add-phrase",
        ).unwrap();
        page.act(phrase_input, NodeAction::SetValue("Private Code".to_string()));
        assert_eq!(
            page.gatekeeper_settings_change_for(add_phrase),
            Some(GatekeeperSettingsChange::AddBlockedPhrase {
                phrase: "Private Code".to_string(),
            })
        );
        assert_eq!(page.apply_gatekeeper_settings_control(add_phrase), Some(Ok(())));
        assert!(page.dom_dump().contains("private code"));
        let remove_phrase = find_by_attribute(
            page.doc(),
            page.doc().root(),
            "data-gatekeeper-action",
            "remove-phrase",
        ).unwrap();
        assert_eq!(page.apply_gatekeeper_settings_control(remove_phrase), Some(Ok(())));
        assert!(service.settings().custom_blocked_phrases.is_empty());
        let extension_input = find_element_by_id(
            page.doc(), page.doc().root(), "gatekeeper-custom-extension",
        ).unwrap();
        let add_extension = find_by_attribute(
            page.doc(), page.doc().root(), "data-gatekeeper-action", "add-extension",
        ).unwrap();
        page.act(extension_input, NodeAction::SetValue(".zip".to_string()));
        assert_eq!(page.apply_gatekeeper_settings_control(add_extension), Some(Ok(())));
        assert!(page.dom_dump().contains(".zip"));
        let remove_extension = find_by_attribute(
            page.doc(), page.doc().root(), "data-gatekeeper-action", "remove-extension",
        ).unwrap();
        assert_eq!(page.apply_gatekeeper_settings_control(remove_extension), Some(Ok(())));
        assert!(service.settings().custom_blocked_download_extensions.is_empty());
        let popup_input = find_element_by_id(
            page.doc(), page.doc().root(), "gatekeeper-custom-popup-phrase",
        ).unwrap();
        let add_popup = find_by_attribute(
            page.doc(), page.doc().root(), "data-gatekeeper-action", "add-popup-phrase",
        ).unwrap();
        page.act(popup_input, NodeAction::SetValue("send secrets".to_string()));
        assert_eq!(page.apply_gatekeeper_settings_control(add_popup), Some(Ok(())));
        assert!(page.dom_dump().contains("send secrets"));
        let remove_popup = find_by_attribute(
            page.doc(), page.doc().root(), "data-gatekeeper-action", "remove-popup-phrase",
        ).unwrap();
        assert_eq!(page.apply_gatekeeper_settings_control(remove_popup), Some(Ok(())));
        assert!(service.settings().custom_blocked_popup_phrases.is_empty());
        let model_name = find_element_by_id(page.doc(), page.doc().root(), "gatekeeper-model-name").unwrap();
        page.act(model_name, NodeAction::SetValue("local-model".to_string()));
        let configure_model = find_by_attribute(
            page.doc(), page.doc().root(), "data-gatekeeper-action", "configure-model",
        ).unwrap();
        assert_eq!(page.apply_gatekeeper_settings_control(configure_model), Some(Ok(())));
        assert!(service.settings().model_review_active);
        assert!(page.dom_dump().contains("Optional local model (blocks if unavailable)"));
        let disable_model = find_by_attribute(
            page.doc(), page.doc().root(), "data-gatekeeper-action", "disable-model",
        ).unwrap();
        assert_eq!(page.apply_gatekeeper_settings_control(disable_model), Some(Ok(())));
        assert!(!service.settings().model_review_active);
        worker.join().unwrap();
        let _ = std::fs::remove_file(socket);
    }

    #[test]
    fn act_set_value_updates_the_value_attribute_and_the_painted_frame() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(r#"<input type="text">"#, None);
        let input_id = find_by_tag(page.doc(), page.doc().root(), "input").unwrap();

        page.act(input_id, NodeAction::SetValue("hello".to_string()));

        let NodeData::Element { attributes, .. } = page.doc().data(input_id) else {
            panic!("expected an element")
        };
        assert!(attributes.contains(&("value".to_string(), "hello".to_string())));
        let frame = page.render();
        assert!(
            frame
                .commands
                .iter()
                .any(|command| matches!(command, PaintCommand::Rect { rect, .. } if rect.width > 0.0 && rect.height > 0.0)),
            "the post-action frame must retain the visible input control box"
        );
        assert!(
            frame.commands.iter().any(|command| {
                matches!(command, PaintCommand::Text { text, .. } if text == "hello")
            }),
            "the post-action frame must visibly contain the core-owned value"
        );
        let snapshot = page.snapshot(1, 1);
        assert_eq!(snapshot.nodes[0].state.value.as_deref(), Some("hello"));
    }

    #[test]
    fn focused_text_entry_is_limited_to_the_enabled_clicked_text_input() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(
            r#"<input id="host" type="text"><input id="locked" disabled><input id="secret" type="password">"#,
            None,
        );
        let host = page.script_get_element_by_id("host").unwrap();
        let locked = page.script_get_element_by_id("locked").unwrap();
        let secret = page.script_get_element_by_id("secret").unwrap();

        assert!(page.focus_text_input_at(Some(host)));
        assert!(page.insert_focused_text("tracker.example"));
        assert!(page.delete_focused_text_backward());
        assert_eq!(
            element_attribute(page.doc(), host, "value"),
            Some("tracker.exampl")
        );

        assert!(page.focus_text_input_at(Some(locked)));
        assert!(!page.insert_focused_text("must not write"));
        assert_eq!(element_attribute(page.doc(), locked, "value"), None);

        assert!(!page.focus_text_input_at(Some(secret)));
        assert!(!page.insert_focused_text("must not write"));
        assert_eq!(element_attribute(page.doc(), secret, "value"), None);
    }

    #[test]
    fn extension_text_input_write_only_accepts_live_supported_text_inputs() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(
            r#"<input id="text"><input id="password" type="password"><div id="other"></div>"#,
            None,
        );
        let text = page.script_get_element_by_id("text").unwrap();
        let password = page.script_get_element_by_id("password").unwrap();
        let other = page.script_get_element_by_id("other").unwrap();

        page.set_text_input_value(text, "from extension".to_string())
            .unwrap();
        assert_eq!(
            page.snapshot(1, 1)
                .nodes
                .iter()
                .find(|node| node.id == text.as_u64())
                .and_then(|node| node.state.value.as_deref()),
            Some("from extension")
        );
        assert!(page
            .set_text_input_value(password, "must not write".to_string())
            .is_err());
        assert!(page
            .set_text_input_value(other, "must not write".to_string())
            .is_err());
        assert!(page
            .set_text_input_value(NodeId::from_u64(9_999), "stale".to_string())
            .is_err());
    }

    #[test]
    fn extension_textarea_write_only_changes_live_enabled_native_textareas() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(
            r#"<textarea id="notes">before</textarea><textarea id="disabled" disabled>locked</textarea><input id="other">"#,
            None,
        );
        let textarea = page.script_get_element_by_id("notes").unwrap();
        let disabled = page.script_get_element_by_id("disabled").unwrap();
        let other = page.script_get_element_by_id("other").unwrap();

        page.set_textarea_value(textarea, "after\nwith detail".to_string())
            .unwrap();
        assert_eq!(
            page.snapshot(1, 1)
                .nodes
                .iter()
                .find(|node| node.id == textarea.as_u64())
                .and_then(|node| node.state.value.as_deref()),
            Some("after with detail")
        );
        assert!(page
            .set_textarea_value(disabled, "must not write".to_string())
            .is_err());
        assert!(page
            .set_textarea_value(other, "must not write".to_string())
            .is_err());
        assert!(page
            .set_textarea_value(NodeId::from_u64(9_999), "stale".to_string())
            .is_err());
    }

    #[test]
    fn extension_visible_leaf_write_preserves_nested_content_and_rejects_hidden_or_builtin_pages() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(
            r#"<h1 id="title">Before</h1><p id="nested">Before <a href="/next">link</a></p><p id="hidden" hidden>Secret</p><p id="aria" aria-hidden="TRUE">Secret</p><p id="named" aria-label="Other">Secret</p><p id="none" style="display:none">Secret</p><input id="field" value="old">"#,
            Some("https://example.test/".to_string()),
        );
        let title = page.script_get_element_by_id("title").unwrap();
        let original_text = page.doc.children(title).next().unwrap();
        page.set_visible_leaf_text(title, "After".to_string()).unwrap();
        assert_eq!(page.doc.children(title).next(), Some(original_text));
        assert_eq!(
            page.snapshot(1, 1)
                .nodes
                .iter()
                .find(|node| node.id == title.as_u64())
                .and_then(|node| node.name.as_deref()),
            Some("After")
        );
        for id in ["nested", "hidden", "aria", "named", "none", "field"] {
            let node = page.script_get_element_by_id(id).unwrap();
            assert!(
                page.set_visible_leaf_text(node, "changed".to_string())
                    .is_err(),
                "{id} must not be writable"
            );
        }
        assert!(page
            .set_visible_leaf_text(
                title,
                "x".repeat(blueice_ipc::extension::MAX_VISIBLE_LEAF_TEXT_BYTES + 1)
            )
            .is_err());
        assert!(page.set_visible_leaf_text(title, "  \n  ".to_string()).is_err());
        page.navigate("about:credits").unwrap();
        assert!(page.set_visible_leaf_text(title, "changed".to_string()).is_err());
    }

    #[test]
    fn extension_visible_text_content_replaces_only_noninteractive_inline_markup() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(
            r#"<p id="formatted">Before <strong>bold <em>and italic</em></strong></p><p id="linked">Before <a href="/next">link</a></p><p id="event">Before <span onclick="go()">event</span></p><p id="named">Before <span id="target">named child</span></p><p id="hidden-child">Before <span style="display:none">secret</span></p><p id="hidden" hidden>Secret</p><input id="field" value="old">"#,
            Some("https://example.test/page".to_string()),
        );
        let formatted = page.script_get_element_by_id("formatted").unwrap();
        let old_child = page.doc.children(formatted).next().unwrap();
        page.set_visible_text_content(formatted, "After".to_string())
            .unwrap();
        assert!(!page.doc.contains(old_child));
        assert_eq!(page.doc.children(formatted).count(), 1);
        assert_eq!(
            page.snapshot(1, 1)
                .nodes
                .iter()
                .find(|node| node.id == formatted.as_u64())
                .and_then(|node| node.name.as_deref()),
            Some("After")
        );
        for id in ["linked", "event", "named", "hidden-child", "hidden", "field"] {
            let node = page.script_get_element_by_id(id).unwrap();
            assert!(
                page.set_visible_text_content(node, "must not change".to_string())
                    .is_err(),
                "{id} must not be writable"
            );
        }
        assert!(page
            .set_visible_text_content(
                formatted,
                "x".repeat(blueice_ipc::extension::MAX_VISIBLE_LEAF_TEXT_BYTES + 1)
            )
            .is_err());
        page.navigate("about:blank").unwrap();
        assert!(page
            .set_visible_text_content(formatted, "must not change".to_string())
            .is_err());
    }

    #[test]
    fn extension_range_write_only_changes_live_enabled_integer_ranges() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(
            r#"
                <input id="volume" type="range" min="-5" max="5" step="2" value="-5">
                <input id="default" type="range">
                <input id="disabled" type="range" disabled>
                <input id="fractional" type="range" min="0.5" max="2">
                <input id="any" type="range" step="any">
                <input id="wide" type="range" min="-1" max="9223372036854775807">
                <input id="text" type="text">
            "#,
            None,
        );
        let volume = page.script_get_element_by_id("volume").unwrap();
        let default = page.script_get_element_by_id("default").unwrap();
        let disabled = page.script_get_element_by_id("disabled").unwrap();
        let fractional = page.script_get_element_by_id("fractional").unwrap();
        let any = page.script_get_element_by_id("any").unwrap();
        let wide = page.script_get_element_by_id("wide").unwrap();
        let text = page.script_get_element_by_id("text").unwrap();

        page.set_range_input_value(volume, 3).unwrap();
        page.set_range_input_value(default, 42).unwrap();
        page.set_range_input_value(wide, i64::MAX).unwrap();
        let snapshot = page.snapshot(1, 1);
        let value = |id: NodeId| {
            snapshot
                .nodes
                .iter()
                .find(|node| node.id == id.as_u64())
                .and_then(|node| node.state.value.as_deref())
        };
        assert_eq!(value(volume), Some("3"));
        assert_eq!(value(default), Some("42"));
        assert_eq!(value(wide), Some("9223372036854775807"));

        assert!(page.set_range_input_value(volume, 2).is_err());
        assert!(page.set_range_input_value(volume, 7).is_err());
        assert!(page.set_range_input_value(disabled, 1).is_err());
        assert!(page.set_range_input_value(fractional, 1).is_err());
        assert!(page.set_range_input_value(any, 1).is_err());
        assert!(page.set_range_input_value(text, 1).is_err());
        assert!(page
            .set_range_input_value(NodeId::from_u64(9_999), 1)
            .is_err());
    }

    #[test]
    fn extension_checkbox_write_only_changes_live_enabled_native_checkboxes() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(
            r#"<input id="check" type="checkbox"><input id="disabled" type="checkbox" disabled><input id="radio" type="radio"><div id="other"></div>"#,
            None,
        );
        let checkbox = page.script_get_element_by_id("check").unwrap();
        let disabled = page.script_get_element_by_id("disabled").unwrap();
        let radio = page.script_get_element_by_id("radio").unwrap();
        let other = page.script_get_element_by_id("other").unwrap();

        page.set_checkbox_checked(checkbox, true).unwrap();
        assert_eq!(
            page.snapshot(1, 1)
                .nodes
                .iter()
                .find(|node| node.id == checkbox.as_u64())
                .and_then(|node| node.state.checked),
            Some(true)
        );
        page.set_checkbox_checked(checkbox, false).unwrap();
        assert_eq!(
            page.snapshot(2, 1)
                .nodes
                .iter()
                .find(|node| node.id == checkbox.as_u64())
                .and_then(|node| node.state.checked),
            Some(false)
        );
        assert!(page.set_checkbox_checked(disabled, true).is_err());
        assert!(page.set_checkbox_checked(radio, true).is_err());
        assert!(page.set_checkbox_checked(other, true).is_err());
        assert!(page
            .set_checkbox_checked(NodeId::from_u64(9_999), true)
            .is_err());
    }

    #[test]
    fn extension_radio_write_selects_only_its_live_named_local_group() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(
            r#"
                <form id="one">
                  <input id="first" type="radio" name="choice" checked>
                  <input id="second" type="radio" name="choice">
                  <input id="other-name" type="radio" name="other" checked>
                  <input id="disabled" type="radio" name="choice" disabled>
                </form>
                <form id="two"><input id="other-form" type="radio" name="choice" checked></form>
                <input id="unnamed" type="radio">
                <input id="external" type="radio" name="choice" form="one">
                <div id="other"></div>
            "#,
            None,
        );
        let first = page.script_get_element_by_id("first").unwrap();
        let second = page.script_get_element_by_id("second").unwrap();
        let other_name = page.script_get_element_by_id("other-name").unwrap();
        let disabled = page.script_get_element_by_id("disabled").unwrap();
        let other_form = page.script_get_element_by_id("other-form").unwrap();
        let unnamed = page.script_get_element_by_id("unnamed").unwrap();
        let external = page.script_get_element_by_id("external").unwrap();
        let other = page.script_get_element_by_id("other").unwrap();

        page.set_radio_checked(second).unwrap();
        let snapshot = page.snapshot(1, 1);
        let checked = |id: NodeId| {
            snapshot
                .nodes
                .iter()
                .find(|node| node.id == id.as_u64())
                .and_then(|node| node.state.checked)
        };
        assert_eq!(checked(first), Some(false));
        assert_eq!(checked(second), Some(true));
        assert_eq!(checked(other_name), Some(true));
        assert_eq!(checked(other_form), Some(true));

        assert!(page.set_radio_checked(disabled).is_err());
        assert!(page.set_radio_checked(unnamed).is_err());
        assert!(page.set_radio_checked(external).is_err());
        assert!(page.set_radio_checked(other).is_err());
        assert!(page.set_radio_checked(NodeId::from_u64(9_999)).is_err());
    }

    #[test]
    fn extension_select_option_selects_only_an_enabled_live_single_select_choice() {
        let mut page = Page::new(320.0, 200.0);
        page.load_html_str(
            r#"
                <label for="priority">Priority</label>
                <select id="priority">
                  <option id="first" selected>First</option>
                  <option id="second">Second</option>
                  <option id="disabled" disabled>Disabled</option>
                  <optgroup label="Locked" disabled><option id="locked">Locked</option></optgroup>
                </select>
                <select id="multiple" multiple><option id="many">Many</option></select>
                <select id="disabled-select" disabled><option id="disabled-owner">Disabled owner</option></select>
                <div id="other"></div>
            "#,
            None,
        );
        let first = page.script_get_element_by_id("first").unwrap();
        let second = page.script_get_element_by_id("second").unwrap();
        let disabled = page.script_get_element_by_id("disabled").unwrap();
        let locked = page.script_get_element_by_id("locked").unwrap();
        let many = page.script_get_element_by_id("many").unwrap();
        let disabled_owner = page.script_get_element_by_id("disabled-owner").unwrap();
        let other = page.script_get_element_by_id("other").unwrap();

        page.select_option(second).unwrap();
        let snapshot = page.snapshot(1, 1);
        let selected = |id: NodeId| {
            snapshot
                .nodes
                .iter()
                .find(|node| node.id == id.as_u64())
                .map(|node| node.state.selected)
        };
        assert_eq!(selected(first), Some(false));
        assert_eq!(selected(second), Some(true));

        assert!(page.select_option(disabled).is_err());
        assert!(page.select_option(locked).is_err());
        assert!(page.select_option(many).is_err());
        assert!(page.select_option(disabled_owner).is_err());
        assert!(page.select_option(other).is_err());
        assert!(page.select_option(NodeId::from_u64(9_999)).is_err());
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
        let response = page.network_response().expect("a network page has response metadata");
        assert_eq!(response.method, "GET");
        assert_eq!(response.status, 200);
        assert_eq!(response.final_url, format!("http://{addr}"));
        assert!(page
            .render()
            .commands
            .iter()
            .any(|c| matches!(c, PaintCommand::Text { text, .. } if text == "fetched")));
        page.navigate("about:blank").unwrap();
        assert!(page.network_response().is_none());
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

    use crate::downloads_page::test_support::{fake_downloads, Scratch};
    use crate::downloads_page::DownloadsSource;
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
    fn about_downloads_says_the_service_is_not_running_when_there_is_no_source_or_it_is_unreachable(
    ) {
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
        assert!(unreachable
            .dom_dump()
            .contains("The downloads service is not running"));
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
