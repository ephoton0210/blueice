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
use std::collections::{BTreeMap, HashMap};

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
    let mut builder = ProjectBuilder::new(loader);
    builder.visit(entry);
    let project = builder.project;
    let mut diagnostics = builder.diagnostics;

    let (checked, checker_diagnostics) = checker::check(
        &project,
        !matches!(options.runtime_policy, RuntimePolicy::TranspileOnly),
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
    Compilation {
        project,
        checked: Some(checked),
        debug_info,
        diagnostics,
        output,
    }
}

struct ProjectBuilder<'a> {
    loader: &'a dyn ModuleLoader,
    project: Project,
    diagnostics: Vec<Diagnostic>,
    state: HashMap<String, VisitState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Done,
}

impl<'a> ProjectBuilder<'a> {
    fn new(loader: &'a dyn ModuleLoader) -> Self {
        Self {
            loader,
            project: Project::empty(""),
            diagnostics: Vec::new(),
            state: HashMap::new(),
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
        let module = match parse_module(source.id.clone(), source.text) {
            Ok(module) => module,
            Err(mut parse_diagnostics) => {
                self.diagnostics.append(&mut parse_diagnostics);
                self.state.insert(module_id.to_string(), VisitState::Done);
                return;
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
}
