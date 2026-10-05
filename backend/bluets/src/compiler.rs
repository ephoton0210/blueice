// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Closed-world module loading and the shared BlueTS compile pipeline.

use crate::checker;
use crate::debug_info;
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::emitter;
use crate::parser::{parse_module, parse_module_with_limits, Module, ParserLimits};
use crate::strict_boundaries;
use crate::{Compilation, LANGUAGE_VERSION};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

/// Closed-project work limits. They are part of [`CompilerOptions`] so a
/// cached result or artifact cannot be reused under a looser resource policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerLimits {
    pub max_modules: usize,
    /// Maximum number of import/type-export edges in the closed graph.
    pub max_module_edges: usize,
    /// Maximum number of resolution edges from the entry module. The entry is
    /// at depth zero, so a limit of one permits its direct dependencies.
    pub max_module_depth: usize,
    pub max_total_source_bytes: usize,
    pub parser: ParserLimits,
    pub max_type_expansions: usize,
    pub max_source_map_segments: usize,
}

impl Default for CompilerLimits {
    fn default() -> Self {
        Self {
            max_modules: 4_096,
            max_module_edges: 16_384,
            max_module_depth: 128,
            max_total_source_bytes: 16 * 1_024 * 1_024,
            parser: ParserLimits::default(),
            max_type_expansions: 256,
            max_source_map_segments: 100_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcmaTarget {
    Es2020,
    Es2022,
}

impl EcmaTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Es2020 => "es2020",
            Self::Es2022 => "es2022",
        }
    }
}

/// The module system emitted JavaScript uses (TypeScript's `module`): ECMAScript
/// modules, or CommonJS (`require`, `exports`, `module.exports`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModuleKind {
    #[default]
    Esm,
    CommonJs,
}

impl ModuleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Esm => "esm",
            Self::CommonJs => "commonjs",
        }
    }
}

/// TypeScript's `jsx` option for `.tsx` modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsxMode {
    /// Keep the JSX syntax (the output is `.jsx`).
    Preserve,
    /// Keep the JSX syntax; the output is `.js` (React Native's Metro reads it).
    ReactNative,
    /// `React.createElement` (or the configured factory) calls.
    React,
    /// The automatic runtime: calls of `jsx`/`jsxs` imported from
    /// `<jsxImportSource>/jsx-runtime`.
    ReactJsx,
    /// As `ReactJsx` with `jsxDEV` and source locations.
    ReactJsxDev,
}

impl JsxMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::ReactNative => "react-native",
            Self::React => "react",
            Self::ReactJsx => "react-jsx",
            Self::ReactJsxDev => "react-jsxdev",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "preserve" => Some(Self::Preserve),
            "react-native" => Some(Self::ReactNative),
            "react" => Some(Self::React),
            "react-jsx" => Some(Self::ReactJsx),
            "react-jsxdev" => Some(Self::ReactJsxDev),
            _ => None,
        }
    }

    /// Whether the JSX syntax is lowered to calls.
    pub fn lowers(self) -> bool {
        matches!(self, Self::React | Self::ReactJsx | Self::ReactJsxDev)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePolicy {
    TranspileOnly,
    Checked,
    StrictRuntime,
}

/// An owner-selected source crossing for the first emitted strict-runtime
/// profile. The source span must match a named exported function exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrictRuntimeBoundary {
    pub contract_id: String,
    pub function: String,
    pub span: SourceSpan,
    pub max_string_bytes: usize,
    pub helper_version: String,
}

impl RuntimePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TranspileOnly => "transpile-only",
            Self::Checked => "checked",
            Self::StrictRuntime => "strict-runtime",
        }
    }
}

