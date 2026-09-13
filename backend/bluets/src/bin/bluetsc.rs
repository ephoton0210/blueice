// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The standalone BlueTSC command-line front end.

use blueice_bluets::{
    compile, BuildArtifact, CompilerOptions, EcmaTarget, ModuleLoader, ModuleSource, RuntimePolicy,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Check,
    Build,
}

#[derive(Debug, PartialEq, Eq)]
struct Args {
    command: Command,
    input: Input,
}

#[derive(Debug, PartialEq, Eq)]
enum Input {
    Entry {
        entry: PathBuf,
        project_root: Option<PathBuf>,
        out_dir: Option<PathBuf>,
        options: CompilerOptions,
    },
    Config(PathBuf),
}

#[derive(Debug)]
struct Invocation {
    root: PathBuf,
    entries: Vec<PathBuf>,
    out_dir: Option<PathBuf>,
    options: CompilerOptions,
    imports: BTreeMap<String, PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BlueTscConfig {
    entries: Vec<String>,
    #[serde(default)]
    project_root: Option<String>,
    #[serde(default)]
    out_dir: Option<String>,
    #[serde(default)]
    source_map: bool,
    #[serde(default)]
    declaration: bool,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    runtime_policy: Option<String>,
    #[serde(default)]
    imports: BTreeMap<String, String>,
}

fn main() -> ExitCode {
    let args = match parse_args(env::args().skip(1)) {
        Ok(args) => args,
        Err(message) if message == "help requested" => {
            println!("{}", usage());
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("bluetsc: {message}\n\n{}", usage());
            return ExitCode::FAILURE;
        }
    };
    let invocation = match resolve_invocation(args.input) {
        Ok(invocation) => invocation,
        Err(message) => {
            eprintln!("bluetsc: {message}");
            return ExitCode::FAILURE;
        }
    };
    if args.command == Command::Build && invocation.out_dir.is_none() {
        eprintln!("bluetsc: build requires --out-dir <directory> or config outDir");
        return ExitCode::FAILURE;
    }
    let loader = FileLoader {
        root: invocation.root.clone(),
        imports: invocation.imports.clone(),
    };
    let summary = compile_entries(
        &invocation.root,
        &invocation.entries,
        &loader,
        invocation.options.clone(),
    );
    if summary.has_errors {
        return ExitCode::FAILURE;
    }
    if args.command == Command::Check {
        println!(
            "checked {} entry point(s), {} module(s), fingerprint {}",
            invocation.entries.len(),
            summary.module_count,
            summary.fingerprint,
        );
        return ExitCode::SUCCESS;
    }
    let metadata = build_metadata(&invocation, &summary);
    let out_dir = invocation
        .out_dir
        .as_deref()
        .expect("build argument parsing requires an output directory");
    match publish_build(&invocation.root, out_dir, &summary.artifacts, &metadata) {
        Ok(()) => {
            println!(
                "built {} entry point(s), {} module(s), fingerprint {}",
                invocation.entries.len(),
                summary.artifacts.len(),
                summary.fingerprint,
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bluetsc: failed to publish build: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut args = args;
    let command = match args.next().as_deref() {
        Some("check") => Command::Check,
        Some("build") => Command::Build,
        Some("--help") | Some("-h") => return Err("help requested".to_string()),
        Some(other) => {
            return Err(format!(
                "unknown command `{other}`; expected `check` or `build`"
            ))
        }
        None => return Err("a command is required".to_string()),
    };
    let first = args
        .next()
        .ok_or_else(|| "an entry .ts file or --config <file> is required".to_string())?;
    if first == "--config" {
        let path = args
            .next()
            .ok_or_else(|| "--config requires a JSON file".to_string())?;
        if let Some(extra) = args.next() {
            return Err(format!(
                "`--config` owns project settings; unsupported extra argument `{extra}`"
            ));
        }
        return Ok(Args {
            command,
            input: Input::Config(PathBuf::from(path)),
        });
    }
    let entry = PathBuf::from(first);
    let mut project_root = None;
    let mut out_dir = None;
    let mut options = CompilerOptions::default();
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match flag.as_str() {
            "--project-root" => project_root = Some(PathBuf::from(value()?)),
            "--out-dir" => out_dir = Some(PathBuf::from(value()?)),
            "--source-map" => options.source_map = true,
            "--declaration" => options.declaration = true,
            "--target" => {
                options.target = match value()?.as_str() {
                    "es2020" => EcmaTarget::Es2020,
                    "es2022" => EcmaTarget::Es2022,
                    other => return Err(format!("unsupported target `{other}`; expected es2020 or es2022")),
                }
            }
            "--runtime-policy" => {
                options.runtime_policy = match value()?.as_str() {
                    "transpile-only" => RuntimePolicy::TranspileOnly,
                    "checked" => RuntimePolicy::Checked,
                    "strict-runtime" => RuntimePolicy::StrictRuntime,
                    other => {
                        return Err(format!(
                            "unsupported runtime policy `{other}`; expected transpile-only, checked, or strict-runtime"
                        ))
                    }
                }
            }
            "--help" | "-h" => return Err("help requested".to_string()),
            other => return Err(format!("unrecognized argument `{other}`")),
        }
    }
    if command == Command::Build && out_dir.is_none() {
        return Err("build requires --out-dir <directory>".to_string());
    }
    if command == Command::Check && out_dir.is_some() {
        return Err("--out-dir is valid only with build".to_string());
    }
    Ok(Args {
        command,
        input: Input::Entry {
            entry,
            project_root,
            out_dir,
            options,
        },
    })
}

fn usage() -> &'static str {
    "Usage:\n  bluetsc check <entry.ts> [--project-root <directory>] [--target es2020|es2022] [--runtime-policy transpile-only|checked|strict-runtime]\n  bluetsc build <entry.ts> --out-dir <directory> [--project-root <directory>] [--source-map] [--declaration] [--target es2020|es2022] [--runtime-policy transpile-only|checked|strict-runtime]\n  bluetsc check --config <bluetsc.json>\n  bluetsc build --config <bluetsc.json>\n\nConfig fields: entries, projectRoot, outDir, sourceMap, declaration, target, runtimePolicy, imports."
}

#[derive(Debug)]
struct CompileSummary {
    artifacts: BTreeMap<String, BuildArtifact>,
    fingerprint: String,
    module_count: usize,
    has_errors: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildMetadata {
    language_version: &'static str,
    fingerprint: String,
    target: &'static str,
    runtime_policy: &'static str,
    source_map: bool,
    declaration: bool,
    entries: Vec<String>,
    imports: BTreeMap<String, String>,
}

fn compile_entries(
    root: &Path,
    entries: &[PathBuf],
    loader: &FileLoader,
    options: CompilerOptions,
) -> CompileSummary {
    let mut artifacts = BTreeMap::new();
    let mut modules = BTreeSet::new();
    let mut fingerprints = Vec::new();
    let mut has_errors = false;
    for entry in entries {
        let result = compile(&project_module_id(root, entry), loader, options.clone());
        for diagnostic in &result.diagnostics {
            eprintln!(
                "{}:{}:{}: {}: {}",
                diagnostic.span.module,
                diagnostic.span.start,
                diagnostic.span.end,
                diagnostic.code,
                diagnostic.message
            );
        }
        has_errors |= result.has_errors();
        modules.extend(result.project.modules.keys().cloned());
        if let Some(output) = result.output {
            fingerprints.push(output.fingerprint);
            for (module_id, artifact) in output.artifacts {
                artifacts.entry(module_id).or_insert(artifact);
            }
        }
    }
    fingerprints.sort();
    CompileSummary {
        artifacts,
        fingerprint: fingerprint_entries(&fingerprints),
        module_count: modules.len(),
        has_errors,
    }
}

fn build_metadata(invocation: &Invocation, summary: &CompileSummary) -> BuildMetadata {
    let entries = invocation
        .entries
        .iter()
        .map(|entry| output_module_path(&invocation.root, entry))
        .collect();
    let imports = invocation
        .imports
        .iter()
        .map(|(specifier, target)| {
            let relative = target
                .strip_prefix(&invocation.root)
                .expect("validated import-map target is beneath project root");
            let emitted = if target.is_dir() {
                format!("{}/", output_path(relative))
            } else {
                output_path(&relative.with_extension("js"))
            };
            (specifier.clone(), format!("./{emitted}"))
        })
        .collect();
    BuildMetadata {
        language_version: blueice_bluets::LANGUAGE_VERSION,
        fingerprint: summary.fingerprint.clone(),
        target: invocation.options.target.as_str(),
        runtime_policy: invocation.options.runtime_policy.as_str(),
        source_map: invocation.options.source_map,
        declaration: invocation.options.declaration,
        entries,
        imports,
    }
}

fn output_module_path(root: &Path, module: &Path) -> String {
    let relative = module
        .strip_prefix(root)
        .expect("validated entry is beneath project root")
        .with_extension("js");
    output_path(&relative)
}

fn output_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn resolve_invocation(input: Input) -> Result<Invocation, String> {
    match input {
        Input::Entry {
            entry,
            project_root,
            out_dir,
            options,
        } => resolve_explicit_invocation(entry, project_root, out_dir, options),
        Input::Config(path) => resolve_config_invocation(path),
    }
}

fn resolve_explicit_invocation(
    entry: PathBuf,
    project_root: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    options: CompilerOptions,
) -> Result<Invocation, String> {
    let entry = absolute_existing_path(&entry)
        .map_err(|error| format!("cannot read entry {}: {error}", entry.display()))?;
    let root = match project_root {
        Some(root) => absolute_existing_path(&root)
            .map_err(|error| format!("cannot access --project-root {}: {error}", root.display()))?,
        None => entry
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
    };
    if !root.is_dir() {
        return Err(format!(
            "project root {} is not a directory",
            root.display()
        ));
    }
    ensure_within(&entry, &root, "entry")?;
    Ok(Invocation {
        root,
        entries: vec![entry],
        out_dir,
        options,
        imports: BTreeMap::new(),
    })
}

fn resolve_config_invocation(path: PathBuf) -> Result<Invocation, String> {
    let config_path = absolute_existing_path(&path)
        .map_err(|error| format!("cannot read config {}: {error}", path.display()))?;
    let config_text = fs::read_to_string(&config_path)
        .map_err(|error| format!("cannot read config {}: {error}", config_path.display()))?;
    let config: BlueTscConfig = serde_json::from_str(&config_text)
        .map_err(|error| format!("invalid config {}: {error}", config_path.display()))?;
    if config.entries.is_empty() {
        return Err("config `entries` must contain at least one .ts entry".to_string());
    }
    let config_directory = config_path
        .parent()
        .ok_or_else(|| "config has no parent directory".to_string())?;
    let root = match config.project_root {
        Some(root) => {
            let root_path = Path::new(&root);
            if root_path.is_absolute()
                || root_path
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir))
            {
                return Err(format!(
                    "config projectRoot `{root}` must stay beneath the config directory"
                ));
            }
            absolute_existing_path(&config_directory.join(root_path)).map_err(|error| {
                format!(
                    "cannot access config projectRoot under {}: {error}",
                    config_directory.display()
                )
            })?
        }
        None => config_directory.to_path_buf(),
    };
    if !root.is_dir() {
        return Err(format!(
            "config projectRoot {} is not a directory",
            root.display()
        ));
    }
    let mut entries = Vec::new();
    for entry in config.entries {
        let entry = absolute_existing_path(&root.join(entry))
            .map_err(|error| format!("cannot read configured entry: {error}"))?;
        ensure_within(&entry, &root, "configured entry")?;
        entries.push(entry);
    }
    entries.sort();
    entries.dedup();
    let out_dir = config
        .out_dir
        .map(|path| configured_output_path(&root, &path))
        .transpose()?;
    let mut imports = BTreeMap::new();
    for (specifier, target) in config.imports {
        if specifier.is_empty() || specifier.starts_with('.') || specifier.starts_with('/') {
            return Err(format!("invalid import-map key `{specifier}`"));
        }
        let target = absolute_existing_path(&root.join(target)).map_err(|error| {
            format!("cannot resolve import-map target for `{specifier}`: {error}")
        })?;
        ensure_within(&target, &root, "import-map target")?;
        if specifier.ends_with('/') && !target.is_dir() {
            return Err(format!(
                "import-map prefix `{specifier}` must target a project-root-confined directory"
            ));
        }
        if !specifier.ends_with('/') && !target.is_file() {
            return Err(format!(
                "import-map key `{specifier}` must target a project-root-confined source file"
            ));
        }
        imports.insert(specifier, target);
    }
    let options = CompilerOptions {
        target: parse_target(config.target.as_deref())?,
        runtime_policy: parse_runtime_policy(config.runtime_policy.as_deref())?,
        source_map: config.source_map,
        declaration: config.declaration,
        resolver_fingerprint: import_map_fingerprint(&root, &imports),
    };
    Ok(Invocation {
        root,
        entries,
        out_dir,
        options,
        imports,
    })
}

fn parse_target(value: Option<&str>) -> Result<EcmaTarget, String> {
    match value.unwrap_or("es2022") {
        "es2020" => Ok(EcmaTarget::Es2020),
        "es2022" => Ok(EcmaTarget::Es2022),
        other => Err(format!(
            "unsupported target `{other}`; expected es2020 or es2022"
        )),
    }
}

fn parse_runtime_policy(value: Option<&str>) -> Result<RuntimePolicy, String> {
    match value.unwrap_or("checked") {
        "transpile-only" => Ok(RuntimePolicy::TranspileOnly),
        "checked" => Ok(RuntimePolicy::Checked),
        "strict-runtime" => Ok(RuntimePolicy::StrictRuntime),
        other => Err(format!(
            "unsupported runtime policy `{other}`; expected transpile-only, checked, or strict-runtime"
        )),
    }
}

fn configured_output_path(root: &Path, value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return Err(format!(
            "config outDir `{value}` must stay beneath projectRoot"
        ));
    }
    Ok(root.join(path))
}

