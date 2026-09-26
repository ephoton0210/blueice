// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit core-owned execution of standard JavaScript page declarations.
//!
//! This module is a deliberately narrow page-host slice. It uses BlueJS's
//! public page runtime for one bounded realm per tab/document, but it grants
//! neither DOM objects nor IPC, network, filesystem, URL-resolution, or
//! ambient host capabilities to page code. Inline sources receive a
//! core-minted identity. An external `src` is rejected unless a core-owned
//! authorizer supplies a complete canonical module graph and every static
//! resolution record. The default core session does not construct this type.

use super::{
    contracts::{core_script_binding_contract, CoreScriptBindingContractLimits},
    direct_page::DirectPageScriptKind,
    BlueJsPageScriptDeclaration, BlueJsPageScriptKind, ScriptRequestReceiver,
};
use crate::{Page, TabId, TabManager};
use blueice_bluejs::{
    parse, parse_module, BlueJsPageDebuggerExecutionState, BlueJsPageOrigin, BlueJsPageRuntime,
    BlueJsPageRuntimeConfig, BlueJsPageRuntimeError, BlueJsProgramHandle, BlueJsProgramV1,
    BlueJsSourceIdentity, CompileError, HostFunctionError, HostValue, ParseError, RuntimeError,
    Value,
};
use blueice_ipc::compiler::CompilerContractValue;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::{fmt, io};

mod debugger_support;
mod execution_support;
use debugger_support::{
    DebuggerBreakpointRecord, DebuggerExecutionStatus, DebuggerProgramKey, DebuggerProgramRecord,
    PendingDebuggerExecution,
};
pub use debugger_support::{
    JavaScriptPageDebuggerBreakpoint, JavaScriptPageDebuggerError,
    JavaScriptPageDebuggerExceptionLocation, JavaScriptPageDebuggerExceptionLocationTarget,
    JavaScriptPageDebuggerExecutionState, JavaScriptPageDebuggerFrame,
    JavaScriptPageDebuggerLinkedExecutionState, JavaScriptPageDebuggerLinkedScopeSnapshot,
    JavaScriptPageDebuggerLinkedSpanAccess, JavaScriptPageDebuggerLinkedStackFrame,
    JavaScriptPageDebuggerLinkedStackSnapshot, JavaScriptPageDebuggerNestedExecutionState,
    JavaScriptPageDebuggerProgram, JavaScriptPageDebuggerSafePoint,
    JavaScriptPageDebuggerScopeEntry, JavaScriptPageDebuggerStackFrame,
    JavaScriptPageDebuggerStackSnapshot, JavaScriptPageDebuggerStaticMetadata,
    JavaScriptPageDebuggerStaticMetadataContractDisplay,
    JavaScriptPageDebuggerStaticMetadataContractId,
    JavaScriptPageDebuggerStaticMetadataContractLocation,
    JavaScriptPageDebuggerStaticMetadataContractLocationTarget,
    JavaScriptPageDebuggerStaticMetadataContractTarget,
    JavaScriptPageDebuggerStaticMetadataContractValidation,
    JavaScriptPageDebuggerStaticMetadataLoweringSummary,
    JavaScriptPageDebuggerStaticMetadataSafePointSpan,
    JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
    JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget,
    JavaScriptPageDebuggerStaticMetadataSourceId,
    JavaScriptPageDebuggerStaticMetadataSourceProvenance,
    JavaScriptPageDebuggerStaticMetadataSourceTarget, JavaScriptPageDebuggerStaticMetadataSummary,
    JavaScriptPageDebuggerStaticMetadataSymbolContract,
    JavaScriptPageDebuggerStaticMetadataSymbolContractTarget,
    JavaScriptPageDebuggerStaticMetadataSymbolDisplay,
    JavaScriptPageDebuggerStaticMetadataSymbolId,
    JavaScriptPageDebuggerStaticMetadataSymbolLocation,
    JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget,
    JavaScriptPageDebuggerStaticMetadataSymbolTarget,
    JavaScriptPageDebuggerStaticMetadataSymbolType,
    JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget,
    JavaScriptPageDebuggerStaticMetadataTypeDisplay, JavaScriptPageDebuggerStaticMetadataTypeId,
    JavaScriptPageDebuggerStaticMetadataTypeTarget, JavaScriptPageDebuggerStaticScopeRelation,
    JavaScriptPageDebuggerStaticScopeTarget, JavaScriptPageDebuggerValuePreview,
    JavaScriptPageDebuggerValueTarget,
};

mod debugger_locations;
pub use debugger_locations::PageJavaScriptDebuggerLocations;

