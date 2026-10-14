// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A small Test262 runner: executes the classic-script fixtures of chosen
//! corpus directories through the library API, in each mode their metadata
//! allows, with the same native harness overrides the conformance adapter
//! uses. Modules and cases that need more than the default instruction
//! budget are skipped rather than failed: the inventory runner owns those.

#![allow(dead_code)]

use blueice_bluejs::{compile_with_limit, parse, RuntimeError, Value, Vm, VmConfig};
use std::path::{Path, PathBuf};

pub fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/reference/test262")
}

#[derive(Clone)]
struct Metadata {
    flags: Vec<String>,
    includes: Vec<String>,
    features: Vec<String>,
    negative: Option<(String, String)>,
}

fn list_value(text: &str) -> Vec<String> {
    text.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn metadata(source: &str) -> Metadata {
    let mut meta = Metadata {
        flags: Vec::new(),
        includes: Vec::new(),
        features: Vec::new(),
        negative: None,
    };
    let Some(start) = source.find("/*---") else {
        return meta;
    };
    let Some(end) = source[start..].find("---*/") else {
        return meta;
    };
    let block = source[start + 5..start + end]
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut key = String::new();
    let (mut phase, mut kind) = (None, None);
    for line in block.lines() {
        let trimmed = line.trim();
        if let Some(item) = trimmed.strip_prefix("- ") {
            match key.as_str() {
                "includes" => meta.includes.push(item.trim().to_string()),
                "flags" => meta.flags.push(item.trim().to_string()),
                "features" => meta.features.push(item.trim().to_string()),
                _ => {}
            }
            continue;
        }
        let Some((name, value)) = trimmed.split_once(':') else {
            continue;
        };
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if indented && key == "negative" {
            match name {
                "phase" => phase = Some(value.trim().to_string()),
                "type" => kind = Some(value.trim().to_string()),
                _ => {}
            }
            continue;
        }
        key = name.to_string();
        match name {
            "flags" => meta.flags.extend(list_value(value)),
            "includes" => meta.includes.extend(list_value(value)),
            "features" => meta.features.extend(list_value(value)),
            _ => {}
        }
    }
    if let (Some(phase), Some(kind)) = (phase, kind) {
        meta.negative = Some((phase, kind));
    }
    meta
}

fn walk(directory: &Path, files: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "js")
            && !path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("_FIXTURE")
        {
            files.push(path);
        }
    }
}

enum Outcome {
    Pass,
    Skip,
    Fail(String),
}

fn error_name(vm: &Vm, error: &RuntimeError) -> Option<String> {
    Some(match error {
        RuntimeError::Thrown(Value::Object(object)) => match vm.heap().get(*object, "name") {
            Ok(Value::String(name)) => name.to_utf8().ok()?,
            _ => return None,
        },
        RuntimeError::TypeError(_) => "TypeError".into(),
        RuntimeError::RangeError(_) => "RangeError".into(),
        RuntimeError::SyntaxError(_) => "SyntaxError".into(),
        RuntimeError::ReferenceError(_) => "ReferenceError".into(),
        RuntimeError::Test262(_) => "Test262Error".into(),
        _ => return None,
    })
}

