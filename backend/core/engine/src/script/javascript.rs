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

use super::{BlueJsPageScriptDeclaration, BlueJsPageScriptKind};
use crate::{Page, TabId, TabManager};
use blueice_bluejs::{
    parse, parse_module, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeConfig,
    BlueJsPageRuntimeError, BlueJsProgramHandle, BlueJsProgramV1, BlueJsSourceIdentity,
    CompileError, ParseError, RuntimeError, Value,
};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;

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
}

impl Default for JavaScriptPageExecutorConfig {
    fn default() -> Self {
        Self {
            runtime: BlueJsPageRuntimeConfig::default(),
            max_source_bytes_per_module: 1024 * 1024,
            max_modules_per_graph: 128,
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
        if config.max_source_bytes_per_module == 0 || config.max_modules_per_graph == 0 {
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
            self.activate_document(tab_id, identity)?;
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
        self.runtime.close_realm(tab_id.as_u64());
    }

    fn activate_document(
        &mut self,
        tab_id: TabId,
        identity: LivePageIdentity,
    ) -> Result<(), JavaScriptPageExecutorError> {
        match self.live_documents.get(&tab_id) {
            Some(current) if current == &identity => {}
            Some(_) => self
                .runtime
                .navigate(tab_id.as_u64(), identity.origin.clone())
                .map_err(JavaScriptPageExecutorError::PageRuntime)?,
            None => self
                .runtime
                .open_realm(tab_id.as_u64(), identity.origin.clone())
                .map_err(JavaScriptPageExecutorError::PageRuntime)?,
        }
        self.live_documents.insert(tab_id, identity);
        Ok(())
    }

    fn execute_declaration(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        document_url: &str,
        declaration: BlueJsPageScriptDeclaration,
    ) {
        let (ordinal, kind) = declaration_identity(&declaration);
        let result = match declaration {
            BlueJsPageScriptDeclaration::Inline { source, .. } => {
                self.execute_inline(tab_id, document_generation, ordinal, kind, source)
            }
            BlueJsPageScriptDeclaration::External { src, .. } => self.execute_external(
                tab_id,
                document_generation,
                ordinal,
                kind,
                document_url,
                src,
            ),
        };
        match result {
            Ok(()) => self.push_report(JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation,
                ordinal,
                kind,
            }),
            Err(category) => self.push_report(JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation,
                ordinal,
                kind,
                category,
            }),
        }
    }

    fn execute_inline(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        ordinal: u32,
        kind: BlueJsPageScriptKind,
        source: String,
    ) -> Result<(), &'static str> {
        let entry = inline_module_id(tab_id, document_generation, ordinal);
        let module = AuthorizedJavaScriptModule::new(entry.clone(), source)
            .expect("core-generated inline JavaScript identity is valid");
        match kind {
            BlueJsPageScriptKind::Classic => self.execute_classic(tab_id, &module),
            BlueJsPageScriptKind::Module => {
                let graph = AuthorizedJavaScriptModuleGraph::new(
                    entry,
                    [module],
                    [],
                    "core-inline-javascript-v1",
                )
                .expect("one core-generated inline module forms a valid graph");
                self.execute_module_graph(tab_id, &graph)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_external(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        ordinal: u32,
        kind: BlueJsPageScriptKind,
        document_url: &str,
        declared_src: String,
    ) -> Result<(), &'static str> {
        let Some(authorizer) = self.external_source_authorizer.as_mut() else {
            return Err("external JavaScript declarations require an authorized loader");
        };
        let graph = authorizer
            .authorize(&JavaScriptPageSourceRequest {
                tab_id,
                document_generation,
                ordinal,
                kind,
                document_url: document_url.to_string(),
                declared_src,
            })
            .map_err(|_| "external JavaScript source authorization rejected the page script")?;
        match kind {
            BlueJsPageScriptKind::Classic => self.execute_external_classic(tab_id, &graph),
            BlueJsPageScriptKind::Module => self.execute_module_graph(tab_id, &graph),
        }
    }

    fn execute_external_classic(
        &mut self,
        tab_id: TabId,
        graph: &AuthorizedJavaScriptModuleGraph,
    ) -> Result<(), &'static str> {
        if graph.modules.len() != 1 {
            return Err("classic JavaScript source graph is not closed");
        }
        let module = graph
            .modules
            .get(graph.entry())
            .expect("graph construction validates its entry");
        self.execute_classic(tab_id, module)
    }

