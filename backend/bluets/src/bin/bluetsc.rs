// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The standalone BlueTSC command-line front end.

use blueice_bluets::{
    compile, CompilerOptions, EcmaTarget, ModuleLoader, ModuleSource, RuntimePolicy,
};
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
    entry: PathBuf,
    project_root: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    options: CompilerOptions,
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
    let entry = match absolute_existing_path(&args.entry) {
        Ok(entry) => entry,
        Err(error) => {
            eprintln!(
                "bluetsc: cannot read entry {}: {error}",
                args.entry.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let root = match args.project_root {
        Some(root) => match absolute_existing_path(&root) {
            Ok(root) if root.is_dir() => root,
            Ok(root) => {
                eprintln!(
                    "bluetsc: --project-root {} is not a directory",
                    root.display()
                );
                return ExitCode::FAILURE;
            }
            Err(error) => {
                eprintln!(
                    "bluetsc: cannot access --project-root {}: {error}",
                    root.display()
                );
                return ExitCode::FAILURE;
            }
        },
        None => entry
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
    };
    if !entry.starts_with(&root) {
        eprintln!(
            "bluetsc: entry {} is outside project root {}",
            entry.display(),
            root.display()
        );
        return ExitCode::FAILURE;
    }
    let loader = FileLoader { root: root.clone() };
    let result = compile(&path_id(&entry), &loader, args.options.clone());
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
    if result.has_errors() {
        return ExitCode::FAILURE;
    }
    if args.command == Command::Check {
        println!(
            "checked {} module(s), fingerprint {}",
            result.project.modules.len(),
            result
                .output
                .as_ref()
                .expect("successful compilation has output")
                .fingerprint
        );
        return ExitCode::SUCCESS;
    }
    let out_dir = args
        .out_dir
        .expect("build argument parsing requires --out-dir");
    let output = result.output.expect("successful compilation has output");
    match publish_build(&root, &out_dir, &output.artifacts) {
        Ok(()) => {
            println!(
                "built {} module(s), fingerprint {}",
                output.artifacts.len(),
                output.fingerprint
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
    let entry = args
        .next()
        .ok_or_else(|| "an entry .ts file is required".to_string())
        .map(PathBuf::from)?;
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
        entry,
        project_root,
        out_dir,
        options,
    })
}

fn usage() -> &'static str {
    "Usage:\n  bluetsc check <entry.ts> [--project-root <directory>] [--target es2020|es2022] [--runtime-policy transpile-only|checked|strict-runtime]\n  bluetsc build <entry.ts> --out-dir <directory> [--project-root <directory>] [--source-map] [--declaration] [--target es2020|es2022] [--runtime-policy transpile-only|checked|strict-runtime]"
}

struct FileLoader {
    root: PathBuf,
}

impl ModuleLoader for FileLoader {
    fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
        let path = PathBuf::from(module_id);
        if !path.starts_with(&self.root) {
            return Err(format!(
                "module `{module_id}` escapes the declared project root"
            ));
        }
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
        if !matches!(specifier, "." | "..")
            && !specifier.starts_with("./")
            && !specifier.starts_with("../")
        {
            return Err(format!(
                "bare specifier `{specifier}` is unsupported; configure an explicit project resolver"
            ));
        }
        let candidate = Path::new(from_module)
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(specifier);
        let resolved = fs::canonicalize(&candidate).map_err(|error| {
            format!("cannot resolve `{specifier}` from `{from_module}`: {error}")
        })?;
        if !resolved.starts_with(&self.root) {
            return Err(format!(
                "specifier `{specifier}` resolves outside the declared project root"
            ));
        }
        Ok(path_id(&resolved))
    }
}

fn publish_build(
    root: &Path,
    out_dir: &Path,
    artifacts: &std::collections::BTreeMap<String, blueice_bluets::BuildArtifact>,
) -> io::Result<()> {
    let output = absolute_path(out_dir)?;
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
            let relative = module_path.strip_prefix(root).map_err(|_| {
                io::Error::other(format!("artifact `{module_id}` is outside project root"))
            })?;
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

fn path_id(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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
        assert_eq!(parsed.options.runtime_policy, RuntimePolicy::StrictRuntime);
        assert_eq!(parsed.options.target, EcmaTarget::Es2020);
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

        publish_build(&root, &output, &artifacts).unwrap();

        assert_eq!(
            fs::read_to_string(output.join("src/main.js")).unwrap(),
            "export const answer = 42;\n//# sourceMappingURL=main.js.map\n"
        );
        assert!(output.join("src/main.js.map").is_file());
        assert!(output.join("src/main.d.ts").is_file());
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

        assert!(publish_build(&root, &output, &artifacts).is_err());
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