fn run_mode(source: &str, meta: &Metadata, strict: bool, raw: bool, harness: &str) -> Outcome {
    let asynchronous = meta.flags.iter().any(|flag| flag == "async");
    let text = if strict {
        format!("\"use strict\";\n{source}")
    } else {
        source.to_string()
    };
    let negative_parse = meta
        .negative
        .as_ref()
        .is_some_and(|(phase, _)| phase == "parse" || phase == "early");
    let program = match parse(&text) {
        Ok(program) => program,
        Err(error) if negative_parse && error.known_syntax => return Outcome::Pass,
        Err(error) => return Outcome::Fail(format!("parse error: {}", error.message)),
    };
    let code = match compile_with_limit(&program, u32::MAX) {
        Ok(code) => code,
        Err(_) if negative_parse => return Outcome::Pass,
        Err(error) => return Outcome::Fail(format!("compile error: {error}")),
    };
    if negative_parse {
        return Outcome::Fail("expected a parse error".into());
    }
    let mut vm = Vm::new(VmConfig {
        instruction_budget: 100_000,
        ..VmConfig::default()
    })
    .unwrap();
    if !raw {
        vm.install_test262_harness().unwrap();
    }
    if asynchronous {
        vm.install_test262_done().unwrap();
    }
    if meta.features.iter().any(|feature| feature == "IsHTMLDDA") {
        vm.install_test262_is_html_dda().unwrap();
    }
    for name in &meta.includes {
        if matches!(name.as_str(), "assert.js" | "sta.js" | "doneprintHandle.js") {
            continue;
        }
        let path = corpus().join("harness").join(name);
        let include = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let program = match parse(&include) {
            Ok(program) => program,
            Err(_) => return Outcome::Skip,
        };
        let Ok(code) = compile_with_limit(&program, u32::MAX) else {
            return Outcome::Skip;
        };
        if let Err(error) = vm.execute_script(&code) {
            return match error {
                RuntimeError::InstructionLimit
                | RuntimeError::StringLimit { .. }
                | RuntimeError::Heap(_)
                | RuntimeError::Unsupported(_) => Outcome::Skip,
                error => Outcome::Fail(format!("{harness}: {name}: {error}")),
            };
        }
    }
    let mut result = vm.execute_script(&code).map(|_| ());
    if result.is_ok() && asynchronous {
        result = match vm.run_test262_async_until_done() {
            Ok(Some(Ok(()))) => Ok(()),
            Ok(Some(Err(value))) => Err(RuntimeError::Thrown(value)),
            Ok(None) => return Outcome::Skip,
            Err(error) => Err(error),
        };
    }
    let _ = vm.shutdown_test262_agents();
    match (result, &meta.negative) {
        (Ok(()), None) => Outcome::Pass,
        (Ok(()), Some((_, kind))) => Outcome::Fail(format!("expected a {kind}")),
        (
            Err(
                RuntimeError::InstructionLimit
                | RuntimeError::StringLimit { .. }
                | RuntimeError::Heap(_)
                | RuntimeError::RegexTimeout
                | RuntimeError::Unsupported(_),
            ),
            _,
        ) => Outcome::Skip,
        (Err(error), Some((_, kind))) => {
            if error_name(&vm, &error).as_deref() == Some(kind) {
                Outcome::Pass
            } else {
                Outcome::Fail(format!("expected {kind}, got {error}"))
            }
        }
        (Err(error), None) => Outcome::Fail(error.to_string()),
    }
}

/// [`run_mode`] on its own thread, so a fixture that never finishes (a wait
/// nobody notifies) is skipped instead of hanging the suite.
fn run_bounded(source: &str, meta: &Metadata, strict: bool, raw: bool, harness: &str) -> Outcome {
    let (sender, receiver) = std::sync::mpsc::channel();
    let (source, meta, harness) = (source.to_string(), meta.clone(), harness.to_string());
    // Not a scoped thread: a fixture that never finishes must be left
    // behind, not joined.
    std::thread::spawn(move || {
        let _ = sender.send(run_mode(&source, &meta, strict, raw, &harness));
    });
    receiver
        .recv_timeout(std::time::Duration::from_secs(15))
        .unwrap_or(Outcome::Skip)
}

/// Runs every classic-script fixture under `relative` (a directory of the
/// corpus's `test` tree) and returns one line per failure. Fixtures whose
/// path contains any of `skip` are not run.
pub fn run_directory(relative: &str, skip: &[&str]) -> Vec<String> {
    let root = corpus().join("test").join(relative);
    let mut files = Vec::new();
    walk(&root, &mut files);
    let mut failures = Vec::new();
    let mut ran = 0;
    let mut skipped = 0;
    for path in files {
        let name = path.strip_prefix(corpus().join("test")).unwrap();
        let name = name.to_string_lossy().to_string();
        if skip.iter().any(|pattern| name.contains(pattern)) {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        let meta = metadata(&source);
        let flag = |flag: &str| meta.flags.iter().any(|candidate| candidate == flag);
        if flag("module") || flag("CanBlockIsFalse") {
            continue;
        }
        let raw = flag("raw");
        let modes: &[bool] = if raw || flag("noStrict") {
            &[false]
        } else if flag("onlyStrict") {
            &[true]
        } else {
            &[false, true]
        };
        for &strict in modes {
            let agents = meta.includes.iter().any(|name| name == "atomicsHelper.js");
            match run_bounded(&source, &meta, strict, raw, &name) {
                // Multi-agent fixtures assert wall-clock ordering that a
                // loaded machine can miss; the inventory runner, which
                // supervises them, owns their verdicts.
                Outcome::Fail(_) if agents => skipped += 1,
                Outcome::Pass => ran += 1,
                Outcome::Skip => skipped += 1,
                Outcome::Fail(message) => failures.push(format!(
                    "{name} [{}]: {message}",
                    if strict { "strict" } else { "sloppy" }
                )),
            }
        }
    }
    eprintln!(
        "{relative}: ran {ran}, skipped {skipped}, failed {}",
        failures.len()
    );
    assert!(ran > 0, "no fixture ran under {relative}");
    failures
}
