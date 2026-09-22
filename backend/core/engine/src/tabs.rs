// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! [`TabManager`]: `core`'s collection of [`Page`]s, per
//! `phase-16-multi-tab-and-tab-groups/PLAN.md`'s minimal first slice.
//! Sits at the same layer `Page` itself does -- testable domain logic,
//! no wire-protocol knowledge -- mirroring the existing `Page`/
//! `session.rs` split: `session.rs` resolves a
//! `blueice_ipc::ClientMessage`'s `tab_id` to a [`TabId`] and dispatches
//! into whichever `Page` that resolves to, exactly the way it already
//! dispatches into a single `Page`'s methods.
//!
//! `Page` itself is unchanged -- still exactly "one navigable context,
//! one instance" (`phase-2-mvp-scope/PLAN.md`'s original framing,
//! un-reversed). What changes is that `core` now owns a collection of
//! them instead of exactly one.

use crate::downloads_page::DownloadsSource;
use crate::Page;
use std::collections::HashMap;
use std::sync::Arc;

/// Stable identity for a tab, assigned once at [`TabManager::open_tab`]
/// (or at [`TabManager::new`] for the initial tab) and never reused --
/// mirrors `blueice_dom::NodeId`'s own monotonic-counter, never-an-
/// array-index design, for the same reason: a closed tab's ID must
/// never be handed to an unrelated later tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(u64);

impl TabId {
    /// The raw counter value -- needed wherever a `TabId` has to cross
    /// a boundary that can't carry the type itself, e.g. the wire
    /// protocol's envelope `tab_id: Option<u64>` field or
    /// `blueice_ipc::AiSnapshot::tab_id`.
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Reconstructs a `TabId` from a raw value previously obtained from
    /// [`TabId::as_u64`] -- for turning a client-supplied integer back
    /// into something [`TabManager`]'s lookups accept. A value that was
    /// never allocated (or belongs to a different `TabManager`, e.g. a
    /// stale ID from before this process started) simply won't resolve
    /// to anything -- callers should treat a failed [`TabManager::get`]
    /// as a real addressing error, not assume every `TabId` is live.
    pub fn from_u64(id: u64) -> TabId {
        TabId(id)
    }
}

/// Stable identity for a tab group. Like [`TabId`], it is monotonic and never
/// reused, so a delayed group command can never accidentally mutate a later,
/// unrelated group that happened to occupy an old array slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupId(u64);

impl GroupId {
    pub fn as_u64(self) -> u64 {
        self.0
    }

    pub fn from_u64(id: u64) -> GroupId {
        GroupId(id)
    }
}

/// Core-owned, observer-independent tab-group state. Selection is
/// intentionally absent: which member a particular frontend displays belongs
/// to that frontend, while this shared organization belongs to the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabGroup {
    id: GroupId,
    name: String,
    color: String,
    collapsed: bool,
}

impl TabGroup {
    pub fn id(&self) -> GroupId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn color(&self) -> &str {
        &self.color
    }

    pub fn collapsed(&self) -> bool {
        self.collapsed
    }
}

/// The behavior to use when a tab leaves a history entry. Reloading is the
/// normal browser behavior: traversing the entry fetches its URL again, so the
/// result can reflect a changed server. Snapshot retention is intentionally
/// opt-in, for callers that value a locally renderable historical view over a
/// fresh network document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistorySnapshotMode {
    /// Retain only the entry URL; Back/Forward fetches it again.
    #[default]
    Reload,
    /// Retain the whole page as a displayable, no-network snapshot.
    Snapshot,
}

/// A direction within one tab's session history. This is crate-visible rather
/// than wire-visible: IPC deliberately keeps Back/Forward as separate unit
/// variants, while the engine uses one type to share its commit logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryDirection {
    Back,
    Forward,
}

/// What a history entry needs before it can become the current page. Kept
/// separate from [`HistoryEntry`] so callers never receive an owned `Page`
/// until they have chosen the explicit snapshot restoration path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryDestination {
    /// A retained page can be displayed without consulting the network.
    Snapshot,
    /// Fetch this URL again. `None` is the initial blank document.
    Reload(Option<String>),
}

/// One item in a tab's history. A `Page` is only retained when snapshot mode
/// was explicitly enabled; the default path stores just a URL and therefore
/// cannot accidentally turn a history traversal into an offline cache.
enum HistoryEntry {
    Reload { url: Option<String> },
    Snapshot(Box<Page>),
}