fn ensure_within(path: &Path, root: &Path, label: &str) -> Result<(), String> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(format!(
            "{label} {} is outside project root {}",
            path.display(),
            root.display()
        ))
    }
}

fn fingerprint_entries(fingerprints: &[String]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for fingerprint in fingerprints {
        for byte in fingerprint.bytes().chain(std::iter::once(0xff)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("bts-project-{hash:016x}")
}

fn import_map_fingerprint(root: &Path, imports: &BTreeMap<String, PathBuf>) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for (specifier, target) in imports {
        let target = target
            .strip_prefix(root)
            .expect("validated import-map target is beneath project root");
        let target = output_path(target);
        for byte in specifier
            .bytes()
            .chain(std::iter::once(0))
            .chain(target.bytes())
            .chain(std::iter::once(0xff))
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("import-map-{hash:016x}")
}

struct FileLoader {
    root: PathBuf,
    imports: BTreeMap<String, PathBuf>,
}

impl ModuleLoader for FileLoader {
    fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
        let path = self.source_path(module_id)?;
        if !matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("ts" | "tsx")
        ) {
            return Err(format!(
                "module `{module_id}` is not a supported .ts source file"
            ));
        }
        let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        Ok(ModuleSource::new(module_id, text))
    }

    fn resolve(&self, from_module: &str, specifier: &str) -> Result<String, String> {
        let candidate = if matches!(specifier, "." | "..")
            || specifier.starts_with("./")
            || specifier.starts_with("../")
        {
            self.root.join(
                Path::new(from_module)
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(specifier),
            )
        } else {
            self.resolve_import_map(specifier)?
        };
        let resolved = fs::canonicalize(&candidate).map_err(|error| {
            format!("cannot resolve `{specifier}` from `{from_module}`: {error}")
        })?;
        if !resolved.starts_with(&self.root) {
            return Err(format!(
                "specifier `{specifier}` resolves outside the declared project root"
            ));
        }
        self.module_id(&resolved)
    }
}

