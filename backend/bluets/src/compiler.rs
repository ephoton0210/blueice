// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Closed-world module loading and the shared BlueTS compile pipeline.

use crate::checker;
use crate::debug_info;
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::emitter;
use crate::parser::{parse_module, Module};
use crate::{Compilation, LANGUAGE_VERSION};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

const MAX_MODULES: usize = 4_096;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePolicy {
    TranspileOnly,
    Checked,
    StrictRuntime,
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
    pub runtime_policy: RuntimePolicy,
    pub source_map: bool,
    pub declaration: bool,
    /// Host-supplied identity for resolution inputs such as an import map.
    /// This prevents an artifact/cache key from being reused under a resolver
    /// policy different from the one that selected its module graph.
    pub resolver_fingerprint: String,
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            target: EcmaTarget::Es2022,
            runtime_policy: RuntimePolicy::Checked,
            source_map: false,
            declaration: false,
            resolver_fingerprint: "relative-v1".to_string(),
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

/// The source graph after parsing and host-controlled resolution.  It contains
/// no JavaScript-runtime dependency and is reusable by check-only and build
/// callers alike.
#[derive(Debug, Clone)]
pub struct Project {
    pub entry: String,
    pub modules: BTreeMap<String, Module>,
    pub(crate) resolutions: BTreeMap<(String, String), String>,
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
    let mut builder = ProjectBuilder::new(loader, previous_project);
    builder.visit(entry);
    let ProjectBuilder {
        project,
        mut diagnostics,
        parsed_modules,
        reused_parsed_modules,
        ..
    } = builder;

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
        !matches!(options.runtime_policy, RuntimePolicy::TranspileOnly),
        previous_checked,
        &rechecked_modules,
    );
    diagnostics.extend(checker_diagnostics);
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
    let output = (!has_errors).then(|| emitter::emit(&checked, &project, &options));
    let debug_info = (!has_errors).then(|| debug_info::build(&checked, &options));
    IncrementalResult {
        compilation: Compilation {
            project,
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
    parsed_modules: BTreeSet<String>,
    reused_parsed_modules: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Done,
}

impl<'a> ProjectBuilder<'a> {
    fn new(loader: &'a dyn ModuleLoader, previous: Option<&'a Project>) -> Self {
        Self {
            loader,
            previous,
            project: Project::empty(""),
            diagnostics: Vec::new(),
            state: HashMap::new(),
            parsed_modules: BTreeSet::new(),
            reused_parsed_modules: BTreeSet::new(),
        }
    }

    fn visit(&mut self, module_id: &str) {
        if self.project.entry.is_empty() {
            self.project.entry = module_id.to_string();
        }
        match self.state.get(module_id) {
            Some(VisitState::Done) => return,
            Some(VisitState::Visiting) => {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::CircularModuleDependency,
                    SourceSpan::new(module_id, 0, 0),
                    format!("cyclic dependency includes `{module_id}`"),
                ));
                return;
            }
            None => {}
        }
        if self.state.len() >= MAX_MODULES {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                SourceSpan::new(module_id, 0, 0),
                format!("project exceeds the {MAX_MODULES} module limit"),
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
        let module = if let Some(module) = self
            .previous
            .and_then(|previous| previous.modules.get(module_id))
            .filter(|module| module.source == source.text)
        {
            self.reused_parsed_modules.insert(module_id.to_string());
            module.clone()
        } else {
            self.parsed_modules.insert(module_id.to_string());
            match parse_module(source.id.clone(), source.text) {
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
            match self.loader.resolve(module_id, specifier) {
                Ok(resolved) => {
                    self.project
                        .resolutions
                        .insert((module_id.to_string(), specifier.clone()), resolved.clone());
                    self.visit(&resolved);
                }
                Err(message) => self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ModuleNotFound,
                    span.clone(),
                    message,
                )),
            }
        }
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
    add(options.target.as_str());
    add(options.runtime_policy.as_str());
    add(&options.resolver_fingerprint);
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
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct MutableLoader {
        modules: RefCell<BTreeMap<String, ModuleSource>>,
        resolutions: RefCell<BTreeMap<(String, String), String>>,
    }

    impl MutableLoader {
        fn from(sources: impl IntoIterator<Item = ModuleSource>) -> Self {
            Self {
                modules: RefCell::new(
                    sources
                        .into_iter()
                        .map(|source| (source.id.clone(), source))
                        .collect(),
                ),
                resolutions: RefCell::new(BTreeMap::new()),
            }
        }

        fn replace(&self, source: ModuleSource) {
            self.modules.borrow_mut().insert(source.id.clone(), source);
        }

        fn remap(&self, from_module: &str, specifier: &str, module_id: &str) {
            self.resolutions.borrow_mut().insert(
                (from_module.to_string(), specifier.to_string()),
                module_id.to_string(),
            );
        }
    }

    impl ModuleLoader for MutableLoader {
        fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
            self.modules
                .borrow()
                .get(module_id)
                .cloned()
                .ok_or_else(|| format!("module `{module_id}` is not present in this loader"))
        }

        fn resolve(&self, from_module: &str, specifier: &str) -> Result<String, String> {
            self.resolutions
                .borrow()
                .get(&(from_module.to_string(), specifier.to_string()))
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| resolve_relative_module(from_module, specifier))
        }
    }

    #[test]
    fn resolves_a_relative_url_module_without_filesystem_access() {
        let loader = MapLoader::from([
            ModuleSource::new(
                "memory:///src/main.ts",
                "import type { User } from './types.ts'; const x: User = { name: 'Ada' };",
            ),
            ModuleSource::new(
                "memory:///src/types.ts",
                "export interface User { name: string }",
            ),
        ]);
        let result = compile("memory:///src/main.ts", &loader, CompilerOptions::default());
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert_eq!(result.project.modules.len(), 2);
    }

    #[test]
    fn rejects_bare_specifiers_without_host_configuration() {
        let loader = MapLoader::from([ModuleSource::new("memory:///a.ts", "import 'package';")]);
        let result = compile("memory:///a.ts", &loader, CompilerOptions::default());
        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::ModuleNotFound));
    }

    #[test]
    fn resolver_identity_changes_the_artifact_fingerprint() {
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///a.ts",
            "export const answer: number = 42;",
        )]);
        let default = compile("memory:///a.ts", &loader, CompilerOptions::default())
            .output
            .unwrap()
            .fingerprint;
        let mapped = compile(
            "memory:///a.ts",
            &loader,
            CompilerOptions {
                resolver_fingerprint: "import-map-deadbeef".to_string(),
                ..CompilerOptions::default()
            },
        )
        .output
        .unwrap()
        .fingerprint;
        assert_ne!(default, mapped);
    }

    #[test]
    fn incremental_compiler_rechecks_only_a_changed_module_and_its_dependents() {
        let loader = MutableLoader::from([
            ModuleSource::new(
                "memory:///src/main.ts",
                "import type { Left } from './left.ts';\nimport type { Right } from './right.ts';\nexport const left: Left = { id: 'left' };\nexport const right: Right = { id: 'right' };",
            ),
            ModuleSource::new(
                "memory:///src/left.ts",
                "export interface Left { id: string }",
            ),
            ModuleSource::new(
                "memory:///src/right.ts",
                "export interface Right { id: string }",
            ),
        ]);
        let mut compiler = IncrementalCompiler::new();

        let first = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());
        assert!(
            !first.compilation.has_errors(),
            "{:?}",
            first.compilation.diagnostics
        );
        assert_eq!(first.parsed_modules.len(), 3);
        assert_eq!(first.rechecked_modules.len(), 3);

        loader.replace(ModuleSource::new(
            "memory:///src/left.ts",
            "export interface Left { id: string; revision?: number }",
        ));
        let second = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());
        assert!(
            !second.compilation.has_errors(),
            "{:?}",
            second.compilation.diagnostics
        );
        assert_eq!(
            second.parsed_modules,
            BTreeSet::from(["memory:///src/left.ts".to_string()])
        );
        assert_eq!(
            second.rechecked_modules,
            BTreeSet::from([
                "memory:///src/left.ts".to_string(),
                "memory:///src/main.ts".to_string(),
            ])
        );
        assert_eq!(
            second.reused_checked_modules,
            BTreeSet::from(["memory:///src/right.ts".to_string()])
        );
        assert!(second.compilation.output.is_some());

        let third = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());
        assert!(third.cache_hit);
        assert!(third.rechecked_modules.is_empty());
        assert_eq!(third.reused_checked_modules.len(), 3);
    }

    #[test]
    fn incremental_compiler_refuses_a_cache_entry_from_another_policy() {
        let loader = MutableLoader::from([ModuleSource::new(
            "memory:///src/main.ts",
            "export const answer: number = 42;",
        )]);
        let mut compiler = IncrementalCompiler::new();
        let _ = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());

        let result = compiler.compile(
            "memory:///src/main.ts",
            &loader,
            CompilerOptions {
                runtime_policy: RuntimePolicy::StrictRuntime,
                ..CompilerOptions::default()
            },
        );
        assert!(!result.cache_hit);
        assert_eq!(result.parsed_modules.len(), 1);
        assert_eq!(result.rechecked_modules.len(), 1);
    }

    #[test]
    fn incremental_compiler_invalidates_an_importer_when_resolution_changes() {
        let loader = MutableLoader::from([
            ModuleSource::new(
                "memory:///src/main.ts",
                "import type { Model } from '@model'; export const model: Model = { id: 'model' };",
            ),
            ModuleSource::new(
                "memory:///src/first.ts",
                "export interface Model { id: string }",
            ),
            ModuleSource::new(
                "memory:///src/second.ts",
                "export interface Model { id: string; version?: number }",
            ),
        ]);
        loader.remap("memory:///src/main.ts", "@model", "memory:///src/first.ts");
        let mut compiler = IncrementalCompiler::new();
        let _ = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());

        loader.remap("memory:///src/main.ts", "@model", "memory:///src/second.ts");
        let result = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());
        assert!(
            !result.compilation.has_errors(),
            "{:?}",
            result.compilation.diagnostics
        );
        assert_eq!(
            result.rechecked_modules,
            BTreeSet::from([
                "memory:///src/main.ts".to_string(),
                "memory:///src/second.ts".to_string(),
            ])
        );
    }

    #[test]
    fn incremental_compiler_keeps_the_last_success_after_an_error() {
        let loader = MutableLoader::from([ModuleSource::new(
            "memory:///src/main.ts",
            "export const answer: number = 42;",
        )]);
        let mut compiler = IncrementalCompiler::new();
        let _ = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());

        loader.replace(ModuleSource::new(
            "memory:///src/main.ts",
            "export const answer: number = 'wrong';",
        ));
        let failed = compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());
        assert!(failed.compilation.has_errors());
        assert!(failed.compilation.output.is_none());

        loader.replace(ModuleSource::new(
            "memory:///src/main.ts",
            "export const answer: number = 42;",
        ));
        let recovered =
            compiler.compile("memory:///src/main.ts", &loader, CompilerOptions::default());
        assert!(recovered.cache_hit);
        assert!(recovered.compilation.output.is_some());
    }
}