impl HistoryEntry {
    fn from_page(page: Page, mode: HistorySnapshotMode) -> Self {
        match mode {
            HistorySnapshotMode::Reload => HistoryEntry::Reload {
                url: page.url().map(str::to_string),
            },
            HistorySnapshotMode::Snapshot => HistoryEntry::Snapshot(Box::new(page)),
        }
    }

    fn snapshot_mut(&mut self) -> Option<&mut Page> {
        match self {
            HistoryEntry::Reload { .. } => None,
            HistoryEntry::Snapshot(page) => Some(page.as_mut()),
        }
    }

    fn snapshot(&self) -> Option<&Page> {
        match self {
            HistoryEntry::Reload { .. } => None,
            HistoryEntry::Snapshot(page) => Some(page.as_ref()),
        }
    }
}

struct Tab {
    /// The document currently exposed by this tab. Prior and next entries are
    /// URL records by default; a full `Page` is retained only under the
    /// explicit [`HistorySnapshotMode::Snapshot`] policy.
    page: Page,
    back: Vec<HistoryEntry>,
    forward: Vec<HistoryEntry>,
    group_id: Option<GroupId>,
}

/// `core`'s collection of [`Page`]s. Always has a [`TabManager::
/// default_tab`] identity fixed at construction -- used when a
/// `ClientMessage`'s envelope carries no explicit `tab_id`, reproducing
/// pre-Phase-16 single-`Page` behavior byte-for-byte for a client that
/// never sends `OpenTab`. That identity is *not* reassigned if the
/// default tab is later closed (real browsers do MRU/adjacent-tab
/// heuristics for "what becomes current next" -- deliberately out of
/// scope here): an untagged message after the default tab is closed
/// resolves to a `TabId` that no longer looks anything up, which
/// `session.rs` already treats as an ordinary addressing error, the
/// same as any other stale ID -- one consistent failure mode, not a
/// special case.
pub struct TabManager {
    tabs: HashMap<TabId, Tab>,
    /// Insertion order, for a deterministic `ListTabs` reply --
    /// `HashMap` iteration order isn't stable, and a browser's tab list
    /// visibly ordered by creation is the whole point of a tab list.
    order: Vec<TabId>,
    next_tab_id: u64,
    default_tab: TabId,
    groups: HashMap<GroupId, TabGroup>,
    /// Creation order gives `ListTabGroups` stable, browser-like ordering.
    group_order: Vec<GroupId>,
    next_group_id: u64,
    /// The one physical window's current size -- every tab shares it
    /// (there is one `frontend` window regardless of tab count), so a
    /// newly [`TabManager::open_tab`]-ed tab starts at whatever the
    /// window's current size is, not a hardcoded default.
    viewport_width: f64,
    viewport_height: f64,
    /// Handed to every tab (existing and future) so `about:downloads` works
    /// in any of them.
    downloads: Option<Arc<DownloadsSource>>,
    /// Whether leaving an entry retains a locally displayable page snapshot,
    /// rather than the default URL-only history record.
    history_snapshot_mode: HistorySnapshotMode,
}

impl TabManager {
    /// Creates the manager with exactly one initial tab -- a client
    /// that never sends `OpenTab` sees identical behavior to before
    /// this phase existed.
    pub fn new(viewport_width: f64, viewport_height: f64) -> Self {
        Self::new_with_history_snapshot_mode(
            viewport_width,
            viewport_height,
            HistorySnapshotMode::Reload,
        )
    }

    /// Creates a manager with explicitly selected history retention. The
    /// ordinary constructor deliberately selects [`HistorySnapshotMode::Reload`]
    /// so opening an old entry has normal URL-reload semantics.
    pub fn new_with_history_snapshot_mode(
        viewport_width: f64,
        viewport_height: f64,
        history_snapshot_mode: HistorySnapshotMode,
    ) -> Self {
        let default_tab = TabId(1);
        let mut tabs = HashMap::new();
        tabs.insert(
            default_tab,
            Tab {
                page: Page::new(viewport_width, viewport_height),
                back: Vec::new(),
                forward: Vec::new(),
                group_id: None,
            },
        );
        TabManager {
            tabs,
            order: vec![default_tab],
            next_tab_id: 2,
            default_tab,
            groups: HashMap::new(),
            group_order: Vec::new(),
            next_group_id: 1,
            viewport_width,
            viewport_height,
            downloads: None,
            history_snapshot_mode,
        }
    }

    pub fn history_snapshot_mode(&self) -> HistorySnapshotMode {
        self.history_snapshot_mode
    }

