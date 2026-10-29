// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Verified native state cross-use adds no filesystem or runtime authority.
use super::interop_model as model;
use super::*;
use serde_json::json;
use std::io::Read;

pub(super) fn current_hash(graph: &Graph, project: &Path) -> io::Result<Option<String>> {
    let path = model::preflight(graph, project)?;
    let metadata = match fs::metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if metadata.len() > MAX_STATE_BYTES {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_STATE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Ok(None);
    }
    Ok(Some(sha256_label(&bytes)))
}
pub(super) fn import(
    graph: &Graph,
    project: &Path,
    fingerprint: &str,
) -> io::Result<Option<State>> {
    let inputs = &graph.inputs[project];
    if !model::eligible(inputs) {
        return Ok(None);
    }
    let path = model::preflight(graph, project)?;
    let metadata = match fs::metadata(&path) {
        Ok(v) => v,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if metadata.len() > MAX_STATE_BYTES {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)?
        .take(MAX_STATE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Ok(None);
    }
    let Ok(info) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(None);
    };
    if info["version"] != "5.9.3" || info["options"] != model::options(project, inputs) {
        return Ok(None);
    }
    if !info.as_object().is_some_and(|object| {
        object.keys().all(|name| {
            matches!(
                name.as_str(),
                "fileNames"
                    | "fileInfos"
                    | "root"
                    | "options"
                    | "latestChangedDtsFile"
                    | "version"
                    | "blueice"
                    | "errors"
                    | "checkPending"
                    | "fileIdsList"
                    | "referencedMap"
                    | "semanticDiagnosticsPerFile"
                    | "emitDiagnosticsPerFile"
                    | "changeFileSet"
                    | "affectedFilesPendingEmit"
                    | "emitSignatures"
                    | "resolvedRoot"
            )
        })
    }) {
        return Ok(None);
    }
    for name in ["errors", "checkPending"] {
        if info.get(name).is_some_and(|v| v != false) {
            return Ok(None);
        }
    }
    for name in [
        "semanticDiagnosticsPerFile",
        "emitDiagnosticsPerFile",
        "changeFileSet",
        "affectedFilesPendingEmit",
        "emitSignatures",
    ] {
        if info.get(name).is_some_and(|v| v != &json!([])) {
            return Ok(None);
        }
    }
    if info.get("resolvedRoot").is_some() {
        return Ok(None);
    }
    let (Some(names), Some(entries), Some(roots)) = (
        info["fileNames"].as_array(),
        info["fileInfos"].as_array(),
        info["root"].as_array(),
    ) else {
        return Ok(None);
    };
    if names.len() != entries.len() || names.len() > inputs.files.len() + 256 {
        return Ok(None);
    }
    let valid_id = |value: &Value| {
        value
            .as_u64()
            .is_some_and(|id| id > 0 && id as usize <= names.len())
    };
    let lists = info["fileIdsList"].as_array();
    if let Some(value) = info.get("fileIdsList") {
        if !value.as_array().is_some_and(|lists| {
            lists
                .iter()
                .all(|list| list.as_array().is_some_and(|ids| ids.iter().all(&valid_id)))
        }) {
            return Ok(None);
        }
    }
    if let Some(value) = info.get("referencedMap") {
        if !value.as_array().is_some_and(|edges| {
            edges.iter().all(|edge| {
                edge.as_array().is_some_and(|pair| {
                    pair.len() == 2
                        && valid_id(&pair[0])
                        && pair[1].as_u64().is_some_and(|id| {
                            id > 0 && lists.is_some_and(|lists| id as usize <= lists.len())
                        })
                })
            })
        }) {
            return Ok(None);
        }
    }
    let directory = project.parent().unwrap();
    let state_directory = path.parent().unwrap();
    let selected = inputs
        .files
        .iter()
        .map(fs::canonicalize)
        .collect::<io::Result<BTreeSet<_>>>()?;
    let mut root_ids = BTreeSet::new();
    for root in roots {
        if let Some(id) = root.as_u64() {
            root_ids.insert(id as usize);
        } else if let Some(range) = root.as_array().filter(|v| v.len() == 2) {
            let (Some(start), Some(end)) = (range[0].as_u64(), range[1].as_u64()) else {
                return Ok(None);
            };
            if start == 0 || end < start || end as usize > names.len() {
                return Ok(None);
            }
            root_ids.extend(start as usize..=end as usize);
        } else {
            return Ok(None);
        }
    }
    if root_ids.iter().any(|id| *id == 0 || *id > names.len()) {
        return Ok(None);
    }
    let mut admitted = BTreeSet::new();
    let mut native_roots = BTreeSet::new();
    let mut libraries = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    let mut declarations = BTreeMap::new();
    let mut runtimes = BTreeMap::new();
    let mut source_bytes = 0usize;
    let mut declaration_bytes = 0usize;
    let source_limit = blueice_bluets::CompilerLimits::default().max_total_source_bytes;
    for (index, (name, entry)) in names.iter().zip(entries).enumerate() {
        let Some(name) = name.as_str() else {
            return Ok(None);
        };
        let version = entry
            .as_str()
            .or_else(|| entry["version"].as_str())
            .unwrap_or("");
        let source = clean_path(&state_directory.join(name));
        if !selected.contains(&source) {
            if let Some(library) = model::library(name, version) {
                libraries.insert(library);
                continue;
            }
            return Ok(None);
        }
        // The metadata can select only a config-admitted source. No candidate
        // outside this set is opened, canonicalized or granted by the state.
        if !admitted.insert(source.clone()) {
            return Ok(None);
        }
        if root_ids.contains(&(index + 1)) {
            native_roots.insert(source.clone());
        }
        let size = fs::metadata(&source)?.len();
        if size > source_limit as u64 {
            return Ok(None);
        }
        source_bytes = source_bytes.saturating_add(size as usize);
        if source_bytes > source_limit {
            return Ok(None);
        }
        let bytes = fs::read(&source)?;
        if model::hash(&bytes) != version {
            return Ok(None);
        }
        let Some(declaration) = model::declaration(project, inputs, &source) else {
            return Ok(None);
        };
        let Some(relative) = declaration
            .strip_prefix(directory)
            .ok()
            .and_then(Path::to_str)
        else {
            return Ok(None);
        };
        let Some(declaration) = safe_output(project, relative) else {
            return Ok(None);
        };
        let signature = entry
            .as_str()
            .or_else(|| entry["signature"].as_str())
            .or_else(|| entry.get("signature").is_none().then_some(version));
        let size = fs::metadata(&declaration)?.len();
        if size > source_limit as u64 {
            return Ok(None);
        }
        declaration_bytes = declaration_bytes.saturating_add(size as usize);
        if declaration_bytes > source_limit {
            return Ok(None);
        }
        if signature != Some(model::hash(&fs::read(&declaration)?).as_str()) {
            return Ok(None);
        }
        if !source.to_string_lossy().ends_with(".d.ts") {
            outputs.insert(declaration.clone());
            declarations.insert(source.clone(), declaration.clone());
            if inputs.options.get("emitDeclarationOnly") != Some(&Value::Bool(true)) {
                let runtime = declaration.with_extension("");
                let runtime = runtime.with_extension("js");
                let Some(relative) = runtime.strip_prefix(directory).ok().and_then(Path::to_str)
                else {
                    return Ok(None);
                };
                let Some(runtime) = safe_output(project, relative) else {
                    return Ok(None);
                };
                outputs.insert(runtime.clone());
                runtimes.insert(source, runtime);
            }
        }
    }
    if admitted != selected || native_roots != selected {
        return Ok(None);
    }
    if let Some(latest) = info.get("latestChangedDtsFile") {
        let Some(name) = latest.as_str() else {
            return Ok(None);
        };
        if !outputs.contains(&clean_path(&state_directory.join(name))) {
            return Ok(None);
        }
    }
    let owned = info.get("blueice").is_some_and(|marker| {
        marker["producer"] == STATE_PRODUCER
            && marker["formatVersion"] == 1
            && marker["inputFingerprint"] == fingerprint
    });
    if !owned && !libraries.contains("lib.es2022.d.ts") {
        return Ok(None);
    }
    if !owned {
        let modified = fs::metadata(&path)?.modified()?;
        for config in &inputs.config_files {
            if fs::metadata(config)?.modified()? > modified {
                return Ok(None);
            }
        }
    }
    let resolution_inputs = admitted
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let fingerprint = resolution_inputs::fingerprint(graph, fingerprint, &resolution_inputs)
        .map_err(io::Error::other)?;
    State::save(
        project,
        fingerprint,
        &outputs.into_iter().collect::<Vec<_>>(),
        &declarations,
        &runtimes,
        resolution_inputs,
        StateStatus {
            native_hash: Some(sha256_label(&bytes)),
            ..StateStatus::default()
        },
    )
    .map(Some)
}
pub(super) fn publish(
    graph: &Graph,
    project: &Path,
    fingerprint: &str,
    declarations: &BTreeMap<PathBuf, PathBuf>,
    observed: &[String],
    status: &StateStatus,
) -> io::Result<Option<String>> {
    let inputs = &graph.inputs[project];
    if !model::eligible(inputs) {
        return Ok(None);
    }
    let path = model::preflight(graph, project)?;
    let directory = path.parent().unwrap();
    let mut sources = inputs.files.iter().cloned().collect::<BTreeSet<_>>();
    sources.extend(
        observed
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.extension().and_then(|v| v.to_str()) == Some("ts") && p.is_file()),
    );
    let mut names = Vec::new();
    let mut entries = Vec::new();
    let mut roots = Vec::new();
    for source in sources {
        graph
            .reader
            .authorize_future(&source, "native state source", false)
            .map_err(io::Error::other)?;
        let bytes = fs::read(&source)?;
        if bytes
            .windows(b"sourceMappingURL".len())
            .any(|w| w == b"sourceMappingURL")
        {
            return Ok(None);
        }
        names.push(model::relative(directory, &source));
        if inputs.files.contains(&source) {
            roots.push(names.len());
        }
        let declaration = if source.to_string_lossy().ends_with(".d.ts") {
            Some(&source)
        } else {
            declarations.get(&source)
        };
        let signature = declaration
            .filter(|p| p.is_file())
            .map(fs::read)
            .transpose()?
            .map(|b| Value::String(model::hash(&b)))
            .unwrap_or(Value::Bool(false));
        entries.push(json!({"version":model::hash(&bytes),"signature":signature}));
    }
    let mut info = json!({"fileNames":names,"fileInfos":entries,"root":roots,"options":model::options(project,inputs),
        "version":"5.9.3","blueice":{"producer":STATE_PRODUCER,"formatVersion":1,"inputFingerprint":fingerprint}});
    if status.failed {
        info["errors"] = Value::Bool(true);
    }
    if status.pending_dependencies {
        info["checkPending"] = Value::Bool(true);
    }
    if let Some(declaration) = declarations.values().last() {
        info["latestChangedDtsFile"] = Value::String(model::relative(directory, declaration));
    }
    let bytes = serde_json::to_vec(&info).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(io::Error::other("native build state exceeds byte limit"));
    }
    fs::create_dir_all(directory)?;
    graph
        .reader
        .authorize_future(&path, "native build state", false)
        .map_err(io::Error::other)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage = directory.join(format!(
        ".bluetsc-native-state-{}-{nonce}.tmp",
        std::process::id()
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&stage, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(stage);
    }
    result?;
    Ok(Some(sha256_label(&bytes)))
}