/// The maximum number of source-free results retained for core observation.
const MAX_EXECUTION_REPORTS: usize = 128;

/// Fixed source and graph budgets for one JavaScript page host.
#[derive(Debug, Clone, Copy)]
pub struct JavaScriptPageExecutorConfig {
    /// Limits copied into every tab realm. Page content cannot change them.
    pub runtime: BlueJsPageRuntimeConfig,
    /// Maximum UTF-8 source bytes in one authorized or inline module.
    pub max_source_bytes_per_module: usize,
    /// Maximum modules in one caller-authorized ESM graph.
    pub max_modules_per_graph: usize,
    /// Fixed validation budgets for the installed immutable host-to-script
    /// snapshot. Page code cannot widen these contract limits.
    pub binding_contract_limits: CoreScriptBindingContractLimits,
    /// Maximum exact instruction boundaries the private debugger may retain in
    /// one reply for an admitted program. It is fixed by the core host before
    /// page execution; a page cannot request a wider inventory.
    pub max_debugger_safe_points_per_program: usize,
    /// Maximum exact breakpoint records retained for one live JavaScript
    /// realm. This bounds configuration storage; a pause capability additionally
    /// requires the separate opt-in root-entry scheduler below.
    pub max_debugger_breakpoints_per_realm: usize,
    /// Defers freshly admitted declarations by one session turn so a native
    /// debugger peer can arm an exact root-entry breakpoint before any BlueJS
    /// bytecode has run. This is deliberately opt-in: it changes page-script
    /// scheduling and is not a substitute for a general VM continuation.
    pub native_debugger_execution_control: bool,
}

impl Default for JavaScriptPageExecutorConfig {
    fn default() -> Self {
        Self {
            runtime: BlueJsPageRuntimeConfig::default(),
            max_source_bytes_per_module: 1024 * 1024,
            max_modules_per_graph: 128,
            binding_contract_limits: CoreScriptBindingContractLimits::default(),
            max_debugger_safe_points_per_program: 4_096,
            max_debugger_breakpoints_per_realm: 256,
            native_debugger_execution_control: false,
        }
    }
}

/// A classic or module declaration result retained by the owning core.
/// Reports intentionally contain no source, diagnostics, bytecode, program
/// handle, object identity, or JavaScript completion value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JavaScriptPageExecutionReport {
    Executed {
        tab_id: u64,
        document_generation: u64,
        ordinal: u32,
        kind: BlueJsPageScriptKind,
    },
    Rejected {
        tab_id: u64,
        document_generation: u64,
        ordinal: u32,
        kind: BlueJsPageScriptKind,
        category: &'static str,
    },
}

/// Source-free outcome from the out-of-process child host's explicit BlueTS
/// lane. It is separate from standard JavaScript reporting so frontend IPC
/// cannot misclassify a checked direct-lowering result as JavaScript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlueTsPageExecutionReport {
    Executed {
        tab_id: u64,
        document_generation: u64,
        ordinal: u32,
        kind: DirectPageScriptKind,
    },
    Rejected {
        tab_id: u64,
        document_generation: u64,
        ordinal: u32,
        kind: DirectPageScriptKind,
        category: &'static str,
    },
}

/// The session-facing lifecycle surface shared by explicitly selected
/// JavaScript page hosts.
///
/// It deliberately exposes only document synchronization and source-free
/// report draining. The in-process executor and the launcher-supervised child
/// executor may both implement it, but a session selects at most one owner for
/// a page at a time.
pub trait PageJavaScriptExecutor {
    /// Synchronizes current tab/document state and records bounded source-free
    /// execution results.
    fn synchronize_and_execute(&mut self, tabs: &TabManager) -> io::Result<()>;

    /// Session-thread-only variant used when an isolated child may issue
    /// synchronous DOM calls while core awaits its document result. Other
    /// executors keep the existing immutable synchronization path.
    fn synchronize_and_execute_serving_script(
        &mut self,
        tabs: &mut TabManager,
        _script_requests: Option<&ScriptRequestReceiver>,
    ) -> io::Result<()> {
        self.synchronize_and_execute(tabs)
    }

    /// Delivers a core-hit-tested click before default navigation. The
    /// default is inert; only an explicitly selected child event profile may
    /// return a cancellation decision. A child may synchronously call back
    /// into core DOM, so the session supplies its mutable tab owner.
    fn dispatch_click_serving_script(
        &mut self,
        _tabs: &mut TabManager,
        _tab_id: TabId,
        _node_id: u64,
        _script_requests: Option<&ScriptRequestReceiver>,
    ) -> io::Result<Option<bool>> {
        Ok(None)
    }