/// All values that affect checking and output are explicit, so callers can
/// persist this alongside build artifacts and cache keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerOptions {
    pub target: EcmaTarget,
    /// TypeScript's `useDefineForClassFields`: `Some(true)` defines class
    /// fields (native ES2022 fields, or `Object.defineProperty` below it),
    /// `Some(false)` assigns them in the constructor, and `None` follows the
    /// target, as TypeScript does (define for ES2022, assign for ES2020).
    pub use_define_for_class_fields: Option<bool>,
    /// Emit the runtime object of a `const enum` even though its uses are
    /// replaced by their values (TypeScript's `preserveConstEnums`).
    pub preserve_const_enums: bool,
    /// Each module must be emittable on its own (TypeScript's `isolatedModules`,
    /// for enums): a `const enum` is emitted as an ordinary enum and referenced
    /// through its object, never inlined, and an ambient `const enum` cannot be
    /// used, since another module's values are not assumed to be known.
    pub isolated_modules: bool,
    /// The module system of the emitted JavaScript.
    pub module_kind: ModuleKind,
    /// TypeScript's `esModuleInterop`: a default or namespace import of a
    /// CommonJS `export =` module is allowed and goes through a helper that gives
    /// it a `default` member. Only meaningful with `ModuleKind::CommonJs`.
    pub es_module_interop: bool,
    /// TypeScript's `experimentalDecorators`: the legacy (pre-TC39) decorators,
    /// with their own evaluation and application order, parameter decorators and
    /// `__decorate` helper, instead of the standard ones.
    pub experimental_decorators: bool,
    /// TypeScript's `emitDecoratorMetadata`: `design:type`, `design:paramtypes` and
    /// `design:returntype` metadata for decorated elements. Needs
    /// `experimental_decorators`.
    pub emit_decorator_metadata: bool,
    /// How `.tsx` JSX is emitted; `None` makes any JSX an error, as in
    /// TypeScript (TS17004).
    pub jsx: Option<JsxMode>,
    /// `jsxFactory` (classic mode): an entity name such as `h` or `Preact.h`.
    pub jsx_factory: Option<String>,
    /// `jsxFragmentFactory` (classic mode): defaults to `React.Fragment`.
    pub jsx_fragment_factory: Option<String>,
    /// `jsxImportSource` (automatic mode): defaults to `react`.
    pub jsx_import_source: Option<String>,
    pub runtime_policy: RuntimePolicy,
    pub source_map: bool,
    pub declaration: bool,
    /// Host-supplied identity for resolution inputs such as an import map.
    /// This prevents an artifact/cache key from being reused under a resolver
    /// policy different from the one that selected its module graph.
    pub resolver_fingerprint: String,
    /// Host-provided `.d.ts` modules made available as ambient declarations to
    /// every checked source module. They are parsed under the ordinary source
    /// and module-count limits, emitted nowhere, and cannot import further
    /// modules. This is for a selected, already-verified host type surface,
    /// not a replacement for caller-authorized source-graph loading.
    pub ambient_declaration_modules: Vec<ModuleSource>,
    /// Rejects a direct call whose callee is neither a local nor an
    /// host-supplied ambient function. Page hosts enable this so a TypeScript
    /// profile cannot silently compile a call to a missing runtime binding.
    pub require_declared_global_calls: bool,
    /// Explicit emitted crossings. Direct page hosts leave this empty because
    /// their runtime contract inventory is owned and checked separately.
    pub strict_runtime_boundaries: Vec<StrictRuntimeBoundary>,
    pub limits: CompilerLimits,
}

impl CompilerOptions {
    /// Whether a use of a `const enum` member is replaced by its value.
    /// Transpile-only compiles without types, so it cannot know the value.
    pub fn inlines_const_enums(&self) -> bool {
        !self.isolated_modules && self.runtime_policy != RuntimePolicy::TranspileOnly
    }

    /// Whether class fields are defined rather than assigned, once the target's
    /// default is applied.
    pub fn defines_class_fields(&self) -> bool {
        self.use_define_for_class_fields
            .unwrap_or(self.target == EcmaTarget::Es2022)
    }
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            target: EcmaTarget::Es2022,
            use_define_for_class_fields: None,
            preserve_const_enums: false,
            isolated_modules: false,
            module_kind: ModuleKind::Esm,
            es_module_interop: false,
            experimental_decorators: false,
            emit_decorator_metadata: false,
            jsx: None,
            jsx_factory: None,
            jsx_fragment_factory: None,
            jsx_import_source: None,
            runtime_policy: RuntimePolicy::Checked,
            source_map: false,
            declaration: false,
            resolver_fingerprint: "relative-v1".to_string(),
            ambient_declaration_modules: Vec::new(),
            require_declared_global_calls: false,
            strict_runtime_boundaries: Vec::new(),
            limits: CompilerLimits::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSource {
    pub id: String,
    pub text: String,
}

impl ModuleSource {
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
        }
    }
}

