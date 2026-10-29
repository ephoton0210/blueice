// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Ordered project execution and failure propagation.
use super::*;

pub(in super::super) fn run(
    graph: &Graph,
    pretty: bool,
    verbose: bool,
    stop_build_on_errors: bool,
    diagnostics_json: bool,
) -> ExitCode {
    let mut declaration_changed = BTreeMap::new();
    let mut available_declarations = BTreeMap::new();
    let mut failed = BTreeMap::<PathBuf, bool>::new();
    let mut pending = BTreeSet::new();
    let mut touched = BTreeSet::new();
    let mut successful_output = false;
    for project in &graph.order {
        if interop_model::eligible(&graph.inputs[project]) {
            if let Err(error) = interop_model::preflight(graph, project) {
                eprintln!("bluetsc: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    for project in &graph.order {
        if stop_build_on_errors {
            let blocked_by = graph
                .references
                .get(project)
                .into_iter()
                .flatten()
                .find_map(|reference| failed.get(reference).map(|blocked| (reference, *blocked)));
            if let Some((reference, upstream_blocked)) = blocked_by {
                if verbose {
                    let reason = if upstream_blocked {
                        "was not built"
                    } else {
                        "has errors"
                    };
                    let project_name = graph.display_path(project);
                    let dependency = graph.display_path(
                        reference
                            .parent()
                            .expect("canonical referenced config has parent"),
                    );
                    println!(
                        "Project '{project_name}' can't be built because its dependency \
                         '{dependency}' {reason}"
                    );
                    println!(
                        "Skipping build of project '{project_name}' because its dependency \
                         '{dependency}' {reason}"
                    );
                }
                failed.insert(project.clone(), true);
                continue;
            }
        }
        let inputs = graph
            .inputs
            .get(project)
            .expect("visited project has inputs");
        let fingerprint = match inputs.fingerprint(graph) {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                eprintln!("bluetsc: {error}");
                return ExitCode::FAILURE;
            }
        };
        let mut state = match State::load(project) {
            Ok(LoadedState::Missing) => None,
            Ok(LoadedState::Invalid) => {
                eprintln!(
                    "bluetsc: invalid or foreign .tsbuildinfo for {}; rebuilding project",
                    graph.display_path(project)
                );
                None
            }
            Ok(LoadedState::Valid(state)) => Some(*state),
            Err(error) => {
                eprintln!("bluetsc: cannot read build state: {error}");
                return ExitCode::FAILURE;
            }
        };
        let native_hash = if interop_model::eligible(inputs) {
            match interop::current_hash(graph, project) {
                Ok(hash) => hash,
                Err(error) => {
                    eprintln!("bluetsc: {error}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            None
        };
        let native_matches = state
            .as_ref()
            .is_some_and(|s| s.status.native_hash == native_hash);
        if state.is_none() || !native_matches {
            match interop::import(graph, project, &fingerprint) {
                Ok(Some(imported)) => state = Some(imported),
                Ok(None) => {}
                Err(error) => {
                    eprintln!("bluetsc: cannot validate native build state: {error}");
                    return ExitCode::FAILURE;
                }
            }
        }
        let native_matches = state
            .as_ref()
            .is_some_and(|s| s.status.native_hash == native_hash);
        let outputs_exist = state
            .as_ref()
            .is_some_and(|state| state.outputs_exist(project));
        let inputs_match = state.as_ref().is_some_and(|state| {
            resolution_inputs::fingerprint(graph, &fingerprint, &state.resolution_inputs)
                .is_ok_and(|fingerprint| state.fingerprint == fingerprint)
        });
        if verbose
            && state
                .as_ref()
                .is_some_and(|_| inputs_match && !outputs_exist)
        {
            println!(
                "Project {} is out of date because a recorded build output is missing or unsafe",
                graph.display_path(project)
            );
        }
        let state_declarations = state
            .as_ref()
            .and_then(|state| state.declaration_paths(project, inputs.files()));
        let dependency_changed = graph.references.get(project).is_some_and(|references| {
            references
                .iter()
                .any(|reference| declaration_changed.get(reference) == Some(&true))
        });
        let dependency_failed = graph.references.get(project).is_some_and(|references| {
            references.iter().any(|reference| {
                pending.contains(reference)
                    || available_declarations.get(reference).is_some_and(
                        |paths: &BTreeMap<PathBuf, file_loader::ReferencedOutput>| {
                            paths.values().any(|output| !output.declaration.is_file())
                        },
                    )
            })
        });
        let dependency_touched = graph.references.get(project).is_some_and(|references| {
            references
                .iter()
                .any(|reference| touched.contains(reference))
        });
        let incomplete = state
            .as_ref()
            .is_some_and(|s| s.status.failed || s.status.pending_dependencies);
        let up_to_date = native_matches
            && !incomplete
            && !dependency_failed
            && state
                .as_ref()
                .is_some_and(|_| inputs_match && outputs_exist)
            && !dependency_changed;
        if let Some(paths) = state_declarations.filter(|_| up_to_date) {
            if verbose {
                if dependency_touched {
                    println!(
                        "Project {} is up to date with dependency declarations",
                        graph.display_path(project)
                    );
                    println!(
                        "Updating output timestamps: {}",
                        graph.display_path(project)
                    );
                } else {
                    println!("Project {} is up to date", graph.display_path(project));
                }
            }
            if dependency_touched {
                let state = state.as_ref().unwrap();
                let mut paths = state
                    .outputs
                    .iter()
                    .filter_map(|p| safe_output(project, p))
                    .collect::<Vec<_>>();
                paths.push(state_path(project));
                if interop_model::eligible(inputs) {
                    paths.push(interop_model::path(project, inputs));
                }
                for path in paths {
                    if let Err(error) = fs::File::open(path).and_then(|file| {
                        file.set_times(fs::FileTimes::new().set_modified(SystemTime::now()))
                    }) {
                        eprintln!("bluetsc: cannot refresh output timestamps: {error}");
                        return ExitCode::FAILURE;
                    }
                }
                touched.insert(project.clone());
                successful_output |= !state.outputs.is_empty();
            }
            declaration_changed.insert(project.clone(), false);
            available_declarations.insert(project.clone(), paths);
            continue;
        }
        let before = if outputs_exist {
            declaration_signatures(project, &state.as_ref().expect("outputs exist").outputs)
                .unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        if verbose {
            println!("Building project: {}", graph.display_path(project));
        }
        let mut emitted = Vec::new();
        let mut observed_inputs = Vec::new();
        let mut declarations = BTreeMap::new();
        let mut runtimes = BTreeMap::new();
        let mut references = BTreeMap::new();
        for reference in graph.references.get(project).into_iter().flatten() {
            references.extend(
                available_declarations
                    .get(reference)
                    .expect("reference built before dependent")
                    .iter()
                    .map(|(source, declaration)| (source.clone(), declaration.clone())),
            );
        }
        let mut arguments = vec![
            "--project".into(),
            project.to_string_lossy().into_owned(),
            "--pretty".into(),
            pretty.to_string(),
        ];
        if diagnostics_json {
            arguments.push("--diagnostics-json".into());
        }
        let result = native_cli::run_build_project(
            arguments,
            &mut emitted,
            &mut declarations,
            &mut runtimes,
            &references,
            &mut observed_inputs,
        );
        touched.insert(project.clone());
        if result != ExitCode::SUCCESS {
            failed.insert(project.clone(), false);
        }
        if dependency_failed {
            pending.insert(project.clone());
        }
        successful_output |= result == ExitCode::SUCCESS && !emitted.is_empty();
        let output_names = match output_names(project, &emitted) {
            Ok(output_names) => output_names,
            Err(error) => {
                eprintln!("bluetsc: cannot inspect emitted outputs: {error}");
                return ExitCode::FAILURE;
            }
        };
        let after = match declaration_signatures(project, &output_names) {
            Ok(after) => after,
            Err(error) => {
                eprintln!("bluetsc: cannot read emitted declarations: {error}");
                return ExitCode::FAILURE;
            }
        };
        declaration_changed.insert(project.clone(), before != after);
        if let Some(previous) = &state {
            if result == ExitCode::SUCCESS {
                if let Err(error) =
                    previous.remove_stale_outputs(project, &output_names, inputs.files())
                {
                    eprintln!("bluetsc: cannot remove obsolete build output: {error}");
                    return ExitCode::FAILURE;
                }
            }
        }
        let observed_inputs = observed_inputs
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let mut status = StateStatus {
            failed: result != ExitCode::SUCCESS,
            pending_dependencies: dependency_failed,
            native_hash: None,
        };
        status.native_hash = match interop::publish(
            graph,
            project,
            &fingerprint,
            &declarations,
            &observed_inputs,
            &status,
        ) {
            Ok(hash) => hash,
            Err(error) => {
                eprintln!("bluetsc: cannot publish native build state: {error}");
                return ExitCode::FAILURE;
            }
        };
        let fingerprint =
            match resolution_inputs::fingerprint(graph, &fingerprint, &observed_inputs) {
                Ok(fingerprint) => fingerprint,
                Err(error) => {
                    eprintln!("bluetsc: cannot fingerprint resolved inputs: {error}");
                    return ExitCode::FAILURE;
                }
            };
        let state = match State::save(
            project,
            fingerprint,
            &emitted,
            &declarations,
            &runtimes,
            observed_inputs,
            status,
        ) {
            Ok(state) => state,
            Err(error) => {
                eprintln!("bluetsc: cannot publish build state: {error}");
                return ExitCode::FAILURE;
            }
        };
        let paths = if let Some(paths) = state.declaration_paths(project, inputs.files()) {
            paths
        } else if result != ExitCode::SUCCESS {
            match expected_paths(graph, project) {
                Ok(paths) => paths,
                Err(error) => {
                    eprintln!("bluetsc: {error}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            eprintln!("bluetsc: cannot validate published declaration paths");
            return ExitCode::FAILURE;
        };
        available_declarations.insert(project.clone(), paths);
    }
    if failed.is_empty() {
        ExitCode::SUCCESS
    } else if successful_output {
        ExitCode::from(2)
    } else {
        ExitCode::from(1)
    }
}

fn expected_paths(
    graph: &Graph,
    project: &Path,
) -> Result<BTreeMap<PathBuf, file_loader::ReferencedOutput>, String> {
    let inputs = &graph.inputs[project];
    let mut result = BTreeMap::new();
    for source in inputs.files().iter().filter(|p| !is_declaration_path(p)) {
        let source = fs::canonicalize(source).map_err(|e| e.to_string())?;
        let declaration = interop_model::declaration(project, inputs, &source)
            .ok_or("cannot plan referenced declaration")?;
        graph
            .reader
            .authorize_future(&declaration, "referenced declaration", false)?;
        let runtime = declaration.with_extension("").with_extension("js");
        let runtime = (inputs.options.get("emitDeclarationOnly") != Some(&Value::Bool(true))
            && runtime.is_file())
        .then_some(runtime);
        result.insert(
            source,
            file_loader::ReferencedOutput {
                declaration,
                runtime,
            },
        );
    }
    Ok(result)
}
