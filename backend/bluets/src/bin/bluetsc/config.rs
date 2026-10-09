// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Command-line arguments and owner-scoped project configuration.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Command {
    Check,
    Build,
    /// The one command that touches the network: it fetches the owner's pinned
    /// remote declaration sources into the cache. `check` and `build` never do.
    FetchDeclarations,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Args {
    pub(super) command: Command,
    pub(super) input: Input,
    pub(super) show_config: bool,
    pub(super) diagnostics_json: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Input {
    Entry {
        entry: PathBuf,
        project_root: Option<PathBuf>,
        out_dir: Option<PathBuf>,
        options: Box<CompilerOptions>,
    },
    Config(PathBuf),
}

#[derive(Debug)]
pub(super) struct Invocation {
    pub(super) root: PathBuf,
    pub(super) entries: Vec<PathBuf>,
    pub(super) out_dir: Option<PathBuf>,
    pub(super) options: CompilerOptions,
    pub(super) imports: BTreeMap<String, PathBuf>,
    pub(super) packages: Option<PackageSettings>,
    pub(super) remote: Option<RemoteSettings>,
    pub(super) project_config: Option<tsconfig::ProjectConfig>,
}

/// The owner's pinned remote declaration sources and where they are cached.
#[derive(Debug, Clone)]
pub(super) struct RemoteSettings {
    pub(super) sources: Vec<RemoteDeclarationSource>,
    pub(super) cache_directory: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RemoteDeclarationConfig {
    pub(super) specifier: String,
    pub(super) url: String,
    pub(super) sha256: String,
}

/// The owner's installed-package settings, from the config file only.
#[derive(Debug, Clone)]
pub(super) struct PackageSettings {
    pub(super) resolution: ModuleResolution,
    /// Canonical directories beyond the project root dependencies may be read
    /// from.
    pub(super) extra_roots: Vec<PathBuf>,
    pub(super) custom_conditions: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BlueTscConfig {
    #[serde(default)]
    pub(super) entries: Vec<String>,
    #[serde(default)]
    pub(super) project_root: Option<String>,
    #[serde(default)]
    pub(super) out_dir: Option<String>,
    #[serde(default)]
    pub(super) source_map: bool,
    #[serde(default)]
    pub(super) declaration: bool,
    #[serde(default)]
    pub(super) target: Option<String>,
    #[serde(default)]
    pub(super) use_define_for_class_fields: Option<bool>,
    #[serde(default)]
    pub(super) preserve_const_enums: bool,
    #[serde(default)]
    pub(super) isolated_modules: bool,
    #[serde(default)]
    pub(super) es_module_interop: bool,
    #[serde(default)]
    pub(super) resolve_json_module: bool,
    #[serde(default)]
    pub(super) module: Option<String>,
    #[serde(default)]
    pub(super) experimental_decorators: bool,
    #[serde(default)]
    pub(super) emit_decorator_metadata: bool,
    #[serde(default)]
    pub(super) jsx: Option<String>,
    #[serde(default)]
    pub(super) jsx_factory: Option<String>,
    #[serde(default)]
    pub(super) jsx_fragment_factory: Option<String>,
    #[serde(default)]
    pub(super) jsx_import_source: Option<String>,
    #[serde(default)]
    pub(super) runtime_policy: Option<String>,
    #[serde(default)]
    pub(super) imports: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) module_resolution: Option<String>,
    #[serde(default)]
    pub(super) package_roots: Vec<String>,
    #[serde(default)]
    pub(super) custom_conditions: Vec<String>,
    #[serde(default)]
    pub(super) remote_declarations: Vec<RemoteDeclarationConfig>,
    #[serde(default)]
    pub(super) declaration_cache: Option<String>,
    #[serde(default)]
    pub(super) strict_boundaries: Vec<StrictBoundaryConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StrictBoundaryConfig {
    pub(super) contract_id: String,
    pub(super) module: String,
    pub(super) function: String,
    pub(super) source_start: usize,
    pub(super) source_end: usize,
    pub(super) max_string_bytes: usize,
    pub(super) helper_version: String,
}

pub(super) fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut args = args;
    let command = match args.next().as_deref() {
        Some("check") => Command::Check,
        Some("build") => Command::Build,
        Some("fetch-declarations") => Command::FetchDeclarations,
        Some("--help") | Some("-h") => return Err("help requested".to_string()),
        Some(other) => {
            return Err(format!(
                "unknown command `{other}`; expected `check`, `build` or `fetch-declarations`"
            ))
        }
        None => return Err("a command is required".to_string()),
    };
    let first = args
        .next()
        .ok_or_else(|| "an entry .ts file or --config <file> is required".to_string())?;
    if first == "--config" {
        let path = args
            .next()
            .ok_or_else(|| "--config requires a JSON file".to_string())?;
        let mut show_config = false;
        let mut diagnostics_json = false;
        for extra in args {
            match extra.as_str() {
                "--showConfig" => show_config = true,
                "--diagnostics-json" => diagnostics_json = true,
                _ => {
                    return Err(format!(
                        "`--config` owns project settings; unsupported extra argument `{extra}`"
                    ))
                }
            }
        }
        return Ok(Args {
            command,
            input: Input::Config(PathBuf::from(path)),
            show_config,
            diagnostics_json,
        });
    }
    if command == Command::FetchDeclarations {
        return Err("fetch-declarations takes only --config <bluetsc.json>".to_string());
    }
    let entry = PathBuf::from(first);
    let mut project_root = None;
    let mut out_dir = None;
    let mut options = CompilerOptions::default();
    let mut diagnostics_json = false;
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match flag.as_str() {
            "--diagnostics-json" => diagnostics_json = true,
            "--project-root" => project_root = Some(PathBuf::from(value()?)),
            "--out-dir" => out_dir = Some(PathBuf::from(value()?)),
            "--source-map" => options.source_map = true,
            "--declaration" => options.declaration = true,
            "--target" => {
                options.target = match value()?.as_str() {
                    "es2020" => EcmaTarget::Es2020,
                    "es2022" => EcmaTarget::Es2022,
                    other => return Err(format!("unsupported target `{other}`; expected es2020 or es2022")),
                }
            }
            "--preserve-const-enums" => options.preserve_const_enums = true,
            "--isolated-modules" => options.isolated_modules = true,
            "--es-module-interop" => options.es_module_interop = true,
            "--experimental-decorators" => options.experimental_decorators = true,
            "--emit-decorator-metadata" => options.emit_decorator_metadata = true,
            "--jsx" => {
                let name = value()?;
                options.jsx = Some(JsxMode::parse(&name).ok_or_else(|| {
                    format!(
                        "unsupported jsx `{name}`; expected preserve, react-native, react, react-jsx or react-jsxdev"
                    )
                })?)
            }
            "--jsx-factory" => options.jsx_factory = Some(value()?),
            "--jsx-fragment-factory" => options.jsx_fragment_factory = Some(value()?),
            "--jsx-import-source" => options.jsx_import_source = Some(value()?),
            "--module" => {
                let module = value()?;
                options.import_attributes = module == "esnext";
                options.module_kind = match module.as_str() {
                    "esnext" | "es2022" | "es2020" | "es2015" | "es6" => ModuleKind::Esm,
                    "commonjs" => ModuleKind::CommonJs,
                    other => {
                        return Err(format!(
                            "unsupported module `{other}`; expected esnext or commonjs"
                        ))
                    }
                }
            }
            "--use-define-for-class-fields" => {
                options.use_define_for_class_fields = Some(match value()?.as_str() {
                    "true" => true,
                    "false" => false,
                    other => {
                        return Err(format!(
                            "unsupported --use-define-for-class-fields value `{other}`; expected true or false"
                        ))
                    }
                })
            }
            "--runtime-policy" => {
                options.runtime_policy = match value()?.as_str() {
                    "transpile-only" => RuntimePolicy::TranspileOnly,
                    "checked" => RuntimePolicy::Checked,
                    "strict-runtime" => RuntimePolicy::StrictRuntime,
                    other => {
                        return Err(format!(
                            "unsupported runtime policy `{other}`; expected transpile-only, checked, or strict-runtime"
                        ))
                    }
                }
            }
            "--help" | "-h" => return Err("help requested".to_string()),
            other => return Err(format!("unrecognized argument `{other}`")),
        }
    }
    if options.emit_decorator_metadata && !options.experimental_decorators {
        return Err("--emit-decorator-metadata requires --experimental-decorators".to_string());
    }
    if command == Command::Build && out_dir.is_none() {
        return Err("build requires --out-dir <directory>".to_string());
    }
    if command == Command::Check && out_dir.is_some() {
        return Err("--out-dir is valid only with build".to_string());
    }
    Ok(Args {
        command,
        input: Input::Entry {
            entry,
            project_root,
            out_dir,
            options: Box::new(options),
        },
        show_config: false,
        diagnostics_json,
    })
}

pub(super) fn usage() -> &'static str {
    "Usage:\n  bluetsc check <entry.ts> [--project-root <directory>] [--target es2020|es2022] [--use-define-for-class-fields true|false] [--preserve-const-enums] [--isolated-modules] [--module esnext|commonjs] [--es-module-interop] [--experimental-decorators] [--emit-decorator-metadata] [--jsx preserve|react-native|react|react-jsx|react-jsxdev] [--jsx-factory <name>] [--jsx-fragment-factory <name>] [--jsx-import-source <module>] [--runtime-policy transpile-only|checked|strict-runtime] [--diagnostics-json]\n  bluetsc build <entry.ts> --out-dir <directory> [--project-root <directory>] [--source-map] [--declaration] [--target es2020|es2022] [--use-define-for-class-fields true|false] [--preserve-const-enums] [--isolated-modules] [--module esnext|commonjs] [--es-module-interop] [--experimental-decorators] [--emit-decorator-metadata] [--jsx preserve|react-native|react|react-jsx|react-jsxdev] [--jsx-factory <name>] [--jsx-fragment-factory <name>] [--jsx-import-source <module>] [--runtime-policy transpile-only|checked|strict-runtime] [--diagnostics-json]\n  bluetsc fetch-declarations --config <bluetsc.json>\n  bluetsc check --config <bluetsc.json>\n  bluetsc check --config <tsconfig.json> [--showConfig] [--diagnostics-json]\n  bluetsc build --config <bluetsc.json>\n  bluetsc build --config <tsconfig.json> [--showConfig] [--diagnostics-json]\n\nConfig fields: entries, projectRoot, outDir, sourceMap, declaration, target, useDefineForClassFields, preserveConstEnums, isolatedModules, module, esModuleInterop, experimentalDecorators, emitDecoratorMetadata, jsx, jsxFactory, jsxFragmentFactory, jsxImportSource, runtimePolicy, imports, moduleResolution, packageRoots, customConditions, remoteDeclarations, declarationCache, strictBoundaries, tsconfig."
}