/// The compiler's only module-loading authority.  Page hosts can implement it
/// from already policy-checked loader records; the standalone CLI implements
/// it from an explicit project root.  BlueTS itself does not open files or
/// URLs.
pub trait ModuleLoader {
    fn load(&self, module_id: &str) -> Result<ModuleSource, String>;

    fn resolve(&self, from_module: &str, specifier: &str) -> Result<String, String> {
        resolve_relative_module(from_module, specifier)
    }
}

/// A deterministic in-memory loader useful to embedders and unit tests.
#[derive(Debug, Default, Clone)]
pub struct MapLoader {
    modules: BTreeMap<String, ModuleSource>,
}

impl MapLoader {
    pub fn from(sources: impl IntoIterator<Item = ModuleSource>) -> Self {
        Self {
            modules: sources
                .into_iter()
                .map(|source| (source.id.clone(), source))
                .collect(),
        }
    }
}

impl ModuleLoader for MapLoader {
    fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
        self.modules
            .get(module_id)
            .cloned()
            .ok_or_else(|| format!("module `{module_id}` is not present in this loader"))
    }
}

/// Returns whether a source identity is a TypeScript declaration module. A
/// host is still responsible for authorizing that source identity before it
/// reaches BlueTS; this helper only controls static-only compiler behavior.
pub(crate) fn is_declaration_module(module_id: &str) -> bool {
    module_id.ends_with(".d.ts")
}

/// Whether a module comes from an installed package: its path has a
/// `node_modules` segment, it is a package file reached through a symlink to a
/// directory without one (`@package/..`), a pinned remote declaration (`@remote/..`), or it is in an owner-authorized
/// external package root (`@external/<n>/..`). Like TypeScript's external-library files it is checked
/// and its types are used, but it is not emitted: the importing program keeps
/// the package's own specifier for the runtime to resolve.
pub fn is_external_library_module(module_id: &str) -> bool {
    module_id.starts_with("@external/")
        || module_id.starts_with("@package/")
        || module_id.starts_with("@remote/")
        || module_id.split('/').any(|part| part == "node_modules")
}

/// The source graph after parsing and host-controlled resolution.  It contains
/// no JavaScript-runtime dependency and is reusable by check-only and build
/// callers alike.
#[derive(Debug, Clone)]
pub struct Project {
    pub entry: String,
    pub modules: BTreeMap<String, Module>,
    pub(crate) resolutions: BTreeMap<(String, String), String>,
    pub(crate) ambient_declaration_modules: BTreeSet<String>,
}

impl Project {
    /// Returns the caller-authorized canonical target selected for one source
    /// module request. Runtime bridges use this rather than independently
    /// resolving a TypeScript specifier under a potentially different policy.
    pub fn resolved_module(&self, from_module: &str, specifier: &str) -> Option<&str> {
        self.resolutions
            .get(&(from_module.to_string(), specifier.to_string()))
            .map(String::as_str)
    }

    /// Returns whether `module_id` was supplied as a host-selected ambient
    /// declaration rather than reached through an import in the source graph.
    /// Such declarations contribute static checking/provenance only and are
    /// never copied to standalone declaration output.
    pub fn is_ambient_declaration_module(&self, module_id: &str) -> bool {
        self.ambient_declaration_modules.contains(module_id)
    }
}

