// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Conservative per-project freshness for the owner-confined build graph.

use super::*;
use serde::{Deserialize, Serialize};
use std::io::Write;

const STATE_VERSION: u8 = 2;

pub(super) struct Inputs {
    config_fingerprint: String,
    config_files: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

impl Inputs {
    pub(super) fn new(
        config_fingerprint: String,
        config_files: Vec<PathBuf>,
        files: Vec<PathBuf>,
    ) -> Self {
        Self {
            config_fingerprint,
            config_files,
            files,
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
        Ok(sha256_label(&bytes))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u8,
    fingerprint: String,
    outputs: Vec<String>,
    declarations: BTreeMap<String, String>,
    runtimes: BTreeMap<String, String>,
}

impl State {
    fn load(project: &Path) -> io::Result<Option<Self>> {
        let path = state_path(project);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(io::Error::other("build state path is not a regular file"));
        }
        let bytes = fs::read(path)?;
        Ok(serde_json::from_slice::<Self>(&bytes)
            .ok()
            .filter(|state| state.version == STATE_VERSION))
    }

    fn outputs_exist(&self, project: &Path) -> bool {
        !self.outputs.is_empty()
            && self
                .outputs
                .iter()
                .all(|output| safe_output(project, output).is_some())
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
            version: STATE_VERSION,
            fingerprint,
            outputs,
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

fn state_path(project: &Path) -> PathBuf {
    let name = project
        .file_name()
        .expect("canonical config has file name")
        .to_string_lossy();
    project.with_file_name(format!("{name}.bluetsc-build-state.json"))
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

pub(super) fn run(graph: &Graph, pretty: bool, verbose: bool) -> ExitCode {
    let mut declaration_changed = BTreeMap::new();
    let mut available_declarations = BTreeMap::new();
    for project in &graph.order {
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
            Ok(state) => state,
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
            return result;
        }
        let after = output_names(project, &emitted)
            .and_then(|names| declaration_signatures(project, &names));
        let after = match after {
            Ok(after) => after,
            Err(error) => {
                eprintln!("bluetsc: cannot read emitted declarations: {error}");
                return ExitCode::FAILURE;
            }
        };
        declaration_changed.insert(project.clone(), before != after);
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
    ExitCode::SUCCESS
}
