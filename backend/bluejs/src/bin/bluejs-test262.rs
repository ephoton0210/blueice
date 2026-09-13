// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSON-lines adapter. The external supervisor owns whole-case wall deadlines.
use blueice_bluejs::{
    compile_module_with_limit, compile_with_limit, parse, parse_module, CompileError, RuntimeError,
    Vm, VmConfig,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
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
    module_path: Option<String>,
    #[serde(default)]
    module_sources: HashMap<String, String>,
    #[serde(default)]
    module_source_requests: Vec<String>,
    #[serde(default)]
    asynchronous: bool,
    #[serde(default)]
    parse_only: bool,
    #[serde(default)]
    is_html_dda: bool,
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
        RuntimeError::Unsupported(reason) => {
            return json!({"kind":"unsupported", "reason":reason, "message":error.to_string()});
        }
        RuntimeError::TypeError(_) => "TypeError",
        RuntimeError::RangeError(_) => "RangeError",
        RuntimeError::SyntaxError(_) => "SyntaxError",
        RuntimeError::ReferenceError(_) => "ReferenceError",
        RuntimeError::Test262(_) => "Test262Error",
        RuntimeError::Thrown(_) => "ThrownValue",
        // An instruction budget is a deterministic runner resource boundary,
        // not a wall-clock hang. Preserve that distinction so inventory
        // timeouts identify work that failed to terminate under supervision.
        RuntimeError::InstructionLimit => "resource_error",
        RuntimeError::RegexTimeout => "timeout",
        RuntimeError::RegexWorker(_) => "worker_error",
        RuntimeError::ModuleResolution(_) => "SyntaxError",
        _ => "resource_error",
    };
    let phase = if matches!(error, RuntimeError::ModuleResolution(_)) {
        "resolution"
    } else {
        "runtime"
    };
    json!({"phase":phase, "kind":kind, "message":error.to_string()})
}

/// Preserve the observable name of an Error object thrown through JavaScript.
/// The host adapter otherwise loses the difference between `$ERROR(...)` and
/// an arbitrary thrown value, which makes valid Test262 runtime-negative
/// cases look like unclassified `ThrownValue` failures.
fn runtime_with_vm(vm: &Vm, error: RuntimeError) -> Value {
    if let RuntimeError::Thrown(blueice_bluejs::Value::Object(object)) = &error {
        if let Ok(blueice_bluejs::Value::String(name)) = vm.heap().get(*object, "name") {
            if let Ok(name) = name.to_utf8() {
                return json!({
                    "phase": "runtime",
                    "kind": name,
                    "message": error.to_string(),
                });
            }
        }
    }
    runtime(error)
}

