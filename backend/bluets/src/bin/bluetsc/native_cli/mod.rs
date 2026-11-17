// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native TypeScript project commands share the owner-confined compiler.

use super::*;
use blueice_bluets::{Diagnostic, Project};
use serde_json::Value;
mod args;
mod declaration_diagnostics;
mod explanations;
mod output;
mod program;
use program::compile_project;
pub(crate) mod presentation;
mod source_maps;
mod statistics;
use args::Args;

#[derive(Default)]
pub(super) struct BuildObservation {
    pub(super) emitted_outputs: Vec<PathBuf>,
    pub(super) emitted_declarations: BTreeMap<PathBuf, PathBuf>,
    pub(super) emitted_runtimes: BTreeMap<PathBuf, PathBuf>,
    pub(super) resolution_inputs: Vec<PathBuf>,
    pub(super) publication_refused: bool,
}

fn refuse_publication(observations: &mut Option<&mut BuildObservation>) {
    if let Some(observation) = observations.as_deref_mut() {
        observation.publication_refused = true;
    }
}

pub(super) fn run(arguments: Vec<String>) -> ExitCode {
    run_with_mode(arguments, false, &BTreeMap::new(), None)
}

pub(super) fn show_build_config(project: &Path, diagnostics_json: bool) -> ExitCode {
    let mut arguments = vec![
        "--project".into(),
        project.to_string_lossy().into_owned(),
        "--showConfig".into(),
    ];
    if diagnostics_json {
        arguments.push("--diagnostics-json".into());
    }
    run_with_mode(arguments, true, &BTreeMap::new(), None)
}

pub(super) fn run_build_project(
    arguments: Vec<String>,
    references: &BTreeMap<PathBuf, file_loader::ReferencedOutput>,
    observations: &mut BuildObservation,
) -> ExitCode {
    run_with_mode(arguments, true, references, Some(observations))
}

