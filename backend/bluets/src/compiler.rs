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
use std::collections::{BTreeMap, BTreeSet, VecDeque};
mod fingerprint;
mod json;
mod native_emit;
mod node_modules;
mod project_builder;
mod resolutions;
mod targets;

pub(crate) use fingerprint::fingerprint;
use project_builder::ProjectBuilder;
pub use targets::EcmaTarget;

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

/// The module system emitted JavaScript uses (TypeScript's `module`): ECMAScript
/// modules, CommonJS, or owner-loaded AMD/UMD wrappers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModuleKind {
    #[default]
    Esm,
    CommonJs,
    Amd,
    Umd,
    System,
    Node16,
    NodeNext,
}

impl ModuleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Esm => "esm",
            Self::CommonJs => "commonjs",
            Self::Amd => "amd",
            Self::Umd => "umd",
            Self::System => "system",
            Self::Node16 => "node16",
            Self::NodeNext => "nodenext",
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
    /// Independent diagnostic selection. `None` retains the legacy checking
    /// surface; explicit options never select JavaScript emit or runtime grants.
    pub checking: Option<crate::CheckingOptions>,
    pub target: EcmaTarget,
    /// Explicit whole ECMAScript library profiles. `None` follows `target`;
    /// an empty list selects no library. Typings never supply runtime authority.
    pub libraries: Option<Vec<EcmaTarget>>,
    /// Use iterator protocols when lowering iteration below ES2015.
    pub downlevel_iteration: bool,
    /// Import supported runtime helper ABIs from an owner-resolved provider.
    pub import_helpers: bool,
    /// Leave supported runtime helper ABIs to the emitted realm's owner.
    pub no_emit_helpers: bool,
    /// TypeScript's `useDefineForClassFields`: `Some(true)` defines class
    /// fields (native ES2022 fields, or `Object.defineProperty` below it),
    /// `Some(false)` assigns them in the constructor, and `None` follows the
    /// target, as TypeScript does (define for ES2022 and later).
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
    /// Preserve value module syntax and diagnose CommonJS ES declarations.
    pub verbatim_module_syntax: bool,
    /// The selected module grammar permits runtime import attributes (ESNext).
    pub import_attributes: bool,
    /// Owner-enabled JSON sources become typed data assets, never script code.
    pub resolve_json_module: bool,
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
    pub(crate) fn library_target(&self) -> Option<EcmaTarget> {
        self.libraries
            .as_ref()
            .map(|libraries| libraries.iter().copied().max())
            .unwrap_or(Some(self.target))
    }
    /// Whether a use of a `const enum` member is replaced by its value.
    /// Transpile-only compiles without types, so it cannot know the value.
    pub fn inlines_const_enums(&self) -> bool {
        !self.isolated_modules
            && !self.verbatim_module_syntax
            && self.runtime_policy != RuntimePolicy::TranspileOnly
    }

    /// Whether class fields are defined rather than assigned, once the target's
    /// default is applied.
    pub fn defines_class_fields(&self) -> bool {
        self.use_define_for_class_fields
            .unwrap_or(self.target >= EcmaTarget::Es2022)
    }
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            target: EcmaTarget::Es2022,
            libraries: None,
            downlevel_iteration: false,
            import_helpers: false,
            no_emit_helpers: false,
            checking: None,
            use_define_for_class_fields: None,
            preserve_const_enums: false,
            isolated_modules: false,
            module_kind: ModuleKind::Esm,
            verbatim_module_syntax: false,
            import_attributes: false,
            resolve_json_module: false,
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

    /// Supplies a Node file's format using only the owner's authorized inputs.
    /// File owners may inspect package manifests; the compiler performs no I/O.
    fn implied_module_kind(&self, module_id: &str) -> Result<ModuleKind, String> {
        Ok(
            if module_id.ends_with(".mts") || module_id.ends_with(".mjs") {
                ModuleKind::Esm
            } else {
                ModuleKind::CommonJs
            },
        )
    }

    fn resolve(&self, from_module: &str, specifier: &str) -> Result<String, String> {
        resolve_relative_module(from_module, specifier)
    }

    /// Resolves one static edge under its explicit package condition. The
    /// default retains the host's existing closed-world resolution policy.
    fn resolve_with_mode(
        &self,
        from_module: &str,
        specifier: &str,
        _mode: Option<crate::package_resolution::ImportMode>,
    ) -> Result<String, String> {
        self.resolve(from_module, specifier)
    }

    /// Resolves a static path directive under the owner's ordinary root policy.
    fn resolve_reference_path(&self, from_module: &str, path: &str) -> Result<String, String> {
        let relative = if path.starts_with('.') || path.starts_with('/') || path.contains("://") {
            path.to_string()
        } else {
            format!("./{path}")
        };
        self.resolve(from_module, &relative)
    }

    /// An owner must explicitly supply its authorized type-package resolver.
    /// The compiler never searches a host filesystem for a types directive.
    fn resolve_reference_types(&self, _from_module: &str, name: &str) -> Result<String, String> {
        Err(format!(
            "type definition `{name}` is not exposed by this owner"
        ))
    }

    /// Identity of owner-authorized resolution observations after the graph
    /// has been loaded. File hosts include absent candidates, canonical paths
    /// and manifest content here; the compiler does not acquire any I/O grant.
    fn resolution_fingerprint(&self) -> String {
        String::new()
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
    [".d.ts", ".d.mts", ".d.cts"]
        .iter()
        .any(|extension| module_id.ends_with(extension))
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
    pub(crate) module_kinds: BTreeMap<String, ModuleKind>,
    pub(crate) resolutions: BTreeMap<(String, String), String>,
    mode_resolutions: BTreeMap<(String, String, crate::package_resolution::ImportMode), String>,
    // Static namespace keys never appear as runtime/physical source targets.
    pub(crate) ambient_resolutions: BTreeMap<
        (
            String,
            String,
            Option<crate::package_resolution::ImportMode>,
        ),
        String,
    >,
    pub(crate) json_modules: BTreeMap<String, crate::Type>,
    pub(crate) augmentation_resolutions: BTreeMap<(String, String), String>,
    pub(crate) reference_resolutions: BTreeMap<(String, String, String), String>,
    pub(crate) referenced_declaration_modules: BTreeSet<String>,
    pub(crate) referenced_libraries: BTreeSet<String>,
    resolution_fingerprint: String,
    pub(crate) ambient_declaration_modules: BTreeSet<String>,
    failed_sources: BTreeMap<String, String>,
}

