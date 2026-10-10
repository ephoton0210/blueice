// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native TypeScript project commands share the owner-confined compiler.

use super::*;
use blueice_bluets::{Diagnostic, Project};
use serde_json::Value;
mod args;
mod output;
mod presentation;
mod source_maps;
mod statistics;
use args::Args;

pub(super) fn run(arguments: Vec<String>) -> ExitCode {
    let started = std::time::Instant::now();
    let empty_command = arguments.is_empty();
    let diagnostics_json = arguments
        .iter()
        .enumerate()
        .rfind(|(_, arg)| arg.as_str() == "--diagnostics-json")
        .is_some_and(|(index, _)| {
            arguments
                .get(index + 1)
                .is_none_or(|value| value != "false")
        });
    let fail = |message: &str, pretty| failure(message, pretty, diagnostics_json, None);
    let pretty = arguments
        .windows(2)
        .any(|pair| pair[0] == "--pretty" && pair[1] == "true");
    let mut args = match Args::parse(args::without_machine_flags(arguments)) {
        Ok(args) => args,
        Err(_) if empty_command => {
            eprintln!("bluetsc: a command is required\n\n{}", usage());
            return ExitCode::FAILURE;
        }
        Err(error) => return failure(&error, pretty, diagnostics_json, args::diagnostic(&error)),
    };
    let mut config_diagnostic = None;
    let mut config_sources = BTreeMap::new();
    let project_path = args.project.clone();
    let mut invocation = match resolve_config_with_sources(
        args.project,
        &mut config_diagnostic,
        &mut config_sources,
    ) {
        Ok(invocation) => invocation,
        Err(error) => {
            let pretty = args
                .flags
                .get("pretty")
                .and_then(Value::as_bool)
                .unwrap_or_else(|| {
                    config_sources
                        .values()
                        .filter_map(|source| tsconfig::parse_jsonc(source).ok())
                        .any(|config| config["compilerOptions"]["pretty"] == true)
                });
            if !diagnostics_json {
                if let Some(diagnostic) = config_diagnostic {
                    print!(
                        "{}",
                        presentation::format(&[diagnostic], &config_sources, pretty)
                    );
                    return ExitCode::FAILURE;
                }
            }
            return failure(&error, pretty, diagnostics_json, config_diagnostic);
        }
    };
    let show_config = args
        .flags
        .remove("showConfig")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    args.flags.remove("diagnostics-json");
    if let Err(error) = tsconfig::apply_output_flags(&mut invocation, args.flags) {
        return fail(&error, pretty);
    }
    let Some(project) = &mut invocation.project_config else {
        return fail("--project requires a TypeScript configuration", pretty);
    };
    let enabled = |name: &str| {
        project
            .options
            .get(name)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    let pretty = enabled("pretty");
    let no_emit = enabled("noEmit");
    let no_emit_on_error = enabled("noEmitOnError");
    let show_diagnostics = enabled("diagnostics") && !diagnostics_json;
    let list_files = enabled("listFiles");
    let list_emitted = enabled("listEmittedFiles");
    if show_config {
        let mut shown = project.show();
        if let Some(options) = shown["compilerOptions"].as_object_mut() {
            for name in ["pretty", "listFiles", "listEmittedFiles"] {
                options.remove(name);
            }
            for name in ["outDir", "rootDir", "declarationDir"] {
                if let Some(Value::String(path)) = options.get_mut(name) {
                    if path == "." {
                        *path = "./".to_string();
                    } else if !path.starts_with('.') {
                        *path = format!("./{path}");
                    }
                }
            }
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&shown).expect("configuration JSON")
        );
        return ExitCode::SUCCESS;
    }
    if let Err(error) = tsconfig::prepare(&mut invocation) {
        let source = project_path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| config_sources.get(name));
        let diagnostic = tsconfig::diagnostics::error(
            &error,
            &project_path,
            source.map(|source| source.as_bytes()),
        );
        return failure(&error, pretty, diagnostics_json, diagnostic);
    }
    let loader = match loader(&invocation) {
        Ok(loader) => loader,
        Err(error) => return fail(&error, pretty),
    };
    let CompiledProject {
        mut summary,
        files,
        diagnostics,
        sources,
        emit_blocked,
        mut statistics,
    } = compile_project(&invocation, &loader, !no_emit && !no_emit_on_error);
    for diagnostic in &diagnostics {
        if diagnostics_json {
            eprintln!("{}", diagnostic.to_json());
        }
    }
    if !diagnostics_json {
        print!("{}", presentation::format(&diagnostics, &sources, pretty));
    }
    if list_files {
        for file in &files {
            println!("{}", file.display());
        }
    }
    if summary.has_errors && ((!no_emit && no_emit_on_error) || emit_blocked) {
        if show_diagnostics {
            print!("{}", statistics.display(started.elapsed()));
        }
        return ExitCode::from(1);
    }
    if let Err(error) = tsconfig::prepare_output(&mut invocation, &mut summary) {
        return fail(&error, pretty);
    }
    if no_emit {
        if show_diagnostics {
            print!("{}", statistics.display(started.elapsed()));
        }
        return ExitCode::from(if summary.has_errors { 2 } else { 0 });
    }
    if invocation.options.runtime_policy == RuntimePolicy::StrictRuntime
        && invocation.options.strict_runtime_boundaries.is_empty()
    {
        return fail(
            "strict-runtime build requires owner-selected strictBoundaries in config",
            pretty,
        );
    }
    let write_started = std::time::Instant::now();
    let emitted = match output::publish(&invocation, &summary, &loader, &files) {
        Ok(emitted) => emitted,
        Err(error) => return fail(&error.to_string(), pretty),
    };
    statistics.written(write_started.elapsed());
    if list_emitted {
        for file in emitted {
            println!("TSFILE: {}", file.display());
        }
    }
    if show_diagnostics {
        print!("{}", statistics.display(started.elapsed()));
    }
    ExitCode::from(if summary.has_errors { 2 } else { 0 })
}
fn failure(message: &str, pretty: bool, json: bool, diagnostic: Option<Diagnostic>) -> ExitCode {
    if json {
        let diagnostic = diagnostic.unwrap_or_else(|| {
            let mut diagnostic = Diagnostic::error(
                blueice_bluets::DiagnosticCode::UnsupportedSyntax,
                SourceSpan::new("", 0, 0),
                message,
            );
            diagnostic.typescript = None;
            diagnostic.no_typescript_counterpart =
                Some("BlueTSC configuration or an explicit owner policy refused this input.");
            diagnostic
        });
        eprintln!("{}", diagnostic.to_json());
    } else if let Some(diagnostic) = diagnostic {
        print!(
            "{}",
            presentation::format(&[diagnostic], &BTreeMap::new(), pretty)
        );
    } else if pretty {
        eprintln!("\x1b[91mbluetsc: {message}\x1b[0m");
    } else {
        eprintln!("bluetsc: {message}");
    }
    ExitCode::FAILURE
}
fn loader(invocation: &Invocation) -> Result<FileLoader, String> {
    Ok(FileLoader {
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
        remote: match invocation.remote.as_ref() {
            Some(settings) => {
                let cache = DeclarationCache::new(
                    settings.cache_directory.clone(),
                    RemoteLimits::default(),
                );
                cache
                    .load_all(&settings.sources)
                    .map_err(|error| error.to_string())?;
                Some((cache, settings.sources.clone()))
            }
            None => None,
        },
        import_mode: if invocation.options.module_kind == ModuleKind::CommonJs {
            ImportMode::Require
        } else {
            ImportMode::Import
        },
    })
}
struct CompiledProject {
    summary: CompileSummary,
    files: Vec<PathBuf>,
    diagnostics: Vec<Diagnostic>,
    sources: BTreeMap<String, String>,
    emit_blocked: bool,
    statistics: statistics::Statistics,
}
fn compile_project(
    invocation: &Invocation,
    loader: &FileLoader,
    emit_errors: bool,
) -> CompiledProject {
    let mut artifacts = BTreeMap::new();
    let mut declaration_modules = BTreeMap::new();
    let mut assets = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut source_ids = Vec::new();
    let mut fingerprints = Vec::new();
    let mut diagnostics = Vec::new();
    let mut has_errors = false;
    let mut sources = BTreeMap::new();
    let mut emit_blocked = false;
    let mut statistics = statistics::Statistics::default();
    let has_runtime_entries = invocation
        .entries
        .iter()
        .any(|entry| !is_declaration_path(entry));
    for entry in &invocation.entries {
        let module = loader
            .module_id(entry)
            .expect("entry has an authorized identity");
        let mut options = invocation.options.clone();
        options
            .ambient_declaration_modules
            .retain(|source| source.id != module);
        if is_declaration_path(entry) {
            if options.runtime_policy == RuntimePolicy::StrictRuntime {
                options.runtime_policy = RuntimePolicy::Checked;
            }
            options.strict_runtime_boundaries.clear();
        }
        let mut result = compile(&module, loader, options.clone());
        statistics.add(&result);
        if emit_errors && result.has_errors() && result.output.is_none() {
            let emit_started = std::time::Instant::now();
            match result.emit_for_native_cli(&options) {
                Ok(Some(output)) => result.output = Some(output),
                Ok(None) => emit_blocked = true,
                Err(diagnostic) => {
                    result.diagnostics.push(diagnostic);
                    emit_blocked = true;
                }
            }
            statistics.extra_emit(emit_started.elapsed());
        }
        for (id, module) in &result.project.modules {
            sources
                .entry(id.clone())
                .or_insert_with(|| module.source.clone());
        }
        for diagnostic in &result.diagnostics {
            if let Some(source) = result.project.source(&diagnostic.span.module) {
                sources
                    .entry(diagnostic.span.module.clone())
                    .or_insert_with(|| source.into());
            }
        }
        has_errors |= result.has_errors();
        diagnostics.extend(result.diagnostics);
        ordered_sources(
            &result.project.entry,
            &result.project,
            &mut seen,
            &mut source_ids,
        );
        if let Some(output) = result.output {
            assets.extend(output.assets);
            if !has_runtime_entries || !is_declaration_path(entry) {
                fingerprints.push(output.fingerprint);
            }
            for (module, artifact) in output.artifacts {
                artifacts.entry(module).or_insert(artifact);
            }
            for (module, source) in output.declaration_modules {
                declaration_modules.entry(module).or_insert(source);
            }
        }
    }
    fingerprints.sort();
    if fingerprints.is_empty() {
        fingerprints.extend([
            blueice_bluets::LANGUAGE_VERSION.to_string(),
            blueice_bluets::standard_library::VERSION.to_string(),
            invocation.options.resolver_fingerprint.clone(),
        ]);
    }
    let files = source_ids
        .iter()
        .filter_map(|module| loader.source_path(module).ok())
        .collect::<Vec<_>>();
    CompiledProject {
        summary: CompileSummary {
            artifacts,
            declaration_modules,
            assets,
            fingerprint: fingerprint_entries(&fingerprints),
            module_count: seen.len(),
            has_errors,
        },
        files,
        diagnostics,
        sources,
        emit_blocked,
        statistics,
    }
}
fn ordered_sources(
    module_id: &str,
    project: &Project,
    seen: &mut BTreeSet<String>,
    ordered: &mut Vec<String>,
) {
    if !seen.insert(module_id.to_string()) {
        return;
    }
    let Some(module) = project.modules.get(module_id) else {
        return;
    };
    for declaration in &module.declarations {
        let edge = match declaration {
            blueice_bluets::Declaration::Import(import) => Some((
                import.specifier.as_str(),
                import
                    .type_only
                    .then(|| {
                        import
                            .attributes
                            .as_ref()
                            .and_then(blueice_bluets::ImportAttributes::resolution_mode)
                    })
                    .flatten(),
            )),
            blueice_bluets::Declaration::TypeExport(export) => {
                export.specifier.as_deref().map(|specifier| {
                    (
                        specifier,
                        export
                            .attributes
                            .as_ref()
                            .and_then(blueice_bluets::ImportAttributes::resolution_mode),
                    )
                })
            }
            blueice_bluets::Declaration::ValueExport(export) => export
                .specifier
                .as_deref()
                .map(|specifier| (specifier, None)),
            _ => None,
        };
        if let Some(target) = edge.and_then(|(specifier, mode)| {
            project.resolved_module_with_mode(module_id, specifier, mode)
        }) {
            ordered_sources(target, project, seen, ordered);
        }
    }
    ordered.push(module_id.to_string());
}