pub(super) fn resolve_invocation(input: Input) -> Result<Invocation, String> {
    match input {
        Input::Entry {
            entry,
            project_root,
            out_dir,
            options,
        } => resolve_explicit_invocation(entry, project_root, out_dir, *options),
        Input::Config(path) => resolve_config_invocation(path),
    }
}

pub(super) fn resolve_explicit_invocation(
    entry: PathBuf,
    project_root: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    options: CompilerOptions,
) -> Result<Invocation, String> {
    let entry = absolute_existing_path(&entry)
        .map_err(|error| format!("cannot read entry {}: {error}", entry.display()))?;
    let root = match project_root {
        Some(root) => absolute_existing_path(&root)
            .map_err(|error| format!("cannot access --project-root {}: {error}", root.display()))?,
        None => entry
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
    };
    if !root.is_dir() {
        return Err(format!(
            "project root {} is not a directory",
            root.display()
        ));
    }
    ensure_within(&entry, &root, "entry")?;
    ensure_not_declaration_entry(&entry, "entry")?;
    Ok(Invocation {
        root,
        entries: vec![entry],
        out_dir,
        options,
        imports: BTreeMap::new(),
        packages: None,
        remote: None,
        project_config: None,
    })
}

pub(super) fn resolve_config_invocation(path: PathBuf) -> Result<Invocation, String> {
    resolve_config_with_diagnostic(path, &mut None)
}