    /// Drains one tab's reports without exposing or consuming another tab's
    /// records.
    fn drain_reports_for_tab(&mut self, tab_id: TabId) -> Vec<JavaScriptPageExecutionReport>;

    /// Whether this selected page host owns the explicit BlueTS language lane
    /// in the same realm as standard JavaScript. The in-process JavaScript
    /// executor intentionally returns false; its API has no BlueTS compiler
    /// profile. The launcher-owned child fixes that profile privately.
    fn supports_blue_ts_page_execution(&self) -> bool {
        false
    }

    /// Drains explicit BlueTS reports when [`Self::supports_blue_ts_page_execution`]
    /// is true. A default empty implementation keeps a standard-JavaScript
    /// executor from accidentally claiming a second language/runtime owner.
    fn drain_blue_ts_reports_for_tab(&mut self, _tab_id: TabId) -> Vec<BlueTsPageExecutionReport> {
        Vec::new()
    }

    /// Returns the in-process program registry when this executor owns one.
    ///
    /// A remote child host intentionally has no core-visible BlueJS registry,
    /// so debugger requests remain unavailable on that route rather than
    /// crossing the process boundary through an unreviewed introspection API.
    fn debugger_executor(&mut self) -> Option<&mut JavaScriptPageExecutor> {
        None
    }

    /// Returns the limited location-only debugger adapter for an isolated
    /// child. It is intentionally separate from [`Self::debugger_executor`],
    /// which grants the in-process implementation its existing breakpoint and
    /// continuation controls.
    fn debugger_locations(&mut self) -> Option<&mut (dyn PageJavaScriptDebuggerLocations + '_)> {
        None
    }

    /// Lets the in-process native debugger preserve one pending declaration
    /// across a discovery turn. Isolated-child location discovery has no
    /// deferred execution seam, so its default is deliberately a no-op.
    fn hold_pending_debugger_execution_once(&mut self) {}
}

/// The core-only context supplied when authorizing an external JavaScript
/// declaration. Both URLs are untrusted page input until the authorizer has
/// enforced its own origin, CSP, integrity, fetch/cache, and resource policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaScriptPageSourceRequest {
    pub tab_id: TabId,
    pub document_generation: u64,
    pub ordinal: u32,
    pub kind: BlueJsPageScriptKind,
    pub document_url: String,
    pub declared_src: String,
}

/// One exact source record selected by a page-host authorizer.
///
/// Its content hash is constructed by this type from its exact source bytes;
/// callers cannot pair an arbitrary supplied hash with different source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedJavaScriptModule {
    canonical_module_id: String,
    source: String,
    source_hash: String,
}

impl AuthorizedJavaScriptModule {
    /// Creates one canonical source record. This performs no I/O or URL
    /// normalization: those remain authorizer responsibilities.
    pub fn new(
        canonical_module_id: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<Self, AuthorizedJavaScriptGraphError> {
        let canonical_module_id = canonical_module_id.into();
        if canonical_module_id.is_empty() || canonical_module_id.contains('\0') {
            return Err(AuthorizedJavaScriptGraphError::InvalidModuleIdentity);
        }
        let source = source.into();
        Ok(Self {
            source_hash: source_hash(&source),
            canonical_module_id,
            source,
        })
    }

    /// The canonical host-selected module identity.
    pub fn canonical_module_id(&self) -> &str {
        &self.canonical_module_id
    }

    /// The exact selected JavaScript source. It stays inside the core host and
    /// is never copied into execution reports or frontend IPC.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// A deterministic fingerprint of [`Self::source`].
    pub fn source_hash(&self) -> &str {
        &self.source_hash
    }
}

/// One closed static ESM resolution selected by the source authorizer.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AuthorizedJavaScriptResolution {
    pub from_module: String,
    pub specifier: String,
    pub canonical_target: String,
}

impl AuthorizedJavaScriptResolution {
    /// Forms a resolution record without interpreting a path or specifier.
    pub fn new(
        from_module: impl Into<String>,
        specifier: impl Into<String>,
        canonical_target: impl Into<String>,
    ) -> Result<Self, AuthorizedJavaScriptGraphError> {
        let from_module = from_module.into();
        let specifier = specifier.into();
        let canonical_target = canonical_target.into();
        if from_module.is_empty()
            || specifier.is_empty()
            || canonical_target.is_empty()
            || from_module.contains('\0')
            || specifier.contains('\0')
            || canonical_target.contains('\0')
        {
            return Err(AuthorizedJavaScriptGraphError::InvalidResolution);
        }
        Ok(Self {
            from_module,
            specifier,
            canonical_target,
        })
    }
}