/// The result of one [`IncrementalCompiler`] invocation. The sets describe
/// work selected by the host-neutral front end, making cache behavior
/// observable to a future page host without exposing any runtime state.
#[derive(Debug, Clone)]
pub struct IncrementalResult {
    pub compilation: Compilation,
    /// True when an unchanged successful graph was returned directly from the
    /// session cache.
    pub cache_hit: bool,
    /// Modules tokenized and parsed during this invocation.
    pub parsed_modules: BTreeSet<String>,
    /// Modules whose parsed syntax was reused after their loaded source bytes
    /// matched the last successful compilation.
    pub reused_parsed_modules: BTreeSet<String>,
    /// Modules bound and type-checked during this invocation.
    pub rechecked_modules: BTreeSet<String>,
    /// Modules whose checked bindings and diagnostics were reused.
    pub reused_checked_modules: BTreeSet<String>,
}

/// A single-entry, dependency-aware compiler session for development hosts.
///
/// The session has no I/O or runtime authority: every invocation still asks
/// its caller-supplied [`ModuleLoader`] for the closed module graph. It reuses
/// parsing for byte-identical modules and rechecks only changed modules and
/// their reverse dependencies. A cache entry is replaced only after a
/// successful compilation, and an entry is never reused when the entry ID or
/// any [`CompilerOptions`] differ.
#[derive(Debug, Default)]
pub struct IncrementalCompiler {
    cached: Option<CachedCompilation>,
}

#[derive(Debug, Clone)]
struct CachedCompilation {
    entry: String,
    options: CompilerOptions,
    compilation: Compilation,
}

impl IncrementalCompiler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Compiles one entry through this session. Failed compilations leave the
    /// last successful cache entry intact, so a transient editor error cannot
    /// poison a later successful incremental build.
    pub fn compile(
        &mut self,
        entry: &str,
        loader: &dyn ModuleLoader,
        options: CompilerOptions,
    ) -> IncrementalResult {
        let cached = self
            .cached
            .as_ref()
            .filter(|cached| cached.entry == entry && cached.options == options);
        let result = compile_with_cache(entry, loader, options.clone(), cached);
        if !result.compilation.has_errors() {
            self.cached = Some(CachedCompilation {
                entry: entry.to_string(),
                options,
                compilation: result.compilation.clone(),
            });
        }
        result
    }
}

impl Project {
    fn empty(entry: impl Into<String>) -> Self {
        Self {
            entry: entry.into(),
            modules: BTreeMap::new(),
            resolutions: BTreeMap::new(),
            ambient_declaration_modules: BTreeSet::new(),
        }
    }
}

/// Parses, resolves, binds, checks, and optionally emits a closed source graph.
/// A failure at any stage leaves `output` absent, providing the library half of
/// BlueTSC's no-emit-on-error guarantee.
pub fn compile(entry: &str, loader: &dyn ModuleLoader, options: CompilerOptions) -> Compilation {
    compile_with_cache(entry, loader, options, None).compilation
}