pub(super) fn resolve_config_with_diagnostic(
    path: PathBuf,
    report: &mut Option<blueice_bluets::Diagnostic>,
) -> Result<Invocation, String> {
    resolve_config_with_sources(path, report, &mut BTreeMap::new())
}

pub(super) fn resolve_config_with_sources(
    path: PathBuf,
    report: &mut Option<blueice_bluets::Diagnostic>,
    sources: &mut BTreeMap<String, String>,
) -> Result<Invocation, String> {
    let config_path = absolute_existing_path(&path).map_err(|error| {
        let message = format!("cannot read config {}: {error}", path.display());
        if error.kind() == io::ErrorKind::NotFound {
            *report = Some(tsconfig::diagnostics::missing_project(&message, &path));
        }
        message
    })?;
    let config_text = fs::read_to_string(&config_path)
        .map_err(|error| format!("cannot read config {}: {error}", config_path.display()))?;
    if config_text.len() > 1024 * 1024 {
        return Err("configuration resource limit exceeded".into());
    }
    if let Some(name) = config_path.file_name().and_then(|name| name.to_str()) {
        sources.insert(name.into(), config_text.clone());
    }
    let value = tsconfig::parse_jsonc(&config_text).map_err(|error| {
        let message = format!("invalid config {}: {error}", config_path.display());
        *report = tsconfig::diagnostics::invalid_json(&message, &config_path, &config_text);
        message
    })?;
    if !value.is_object() {
        return Err(format!(
            "invalid config {}: expected an object",
            config_path.display()
        ));
    }
    let config_directory = config_path
        .parent()
        .ok_or_else(|| "config has no parent directory".to_string())?;
    let ts_shape = config_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("tsconfig"))
        || ["compilerOptions", "files", "include", "exclude", "extends"]
            .iter()
            .any(|key| value.get(*key).is_some());
    let owner = !ts_shape || value.get("entries").is_some() || value.get("tsconfig").is_some();
    if !owner {
        let owner_path = config_directory.join("bluetsc.json");
        return tsconfig::resolve(
            &config_path,
            owner_path.is_file().then_some(owner_path.as_path()),
            report,
            sources,
        );
    }
    let selected = value
        .get("tsconfig")
        .and_then(serde_json::Value::as_str)
        .map(|path| tsconfig::clean_path(&config_directory.join(path)))
        .unwrap_or_else(|| config_directory.join("tsconfig.json"));
    if value
        .get("tsconfig")
        .is_some_and(|value| !value.is_null() && !value.is_string())
    {
        return Err("owner tsconfig requires a string path".to_string());
    }
    if value.get("tsconfig").is_some() || selected.is_file() {
        return tsconfig::resolve(&selected, Some(&config_path), report, sources);
    }
    if value.get("entries").is_none() {
        return Err(format!(
            "invalid config {}: missing field `entries`",
            config_path.display()
        ));
    }
    let config: BlueTscConfig = serde_json::from_value(value)
        .map_err(|error| format!("invalid config {}: {error}", config_path.display()))?;
    if config.entries.is_empty() {
        return Err("config `entries` must contain at least one .ts entry".to_string());
    }
    let root = match config.project_root.as_deref() {
        Some(root) => {
            let root_path = Path::new(root);
            if root_path.is_absolute()
                || root_path
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir))
            {
                return Err(format!(
                    "config projectRoot `{root}` must stay beneath the config directory"
                ));
            }
            absolute_existing_path(&config_directory.join(root_path)).map_err(|error| {
                format!(
                    "cannot access config projectRoot under {}: {error}",
                    config_directory.display()
                )
            })?
        }
        None => config_directory.to_path_buf(),
    };
    resolve_config_document(config_directory, root, config, true)
}

