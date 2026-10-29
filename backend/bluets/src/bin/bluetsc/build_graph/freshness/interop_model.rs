// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Independently implemented pinned build-info layout and owned artifact names.
//! Native library hashes describe metadata; they never authorize library reads.
use super::*;

pub(super) fn hash(bytes: &[u8]) -> String {
    sha256_label(bytes)
        .strip_prefix("sha256:")
        .unwrap()
        .to_string()
}
pub(super) fn relative(from: &Path, to: &Path) -> String {
    let left = from.components().collect::<Vec<_>>();
    let right = to.components().collect::<Vec<_>>();
    let common = left.iter().zip(&right).take_while(|(a, b)| a == b).count();
    let mut text = std::iter::repeat_n("..".to_string(), left.len() - common)
        .chain(
            right[common..]
                .iter()
                .map(|c| c.as_os_str().to_string_lossy().into_owned()),
        )
        .collect::<Vec<_>>()
        .join("/");
    if !text.starts_with('.') {
        text = format!("./{text}");
    }
    text
}
pub(super) fn path(project: &Path, inputs: &Inputs) -> PathBuf {
    if let Some(path) = inputs
        .options
        .get("tsBuildInfoFile")
        .and_then(Value::as_str)
    {
        return PathBuf::from(path);
    }
    if let Some(directory) = inputs.options.get("outDir").and_then(Value::as_str) {
        let suffix = if inputs.options.contains_key("rootDir") {
            relative(
                inputs.emit_root.as_ref().unwrap(),
                &project.with_extension("tsbuildinfo"),
            )
        } else {
            project
                .with_extension("tsbuildinfo")
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };
        return clean_path(&Path::new(directory).join(suffix));
    }
    project.with_extension("tsbuildinfo")
}
pub(super) fn eligible(inputs: &Inputs) -> bool {
    let options = &inputs.options;
    options.get("composite") == Some(&Value::Bool(true))
        && options.get("declaration") == Some(&Value::Bool(true))
        && options.get("strict") == Some(&Value::Bool(true))
        && options.get("target").and_then(Value::as_str) == Some("es2022")
        && matches!(
            options.get("module").and_then(Value::as_str),
            Some("commonjs" | "esnext")
        )
        && options.get("lib") == Some(&serde_json::json!(["es2022"]))
        && options.get("types") == Some(&serde_json::json!([]))
        && options
            .get("moduleResolution")
            .is_none_or(|v| v == "node10")
        && options.keys().all(|name| {
            matches!(
                name.as_str(),
                "composite"
                    | "declaration"
                    | "strict"
                    | "target"
                    | "module"
                    | "lib"
                    | "types"
                    | "rootDir"
                    | "outDir"
                    | "noEmitOnError"
                    | "emitDeclarationOnly"
                    | "moduleResolution"
                    | "tsBuildInfoFile"
            )
        })
        && inputs
            .files
            .iter()
            .all(|p| p.extension().and_then(|s| s.to_str()) == Some("ts"))
}
pub(super) fn options(project: &Path, inputs: &Inputs) -> Value {
    let path = path(project, inputs);
    let directory = path.parent().unwrap();
    let mut result = serde_json::Map::new();
    for name in [
        "composite",
        "declaration",
        "strict",
        "noEmitOnError",
        "emitDeclarationOnly",
    ] {
        if let Some(value) = inputs.options.get(name) {
            result.insert(name.into(), value.clone());
        }
    }
    result.insert("target".into(), serde_json::json!(9));
    result.insert(
        "module".into(),
        serde_json::json!(if inputs.options["module"] == "commonjs" {
            1
        } else {
            99
        }),
    );
    for name in ["rootDir", "outDir", "tsBuildInfoFile"] {
        if let Some(value) = inputs.options.get(name).and_then(Value::as_str) {
            result.insert(
                name.into(),
                Value::String(relative(directory, Path::new(value))),
            );
        }
    }
    Value::Object(result)
}
pub(super) fn declaration(project: &Path, inputs: &Inputs, source: &Path) -> Option<PathBuf> {
    if source.to_string_lossy().ends_with(".d.ts") {
        return Some(source.to_path_buf());
    }
    let output = if let Some(directory) = inputs.options.get("outDir").and_then(Value::as_str) {
        Path::new(directory).join(source.strip_prefix(inputs.emit_root.as_ref()?).ok()?)
    } else {
        source.to_path_buf()
    };
    let output = output.with_extension("d.ts");
    output.strip_prefix(project.parent()?).ok()?;
    Some(output)
}
pub(super) fn library(name: &str, version: &str) -> Option<String> {
    static TABLE: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let table = TABLE
        .get_or_init(|| serde_json::from_str(include_str!("native_library_hashes.json")).unwrap());
    let basename = name
        .strip_prefix("@typescript/lib/")
        .or_else(|| name.rsplit_once("/typescript/lib/").map(|(_, name)| name))?;
    if basename.contains('/') || basename.contains('\\') {
        return None;
    }
    (table["files"][basename].as_str() == Some(version)).then(|| basename.to_string())
}
pub(super) fn preflight(graph: &Graph, project: &Path) -> io::Result<PathBuf> {
    let path = path(project, &graph.inputs[project]);
    graph
        .reader
        .authorize_future(&path, "native build state", false)
        .map_err(io::Error::other)?;
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err(io::Error::other(
                "native build state path is not a regular file",
            ))
        }
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    Ok(path)
}
