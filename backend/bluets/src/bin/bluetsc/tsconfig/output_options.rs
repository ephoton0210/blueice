// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Command-line output overrides use the same validation and owner boundary.

use super::*;

pub(in super::super) fn apply_output_flags(
    invocation: &mut Invocation,
    flags: Map<String, Value>,
) -> Result<(), String> {
    let directory = env::current_dir().map_err(|error| error.to_string())?;
    let reader = Reader::new(invocation.root.clone(), Vec::new());
    let flags = options::validate(&Value::Object(flags), &directory, &reader)?;
    let options = &mut invocation.options;
    for (name, field) in [
        ("removeComments", &mut options.remove_comments),
        ("emitBOM", &mut options.emit_bom),
        ("inlineSources", &mut options.inline_sources),
        ("stripInternal", &mut options.strip_internal),
        ("downlevelIteration", &mut options.downlevel_iteration),
        ("sourceMap", &mut options.source_map),
        ("declaration", &mut options.declaration),
    ] {
        if let Some(value) = flags.get(name).and_then(Value::as_bool) {
            *field = value;
        }
    }
    if let Some(value) = flags.get("newLine").and_then(Value::as_str) {
        options.new_line = if value == "crlf" {
            blueice_bluets::NewLine::CrLf
        } else {
            blueice_bluets::NewLine::Lf
        };
    }
    for (name, field) in [
        ("sourceRoot", &mut options.source_root),
        ("mapRoot", &mut options.map_root),
    ] {
        if let Some(value) = flags.get(name).and_then(Value::as_str) {
            *field = Some(value.to_string());
        }
    }
    if let Some(value) = flags.get("outDir").and_then(Value::as_str) {
        invocation.out_dir = Some(PathBuf::from(value));
    }
    if let Some(project) = &mut invocation.project_config {
        project.options.extend(flags);
    }
    Ok(())
}

pub(super) fn validate_dependencies(options: &Map<String, Value>) -> Result<(), String> {
    let enabled = |name| options.get(name).and_then(Value::as_bool) == Some(true);
    let nonempty = |name| {
        options
            .get(name)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty())
    };
    if !enabled("sourceMap") && !enabled("inlineSourceMap") {
        for name in ["inlineSources", "sourceRoot"] {
            if enabled(name) || nonempty(name) {
                return Err(format!("compiler option `{name}` requires sourceMap"));
            }
        }
    }
    if nonempty("mapRoot") && !enabled("sourceMap") && !enabled("declarationMap") {
        return Err("compiler option `mapRoot` requires sourceMap or declarationMap".into());
    }
    Ok(())
}