/// A complete caller-authorized JavaScript module graph.
///
/// It has no ambient resolver. Before compiling a module, the executor checks
/// and replaces every static source specifier with the corresponding canonical
/// target from this record. Thus BlueJS receives canonical IDs only and never
/// chooses a relative path or falls back to a second resolver at this boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedJavaScriptModuleGraph {
    entry: String,
    modules: BTreeMap<String, AuthorizedJavaScriptModule>,
    resolutions: BTreeMap<(String, String), String>,
    resolver_fingerprint: String,
}

impl AuthorizedJavaScriptModuleGraph {
    /// Constructs a closed graph record with unique canonical modules and
    /// static resolution keys. Structural identity checks happen here; syntax
    /// and the requirement that every requested edge is present happen before
    /// any program enters a page realm.
    pub fn new(
        entry: impl Into<String>,
        modules: impl IntoIterator<Item = AuthorizedJavaScriptModule>,
        resolutions: impl IntoIterator<Item = AuthorizedJavaScriptResolution>,
        resolver_fingerprint: impl Into<String>,
    ) -> Result<Self, AuthorizedJavaScriptGraphError> {
        let entry = entry.into();
        if entry.is_empty() || entry.contains('\0') {
            return Err(AuthorizedJavaScriptGraphError::InvalidEntry);
        }
        let resolver_fingerprint = resolver_fingerprint.into();
        if resolver_fingerprint.trim().is_empty() || resolver_fingerprint.contains('\0') {
            return Err(AuthorizedJavaScriptGraphError::InvalidResolverFingerprint);
        }
        let mut module_map = BTreeMap::new();
        for module in modules {
            let id = module.canonical_module_id.clone();
            if module_map.insert(id.clone(), module).is_some() {
                return Err(AuthorizedJavaScriptGraphError::DuplicateModule(id));
            }
        }
        if !module_map.contains_key(&entry) {
            return Err(AuthorizedJavaScriptGraphError::MissingEntry(entry));
        }
        let mut resolution_map = BTreeMap::new();
        for resolution in resolutions {
            if !module_map.contains_key(&resolution.from_module)
                || !module_map.contains_key(&resolution.canonical_target)
            {
                return Err(AuthorizedJavaScriptGraphError::DanglingResolution {
                    from_module: resolution.from_module,
                    canonical_target: resolution.canonical_target,
                });
            }
            let key = (resolution.from_module, resolution.specifier);
            if resolution_map
                .insert(key.clone(), resolution.canonical_target)
                .is_some()
            {
                return Err(AuthorizedJavaScriptGraphError::DuplicateResolution {
                    from_module: key.0,
                    specifier: key.1,
                });
            }
        }
        Ok(Self {
            entry,
            modules: module_map,
            resolutions: resolution_map,
            resolver_fingerprint,
        })
    }

    /// The canonical graph entry selected by the host.
    pub fn entry(&self) -> &str {
        &self.entry
    }

    /// A host-selected resolver/cache policy identity. The current executor
    /// validates and preserves this record for its caller but does not yet
    /// expose a cross-process cache or debugger metadata API.
    pub fn resolver_fingerprint(&self) -> &str {
        &self.resolver_fingerprint
    }

    /// Iterates the exact source records selected when this closed graph was
    /// constructed. The iterator grants neither a resolver nor any source
    /// acquisition authority; a trusted owner may use it only to copy the
    /// already-authorized graph into another private execution transport.
    pub fn authorized_modules(&self) -> impl Iterator<Item = &AuthorizedJavaScriptModule> {
        self.modules.values()
    }

    /// Iterates the exact static edges selected when this closed graph was
    /// constructed. It does not resolve any new specifier or expose the
    /// executor's internal resolution map for mutation.
    pub fn authorized_resolutions(
        &self,
    ) -> impl Iterator<Item = AuthorizedJavaScriptResolution> + '_ {
        self.resolutions
            .iter()
            .map(
                |((from_module, specifier), target)| AuthorizedJavaScriptResolution {
                    from_module: from_module.clone(),
                    specifier: specifier.clone(),
                    canonical_target: target.clone(),
                },
            )
    }
}

/// Structural rejection for an authorized JavaScript graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizedJavaScriptGraphError {
    InvalidEntry,
    InvalidModuleIdentity,
    InvalidResolution,
    InvalidResolverFingerprint,
    DuplicateModule(String),
    MissingEntry(String),
    DanglingResolution {
        from_module: String,
        canonical_target: String,
    },
    DuplicateResolution {
        from_module: String,
        specifier: String,
    },
}