impl FileLoader {
    fn source_path(&self, module_id: &str) -> Result<PathBuf, String> {
        let module_path = Path::new(module_id);
        if module_path.is_absolute()
            || module_path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(format!(
                "module `{module_id}` escapes the declared project root"
            ));
        }
        let path = fs::canonicalize(self.root.join(module_path))
            .map_err(|error| format!("cannot read module `{module_id}`: {error}"))?;
        if !path.starts_with(&self.root) {
            return Err(format!(
                "module `{module_id}` escapes the declared project root"
            ));
        }
        Ok(path)
    }

    fn module_id(&self, path: &Path) -> Result<String, String> {
        path.strip_prefix(&self.root).map(output_path).map_err(|_| {
            format!(
                "module {} escapes the declared project root",
                path.display()
            )
        })
    }

    fn resolve_import_map(&self, specifier: &str) -> Result<PathBuf, String> {
        if let Some(target) = self.imports.get(specifier) {
            return Ok(target.clone());
        }
        let Some((prefix, target)) = self
            .imports
            .iter()
            .filter(|(prefix, _)| prefix.ends_with('/') && specifier.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
        else {
            return Err(format!(
                "bare specifier `{specifier}` is unsupported; add an exact or trailing-slash `imports` mapping"
            ));
        };
        Ok(target.join(&specifier[prefix.len()..]))
    }
}

