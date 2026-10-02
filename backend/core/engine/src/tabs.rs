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

use crate::assistant_page::AssistantPanel;
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
    PathPrefix {
        host: String,
        path_prefix: String,
    },
    RedirectExactUrl {
        source_url: String,
        target_url: String,
    },
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
        self.registry
            .capability_generation(&self.extension_id, "network:intercept")
    }
}

impl fmt::Debug for ExtensionPermissionView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionPermissionView")
            .field("extension_id", &self.extension_id)
            .finish_non_exhaustive()
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
    Post {
        url: String,
        navigation: Option<crate::navigation_request::BrowserNavigation>,
    },
}

/// One item in a tab's history. A `Page` is only retained when snapshot mode
/// was explicitly enabled; the default path stores just a URL and therefore
/// cannot accidentally turn a history traversal into an offline cache.
enum HistoryEntry {
    Reload {
        url: Option<String>,
        navigation: Option<crate::navigation_request::BrowserNavigation>,
        was_post: bool,
    },
    Snapshot(Box<Page>),
}

impl HistoryEntry {
    fn post_request(&self) -> Option<&crate::navigation_request::BrowserNavigation> {
        match self {
            Self::Reload { navigation, .. } => navigation.as_ref(),
            Self::Snapshot(page) => page
                .last_navigation
                .as_ref()
                .filter(|nav| nav.request.method() == "POST"),
        }
    }
    fn forget_post(&mut self) {
        match self {
            Self::Reload { navigation, .. } => *navigation = None,
            Self::Snapshot(page) => {
                page.last_navigation = None;
                page.post_expired = true;
            }
        }
    }
}