impl fmt::Display for AuthorizedJavaScriptGraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEntry => formatter.write_str("JavaScript graph entry is invalid"),
            Self::InvalidModuleIdentity => {
                formatter.write_str("JavaScript module identity is invalid")
            }
            Self::InvalidResolution => formatter.write_str("JavaScript resolution record is invalid"),
            Self::InvalidResolverFingerprint => {
                formatter.write_str("JavaScript resolver fingerprint is invalid")
            }
            Self::DuplicateModule(module) => write!(formatter, "duplicate JavaScript module `{module}`"),
            Self::MissingEntry(entry) => write!(formatter, "JavaScript graph has no entry `{entry}`"),
            Self::DanglingResolution {
                from_module,
                canonical_target,
            } => write!(
                formatter,
                "JavaScript resolution from `{from_module}` targets absent module `{canonical_target}`"
            ),
            Self::DuplicateResolution {
                from_module,
                specifier,
            } => write!(
                formatter,
                "duplicate JavaScript resolution `{from_module}` -> `{specifier}`"
            ),
        }
    }
}

impl std::error::Error for AuthorizedJavaScriptGraphError {}

/// The only authority accepted for an external JavaScript declaration.
/// Implementors are core/page-host owned: a parsed page and BlueJS never get
/// one, so they cannot fetch or resolve page content themselves.
pub trait JavaScriptPageSourceAuthorizer {
    fn authorize(
        &mut self,
        request: &JavaScriptPageSourceRequest,
    ) -> Result<AuthorizedJavaScriptModuleGraph, JavaScriptPageSourceAuthorizationError>;
}

/// A private external-source authorization failure. Its detail deliberately
/// never crosses the source-free page execution report boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaScriptPageSourceAuthorizationError {
    message: String,
}

impl JavaScriptPageSourceAuthorizationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for JavaScriptPageSourceAuthorizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for JavaScriptPageSourceAuthorizationError {}

/// A configured in-process JavaScript page host. It owns no DOM binding and
/// never runs unless the core lifecycle explicitly invokes
/// [`Self::synchronize_and_execute`].
pub struct JavaScriptPageExecutor {
    config: JavaScriptPageExecutorConfig,
    runtime: BlueJsPageRuntime,
    external_source_authorizer: Option<Box<dyn JavaScriptPageSourceAuthorizer>>,
    live_documents: BTreeMap<TabId, LivePageIdentity>,
    observed_documents: BTreeMap<TabId, u64>,
    debugger_programs: BTreeMap<TabId, Vec<DebuggerProgramRecord>>,
    debugger_breakpoints: BTreeMap<TabId, BTreeSet<DebuggerBreakpointRecord>>,
    pending_debugger_executions: BTreeMap<TabId, VecDeque<PendingDebuggerExecution>>,
    debugger_execution_states:
        BTreeMap<TabId, BTreeMap<DebuggerProgramKey, DebuggerExecutionStatus>>,
    /// A handshaken debugger discovery request gets one further core-session
    /// boundary to inspect an admitted declaration and arm its exact entry
    /// location. This is consumed by the next lifecycle synchronization; an
    /// idle session still starts the declaration normally.
    hold_pending_debugger_execution_once: bool,
    next_debugger_program_handle: u64,
    reports: VecDeque<JavaScriptPageExecutionReport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivePageIdentity {
    document_generation: u64,
    origin: BlueJsPageOrigin,
}

impl JavaScriptPageExecutor {
    /// Creates an executor with fixed default page and source-graph limits.
    pub fn new() -> Result<Self, JavaScriptPageExecutorError> {
        Self::with_config(JavaScriptPageExecutorConfig::default())
    }

    /// Creates an executor with fixed caller-selected resource limits.
    pub fn with_config(
        config: JavaScriptPageExecutorConfig,
    ) -> Result<Self, JavaScriptPageExecutorError> {
        if config.max_source_bytes_per_module == 0
            || config.max_modules_per_graph == 0
            || config.max_debugger_safe_points_per_program == 0
            || u32::try_from(config.max_debugger_safe_points_per_program).is_err()
            || config.max_debugger_breakpoints_per_realm == 0
            || u32::try_from(config.max_debugger_breakpoints_per_realm).is_err()
        {
            return Err(JavaScriptPageExecutorError::InvalidConfiguration);
        }
        let runtime = BlueJsPageRuntime::new(config.runtime)
            .map_err(JavaScriptPageExecutorError::PageRuntime)?;
        Ok(Self {
            config,
            runtime,
            external_source_authorizer: None,
            live_documents: BTreeMap::new(),
            observed_documents: BTreeMap::new(),
            debugger_programs: BTreeMap::new(),
            debugger_breakpoints: BTreeMap::new(),
            pending_debugger_executions: BTreeMap::new(),
            debugger_execution_states: BTreeMap::new(),
            hold_pending_debugger_execution_once: false,
            next_debugger_program_handle: 1,
            reports: VecDeque::new(),
        })
    }

