// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit state outside the finite Native interchange profile remains owned.
//! The configured file corroborates private state; it never supplies inputs.
use super::*;

pub(super) fn enabled(inputs: &Inputs) -> bool {
    inputs.options.get("incremental") == Some(&Value::Bool(true))
}
pub(super) fn has_public_state(inputs: &Inputs) -> bool {
    enabled(inputs) || interop_model::eligible(inputs)
}

pub(super) fn check_path(graph: &Graph, project: &Path, path: &Path) -> io::Result<()> {
    for (candidate, inputs) in &graph.inputs {
        if inputs
            .files
            .iter()
            .chain(&inputs.config_files)
            .any(|input| input == path)
            || path == state_path(candidate)
            || (candidate != project
                && has_public_state(inputs)
                && path == interop_model::path(candidate, inputs))
        {
            return Err(io::Error::other(
                "build state path collides with a project input or state",
            ));
        }
        for source in &inputs.files {
            for option in ["outDir", "declarationDir"] {
                let output =
                    if let Some(directory) = inputs.options.get(option).and_then(Value::as_str) {
                        let Some(relative) = inputs
                            .emit_root
                            .as_ref()
                            .and_then(|root| source.strip_prefix(root).ok())
                        else {
                            continue;
                        };
                        Path::new(directory).join(relative)
                    } else if option == "outDir" {
                        source.clone()
                    } else {
                        continue;
                    };
                // Reserve source artifacts before state can overwrite inputs,
                // another project's state, or a just-published output.
                for extension in [
                    "js",
                    "jsx",
                    "mjs",
                    "cjs",
                    "d.ts",
                    "d.mts",
                    "d.cts",
                    "js.map",
                    "jsx.map",
                    "mjs.map",
                    "cjs.map",
                    "d.ts.map",
                    "d.mts.map",
                    "d.cts.map",
                ] {
                    if path == output.with_extension(extension) {
                        return Err(io::Error::other(
                            "build state path collides with a project output",
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}
pub(super) fn publish(
    graph: &Graph,
    project: &Path,
    fingerprint: &str,
    status: &StateStatus,
) -> io::Result<Option<String>> {
    let inputs = &graph.inputs[project];
    if !enabled(inputs) {
        return Ok(None);
    }
    let path = interop_model::preflight(graph, project)?;
    let bytes = serde_json::to_vec(&serde_json::json!({
        "producer": STATE_PRODUCER,
        "version": STATE_VERSION,
        "inputFingerprint": fingerprint,
        "failed": status.failed,
        "pendingDependencies": status.pending_dependencies,
        "options": inputs.options,
    }))
    .map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(io::Error::other("explicit build state exceeds byte limit"));
    }
    let directory = path.parent().unwrap();
    fs::create_dir_all(directory)?;
    interop_model::preflight(graph, project)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage = directory.join(format!(
        ".bluetsc-explicit-state-{}-{nonce}.tmp",
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