fn compile_with_cache(
    entry: &str,
    loader: &dyn ModuleLoader,
    options: CompilerOptions,
    cached: Option<&CachedCompilation>,
) -> IncrementalResult {
    let previous_project = cached.map(|cached| &cached.compilation.project);
    let mut builder = ProjectBuilder::new(loader, previous_project, options.limits.clone());
    builder.visit(entry, 0);
    for declaration in &options.ambient_declaration_modules {
        builder.visit_ambient_declaration(declaration);
    }
    let ProjectBuilder {
        project,
        mut diagnostics,
        parsed_modules,
        reused_parsed_modules,
        ..
    } = builder;
    let project_fingerprint = fingerprint(&project, &options);

    let changed_modules = previous_project
        .map(|previous| changed_modules(previous, &project))
        .unwrap_or_else(|| project.modules.keys().cloned().collect());
    let all_modules = project.modules.keys().cloned().collect::<BTreeSet<_>>();

    if let Some(cached) = cached.filter(|_| changed_modules.is_empty()) {
        return IncrementalResult {
            compilation: cached.compilation.clone(),
            cache_hit: true,
            parsed_modules,
            reused_parsed_modules,
            rechecked_modules: BTreeSet::new(),
            reused_checked_modules: all_modules,
        };
    }

    let rechecked_modules = previous_project
        .map(|previous| affected_modules(previous, &project, &changed_modules))
        .unwrap_or_else(|| all_modules.clone());
    let previous_checked = cached.and_then(|cached| cached.compilation.checked.as_ref());

    let (checked, checker_diagnostics) = checker::check_incremental(
        &project,
        checker::CheckerPolicy {
            target: options.target,
            enforce_types: !matches!(options.runtime_policy, RuntimePolicy::TranspileOnly),
            require_declared_global_calls: options.require_declared_global_calls,
            define_class_fields: options.defines_class_fields(),
            isolated_modules: options.isolated_modules,
            module_kind: options.module_kind,
            es_module_interop: options.es_module_interop,
            jsx: options.jsx,
            experimental_decorators: options.experimental_decorators,
            jsx_factory: options.jsx_factory.clone(),
            jsx_fragment_factory: options.jsx_fragment_factory.clone(),
        },
        previous_checked,
        &rechecked_modules,
        options.limits.max_type_expansions,
    );
    diagnostics.extend(checker_diagnostics);
    diagnostics.extend(strict_boundaries::validate_descriptors(&project, &options));
    if options.source_map {
        diagnostics.extend(emitter::validate_source_map_limits(&checked, &options));
    }
    diagnostics.sort_by(|left, right| {
        (&left.span.module, left.span.start, left.code.to_string()).cmp(&(
            &right.span.module,
            right.span.start,
            right.code.to_string(),
        ))
    });
    let has_errors = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == crate::diagnostic::Severity::Error);
    let output = if has_errors {
        None
    } else {
        match emitter::emit(&checked, &project, &options) {
            Ok(output) => Some(output),
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                None
            }
        }
    };
    let debug_info = output
        .as_ref()
        .map(|_| debug_info::build(&checked, &options));
    IncrementalResult {
        compilation: Compilation {
            project,
            project_fingerprint,
            checked: Some(checked),
            debug_info,
            diagnostics,
            output,
        },
        cache_hit: false,
        parsed_modules,
        reused_parsed_modules,
        reused_checked_modules: all_modules
            .difference(&rechecked_modules)
            .cloned()
            .collect(),
        rechecked_modules,
    }
}

fn changed_modules(previous: &Project, current: &Project) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    for module_id in previous.modules.keys().chain(current.modules.keys()) {
        if previous.modules.get(module_id).map(|module| &module.source)
            != current.modules.get(module_id).map(|module| &module.source)
        {
            changed.insert(module_id.clone());
        }
    }
    for (module_id, specifier) in previous
        .resolutions
        .keys()
        .chain(current.resolutions.keys())
    {
        let key = (module_id.clone(), specifier.clone());
        if previous.resolutions.get(&key) != current.resolutions.get(&key) {
            changed.insert(module_id.clone());
        }
    }
    changed
}

fn affected_modules(
    previous: &Project,
    current: &Project,
    changed: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut reverse_dependencies = BTreeMap::<String, BTreeSet<String>>::new();
    for ((module_id, _), dependency) in previous
        .resolutions
        .iter()
        .chain(current.resolutions.iter())
    {
        reverse_dependencies
            .entry(dependency.clone())
            .or_default()
            .insert(module_id.clone());
    }

    let mut affected = changed.clone();
    let mut pending = changed.iter().cloned().collect::<VecDeque<_>>();
    while let Some(module_id) = pending.pop_front() {
        for dependent in reverse_dependencies.get(&module_id).into_iter().flatten() {
            if affected.insert(dependent.clone()) {
                pending.push_back(dependent.clone());
            }
        }
    }
    affected
        .into_iter()
        .filter(|module_id| current.modules.contains_key(module_id))
        .collect()
}

struct ProjectBuilder<'a> {
    loader: &'a dyn ModuleLoader,
    previous: Option<&'a Project>,
    project: Project,
    diagnostics: Vec<Diagnostic>,
    state: HashMap<String, VisitState>,
    limits: CompilerLimits,
    total_source_bytes: usize,
    parsed_modules: BTreeSet<String>,
    reused_parsed_modules: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Done,
}