    /// Adds the sole external-source authority. The executor itself continues
    /// to perform no fetch, URL resolution, import-map lookup, or fallback.
    pub fn with_external_source_authorizer(
        config: JavaScriptPageExecutorConfig,
        authorizer: impl JavaScriptPageSourceAuthorizer + 'static,
    ) -> Result<Self, JavaScriptPageExecutorError> {
        let mut executor = Self::with_config(config)?;
        executor.external_source_authorizer = Some(Box::new(authorizer));
        Ok(executor)
    }

    /// Synchronizes realm lifecycle and executes each supported declaration
    /// at most once for one tab/document generation. An independent rejection
    /// does not prevent a later declaration in that same document from being
    /// attempted.
    pub fn synchronize_and_execute(
        &mut self,
        tabs: &TabManager,
    ) -> Result<(), JavaScriptPageExecutorError> {
        self.close_removed_tabs(tabs);
        let tab_ids: Vec<_> = tabs.ids().collect();
        // Reconcile replacement before advancing a deferred program. A
        // declaration from the superseded document must be discarded rather
        // than execute during the new document's first lifecycle turn.
        for tab_id in &tab_ids {
            let Some(page) = tabs.get(*tab_id) else {
                continue;
            };
            if self.observed_documents.get(tab_id) != Some(&page.document_generation())
                && self.live_documents.contains_key(tab_id)
            {
                self.close_page(*tab_id);
            }
        }
        if !std::mem::take(&mut self.hold_pending_debugger_execution_once) {
            self.drive_debugger_executions();
        }
        for tab_id in tab_ids {
            let Some(page) = tabs.get(tab_id) else {
                continue;
            };
            let document_generation = page.document_generation();
            if self.observed_documents.get(&tab_id) == Some(&document_generation) {
                continue;
            }
            let declarations = page.blue_js_script_declarations();
            self.observed_documents.insert(tab_id, document_generation);
            if declarations.is_empty() {
                self.close_page(tab_id);
                continue;
            }
            let identity = match live_page_identity(page) {
                Ok(identity) => identity,
                Err(category) => {
                    self.close_page(tab_id);
                    for declaration in declarations {
                        self.reject_declaration(tab_id, document_generation, declaration, category);
                    }
                    continue;
                }
            };
            let document_text = page.script_document_text_content();
            if self.validate_document_text(&document_text).is_err()
                || self
                    .validate_document_origin(identity.origin.as_str())
                    .is_err()
            {
                self.close_page(tab_id);
                for declaration in declarations {
                    self.reject_declaration(
                        tab_id,
                        document_generation,
                        declaration,
                        "host binding contract rejected the page script",
                    );
                }
                continue;
            }
            self.activate_document(tab_id, identity, document_text)?;
            let document_url = page
                .url()
                .expect("a live page identity has a URL")
                .to_string();
            for declaration in declarations {
                self.execute_declaration(tab_id, document_generation, &document_url, declaration);
            }
        }
        Ok(())
    }

    /// Drains reports for one tab only, preserving all other-tab records.
    pub fn drain_reports_for_tab(&mut self, tab_id: TabId) -> Vec<JavaScriptPageExecutionReport> {
        let mut reports = Vec::new();
        let mut remaining = VecDeque::with_capacity(self.reports.len());
        while let Some(report) = self.reports.pop_front() {
            if report_tab_id(&report) == tab_id.as_u64() {
                reports.push(report);
            } else {
                remaining.push_back(report);
            }
        }
        self.reports = remaining;
        reports
    }

    /// Returns bounded accounting for one current JavaScript page realm.
    pub fn realm_stats(
        &self,
        tab_id: TabId,
    ) -> Result<blueice_bluejs::BlueJsPageRealmStats, JavaScriptPageExecutorError> {
        self.runtime
            .realm_stats(tab_id.as_u64())
            .map_err(JavaScriptPageExecutorError::PageRuntime)
    }

    fn close_removed_tabs(&mut self, tabs: &TabManager) {
        let closed: Vec<_> = self
            .live_documents
            .keys()
            .copied()
            .filter(|tab_id| tabs.get(*tab_id).is_none())
            .collect();
        for tab_id in closed {
            self.close_page(tab_id);
        }
        self.observed_documents
            .retain(|tab_id, _| tabs.get(*tab_id).is_some());
    }