fn run_with_mode(
    arguments: Vec<String>,
    allow_references: bool,
    references: &BTreeMap<PathBuf, file_loader::ReferencedOutput>,
    mut observations: Option<&mut BuildObservation>,
) -> ExitCode {
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
        Err(error) => {
            let diagnostics = args::diagnostics(&error);
            if diagnostics.is_empty() {
                return failure(&error, pretty, diagnostics_json, None);
            }
            if diagnostics_json {
                for diagnostic in diagnostics {
                    eprintln!("{}", diagnostic.to_json());
                }
            } else {
                print!(
                    "{}",
                    presentation::format(&diagnostics, &BTreeMap::new(), pretty)
                );
            }
            return ExitCode::FAILURE;
        }
    };
    let show_config = args
        .flags
        .remove("showConfig")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    args.flags.remove("diagnostics-json");
    let source_mode = !args.source_files.is_empty();
    let mut config_diagnostic = None;
    let mut config_sources = BTreeMap::new();
    let project_path = args.project.clone();
    let resolved = if source_mode {
        tsconfig::resolve_source_files(args.source_files, args.flags.clone())
    } else {
        resolve_config_with_sources(
            args.project,
            &mut config_diagnostic,
            &mut config_sources,
            allow_references,
        )
    };
    let mut invocation = match resolved {
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
    let explain_files = enabled("explainFiles");
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
    let mut missing = Vec::new();
    if source_mode {
        invocation.entries.retain(|file| {
            if file.is_file(){return true;}
            let name=tsconfig::relative_text(&invocation.root,file);
            let mut diagnostic=Diagnostic::error(blueice_bluets::DiagnosticCode::ModuleNotFound,SourceSpan::new("",0,0),"missing source file")
                .with_typescript(6053,vec![name]);
            if let Some(counterpart)=&mut diagnostic.typescript {
                counterpart.message.push_str("\n  The file is in the program because:\n    Root file specified for compilation");
            }
            missing.push(diagnostic);false
        });
    }
    if source_mode && invocation.entries.is_empty() && !missing.is_empty() {
        // No program was selected. Preserve Native's missing-root diagnostics
        // and exit status without compiling an empty root set or publishing.
        if diagnostics_json {
            for diagnostic in &missing {
                eprintln!("{}", diagnostic.to_json());
            }
        } else {
            print!(
                "{}",
                presentation::format(&missing, &BTreeMap::new(), pretty)
            );
        }
        return ExitCode::from(if no_emit_on_error && !no_emit { 1 } else { 2 });
    }
    if let Err(error) = tsconfig::prepare(&mut invocation) {
        let source = project_path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| config_sources.get(name));
        let diagnostics = tsconfig::diagnostics::errors(
            &error,
            &project_path,
            source.map(|source| source.as_bytes()),
        );
        if diagnostics.is_empty() {
            return failure(&error, pretty, diagnostics_json, None);
        }
        if diagnostics_json {
            for diagnostic in diagnostics {
                eprintln!("{}", diagnostic.to_json());
            }
        } else {
            print!(
                "{}",
                presentation::format(&diagnostics, &config_sources, pretty)
            );
        }
        return ExitCode::FAILURE;
    }
    let loader = match loader(&invocation, references) {
        Ok(loader) => loader,
        Err(error) => return fail(&error, pretty),
    };
    let CompiledProject {
        mut summary,
        files,
        mut diagnostics,
        sources,
        emit_blocked,
        mut statistics,
        explanations,
    } = compile_project(&invocation, &loader, !no_emit && !no_emit_on_error);
    summary.has_errors |= !missing.is_empty();
    diagnostics.extend(missing);
    // Counterpart locations can be narrower than the internal BTS span.
    // Native presentation orders the final counterpart span and diagnostic code.
    diagnostics.sort_by_key(|diagnostic| {
        let (span, code) = diagnostic
            .typescript
            .as_ref()
            .map_or((&diagnostic.span, u32::MAX), |ts| (&ts.span, ts.code));
        (span.module.clone(), span.start, span.end, code)
    });
    if let Some(observation) = observations.as_deref_mut() {
        let inputs = &mut observation.resolution_inputs;
        inputs.extend(files.iter().cloned());
        inputs.extend(loader.relative.observed_paths());
        if let Some(packages) = &loader.packages {
            inputs.extend(packages.observed_paths());
        }
        inputs.sort();
        inputs.dedup();
    }
    for diagnostic in &diagnostics {
        if diagnostics_json {
            let mut diagnostic = diagnostic.clone();
            if allow_references && !diagnostic.span.module.is_empty() {
                if let Ok(path) = loader.source_path(&diagnostic.span.module) {
                    let name = path.to_string_lossy().replace('\\', "/");
                    diagnostic.span.module = name.clone();
                    if let Some(counterpart) = &mut diagnostic.typescript {
                        counterpart.span.module = name;
                    }
                }
            }
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
    if explain_files {
        print!("{explanations}");
    }
    if emit_blocked {
        refuse_publication(&mut observations);
    }
    if summary.has_errors && ((!no_emit && no_emit_on_error) || emit_blocked) {
        if show_diagnostics {
            print!("{}", statistics.display(started.elapsed()));
        }
        return ExitCode::from(1);
    }
    if let Err(error) = tsconfig::prepare_output(&mut invocation, &mut summary) {
        refuse_publication(&mut observations);
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
        refuse_publication(&mut observations);
        return fail(
            "strict-runtime build requires owner-selected strictBoundaries in config",
            pretty,
        );
    }
    // Ordinary graph builds retain their established conservative cleanup.
    // Only this internal call path may release modified obsolete regular files;
    // explicit incremental, direct project and owner publications remain strict.
    let retirement = if observations.is_some()
        && invocation
            .project_config
            .as_ref()
            .is_some_and(|project| project.options.get("incremental") != Some(&Value::Bool(true)))
    {
        publication_ownership::RetirementPolicy::PreserveModified
    } else {
        publication_ownership::RetirementPolicy::Strict
    };
    let write_started = std::time::Instant::now();
    let output::Published {
        paths: emitted,
        declarations,
        runtimes,
    } = match output::publish(&invocation, &summary, &loader, &files, retirement) {
        Ok(emitted) => emitted,
        Err(error) => {
            refuse_publication(&mut observations);
            return fail(&error.to_string(), pretty);
        }
    };
    if let Some(observation) = observations {
        observation.emitted_outputs.extend(emitted.iter().cloned());
        observation.emitted_declarations.extend(declarations);
        observation.emitted_runtimes.extend(runtimes);
    }
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
fn loader(
    invocation: &Invocation,
    references: &BTreeMap<PathBuf, file_loader::ReferencedOutput>,
) -> Result<FileLoader, String> {
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
        allow_js: invocation.options.allow_js,
        max_node_module_js_depth: invocation.max_node_module_js_depth,
        javascript_depths: Default::default(),
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
        references: references.clone(),
    })
}
struct CompiledProject {
    summary: CompileSummary,
    files: Vec<PathBuf>,
    diagnostics: Vec<Diagnostic>,
    sources: BTreeMap<String, String>,
    emit_blocked: bool,
    statistics: statistics::Statistics,
    explanations: String,
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
                if import.equals_require {
                    Some(ImportMode::Require)
                } else {
                    import
                        .type_only
                        .then(|| {
                            import
                                .attributes
                                .as_ref()
                                .and_then(blueice_bluets::ImportAttributes::resolution_mode)
                        })
                        .flatten()
                },
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
    if let Some(provider) = project.resolved_module(module_id, "tslib") {
        ordered_sources(provider, project, seen, ordered);
    }
    if let Some(provider) = project.jsx_runtime_namespace_provider(module_id) {
        ordered_sources(provider, project, seen, ordered);
    }
    for (_, target) in project.resolved_import_queries(module_id) {
        ordered_sources(target, project, seen, ordered);
    }
    ordered.push(module_id.to_string());
}