    fn execute_classic(
        &mut self,
        tab_id: TabId,
        module: &AuthorizedJavaScriptModule,
    ) -> Result<(), &'static str> {
        self.check_module_source(module)?;
        let program = parse(module.source()).map_err(parse_category)?;
        let program = BlueJsProgramV1::Script(program);
        program.compile().map_err(compile_category)?;
        let origin = self.origin_for_tab(tab_id)?;
        let handle = self
            .runtime
            .install_program(tab_id.as_u64(), &origin, source_identity(module), &program)
            .map_err(page_runtime_category)?;
        self.runtime
            .execute_program(tab_id.as_u64(), handle)
            .map(|_: Value| ())
            .map_err(page_runtime_category)
    }

    fn execute_module_graph(
        &mut self,
        tab_id: TabId,
        graph: &AuthorizedJavaScriptModuleGraph,
    ) -> Result<(), &'static str> {
        if graph.modules.len() > self.config.max_modules_per_graph {
            return Err("JavaScript module graph exceeds configured policy");
        }
        let mut programs = BTreeMap::new();
        for (module_id, module) in &graph.modules {
            self.check_module_source(module)?;
            let mut parsed = parse_module(module.source()).map_err(parse_category)?;
            rewrite_static_module_requests(module_id, &mut parsed, &graph.resolutions)?;
            let program = BlueJsProgramV1::Module(parsed);
            // Preflight every module before admitting any part of the graph.
            program.compile().map_err(compile_category)?;
            programs.insert(module_id.clone(), program);
        }
        let origin = self.origin_for_tab(tab_id)?;
        let mut installed = Vec::new();
        for (module_id, program) in &programs {
            let module = graph
                .modules
                .get(module_id)
                .expect("programs derive from every graph module");
            let handle = match self.runtime.install_program(
                tab_id.as_u64(),
                &origin,
                source_identity(module),
                program,
            ) {
                Ok(handle) => handle,
                Err(error) => {
                    discard_programs(&mut self.runtime, tab_id, &installed);
                    return Err(page_runtime_category(error));
                }
            };
            installed.push(handle);
        }
        let entry = programs
            .keys()
            .position(|module_id| module_id == graph.entry())
            .and_then(|index| installed.get(index).copied())
            .expect("graph construction validates the entry");
        self.runtime
            .execute_module_graph(tab_id.as_u64(), entry, installed)
            .map(|_: Value| ())
            .map_err(page_runtime_category)
    }

    fn check_module_source(&self, module: &AuthorizedJavaScriptModule) -> Result<(), &'static str> {
        (module.source().len() <= self.config.max_source_bytes_per_module)
            .then_some(())
            .ok_or("JavaScript source exceeds configured policy")
    }

    fn origin_for_tab(&self, tab_id: TabId) -> Result<BlueJsPageOrigin, &'static str> {
        self.live_documents
            .get(&tab_id)
            .map(|identity| identity.origin.clone())
            .ok_or("page realm is no longer available")
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
mod tests {
    use super::*;

