// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSON-lines adapter. The external supervisor owns whole-case wall deadlines.
use blueice_bluejs::{compile_with_limit, parse, CompileError, RuntimeError, Vm, VmConfig};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

#[derive(Deserialize)]
struct Request {
    source: String,
    mode: String,
    #[serde(default)]
    includes: Vec<String>,
    #[serde(default)]
    harness_sources: Vec<String>,
    #[serde(default)]
    asynchronous: bool,
    #[serde(default)]
    parse_only: bool,
    bytecode_limit: Option<u32>,
    instruction_budget: Option<u64>,
    heap_limit: Option<usize>,
    regex_timeout_ms: Option<u64>,
    string_limit: Option<usize>,
}

fn runtime(error: RuntimeError) -> Value {
    if matches!(&error, RuntimeError::ReferenceError(name) if matches!(name.as_str(), "$262" | "$DONE" | "print"))
    {
        return json!({"kind":"unsupported", "reason":"missing Test262 host hook", "message":error.to_string()});
    }
    let kind = match &error {
        RuntimeError::TypeError(_) => "TypeError",
        RuntimeError::RangeError(_) => "RangeError",
        RuntimeError::SyntaxError(_) => "SyntaxError",
        RuntimeError::ReferenceError(_) => "ReferenceError",
        RuntimeError::Test262(_) => "Test262Error",
        RuntimeError::Thrown(_) => "ThrownValue",
        RuntimeError::RegexTimeout | RuntimeError::InstructionLimit => "timeout",
        RuntimeError::RegexWorker(_) => "worker_error",
        _ => "resource_error",
    };
    json!({"phase":"runtime", "kind":kind, "message":error.to_string()})
}

fn evaluate(request: Request) -> Value {
    if request.mode == "module" {
        return json!({"kind":"unsupported", "reason":"module host"});
    }
    let source = if request.mode == "strict" {
        format!("\"use strict\";\n{}", request.source)
    } else {
        request.source
    };
    let program = match parse(&source) {
        Ok(program) => program,
        Err(error) => {
            return match error.resource {
                Some(resource) => {
                    let mut reply = runtime(resource);
                    reply["phase"] = json!("parse");
                    reply
                }
                // The subset parser has no complete unsupported-grammar taxonomy.
                // Never let its arbitrary rejection satisfy a negative test.
                None if error.known_syntax => {
                    json!({"phase":"parse", "kind":"SyntaxError", "message":error.message})
                }
                None => {
                    json!({"phase":"parse", "kind":"unclassified_parse_error", "message":error.message})
                }
            };
        }
    };
    let code = match compile_with_limit(&program, request.bytecode_limit.unwrap_or(u32::MAX)) {
        Ok(code) => code,
        Err(error) => {
            return match error {
                CompileError::Unsupported(reason) => json!({"kind":"unsupported", "reason":reason}),
                CompileError::ProgramTooLarge => {
                    json!({"kind":"resource_error", "message":error.to_string()})
                }
                _ => json!({"phase":"parse", "kind":"SyntaxError", "message":error.to_string()}),
            };
        }
    };
    if request.parse_only {
        return json!({"kind":"ok", "phase":"parse"});
    }
    if request.asynchronous {
        return json!({"kind":"unsupported", "reason":"async jobs and $DONE host"});
    }
    // The native host replaces these core helpers. Other includes execute as
    // separate classic scripts in the same VM realm.
    let unknown: Vec<_> = request
        .includes
        .iter()
        .filter(|name| {
            !matches!(
                name.as_str(),
                "sta.js" | "assert.js" | "propertyHelper.js" | "isConstructor.js"
            )
        })
        .collect();
    if request.mode != "raw" && !unknown.is_empty() && request.harness_sources.is_empty() {
        return json!({"kind":"unsupported", "reason":"harness includes require persistent script globals", "includes":unknown});
    }
    // Conformance inputs run under an explicit, bounded interpreter budget.
    // Keep the library VM default independent from the runner's resource policy.
    let mut config = VmConfig {
        instruction_budget: request.instruction_budget.unwrap_or(100_000),
        ..VmConfig::default()
    };
    if let Some(limit) = request.heap_limit {
        config.heap.max_heap_bytes = limit;
        config.heap.major_threshold_bytes = config.heap.major_threshold_bytes.min(limit);
    }
    if let Some(limit) = request.string_limit {
        config.max_string_bytes = limit;
    }
    if let Some(timeout) = request.regex_timeout_ms {
        config.regex_timeout = std::time::Duration::from_millis(timeout);
    }
    let mut vm = match Vm::new(config) {
        Ok(vm) => vm,
        Err(error) => return json!({"kind":"harness_error", "message":error.to_string()}),
    };
    if request.mode != "raw" {
        if let Err(error) = vm.install_test262_harness() {
            return json!({"kind":"harness_error", "message":error.to_string()});
        }
    }
    for source in request.harness_sources {
        let source = if request.mode == "strict" {
            format!("\"use strict\";\n{source}")
        } else {
            source
        };
        let program = match parse(&source) {
            Ok(program) => program,
            Err(error) => {
                return json!({"kind":"unsupported", "reason":"harness source parse unsupported", "message":error.message})
            }
        };
        let code = match compile_with_limit(&program, request.bytecode_limit.unwrap_or(u32::MAX)) {
            Ok(code) => code,
            Err(error) => {
                return json!({"kind":"unsupported", "reason":"harness source compile unsupported", "message":error.to_string()})
            }
        };
        if let Err(error) = vm.execute_script(&code) {
            return runtime(error);
        }
    }
    match vm.execute_script(&code) {
        Ok(_) => json!({"kind":"ok", "phase":"runtime"}),
        Err(error) => runtime(error),
    }
}

fn main() -> io::Result<()> {
    let mut output = io::stdout().lock();
    writeln!(output, "{{\"ready\":1}}")?;
    output.flush()?;
    for line in io::stdin().lock().lines() {
        let reply = match serde_json::from_str::<Request>(&line?) {
            Ok(request) => evaluate(request),
            Err(error) => json!({"kind":"harness_error", "message":error.to_string()}),
        };
        writeln!(output, "{reply}")?;
        output.flush()?;
    }
    Ok(())
}