pub(super) fn resolve_config_document(
    config_directory: &Path,
    root: PathBuf,
    config: BlueTscConfig,
    validate_entries: bool,
) -> Result<Invocation, String> {
    if !root.is_dir() {
        return Err(format!(
            "config projectRoot {} is not a directory",
            root.display()
        ));
    }
    let mut entries = Vec::new();
    for entry in config.entries {
        let entry = if validate_entries {
            absolute_existing_path(&root.join(entry))
                .map_err(|error| format!("cannot read configured entry: {error}"))?
        } else {
            tsconfig::clean_path(&root.join(entry))
        };
        ensure_within(&entry, &root, "configured entry")?;
        if validate_entries {
            ensure_not_declaration_entry(&entry, "configured entry")?;
        }
        entries.push(entry);
    }
    entries.sort();
    entries.dedup();
    if !config.strict_boundaries.is_empty()
        && entries
            .iter()
            .filter(|entry| !is_declaration_path(entry))
            .count()
            != 1
    {
        return Err(
            "the first emitted strict boundary profile requires exactly one entry".to_string(),
        );
    }
    let out_dir = config
        .out_dir
        .map(|path| configured_output_path(&root, &path))
        .transpose()?;
    if let Some(out_dir) = &out_dir {
        ensure_existing_ancestor_within(out_dir, &root, "config outDir")?;
    }
    let mut imports = BTreeMap::new();
    for (specifier, target) in config.imports {
        if specifier.is_empty() || specifier.starts_with('.') || specifier.starts_with('/') {
            return Err(format!("invalid import-map key `{specifier}`"));
        }
        let target = absolute_existing_path(&root.join(target)).map_err(|error| {
            format!("cannot resolve import-map target for `{specifier}`: {error}")
        })?;
        ensure_within(&target, &root, "import-map target")?;
        if specifier.ends_with('/') && !target.is_dir() {
            return Err(format!(
                "import-map prefix `{specifier}` must target a project-root-confined directory"
            ));
        }
        if !specifier.ends_with('/') && !target.is_file() {
            return Err(format!(
                "import-map key `{specifier}` must target a project-root-confined source file"
            ));
        }
        imports.insert(specifier, target);
    }
    let packages = configured_packages(
        config_directory,
        &root,
        config.module_resolution.as_deref(),
        &config.package_roots,
        config.custom_conditions,
    )?;
    let remote = configured_remote(
        &root,
        config.remote_declarations,
        config.declaration_cache.as_deref(),
    )?;
    let strict_runtime_boundaries = config
        .strict_boundaries
        .into_iter()
        .map(|boundary| configured_strict_boundary(&root, boundary))
        .collect::<Result<Vec<_>, _>>()?;
    if config.emit_decorator_metadata && !config.experimental_decorators {
        return Err("config emitDecoratorMetadata requires experimentalDecorators".to_string());
    }
    let options_module_kind = parse_module_kind(config.module.as_deref())?;
    let options = CompilerOptions {
        checking: None,
        target: parse_target(config.target.as_deref())?,
        use_define_for_class_fields: config.use_define_for_class_fields,
        preserve_const_enums: config.preserve_const_enums,
        isolated_modules: config.isolated_modules,
        es_module_interop: config.es_module_interop,
        experimental_decorators: config.experimental_decorators,
        emit_decorator_metadata: config.emit_decorator_metadata,
        jsx: config
            .jsx
            .as_deref()
            .map(|name| {
                JsxMode::parse(name).ok_or_else(|| {
                    format!(
                        "unsupported jsx `{name}`; expected preserve, react-native, react, react-jsx or react-jsxdev"
                    )
                })
            })
            .transpose()?,
        jsx_factory: config.jsx_factory,
        jsx_fragment_factory: config.jsx_fragment_factory,
        jsx_import_source: config.jsx_import_source,
        module_kind: options_module_kind,
        import_attributes: config.module.as_deref() == Some("esnext"),
        resolve_json_module: config.resolve_json_module,
        runtime_policy: parse_runtime_policy(config.runtime_policy.as_deref())?,
        source_map: config.source_map,
        declaration: config.declaration,
        resolver_fingerprint: format!(
            "{}{}{}",
            import_map_fingerprint(&root, &imports),
            packages
                .as_ref()
                .map(|settings| format!("+{}", package_resolver(&root, settings).fingerprint()))
                .unwrap_or_default(),
            remote
                .as_ref()
                .map(|settings| remote_fingerprint(&settings.sources))
                .unwrap_or_default()
        ),
        // The standalone CLI has no page-host profile authority. Only a host
        // that verified a generated `lib.blueice.d.ts` may add ambient
        // declarations through the library API.
        ambient_declaration_modules: Vec::new(),
        require_declared_global_calls: false,
        strict_runtime_boundaries,
        limits: CompilerLimits::default(),
    };
    Ok(Invocation {
        root,
        entries,
        out_dir,
        options,
        imports,
        packages,
        remote,
        project_config: None,
    })
}