    fn loaded_tabs(html: &str, url: &str) -> (TabManager, TabId) {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id)
            .unwrap()
            .load_html_str(html, Some(url.to_string()));
        (tabs, tab_id)
    }

    fn reports(
        executor: &mut JavaScriptPageExecutor,
        tab_id: TabId,
    ) -> Vec<JavaScriptPageExecutionReport> {
        executor.drain_reports_for_tab(tab_id)
    }

    #[test]
    fn executes_inline_classic_and_module_declarations_in_one_realm() {
        let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script>const answer = 40 + 2; answer;</script>",
                "<script type=\"module\">export const moduleAnswer = 43;</script>"
            ),
            "https://example.test/app/index.html",
        );
        let mut executor = JavaScriptPageExecutor::default();

        executor.synchronize_and_execute(&tabs).unwrap();

        assert_eq!(
            reports(&mut executor, tab_id),
            vec![
                JavaScriptPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 0,
                    kind: BlueJsPageScriptKind::Classic,
                },
                JavaScriptPageExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 1,
                    kind: BlueJsPageScriptKind::Module,
                },
            ]
        );
        assert_eq!(executor.realm_stats(tab_id).unwrap().program_count, 2);
    }

    #[test]
    fn rejected_declaration_does_not_block_a_later_script() {
        let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script>const = syntaxError;</script>",
                "<script>41 + 1;</script>"
            ),
            "https://example.test/app/index.html",
        );
        let mut executor = JavaScriptPageExecutor::default();

        executor.synchronize_and_execute(&tabs).unwrap();

        let reports = reports(&mut executor, tab_id);
        assert!(matches!(
            reports.as_slice(),
            [
                JavaScriptPageExecutionReport::Rejected {
                    category: "JavaScript parsing rejected the page script",
                    ..
                },
                JavaScriptPageExecutionReport::Executed { .. }
            ]
        ));
    }

    #[test]
    fn external_declaration_fails_closed_without_an_authorizer() {
        let (tabs, tab_id) = loaded_tabs(
            "<script src=\"/assets/app.js\"></script>",
            "https://example.test/app/index.html",
        );
        let mut executor = JavaScriptPageExecutor::default();

        executor.synchronize_and_execute(&tabs).unwrap();

        assert!(matches!(
            reports(&mut executor, tab_id).as_slice(),
            [JavaScriptPageExecutionReport::Rejected {
                category: "external JavaScript declarations require an authorized loader",
                ..
            }]
        ));
    }

    #[test]
    fn navigation_replaces_the_realm_and_releases_old_programs() {
        let (mut tabs, tab_id) = loaded_tabs(
            "<script>const first = 1;</script>",
            "https://example.test/first.html",
        );
        let mut executor = JavaScriptPageExecutor::default();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(executor.realm_stats(tab_id).unwrap().program_count, 1);
        let _ = reports(&mut executor, tab_id);

        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script>const second = 2;</script>",
            Some("https://example.test/second.html".to_string()),
        );
        executor.synchronize_and_execute(&tabs).unwrap();

        assert_eq!(executor.realm_stats(tab_id).unwrap().program_count, 1);
        assert!(matches!(
            reports(&mut executor, tab_id).as_slice(),
            [JavaScriptPageExecutionReport::Executed {
                document_generation: 2,
                ..
            }]
        ));
    }

    #[test]
    fn source_budget_rejects_before_parser_or_program_admission() {
        let (tabs, tab_id) = loaded_tabs(
            "<script>const answer = 42;</script>",
            "https://example.test/app/index.html",
        );
        let mut executor = JavaScriptPageExecutor::with_config(JavaScriptPageExecutorConfig {
            max_source_bytes_per_module: 1,
            ..JavaScriptPageExecutorConfig::default()
        })
        .unwrap();

        executor.synchronize_and_execute(&tabs).unwrap();

        assert!(matches!(
            reports(&mut executor, tab_id).as_slice(),
            [JavaScriptPageExecutionReport::Rejected {
                category: "JavaScript source exceeds configured policy",
                ..
            }]
        ));
        assert_eq!(executor.realm_stats(tab_id).unwrap().program_count, 0);
    }

    struct FixedAuthorizer {
        graph: AuthorizedJavaScriptModuleGraph,
    }

    impl JavaScriptPageSourceAuthorizer for FixedAuthorizer {
        fn authorize(
            &mut self,
            _request: &JavaScriptPageSourceRequest,
        ) -> Result<AuthorizedJavaScriptModuleGraph, JavaScriptPageSourceAuthorizationError>
        {
            Ok(self.graph.clone())
        }
    }

    #[test]
    fn external_module_uses_authorized_canonical_resolution_records() {
        let entry = AuthorizedJavaScriptModule::new(
            "blueice://authorized/main.js",
            "import { value } from './dep.js'; value;",
        )
        .unwrap();
        let dependency = AuthorizedJavaScriptModule::new(
            "blueice://authorized/dep.js",
            "export const value = 42;",
        )
        .unwrap();
        let graph = AuthorizedJavaScriptModuleGraph::new(
            "blueice://authorized/main.js",
            [entry, dependency],
            [AuthorizedJavaScriptResolution::new(
                "blueice://authorized/main.js",
                "./dep.js",
                "blueice://authorized/dep.js",
            )
            .unwrap()],
            "test-authorized-javascript-resolver-v1",
        )
        .unwrap();
        let (tabs, tab_id) = loaded_tabs(
            "<script type=\"module\" src=\"/assets/main.js\"></script>",
            "https://example.test/app/index.html",
        );
        let mut executor = JavaScriptPageExecutor::with_external_source_authorizer(
            JavaScriptPageExecutorConfig::default(),
            FixedAuthorizer { graph },
        )
        .unwrap();

        executor.synchronize_and_execute(&tabs).unwrap();

        assert!(matches!(
            reports(&mut executor, tab_id).as_slice(),
            [JavaScriptPageExecutionReport::Executed {
                kind: BlueJsPageScriptKind::Module,
                ..
            }]
        ));
        assert_eq!(executor.realm_stats(tab_id).unwrap().program_count, 2);
    }

    #[test]
    fn missing_static_resolution_rejects_without_admitting_any_graph_program() {
        let entry = AuthorizedJavaScriptModule::new(
            "blueice://authorized/main.js",
            "import { value } from './dep.js'; value;",
        )
        .unwrap();
        let dependency = AuthorizedJavaScriptModule::new(
            "blueice://authorized/dep.js",
            "export const value = 42;",
        )
        .unwrap();
        let graph = AuthorizedJavaScriptModuleGraph::new(
            "blueice://authorized/main.js",
            [entry, dependency],
            [],
            "test-authorized-javascript-resolver-v1",
        )
        .unwrap();
        let (tabs, tab_id) = loaded_tabs(
            "<script type=\"module\" src=\"/assets/main.js\"></script>",
            "https://example.test/app/index.html",
        );
        let mut executor = JavaScriptPageExecutor::with_external_source_authorizer(
            JavaScriptPageExecutorConfig::default(),
            FixedAuthorizer { graph },
        )
        .unwrap();

        executor.synchronize_and_execute(&tabs).unwrap();

        assert!(matches!(
            reports(&mut executor, tab_id).as_slice(),
            [JavaScriptPageExecutionReport::Rejected {
                category: "authorized JavaScript graph is missing a static resolution",
                ..
            }]
        ));
        assert_eq!(executor.realm_stats(tab_id).unwrap().program_count, 0);
    }
}