    pub fn default_tab(&self) -> TabId {
        self.default_tab
    }

    /// Opens a new, blank tab at the window's current size, returning
    /// its `TabId`.
    pub fn open_tab(&mut self) -> TabId {
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        let mut page = Page::new(self.viewport_width, self.viewport_height);
        page.set_downloads_source(self.downloads.clone());
        self.tabs.insert(
            id,
            Tab {
                page,
                back: Vec::new(),
                forward: Vec::new(),
                group_id: None,
            },
        );
        self.order.push(id);
        id
    }

    /// Where every tab's `about:downloads` reads from -- applied to the
    /// tabs that exist now and to every tab opened later.
    pub fn set_downloads_source(&mut self, source: Arc<DownloadsSource>) {
        for tab in self.tabs.values_mut() {
            tab.page.set_downloads_source(Some(source.clone()));
            for entry in tab.back.iter_mut().chain(tab.forward.iter_mut()) {
                if let Some(page) = entry.snapshot_mut() {
                    page.set_downloads_source(Some(source.clone()));
                }
            }
        }
        self.downloads = Some(source);
    }

    /// The source `about:downloads` reads from, if one was set.
    pub fn downloads_source(&self) -> Option<&Arc<DownloadsSource>> {
        self.downloads.as_ref()
    }

    /// Closes `id`, returning `true` if it existed. Closing the last
    /// remaining tab -- or the current default tab -- is allowed;
    /// `core` doesn't force a tab to always exist, matching
    /// `ChromeCommand`-level chrome policy staying `frontend`'s concern,
    /// not something `core` enforces.
    pub fn close_tab(&mut self, id: TabId) -> bool {
        let existed = self.tabs.remove(&id).is_some();
        if existed {
            self.order.retain(|&t| t != id);
        }
        existed
    }

