// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Resolution option shapes retain the declaring configuration's paths.

use super::*;

pub(super) fn validate(
    name: &str,
    value: &Value,
    directory: &Path,
    reader: &Reader,
) -> Result<Option<Value>, String> {
    let checked = match name {
        "baseUrl" => {
            let text = value
                .as_str()
                .ok_or("compiler option `baseUrl` requires a string")?;
            let path = clean_path(&directory.join(text));
            reader.authorize_future(&path, name, true)?;
            json!(path.to_string_lossy())
        }
        "rootDirs" | "typeRoots" | "types" => {
            let values = value
                .as_array()
                .ok_or_else(|| format!("compiler option `{name}` requires an array"))?;
            let mut result = Vec::new();
            for item in values {
                let text = item
                    .as_str()
                    .ok_or_else(|| format!("compiler option `{name}` requires strings"))?;
                if name == "types" {
                    result.push(json!(text));
                } else {
                    let path = clean_path(&directory.join(text));
                    reader.authorize_future(&path, name, true)?;
                    result.push(json!(path.to_string_lossy()));
                }
            }
            Value::Array(result)
        }
        "paths" => {
            let patterns = value
                .as_object()
                .ok_or("compiler option `paths` requires an object")?;
            for (pattern, targets) in patterns {
                if pattern.matches('*').count() > 1 {
                    return Err("a paths pattern may contain at most one wildcard".into());
                }
                let targets = targets.as_array().ok_or("paths targets require an array")?;
                for target in targets {
                    let target = target.as_str().ok_or("paths targets require strings")?;
                    if target.matches('*').count() > 1 {
                        return Err("a paths target may contain at most one wildcard".into());
                    }
                }
            }
            value.clone()
        }
        _ => return Ok(None),
    };
    Ok(Some(checked))
}

pub(super) fn show(values: &mut Map<String, Value>, directory: &Path) {
    if let Some(Value::String(path)) = values.get_mut("baseUrl") {
        *path = relative_text(directory, Path::new(path));
    }
    for name in ["rootDirs", "typeRoots"] {
        if let Some(Value::Array(paths)) = values.get_mut(name) {
            for path in paths {
                if let Value::String(text) = path {
                    *text = relative_text(directory, Path::new(text));
                }
            }
        }
    }
}
