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

#[path = "bluetsc/file_loader.rs"]
mod file_loader;
use file_loader::FileLoader;

#[path = "bluetsc/node_modules.rs"]
mod node_modules;

#[path = "bluetsc/native_cli/mod.rs"]
mod native_cli;
#[path = "bluetsc/tsconfig/mod.rs"]
mod tsconfig;

const RUNTIME_HELPER_V1_FILE: &str = "bluets.runtime-helper.v1.mjs";
const RUNTIME_HELPER_V1_VERSION: &str = "bluets-runtime-helper-v1";
const RUNTIME_HELPER_V1_SOURCE: &str = include_str!("../runtime_helper_v1.mjs");

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments
        .first()
        .is_none_or(|arg| arg.starts_with('-') && !matches!(arg.as_str(), "--help" | "-h"))
    {
        return native_cli::run(arguments);
    }
    let args = match parse_args(arguments.into_iter()) {
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
    let mut invocation = match resolve_invocation(args.input) {
        Ok(invocation) => invocation,
        Err(message) => {
            eprintln!("bluetsc: {message}");
            return ExitCode::FAILURE;
        }
    };
    if args.show_config {
        let Some(project) = &invocation.project_config else {
            eprintln!("bluetsc: --showConfig requires a TypeScript project config");
            return ExitCode::FAILURE;
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&project.show())
                .expect("configuration JSON is serializable")
        );
        return ExitCode::SUCCESS;
    }
    if let Err(message) = tsconfig::prepare(&mut invocation) {
        eprintln!("bluetsc: {message}");
        return ExitCode::FAILURE;
    }
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
        mappings: invocation
            .project_config
            .as_ref()
            .and_then(|project| project.mappings.clone()),
        extra_roots: invocation
            .packages
            .as_ref()
            .map(|settings| settings.extra_roots.clone())
            .unwrap_or_default(),
        packages: invocation
            .packages
            .as_ref()
            .map(|settings| package_resolver(&invocation.root, settings)),
        relative: relative_resolver(&invocation.root),
        resolve_json_module: invocation.options.resolve_json_module,
        allow_js: invocation.options.allow_js,
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
    let mut summary = compile_entries(
        &invocation.entries,
        &loader,
        invocation.options.clone(),
        args.diagnostics_json,
    );
    if summary.has_errors {
        return ExitCode::FAILURE;
    }
    if let Err(message) = tsconfig::prepare_output(&mut invocation, &mut summary) {
        eprintln!("bluetsc: {message}");
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
        &summary.assets,
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
    assets: BTreeMap<String, String>,
    fingerprint: String,
    module_count: usize,
    has_errors: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    root_dir: Option<String>,
    standard_library: blueice_bluets::standard_library::Identity,
    language_version: &'static str,
    fingerprint: String,
    target: &'static str,
    downlevel_iteration: bool,
    import_helpers: bool,
    no_emit_helpers: bool,
    verbatim_module_syntax: bool,
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
    #[serde(rename = "target_helper_version")]
    target_helper_version: &'static str,
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
    remove_comments: bool,
    new_line: &'static str,
    emit_bom: bool,
    inline_sources: bool,
    strip_internal: bool,
    source_root: Option<String>,
    map_root: Option<String>,
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
    entries: &[PathBuf],
    loader: &FileLoader,
    options: CompilerOptions,
    diagnostics_json: bool,
) -> CompileSummary {
    let mut artifacts = BTreeMap::new();
    let mut declaration_modules = BTreeMap::new();
    let mut assets = BTreeMap::new();
    let mut modules = BTreeSet::new();
    let mut fingerprints = Vec::new();
    let mut has_errors = false;
    let has_runtime_entries = entries.iter().any(|entry| !is_declaration_path(entry));
    for entry in entries {
        let module = loader
            .module_id(entry)
            .expect("entry has an authorized identity");
        let mut entry_options = options.clone();
        entry_options
            .ambient_declaration_modules
            .retain(|source| source.id != module);
        if is_declaration_path(entry) {
            if entry_options.runtime_policy == RuntimePolicy::StrictRuntime {
                entry_options.runtime_policy = RuntimePolicy::Checked;
            }
            entry_options.strict_runtime_boundaries.clear();
        }
        let result = compile(&module, loader, entry_options);
        for diagnostic in &result.diagnostics {
            if diagnostics_json {
                eprintln!("{}", diagnostic.to_json());
            } else {
                eprintln!(
                    "{}:{}:{}: {}: {}",
                    diagnostic.span.module,
                    diagnostic.span.start,
                    diagnostic.span.end,
                    diagnostic.code,
                    diagnostic.message
                );
            }
        }
        has_errors |= result.has_errors();
        modules.extend(result.project.modules.keys().cloned());
        if let Some(output) = result.output {
            assets.extend(output.assets);
            if !has_runtime_entries || !is_declaration_path(entry) {
                fingerprints.push(output.fingerprint);
            }
            for (module_id, artifact) in output.artifacts {
                artifacts.entry(module_id).or_insert(artifact);
            }
            for (module_id, source) in output.declaration_modules {
                declaration_modules.entry(module_id).or_insert(source);
            }
        }
    }
    fingerprints.sort();
    if fingerprints.is_empty() {
        fingerprints.extend([
            blueice_bluets::LANGUAGE_VERSION.to_string(),
            blueice_bluets::standard_library::VERSION.to_string(),
            options.resolver_fingerprint,
        ]);
    }
    CompileSummary {
        artifacts,
        declaration_modules,
        assets,
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
        .filter(|entry| !is_declaration_path(entry))
        .map(|entry| {
            output_module_path(
                invocation
                    .project_config
                    .as_ref()
                    .map(|project| project.emit_root.as_path())
                    .unwrap_or(&invocation.root),
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
                .strip_prefix(
                    invocation
                        .project_config
                        .as_ref()
                        .map(|project| project.emit_root.as_path())
                        .unwrap_or(&invocation.root),
                )
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
        root_dir: invocation
            .project_config
            .as_ref()
            .map(|project| tsconfig::relative_text(&invocation.root, &project.emit_root)),
        standard_library: blueice_bluets::standard_library::identity_with_libraries(
            invocation.options.target,
            invocation.options.libraries.as_deref(),
        ),
        language_version: blueice_bluets::LANGUAGE_VERSION,
        fingerprint: summary.fingerprint.clone(),
        target: invocation.options.target.as_str(),
        downlevel_iteration: invocation.options.downlevel_iteration,
        import_helpers: invocation.options.import_helpers,
        no_emit_helpers: invocation.options.no_emit_helpers,
        verbatim_module_syntax: invocation.options.verbatim_module_syntax,
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
        target_helper_version: blueice_bluets::TARGET_HELPER_V1_VERSION,
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
        remove_comments: invocation.options.remove_comments,
        new_line: invocation.options.new_line.as_str(),
        emit_bom: invocation.options.emit_bom,
        inline_sources: invocation.options.inline_sources,
        strip_internal: invocation.options.strip_internal,
        source_root: invocation.options.source_root.clone(),
        map_root: invocation.options.map_root.clone(),
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
    match source.extension().and_then(|extension| extension.to_str()) {
        Some("mts") => return "mjs",
        Some("cts") => return "cjs",
        _ => {}
    }
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

fn relative_resolver(root: &Path) -> PackageResolver<OsPackageFs> {
    PackageResolver::new(
        OsPackageFs,
        PackageResolverConfig {
            roots: vec![root.to_path_buf()],
            resolution: ModuleResolution::Node10,
            custom_conditions: Vec::new(),
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

fn publish_build(
    root: &Path,
    out_dir: &Path,
    artifacts: &std::collections::BTreeMap<String, blueice_bluets::BuildArtifact>,
    declaration_modules: &std::collections::BTreeMap<String, String>,
    assets: &BTreeMap<String, String>,
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
    assets_publish::validate_sources(&output, root, assets)?;
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
            let relative = if metadata.root_dir.is_some() {
                tsconfig::emitted_path(module_id, metadata)?
            } else {
                artifact_relative_path(root, module_path, module_id)?
            };
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
                let url = metadata
                    .map_root
                    .as_deref()
                    .filter(|root| !root.is_empty())
                    .map_or_else(
                        || map_name.clone(),
                        |root| format!("{}/{map_name}", root.trim_end_matches('/')),
                    );
                let newline = if metadata.new_line == "crlf" {
                    "\r\n"
                } else {
                    "\n"
                };
                javascript.push_str(&format!("{newline}//# sourceMappingURL={url}{newline}"));
            }
            fs::write(&js_path, javascript)?;
            if let Some(source_map) = &artifact.source_map {
                fs::write(
                    js_path.with_extension(format!("{extension}.map")),
                    source_map.to_json(),
                )?;
            }
            if let Some(declaration) = &artifact.declaration {
                fs::write(
                    js_path.with_extension(node_modules::declaration_extension(extension)),
                    declaration,
                )?;
            }
        }
        for (module_id, source) in declaration_modules {
            if metadata.root_dir.is_some() {
                continue;
            }
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
        assets_publish::stage(root, &stage, assets, metadata)?;
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

#[path = "bluetsc/assets_publish.rs"]
mod assets_publish;