    pub fn get(&self, id: TabId) -> Option<&Page> {
        self.tabs.get(&id).map(|tab| &tab.page)
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Page> {
        self.tabs.get_mut(&id).map(|tab| &mut tab.page)
    }

    /// Whether this tab has a previous session-history entry. History belongs
    /// to the tab, never to `TabManager` globally: one observer can move tab 2
    /// back while tab 1 stays exactly where another observer left it.
    pub fn can_go_back(&self, id: TabId) -> Option<bool> {
        self.tabs.get(&id).map(|tab| !tab.back.is_empty())
    }

    /// Whether this tab has a next session-history entry.
    pub fn can_go_forward(&self, id: TabId) -> Option<bool> {
        self.tabs.get(&id).map(|tab| !tab.forward.is_empty())
    }

    /// Returns what a history traversal needs to do before committing. The
    /// initial blank entry is rebuilt locally rather than fetched.
    pub(crate) fn history_destination(
        &self,
        id: TabId,
        direction: HistoryDirection,
    ) -> Option<HistoryDestination> {
        let tab = self.tabs.get(&id)?;
        let entry = match direction {
            HistoryDirection::Back => tab.back.last(),
            HistoryDirection::Forward => tab.forward.last(),
        }?;
        match entry {
            HistoryEntry::Reload { url } => Some(HistoryDestination::Reload(url.clone())),
            HistoryEntry::Snapshot(_) => Some(HistoryDestination::Snapshot),
        }
    }

    /// Restores the selected snapshot entry. The `false` result means this
    /// entry is URL-only and must go through the normal navigation path.
    pub(crate) fn restore_history_snapshot(
        &mut self,
        id: TabId,
        direction: HistoryDirection,
    ) -> bool {
        let mode = self.history_snapshot_mode;
        let Some(tab) = self.tabs.get_mut(&id) else {
            return false;
        };
        let is_snapshot = match direction {
            HistoryDirection::Back => tab.back.last(),
            HistoryDirection::Forward => tab.forward.last(),
        }
        .is_some_and(|entry| matches!(entry, HistoryEntry::Snapshot(_)));
        if !is_snapshot {
            return false;
        }
        let target = match direction {
            HistoryDirection::Back => tab.back.pop(),
            HistoryDirection::Forward => tab.forward.pop(),
        }
        .expect("a checked history entry still exists");
        let HistoryEntry::Snapshot(next) = target else {
            unreachable!("the checked history entry is a snapshot");
        };
        let current = std::mem::replace(&mut tab.page, *next);
        let departure = HistoryEntry::from_page(current, mode);
        match direction {
            HistoryDirection::Back => tab.forward.push(departure),
            HistoryDirection::Forward => tab.back.push(departure),
        }
        true
    }

    /// Applies a trusted built-in navigation as a new session-history entry.
    /// `false` means `url` was not one of BlueIce's built-in pages; callers
    /// must then validate/gate an ordinary network navigation instead.
    pub(crate) fn navigate_to_built_in(&mut self, id: TabId, url: &str) -> bool {
        let mut next = self.new_history_page(id);
        if !next.load_built_in(url) {
            return false;
        }
        self.replace_current_as_new_navigation(id, next);
        true
    }

    /// Commits an already-gated, already-fetched network document as a new
    /// history entry. The clearance type keeps this path unavailable to
    /// callers that have not passed the gatekeeper.
    pub(crate) fn apply_fetched_navigation(
        &mut self,
        id: TabId,
        clearance: crate::gatekeeper_client::GatekeeperClearance,
        url: &str,
        html: &str,
    ) {
        let mut next = self.new_history_page(id);
        next.apply_fetched(clearance, url, html);
        self.replace_current_as_new_navigation(id, next);
    }

    /// Rebuilds an addressed built-in history entry without creating a new
    /// branch. `false` means the entry isn't built in and must be gated and
    /// fetched like an ordinary URL.
    pub(crate) fn navigate_history_to_built_in(
        &mut self,
        id: TabId,
        direction: HistoryDirection,
        url: &str,
    ) -> bool {
        let mut next = self.new_history_page(id);
        if !next.load_built_in(url) {
            return false;
        }
        self.replace_current_from_history(id, direction, next)
    }

    /// Restores the initial blank history entry. It is intentionally local:
    /// an absent URL does not denote a network resource to fetch.
    pub(crate) fn navigate_history_to_blank(
        &mut self,
        id: TabId,
        direction: HistoryDirection,
    ) -> bool {
        let next = self.new_history_page(id);
        self.replace_current_from_history(id, direction, next)
    }

    /// Commits a cleared, fetched document into the entry selected before the
    /// asynchronous request started. Unlike [`Self::apply_fetched_navigation`]
    /// this moves the history cursor and does not clear the opposite branch.
    pub(crate) fn apply_fetched_history_navigation(
        &mut self,
        id: TabId,
        direction: HistoryDirection,
        clearance: crate::gatekeeper_client::GatekeeperClearance,
        url: &str,
        html: &str,
    ) -> bool {
        let mut next = self.new_history_page(id);
        next.apply_fetched(clearance, url, html);
        self.replace_current_from_history(id, direction, next)
    }

    /// Allocates a replacement page with the physical window's current size,
    /// the same shared downloads source every live tab uses, and a node-ID
    /// range beyond *all* retained entries of this one tab. `NodeId` remains
    /// intentionally page-local across different tabs, but it must never be
    /// reused between documents a single tab can restore from history.
    fn new_history_page(&self, id: TabId) -> Page {
        let tab = self
            .tabs
            .get(&id)
            .expect("callers validate a tab before creating a history entry");
        let next_node_id = std::iter::once(&tab.page)
            .chain(tab.back.iter().filter_map(HistoryEntry::snapshot))
            .chain(tab.forward.iter().filter_map(HistoryEntry::snapshot))
            .map(Page::next_node_id)
            .max()
            .expect("a live tab always has a current page");
        let mut page =
            Page::new_continuing_from(self.viewport_width, self.viewport_height, next_node_id);
        page.set_downloads_source(self.downloads.clone());
        page
    }

    /// Replacing the current document is the one operation that creates a
    /// new branch in a tab's history. Any forward entries are deliberately
    /// discarded, just as a browser does after navigating from a page reached
    /// via Back.
    fn replace_current_as_new_navigation(&mut self, id: TabId, next: Page) {
        let mode = self.history_snapshot_mode;
        let tab = self
            .tabs
            .get_mut(&id)
            .expect("callers validate a tab before committing navigation");
        let previous = std::mem::replace(&mut tab.page, next);
        tab.back.push(HistoryEntry::from_page(previous, mode));
        tab.forward.clear();
    }

    /// Replaces the live page with an already-loaded history destination. The
    /// target is removed from one stack and the departing page is appended to
    /// the other, preserving the normal Back/Forward cursor shape.
    fn replace_current_from_history(
        &mut self,
        id: TabId,
        direction: HistoryDirection,
        next: Page,
    ) -> bool {
        let mode = self.history_snapshot_mode;
        let Some(tab) = self.tabs.get_mut(&id) else {
            return false;
        };
        let target = match direction {
            HistoryDirection::Back => tab.back.pop(),
            HistoryDirection::Forward => tab.forward.pop(),
        };
        if target.is_none() {
            return false;
        }
        let current = std::mem::replace(&mut tab.page, next);
        let departure = HistoryEntry::from_page(current, mode);
        match direction {
            HistoryDirection::Back => tab.forward.push(departure),
            HistoryDirection::Forward => tab.back.push(departure),
        }
        true
    }

    /// Every currently-open tab's ID, in creation order.
    pub fn ids(&self) -> impl Iterator<Item = TabId> + '_ {
        self.order.iter().copied()
    }