fn evaluate(request: Request) -> Value {
    let source = if request.mode == "strict" {
        format!("\"use strict\";\n{}", request.source)
    } else {
        request.source
    };
    let parse_error = |error: blueice_bluejs::ParseError| match error.resource {
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
    let compile_error = |error: CompileError| match error {
        CompileError::Unsupported(reason) => json!({"kind":"unsupported", "reason":reason}),
        CompileError::ProgramTooLarge => {
            json!({"kind":"resource_error", "message":error.to_string()})
        }
        _ => json!({"phase":"parse", "kind":"SyntaxError", "message":error.to_string()}),
    };
    let mut module_codes = HashMap::new();
    let code = if request.mode == "module" {
        let entry = request
            .module_path
            .clone()
            .unwrap_or_else(|| "<entry>".to_string());
        let program = match parse_module(&source) {
            Ok(program) => program,
            Err(error) => return parse_error(error),
        };
        let code =
            match compile_module_with_limit(&program, request.bytecode_limit.unwrap_or(u32::MAX)) {
                Ok(code) => code,
                Err(error) => return compile_error(error),
            };
        module_codes.insert(entry, code);
        for (path, module_source) in &request.module_sources {
            if module_codes.contains_key(path) {
                continue;
            }
            let program = match parse_module(module_source) {
                Ok(program) => program,
                Err(error) if error.known_syntax => {
                    return json!({
                        "phase":"resolution",
                        "kind":"SyntaxError",
                        "message":error.message,
                    });
                }
                Err(error) => return parse_error(error),
            };
            let code = match compile_module_with_limit(
                &program,
                request.bytecode_limit.unwrap_or(u32::MAX),
            ) {
                Ok(code) => code,
                Err(CompileError::DuplicateBinding(message)) => {
                    return json!({
                        "phase":"resolution",
                        "kind":"SyntaxError",
                        "message":message,
                    });
                }
                Err(CompileError::InvalidSyntax(message)) => {
                    return json!({
                        "phase":"resolution",
                        "kind":"SyntaxError",
                        "message":message,
                    });
                }
                Err(error) => return compile_error(error),
            };
            module_codes.insert(path.clone(), code);
        }
        None
    } else {
        let program = match parse(&source) {
            Ok(program) => program,
            Err(error) => return parse_error(error),
        };
        match compile_with_limit(&program, request.bytecode_limit.unwrap_or(u32::MAX)) {
            Ok(code) => Some(code),
            Err(error) => return compile_error(error),
        }
    };
    if request.mode != "module" {
        for (path, module_source) in &request.module_sources {
            if request.module_path.as_deref() == Some(path.as_str()) {
                continue;
            }
            let program = match parse_module(module_source) {
                Ok(program) => program,
                Err(error) => return parse_error(error),
            };
            let code = match compile_module_with_limit(
                &program,
                request.bytecode_limit.unwrap_or(u32::MAX),
            ) {
                Ok(code) => code,
                Err(error) => return compile_error(error),
            };
            module_codes.insert(path.clone(), code);
        }
    }
    if request.parse_only {
        return json!({"kind":"ok", "phase":"parse"});
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
    vm.set_module_source_loader_context(request.module_source_requests);
    if request.mode != "raw" {
        if let Err(error) = vm.install_test262_harness() {
            return json!({"kind":"harness_error", "message":error.to_string()});
        }
    }
    if request.asynchronous {
        if let Err(error) = vm.install_test262_done() {
            return json!({"kind":"harness_error", "message":error.to_string()});
        }
    }
    if request.is_html_dda {
        if let Err(error) = vm.install_test262_is_html_dda() {
            return json!({"kind":"harness_error", "message":error.to_string()});
        }
    }
    if request.mode != "module" && !module_codes.is_empty() {
        vm.set_module_loader_context(
            request
                .module_path
                .clone()
                .unwrap_or_else(|| "<script>".to_string()),
            module_codes.clone(),
        );
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
            return runtime_with_vm(&vm, error);
        }
    }
    let execution = if request.mode == "module" {
        let entry = request.module_path.as_deref().unwrap_or("<entry>");
        vm.execute_module_graph(entry, &module_codes)
    } else {
        vm.execute_script(code.as_ref().expect("script compilation produced bytecode"))
    };
    let reply = match execution {
        Ok(_) if request.asynchronous => match vm.run_test262_async_until_done() {
            Err(error) => runtime_with_vm(&vm, error),
            Ok(Some(Ok(()))) => json!({"kind":"ok", "phase":"runtime"}),
            Ok(Some(Err(value))) => runtime_with_vm(&vm, RuntimeError::Thrown(value)),
            Ok(None) => json!({"kind":"timeout", "message":"async test did not call $DONE"}),
        },
        Ok(_) => json!({"kind":"ok", "phase":"runtime"}),
        Err(error) => runtime_with_vm(&vm, error),
    };
    match vm.shutdown_test262_agents() {
        Ok(()) => reply,
        Err(error) if reply.get("kind") == Some(&json!("ok")) => runtime_with_vm(&vm, error),
        Err(_) => reply,
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