    fn close_page(&mut self, tab_id: TabId) {
        self.live_documents.remove(&tab_id);
        self.debugger_programs.remove(&tab_id);
        self.debugger_breakpoints.remove(&tab_id);
        self.pending_debugger_executions.remove(&tab_id);
        self.debugger_execution_states.remove(&tab_id);
        self.runtime.close_realm(tab_id.as_u64());
    }

    fn activate_document(
        &mut self,
        tab_id: TabId,
        identity: LivePageIdentity,
        document_text: String,
    ) -> Result<(), JavaScriptPageExecutorError> {
        let document_origin = identity.origin.as_str().to_string();
        match self.live_documents.get(&tab_id) {
            Some(current) if current == &identity => {}
            Some(_) => {
                self.debugger_programs.remove(&tab_id);
                self.debugger_breakpoints.remove(&tab_id);
                self.pending_debugger_executions.remove(&tab_id);
                self.debugger_execution_states.remove(&tab_id);
                self.runtime
                    .navigate(tab_id.as_u64(), identity.origin.clone())
                    .map_err(JavaScriptPageExecutorError::PageRuntime)?
            }
            None => self
                .runtime
                .open_realm(tab_id.as_u64(), identity.origin.clone())
                .map_err(JavaScriptPageExecutorError::PageRuntime)?,
        }
        self.runtime
            .configure_realm_bindings(tab_id.as_u64(), move |bindings| {
                bindings.install_global_function(
                    "blueiceDocumentOrigin",
                    0,
                    move |arguments: &[HostValue]| {
                        require_no_arguments(arguments, "blueiceDocumentOrigin")?;
                        Ok(HostValue::String(document_origin.clone().into()))
                    },
                )?;
                bindings.install_global_function(
                    "blueiceDocumentText",
                    0,
                    move |arguments: &[HostValue]| {
                        require_no_arguments(arguments, "blueiceDocumentText")?;
                        Ok(HostValue::String(document_text.clone().into()))
                    },
                )
            })
            .map_err(JavaScriptPageExecutorError::PageRuntime)?;
        self.live_documents.insert(tab_id, identity);
        Ok(())
    }

    fn validate_document_text(&self, document_text: &str) -> Result<(), ()> {
        core_script_binding_contract("dom.document-text")
            .expect("the installed document-text binding has a contract inventory entry")
            .validate_string(
                document_text,
                self.config.binding_contract_limits.document_text,
            )
            .map_err(|_| ())
    }

    fn validate_document_origin(&self, document_origin: &str) -> Result<(), ()> {
        core_script_binding_contract("dom.document-origin")
            .expect("the installed document-origin binding has a contract inventory entry")
            .validate_string(
                document_origin,
                self.config.binding_contract_limits.document_origin,
            )
            .map_err(|_| ())
    }

    fn reject_declaration(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        declaration: BlueJsPageScriptDeclaration,
        category: &'static str,
    ) {
        let (ordinal, kind) = declaration_identity(&declaration);
        self.push_report(JavaScriptPageExecutionReport::Rejected {
            tab_id: tab_id.as_u64(),
            document_generation,
            ordinal,
            kind,
            category,
        });
    }

    fn push_report(&mut self, report: JavaScriptPageExecutionReport) {
        if self.reports.len() == MAX_EXECUTION_REPORTS {
            self.reports.pop_front();
        }
        self.reports.push_back(report);
    }
}

impl Default for JavaScriptPageExecutor {
    fn default() -> Self {
        Self::new().expect("default JavaScript page executor configuration is valid")
    }
}

impl PageJavaScriptExecutor for JavaScriptPageExecutor {
    fn synchronize_and_execute(&mut self, tabs: &TabManager) -> io::Result<()> {
        Self::synchronize_and_execute(self, tabs).map_err(|error| {
            io::Error::other(format!("inline JavaScript execution failed: {error}"))
        })
    }

    fn drain_reports_for_tab(&mut self, tab_id: TabId) -> Vec<JavaScriptPageExecutionReport> {
        Self::drain_reports_for_tab(self, tab_id)
    }

    fn debugger_executor(&mut self) -> Option<&mut JavaScriptPageExecutor> {
        Some(self)
    }

    fn hold_pending_debugger_execution_once(&mut self) {
        Self::hold_pending_debugger_execution_once(self);
    }
}

/// Construction/lifecycle failure distinct from page-controlled declaration
/// rejections. Page parse, compilation, policy, and runtime failures become a
/// bounded report and do not terminate a core session.
#[derive(Debug)]
pub enum JavaScriptPageExecutorError {
    InvalidConfiguration,
    PageRuntime(BlueJsPageRuntimeError),
}

impl fmt::Display for JavaScriptPageExecutorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => {
                formatter.write_str("invalid JavaScript page executor configuration")
            }
            Self::PageRuntime(error) => {
                write!(formatter, "JavaScript page runtime failed: {error}")
            }
        }
    }
}