impl Project {
    /// Node16 and NodeNext default to module detection for runtime inputs;
    /// declaration files retain their syntax-based global/module distinction.
    pub(crate) fn is_external_module(&self, module: &Module) -> bool {
        module.is_external_module()
            || !is_declaration_module(&module.id) && self.module_kinds.contains_key(&module.id)
    }

    /// Returns an owner-selected Node format, or the caller's fixed format.
    pub fn module_kind(&self, module_id: &str, fallback: ModuleKind) -> ModuleKind {
        self.module_kinds
            .get(module_id)
            .copied()
            .unwrap_or(fallback)
    }
    /// Returns input already supplied by the owner, including a source whose
    /// parsing failed. This performs no I/O and does not resolve new inputs.
    pub fn source(&self, module_id: &str) -> Option<&str> {
        self.modules
            .get(module_id)
            .map(|module| module.source.as_str())
            .or_else(|| self.failed_sources.get(module_id).map(String::as_str))
    }
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
            module_kinds: BTreeMap::new(),
            resolutions: BTreeMap::new(),
            mode_resolutions: BTreeMap::new(),
            ambient_resolutions: BTreeMap::new(),
            json_modules: BTreeMap::new(),
            resolution_fingerprint: String::new(),
            ambient_declaration_modules: BTreeSet::new(),
            augmentation_resolutions: BTreeMap::new(),
            reference_resolutions: BTreeMap::new(),
            referenced_declaration_modules: BTreeSet::new(),
            referenced_libraries: BTreeSet::new(),
            failed_sources: BTreeMap::new(),
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
    let performance = crate::performance::Session::new();
    let graph_started = std::time::Instant::now();
    let previous_project = cached.map(|cached| &cached.compilation.project);
    let mut builder = ProjectBuilder::new(loader, previous_project, options.limits.clone());
    builder.resolve_json_module = options.resolve_json_module;
    builder.module_kind = options.module_kind;
    builder.visit(entry, 0);
    builder.resolve_helper_providers(&options);
    for declaration in &options.ambient_declaration_modules {
        builder.visit_ambient_declaration(declaration);
    }
    builder.resolve_ambient_imports();
    builder.project.resolution_fingerprint = loader.resolution_fingerprint();
    let ProjectBuilder {
        project,
        mut diagnostics,
        parsed_modules,
        reused_parsed_modules,
        ..
    } = builder;
    diagnostics.extend(node_modules::validate(&project, &options));
    let project_fingerprint = fingerprint(&project, &options);
    let graph_time = graph_started.elapsed();

    let changed_modules = previous_project
        .map(|previous| changed_modules(previous, &project))
        .unwrap_or_else(|| project.modules.keys().cloned().collect());
    let all_modules = project.modules.keys().cloned().collect::<BTreeSet<_>>();