fn publish_build(
    root: &Path,
    out_dir: &Path,
    artifacts: &std::collections::BTreeMap<String, blueice_bluets::BuildArtifact>,
    metadata: &BuildMetadata,
) -> io::Result<()> {
    let output = absolute_path(out_dir)?;
    if output == root || (output.exists() && fs::canonicalize(&output)? == root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "output directory must not replace the project root",
        ));
    }
    let parent = output
        .parent()
        .ok_or_else(|| io::Error::other("output directory has no parent"))?;
    fs::create_dir_all(parent)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage = parent.join(format!(".bluetsc-stage-{}-{nonce}", std::process::id()));
    fs::create_dir(&stage)?;
    let write_result = (|| -> io::Result<()> {
        for (module_id, artifact) in artifacts {
            let module_path = Path::new(module_id);
            let relative = artifact_relative_path(root, module_path, module_id)?;
            let output_relative = relative.with_extension("js");
            let js_path = stage.join(&output_relative);
            let js_parent = js_path
                .parent()
                .ok_or_else(|| io::Error::other("artifact path has no parent"))?;
            fs::create_dir_all(js_parent)?;
            let mut javascript = artifact.javascript.clone();
            if artifact.source_map.is_some() {
                let map_name = js_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| format!("{name}.map"))
                    .ok_or_else(|| io::Error::other("artifact filename is not UTF-8"))?;
                javascript.push_str(&format!("\n//# sourceMappingURL={map_name}\n"));
            }
            fs::write(&js_path, javascript)?;
            if let Some(source_map) = &artifact.source_map {
                fs::write(js_path.with_extension("js.map"), source_map.to_json())?;
            }
            if let Some(declaration) = &artifact.declaration {
                fs::write(js_path.with_extension("d.ts"), declaration)?;
            }
        }
        let manifest = serde_json::to_vec_pretty(metadata).map_err(io::Error::other)?;
        fs::write(stage.join("bluetsc.manifest.json"), manifest)?;
        if !metadata.imports.is_empty() {
            let import_map = serde_json::json!({ "imports": &metadata.imports });
            let import_map = serde_json::to_vec_pretty(&import_map).map_err(io::Error::other)?;
            fs::write(stage.join("bluetsc.importmap.json"), import_map)?;
        }
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }
    if !output.exists() {
        return fs::rename(stage, output);
    }
    if !output.is_dir() {
        let _ = fs::remove_dir_all(&stage);
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists and is not a directory", output.display()),
        ));
    }
    let backup = parent.join(format!(".bluetsc-backup-{}-{nonce}", std::process::id()));
    fs::rename(&output, &backup)?;
    if let Err(error) = fs::rename(&stage, &output) {
        let _ = fs::rename(&backup, &output);
        return Err(error);
    }
    fs::remove_dir_all(backup)
}