impl std::error::Error for JavaScriptPageExecutorError {}

fn live_page_identity(page: &Page) -> Result<LivePageIdentity, &'static str> {
    let url = page
        .url()
        .ok_or("page document has no supported script origin")?;
    let origin = blueice_net::canonical_http_origin(url)
        .map_err(|_| "page document has no supported script origin")?;
    let origin = BlueJsPageOrigin::new(origin)
        .map_err(|_| "page document has no supported script origin")?;
    Ok(LivePageIdentity {
        document_generation: page.document_generation(),
        origin,
    })
}

fn declaration_identity(declaration: &BlueJsPageScriptDeclaration) -> (u32, BlueJsPageScriptKind) {
    match declaration {
        BlueJsPageScriptDeclaration::Inline { ordinal, kind, .. }
        | BlueJsPageScriptDeclaration::External { ordinal, kind, .. } => (*ordinal, *kind),
    }
}

fn report_tab_id(report: &JavaScriptPageExecutionReport) -> u64 {
    match report {
        JavaScriptPageExecutionReport::Executed { tab_id, .. }
        | JavaScriptPageExecutionReport::Rejected { tab_id, .. } => *tab_id,
    }
}

fn inline_module_id(tab_id: TabId, document_generation: u64, ordinal: u32) -> String {
    format!(
        "blueice://page/tab-{}/document-{document_generation}/inline-{ordinal}.js",
        tab_id.as_u64()
    )
}

fn source_hash(source: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in source.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}

fn require_no_arguments(arguments: &[HostValue], function: &str) -> Result<(), HostFunctionError> {
    if arguments.is_empty() {
        Ok(())
    } else {
        Err(HostFunctionError::new(format!(
            "{function} requires no arguments"
        )))
    }
}

fn source_identity(module: &AuthorizedJavaScriptModule) -> BlueJsSourceIdentity {
    BlueJsSourceIdentity::new(module.canonical_module_id(), module.source_hash())
        .expect("an authorized JavaScript module has a valid canonical identity and hash")
}

fn discard_programs(
    runtime: &mut BlueJsPageRuntime,
    tab_id: TabId,
    handles: &[BlueJsProgramHandle],
) {
    for handle in handles.iter().rev().copied() {
        let _ = runtime.discard_program(tab_id.as_u64(), handle);
    }
}

fn rewrite_static_module_requests(
    module_id: &str,
    module: &mut blueice_bluejs::Module,
    resolutions: &BTreeMap<(String, String), String>,
) -> Result<(), &'static str> {
    let resolve = |specifier: &str| {
        resolutions
            .get(&(module_id.to_string(), specifier.to_string()))
            .cloned()
            .ok_or("authorized JavaScript graph is missing a static resolution")
    };
    for import in &mut module.imports {
        import.module_request = resolve(&import.module_request)?;
    }
    for export in &mut module.exports {
        match export {
            blueice_bluejs::ExportEntry::Indirect { module_request, .. }
            | blueice_bluejs::ExportEntry::Star { module_request, .. }
            | blueice_bluejs::ExportEntry::Namespace { module_request, .. } => {
                *module_request = resolve(module_request)?;
            }
            blueice_bluejs::ExportEntry::Local { .. } => {}
        }
    }
    for request in &mut module.requests {
        request.specifier = resolve(&request.specifier)?;
    }
    Ok(())
}

fn parse_category(_: ParseError) -> &'static str {
    "JavaScript parsing rejected the page script"
}

fn compile_category(_: CompileError) -> &'static str {
    "BlueJS compilation rejected the page script"
}

fn page_runtime_category(error: BlueJsPageRuntimeError) -> &'static str {
    match error {
        BlueJsPageRuntimeError::BytecodeLimit { .. }
        | BlueJsPageRuntimeError::ProgramLimit { .. }
        | BlueJsPageRuntimeError::RealmLimit { .. } => {
            "JavaScript page resource policy rejected the page script"
        }
        BlueJsPageRuntimeError::Runtime(RuntimeError::ModuleResolution(_)) => {
            "authorized JavaScript graph rejected the page script"
        }
        BlueJsPageRuntimeError::Runtime(_) => "BlueJS page execution failed",
        _ => "BlueJS page host rejected the page script",
    }
}

#[cfg(test)]
#[path = "javascript/tests.rs"]
mod tests;