impl<'a> ProjectBuilder<'a> {
    fn new(
        loader: &'a dyn ModuleLoader,
        previous: Option<&'a Project>,
        limits: CompilerLimits,
    ) -> Self {
        Self {
            loader,
            previous,
            project: Project::empty(""),
            diagnostics: Vec::new(),
            state: HashMap::new(),
            limits,
            total_source_bytes: 0,
            parsed_modules: BTreeSet::new(),
            reused_parsed_modules: BTreeSet::new(),
        }
    }

    fn visit(&mut self, module_id: &str, depth: usize) {
        if self.project.entry.is_empty() {
            self.project.entry = module_id.to_string();
        }
        match self.state.get(module_id) {
            Some(VisitState::Done) => return,
            // A closed cycle is a valid static module graph. Declared import
            // types seed checking; unannotated circular inference is diagnosed
            // by the checker. Emitted execution retains the module system's TDZ
            // or partial-export behavior without reading any additional source.
            Some(VisitState::Visiting) => return,
            None => {}
        }
        if depth > self.limits.max_module_depth {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                SourceSpan::new(module_id, 0, 0),
                format!(
                    "module graph exceeds the {} import-depth limit",
                    self.limits.max_module_depth
                ),
            ));
            return;
        }
        if self.state.len() >= self.limits.max_modules {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                SourceSpan::new(module_id, 0, 0),
                format!(
                    "project exceeds the {} module limit",
                    self.limits.max_modules
                ),
            ));
            return;
        }
        self.state
            .insert(module_id.to_string(), VisitState::Visiting);
        let source = match self.loader.load(module_id) {
            Ok(source) => source,
            Err(message) => {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ModuleNotFound,
                    SourceSpan::new(module_id, 0, 0),
                    message,
                ));
                self.state.insert(module_id.to_string(), VisitState::Done);
                return;
            }
        };
        self.visit_loaded_source(module_id, source, depth);
    }

    fn visit_ambient_declaration(&mut self, source: &ModuleSource) {
        let module_id = source.id.as_str();
        if !is_declaration_module(module_id) {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidDeclarationFile,
                SourceSpan::new(module_id, 0, 0),
                "ambient host declaration modules must use a `.d.ts` identity",
            ));
            return;
        }
        if self.state.contains_key(module_id) {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidDeclarationFile,
                SourceSpan::new(module_id, 0, 0),
                "ambient host declaration module duplicates a source-graph module",
            ));
            return;
        }
        if self.state.len() >= self.limits.max_modules {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                SourceSpan::new(module_id, 0, 0),
                format!(
                    "project exceeds the {} module limit",
                    self.limits.max_modules
                ),
            ));
            return;
        }
        self.state
            .insert(module_id.to_string(), VisitState::Visiting);
        self.project
            .ambient_declaration_modules
            .insert(module_id.to_string());
        self.visit_loaded_source(module_id, source.clone(), 0);
    }

    fn visit_loaded_source(&mut self, module_id: &str, source: ModuleSource, depth: usize) {
        if source.id != module_id {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::ModuleNotFound,
                SourceSpan::new(module_id, 0, 0),
                format!(
                    "loader returned `{}` while `{module_id}` was requested; module identities must be stable",
                    source.id
                ),
            ));
            self.state.insert(module_id.to_string(), VisitState::Done);
            return;
        }
        let Some(total_source_bytes) = self.total_source_bytes.checked_add(source.text.len())
        else {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                SourceSpan::new(module_id, 0, 0),
                "project source-byte accounting overflowed",
            ));
            self.state.insert(module_id.to_string(), VisitState::Done);
            return;
        };
        if total_source_bytes > self.limits.max_total_source_bytes {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                SourceSpan::new(module_id, 0, 0),
                format!(
                    "project exceeds the {} total source-byte limit",
                    self.limits.max_total_source_bytes
                ),
            ));
            self.state.insert(module_id.to_string(), VisitState::Done);
            return;
        }
        self.total_source_bytes = total_source_bytes;
        let module = if let Some(module) = self
            .previous
            .and_then(|previous| previous.modules.get(module_id))
            .filter(|module| module.source == source.text)
        {
            self.reused_parsed_modules.insert(module_id.to_string());
            module.clone()
        } else {
            self.parsed_modules.insert(module_id.to_string());
            let parsed = if self.limits.parser == ParserLimits::default() {
                parse_module(source.id.clone(), source.text)
            } else {
                parse_module_with_limits(source.id.clone(), source.text, self.limits.parser.clone())
            };
            match parsed {
                Ok(module) => module,
                Err(mut parse_diagnostics) => {
                    self.diagnostics.append(&mut parse_diagnostics);
                    self.state.insert(module_id.to_string(), VisitState::Done);
                    return;
                }
            }
        };
        for declaration in &module.declarations {
            let (specifier, span) = match declaration {
                crate::parser::Declaration::Import(import) => (&import.specifier, &import.span),
                crate::parser::Declaration::TypeExport(export) => {
                    let Some(specifier) = &export.specifier else {
                        continue;
                    };
                    (specifier, &export.span)
                }
                _ => continue,
            };
            if self.project.ambient_declaration_modules.contains(module_id) {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::InvalidDeclarationFile,
                    span.clone(),
                    "ambient host declaration modules cannot import or re-export another module",
                ));
                continue;
            }
            match self.loader.resolve(module_id, specifier) {
                Ok(resolved) => {
                    let resolution_key = (module_id.to_string(), specifier.clone());
                    if !self.project.resolutions.contains_key(&resolution_key)
                        && self.project.resolutions.len() >= self.limits.max_module_edges
                    {
                        self.diagnostics.push(Diagnostic::error(
                            DiagnosticCode::ResourceLimit,
                            span.clone(),
                            format!(
                                "module graph exceeds the {} import-edge limit",
                                self.limits.max_module_edges
                            ),
                        ));
                        continue;
                    }
                    self.project
                        .resolutions
                        .insert(resolution_key, resolved.clone());
                    self.visit(&resolved, depth.saturating_add(1));
                }
                Err(message) => self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ModuleNotFound,
                    span.clone(),
                    message,
                )),
            }
        }
        // A module that imports a namespace is parsed again with what the
        // namespace exports, so a reference to its members is one token.
        let mut imported_namespaces: Vec<(String, crate::parser::NamespaceTree)> = Vec::new();
        for declaration in &module.declarations {
            let crate::parser::Declaration::Import(import) = declaration else {
                continue;
            };
            let Some(resolved) = self
                .project
                .resolutions
                .get(&(module_id.to_string(), import.specifier.clone()))
            else {
                continue;
            };
            let Some(dependency) = self.project.modules.get(resolved) else {
                continue;
            };
            let trees = crate::parser::exported_namespace_trees(dependency);
            for binding in &import.bindings {
                if let Some(tree) = trees.get(&binding.imported) {
                    imported_namespaces.push((binding.local.clone(), tree.clone()));
                }
            }
        }
        let module = if imported_namespaces.is_empty() {
            module
        } else {
            match crate::parser::parse_module_with_namespaces(
                module.id.clone(),
                module.source.clone(),
                self.limits.parser.clone(),
                &imported_namespaces,
            ) {
                Ok(merged) => merged,
                Err(mut parse_diagnostics) => {
                    self.diagnostics.append(&mut parse_diagnostics);
                    module
                }
            }
        };
        self.project.modules.insert(module_id.to_string(), module);
        self.state.insert(module_id.to_string(), VisitState::Done);
    }
}

