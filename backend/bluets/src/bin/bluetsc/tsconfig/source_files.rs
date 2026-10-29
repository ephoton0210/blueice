// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bare-source invocations use the current directory as their canonical owner.
//! They never discover or read a nearby tsconfig or owner overlay.
use super::*;

pub(in super::super) fn resolve(
    files: Vec<PathBuf>,
    flags: Map<String, Value>,
) -> Result<Invocation, String> {
    let root = env::current_dir()
        .map_err(|e| e.to_string())?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let reader = Reader::new(root.clone(), Vec::new());
    let raw = options::validate(&Value::Object(flags), &root, &reader)?;
    let mut selected = Vec::new();
    for file in files {
        let path = clean_path(&root.join(file));
        reader.authorize_future(&path, "source file", false)?;
        let path = if path.exists() {
            let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
            ensure_within(&path, &root, "source file")?;
            path
        } else {
            path
        };
        if !selected.contains(&path) {
            selected.push(path);
        }
    }
    let emit_root = raw
        .get("rootDir")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| common_source_root(&selected, &root));
    reader.authorize_future(&emit_root, "rootDir", false)?;
    let mut merged = options::owner_options(&raw, &root)?;
    if !merged.contains_key("module") {
        merged.insert("module".into(), json!("commonjs"));
    }
    merged.insert(
        "entries".into(),
        json!(selected
            .iter()
            .map(|p| relative_text(&root, p))
            .collect::<Vec<_>>()),
    );
    let config: BlueTscConfig =
        serde_json::from_value(Value::Object(merged)).map_err(|e| e.to_string())?;
    let mut invocation = config::resolve_config_document(&root, root.clone(), config, false)?;
    let fingerprint = sha256_label(&serde_json::to_vec(&raw).expect("native options serialize"));
    invocation
        .options
        .resolver_fingerprint
        .push_str(&format!("+source-cli-v1:{fingerprint}"));
    invocation.project_config = Some(ProjectConfig {
        options: raw,
        mappings: None,
        files: selected.clone(),
        file_order: selected.clone(),
        explicit_files: selected.into_iter().collect(),
        type_libraries: BTreeMap::new(),
        config_path: root.join("tsconfig.json"),
        directory: root,
        emit_root,
        fingerprint,
        selectors: Map::new(),
        config_inputs: Vec::new(),
    });
    Ok(invocation)
}
