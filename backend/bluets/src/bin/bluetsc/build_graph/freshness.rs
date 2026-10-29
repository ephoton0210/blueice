// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Conservative per-project freshness for the owner-confined build graph.

use super::*;
use serde::{Deserialize, Serialize};
use std::io::Write;
#[path = "freshness/interop.rs"]
mod interop;
#[path = "freshness/interop_model.rs"]
mod interop_model;
#[path = "freshness/resolution_inputs.rs"]
mod resolution_inputs;
#[path = "freshness/runner.rs"]
mod runner;
pub(super) use runner::run;

// G-M2 adds failure state and native interchange provenance.
const STATE_VERSION: u8 = 25;
const STATE_PRODUCER: &str = "bluetsc";
const MAX_STATE_BYTES: u64 = 8 * 1024 * 1024;

pub(super) struct Inputs {
    options: serde_json::Map<String, Value>,
    emit_root: Option<PathBuf>,
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
            options: serde_json::Map::new(),
            emit_root: None,
            config_fingerprint,
            config_files,
            files,
            observe_package_scopes,
        }
    }

    pub(super) fn with_options(
        mut self,
        options: serde_json::Map<String, Value>,
        emit_root: PathBuf,
    ) -> Self {
        self.options = options;
        self.emit_root = Some(emit_root);
        self
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
    status: StateStatus,
    resolution_inputs: Vec<String>,
    producer: String,
    version: u8,
    fingerprint: String,
    outputs: Vec<String>,
    output_hashes: BTreeMap<String, String>,
    declarations: BTreeMap<String, String>,
    runtimes: BTreeMap<String, String>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StateStatus {
    failed: bool,
    pending_dependencies: bool,
    native_hash: Option<String>,
}

enum LoadedState {
    Missing,
    Invalid,
    Valid(Box<State>),
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
                LoadedState::Valid(Box::new(state))
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
        resolution_inputs: Vec<String>,
        status: StateStatus,
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
            status,
            resolution_inputs,
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