fn resolve_relative_module(from_module: &str, specifier: &str) -> Result<String, String> {
    if specifier.contains("://") || specifier.starts_with('/') {
        return Ok(normalize_module_id(specifier));
    }
    if !matches!(specifier, "." | "..")
        && !specifier.starts_with("./")
        && !specifier.starts_with("../")
    {
        return Err(format!(
            "bare specifier `{specifier}` is unsupported; configure the host to resolve it explicitly"
        ));
    }
    let separator = from_module.rfind('/').ok_or_else(|| {
        format!("cannot resolve `{specifier}` from non-hierarchical module `{from_module}`")
    })?;
    Ok(normalize_module_id(&format!(
        "{}{specifier}",
        &from_module[..=separator]
    )))
}

fn normalize_module_id(value: &str) -> String {
    let (prefix, path) = value
        .find("://")
        .map(|index| (&value[..index + 3], &value[index + 3..]))
        .unwrap_or(("", value));
    let leading_slash = path.starts_with('/');
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let separator = if prefix.is_empty() { "" } else { prefix };
    let slash = if leading_slash { "/" } else { "" };
    format!("{separator}{slash}{}", parts.join("/"))
}

/// A stable, non-cryptographic content fingerprint for artifact provenance and
/// cache keys.  It intentionally includes compiler policy and source identity.
pub(crate) fn fingerprint(project: &Project, options: &CompilerOptions) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    let mut add = |text: &str| {
        for byte in text.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    };
    add(LANGUAGE_VERSION);
    add(crate::standard_library::VERSION);
    add(&crate::standard_library::identity(options.target).source_fingerprint);
    add(options.target.as_str());
    add(if options.defines_class_fields() {
        "define-class-fields"
    } else {
        "assign-class-fields"
    });
    add(crate::emitter::CLASS_HELPER_V1_VERSION);
    add(crate::emitter::DECORATOR_HELPER_V1_VERSION);
    add(if options.preserve_const_enums {
        "preserve-const-enums"
    } else {
        "erase-const-enums"
    });
    add(if options.inlines_const_enums() {
        "inline-const-enums"
    } else {
        "object-const-enums"
    });
    add(options.module_kind.as_str());
    add(if options.experimental_decorators {
        "legacy-decorators"
    } else {
        "standard-decorators"
    });
    add(if options.emit_decorator_metadata {
        "decorator-metadata"
    } else {
        "no-decorator-metadata"
    });
    add(crate::emitter::LEGACY_DECORATOR_HELPER_V1_VERSION);
    add(options.jsx.map_or("jsx-none", JsxMode::as_str));
    add(options
        .jsx_factory
        .as_deref()
        .unwrap_or("jsx-factory-default"));
    add(options
        .jsx_fragment_factory
        .as_deref()
        .unwrap_or("jsx-fragment-default"));
    add(options
        .jsx_import_source
        .as_deref()
        .unwrap_or("jsx-import-source-default"));
    add(if options.es_module_interop {
        "es-module-interop"
    } else {
        "no-es-module-interop"
    });
    add(options.runtime_policy.as_str());
    add(&options.resolver_fingerprint);
    add(&options.require_declared_global_calls.to_string());
    for boundary in &options.strict_runtime_boundaries {
        add(&boundary.contract_id);
        add(&boundary.function);
        add(&boundary.span.module);
        add(&boundary.span.start.to_string());
        add(&boundary.span.end.to_string());
        add(&boundary.max_string_bytes.to_string());
        add(&boundary.helper_version);
    }
    for declaration in &options.ambient_declaration_modules {
        add(&declaration.id);
        add(&declaration.text);
    }
    add(&options.limits.max_modules.to_string());
    add(&options.limits.max_module_edges.to_string());
    add(&options.limits.max_module_depth.to_string());
    add(&options.limits.max_total_source_bytes.to_string());
    add(&options.limits.parser.max_source_bytes.to_string());
    add(&options.limits.parser.max_tokens.to_string());
    add(&options.limits.parser.max_type_depth.to_string());
    add(&options.limits.max_type_expansions.to_string());
    add(&options.limits.max_source_map_segments.to_string());
    add(if options.source_map {
        "source-map"
    } else {
        "no-source-map"
    });
    add(if options.declaration {
        "declaration"
    } else {
        "no-declaration"
    });
    for (id, module) in &project.modules {
        add(id);
        add(&module.source);
    }
    format!("bts-{hash:016x}")
}

#[cfg(test)]
mod tests;