fn absolute_existing_path(path: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(path)
}

fn absolute_path(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

fn artifact_relative_path(root: &Path, path: &Path, module_id: &str) -> io::Result<PathBuf> {
    if path.is_absolute() {
        return path.strip_prefix(root).map(Path::to_path_buf).map_err(|_| {
            io::Error::other(format!("artifact `{module_id}` is outside project root"))
        });
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(io::Error::other(format!(
            "artifact `{module_id}` escapes the project root"
        )));
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
fn path_id(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn project_module_id(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(output_path)
        .expect("validated module is beneath project root")
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_bluets::{BuildArtifact, SourceMap};
    use std::collections::BTreeMap;

    fn args(values: &[&str]) -> Result<Args, String> {
        parse_args(values.iter().map(|value| value.to_string()))
    }

    #[test]
    fn build_requires_an_output_directory() {
        assert_eq!(
            args(&["build", "main.ts"]),
            Err("build requires --out-dir <directory>".to_string())
        );
    }

    #[test]
    fn parses_check_with_a_strict_policy() {
        let parsed = args(&[
            "check",
            "main.ts",
            "--runtime-policy",
            "strict-runtime",
            "--target",
            "es2020",
        ])
        .unwrap();
        assert_eq!(parsed.command, Command::Check);
        let Input::Entry { options, .. } = parsed.input else {
            panic!("expected explicit entry input");
        };
        assert_eq!(options.runtime_policy, RuntimePolicy::StrictRuntime);
        assert_eq!(options.target, EcmaTarget::Es2020);
    }

    #[test]
    fn config_mode_has_no_flag_escape_hatch() {
        assert_eq!(
            args(&["check", "--config", "bluetsc.json", "--source-map"]),
            Err(
                "`--config` owns project settings; unsupported extra argument `--source-map`"
                    .to_string()
            )
        );
    }

    #[test]
    fn configured_output_cannot_escape_its_project_root() {
        assert!(configured_output_path(Path::new("/project"), "../outside").is_err());
        assert!(configured_output_path(Path::new("/project"), ".").is_err());
        assert!(configured_output_path(Path::new("/project"), "").is_err());
        assert_eq!(
            configured_output_path(Path::new("/project"), "dist").unwrap(),
            PathBuf::from("/project/dist")
        );
    }

    #[test]
    fn resolver_fingerprint_is_project_root_relative() {
        let first_root = Path::new("/first/project");
        let second_root = Path::new("/second/project");
        let first = BTreeMap::from([("@shared/".to_string(), first_root.join("src/shared"))]);
        let second = BTreeMap::from([("@shared/".to_string(), second_root.join("src/shared"))]);
        assert_eq!(
            import_map_fingerprint(first_root, &first),
            import_map_fingerprint(second_root, &second)
        );
    }

    #[test]
    fn artifact_paths_accept_root_relative_ids_but_reject_parent_traversal() {
        assert_eq!(
            artifact_relative_path(
                Path::new("/project"),
                Path::new("src/main.ts"),
                "src/main.ts"
            )
            .unwrap(),
            PathBuf::from("src/main.ts")
        );
        assert!(artifact_relative_path(
            Path::new("/project"),
            Path::new("../outside.ts"),
            "../outside.ts"
        )
        .is_err());
    }

    #[test]
    fn project_module_ids_are_root_relative() {
        assert_eq!(
            project_module_id(Path::new("/project"), Path::new("/project/src/main.ts")),
            "src/main.ts"
        );
    }

    fn test_metadata() -> BuildMetadata {
        BuildMetadata {
            language_version: "blue-ts-test",
            fingerprint: "bts-project-test".to_string(),
            target: "es2022",
            runtime_policy: "checked",
            source_map: true,
            declaration: true,
            entries: vec!["src/main.js".to_string()],
            imports: BTreeMap::new(),
        }
    }

    #[test]
    fn publishing_replaces_a_complete_output_directory_only_after_staging() {
        let temporary = unique_test_directory("publish");
        let root = temporary.join("source");
        let output = temporary.join("output");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("obsolete.js"), "old output").unwrap();
        let module = root.join("src/main.ts");
        let artifacts = BTreeMap::from([(
            path_id(&module),
            BuildArtifact {
                module_id: path_id(&module),
                javascript: "export const answer = 42;".to_string(),
                source_map: Some(SourceMap {
                    file: "main.js".to_string(),
                    sources: vec!["src/main.ts".to_string()],
                    sources_content: vec!["export const answer: number = 42;".to_string()],
                    mappings: "AAAA".to_string(),
                }),
                declaration: Some("export declare const answer: number;\n".to_string()),
                fingerprint: "test".to_string(),
            },
        )]);

        publish_build(&root, &output, &artifacts, &test_metadata()).unwrap();

        assert_eq!(
            fs::read_to_string(output.join("src/main.js")).unwrap(),
            "export const answer = 42;\n//# sourceMappingURL=main.js.map\n"
        );
        assert!(output.join("src/main.js.map").is_file());
        assert!(output.join("src/main.d.ts").is_file());
        let manifest = fs::read_to_string(output.join("bluetsc.manifest.json")).unwrap();
        assert!(manifest.contains("\"languageVersion\": \"blue-ts-test\""));
        assert!(!manifest.contains(&root.to_string_lossy().into_owned()));
        assert!(!output.join("obsolete.js").exists());
        fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn staging_failure_does_not_replace_an_existing_output_directory() {
        let temporary = unique_test_directory("publish-failure");
        let root = temporary.join("source");
        let output = temporary.join("output");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("preserved.js"), "previous artifact").unwrap();
        let artifacts = BTreeMap::from([(
            path_id(&temporary.join("outside.ts")),
            BuildArtifact {
                module_id: path_id(&temporary.join("outside.ts")),
                javascript: String::new(),
                source_map: None,
                declaration: None,
                fingerprint: "test".to_string(),
            },
        )]);

        assert!(publish_build(&root, &output, &artifacts, &test_metadata()).is_err());
        assert_eq!(
            fs::read_to_string(output.join("preserved.js")).unwrap(),
            "previous artifact"
        );
        fs::remove_dir_all(temporary).unwrap();
    }

    fn unique_test_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "blueice-bluets-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        path
    }
}
