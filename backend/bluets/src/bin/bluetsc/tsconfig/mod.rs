// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! TypeScript configuration is read by the filesystem-owning CLI only.

use super::*;
use inheritance::{Document, Reader};
use serde_json::{json, Map, Value};

pub(super) mod diagnostics;
mod discovery;
mod inheritance;
mod jsonc;
mod options;
mod output_layout;

#[derive(Debug)]
pub(super) struct ProjectConfig {
    pub(super) options: Map<String, Value>,
    pub(super) files: Vec<PathBuf>,
    pub(super) directory: PathBuf,
    pub(super) emit_root: PathBuf,
    pub(super) fingerprint: String,
    pub(super) selectors: Map<String, Value>,
    pub(super) config_inputs: Vec<PathBuf>,
}

impl ProjectConfig {
    pub(super) fn show(&self) -> Value {
        let files = self
            .files
            .iter()
            .map(|file| format!("./{}", relative_text(&self.directory, file)))
            .collect::<Vec<_>>();
        let mut shown = json!({"compilerOptions": options::effective(&self.options, &self.directory), "files": files});
        shown
            .as_object_mut()
            .expect("configuration object")
            .extend(self.selectors.clone());
        shown
    }
}

pub(super) fn resolve(
    path: &Path,
    owner_path: Option<&Path>,
    report: &mut Option<blueice_bluets::Diagnostic>,
    sources: &mut BTreeMap<String, String>,
) -> Result<Invocation, String> {
    let directory = path
        .parent()
        .ok_or_else(|| "tsconfig has no parent".to_string())?
        .to_path_buf();
    let owner_directory = owner_path.and_then(Path::parent).unwrap_or(&directory);
    let owner_text = owner_path
        .map(|path| {
            let canonical = fs::canonicalize(path)
                .map_err(|error| format!("cannot resolve owner config: {error}"))?;
            ensure_within(&canonical, owner_directory, "owner config")?;
            fs::read_to_string(canonical)
                .map_err(|error| format!("cannot read owner config: {error}"))
        })
        .transpose()?;
    let owner_value = owner_text
        .as_deref()
        .map(jsonc::parse)
        .transpose()?
        .unwrap_or(json!({}));
    let owner = owner_value
        .as_object()
        .ok_or_else(|| "owner config must be an object".to_string())?;
    let root = if let Some(project_root) = owner.get("projectRoot").and_then(Value::as_str) {
        if Path::new(project_root).is_absolute()
            || Path::new(project_root)
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err("owner projectRoot must stay beneath the config directory".to_string());
        }
        let selected = clean_path(&owner_directory.join(project_root));
        let canonical = fs::canonicalize(&selected)
            .map_err(|error| format!("cannot access owner projectRoot: {error}"))?;
        ensure_within(&canonical, owner_directory, "owner projectRoot")?;
        canonical
    } else {
        if owner
            .get("projectRoot")
            .is_some_and(|value| !value.is_null())
        {
            return Err("owner projectRoot requires a string".to_string());
        }
        owner_directory.to_path_buf()
    };
    let mut extra_roots = Vec::new();
    if let Some(packages) = owner.get("packageRoots").and_then(Value::as_array) {
        for package in packages {
            let package = package
                .as_str()
                .ok_or_else(|| "packageRoots requires strings".to_string())?;
            let canonical = fs::canonicalize(owner_directory.join(package))
                .map_err(|error| format!("cannot read owner packageRoot: {error}"))?;
            extra_roots.push(canonical);
        }
    }
    let mut reader = Reader::new(root.clone(), extra_roots);
    let mut document = match reader.load(path) {
        Ok(document) => document,
        Err(error) => {
            *report = reader.diagnostic.take();
            for (path, bytes) in &reader.inputs {
                if let (Some(name), Ok(source)) = (
                    path.file_name().and_then(|name| name.to_str()),
                    std::str::from_utf8(bytes),
                ) {
                    sources.insert(name.into(), source.into());
                }
            }
            return Err(error);
        }
    };
    // Owner fields replace only explicitly supplied compiler settings.
    let mut owner_compiler = Map::new();
    for (key, value) in owner {
        if matches!(
            key.as_str(),
            "target"
                | "module"
                | "sourceMap"
                | "declaration"
                | "outDir"
                | "useDefineForClassFields"
                | "preserveConstEnums"
                | "isolatedModules"
                | "esModuleInterop"
                | "experimentalDecorators"
                | "emitDecoratorMetadata"
                | "jsx"
                | "jsxFactory"
                | "jsxFragmentFactory"
                | "jsxImportSource"
                | "moduleResolution"
        ) || options::STRICT.contains(&key.as_str())
            || matches!(
                key.as_str(),
                "strict"
                    | "noUnusedLocals"
                    | "noUnusedParameters"
                    | "noImplicitReturns"
                    | "noFallthroughCasesInSwitch"
                    | "exactOptionalPropertyTypes"
                    | "noUncheckedIndexedAccess"
            )
        {
            owner_compiler.insert(key.clone(), value.clone());
        }
    }
    document.options.extend(options::validate(
        &Value::Object(owner_compiler.clone()),
        &root,
        &reader,
    )?);
    if let Some(entries) = owner.get("entries") {
        let entries = entries
            .as_array()
            .ok_or_else(|| "owner entries requires an array".to_string())?;
        if entries.is_empty() {
            return Err("owner entries must not be empty".to_string());
        }
        document.files = Some(
            entries
                .iter()
                .map(|entry| {
                    entry
                        .as_str()
                        .map(|entry| clean_path(&root.join(entry)))
                        .ok_or_else(|| "owner entries requires strings".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
        document.files_supplied = true;
        document.local_empty_files = false;
        document.include = Some(Vec::new());
    }
    let files = match discovery::select(&document, &reader) {
        Ok(files) => files,
        Err(error) => {
            *report = diagnostics::error(&error, path, reader.inputs.get(path).map(Vec::as_slice));
            return Err(error);
        }
    };
    let emit_root = match document.options.get("rootDir").and_then(Value::as_str) {
        Some(root) => PathBuf::from(root),
        None => common_source_root(&files, &root),
    };
    reader.authorize_future(&emit_root, "rootDir", false)?;
    let mut fingerprint = Vec::new();
    for (path, bytes) in &reader.inputs {
        let identity = if let Ok(path) = path.strip_prefix(&root) {
            output_path(path)
        } else {
            let (index, relative) = reader
                .extra_roots
                .iter()
                .enumerate()
                .find_map(|(index, root)| path.strip_prefix(root).ok().map(|path| (index, path)))
                .ok_or_else(|| "config input has no authorized identity".to_string())?;
            format!("@config/{index}/{}", output_path(relative))
        };
        fingerprint.extend(identity.bytes());
        fingerprint.push(0);
        fingerprint.extend(bytes);
        fingerprint.push(0);
    }
    if let Some(owner_text) = &owner_text {
        fingerprint.extend(owner_text.bytes());
        fingerprint.push(0);
    }
    for file in &files {
        fingerprint.extend(relative_text(&root, file).bytes());
        fingerprint.push(0);
    }
    let mut selectors = Map::new();
    for (name, paths) in [
        ("include", document.include.as_ref()),
        ("exclude", document.exclude.as_ref()),
    ] {
        if let Some(paths) = paths {
            selectors.insert(
                name.to_string(),
                json!(paths
                    .iter()
                    .map(|path| relative_text(&directory, path))
                    .collect::<Vec<_>>()),
            );
        }
    }
    if document.exclude.is_none() {
        if let Some(out) = document.options.get("outDir").and_then(Value::as_str) {
            selectors.insert("exclude".to_string(), json!([out]));
        }
    }
    let mut config_inputs = reader.inputs.keys().cloned().collect::<Vec<_>>();
    if let Some(path) = owner_path {
        config_inputs.push(path.to_path_buf());
    }
    let project = ProjectConfig {
        selectors,
        config_inputs,
        options: document.options.clone(),
        files: files.clone(),
        directory: directory.clone(),
        emit_root,
        fingerprint: format!("tsconfig-v1:{}", sha256_label(&fingerprint)),
    };
    let mut merged = options::owner_options(&document.options, &root)?;
    for (name, value) in owner {
        if !matches!(name.as_str(), "tsconfig" | "projectRoot" | "entries")
            && !owner_compiler.contains_key(name)
        {
            merged.insert(name.clone(), value.clone());
        }
    }
    let entries = files
        .iter()
        .map(|file| json!(relative_text(&root, file)))
        .collect::<Vec<_>>();
    merged.insert("entries".to_string(), json!(entries));
    // No source is opened by a configuration-only query. A missing explicit
    // file remains in --showConfig, just as it does in pinned TypeScript.
    let config: BlueTscConfig = serde_json::from_value(Value::Object(merged))
        .map_err(|error| format!("invalid owner config: {error}"))?;
    let mut invocation = config::resolve_config_document(owner_directory, root, config, false)?;
    invocation
        .options
        .resolver_fingerprint
        .push_str(&format!("+{}", project.fingerprint));
    invocation.project_config = Some(project);
    Ok(invocation)
}

pub(super) fn prepare(invocation: &mut Invocation) -> Result<(), String> {
    let Some(project) = &invocation.project_config else {
        return Ok(());
    };
    invocation.options.checking = Some(options::checking(&project.options)?);
    if project
        .options
        .get("target")
        .and_then(Value::as_str)
        .is_none()
    {
        return Err("unsupported tsconfig target `es5`; select es2020 or es2022".to_string());
    }
    for entry in &mut invocation.entries {
        let canonical = absolute_existing_path(entry).map_err(|error| {
            format!("cannot read configured entry {}: {error}", entry.display())
        })?;
        ensure_within(&canonical, &invocation.root, "configured entry")?;
        if project
            .options
            .get("rootDir")
            .and_then(Value::as_str)
            .is_some()
            && !is_declaration_path(&canonical)
        {
            ensure_within(&canonical, &project.emit_root, "source outside rootDir")?;
        }
        *entry = canonical;
    }
    for file in &project.files {
        if is_declaration_path(file) {
            let canonical = absolute_existing_path(file)
                .map_err(|error| format!("cannot read configured declaration: {error}"))?;
            ensure_within(&canonical, &invocation.root, "configured declaration")?;
            let source = fs::read_to_string(&canonical)
                .map_err(|error| format!("cannot read configured declaration: {error}"))?;
            let identity = project_module_id(&invocation.root, &canonical);
            invocation.options.resolver_fingerprint.push_str(&format!(
                "+declaration:{identity}:{}",
                sha256_label(source.as_bytes())
            ));
            if global_declaration(&identity, &source)? {
                invocation
                    .options
                    .ambient_declaration_modules
                    .push(ModuleSource::new(identity, source));
            }
        }
    }
    Ok(())
}

fn global_declaration(module: &str, source: &str) -> Result<bool, String> {
    let tokens = blueice_bluets::lex(module, source).map_err(|diagnostics| {
        diagnostics
            .first()
            .map(|diagnostic| diagnostic.message.clone())
            .unwrap_or_else(|| "invalid configured declaration".to_string())
    })?;
    let mut depth: usize = 0;
    for token in tokens {
        if depth == 0 && matches!(token.text.as_str(), "import" | "export") {
            return Ok(false);
        }
        match token.text.as_str() {
            "{" | "(" | "[" => depth += 1,
            "}" | ")" | "]" => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(true)
}

fn common_source_root(files: &[PathBuf], fallback: &Path) -> PathBuf {
    let mut sources = files.iter().filter(|file| !is_declaration_path(file));
    let Some(first) = sources.next() else {
        return fallback.to_path_buf();
    };
    let mut common = first.parent().unwrap_or(fallback).to_path_buf();
    for file in sources {
        while !file.starts_with(&common) {
            if !common.pop() {
                return fallback.to_path_buf();
            }
        }
    }
    common
}

pub(super) fn clean_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

pub(super) fn wild_component(component: std::path::Component<'_>) -> bool {
    matches!(component, std::path::Component::Normal(value) if value.to_string_lossy().contains(['*', '?']))
}

pub(super) fn relative_text(base: &Path, path: &Path) -> String {
    let left = base.components().collect::<Vec<_>>();
    let right = path.components().collect::<Vec<_>>();
    let common = left
        .iter()
        .zip(&right)
        .take_while(|(left, right)| left == right)
        .count();
    let mut result = vec!["..".to_string(); left.len() - common];
    result.extend(
        right[common..]
            .iter()
            .map(|component| component.as_os_str().to_string_lossy().to_string()),
    );
    if result.is_empty() {
        ".".to_string()
    } else {
        result.join("/")
    }
}

pub(super) fn parse_jsonc(text: &str) -> Result<Value, String> {
    jsonc::parse(text)
}

pub(super) fn prepare_output(
    invocation: &mut Invocation,
    summary: &mut CompileSummary,
) -> Result<(), String> {
    output_layout::prepare(invocation, summary)
}

pub(super) fn emitted_path(module: &str, metadata: &BuildMetadata) -> io::Result<PathBuf> {
    output_layout::emitted_path(module, metadata)
}