    if let Some(cached) = cached.filter(|cached| {
        changed_modules.is_empty() && cached.compilation.project_fingerprint == project_fingerprint
    }) {
        let mut compilation = cached.compilation.clone();
        compilation.performance = performance.finish(
            &project,
            compilation.checked.as_ref(),
            graph_time,
            std::time::Duration::ZERO,
            std::time::Duration::ZERO,
        );
        return IncrementalResult {
            compilation,
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

    let check_started = std::time::Instant::now();
    if options.runtime_policy == RuntimePolicy::StrictRuntime && !project.json_modules.is_empty() {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::UnsupportedSyntax,
            SourceSpan::new(entry, 0, 0),
            "strict-runtime JSON assets require a supported runtime profile",
        ));
    }
    let (checked, checker_diagnostics) = checker::check_incremental(
        &project,
        checker::CheckerPolicy {
            target: options.target,
            library_target: options
                .library_target()
                .into_iter()
                .chain(
                    project
                        .referenced_libraries
                        .iter()
                        .filter_map(|name| EcmaTarget::parse(name)),
                )
                .max(),
            checking: options.checking,
            enforce_types: !matches!(options.runtime_policy, RuntimePolicy::TranspileOnly),
            require_declared_global_calls: options.require_declared_global_calls,
            define_class_fields: options.defines_class_fields(),
            isolated_modules: options.isolated_modules || options.verbatim_module_syntax,
            module_kind: options.module_kind,
            import_attributes: options.import_attributes,
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
    let check_time = check_started.elapsed();
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
    let emit_started = std::time::Instant::now();
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
    let emit_time = emit_started.elapsed();
    crate::diagnostic::positions::attach(&project, &mut diagnostics);
    crate::diagnostic::rendered::attach(&project, &mut diagnostics);
    crate::diagnostic::related::attach(&project, &mut diagnostics);
    let debug_info = output
        .as_ref()
        .map(|_| debug_info::build(&checked, &options));
    let performance =
        performance.finish(&project, Some(&checked), graph_time, check_time, emit_time);
    IncrementalResult {
        compilation: Compilation {
            project,
            project_fingerprint,
            checked: Some(checked),
            debug_info,
            diagnostics,
            output,
            performance,
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
    for module_id in previous
        .modules
        .keys()
        .chain(current.modules.keys())
        .chain(previous.failed_sources.keys())
        .chain(current.failed_sources.keys())
    {
        if previous.source(module_id) != current.source(module_id)
            || previous.module_kinds.get(module_id) != current.module_kinds.get(module_id)
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
    for (module_id, specifier, mode) in previous
        .mode_resolutions
        .keys()
        .chain(current.mode_resolutions.keys())
    {
        let key = (module_id.clone(), specifier.clone(), *mode);
        if previous.mode_resolutions.get(&key) != current.mode_resolutions.get(&key) {
            changed.insert(module_id.clone());
        }
    }
    for (module, specifier, mode) in previous
        .ambient_resolutions
        .keys()
        .chain(current.ambient_resolutions.keys())
    {
        let key = (module.clone(), specifier.clone(), *mode);
        if previous.ambient_resolutions.get(&key) != current.ambient_resolutions.get(&key) {
            changed.insert(module.clone());
        }
    }
    for key in previous
        .augmentation_resolutions
        .keys()
        .chain(current.augmentation_resolutions.keys())
    {
        if previous.augmentation_resolutions.get(key) != current.augmentation_resolutions.get(key) {
            changed.insert(key.0.clone());
        }
    }
    for key in previous
        .reference_resolutions
        .keys()
        .chain(current.reference_resolutions.keys())
    {
        if previous.reference_resolutions.get(key) != current.reference_resolutions.get(key) {
            changed.insert(key.0.clone());
        }
    }
    changed
}

fn affected_modules(
    previous: &Project,
    current: &Project,
    changed: &BTreeSet<String>,
) -> BTreeSet<String> {
    if previous.referenced_libraries != current.referenced_libraries {
        return current.modules.keys().cloned().collect();
    }
    fn augments_global(declarations: &[crate::parser::Declaration]) -> bool {
        declarations.iter().any(|declaration| match declaration {
            crate::parser::Declaration::Ambient(item) => {
                item.specifier.is_none() || augments_global(&item.body)
            }
            crate::parser::Declaration::Namespace(item) => augments_global(&item.body),
            _ => false,
        })
    }
    let mut reverse_dependencies = BTreeMap::<String, BTreeSet<String>>::new();
    for (module_id, dependency) in previous
        .resolution_edges()
        .chain(current.resolution_edges())
    {
        reverse_dependencies
            .entry(dependency.to_string())
            .or_default()
            .insert(module_id.to_string());
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
    // An augmentation's consumers need not import the module supplying it.
    // Include its transitive imports above, then invalidate every consumer if
    // either graph contains an affected global augmentation or a referenced
    // script declaration that supplies names to otherwise unconnected modules.
    if affected.iter().any(|id| {
        [previous, current].iter().any(|project| {
            project.modules.get(id).is_some_and(|module| {
                augments_global(&module.declarations)
                    || (project.referenced_declaration_modules.contains(id)
                        && !crate::parser::has_module_syntax(&module.declarations))
            })
        })
    }) {
        return current.modules.keys().cloned().collect();
    }
    affected
        .into_iter()
        .filter(|module_id| current.modules.contains_key(module_id))
        .collect()
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
#[cfg(test)]
mod tests;
