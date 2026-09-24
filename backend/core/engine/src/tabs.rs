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
use crate::gatekeeper_settings_page::GatekeeperSettingsSource;
use crate::Page;
use blueice_extension_host::ExtensionRegistry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use url::Url;

/// A single extension connection cannot install an unbounded number of
/// navigation rules. The small cap keeps this deliberately declarative slice
/// from becoming a general purpose core-side routing database.
const MAX_EXTENSION_NAVIGATION_RULES_PER_CONNECTION: usize = 64;

/// A small, connection-owned declarative navigation rule. Keep exact URL,
/// host, path, and rewrite effects distinct; none is a guest callback or an
/// arbitrary request-programming language.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ExtensionNavigationBlockRule {
    ExactUrl(String),
    Host(String),
    PathPrefix { host: String, path_prefix: String },
    RedirectExactUrl { source_url: String, target_url: String },
}

/// Navigation-start rule snapshot passed to the fetch worker. The rule set is
/// immutable, but its live permission view is rechecked for every hop so a
/// revoke invalidates even an in-flight snapshot. A scoped grant applies only
/// when the target request URL has one of its exact origins.
#[derive(Debug, Clone, Default)]
pub(crate) struct ExtensionNavigationRuleSnapshot {
    rules: HashSet<(u64, ExtensionNavigationBlockRule)>,
    allowed_origins: Option<BTreeSet<String>>,
    permission: Option<ExtensionPermissionView>,
}

#[derive(Clone)]
struct ExtensionPermissionView {
    registry: Arc<ExtensionRegistry>,
    extension_id: String,
}

impl ExtensionPermissionView {
    fn intercept_generation(&self) -> Option<u64> {
        self.registry.capability_generation(&self.extension_id, "network:intercept")
    }
}

impl fmt::Debug for ExtensionPermissionView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ExtensionPermissionView")
            .field("extension_id", &self.extension_id).finish_non_exhaustive()
    }
}