pub(super) fn configured_remote(
    root: &Path,
    configured: Vec<RemoteDeclarationConfig>,
    cache: Option<&str>,
) -> Result<Option<RemoteSettings>, String> {
    if configured.is_empty() {
        if cache.is_some() {
            return Err("config declarationCache needs remoteDeclarations".to_string());
        }
        return Ok(None);
    }
    let sources: Vec<RemoteDeclarationSource> = configured
        .into_iter()
        .map(|source| RemoteDeclarationSource {
            specifier: source.specifier,
            url: source.url,
            sha256: source.sha256,
        })
        .collect();
    blueice_bluets::remote_declarations::validate_sources(&sources, &RemoteLimits::default())
        .map_err(|error| error.to_string())?;
    let relative = Path::new(cache.unwrap_or(".bluetsc/declarations"));
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("config declarationCache must stay beneath the project root".to_string());
    }
    Ok(Some(RemoteSettings {
        sources,
        cache_directory: root.join(relative),
    }))
}

pub(super) fn configured_packages(
    config_directory: &Path,
    root: &Path,
    module_resolution: Option<&str>,
    package_roots: &[String],
    custom_conditions: Vec<String>,
) -> Result<Option<PackageSettings>, String> {
    let Some(name) = module_resolution else {
        if !package_roots.is_empty() || !custom_conditions.is_empty() {
            return Err(
                "config packageRoots and customConditions need a moduleResolution".to_string(),
            );
        }
        return Ok(None);
    };
    let resolution = ModuleResolution::parse(name).ok_or_else(|| {
        format!("unsupported moduleResolution `{name}`; expected node10, node16 or bundler")
    })?;
    let mut extra_roots = Vec::new();
    for configured in package_roots {
        let path = absolute_existing_path(&config_directory.join(configured))
            .map_err(|error| format!("cannot access packageRoot `{configured}`: {error}"))?;
        if !path.is_dir() {
            return Err(format!("packageRoot `{configured}` is not a directory"));
        }
        if path != root && !extra_roots.contains(&path) {
            extra_roots.push(path);
        }
    }
    Ok(Some(PackageSettings {
        resolution,
        extra_roots,
        custom_conditions,
    }))
}

