// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native TypeScript project commands share the owner-confined compiler.

use super::*;
use blueice_bluets::{Diagnostic, Project};
use serde_json::Value;
mod args;
mod output;
use args::Args;

pub(super) fn run(arguments: Vec<String>) -> ExitCode {
    let empty_command = arguments.is_empty();
    let pretty = arguments
        .windows(2)
        .any(|pair| pair[0] == "--pretty" && pair[1] == "true");
    let mut args = match Args::parse(arguments) {
        Ok(args) => args,
        Err(_) if empty_command => {
            eprintln!("bluetsc: a command is required\n\n{}", usage());
            return ExitCode::FAILURE;
        }
        Err(error) => return fail(&error, pretty),
    };
    let mut invocation = match resolve_invocation(Input::Config(args.project)) {
        Ok(invocation) => invocation,
        Err(error) => return fail(&error, pretty),
    };
    let Some(project) = &mut invocation.project_config else {
        return fail("--project requires a TypeScript configuration", pretty);
    };
    let show_config = args
        .flags
        .remove("showConfig")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let diagnostics_json = args
        .flags
        .remove("diagnostics-json")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    project.options.extend(args.flags);
    let enabled = |name: &str| {
        project
            .options
            .get(name)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    let pretty = enabled("pretty");
    let no_emit = enabled("noEmit");
    let list_files = enabled("listFiles");
    let list_emitted = enabled("listEmittedFiles");
    if show_config {
        let mut shown = project.show();
        if let Some(options) = shown["compilerOptions"].as_object_mut() {
            for name in ["pretty", "listFiles", "listEmittedFiles"] {
                options.remove(name);
            }
            for name in ["outDir", "rootDir"] {
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
        return fail(&error, pretty);
    }
    let loader = match loader(&invocation) {
        Ok(loader) => loader,
        Err(error) => return fail(&error, pretty),
    };
    let (mut summary, files, diagnostics) = compile_project(&invocation, &loader);
    for diagnostic in &diagnostics {
        if diagnostics_json {
            eprintln!("{}", diagnostic.to_json());
        } else {
            report(diagnostic, pretty);
        }
    }
    if list_files {
        for file in &files {
            println!("{}", file.display());
        }
    }
    if summary.has_errors {
        return ExitCode::from(2);
    }
    if let Err(error) = tsconfig::prepare_output(&mut invocation, &mut summary) {
        return fail(&error, pretty);
    }
    if no_emit {
        return ExitCode::SUCCESS;
    }
    if invocation.options.runtime_policy == RuntimePolicy::StrictRuntime
        && invocation.options.strict_runtime_boundaries.is_empty()
    {
        return fail(
            "strict-runtime build requires owner-selected strictBoundaries in config",
            pretty,
        );
    }
    let emitted = match output::publish(&invocation, &summary, &loader, &files) {
        Ok(emitted) => emitted,
        Err(error) => return fail(&error.to_string(), pretty),
    };
    if list_emitted {
        for file in emitted {
            println!("TSFILE: {}", file.display());
        }
    }
    ExitCode::SUCCESS
}
fn fail(message: &str, pretty: bool) -> ExitCode {
    if pretty {
        eprintln!("\x1b[91mbluetsc: {message}\x1b[0m");
    } else {
        eprintln!("bluetsc: {message}");
    }
    ExitCode::FAILURE
}
fn report(diagnostic: &Diagnostic, pretty: bool) {
    let text = format!(
        "{}:{}:{}: {}: {}",
        diagnostic.span.module,
        diagnostic.span.start,
        diagnostic.span.end,
        diagnostic.code,
        diagnostic.message
    );
    if pretty {
        eprintln!("\x1b[91m{text}\x1b[0m");
    } else {
        eprintln!("{text}");
    }
}
fn loader(invocation: &Invocation) -> Result<FileLoader, String> {
    Ok(FileLoader {
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
fn compile_project(
    invocation: &Invocation,
    loader: &FileLoader,
) -> (CompileSummary, Vec<PathBuf>, Vec<Diagnostic>) {
    let mut artifacts = BTreeMap::new();
    let mut declaration_modules = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut source_ids = Vec::new();
    let mut fingerprints = Vec::new();
    let mut diagnostics = Vec::new();
    let mut has_errors = false;
    let has_runtime_entries = invocation
        .entries
        .iter()
        .any(|entry| !is_declaration_path(entry));
    for entry in &invocation.entries {
        let module = project_module_id(&invocation.root, entry);
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
        let result = compile(&module, loader, options);
        has_errors |= result.has_errors();
        diagnostics.extend(result.diagnostics);
        ordered_sources(
            &result.project.entry,
            &result.project,
            &mut seen,
            &mut source_ids,
        );
        if let Some(output) = result.output {
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
    (
        CompileSummary {
            artifacts,
            declaration_modules,
            fingerprint: fingerprint_entries(&fingerprints),
            module_count: seen.len(),
            has_errors,
        },
        files,
        diagnostics,
    )
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
        let specifier = match declaration {
            blueice_bluets::Declaration::Import(import) => Some(import.specifier.as_str()),
            blueice_bluets::Declaration::TypeExport(export) => export.specifier.as_deref(),
            _ => None,
        };
        if let Some(target) =
            specifier.and_then(|specifier| project.resolved_module(module_id, specifier))
        {
            ordered_sources(target, project, seen, ordered);
        }
    }
    ordered.push(module_id.to_string());
}