    /// Updates the one shared window size every future [`Self::
    /// open_tab`] call starts new tabs at -- called on every `Resize`
    /// regardless of which tab it's addressed to, since there is one
    /// physical window's dimensions, not one per tab.
    pub fn set_window_size(&mut self, width: f64, height: f64) {
        self.viewport_width = width;
        self.viewport_height = height;
    }

    /// Reflows every live tab to the one physical frontend window's content
    /// size. There is no active tab in core, so eagerly updating every page is
    /// the only policy that keeps a newly selected background tab from showing
    /// a stale viewport. The caller owns frame publication and can tag the
    /// selected tab's response with its request id.
    pub fn resize_all(&mut self, width: f64, height: f64) {
        self.set_window_size(width, height);
        for tab in self.tabs.values_mut() {
            tab.page.resize(width, height);
            // Snapshot entries are restored without a network round trip, so
            // keep their retained layouts at the one physical window's current
            // viewport too. URL-only entries have no retained page to resize.
            for entry in tab.back.iter_mut().chain(tab.forward.iter_mut()) {
                if let Some(page) = entry.snapshot_mut() {
                    page.resize(width, height);
                }
            }
        }
    }

    /// Creates a group in deterministic creation order. Validation of the
    /// wire-facing name/color form lives in `session.rs`; this domain object
    /// simply owns the resulting shared state.
    pub fn create_group(&mut self, name: String, color: String) -> GroupId {
        let id = GroupId(self.next_group_id);
        self.next_group_id += 1;
        self.groups.insert(
            id,
            TabGroup {
                id,
                name,
                color,
                collapsed: false,
            },
        );
        self.group_order.push(id);
        id
    }

    pub fn group(&self, id: GroupId) -> Option<&TabGroup> {
        self.groups.get(&id)
    }

    pub fn groups(&self) -> impl Iterator<Item = &TabGroup> {
        self.group_order.iter().filter_map(|id| self.groups.get(id))
    }

    pub fn tab_group(&self, tab_id: TabId) -> Option<GroupId> {
        self.tabs.get(&tab_id).and_then(|tab| tab.group_id)
    }

    /// Assigns an existing tab to an existing group, or removes an existing
    /// tab from its group. Callers validate both IDs first, keeping this small
    /// domain API free of protocol-specific error wording.
    pub fn set_tab_group(&mut self, tab_id: TabId, group_id: Option<GroupId>) {
        self.tabs
            .get_mut(&tab_id)
            .expect("caller validates tab before assigning a group")
            .group_id = group_id;
    }

    pub fn rename_group(&mut self, id: GroupId, name: String) {
        self.groups
            .get_mut(&id)
            .expect("caller validates group before renaming it")
            .name = name;
    }

    pub fn set_group_color(&mut self, id: GroupId, color: String) {
        self.groups
            .get_mut(&id)
            .expect("caller validates group before recoloring it")
            .color = color;
    }

    pub fn set_group_collapsed(&mut self, id: GroupId, collapsed: bool) {
        self.groups
            .get_mut(&id)
            .expect("caller validates group before collapsing it")
            .collapsed = collapsed;
    }

