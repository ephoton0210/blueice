// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Conservative per-project freshness for the owner-confined build graph.

use super::*;
use serde::{Deserialize, Serialize};
use std::io::Write;

// G-T2 changes inference and declarations: older state must rebuild.
const STATE_VERSION: u8 = 13;
const STATE_PRODUCER: &str = "bluetsc";
const MAX_STATE_BYTES: u64 = 8 * 1024 * 1024;

pub(super) struct Inputs {
    config_fingerprint: String,
    config_files: Vec<PathBuf>,
    files: Vec<PathBuf>,
    observe_package_scopes: bool,
}

impl Inputs {
    pub(super) fn new(
        config_fingerprint: String,
        config_files: Vec<PathBuf>,
        files: Vec<PathBuf>,
        observe_package_scopes: bool,
    ) -> Self {
        Self {
            config_fingerprint,
            config_files,
            files,
            observe_package_scopes,
        }
    }

    pub(super) fn files(&self) -> &[PathBuf] {
        &self.files
    }

    fn fingerprint(&self, graph: &Graph) -> Result<String, String> {
        let mut bytes = self.config_fingerprint.as_bytes().to_vec();
        bytes.push(0);
        for path in &self.config_files {
            graph.reader.authorize_future(path, "build config", false)?;
            let canonical = fs::canonicalize(path).map_err(|error| {
                format!("cannot resolve build config {}: {error}", path.display())
            })?;
            let metadata = fs::metadata(&canonical).map_err(|error| {
                format!("cannot stat build config {}: {error}", canonical.display())
            })?;
            let modified = metadata
                .modified()
                .map_err(|error| format!("cannot date build config: {error}"))?
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            bytes.extend(canonical.to_string_lossy().as_bytes());
            bytes.push(0);
            bytes.extend(modified.to_le_bytes());
            bytes.push(0);
        }
        for path in &self.files {
            graph.reader.authorize_future(path, "build source", false)?;
            let canonical = fs::canonicalize(path).map_err(|error| {
                format!("cannot resolve build source {}: {error}", path.display())
            })?;
            let identity = canonical.strip_prefix(&graph.root).map_err(|_| {
                format!(
                    "build source {} is outside project root",
                    canonical.display()
                )
            })?;
            bytes.extend(identity.to_string_lossy().as_bytes());
            bytes.push(0);
            let source = fs::read(&canonical).map_err(|error| {
                format!("cannot read build source {}: {error}", canonical.display())
            })?;
            bytes.extend(sha256_label(&source).bytes());
            bytes.push(0);
        }
        if self.observe_package_scopes {
            self.add_package_scope_observations(graph, &mut bytes)?;
        }
        Ok(sha256_label(&bytes))
    }

    fn add_package_scope_observations(
        &self,
        graph: &Graph,
        bytes: &mut Vec<u8>,
    ) -> Result<(), String> {
        let mut candidates = BTreeSet::new();
        for source in &self.files {
            let canonical = fs::canonicalize(source).map_err(|error| {
                format!("cannot resolve build source {}: {error}", source.display())
            })?;
            let mut directory = canonical.parent();
            while let Some(current) = directory.filter(|path| path.starts_with(&graph.root)) {
                candidates.insert(current.join("package.json"));
                if current == graph.root {
                    break;
                }
                directory = current.parent();
            }
        }
        for candidate in candidates {
            graph
                .reader
                .authorize_future(&candidate, "package scope", false)?;
            let identity = candidate
                .strip_prefix(&graph.root)
                .map_err(|_| "package scope is outside project root".to_string())?;
            bytes.extend(identity.to_string_lossy().bytes());
            bytes.push(0);
            match fs::symlink_metadata(&candidate) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => bytes.push(0),
                Err(error) => {
                    return Err(format!(
                        "cannot inspect package scope {}: {error}",
                        candidate.display()
                    ));
                }
                Ok(_) => {
                    let canonical = fs::canonicalize(&candidate).map_err(|error| {
                        format!(
                            "cannot resolve package scope {}: {error}",
                            candidate.display()
                        )
                    })?;
                    let target = canonical
                        .strip_prefix(&graph.root)
                        .map_err(|_| "package scope resolves outside project root".to_string())?;
                    let metadata = fs::metadata(&canonical).map_err(|error| {
                        format!(
                            "cannot inspect package scope {}: {error}",
                            canonical.display()
                        )
                    })?;
                    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
                        return Err("package scope is not a bounded regular file".into());
                    }
                    let contents = fs::read(&canonical).map_err(|error| {
                        format!("cannot read package scope {}: {error}", canonical.display())
                    })?;
                    bytes.push(1);
                    bytes.extend(target.to_string_lossy().bytes());
                    bytes.push(0);
                    bytes.extend(sha256_label(&contents).bytes());
                }
            }
            bytes.push(0);
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    producer: String,
    version: u8,
    fingerprint: String,
    outputs: Vec<String>,
    output_hashes: BTreeMap<String, String>,
    declarations: BTreeMap<String, String>,
    runtimes: BTreeMap<String, String>,
}