pub(super) fn configured_strict_boundary(
    root: &Path,
    boundary: StrictBoundaryConfig,
) -> Result<StrictRuntimeBoundary, String> {
    let relative = Path::new(&boundary.module);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err("strict boundary module must be a root-relative source path".to_string());
    }
    let module = absolute_existing_path(&root.join(relative))
        .map_err(|error| format!("cannot read strict boundary module: {error}"))?;
    ensure_within(&module, root, "strict boundary module")?;
    if !module.is_file()
        || !matches!(
            module.extension().and_then(|value| value.to_str()),
            Some("ts" | "tsx")
        )
        || is_declaration_path(&module)
    {
        return Err("strict boundary module must be a .ts or .tsx source file".to_string());
    }
    Ok(StrictRuntimeBoundary {
        contract_id: boundary.contract_id,
        function: boundary.function,
        span: SourceSpan::new(
            project_module_id(root, &module),
            boundary.source_start,
            boundary.source_end,
        ),
        max_string_bytes: boundary.max_string_bytes,
        helper_version: boundary.helper_version,
    })
}

pub(super) fn parse_module_kind(value: Option<&str>) -> Result<ModuleKind, String> {
    match value {
        None | Some("esnext" | "es2022" | "es2020" | "es2015" | "es6") => Ok(ModuleKind::Esm),
        Some("commonjs") => Ok(ModuleKind::CommonJs),
        Some(other) => Err(format!(
            "unsupported module `{other}`; expected esnext or commonjs"
        )),
    }
}