    /// Removes a group but deliberately preserves all of its tabs. The
    /// frontend can immediately render those former members as ungrouped.
    pub fn close_group(&mut self, id: GroupId) -> bool {
        if self.groups.remove(&id).is_none() {
            return false;
        }
        self.group_order.retain(|&group_id| group_id != id);
        for tab in self.tabs.values_mut() {
            if tab.group_id == Some(id) {
                tab.group_id = None;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_exactly_one_tab_as_the_default() {
        let tabs = TabManager::new(320.0, 200.0);
        assert_eq!(tabs.ids().collect::<Vec<_>>(), vec![tabs.default_tab()]);
        assert!(tabs.get(tabs.default_tab()).is_some());
    }

    #[test]
    fn open_tab_returns_a_distinct_id_and_the_tab_becomes_gettable() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let new_id = tabs.open_tab();
        assert_ne!(new_id, tabs.default_tab());
        assert!(tabs.get(new_id).is_some());
        assert_eq!(
            tabs.ids().collect::<Vec<_>>(),
            vec![tabs.default_tab(), new_id]
        );
    }

    #[test]
    fn open_tab_never_reuses_an_id_even_after_the_tab_that_had_it_closes() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let first = tabs.open_tab();
        assert!(tabs.close_tab(first));
        let second = tabs.open_tab();
        assert_ne!(
            first, second,
            "a closed tab's id must never be reissued to an unrelated later tab"
        );
    }

    #[test]
    fn close_tab_on_an_unknown_id_returns_false_and_changes_nothing() {
        let mut tabs = TabManager::new(320.0, 200.0);
        assert!(!tabs.close_tab(TabId::from_u64(999_999)));
        assert_eq!(tabs.ids().count(), 1);
    }

    #[test]
    fn close_tab_on_an_already_closed_id_returns_false_the_second_time() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let id = tabs.open_tab();
        assert!(tabs.close_tab(id));
        assert!(!tabs.close_tab(id));
    }