impl HistoryEntry {
    fn from_page(page: Page, mode: HistorySnapshotMode) -> Self {
        match mode {
            HistorySnapshotMode::Reload => HistoryEntry::Reload {
                url: page.url().map(str::to_string),
                was_post: page.post_expired
                    || page
                        .last_navigation
                        .as_ref()
                        .is_some_and(|nav| nav.request.method() == "POST"),
                navigation: page
                    .last_navigation
                    .filter(|nav| nav.request.method() == "POST"),
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
    /// Changes on every committed document replacement, never on repaint.
    /// A gesture-bound lease can capture it to reject navigation/reload.
    document_epoch: u64,
    back: Vec<HistoryEntry>,
    forward: Vec<HistoryEntry>,
    group_id: Option<GroupId>,
}

impl Tab {
    fn bound_post_history(&mut self) {
        let mut bytes = self
            .page
            .last_navigation
            .as_ref()
            .map_or(0, |nav| nav.request.body().len());
        let mut count = usize::from(
            self.page
                .last_navigation
                .as_ref()
                .is_some_and(|nav| nav.request.method() == "POST"),
        );
        for entry in self
            .back
            .iter_mut()
            .rev()
            .chain(self.forward.iter_mut().rev())
        {
            if let Some(nav) = entry.post_request() {
                bytes = bytes.saturating_add(nav.request.body().len());
                count += 1;
                if count > 8 || bytes > 8 * blueice_net::MAX_FORM_BODY_BYTES {
                    entry.forget_post();
                }
            }
        }
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
    display_viewport: Option<blueice_ipc::viewport::DisplayViewport>,
    display_preferences: Option<blueice_ipc::display::DisplayPreferences>,
    /// Handed to every tab (existing and future) so `about:downloads` works
    /// in any of them.
    downloads: Option<Arc<DownloadsSource>>,
    gatekeeper_settings: Option<Arc<GatekeeperSettingsSource>>,
    /// The results `about:assistant` shows, shared by every tab. Always
    /// present; whether an assistant exists is the panel's own flag.
    assistant_panel: Arc<AssistantPanel>,
    /// Whether leaving an entry retains a locally displayable page snapshot,
    /// rather than the default URL-only history record.
    history_snapshot_mode: HistorySnapshotMode,
    /// Canonical HTTP(S) URL and ASCII host rules, keyed by an opaque
    /// core-allocated connection ID. Rules disappear on disconnect.
    extension_navigation_block_rules: HashMap<u64, (u64, HashSet<ExtensionNavigationBlockRule>)>,
    /// The assistant live translation may use and its per-navigation budget
    /// (`phase-7-local-ai/PLAN.md`). Fixed by `blueice-core`'s startup flags;
    /// no client message can change it.
    translation_endpoint: Option<(std::path::PathBuf, std::time::Duration)>,
    /// The target language for pages fetched from now on; `None` leaves every
    /// page as the site wrote it. Read when a navigation starts, so a later
    /// change never alters a navigation already in flight.
    translation_language: Option<String>,
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
        let assistant_panel = Arc::new(AssistantPanel::new());
        let mut first_page = Page::new(viewport_width, viewport_height);
        first_page.set_assistant_panel(Some(assistant_panel.clone()));
        let mut tabs = HashMap::new();
        tabs.insert(
            default_tab,
            Tab {
                page: first_page,
                document_epoch: 0,
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
            display_viewport: None,
            display_preferences: None,
            downloads: None,
            gatekeeper_settings: None,
            assistant_panel,
            history_snapshot_mode,
            extension_navigation_block_rules: HashMap::new(),
            translation_endpoint: None,
            translation_language: None,
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
        self.extension_permissions = Some(ExtensionPermissionView {
            registry,
            extension_id,
        });
        self.prune_stale_extension_navigation_rules();
    }

    fn intercept_generation(&self) -> Option<u64> {
        self.extension_capability_generation("network:intercept")
    }

    pub(crate) fn extension_capability_generation(&self, capability: &str) -> Option<u64> {
        self.extension_permissions.as_ref().map_or(Some(0), |view| {
            view.registry
                .capability_generation(&view.extension_id, capability)
        })
    }

    /// The session owner calls this at the actual read, after resolving the
    /// live tab and origin. A ticket captured by the extension worker when
    /// its request arrived cannot borrow a later native gesture, and a
    /// same-URL reload has a different document epoch.
    pub(crate) fn consume_extension_runtime_ephemeral(
        &self,
        capability: &str,
        tab_id: TabId,
        expected_ticket: &str,
    ) -> Result<(), String> {
        let view = self
            .extension_permissions
            .as_ref()
            .ok_or_else(|| "no installed extension can hold an ephemeral lease".to_string())?;
        let epoch = self
            .document_epoch(tab_id)
            .ok_or_else(|| "the requested tab is not live".to_string())?;
        view.registry
            .consume_runtime_ephemeral(
                &view.extension_id,
                capability,
                expected_ticket,
                tab_id.as_u64(),
                epoch,
            )
            .then_some(())
            .ok_or_else(|| {
                "the runtime-ephemeral lease is absent, spent, or bound to another document"
                    .to_string()
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
                &view.extension_id,
                capability,
                expected_generation,
                || effect(self),
            ),
            None if expected_generation == 0 => Ok(effect(self)),
            None => Err(format!("{capability} grant generation is unavailable")),
        }
    }

    pub(crate) fn prune_stale_extension_navigation_rules(&mut self) {
        let current = self.intercept_generation();
        self.extension_navigation_block_rules
            .retain(|_, (generation, _)| current == Some(*generation));
    }

    #[cfg(all(test, unix))]
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
        if origin
            .as_ref()
            .is_some_and(|origin| allowed.contains(origin))
        {
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
            return Err(
                "navigation redirects must change the URL within one exact origin".to_string(),
            );
        }
        if self
            .extension_navigation_block_rules
            .values()
            .flat_map(|(_, rules)| rules.iter())
            .any(|rule| {
                matches!(rule, ExtensionNavigationBlockRule::RedirectExactUrl {
                source_url: existing_source, target_url: existing_target,
            } if existing_source == &source_url && existing_target != &target_url)
            })
        {
            return Err("a navigation redirect for this source URL already exists".to_string());
        }
        self.add_extension_navigation_rule(
            connection_id,
            ExtensionNavigationBlockRule::RedirectExactUrl {
                source_url,
                target_url,
            },
        )
    }

    fn add_extension_navigation_rule(
        &mut self,
        connection_id: u64,
        rule: ExtensionNavigationBlockRule,
    ) -> Result<(), String> {
        let generation = self
            .intercept_generation()
            .ok_or_else(|| "network:intercept is not currently granted".to_string())?;
        self.prune_stale_extension_navigation_rules();
        let (stored_generation, rules) = self
            .extension_navigation_block_rules
            .entry(connection_id)
            .or_insert_with(|| (generation, HashSet::new()));
        if *stored_generation != generation {
            return Err("network:intercept grant changed while registering a rule".to_string());
        }
        if !rules.contains(&rule) && rules.len() >= MAX_EXTENSION_NAVIGATION_RULES_PER_CONNECTION {
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
    pub(crate) fn extension_navigation_block_rule_snapshot(
        &self,
    ) -> ExtensionNavigationRuleSnapshot {
        ExtensionNavigationRuleSnapshot {
            rules: self
                .extension_navigation_block_rules
                .values()
                .flat_map(|(generation, rules)| {
                    rules.iter().cloned().map(|rule| (*generation, rule))
                })
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
        if let Some(display) = self.display_viewport {
            page.configure_display(display);
        }
        if let Some(preferences) = self.display_preferences {
            page.set_display_preferences(preferences);
        }
        page.set_downloads_source(self.downloads.clone());
        page.set_gatekeeper_settings_source(self.gatekeeper_settings.clone());
        page.set_assistant_panel(Some(self.assistant_panel.clone()));
        self.tabs.insert(
            id,
            Tab {
                page,
                document_epoch: 0,
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

    /// Monotonic live-document identity within this tab and core generation.
    /// Reloading the same URL and restoring a saved history snapshot advance
    /// it; closing the tab makes it unavailable. Unlike frame generation,
    /// hover, resize, and other repaints do not change this identity.
    pub fn document_epoch(&self, id: TabId) -> Option<u64> {
        self.tabs.get(&id).map(|tab| tab.document_epoch)
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
            HistoryEntry::Reload {
                url,
                navigation,
                was_post: true,
            } => Some(HistoryDestination::Post {
                url: url.clone().unwrap_or_default(),
                navigation: navigation.clone(),
            }),
            HistoryEntry::Reload { url, .. } => Some(HistoryDestination::Reload(url.clone())),
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
        let HistoryEntry::Snapshot(mut next) = target else {
            unreachable!("the checked history entry is a snapshot");
        };
        // Generations belong to the tab's publication/realm lifetime, not to
        // the saved page. Restoring older counters would make the frontend
        // discard the frame and could reuse a previous document identity.
        next.continue_tab_generations_from(&tab.page);
        let current = std::mem::replace(&mut tab.page, *next);
        tab.document_epoch = tab
            .document_epoch
            .checked_add(1)
            .expect("document identity exhausted; core must fail closed");
        let departure = HistoryEntry::from_page(current, mode);
        match direction {
            HistoryDirection::Back => tab.forward.push(departure),
            HistoryDirection::Forward => tab.back.push(departure),
        }
        tab.bound_post_history();
        true
    }

    /// Declares which assistant serves live translation. Called once at
    /// startup from `blueice-core`'s own flags; without it translation is
    /// unavailable and [`Self::set_translation_language`] has no effect.
    pub fn set_translation_endpoint(
        &mut self,
        socket: std::path::PathBuf,
        deadline: std::time::Duration,
    ) {
        self.translation_endpoint = Some((socket, deadline));
        self.assistant_panel.set_available(true);
    }

    /// The assistant a summarize or organize task talks to: the same startup-fixed
    /// socket translation uses, `None` when core has no assistant.
    pub fn assistant_socket(&self) -> Option<std::path::PathBuf> {
        self.translation_endpoint
            .as_ref()
            .map(|(socket, _)| socket.clone())
    }

    /// The shared results behind every tab's `about:assistant`.
    pub fn assistant_panel(&self) -> &Arc<AssistantPanel> {
        &self.assistant_panel
    }

    /// Re-renders every tab currently showing `about:assistant` from the
    /// panel's state and returns those tabs, so the caller can send each a
    /// fresh frame.
    pub(crate) fn refresh_assistant_panels(&mut self) -> Vec<TabId> {
        let ids: Vec<TabId> = self.order.clone();
        ids.into_iter()
            .filter(|id| {
                self.tabs
                    .get_mut(id)
                    .is_some_and(|tab| tab.page.refresh_assistant_panel())
            })
            .collect()
    }

    /// Whether core was started with an assistant for translation.
    pub fn translation_available(&self) -> bool {
        self.translation_endpoint.is_some()
    }

    /// Sets the target language for later navigations (`None` = off).
    /// Returns `false`, changing nothing, when translation is unavailable.
    pub fn set_translation_language(&mut self, language: Option<String>) -> bool {
        if self.translation_endpoint.is_none() {
            return false;
        }
        self.translation_language = language;
        true
    }

    pub fn translation_language(&self) -> Option<&str> {
        self.translation_language.as_deref()
    }

    /// The live-translation settings a navigation started now would use:
    /// `None` unless an assistant is configured *and* a language is chosen.
    pub fn translation_config(&self) -> Option<crate::assistant_client::AssistantConfig> {
        let (socket, deadline) = self.translation_endpoint.as_ref()?;
        Some(crate::assistant_client::AssistantConfig {
            socket: socket.clone(),
            target_language: self.translation_language.clone()?,
            deadline: *deadline,
        })
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
        translations: Option<&[String]>,
        trace: blueice_ipc::extension::NetworkTraceInfo,
    ) {
        let mut next = self.new_history_page(id);
        next.apply_fetched_translated(clearance, url, html, translations);
        next.set_network_response(trace.response.clone());
        next.set_network_trace(trace);
        self.replace_current_as_new_navigation(id, next);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_fetched_reload(
        &mut self,
        id: TabId,
        clearance: crate::gatekeeper_client::GatekeeperClearance,
        url: &str,
        html: &str,
        translations: Option<&[String]>,
        trace: blueice_ipc::extension::NetworkTraceInfo,
    ) {
        let mut next = self.new_history_page(id);
        next.apply_fetched_translated(clearance, url, html, translations);
        next.set_network_response(trace.response.clone());
        next.set_network_trace(trace);
        let tab = self.tabs.get_mut(&id).expect("validated tab");
        next.continue_tab_generations_from(&tab.page);
        tab.page = next;
        tab.document_epoch = tab
            .document_epoch
            .checked_add(1)
            .expect("document identity exhausted");
    }

    pub(crate) fn remember_navigation(
        &mut self,
        id: TabId,
        navigation: crate::navigation_request::BrowserNavigation,
    ) {
        let tab = self.tabs.get_mut(&id).expect("committed tab");
        tab.page.last_navigation = Some(navigation);
        tab.bound_post_history();
    }
    pub(crate) fn reload_built_in(&mut self, id: TabId, url: &str) -> bool {
        let mut next = self.new_history_page(id);
        if !next.load_built_in(url) {
            return false;
        }
        let tab = self.tabs.get_mut(&id).expect("live tab");
        next.continue_tab_generations_from(&tab.page);
        tab.page = next;
        tab.document_epoch = tab
            .document_epoch
            .checked_add(1)
            .expect("document identity exhausted");
        true
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
    #[allow(clippy::too_many_arguments)] // mirrors `apply_fetched_navigation` plus the history direction
    pub(crate) fn apply_fetched_history_navigation(
        &mut self,
        id: TabId,
        direction: HistoryDirection,
        clearance: crate::gatekeeper_client::GatekeeperClearance,
        url: &str,
        html: &str,
        translations: Option<&[String]>,
        trace: blueice_ipc::extension::NetworkTraceInfo,
    ) -> bool {
        let mut next = self.new_history_page(id);
        next.apply_fetched_translated(clearance, url, html, translations);
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
        if let Some(display) = tab.page.display_viewport {
            page.configure_display(display);
        }
        if tab.page.page_zoom != 1.0 {
            page.set_page_zoom(tab.page.page_zoom);
        }
        if let Some(preferences) = tab.page.display_preferences {
            page.set_display_preferences(preferences);
        }
        page.set_downloads_source(self.downloads.clone());
        page.set_gatekeeper_settings_source(self.gatekeeper_settings.clone());
        page.set_assistant_panel(Some(self.assistant_panel.clone()));
        page
    }

    /// Replacing the current document is the one operation that creates a
    /// new branch in a tab's history. Any forward entries are deliberately
    /// discarded, just as a browser does after navigating from a page reached
    /// via Back.
    fn replace_current_as_new_navigation(&mut self, id: TabId, mut next: Page) {
        let mode = self.history_snapshot_mode;
        let tab = self
            .tabs
            .get_mut(&id)
            .expect("callers validate a tab before committing navigation");
        next.continue_tab_generations_from(&tab.page);
        let previous = std::mem::replace(&mut tab.page, next);
        tab.document_epoch = tab
            .document_epoch
            .checked_add(1)
            .expect("document identity exhausted; core must fail closed");
        tab.back.push(HistoryEntry::from_page(previous, mode));
        tab.forward.clear();
        tab.bound_post_history();
    }

    /// Replaces the live page with an already-loaded history destination. The
    /// target is removed from one stack and the departing page is appended to
    /// the other, preserving the normal Back/Forward cursor shape.
    fn replace_current_from_history(
        &mut self,
        id: TabId,
        direction: HistoryDirection,
        mut next: Page,
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
        next.continue_tab_generations_from(&tab.page);
        let current = std::mem::replace(&mut tab.page, next);
        tab.document_epoch = tab
            .document_epoch
            .checked_add(1)
            .expect("document identity exhausted; core must fail closed");
        let departure = HistoryEntry::from_page(current, mode);
        match direction {
            HistoryDirection::Back => tab.forward.push(departure),
            HistoryDirection::Forward => tab.back.push(departure),
        }
        tab.bound_post_history();
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
        if self.display_viewport.is_some() {
            self.configure_viewport_all(blueice_ipc::viewport::DisplayViewport {
                width,
                height,
                device_scale: 1.0,
                backing_scale: None,
            });
            return;
        }
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

    pub(crate) fn configure_viewport_all(
        &mut self,
        display: blueice_ipc::viewport::DisplayViewport,
    ) {
        self.set_window_size(display.width, display.height);
        self.display_viewport = Some(display);
        for tab in self.tabs.values_mut() {
            tab.page.configure_display(display);
            for entry in tab.back.iter_mut().chain(tab.forward.iter_mut()) {
                if let Some(page) = entry.snapshot_mut() {
                    page.configure_display(display);
                }
            }
        }
    }

    pub(crate) fn set_page_zoom(&mut self, id: TabId, zoom: f64) {
        let tab = self.tabs.get_mut(&id).expect("validated zoom tab");
        tab.page.set_page_zoom(zoom);
        for entry in tab.back.iter_mut().chain(tab.forward.iter_mut()) {
            if let Some(page) = entry.snapshot_mut() {
                page.set_page_zoom(zoom);
            }
        }
    }

    pub(crate) fn set_display_preferences_all(
        &mut self,
        preferences: blueice_ipc::display::DisplayPreferences,
    ) {
        self.display_preferences = Some(preferences);
        for tab in self.tabs.values_mut() {
            tab.page.set_display_preferences(preferences);
            for entry in tab.back.iter_mut().chain(tab.forward.iter_mut()) {
                if let Some(page) = entry.snapshot_mut() {
                    page.set_display_preferences(preferences);
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
    let Some(active_generation) = active_generation else {
        return false;
    };
    let Ok(mut parsed) = Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    if snapshot
        .allowed_origins
        .as_ref()
        .is_some_and(|allowed| !allowed.contains(&parsed.origin().ascii_serialization()))
    {
        return false;
    }
    let Some(host) = parsed
        .host_str()
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
    else {
        return false;
    };
    let has_credentials = !parsed.username().is_empty() || parsed.password().is_some();
    parsed.set_fragment(None);
    let canonical_url = parsed.to_string();
    snapshot
        .rules
        .iter()
        .filter(|(generation, _)| *generation == active_generation)
        .any(|(_, rule)| match rule {
            ExtensionNavigationBlockRule::ExactUrl(blocked) => {
                !has_credentials && blocked == &canonical_url
            }
            ExtensionNavigationBlockRule::Host(blocked) => host_matches_block_rule(&host, blocked),
            ExtensionNavigationBlockRule::PathPrefix {
                host: blocked,
                path_prefix,
            } => {
                host_matches_block_rule(&host, blocked)
                    && (parsed.path() == path_prefix
                        || (path_prefix.ends_with('/') && parsed.path().starts_with(path_prefix))
                        || parsed
                            .path()
                            .strip_prefix(path_prefix)
                            .is_some_and(|rest| rest.starts_with('/')))
            }
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
    if snapshot
        .allowed_origins
        .as_ref()
        .is_some_and(|allowed| !allowed.contains(&parsed.origin().ascii_serialization()))
    {
        return None;
    }
    parsed.set_fragment(None);
    let canonical_url = parsed.to_string();
    snapshot
        .rules
        .iter()
        .filter(|(generation, _)| *generation == active_generation)
        .find_map(|(_, rule)| match rule {
            ExtensionNavigationBlockRule::RedirectExactUrl {
                source_url,
                target_url,
            } if source_url == &canonical_url => Some(target_url.clone()),
            _ => None,
        })
}

mod navigation_rules;
use navigation_rules::*;

#[cfg(test)]
mod tests;
