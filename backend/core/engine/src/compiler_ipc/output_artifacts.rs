// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Preflight physical compiler artifact paths before owner output staging.

use blueice_bluets::{BuildOutput, SourceMap};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

enum FileContent<'a> {
    Text(&'a str),
    JavaScript(&'a str, Option<String>),
    SourceMap(&'a SourceMap),
}

struct PlannedFile<'a> {
    relative: PathBuf,
    content: FileContent<'a>,
}

pub(super) struct PlannedOutput<'a> {
    files: Vec<PlannedFile<'a>>,
}

pub(super) fn plan<'a>(
    project_root: &str,
    output: &'a BuildOutput,
) -> io::Result<PlannedOutput<'a>> {
    let root = Path::new(project_root);
    if contains_dot_segment(project_root) || !root.is_absolute() || fs::canonicalize(root)? != root
    {
        return Err(invalid("compiler project root is not canonical"));
    }
    let mut files = Vec::new();
    let mut destinations = BTreeSet::new();
    for (module_id, artifact) in &output.artifacts {
        if artifact.module_id != *module_id {
            return Err(invalid("compiler artifact identity does not match its key"));
        }
        if artifact.strict_runtime.is_some() {
            return Err(invalid(
                "registered strict artifact helper publication is unavailable",
            ));
        }
        let relative = source_relative(root, module_id)?;
        if !relative.to_string_lossy().ends_with(".ts")
            || relative.to_string_lossy().ends_with(".d.ts")
        {
            return Err(invalid(
                "compiler JavaScript artifact is not a source module",
            ));
        }
        let javascript = relative.with_extension("js");
        let map_name = if artifact.source_map.is_some() {
            Some(format!(
                "{}.map",
                javascript
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| invalid("compiler artifact name is not UTF-8"))?
            ))
        } else {
            None
        };
        add_file(
            &mut files,
            &mut destinations,
            javascript.clone(),
            FileContent::JavaScript(&artifact.javascript, map_name),
        )?;
        if let Some(map) = &artifact.source_map {
            add_file(
                &mut files,
                &mut destinations,
                javascript.with_extension("js.map"),
                FileContent::SourceMap(map),
            )?;
        }
        if let Some(declaration) = &artifact.declaration {
            add_file(
                &mut files,
                &mut destinations,
                javascript.with_extension("d.ts"),
                FileContent::Text(declaration),
            )?;
        }
    }
    for (module_id, source) in &output.declaration_modules {
        let relative = source_relative(root, module_id)?;
        if !relative.to_string_lossy().ends_with(".d.ts") {
            return Err(invalid(
                "compiler declaration artifact has an invalid source",
            ));
        }
        add_file(
            &mut files,
            &mut destinations,
            relative,
            FileContent::Text(source),
        )?;
    }
    Ok(PlannedOutput { files })
}

impl PlannedOutput<'_> {
    pub(super) fn write_to(&self, stage: &Path) -> io::Result<()> {
        for planned in &self.files {
            let path = stage.join(&planned.relative);
            let parent = path
                .parent()
                .ok_or_else(|| invalid("compiler artifact has no parent"))?;
            fs::create_dir_all(parent)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            match &planned.content {
                FileContent::Text(text) => file.write_all(text.as_bytes())?,
                FileContent::JavaScript(text, Some(map_name)) => {
                    file.write_all(text.as_bytes())?;
                    file.write_all(format!("\n//# sourceMappingURL={map_name}\n").as_bytes())?;
                }
                FileContent::JavaScript(text, None) => file.write_all(text.as_bytes())?,
                FileContent::SourceMap(map) => file.write_all(map.to_json().as_bytes())?,
            }
        }
        Ok(())
    }
}

fn source_relative(root: &Path, module_id: &str) -> io::Result<PathBuf> {
    let path = Path::new(module_id);
    if contains_dot_segment(module_id) || !path.is_absolute() || fs::canonicalize(path)? != path {
        return Err(invalid("compiler source identity is not canonical"));
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| invalid("compiler artifact source escapes its project root"))?;
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(invalid("compiler artifact path escapes its project root"));
    }
    Ok(relative.to_path_buf())
}

fn contains_dot_segment(path: &str) -> bool {
    // Windows' verbatim paths preserve `.` and `..` instead of normalizing
    // them, so compare the original spelling before filesystem resolution.
    #[cfg(windows)]
    let mut segments = path.split(['/', '\\']);
    #[cfg(not(windows))]
    let mut segments = path.split('/');
    segments.any(|segment| segment == "." || segment == "..")
}

fn add_file<'a>(
    files: &mut Vec<PlannedFile<'a>>,
    destinations: &mut BTreeSet<PathBuf>,
    relative: PathBuf,
    content: FileContent<'a>,
) -> io::Result<()> {
    if destinations
        .iter()
        .any(|existing| relative.starts_with(existing) || existing.starts_with(&relative))
    {
        return Err(invalid("compiler artifact output paths collide"));
    }
    destinations.insert(relative.clone());
    files.push(PlannedFile { relative, content });
    Ok(())
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
