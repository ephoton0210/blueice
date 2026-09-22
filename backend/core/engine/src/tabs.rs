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

use crate::Page;
pub use blueice_ipc::automation::BrowserContextId;
use std::collections::HashMap;

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
    tabs: HashMap<TabId, Page>,
    /// Insertion order, for a deterministic `ListTabs` reply --
    /// `HashMap` iteration order isn't stable, and a browser's tab list
    /// visibly ordered by creation is the whole point of a tab list.
    order: Vec<TabId>,
    next_tab_id: u64,
    default_tab: TabId,
    /// The one physical window's current size -- every tab shares it
    /// (there is one `frontend` window regardless of tab count), so a
    /// newly [`TabManager::open_tab`]-ed tab starts at whatever the
    /// window's current size is, not a hardcoded default.
    viewport_width: f64,
    viewport_height: f64,
    /// `phase-17-automation-devtools-and-ajax/PLAN.md`'s "`TabManager`
    /// gains a `BrowserContextId -> TabId -> Page` ownership layer"
    /// decision: every tab belongs to exactly one context, in creation
    /// order within it (same reasoning as `order` above). A plain
    /// `HashMap<BrowserContextId, Vec<TabId>>` rather than a richer
    /// `BrowserContext` struct -- this slice doesn't yet give a context
    /// any state of its own (cookie/cache/storage isolation, the plan's
    /// own "required part of a usable context once those stores exist"
    /// note), so there is nothing else to hang off it yet.
    contexts: HashMap<BrowserContextId, Vec<TabId>>,
    /// Reverse lookup, so closing or addressing a single tab doesn't
    /// need to scan every context's tab list.
    tab_context: HashMap<TabId, BrowserContextId>,
    next_context_id: u64,
    /// Fixed at construction, mirrors [`TabManager::default_tab`]'s own
    /// "reproduces pre-Phase-16/17 behavior byte-for-byte" role: a
    /// caller that never creates a context (every client before this
    /// phase) always finds its tabs under this one, so
    /// [`TabManager::open_tab`] (no context argument) keeps working
    /// unchanged.
    default_context: BrowserContextId,
}

impl TabManager {
    /// Creates the manager with exactly one initial tab -- a client
    /// that never sends `OpenTab` sees identical behavior to before
    /// this phase existed.
    pub fn new(viewport_width: f64, viewport_height: f64) -> Self {
        let default_tab = TabId(1);
        let default_context = BrowserContextId(1);
        let mut tabs = HashMap::new();
        tabs.insert(default_tab, Page::new(viewport_width, viewport_height));
        let mut contexts = HashMap::new();
        contexts.insert(default_context, vec![default_tab]);
        let mut tab_context = HashMap::new();
        tab_context.insert(default_tab, default_context);
        TabManager {
            tabs,
            order: vec![default_tab],
            next_tab_id: 2,
            default_tab,
            viewport_width,
            viewport_height,
            contexts,
            tab_context,
            next_context_id: 2,
            default_context,
        }
    }

    pub fn default_tab(&self) -> TabId {
        self.default_tab
    }

    pub fn default_context(&self) -> BrowserContextId {
        self.default_context
    }

    /// Opens a new, blank tab at the window's current size, returning
    /// its `TabId`. Placed under [`TabManager::default_context`] --
    /// this is the pre-context-existing entry point every `OpenTab`
    /// client message still calls, and it must keep resolving exactly
    /// where it always has.
    pub fn open_tab(&mut self) -> TabId {
        self.open_tab_in_context(self.default_context)
            .expect("the default context always exists")
    }

    /// Opens a new, blank tab under `context`, returning its `TabId` --
    /// or `None` if `context` doesn't currently exist (closed, or never
    /// allocated). The automation-service counterpart to
    /// [`TabManager::open_tab`] for a client that created its own
    /// context via [`TabManager::create_context`].
    pub fn open_tab_in_context(&mut self, context: BrowserContextId) -> Option<TabId> {
        if !self.contexts.contains_key(&context) {
            return None;
        }
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        self.tabs
            .insert(id, Page::new(self.viewport_width, self.viewport_height));
        self.order.push(id);
        self.contexts.get_mut(&context).unwrap().push(id);
        self.tab_context.insert(id, context);
        Some(id)
    }

    /// Creates a new, empty [`BrowserContextId`] -- per the plan's
    /// "`TabManager` gains a `BrowserContextId -> TabId -> Page`
    /// ownership layer" decision. A fresh context starts with no tabs;
    /// callers open pages into it with [`TabManager::open_tab_in_context`].
    pub fn create_context(&mut self) -> BrowserContextId {
        let id = BrowserContextId(self.next_context_id);
        self.next_context_id += 1;
        self.contexts.insert(id, Vec::new());
        id
    }

    /// Closes `context` and every tab/page under it, returning `true`
    /// if it existed. Closing [`TabManager::default_context`] is
    /// allowed, the same "`core` doesn't force anything to always
    /// exist" reasoning [`TabManager::close_tab`] already documents for
    /// the default tab -- its identity still doesn't get reassigned,
    /// it simply stops resolving to anything.
    pub fn close_context(&mut self, context: BrowserContextId) -> bool {
        let Some(tab_ids) = self.contexts.remove(&context) else {
            return false;
        };
        for id in tab_ids {
            self.tabs.remove(&id);
            self.order.retain(|&t| t != id);
            self.tab_context.remove(&id);
        }
        true
    }