    #[test]
    fn closing_every_tab_including_the_default_is_allowed() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let default = tabs.default_tab();
        assert!(tabs.close_tab(default));
        assert_eq!(tabs.ids().count(), 0);
        // The default identity itself doesn't change -- it just no
        // longer resolves to anything, the same failure mode any other
        // stale TabId already has.
        assert_eq!(tabs.default_tab(), default);
        assert!(tabs.get(default).is_none());
    }

    #[test]
    fn get_mut_on_an_unknown_id_is_none_not_a_panic() {
        let mut tabs = TabManager::new(320.0, 200.0);
        assert!(tabs.get_mut(TabId::from_u64(999_999)).is_none());
    }

    #[test]
    fn a_newly_opened_tab_starts_at_the_current_window_size() {
        let mut tabs = TabManager::new(320.0, 200.0);
        tabs.set_window_size(640.0, 480.0);
        let id = tabs.open_tab();
        assert_eq!(tabs.get(id).unwrap().viewport_size(), (640.0, 480.0));
    }

    #[test]
    fn tab_id_round_trips_through_as_u64_and_from_u64() {
        let id = TabId::from_u64(42);
        assert_eq!(TabId::from_u64(id.as_u64()), id);
    }

    #[test]
    fn the_downloads_source_reaches_existing_and_newly_opened_tabs() {
        let mut tabs = TabManager::new(300.0, 200.0);
        let before = tabs.open_tab();
        assert!(
            tabs.downloads_source().is_none()
                && tabs.get(before).unwrap().downloads_source().is_none()
        );

        let source = Arc::new(DownloadsSource::without_spawner(std::path::PathBuf::from(
            "/nonexistent/downloads.sock",
        )));
        tabs.set_downloads_source(source.clone());
        let after = tabs.open_tab();
        for id in [tabs.default_tab(), before, after] {
            let page_source = tabs
                .get(id)
                .unwrap()
                .downloads_source()
                .unwrap_or_else(|| panic!("tab {id:?} has no source"));
            assert!(
                Arc::ptr_eq(page_source, &source),
                "every tab shares the one source"
            );
        }
        assert!(Arc::ptr_eq(tabs.downloads_source().unwrap(), &source));
    }

    #[test]
    fn groups_are_ordered_and_tab_membership_is_independent_of_page_state() {
        let mut tabs = TabManager::new(300.0, 200.0);
        let second = tabs.open_tab();
        let research = tabs.create_group("Research".to_string(), "#4f8cff".to_string());
        let reference = tabs.create_group("Reference".to_string(), "#f06a6a".to_string());
        tabs.set_tab_group(second, Some(research));

        assert_eq!(tabs.tab_group(second), Some(research));
        assert_eq!(
            tabs.groups().map(|group| group.id()).collect::<Vec<_>>(),
            vec![research, reference]
        );
        assert_eq!(tabs.group(research).unwrap().name(), "Research");
        assert_eq!(tabs.group(research).unwrap().color(), "#4f8cff");
        assert!(!tabs.group(research).unwrap().collapsed());
        assert!(
            tabs.get(second).is_some(),
            "grouping never replaces the page"
        );
    }

    #[test]
    fn closing_a_group_ungroups_but_never_closes_its_tabs() {
        let mut tabs = TabManager::new(300.0, 200.0);
        let second = tabs.open_tab();
        let group = tabs.create_group("Work".to_string(), "#2fa36b".to_string());
        tabs.set_tab_group(tabs.default_tab(), Some(group));
        tabs.set_tab_group(second, Some(group));

        assert!(tabs.close_group(group));
        assert!(tabs.get(tabs.default_tab()).is_some());
        assert!(tabs.get(second).is_some());
        assert_eq!(tabs.tab_group(tabs.default_tab()), None);
        assert_eq!(tabs.tab_group(second), None);
        assert!(tabs.groups().next().is_none());
        assert!(!tabs.close_group(group));
    }

    #[test]
    fn resize_all_keeps_background_tabs_ready_for_selection() {
        let mut tabs = TabManager::new(300.0, 200.0);
        let background = tabs.open_tab();
        tabs.resize_all(640.0, 480.0);
        assert_eq!(
            tabs.get(tabs.default_tab()).unwrap().viewport_size(),
            (640.0, 480.0)
        );
        assert_eq!(
            tabs.get(background).unwrap().viewport_size(),
            (640.0, 480.0)
        );
    }

    fn traverse_built_in_history(tabs: &mut TabManager, tab: TabId, direction: HistoryDirection) {
        match tabs.history_destination(tab, direction) {
            Some(HistoryDestination::Snapshot) => {
                assert!(tabs.restore_history_snapshot(tab, direction))
            }
            Some(HistoryDestination::Reload(None)) => {
                assert!(tabs.navigate_history_to_blank(tab, direction))
            }
            Some(HistoryDestination::Reload(Some(url))) => {
                assert!(tabs.navigate_history_to_built_in(tab, direction, &url))
            }
            None => panic!("expected a history entry"),
        }
    }

    #[test]
    fn each_tab_owns_an_independent_back_and_forward_stack() {
        let mut tabs = TabManager::new(300.0, 200.0);
        let first = tabs.default_tab();
        let second = tabs.open_tab();

        assert!(tabs.navigate_to_built_in(first, "about:credits"));
        assert!(tabs.navigate_to_built_in(second, "about:downloads"));
        assert_eq!(tabs.can_go_back(first), Some(true));
        assert_eq!(tabs.can_go_back(second), Some(true));
        assert_eq!(tabs.can_go_forward(first), Some(false));

        assert_eq!(
            tabs.history_destination(first, HistoryDirection::Back),
            Some(HistoryDestination::Reload(None)),
            "the default history policy rebuilds the initial blank page"
        );
        traverse_built_in_history(&mut tabs, first, HistoryDirection::Back);
        assert_eq!(tabs.get(first).unwrap().url(), None);
        assert_eq!(tabs.can_go_back(first), Some(false));
        assert_eq!(tabs.can_go_forward(first), Some(true));
        assert_eq!(
            tabs.get(second).unwrap().url(),
            Some("about:downloads"),
            "going back in one tab must not move another tab"
        );
        assert_eq!(tabs.can_go_forward(second), Some(false));

        traverse_built_in_history(&mut tabs, first, HistoryDirection::Forward);
        assert_eq!(tabs.get(first).unwrap().url(), Some("about:credits"));
        assert_eq!(tabs.can_go_forward(first), Some(false));
    }

    #[test]
    fn a_fresh_navigation_after_back_discards_only_that_tabs_forward_entries() {
        let mut tabs = TabManager::new(300.0, 200.0);
        let tab = tabs.default_tab();
        assert!(tabs.navigate_to_built_in(tab, "about:credits"));
        assert!(tabs.navigate_to_built_in(tab, "about:downloads"));
        traverse_built_in_history(&mut tabs, tab, HistoryDirection::Back);
        assert_eq!(tabs.get(tab).unwrap().url(), Some("about:credits"));
        assert_eq!(tabs.can_go_forward(tab), Some(true));

        assert!(tabs.navigate_to_built_in(tab, "about:blank"));
        assert_eq!(tabs.get(tab).unwrap().url(), Some("about:blank"));
        assert_eq!(tabs.can_go_forward(tab), Some(false));
        assert_eq!(
            tabs.history_destination(tab, HistoryDirection::Forward),
            None
        );
    }

    #[test]
    fn default_history_entries_retain_urls_and_require_a_reload() {
        let mut tabs = TabManager::new(300.0, 200.0);
        let tab = tabs.default_tab();
        tabs.get_mut(tab).unwrap().load_html_str(
            "<input id='draft' value='kept locally'><p>original document</p>",
            Some("https://example.test/original".to_string()),
        );
        assert!(tabs.navigate_to_built_in(tab, "about:credits"));
        assert_eq!(tabs.history_snapshot_mode(), HistorySnapshotMode::Reload);
        assert_eq!(
            tabs.history_destination(tab, HistoryDirection::Back),
            Some(HistoryDestination::Reload(Some(
                "https://example.test/original".to_string()
            )))
        );
        assert!(
            !tabs.restore_history_snapshot(tab, HistoryDirection::Back),
            "the default policy must not silently use the stale in-memory DOM"
        );
    }

    #[test]
    fn explicit_snapshot_history_restores_the_left_pages_dom_state() {
        let mut tabs =
            TabManager::new_with_history_snapshot_mode(300.0, 200.0, HistorySnapshotMode::Snapshot);
        let tab = tabs.default_tab();
        tabs.get_mut(tab).unwrap().load_html_str(
            "<input id='draft' value='kept locally'><p>original document</p>",
            Some("https://example.test/original".to_string()),
        );
        assert!(tabs.navigate_to_built_in(tab, "about:credits"));
        assert_eq!(
            tabs.history_destination(tab, HistoryDirection::Back),
            Some(HistoryDestination::Snapshot)
        );
        assert!(tabs.restore_history_snapshot(tab, HistoryDirection::Back));
        let dom = tabs.get(tab).unwrap().dom_dump();
        assert!(dom.contains("kept locally"));
        assert!(dom.contains("original document"));
        assert_eq!(
            tabs.get(tab).unwrap().url(),
            Some("https://example.test/original")
        );
    }

    #[test]
    fn snapshot_history_documents_never_reuse_node_ids_from_retained_entries() {
        let mut tabs =
            TabManager::new_with_history_snapshot_mode(300.0, 200.0, HistorySnapshotMode::Snapshot);
        let tab = tabs.default_tab();
        tabs.get_mut(tab).unwrap().load_html_str(
            "<button>first</button>",
            Some("https://example.test/first".to_string()),
        );
        let first_ids: std::collections::HashSet<u64> = tabs
            .get(tab)
            .unwrap()
            .snapshot(0, tab.as_u64())
            .nodes
            .into_iter()
            .map(|node| node.id)
            .collect();

        assert!(tabs.navigate_to_built_in(tab, "about:credits"));
        let second_ids: std::collections::HashSet<u64> = tabs
            .get(tab)
            .unwrap()
            .snapshot(0, tab.as_u64())
            .nodes
            .into_iter()
            .map(|node| node.id)
            .collect();
        assert!(first_ids.is_disjoint(&second_ids));

        // A new branch after Back clears the forward *history*, but must still
        // allocate beyond the retained document it just displaced; an old
        // client-side NodeId cannot be redirected to this replacement page.
        assert!(tabs.restore_history_snapshot(tab, HistoryDirection::Back));
        assert!(tabs.navigate_to_built_in(tab, "about:credits?lang=zh-TW"));
        let branch_ids: std::collections::HashSet<u64> = tabs
            .get(tab)
            .unwrap()
            .snapshot(0, tab.as_u64())
            .nodes
            .into_iter()
            .map(|node| node.id)
            .collect();
        assert!(first_ids.is_disjoint(&branch_ids));
        assert!(second_ids.is_disjoint(&branch_ids));
    }

    #[test]
    fn retained_snapshot_entries_reflow_with_the_shared_window() {
        let mut tabs =
            TabManager::new_with_history_snapshot_mode(300.0, 200.0, HistorySnapshotMode::Snapshot);
        let tab = tabs.default_tab();
        assert!(tabs.navigate_to_built_in(tab, "about:credits"));
        tabs.resize_all(640.0, 480.0);
        assert!(tabs.restore_history_snapshot(tab, HistoryDirection::Back));
        assert_eq!(tabs.get(tab).unwrap().viewport_size(), (640.0, 480.0));
        assert!(tabs.restore_history_snapshot(tab, HistoryDirection::Forward));
        assert_eq!(tabs.get(tab).unwrap().viewport_size(), (640.0, 480.0));
    }

    #[test]
    fn group_ids_never_reuse_a_closed_groups_identity() {
        let mut tabs = TabManager::new(300.0, 200.0);
        let first = tabs.create_group("First".to_string(), "#111111".to_string());
        assert!(tabs.close_group(first));
        let second = tabs.create_group("Second".to_string(), "#222222".to_string());
        assert_ne!(first, second);
    }
}