#[cfg(test)]
impl From<HashSet<ExtensionNavigationBlockRule>> for ExtensionNavigationRuleSnapshot {
    fn from(rules: HashSet<ExtensionNavigationBlockRule>) -> Self {
        Self {
            rules: rules.into_iter().map(|rule| (0, rule)).collect(),
            allowed_origins: None,
            permission: None,
        }
    }
}

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
    gatekeeper_settings: Option<Arc<GatekeeperSettingsSource>>,
    /// Whether leaving an entry retains a locally displayable page snapshot,
    /// rather than the default URL-only history record.
    history_snapshot_mode: HistorySnapshotMode,
    /// Canonical HTTP(S) URL and ASCII host rules, keyed by an opaque
    /// core-allocated connection ID. Rules disappear on disconnect.
    extension_navigation_block_rules: HashMap<u64, (u64, HashSet<ExtensionNavigationBlockRule>)>,
    /// Install-time exact-origin restrictions for page-facing extension
    /// capabilities. The session checks these against the live tab at the
    /// same point it performs each read or write, avoiding a URL-check race.
    extension_capability_origins: BTreeMap<String, BTreeSet<String>>,
    extension_permissions: Option<ExtensionPermissionView>,
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
            gatekeeper_settings: None,
            history_snapshot_mode,
            extension_navigation_block_rules: HashMap::new(),
            extension_capability_origins: BTreeMap::new(),
            extension_permissions: None,
        }
    }

    /// Core installs its own registry view; external clients cannot supply
    /// this gate. Navigation workers retain it in immutable rule snapshots so
    /// a live revoke also invalidates rules already captured for a fetch.
    pub fn set_extension_permission_registry(
        &mut self,
        registry: Arc<ExtensionRegistry>,
        extension_id: String,
    ) {
        self.extension_permissions = Some(ExtensionPermissionView { registry, extension_id });
        self.prune_stale_extension_navigation_rules();
    }

    fn intercept_generation(&self) -> Option<u64> {
        self.extension_capability_generation("network:intercept")
    }

    pub(crate) fn extension_capability_generation(&self, capability: &str) -> Option<u64> {
        self.extension_permissions.as_ref().map_or(Some(0), |view| {
            view.registry.capability_generation(&view.extension_id, capability)
        })
    }

    /// Serializes a core-owned publication with the same optional grant
    /// generation captured before Gatekeeper review. A late queued request
    /// cannot borrow a newer grant after its caller timed out.
    pub(crate) fn with_stable_extension_capability<T>(
        &mut self,
        capability: &str,
        expected_generation: u64,
        effect: impl FnOnce(&mut Self) -> T,
    ) -> Result<T, String> {
        match self.extension_permissions.clone() {
            Some(view) => view.registry.with_stable_capability(
                &view.extension_id, capability, expected_generation, || effect(self),
            ),
            None if expected_generation == 0 => Ok(effect(self)),
            None => Err(format!("{capability} grant generation is unavailable")),
        }
    }

    pub(crate) fn prune_stale_extension_navigation_rules(&mut self) {
        let current = self.intercept_generation();
        self.extension_navigation_block_rules.retain(|_, (generation, _)| {
            current == Some(*generation)
        });
    }

    #[cfg(test)]
    pub(crate) fn extension_navigation_rule_owner_count(&self) -> usize {
        self.extension_navigation_block_rules.len()
    }

    pub fn set_extension_capability_origins(&mut self, scopes: BTreeMap<String, BTreeSet<String>>) {
        self.extension_capability_origins = scopes;
    }

    pub(crate) fn check_extension_origin(
        &self,
        capability: &str,
        tab_id: TabId,
    ) -> Result<(), String> {
        let page = self
            .get(tab_id)
            .ok_or_else(|| format!("unknown tab {}", tab_id.as_u64()))?;
        let Some(allowed) = self.extension_capability_origins.get(capability) else {
            return Ok(());
        };
        let origin = page
            .url()
            .and_then(|url| Url::parse(url).ok())
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .map(|url| url.origin().ascii_serialization());
        if origin.as_ref().is_some_and(|origin| allowed.contains(origin)) {
            Ok(())
        } else {
            Err(format!("{capability} is not granted for this tab's origin"))
        }
    }

    /// Adds a connection-scoped rule for a canonical HTTP(S) navigation URL.
    /// A navigation snapshots the live rule set when it begins and evaluates
    /// that snapshot before every redirect hop, but this remains declarative:
    /// there is no callback, header, or request-body semantics.
    pub(crate) fn add_extension_navigation_block_rule(
        &mut self,
        connection_id: u64,
        url: String,
    ) -> Result<(), String> {
        self.add_extension_navigation_rule(
            connection_id,
            ExtensionNavigationBlockRule::ExactUrl(canonical_http_navigation_url(&url)?),
        )
    }

    /// Adds a canonical ASCII host rule. It matches that host and its DNS
    /// subdomains before a navigation or redirect target opens a connection.
    pub(crate) fn add_extension_navigation_block_host_rule(
        &mut self,
        connection_id: u64,
        host: String,
    ) -> Result<(), String> {
        self.add_extension_navigation_rule(
            connection_id,
            ExtensionNavigationBlockRule::Host(canonical_navigation_block_host(&host)?),
        )
    }

    /// Adds a literal, segment-boundary path prefix under a canonical host.
    /// The rule shares the same per-connection quota and cleanup as v2/v4.
    pub(crate) fn add_extension_navigation_block_path_prefix_rule(
        &mut self,
        connection_id: u64,
        host: String,
        path_prefix: String,
    ) -> Result<(), String> {
        self.add_extension_navigation_rule(
            connection_id,
            ExtensionNavigationBlockRule::PathPrefix {
                host: canonical_navigation_block_host(&host)?,
                path_prefix: canonical_navigation_block_path_prefix(&path_prefix)?,
            },
        )
    }

    /// Rewrites one exact navigation request to a fixed URL on the same
    /// origin. Only core may register the validated rule, and a conflicting
    /// source mapping is rejected rather than resolved by hash iteration.
    pub(crate) fn add_extension_navigation_redirect_rule(
        &mut self,
        connection_id: u64,
        source_url: String,
        target_url: String,
    ) -> Result<(), String> {
        self.prune_stale_extension_navigation_rules();
        let source_url = canonical_http_navigation_url(&source_url)?;
        let target_url = canonical_http_navigation_url(&target_url)?;
        let source = Url::parse(&source_url).expect("a canonical URL must parse");
        let target = Url::parse(&target_url).expect("a canonical URL must parse");
        if source.origin() != target.origin() || source_url == target_url {
            return Err("navigation redirects must change the URL within one exact origin".to_string());
        }
        if self.extension_navigation_block_rules.values().flat_map(|(_, rules)| rules.iter()).any(|rule| {
            matches!(rule, ExtensionNavigationBlockRule::RedirectExactUrl {
                source_url: existing_source, target_url: existing_target,
            } if existing_source == &source_url && existing_target != &target_url)
        }) {
            return Err("a navigation redirect for this source URL already exists".to_string());
        }
        self.add_extension_navigation_rule(connection_id, ExtensionNavigationBlockRule::RedirectExactUrl {
            source_url, target_url,
        })
    }

    fn add_extension_navigation_rule(
        &mut self,
        connection_id: u64,
        rule: ExtensionNavigationBlockRule,
    ) -> Result<(), String> {
        let generation = self.intercept_generation()
            .ok_or_else(|| "network:intercept is not currently granted".to_string())?;
        self.prune_stale_extension_navigation_rules();
        let (stored_generation, rules) = self
            .extension_navigation_block_rules
            .entry(connection_id)
            .or_insert_with(|| (generation, HashSet::new()));
        if *stored_generation != generation {
            return Err("network:intercept grant changed while registering a rule".to_string());
        }
        if !rules.contains(&rule)
            && rules.len() >= MAX_EXTENSION_NAVIGATION_RULES_PER_CONNECTION
        {
            return Err(format!(
                "an extension connection may register at most {MAX_EXTENSION_NAVIGATION_RULES_PER_CONNECTION} navigation rules"
            ));
        }
        rules.insert(rule);
        Ok(())
    }

    /// Removes every declarative navigation block or rewrite rule owned by one extension
    /// connection. A disconnect always calls this, so a stale package cannot
    /// leave navigation policy behind after its host is gone.
    pub(crate) fn clear_extension_navigation_block_rules(&mut self, connection_id: u64) {
        self.extension_navigation_block_rules.remove(&connection_id);
    }

    /// Takes a per-navigation immutable rule set plus its live grant view.
    /// The background fetch worker receives no `TabManager` reference; the
    /// session thread remains the sole owner of live tab state.
    pub(crate) fn extension_navigation_block_rule_snapshot(&self) -> ExtensionNavigationRuleSnapshot {
        ExtensionNavigationRuleSnapshot {
            rules: self
                .extension_navigation_block_rules
                .values()
                .flat_map(|(generation, rules)| rules.iter().cloned().map(|rule| (*generation, rule)))
                .collect(),
            allowed_origins: self
                .extension_capability_origins
                .get("network:intercept")
                .cloned(),
            permission: self.extension_permissions.clone(),
        }
    }

    /// Tests the exact canonical initial navigation URL against every live
    /// connection's declarative block rules. Invalid/non-network URLs are not
    /// matches; their normal built-in/scheme validation paths still apply.
    #[cfg(test)]
    pub(crate) fn is_extension_navigation_blocked(&self, url: &str) -> bool {
        extension_navigation_rules_block_url(&self.extension_navigation_block_rule_snapshot(), url)
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
        page.set_gatekeeper_settings_source(self.gatekeeper_settings.clone());
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

    /// Where every tab's `about:settings` reaches the one real gatekeeper
    /// service. Applied to current tabs, retained snapshot pages, and tabs
    /// opened later just like the downloads source.
    pub fn set_gatekeeper_settings_source(&mut self, source: Arc<GatekeeperSettingsSource>) {
        for tab in self.tabs.values_mut() {
            tab.page
                .set_gatekeeper_settings_source(Some(source.clone()));
            for entry in tab.back.iter_mut().chain(tab.forward.iter_mut()) {
                if let Some(page) = entry.snapshot_mut() {
                    page.set_gatekeeper_settings_source(Some(source.clone()));
                }
            }
        }
        self.gatekeeper_settings = Some(source);
    }

    pub fn gatekeeper_settings_source(&self) -> Option<&Arc<GatekeeperSettingsSource>> {
        self.gatekeeper_settings.as_ref()
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
        trace: blueice_ipc::extension::NetworkTraceInfo,
    ) {
        let mut next = self.new_history_page(id);
        next.apply_fetched(clearance, url, html);
        next.set_network_response(trace.response.clone());
        next.set_network_trace(trace);
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
        trace: blueice_ipc::extension::NetworkTraceInfo,
    ) -> bool {
        let mut next = self.new_history_page(id);
        next.apply_fetched(clearance, url, html);
        next.set_network_response(trace.response.clone());
        next.set_network_trace(trace);
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
        page.set_gatekeeper_settings_source(self.gatekeeper_settings.clone());
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

/// Produces the sole representation used in declarative navigation rules.
/// Fragments never cross HTTP, so they cannot make a different network rule;
/// credentials are forbidden rather than retained in long-lived core state.
pub(crate) fn extension_navigation_rules_block_url(
    snapshot: &ExtensionNavigationRuleSnapshot,
    url: &str,
) -> bool {
    let active_generation = match snapshot.permission.as_ref() {
        Some(permission) => permission.intercept_generation(),
        None => Some(0),
    };
    let Some(active_generation) = active_generation else { return false };
    let Ok(mut parsed) = Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    if snapshot.allowed_origins.as_ref().is_some_and(|allowed| {
        !allowed.contains(&parsed.origin().ascii_serialization())
    }) {
        return false;
    }
    let Some(host) = parsed.host_str().map(|host| host.trim_end_matches('.').to_ascii_lowercase()) else {
        return false;
    };
    let has_credentials = !parsed.username().is_empty() || parsed.password().is_some();
    parsed.set_fragment(None);
    let canonical_url = parsed.to_string();
    snapshot.rules.iter().filter(|(generation, _)| *generation == active_generation).any(|(_, rule)| match rule {
        ExtensionNavigationBlockRule::ExactUrl(blocked) => !has_credentials && blocked == &canonical_url,
        ExtensionNavigationBlockRule::Host(blocked) => host_matches_block_rule(&host, blocked),
        ExtensionNavigationBlockRule::PathPrefix { host: blocked, path_prefix } => {
            host_matches_block_rule(&host, blocked)
                && (parsed.path() == path_prefix
                    || (path_prefix.ends_with('/') && parsed.path().starts_with(path_prefix))
                    || parsed.path().strip_prefix(path_prefix).is_some_and(|rest| rest.starts_with('/')))
        },
        ExtensionNavigationBlockRule::RedirectExactUrl { .. } => false,
    })
}

/// The only request-modification effect: one exact same-origin rewrite from
/// the immutable navigation-start snapshot. A blocked source takes precedence
/// in the caller, and both source and target receive URL review before any
/// connection to the target is opened.
pub(crate) fn extension_navigation_rules_redirect_url(
    snapshot: &ExtensionNavigationRuleSnapshot,
    url: &str,
) -> Option<String> {
    let active_generation = match snapshot.permission.as_ref() {
        Some(permission) => permission.intercept_generation(),
        None => Some(0),
    }?;
    let mut parsed = Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    if snapshot.allowed_origins.as_ref().is_some_and(|allowed| {
        !allowed.contains(&parsed.origin().ascii_serialization())
    }) {
        return None;
    }
    parsed.set_fragment(None);
    let canonical_url = parsed.to_string();
    snapshot.rules.iter().filter(|(generation, _)| *generation == active_generation).find_map(|(_, rule)| match rule {
        ExtensionNavigationBlockRule::RedirectExactUrl { source_url, target_url }
            if source_url == &canonical_url => Some(target_url.clone()),
        _ => None,
    })
}

fn host_matches_block_rule(host: &str, blocked: &str) -> bool {
    host == blocked
        || (blocked.parse::<std::net::Ipv4Addr>().is_err()
            && host.strip_suffix(blocked).is_some_and(|prefix| prefix.ends_with('.')))
}

fn canonical_navigation_block_path_prefix(input: &str) -> Result<String, String> {
    if input.is_empty() || input.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_PATH_BYTES
        || !input.starts_with('/')
        || !input.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~')
        })
        || input.split('/').skip(1).any(|segment| segment == "." || segment == "..")
        || input.contains("//")
    {
        return Err("navigation-block path prefixes must be literal ASCII paths of 1–512 bytes, without empty or dot segments, queries, fragments, or percent escapes".to_string());
    }
    Ok(input.to_string())
}