enum LoadedState {
    Missing,
    Invalid,
    Valid(State),
}

impl State {
    fn load(project: &Path) -> io::Result<LoadedState> {
        let path = state_path(project);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(LoadedState::Missing);
            }
            Err(error) => return Err(error),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(io::Error::other("build state path is not a regular file"));
        }
        if metadata.len() > MAX_STATE_BYTES {
            return Ok(LoadedState::Invalid);
        }
        let bytes = fs::read(path)?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Ok(LoadedState::Invalid);
        }
        Ok(match serde_json::from_slice::<Self>(&bytes) {
            Ok(state) if state.version == STATE_VERSION && state.producer == STATE_PRODUCER => {
                LoadedState::Valid(state)
            }
            _ => LoadedState::Invalid,
        })
    }

    fn outputs_exist(&self, project: &Path) -> bool {
        !self.outputs.is_empty()
            && self
                .outputs
                .iter()
                .all(|output| safe_output(project, output).is_some())
    }

    fn remove_stale_outputs(
        &self,
        project: &Path,
        current_outputs: &[String],
        selected_files: &[PathBuf],
    ) -> io::Result<()> {
        let current = current_outputs.iter().collect::<BTreeSet<_>>();
        for old in &self.outputs {
            if current.contains(old) || !is_generated_source_output(old) {
                continue;
            }
            let Some(expected_hash) = self.output_hashes.get(old) else {
                continue;
            };
            let Some(path) = safe_output(project, old) else {
                continue;
            };
            if selected_files.contains(&path) {
                continue;
            }
            let bytes = fs::read(&path)?;
            if sha256_label(&bytes) == *expected_hash {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    fn declaration_paths(
        &self,
        project: &Path,
        selected_files: &[PathBuf],
    ) -> Option<BTreeMap<PathBuf, file_loader::ReferencedOutput>> {
        let mut paths = BTreeMap::new();
        for (source, declaration) in &self.declarations {
            let source_path = safe_output(project, source)?;
            if !selected_files.contains(&source_path)
                || !self.outputs.contains(declaration)
                || ![".d.ts", ".d.mts", ".d.cts"]
                    .iter()
                    .any(|extension| declaration.ends_with(extension))
            {
                return None;
            }
            let declaration = safe_output(project, declaration)?;
            let runtime = self.runtimes.get(source).map(|runtime| {
                self.outputs
                    .contains(runtime)
                    .then(|| safe_output(project, runtime))
                    .flatten()
            });
            let runtime = match runtime {
                Some(Some(runtime)) => Some(runtime),
                Some(None) => return None,
                None => None,
            };
            paths.insert(
                source_path,
                file_loader::ReferencedOutput {
                    declaration,
                    runtime,
                },
            );
        }
        if selected_files.iter().any(|source| {
            ![".d.ts", ".d.mts", ".d.cts"]
                .iter()
                .any(|extension| source.to_string_lossy().ends_with(extension))
                && !paths.contains_key(source)
        }) {
            return None;
        }
        Some(paths)
    }

    fn save(
        project: &Path,
        fingerprint: String,
        outputs: &[PathBuf],
        declarations: &BTreeMap<PathBuf, PathBuf>,
        runtimes: &BTreeMap<PathBuf, PathBuf>,
    ) -> io::Result<Self> {
        let directory = project.parent().expect("canonical config has parent");
        let outputs = outputs
            .iter()
            .map(|path| {
                path.strip_prefix(directory)
                    .map(|relative| relative.to_string_lossy().into_owned())
                    .map_err(|_| io::Error::other("build output is outside its project root"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let output_hashes = outputs
            .iter()
            .map(|output| {
                let path = safe_output(project, output)
                    .ok_or_else(|| io::Error::other("build output is missing or unsafe"))?;
                let bytes = fs::read(path)?;
                Ok((output.clone(), sha256_label(&bytes)))
            })
            .collect::<io::Result<BTreeMap<_, _>>>()?;
        let declarations = declarations
            .iter()
            .map(|(source, declaration)| {
                let source = source
                    .strip_prefix(directory)
                    .map_err(|_| io::Error::other("declaration source is outside project root"))?;
                let declaration = declaration
                    .strip_prefix(directory)
                    .map_err(|_| io::Error::other("declaration output is outside project root"))?;
                Ok((
                    source.to_string_lossy().into_owned(),
                    declaration.to_string_lossy().into_owned(),
                ))
            })
            .collect::<io::Result<BTreeMap<_, _>>>()?;
        let runtimes = runtimes
            .iter()
            .map(|(source, runtime)| {
                let source = source
                    .strip_prefix(directory)
                    .map_err(|_| io::Error::other("runtime source is outside project root"))?;
                let runtime = runtime
                    .strip_prefix(directory)
                    .map_err(|_| io::Error::other("runtime output is outside project root"))?;
                Ok((
                    source.to_string_lossy().into_owned(),
                    runtime.to_string_lossy().into_owned(),
                ))
            })
            .collect::<io::Result<BTreeMap<_, _>>>()?;
        let state = Self {
            producer: STATE_PRODUCER.into(),
            version: STATE_VERSION,
            fingerprint,
            outputs,
            output_hashes,
            declarations,
            runtimes,
        };
        let path = state_path(project);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let stage = directory.join(format!(
            ".bluetsc-build-state-{}-{nonce}.tmp",
            std::process::id()
        ));
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&stage)?;
            file.write_all(&serde_json::to_vec(&state).map_err(io::Error::other)?)?;
            file.sync_all()?;
            fs::rename(&stage, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(stage);
        }
        result.map(|()| state)
    }
}

fn is_generated_source_output(path: &str) -> bool {
    [
        ".js", ".jsx", ".mjs", ".cjs", ".d.ts", ".d.mts", ".d.cts", ".map",
    ]
    .iter()
    .any(|suffix| path.ends_with(suffix))
}

fn state_path(project: &Path) -> PathBuf {
    project.with_extension("bluetsc.tsbuildinfo")
}

fn safe_output(project: &Path, output: &str) -> Option<PathBuf> {
    let directory = project.parent()?;
    let mut path = directory.to_path_buf();
    let mut parts = Path::new(output).components().peekable();
    parts.peek()?;
    for component in parts {
        let std::path::Component::Normal(name) = component else {
            return None;
        };
        path.push(name);
        let metadata = fs::symlink_metadata(&path).ok()?;
        if metadata.file_type().is_symlink() {
            return None;
        }
    }
    fs::metadata(&path).ok()?.is_file().then_some(path)
}

fn declaration_signatures(
    project: &Path,
    outputs: &[String],
) -> io::Result<BTreeMap<String, String>> {
    let mut signatures = BTreeMap::new();
    for output in outputs {
        if ![".d.ts", ".d.mts", ".d.cts"]
            .iter()
            .any(|extension| output.ends_with(extension))
        {
            continue;
        }
        let path = safe_output(project, output)
            .ok_or_else(|| io::Error::other("declaration output is missing or unsafe"))?;
        let source = fs::read_to_string(path)?;
        let signature = match blueice_bluets::lex(output, &source) {
            Ok(tokens) => {
                let mut bytes = Vec::new();
                for token in tokens {
                    bytes.extend(token.text.bytes());
                    bytes.push(0);
                }
                sha256_label(&bytes)
            }
            Err(_) => sha256_label(source.as_bytes()),
        };
        signatures.insert(output.clone(), signature);
    }
    Ok(signatures)
}

fn output_names(project: &Path, outputs: &[PathBuf]) -> io::Result<Vec<String>> {
    let directory = project.parent().expect("canonical config has parent");
    outputs
        .iter()
        .map(|path| {
            path.strip_prefix(directory)
                .map(|relative| relative.to_string_lossy().into_owned())
                .map_err(|_| io::Error::other("build output is outside its project root"))
        })
        .collect()
}

pub(super) fn run(
    graph: &Graph,
    pretty: bool,
    verbose: bool,
    stop_build_on_errors: bool,
) -> ExitCode {
    let mut declaration_changed = BTreeMap::new();
    let mut available_declarations = BTreeMap::new();
    let mut failed = BTreeMap::<PathBuf, bool>::new();
    let mut successful_output = false;
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
        let state = match State::load(project) {
            Ok(LoadedState::Missing) => None,
            Ok(LoadedState::Invalid) => {
                eprintln!(
                    "bluetsc: invalid or foreign .tsbuildinfo for {}; rebuilding project",
                    graph.display_path(project)
                );
                None
            }
            Ok(LoadedState::Valid(state)) => Some(state),
            Err(error) => {
                eprintln!("bluetsc: cannot read build state: {error}");
                return ExitCode::FAILURE;
            }
        };
        let outputs_exist = state
            .as_ref()
            .is_some_and(|state| state.outputs_exist(project));
        if verbose
            && state
                .as_ref()
                .is_some_and(|state| state.fingerprint == fingerprint && !outputs_exist)
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
        let up_to_date = state
            .as_ref()
            .is_some_and(|state| state.fingerprint == fingerprint && outputs_exist)
            && !dependency_changed;
        if let Some(paths) = state_declarations.filter(|_| up_to_date) {
            if verbose {
                println!("Project {} is up to date", graph.display_path(project));
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
        let result = native_cli::run_build_project(
            vec![
                "--project".into(),
                project.to_string_lossy().into_owned(),
                "--pretty".into(),
                pretty.to_string(),
            ],
            &mut emitted,
            &mut declarations,
            &mut runtimes,
            &references,
        );
        if result != ExitCode::SUCCESS {
            if stop_build_on_errors {
                failed.insert(project.clone(), false);
                continue;
            }
            return result;
        }
        successful_output |= !emitted.is_empty();
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
            if let Err(error) =
                previous.remove_stale_outputs(project, &output_names, inputs.files())
            {
                eprintln!("bluetsc: cannot remove obsolete build output: {error}");
                return ExitCode::FAILURE;
            }
        }
        let state = match State::save(project, fingerprint, &emitted, &declarations, &runtimes) {
            Ok(state) => state,
            Err(error) => {
                eprintln!("bluetsc: cannot publish build state: {error}");
                return ExitCode::FAILURE;
            }
        };
        let Some(paths) = state.declaration_paths(project, inputs.files()) else {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_fingerprint_changes_even_when_mtime_is_restored() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "bluets-projkey-hash-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let source = root.join("main.ts");
        fs::write(&source, "export const value = 1;\n").unwrap();
        let modified = fs::metadata(&source).unwrap().modified().unwrap();
        let inputs = Inputs::new("config".into(), Vec::new(), vec![source.clone()], false);
        let graph = Graph::new(root.clone());
        let first = inputs.fingerprint(&graph).unwrap();
        fs::write(&source, "export const value = 2;\n").unwrap();
        fs::File::options()
            .write(true)
            .open(&source)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert_eq!(fs::metadata(&source).unwrap().modified().unwrap(), modified);
        assert_ne!(inputs.fingerprint(&graph).unwrap(), first);
        fs::remove_dir_all(root).unwrap();
    }
}
