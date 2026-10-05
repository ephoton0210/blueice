// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The standalone BlueTSC command-line front end.

use blueice_bluets::package_resolution::{
    ImportMode, ModuleResolution, OsPackageFs, PackageResolver, PackageResolverConfig,
    ResolveError, RESOLUTION_VERSION,
};
use blueice_bluets::remote_declarations::{
    DeclarationCache, RemoteDeclarationSource, RemoteFetcher, RemoteLimits,
};
use blueice_bluets::{
    compile, BuildArtifact, CompilerLimits, CompilerOptions, EcmaTarget, JsxMode, ModuleKind,
    ModuleLoader, ModuleSource, RuntimePolicy, SourceSpan, StrictRuntimeBoundary,
};
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "bluetsc/config.rs"]
mod config;
use config::*;

const RUNTIME_HELPER_V1_FILE: &str = "bluets.runtime-helper.v1.mjs";
const RUNTIME_HELPER_V1_VERSION: &str = "bluets-runtime-helper-v1";
const RUNTIME_HELPER_V1_SOURCE: &str = include_str!("../runtime_helper_v1.mjs");

fn main() -> ExitCode {
    let args = match parse_args(env::args().skip(1)) {
        Ok(args) => args,
        Err(message) if message == "help requested" => {
            println!("{}", usage());
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("bluetsc: {message}\n\n{}", usage());
            return ExitCode::FAILURE;
        }
    };
    let invocation = match resolve_invocation(args.input) {
        Ok(invocation) => invocation,
        Err(message) => {
            eprintln!("bluetsc: {message}");
            return ExitCode::FAILURE;
        }
    };
    if args.command == Command::FetchDeclarations {
        return run_fetch_declarations(&invocation);
    }
    if args.command == Command::Build && invocation.out_dir.is_none() {
        eprintln!("bluetsc: build requires --out-dir <directory> or config outDir");
        return ExitCode::FAILURE;
    }
    if args.command == Command::Build
        && invocation.options.runtime_policy == RuntimePolicy::StrictRuntime
        && invocation.options.strict_runtime_boundaries.is_empty()
    {
        eprintln!(
            "bluetsc: strict-runtime build requires owner-selected strictBoundaries in config"
        );
        return ExitCode::FAILURE;
    }
    let loader = FileLoader {
        root: invocation.root.clone(),
        imports: invocation.imports.clone(),
        extra_roots: invocation
            .packages
            .as_ref()
            .map(|settings| settings.extra_roots.clone())
            .unwrap_or_default(),
        packages: invocation
            .packages
            .as_ref()
            .map(|settings| package_resolver(&invocation.root, settings)),
        remote: match invocation.remote.as_ref() {
            Some(settings) => {
                let cache = DeclarationCache::new(
                    settings.cache_directory.clone(),
                    RemoteLimits::default(),
                );
                // Compilation reads only verified cache entries; a missing one
                // names the explicit fetch step instead of fetching.
                match cache.load_all(&settings.sources) {
                    Ok(_) => Some((cache, settings.sources.clone())),
                    Err(error) => {
                        eprintln!("bluetsc: {error}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            None => None,
        },
        import_mode: if invocation.options.module_kind == ModuleKind::CommonJs {
            ImportMode::Require
        } else {
            ImportMode::Import
        },
    };
    let summary = compile_entries(
        &invocation.root,
        &invocation.entries,
        &loader,
        invocation.options.clone(),
    );
    if summary.has_errors {
        return ExitCode::FAILURE;
    }
    if args.command == Command::Check {
        println!(
            "checked {} entry point(s), {} module(s), fingerprint {}",
            invocation.entries.len(),
            summary.module_count,
            summary.fingerprint,
        );
        return ExitCode::SUCCESS;
    }
    let metadata = build_metadata(
        &invocation,
        &summary,
        package_manifest(&invocation, &loader),
    );
    let out_dir = invocation
        .out_dir
        .as_deref()
        .expect("build argument parsing requires an output directory");
    match publish_build(
        &invocation.root,
        out_dir,
        &summary.artifacts,
        &summary.declaration_modules,
        &metadata,
    ) {
        Ok(()) => {
            println!(
                "built {} entry point(s), {} module(s), fingerprint {}",
                invocation.entries.len(),
                summary.artifacts.len() + summary.declaration_modules.len(),
                summary.fingerprint,
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bluetsc: failed to publish build: {error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug)]
struct CompileSummary {
    artifacts: BTreeMap<String, BuildArtifact>,
    declaration_modules: BTreeMap<String, String>,
    fingerprint: String,
    module_count: usize,
    has_errors: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildMetadata {
    language_version: &'static str,
    fingerprint: String,
    target: &'static str,
    /// Whether class fields are defined rather than assigned, with the
    /// target's default applied.
    use_define_for_class_fields: bool,
    /// Whether a `const enum`'s object is emitted and whether its uses are
    /// replaced by values.
    preserve_const_enums: bool,
    inline_const_enums: bool,
    module: &'static str,
    es_module_interop: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    jsx: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jsx_factory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jsx_fragment_factory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jsx_import_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    package_resolution: Option<PackageResolutionManifest>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    remote_declarations: Vec<RemoteDeclarationManifest>,
    /// The version of the private-name helper text an artifact may embed.
    class_helper_version: &'static str,
    /// The version of the decorator helper text an artifact may embed.
    decorator_helper_version: &'static str,
    legacy_decorator_helper_version: &'static str,
    experimental_decorators: bool,
    emit_decorator_metadata: bool,
    runtime_policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_helper: Option<RuntimeHelperIdentity>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    strict_boundaries: Vec<StrictBoundaryManifest>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    strict_artifacts: Vec<StrictArtifactIdentity>,
    source_map: bool,
    declaration: bool,
    entries: Vec<String>,
    declaration_modules: Vec<String>,
    imports: BTreeMap<String, String>,
    #[serde(skip)]
    has_configured_imports: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RemoteDeclarationManifest {
    specifier: String,
    url: String,
    sha256: String,
}

/// The owner's installed-package settings and what resolution actually used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageResolutionManifest {
    version: &'static str,
    module_resolution: &'static str,
    custom_conditions: Vec<String>,
    /// How many extra authorized package roots beyond the project root.
    external_roots: usize,
    /// Configuration plus every file the resolution depended on.
    fingerprint: String,
    packages: Vec<ResolvedPackageManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolvedPackageManifest {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    types_package: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeHelperIdentity {
    version: &'static str,
    file: &'static str,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct StrictBoundaryManifest {
    contract_id: String,
    module: String,
    function: String,
    source_start: usize,
    source_end: usize,
    max_string_bytes: usize,
    helper_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct StrictArtifactIdentity {
    module: String,
    emitted_javascript_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_map_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    declaration_sha256: Option<String>,
}

impl From<&StrictRuntimeBoundary> for StrictBoundaryManifest {
    fn from(boundary: &StrictRuntimeBoundary) -> Self {
        Self {
            contract_id: boundary.contract_id.clone(),
            module: boundary.span.module.clone(),
            function: boundary.function.clone(),
            source_start: boundary.span.start,
            source_end: boundary.span.end,
            max_string_bytes: boundary.max_string_bytes,
            helper_version: boundary.helper_version.clone(),
        }
    }
}

fn runtime_helper_v1_identity() -> RuntimeHelperIdentity {
    RuntimeHelperIdentity {
        version: RUNTIME_HELPER_V1_VERSION,
        file: RUNTIME_HELPER_V1_FILE,
        sha256: sha256_label(RUNTIME_HELPER_V1_SOURCE.as_bytes()),
    }
}

fn sha256_label(bytes: &[u8]) -> String {
    let hash = digest(&SHA256, bytes);
    format!(
        "sha256:{}",
        hash.as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn strict_artifact_inventory(
    artifacts: &BTreeMap<String, BuildArtifact>,
) -> Vec<StrictArtifactIdentity> {
    artifacts
        .iter()
        .map(|(module, artifact)| StrictArtifactIdentity {
            module: module.clone(),
            emitted_javascript_sha256: sha256_label(artifact.javascript.as_bytes()),
            source_map_sha256: artifact
                .source_map
                .as_ref()
                .map(|map| sha256_label(map.to_json().as_bytes())),
            declaration_sha256: artifact
                .declaration
                .as_ref()
                .map(|declaration| sha256_label(declaration.as_bytes())),
        })
        .collect()
}

fn compile_entries(
    root: &Path,
    entries: &[PathBuf],
    loader: &FileLoader,
    options: CompilerOptions,
) -> CompileSummary {
    let mut artifacts = BTreeMap::new();
    let mut declaration_modules = BTreeMap::new();
    let mut modules = BTreeSet::new();
    let mut fingerprints = Vec::new();
    let mut has_errors = false;
    for entry in entries {
        let result = compile(&project_module_id(root, entry), loader, options.clone());
        for diagnostic in &result.diagnostics {
            eprintln!(
                "{}:{}:{}: {}: {}",
                diagnostic.span.module,
                diagnostic.span.start,
                diagnostic.span.end,
                diagnostic.code,
                diagnostic.message
            );
        }
        has_errors |= result.has_errors();
        modules.extend(result.project.modules.keys().cloned());
        if let Some(output) = result.output {
            fingerprints.push(output.fingerprint);
            for (module_id, artifact) in output.artifacts {
                artifacts.entry(module_id).or_insert(artifact);
            }
            for (module_id, source) in output.declaration_modules {
                declaration_modules.entry(module_id).or_insert(source);
            }
        }
    }
    fingerprints.sort();
    CompileSummary {
        artifacts,
        declaration_modules,
        fingerprint: fingerprint_entries(&fingerprints),
        module_count: modules.len(),
        has_errors,
    }
}

fn build_metadata(
    invocation: &Invocation,
    summary: &CompileSummary,
    package_resolution: Option<PackageResolutionManifest>,
) -> BuildMetadata {
    let entries = invocation
        .entries
        .iter()
        .map(|entry| {
            output_module_path(
                &invocation.root,
                entry,
                invocation.options.jsx == Some(JsxMode::Preserve),
            )
        })
        .collect();
    let imports = invocation
        .imports
        .iter()
        .filter(|(_, target)| !is_declaration_path(target))
        .map(|(specifier, target)| {
            let relative = target
                .strip_prefix(&invocation.root)
                .expect("validated import-map target is beneath project root");
            let emitted = if target.is_dir() {
                format!("{}/", output_path(relative))
            } else {
                output_path(&relative.with_extension(output_extension(
                    relative,
                    invocation.options.jsx == Some(JsxMode::Preserve),
                )))
            };
            (specifier.clone(), format!("./{emitted}"))
        })
        .collect();
    let declaration_modules = summary.declaration_modules.keys().cloned().collect();
    BuildMetadata {
        language_version: blueice_bluets::LANGUAGE_VERSION,
        fingerprint: summary.fingerprint.clone(),
        target: invocation.options.target.as_str(),
        use_define_for_class_fields: invocation.options.defines_class_fields(),
        preserve_const_enums: invocation.options.preserve_const_enums,
        inline_const_enums: invocation.options.inlines_const_enums(),
        module: invocation.options.module_kind.as_str(),
        es_module_interop: invocation.options.es_module_interop,
        jsx: invocation.options.jsx.map(JsxMode::as_str),
        jsx_factory: invocation.options.jsx_factory.clone(),
        jsx_fragment_factory: invocation.options.jsx_fragment_factory.clone(),
        jsx_import_source: invocation.options.jsx_import_source.clone(),
        package_resolution,
        remote_declarations: invocation
            .remote
            .iter()
            .flat_map(|settings| settings.sources.iter())
            .map(|source| RemoteDeclarationManifest {
                specifier: source.specifier.clone(),
                url: source.url.clone(),
                sha256: source.sha256.clone(),
            })
            .collect(),
        class_helper_version: blueice_bluets::CLASS_HELPER_V1_VERSION,
        decorator_helper_version: blueice_bluets::DECORATOR_HELPER_V1_VERSION,
        legacy_decorator_helper_version: blueice_bluets::LEGACY_DECORATOR_HELPER_V1_VERSION,
        experimental_decorators: invocation.options.experimental_decorators,
        emit_decorator_metadata: invocation.options.emit_decorator_metadata,
        runtime_policy: invocation.options.runtime_policy.as_str(),
        runtime_helper: (invocation.options.runtime_policy == RuntimePolicy::StrictRuntime)
            .then(runtime_helper_v1_identity),
        strict_boundaries: invocation
            .options
            .strict_runtime_boundaries
            .iter()
            .map(StrictBoundaryManifest::from)
            .collect(),
        strict_artifacts: if invocation.options.runtime_policy == RuntimePolicy::StrictRuntime {
            strict_artifact_inventory(&summary.artifacts)
        } else {
            Vec::new()
        },
        source_map: invocation.options.source_map,
        declaration: invocation.options.declaration,
        entries,
        declaration_modules,
        imports,
        has_configured_imports: !invocation.imports.is_empty(),
    }
}

fn output_module_path(root: &Path, module: &Path, preserve_jsx: bool) -> String {
    let relative = module
        .strip_prefix(root)
        .expect("validated entry is beneath project root");
    output_path(&relative.with_extension(output_extension(relative, preserve_jsx)))
}

/// What an emitted module is called: `.js`, or `.jsx` for a `.tsx` module whose
/// JSX is preserved.
fn output_extension(source: &Path, preserve_jsx: bool) -> &'static str {
    if preserve_jsx
        && source
            .extension()
            .is_some_and(|extension| extension == "tsx")
    {
        "jsx"
    } else {
        "js"
    }
}

fn output_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn remote_fingerprint(sources: &[RemoteDeclarationSource]) -> String {
    let mut text = String::new();
    for source in sources {
        text.push_str(&format!(
            "{}\0{}\0{}\n",
            source.specifier, source.url, source.sha256
        ));
    }
    format!(
        "+remote-{}",
        &blueice_bluets::remote_declarations::sha256_hex(text.as_bytes())[..16]
    )
}

/// The explicit network step. Only the owner's `https` URLs are ever requested:
/// no redirects, a total timeout, and a read that stops one byte past the limit.
struct UreqFetcher;

impl RemoteFetcher for UreqFetcher {
    fn fetch(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .https_only(true)
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build()
            .into();
        let mut response = agent.get(url).call().map_err(|error| error.to_string())?;
        response
            .body_mut()
            .with_config()
            .limit(max_bytes as u64 + 1)
            .read_to_vec()
            .map_err(|error| error.to_string())
    }
}

fn run_fetch_declarations(invocation: &Invocation) -> ExitCode {
    let Some(remote) = &invocation.remote else {
        eprintln!("bluetsc: config has no remoteDeclarations to fetch");
        return ExitCode::FAILURE;
    };
    let cache = DeclarationCache::new(remote.cache_directory.clone(), RemoteLimits::default());
    match cache.fetch_missing(&remote.sources, &UreqFetcher) {
        Ok(report) => {
            println!(
                "fetched {} declaration source(s), {} already cached",
                report.fetched.len(),
                report.already_cached.len()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bluetsc: {error}");
            ExitCode::FAILURE
        }
    }
}

fn package_resolver(root: &Path, settings: &PackageSettings) -> PackageResolver<OsPackageFs> {
    let mut roots = vec![root.to_path_buf()];
    roots.extend(settings.extra_roots.iter().cloned());
    PackageResolver::new(
        OsPackageFs,
        PackageResolverConfig {
            roots,
            resolution: settings.resolution,
            custom_conditions: settings.custom_conditions.clone(),
        },
    )
}

fn fingerprint_entries(fingerprints: &[String]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for fingerprint in fingerprints {
        for byte in fingerprint.bytes().chain(std::iter::once(0xff)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("bts-project-{hash:016x}")
}

fn package_manifest(
    invocation: &Invocation,
    loader: &FileLoader,
) -> Option<PackageResolutionManifest> {
    let settings = invocation.packages.as_ref()?;
    let resolver = loader.packages.as_ref()?;
    let mut packages: Vec<ResolvedPackageManifest> = resolver
        .resolved_packages()
        .into_iter()
        .map(|package| ResolvedPackageManifest {
            name: package.name,
            version: package.version,
            types_package: package.from_types_package,
        })
        .collect();
    packages.sort_by(|left, right| (&left.name, &left.version).cmp(&(&right.name, &right.version)));
    Some(PackageResolutionManifest {
        version: RESOLUTION_VERSION,
        module_resolution: settings.resolution.as_str(),
        custom_conditions: settings.custom_conditions.clone(),
        external_roots: settings.extra_roots.len(),
        fingerprint: resolver.fingerprint(),
        packages,
    })
}

fn import_map_fingerprint(root: &Path, imports: &BTreeMap<String, PathBuf>) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for (specifier, target) in imports {
        let target = target
            .strip_prefix(root)
            .expect("validated import-map target is beneath project root");
        let target = output_path(target);
        for byte in specifier
            .bytes()
            .chain(std::iter::once(0))
            .chain(target.bytes())
            .chain(std::iter::once(0xff))
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("import-map-{hash:016x}")
}

struct FileLoader {
    root: PathBuf,
    imports: BTreeMap<String, PathBuf>,
    /// Authorized package roots beyond `root`; a file under one has the module
    /// id `@external/<index>/<path inside it>`.
    extra_roots: Vec<PathBuf>,
    /// Present only when the owner configured a `moduleResolution`.
    packages: Option<PackageResolver<OsPackageFs>>,
    /// The verified cache and the pinned sources whose specifiers resolve to it.
    remote: Option<(DeclarationCache, Vec<RemoteDeclarationSource>)>,
    import_mode: ImportMode,
}

impl ModuleLoader for FileLoader {
    fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
        if let Some(pin) = module_id
            .strip_prefix("@remote/")
            .and_then(|rest| rest.strip_suffix(".d.ts"))
        {
            let (cache, sources) = self.remote.as_ref().ok_or_else(|| {
                format!("module `{module_id}` is not an authorized remote declaration")
            })?;
            if !sources.iter().any(|source| source.sha256 == pin) {
                return Err(format!(
                    "module `{module_id}` is not an authorized remote declaration"
                ));
            }
            // Re-verified against its pin on every read.
            let text = cache
                .read(pin)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("remote declaration {pin} is not cached"))?;
            return Ok(ModuleSource::new(module_id, text));
        }
        let path = self.source_path(module_id)?;
        if !matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("ts" | "tsx")
        ) {
            return Err(format!(
                "module `{module_id}` is not a supported .ts, .tsx, or .d.ts source file"
            ));
        }
        let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        Ok(ModuleSource::new(module_id, text))
    }

    fn resolve(&self, from_module: &str, specifier: &str) -> Result<String, String> {
        let is_relative = matches!(specifier, "." | "..")
            || specifier.starts_with("./")
            || specifier.starts_with("../");
        if from_module.starts_with("@remote/") {
            return Err(format!(
                "remote declaration `{from_module}` cannot import `{specifier}`: a remote \
                 declaration must be self-contained"
            ));
        }
        if let Some((_, sources)) = &self.remote {
            if let Some(source) = sources.iter().find(|source| source.specifier == specifier) {
                return Ok(format!("@remote/{}.d.ts", source.sha256));
            }
        }
        if let Some(packages) = &self.packages {
            let from_directory = self
                .source_path(from_module)?
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| format!("module `{from_module}` has no directory"))?;
            let has_source_extension = specifier.ends_with(".ts") || specifier.ends_with(".tsx");
            // A relative import without a TypeScript extension is probed the way
            // TypeScript does (`./x` -> `./x.ts`, `./x.d.ts`, `./x/index.ts`).
            if is_relative && !has_source_extension {
                let found = packages
                    .resolve_relative(&from_directory, specifier)
                    .map_err(|error| {
                        format!("cannot resolve `{specifier}` from `{from_module}`: {error}")
                    })?;
                // A package's own files stay marked as package files.
                return if blueice_bluets::is_external_library_module(from_module) {
                    self.package_module_id(&found.path)
                } else {
                    self.module_id(&found.path)
                };
            }
            if !is_relative
                && !self.imports.contains_key(specifier)
                && !self.matches_import_prefix(specifier)
            {
                return match packages.resolve(&from_directory, specifier, self.import_mode) {
                    Ok(found) => self.package_module_id(&found.path),
                    Err(
                        error @ (ResolveError::NotFound { .. }
                        | ResolveError::JavaScriptOnly { .. }),
                    ) => Err(format!(
                        "cannot resolve `{specifier}` from `{from_module}`: {error}"
                    )),
                    Err(error) => Err(format!("`{specifier}` from `{from_module}`: {error}")),
                };
            }
        }
        let candidate = if is_relative {
            self.root.join(
                Path::new(from_module)
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(specifier),
            )
        } else {
            self.resolve_import_map(specifier)?
        };
        let resolved = fs::canonicalize(&candidate).map_err(|error| {
            format!("cannot resolve `{specifier}` from `{from_module}`: {error}")
        })?;
        if !resolved.starts_with(&self.root) {
            return Err(format!(
                "specifier `{specifier}` resolves outside the declared project root"
            ));
        }
        self.module_id(&resolved)
    }
}

impl FileLoader {
    /// The module id of a file found by package resolution: ordinary, except
    /// that a package reached through a symlink to a directory with no
    /// `node_modules` in its path is still marked as an installed package.
    /// A relative import from inside a package stays in its marking.
    fn package_module_id(&self, path: &Path) -> Result<String, String> {
        let id = self.module_id(path)?;
        Ok(if blueice_bluets::is_external_library_module(&id) {
            id
        } else {
            format!("@package/{id}")
        })
    }

    fn matches_import_prefix(&self, specifier: &str) -> bool {
        self.imports
            .keys()
            .any(|prefix| prefix.ends_with('/') && specifier.starts_with(prefix.as_str()))
    }

    fn source_path(&self, module_id: &str) -> Result<PathBuf, String> {
        let module_path = Path::new(module_id);
        if module_path.is_absolute()
            || module_path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(format!(
                "module `{module_id}` escapes the declared project root"
            ));
        }
        let module_id = module_id.strip_prefix("@package/").unwrap_or(module_id);
        let module_path = Path::new(module_id);
        let (base, relative) = match module_id
            .strip_prefix("@external/")
            .and_then(|rest| rest.split_once('/'))
        {
            Some((index, relative)) => {
                let base = index
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| self.extra_roots.get(index))
                    .ok_or_else(|| {
                        format!("module `{module_id}` names no authorized package root")
                    })?;
                (base, Path::new(relative))
            }
            None => (&self.root, module_path),
        };
        let path = fs::canonicalize(base.join(relative))
            .map_err(|error| format!("cannot read module `{module_id}`: {error}"))?;
        if !path.starts_with(base) {
            return Err(format!(
                "module `{module_id}` escapes the declared project root"
            ));
        }
        Ok(path)
    }

    fn module_id(&self, path: &Path) -> Result<String, String> {
        if let Ok(relative) = path.strip_prefix(&self.root) {
            return Ok(output_path(relative));
        }
        for (index, root) in self.extra_roots.iter().enumerate() {
            if let Ok(relative) = path.strip_prefix(root) {
                return Ok(format!("@external/{index}/{}", output_path(relative)));
            }
        }
        Err(format!(
            "module {} escapes the declared project root",
            path.display()
        ))
    }

    fn resolve_import_map(&self, specifier: &str) -> Result<PathBuf, String> {
        if let Some(target) = self.imports.get(specifier) {
            return Ok(target.clone());
        }
        let Some((prefix, target)) = self
            .imports
            .iter()
            .filter(|(prefix, _)| prefix.ends_with('/') && specifier.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
        else {
            return Err(format!(
                "bare specifier `{specifier}` is unsupported; add an exact or trailing-slash `imports` mapping"
            ));
        };
        Ok(target.join(&specifier[prefix.len()..]))
    }
}

fn publish_build(
    root: &Path,
    out_dir: &Path,
    artifacts: &std::collections::BTreeMap<String, blueice_bluets::BuildArtifact>,
    declaration_modules: &std::collections::BTreeMap<String, String>,
    metadata: &BuildMetadata,
) -> io::Result<()> {
    let expected_helper = (metadata.runtime_policy == RuntimePolicy::StrictRuntime.as_str())
        .then(runtime_helper_v1_identity);
    if metadata.runtime_helper != expected_helper {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "build metadata does not bind the exact runtime helper",
        ));
    }
    strict_publish::verify(metadata, artifacts, declaration_modules)?;
    let output = absolute_path(out_dir)?;
    if output == root || (output.exists() && fs::canonicalize(&output)? == root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "output directory must not replace the project root",
        ));
    }
    ensure_output_does_not_contain_sources(&output, root, artifacts, declaration_modules)?;
    let parent = output
        .parent()
        .ok_or_else(|| io::Error::other("output directory has no parent"))?;
    fs::create_dir_all(parent)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage = parent.join(format!(".bluetsc-stage-{}-{nonce}", std::process::id()));
    fs::create_dir(&stage)?;
    let write_result = (|| -> io::Result<()> {
        if metadata.runtime_policy == RuntimePolicy::StrictRuntime.as_str() {
            fs::write(stage.join(RUNTIME_HELPER_V1_FILE), RUNTIME_HELPER_V1_SOURCE)?;
        }
        for (module_id, artifact) in artifacts {
            let module_path = Path::new(module_id);
            let relative = artifact_relative_path(root, module_path, module_id)?;
            let extension = output_extension(&relative, metadata.jsx == Some("preserve"));
            let output_relative = relative.with_extension(extension);
            let js_path = stage.join(&output_relative);
            let js_parent = js_path
                .parent()
                .ok_or_else(|| io::Error::other("artifact path has no parent"))?;
            fs::create_dir_all(js_parent)?;
            let mut javascript = artifact.javascript.clone();
            if artifact.source_map.is_some() {
                let map_name = js_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| format!("{name}.map"))
                    .ok_or_else(|| io::Error::other("artifact filename is not UTF-8"))?;
                javascript.push_str(&format!("\n//# sourceMappingURL={map_name}\n"));
            }
            fs::write(&js_path, javascript)?;
            if let Some(source_map) = &artifact.source_map {
                fs::write(
                    js_path.with_extension(format!("{extension}.map")),
                    source_map.to_json(),
                )?;
            }
            if let Some(declaration) = &artifact.declaration {
                fs::write(js_path.with_extension("d.ts"), declaration)?;
            }
        }
        for (module_id, source) in declaration_modules {
            let module_path = Path::new(module_id);
            let relative = artifact_relative_path(root, module_path, module_id)?;
            if !is_declaration_path(&relative) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("declaration module `{module_id}` does not end in .d.ts"),
                ));
            }
            let declaration_path = stage.join(relative);
            let declaration_parent = declaration_path
                .parent()
                .ok_or_else(|| io::Error::other("declaration path has no parent"))?;
            fs::create_dir_all(declaration_parent)?;
            fs::write(declaration_path, source)?;
        }
        let manifest = serde_json::to_vec_pretty(metadata).map_err(io::Error::other)?;
        fs::write(stage.join("bluetsc.manifest.json"), manifest)?;
        if metadata.has_configured_imports {
            let import_map = serde_json::json!({ "imports": &metadata.imports });
            let import_map = serde_json::to_vec_pretty(&import_map).map_err(io::Error::other)?;
            fs::write(stage.join("bluetsc.importmap.json"), import_map)?;
        }
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }
    if !output.exists() {
        return fs::rename(stage, output);
    }
    if !output.is_dir() {
        let _ = fs::remove_dir_all(&stage);
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists and is not a directory", output.display()),
        ));
    }
    let backup = parent.join(format!(".bluetsc-backup-{}-{nonce}", std::process::id()));
    fs::rename(&output, &backup)?;
    if let Err(error) = fs::rename(&stage, &output) {
        let _ = fs::rename(&backup, &output);
        return Err(error);
    }
    fs::remove_dir_all(backup)
}

/// An atomic publish replaces the selected output directory. Reject an output
/// directory that contains an input module before staging anything, so a
/// configuration such as `outDir: "src"` cannot replace source files.
fn ensure_output_does_not_contain_sources(
    output: &Path,
    root: &Path,
    artifacts: &std::collections::BTreeMap<String, blueice_bluets::BuildArtifact>,
    declaration_modules: &std::collections::BTreeMap<String, String>,
) -> io::Result<()> {
    for module_id in artifacts.keys().chain(declaration_modules.keys()) {
        let module_path = Path::new(module_id);
        let source = if module_path.is_absolute() {
            module_path.to_path_buf()
        } else {
            root.join(module_path)
        };
        if source.starts_with(output) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "output directory {} would replace source module {module_id}",
                    output.display()
                ),
            ));
        }
    }
    Ok(())
}

fn absolute_existing_path(path: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(path)
}

fn absolute_path(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

fn artifact_relative_path(root: &Path, path: &Path, module_id: &str) -> io::Result<PathBuf> {
    if path.is_absolute() {
        return path.strip_prefix(root).map(Path::to_path_buf).map_err(|_| {
            io::Error::other(format!("artifact `{module_id}` is outside project root"))
        });
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(io::Error::other(format!(
            "artifact `{module_id}` escapes the project root"
        )));
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
fn path_id(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn project_module_id(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(output_path)
        .expect("validated module is beneath project root")
}

#[cfg(test)]
#[path = "bluetsc/tests.rs"]
mod tests;

#[path = "bluetsc/strict_publish.rs"]
mod strict_publish;