pub(super) fn parse_target(value: Option<&str>) -> Result<EcmaTarget, String> {
    match value.unwrap_or("es2022") {
        "es2020" => Ok(EcmaTarget::Es2020),
        "es2022" => Ok(EcmaTarget::Es2022),
        other => Err(format!(
            "unsupported target `{other}`; expected es2020 or es2022"
        )),
    }
}

pub(super) fn parse_runtime_policy(value: Option<&str>) -> Result<RuntimePolicy, String> {
    match value.unwrap_or("checked") {
        "transpile-only" => Ok(RuntimePolicy::TranspileOnly),
        "checked" => Ok(RuntimePolicy::Checked),
        "strict-runtime" => Ok(RuntimePolicy::StrictRuntime),
        other => Err(format!(
            "unsupported runtime policy `{other}`; expected transpile-only, checked, or strict-runtime"
        )),
    }
}

pub(super) fn configured_output_path(root: &Path, value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return Err(format!(
            "config outDir `{value}` must stay beneath projectRoot"
        ));
    }
    Ok(root.join(path))
}

pub(super) fn ensure_within(path: &Path, root: &Path, label: &str) -> Result<(), String> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(format!(
            "{label} {} is outside project root {}",
            path.display(),
            root.display()
        ))
    }
}

/// Checks the existing portion of a future path after resolving symlinks. A
/// syntactically root-relative output such as `linked/dist` must not acquire
/// authority outside the project through an already-existing `linked`
/// symlink.
pub(super) fn ensure_existing_ancestor_within(
    path: &Path,
    root: &Path,
    label: &str,
) -> Result<(), String> {
    let mut ancestor = path;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| format!("{label} {} has no existing ancestor", path.display()))?;
    }
    let canonical = fs::canonicalize(ancestor).map_err(|error| {
        format!(
            "cannot resolve existing ancestor {} for {label}: {error}",
            ancestor.display()
        )
    })?;
    ensure_within(&canonical, root, label)
}

pub(super) fn ensure_not_declaration_entry(path: &Path, label: &str) -> Result<(), String> {
    if is_declaration_path(path) {
        Err(format!(
            "{label} {} is a .d.ts declaration module, not an executable entry",
            path.display()
        ))
    } else {
        Ok(())
    }
}

pub(super) fn is_declaration_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".d.ts"))
}
