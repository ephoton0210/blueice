// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native project arguments remain separate from owner runtime authority.

use super::*;
use serde_json::{Map, Value};

pub(super) struct Args {
    pub(super) project: PathBuf,
    pub(super) flags: Map<String, Value>,
}
impl Args {
    pub(super) fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut args = args.into_iter().peekable();
        let mut project = None;
        let mut sources = Vec::new();
        let mut flags = Map::new();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--project" | "-p" => {
                    project = Some(PathBuf::from(
                        args.next()
                            .ok_or_else(|| format!("{arg} requires a path"))?,
                    ));
                }
                "--noEmit" | "--showConfig" | "--listFiles" | "--listEmittedFiles" | "--pretty"
                | "--diagnostics-json" => {
                    let value = match args.peek().map(String::as_str) {
                        Some("true") => {
                            args.next();
                            true
                        }
                        Some("false") => {
                            args.next();
                            false
                        }
                        _ => true,
                    };
                    flags.insert(arg[2..].to_string(), Value::Bool(value));
                }
                "--noEmitOnError" | "--diagnostics" => {
                    let value = match args.peek().map(String::as_str) {
                        Some("true") => {
                            args.next();
                            true
                        }
                        Some("false") => {
                            args.next();
                            false
                        }
                        _ => true,
                    };
                    flags.insert(arg[2..].to_string(), Value::Bool(value));
                }
                _ if arg.starts_with('-') => {
                    return Err(format!("unknown compiler option `{arg}`"))
                }
                _ => sources.push(arg),
            }
        }
        if project.is_some() && !sources.is_empty() {
            return Err("--project cannot be mixed with source files".to_string());
        }
        if !sources.is_empty() {
            return Err(
                "source-file invocations require the check/build command or --project".to_string(),
            );
        }
        let project = match project {
            Some(path) if path.is_dir() => path.join("tsconfig.json"),
            Some(path) => path,
            None => find_config()?,
        };
        Ok(Self { project, flags })
    }
}
fn find_config() -> Result<PathBuf, String> {
    let mut directory = env::current_dir().map_err(|e| e.to_string())?;
    loop {
        let path = directory.join("tsconfig.json");
        if path.is_file() {
            return Ok(path);
        }
        if !directory.pop() {
            return Err("no tsconfig.json found; use --project <path>".to_string());
        }
    }
}

pub(super) fn diagnostic(message: &str) -> Option<Diagnostic> {
    let (code, arguments) = if message.starts_with("unknown compiler option ") {
        let name = message.split('`').nth(1)?.trim_start_matches('-');
        if blueice_bluets::TypeScriptDiagnostic::is_known_compiler_option(name) {
            return None;
        }
        (5023, vec![format!("--{name}")])
    } else if message.ends_with("requires a path") {
        (
            6044,
            vec![message
                .split_whitespace()
                .next()?
                .trim_start_matches('-')
                .to_string()],
        )
    } else if message == "--project cannot be mixed with source files" {
        (5042, Vec::new())
    } else {
        return None;
    };
    Some(
        Diagnostic::error(
            blueice_bluets::DiagnosticCode::ParseError,
            SourceSpan::new("", 0, 0),
            message,
        )
        .with_typescript(code, arguments),
    )
}

pub(super) fn without_machine_flags(arguments: Vec<String>) -> Vec<String> {
    let mut arguments = arguments.into_iter().peekable();
    let mut result = Vec::new();
    while let Some(argument) = arguments.next() {
        if argument == "--diagnostics-json" {
            if arguments
                .peek()
                .is_some_and(|value| value == "true" || value == "false")
            {
                arguments.next();
            }
        } else {
            result.push(argument);
        }
    }
    result
}