fn canonical_navigation_block_host(input: &str) -> Result<String, String> {
    if input.is_empty() || input.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_HOST_BYTES {
        return Err("navigation-block hosts must be 1–253 ASCII bytes".to_string());
    }
    let host = input.strip_suffix('.').unwrap_or(input);
    if host.split('.').any(|label| {
        label.is_empty()
            || label.len() > 63
            || !label.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
            || !label.as_bytes().last().is_some_and(u8::is_ascii_alphanumeric)
            || !label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }) {
        return Err("navigation-block hosts must use ASCII DNS labels without wildcards".to_string());
    }
    let canonical = host.to_ascii_lowercase();
    let parsed = Url::parse(&format!("http://{canonical}/"))
        .map_err(|_| "navigation-block host cannot be interpreted as a canonical HTTP host".to_string())?;
    if parsed.host_str() != Some(canonical.as_str()) {
        return Err("navigation-block host must already use canonical IPv4 or DNS spelling".to_string());
    }
    Ok(canonical)
}

fn canonical_http_navigation_url(input: &str) -> Result<String, String> {
    if input.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_URL_BYTES {
        return Err("navigation-rule URL exceeds the protocol limit".to_string());
    }
    let mut url =
        Url::parse(input).map_err(|error| format!("navigation-block URL is invalid: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err("navigation-block URLs must be absolute HTTP(S) URLs".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("navigation-block URLs must not contain credentials".to_string());
    }
    url.set_fragment(None);
    let canonical = url.to_string();
    if canonical.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_URL_BYTES {
        return Err("canonical navigation-rule URL exceeds the protocol limit".to_string());
    }
    Ok(canonical)
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
    fn extension_origin_scopes_follow_the_live_tab_url_and_default_to_legacy_global_grants() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab = tabs.default_tab();
        assert!(tabs.check_extension_origin("dom:read", tab).is_ok());
        tabs.set_extension_capability_origins(BTreeMap::from([
            ("dom:read".to_string(), BTreeSet::from(["https://example.test".to_string()])),
            ("dom:write".to_string(), BTreeSet::from(["http://127.0.0.1:4312".to_string()])),
        ]));
        assert!(tabs.check_extension_origin("dom:read", tab).is_err());
        tabs.get_mut(tab).unwrap().load_html_str("<p>one</p>", Some("https://example.test/path?query=1".to_string()));
        assert!(tabs.check_extension_origin("dom:read", tab).is_ok());
        assert!(tabs.check_extension_origin("dom:write", tab).is_err());
        assert!(tabs.check_extension_origin("network:observe", tab).is_ok());
        tabs.get_mut(tab).unwrap().load_html_str("<p>two</p>", Some("http://127.0.0.1:4312/page".to_string()));
        assert!(tabs.check_extension_origin("dom:read", tab).is_err());
        assert!(tabs.check_extension_origin("dom:write", tab).is_ok());
        tabs.get_mut(tab).unwrap().navigate("about:blank").unwrap();
        assert!(tabs.check_extension_origin("dom:write", tab).is_err());
        assert!(tabs.check_extension_origin("dom:write", TabId::from_u64(999)).is_err());
    }

    #[test]
    fn scoped_network_intercept_rules_cannot_block_other_origins_even_when_host_matches() {
        let mut tabs = TabManager::new(320.0, 200.0);
        tabs.add_extension_navigation_block_host_rule(7, "127.0.0.1".to_string())
            .unwrap();
        assert!(tabs.is_extension_navigation_blocked("http://127.0.0.1:4312/page"));
        assert!(tabs.is_extension_navigation_blocked("http://127.0.0.1:4313/page"));

        tabs.set_extension_capability_origins(BTreeMap::from([(
            "network:intercept".to_string(),
            BTreeSet::from(["http://127.0.0.1:4312".to_string()]),
        )]));
        let snapshot = tabs.extension_navigation_block_rule_snapshot();
        assert!(extension_navigation_rules_block_url(
            &snapshot,
            "http://127.0.0.1:4312/page"
        ));
        assert!(!extension_navigation_rules_block_url(
            &snapshot,
            "http://127.0.0.1:4313/page"
        ));
        assert!(!extension_navigation_rules_block_url(
            &snapshot,
            "https://127.0.0.1:4312/page"
        ));
        assert!(!extension_navigation_rules_block_url(&snapshot, "about:settings"));
    }

    #[test]
    fn optional_intercept_revocation_invalidates_captured_rules_and_regrant_does_not_resurrect_them() {
        let root = std::env::temp_dir().join(format!(
            "blueice-optional-network-generation-{}", std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let manifest = root.join("extension.json");
        std::fs::write(&manifest,
            r#"{"name":"Optional network","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["network:intercept"]}}"#
        ).unwrap();
        std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
        let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
        let id = installed.extension_id().to_string();
        let registry = Arc::new(blueice_extension_host::registry_for_installed_extension(&installed));
        let mut tabs = TabManager::new(320.0, 200.0);
        tabs.set_extension_permission_registry(Arc::clone(&registry), id.clone());
        assert!(tabs.add_extension_navigation_block_host_rule(7, "old.example.test".into()).is_err());

        assert!(registry.grant_optional(&id, "network:intercept").unwrap());
        tabs.add_extension_navigation_block_host_rule(7, "old.example.test".into()).unwrap();
        tabs.add_extension_navigation_redirect_rule(7,
            "https://example.test/old".into(), "https://example.test/new".into()).unwrap();
        let captured = tabs.extension_navigation_block_rule_snapshot();
        assert!(extension_navigation_rules_block_url(&captured, "https://old.example.test/page"));
        assert_eq!(extension_navigation_rules_redirect_url(&captured, "https://example.test/old"),
            Some("https://example.test/new".into()));

        assert!(registry.revoke_optional(&id, "network:intercept").unwrap());
        assert!(!extension_navigation_rules_block_url(&captured, "https://old.example.test/page"));
        assert_eq!(extension_navigation_rules_redirect_url(&captured, "https://example.test/old"), None);
        assert_eq!(tabs.extension_navigation_block_rules.len(), 1,
            "revocation invalidates the captured rules before session cleanup");
        tabs.prune_stale_extension_navigation_rules();
        assert!(tabs.extension_navigation_block_rules.is_empty(),
            "the next session tick must physically remove revoked rules");
        assert!(registry.grant_optional(&id, "network:intercept").unwrap());
        assert!(!extension_navigation_rules_block_url(&captured, "https://old.example.test/page"));
        assert_eq!(extension_navigation_rules_redirect_url(&captured, "https://example.test/old"), None);

        assert!(tabs.with_stable_extension_capability("network:intercept", 0, |tabs| {
            tabs.add_extension_navigation_block_host_rule(7, "stale.example.test".into())
        }).is_err(), "a queued registration cannot borrow the new grant");
        let fresh_generation = registry.capability_generation(&id, "network:intercept").unwrap();
        tabs.with_stable_extension_capability("network:intercept", fresh_generation, |tabs| {
            tabs.add_extension_navigation_block_host_rule(7, "new.example.test".into())
        }).unwrap().unwrap();
        let renewed = tabs.extension_navigation_block_rule_snapshot();
        assert!(!extension_navigation_rules_block_url(&renewed, "https://old.example.test/page"));
        assert!(!extension_navigation_rules_block_url(&renewed, "https://stale.example.test/page"));
        assert!(extension_navigation_rules_block_url(&renewed, "https://new.example.test/page"));
        assert_eq!(extension_navigation_rules_redirect_url(&renewed, "https://example.test/old"), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn extension_navigation_block_rules_match_canonical_urls_and_clear_per_connection() {
        let mut tabs = TabManager::new(320.0, 200.0);
        tabs.add_extension_navigation_block_rule(
            41,
            "HTTPS://EXAMPLE.test/private#client-fragment".to_string(),
        )
        .unwrap();
        assert!(tabs.is_extension_navigation_blocked("https://example.test/private"));
        assert!(tabs.is_extension_navigation_blocked("https://example.test/private#another"));
        assert!(!tabs.is_extension_navigation_blocked("https://example.test/other"));

        tabs.clear_extension_navigation_block_rules(40);
        assert!(tabs.is_extension_navigation_blocked("https://example.test/private"));
        tabs.clear_extension_navigation_block_rules(41);
        assert!(!tabs.is_extension_navigation_blocked("https://example.test/private"));
    }

    #[test]
    fn extension_navigation_block_rules_reject_non_http_and_credentialed_urls() {
        let mut tabs = TabManager::new(320.0, 200.0);
        assert!(tabs
            .add_extension_navigation_block_rule(1, "about:blank".to_string())
            .is_err());
        assert!(tabs
            .add_extension_navigation_block_rule(1, "https://user:secret@example.test/".to_string())
            .is_err());
        assert!(!tabs.is_extension_navigation_blocked("about:blank"));
    }

    #[test]
    fn exact_navigation_redirects_are_same_origin_scoped_conflict_free_and_connection_owned() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let source = "https://example.test/old?x=1";
        let target = "https://example.test/new?x=1";
        tabs.add_extension_navigation_redirect_rule(41, source.into(), target.into()).unwrap();
        let snapshot = tabs.extension_navigation_block_rule_snapshot();
        assert_eq!(extension_navigation_rules_redirect_url(&snapshot, source), Some(target.into()));
        assert_eq!(extension_navigation_rules_redirect_url(&snapshot, "https://example.test/other"), None);
        assert_eq!(extension_navigation_rules_redirect_url(&snapshot, "https://user:secret@example.test/old?x=1"), None);
        assert!(tabs.add_extension_navigation_redirect_rule(42, source.into(), "https://example.test/other".into()).is_err());
        assert!(tabs.add_extension_navigation_redirect_rule(41, source.into(), "https://outside.test/new".into()).is_err());
        assert!(tabs.add_extension_navigation_redirect_rule(41, source.into(), source.into()).is_err());
        assert!(tabs.add_extension_navigation_redirect_rule(41, "about:blank".into(), target.into()).is_err());
        assert!(tabs.add_extension_navigation_redirect_rule(
            41, source.into(), format!("https://example.test/{}", "x".repeat(2048)),
        ).is_err());
        tabs.set_extension_capability_origins(BTreeMap::from([(
            "network:intercept".to_string(), BTreeSet::from(["https://other.test".to_string()]),
        )]));
        assert_eq!(extension_navigation_rules_redirect_url(&tabs.extension_navigation_block_rule_snapshot(), source), None);
        tabs.clear_extension_navigation_block_rules(42);
        tabs.set_extension_capability_origins(BTreeMap::new());
        assert_eq!(extension_navigation_rules_redirect_url(&tabs.extension_navigation_block_rule_snapshot(), source), Some(target.into()));
        tabs.clear_extension_navigation_block_rules(41);
        assert_eq!(extension_navigation_rules_redirect_url(&tabs.extension_navigation_block_rule_snapshot(), source), None);
    }

    #[test]
    fn extension_host_rules_match_exact_hosts_and_subdomains_but_not_suffix_tricks() {
        let mut tabs = TabManager::new(320.0, 200.0);
        tabs.add_extension_navigation_block_host_rule(41, "EXAMPLE.test.".to_string())
            .unwrap();
        assert!(tabs.is_extension_navigation_blocked("https://example.test/path"));
        assert!(tabs.is_extension_navigation_blocked("http://sub.example.test/other"));
        assert!(!tabs.is_extension_navigation_blocked("https://notexample.test/"));
        assert!(!tabs.is_extension_navigation_blocked("https://example.test.evil/"));
        assert!(!tabs.is_extension_navigation_blocked("about:blank"));
        tabs.add_extension_navigation_block_host_rule(41, "127.0.0.1".to_string())
            .unwrap();
        assert!(tabs.is_extension_navigation_blocked("http://127.0.0.1/"));
        assert!(!tabs.is_extension_navigation_blocked("http://sub.127.0.0.1/"));
        tabs.clear_extension_navigation_block_rules(40);
        assert!(tabs.is_extension_navigation_blocked("https://example.test/"));
        tabs.clear_extension_navigation_block_rules(41);
        assert!(!tabs.is_extension_navigation_blocked("https://example.test/"));
    }

    #[test]
    fn extension_host_rules_reject_urls_wildcards_unicode_and_invalid_dns_labels() {
        let mut tabs = TabManager::new(320.0, 200.0);
        for host in [
            "https://example.test",
            "*.example.test",
            "example..test",
            "-example.test",
            "example-.test",
            "ex ample.test",
            "exämple.test",
            "example.test:443",
            "127.000.000.001",
        ] {
            assert!(
                tabs.add_extension_navigation_block_host_rule(7, host.to_string())
                    .is_err(),
                "accepted invalid host {host:?}"
            );
        }
    }

    #[test]
    fn extension_path_prefix_rules_use_host_and_path_segment_boundaries() {
        let mut tabs = TabManager::new(320.0, 200.0);
        tabs.add_extension_navigation_block_path_prefix_rule(
            41, "EXAMPLE.test.".into(), "/private".into(),
        ).unwrap();
        for url in [
            "https://example.test/private",
            "https://sub.example.test/private/report?download=1#section",
        ] {
            assert!(tabs.is_extension_navigation_blocked(url), "missed {url}");
        }
        for url in [
            "https://example.test/privateer",
            "https://notexample.test/private",
            "https://example.test.evil/private",
            "https://example.test/public/private",
            "https://example.test/%70rivate",
        ] {
            assert!(!tabs.is_extension_navigation_blocked(url), "overmatched {url}");
        }
        tabs.clear_extension_navigation_block_rules(41);
        assert!(!tabs.is_extension_navigation_blocked("https://example.test/private"));
        tabs.add_extension_navigation_block_path_prefix_rule(
            42, "127.0.0.1".into(), "/private".into(),
        ).unwrap();
        assert!(tabs.is_extension_navigation_blocked("http://127.0.0.1/private/child"));
        assert!(!tabs.is_extension_navigation_blocked("http://sub.127.0.0.1/private/child"));
    }

    #[test]
    fn extension_path_prefix_rules_reject_ambiguous_paths_and_share_the_quota() {
        let mut tabs = TabManager::new(320.0, 200.0);
        for path in ["", "private", "/a//b", "/./x", "/../x", "/a?x=1", "/a#frag", "/a%2fb", "/é"] {
            assert!(tabs.add_extension_navigation_block_path_prefix_rule(
                7, "example.test".into(), path.into(),
            ).is_err(), "accepted {path:?}");
        }
        assert!(tabs.add_extension_navigation_block_path_prefix_rule(
            7, "example.test".into(), format!("/{}", "x".repeat(512)),
        ).is_err());
        for index in 0..MAX_EXTENSION_NAVIGATION_RULES_PER_CONNECTION {
            tabs.add_extension_navigation_block_path_prefix_rule(
                7, "example.test".into(), format!("/private/{index}"),
            ).unwrap();
        }
        assert!(tabs.add_extension_navigation_block_host_rule(7, "overflow.test".into()).is_err());
    }

    #[test]
    fn extension_url_and_host_rules_share_a_quota_and_clear_together() {
        let mut tabs = TabManager::new(320.0, 200.0);
        tabs.add_extension_navigation_block_rule(7, "https://exact.example/".to_string())
            .unwrap();
        for index in 1..MAX_EXTENSION_NAVIGATION_RULES_PER_CONNECTION {
            tabs.add_extension_navigation_block_host_rule(7, format!("host{index}.example"))
                .unwrap();
        }
        assert!(tabs.add_extension_navigation_block_host_rule(7, "overflow.example".to_string()).is_err());
        assert!(tabs.is_extension_navigation_blocked("https://exact.example/"));
        assert!(tabs.is_extension_navigation_blocked("https://host1.example/"));
        tabs.clear_extension_navigation_block_rules(7);
        assert!(!tabs.is_extension_navigation_blocked("https://exact.example/"));
        assert!(!tabs.is_extension_navigation_blocked("https://host1.example/"));
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