    /// Which context `tab` belongs to, or `None` if `tab` doesn't
    /// currently exist.
    pub fn context_of(&self, tab: TabId) -> Option<BrowserContextId> {
        self.tab_context.get(&tab).copied()
    }

    /// Every currently open context's ID. No ordering guarantee beyond
    /// "some order" -- unlike [`TabManager::ids`], nothing yet consumes
    /// a context list the way `ListTabs` consumes a tab list, so there
    /// is no established creation-order contract to keep here yet.
    pub fn context_ids(&self) -> impl Iterator<Item = BrowserContextId> + '_ {
        self.contexts.keys().copied()
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
            if let Some(context) = self.tab_context.remove(&id) {
                if let Some(tab_ids) = self.contexts.get_mut(&context) {
                    tab_ids.retain(|&t| t != id);
                }
            }
        }
        existed
    }

    pub fn get(&self, id: TabId) -> Option<&Page> {
        self.tabs.get(&id)
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Page> {
        self.tabs.get_mut(&id)
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
    fn new_places_the_default_tab_under_the_default_context() {
        let tabs = TabManager::new(320.0, 200.0);
        assert_eq!(
            tabs.context_of(tabs.default_tab()),
            Some(tabs.default_context())
        );
        assert_eq!(
            tabs.context_ids().collect::<Vec<_>>(),
            vec![tabs.default_context()]
        );
    }

    #[test]
    fn open_tab_without_a_context_argument_still_uses_the_default_context() {
        // The pre-context-existing entry point every OpenTab client
        // message still calls (phase-16) must keep resolving exactly
        // where it always has -- Phase 17's own "retains the current
        // tab_id behavior" requirement.
        let mut tabs = TabManager::new(320.0, 200.0);
        let id = tabs.open_tab();
        assert_eq!(tabs.context_of(id), Some(tabs.default_context()));
    }

    #[test]
    fn create_context_returns_a_distinct_empty_context() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let context = tabs.create_context();
        assert_ne!(context, tabs.default_context());
        assert_eq!(
            tabs.context_ids().collect::<std::collections::HashSet<_>>(),
            [tabs.default_context(), context].into_iter().collect()
        );
    }

    #[test]
    fn open_tab_in_context_places_the_new_tab_under_that_context_only() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let context = tabs.create_context();
        let id = tabs.open_tab_in_context(context).unwrap();
        assert_eq!(tabs.context_of(id), Some(context));
        assert_ne!(
            tabs.context_of(id),
            tabs.context_of(tabs.default_tab()),
            "a tab opened in a new context must not resolve to the default context"
        );
        assert!(tabs.get(id).is_some());
    }

    #[test]
    fn open_tab_in_context_on_an_unknown_context_returns_none_and_creates_no_tab() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let before = tabs.ids().collect::<Vec<_>>();
        assert_eq!(tabs.open_tab_in_context(BrowserContextId(999_999)), None);
        assert_eq!(tabs.ids().collect::<Vec<_>>(), before);
    }

    #[test]
    fn context_ids_are_never_reused_even_after_the_context_closes() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let first = tabs.create_context();
        assert!(tabs.close_context(first));
        let second = tabs.create_context();
        assert_ne!(
            first, second,
            "a closed context's id must never be reissued to an unrelated later context"
        );
    }

    #[test]
    fn close_context_closes_every_tab_under_it_but_leaves_other_contexts_alone() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let context = tabs.create_context();
        let a = tabs.open_tab_in_context(context).unwrap();
        let b = tabs.open_tab_in_context(context).unwrap();
        let default_tab = tabs.default_tab();

        assert!(tabs.close_context(context));

        assert!(tabs.get(a).is_none());
        assert!(tabs.get(b).is_none());
        assert!(tabs.context_of(a).is_none());
        assert!(tabs.context_of(b).is_none());
        assert!(
            tabs.get(default_tab).is_some(),
            "closing one context must not touch a tab under a different context"
        );
        assert!(!tabs.context_ids().any(|c| c == context));
    }

    #[test]
    fn close_context_on_an_unknown_id_returns_false_and_changes_nothing() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let before_tabs = tabs.ids().collect::<Vec<_>>();
        assert!(!tabs.close_context(BrowserContextId(999_999)));
        assert_eq!(tabs.ids().collect::<Vec<_>>(), before_tabs);
    }

    #[test]
    fn close_tab_removes_it_from_its_contexts_tab_list_without_closing_the_context() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let context = tabs.create_context();
        let id = tabs.open_tab_in_context(context).unwrap();

        assert!(tabs.close_tab(id));

        assert!(tabs.context_of(id).is_none());
        assert!(
            tabs.context_ids().any(|c| c == context),
            "closing a tab must not close the context that still exists (even if now empty)"
        );
    }

    #[test]
    fn context_of_is_none_for_a_tab_id_that_was_never_allocated() {
        let tabs = TabManager::new(320.0, 200.0);
        assert_eq!(tabs.context_of(TabId::from_u64(999_999)), None);
    }
}
