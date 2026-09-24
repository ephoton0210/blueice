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
    /// Raw text for non-`.js` module fixtures (currently JSON only), keyed
    /// by the same test-root-relative path `module_sources`/`module_path`
    /// use. Kept separate from `module_sources` because those entries are
    /// parsed as JavaScript module source unconditionally.
    #[serde(default)]
    module_json_sources: HashMap<String, String>,
    /// Decoded text (UTF-8) for resources imported with `type: "text"`,
    /// keyed like `module_json_sources`. A resource may appear here and in
    /// `module_sources` at once: the attribute is part of a request's identity.
    #[serde(default)]
    module_text_sources: HashMap<String, String>,
    /// Raw bytes for resources imported with `type: "bytes"`, keyed like
    /// `module_json_sources`.
    #[serde(default)]
    module_bytes_sources: HashMap<String, Vec<u8>>,
    /// Paths in `module_sources` reached only through a relative-string
    /// heuristic (e.g. a `ShadowRealm.prototype.importValue` specifier
    /// argument), never through an actual `import`/dynamic-`import()`
    /// reference. Such a candidate may be a deliberately invalid fixture
    /// meant to be discovered lazily at runtime rather than linked eagerly;
    /// a parse/compile failure there is simply excluded from the module
    /// registry instead of failing the whole request, unlike every other
    /// (genuinely required) `module_sources` entry.
    #[serde(default)]
    speculative_module_sources: std::collections::HashSet<String>,
    /// Raw JavaScript text for `.js` fixtures reached only through a
    /// dynamic import, never a static one -- kept separate from
    /// `module_sources` (which this adapter parses/compiles eagerly, before
    /// any code runs) specifically so a fixture that is a syntax/semantic
    /// error only *as a module* fails lazily, as that dynamic import's own
    /// promise rejection, via `Vm::set_dynamic_module_sources`.
    #[serde(default)]
    module_dynamic_sources: HashMap<String, String>,
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

fn apply_gc_stress_overrides(config: &mut VmConfig, var: impl Fn(&str) -> Option<String>) {
    if let Some(capacity) =
        var("BLUEJS_TEST262_NURSERY_CAPACITY").and_then(|value| value.parse().ok())
    {
        config.heap.nursery_capacity = capacity;
    }
    if let Some(bytes) = var("BLUEJS_TEST262_MAJOR_THRESHOLD").and_then(|value| value.parse().ok())
    {
        config.heap.major_threshold_bytes = bytes;
    }
}

fn parse_error(error: blueice_bluejs::ParseError) -> Value {
    match error.resource {
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
    }
}

fn compile_error(error: CompileError) -> Value {
    match error {
        CompileError::Unsupported(reason) => json!({"kind":"unsupported", "reason":reason}),
        CompileError::ProgramTooLarge => {
            json!({"kind":"resource_error", "message":error.to_string()})
        }
        _ => json!({"phase":"parse", "kind":"SyntaxError", "message":error.to_string()}),
    }
}

/// A module the graph genuinely requires that fails to build is a static
/// linking failure, so a specified syntax error is reported in the resolution
/// phase rather than as the entry point's own parse failure.
fn required_module_parse_error(error: blueice_bluejs::ParseError) -> Value {
    if error.known_syntax {
        resolution_error(error.message)
    } else {
        parse_error(error)
    }
}

fn required_module_compile_error(error: CompileError) -> Value {
    match error {
        CompileError::DuplicateBinding(message) => resolution_error(message),
        CompileError::InvalidSyntax(message) => resolution_error(message.to_string()),
        error => compile_error(error),
    }
}

fn resolution_error(message: String) -> Value {
    json!({"phase":"resolution", "kind":"SyntaxError", "message":message})
}

fn evaluate(request: Request) -> Value {
    let source = if request.mode == "strict" {
        format!("\"use strict\";\n{}", request.source)
    } else {
        request.source
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
            let speculative = request.speculative_module_sources.contains(path);
            let program = match parse_module(module_source) {
                Ok(program) => program,
                Err(_) if speculative => continue,
                Err(error) => return required_module_parse_error(error),
            };
            let code = match compile_module_with_limit(
                &program,
                request.bytecode_limit.unwrap_or(u32::MAX),
            ) {
                Ok(code) => code,
                Err(_) if speculative => continue,
                Err(error) => return required_module_compile_error(error),
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
    // A script-mode entry that dynamically imports *itself* (e.g.
    // `language/expressions/dynamic-import/eval-self-once-script.js`) names
    // a path that is always classified "static" (the entry) by the
    // harness's own `module_sources()`, yet is deliberately excluded from
    // `module_codes` just below -- it is compiled once, as the script this
    // request actually executes, never twice as a module too. Its raw text
    // must still reach `Vm::set_dynamic_module_sources` (below), so that
    // self-referential dynamic import can compile it on demand rather than
    // finding it in neither registry.
    let mut dynamic_sources = request.module_dynamic_sources.clone();
    if request.mode != "module" {
        for (path, module_source) in &request.module_sources {
            if request.module_path.as_deref() == Some(path.as_str()) {
                dynamic_sources
                    .entry(path.clone())
                    .or_insert_with(|| module_source.clone());
                continue;
            }
            let speculative = request.speculative_module_sources.contains(path);
            let program = match parse_module(module_source) {
                Ok(program) => program,
                Err(_) if speculative => continue,
                Err(error) => return parse_error(error),
            };
            let code = match compile_module_with_limit(
                &program,
                request.bytecode_limit.unwrap_or(u32::MAX),
            ) {
                Ok(code) => code,
                Err(_) if speculative => continue,
                Err(error) => return compile_error(error),
            };
            module_codes.insert(path.clone(), code);
        }
    }
    if request.parse_only {
        return json!({"kind":"ok", "phase":"parse"});
    }
    // The native host replaces these two core helpers. Every other include,
    // `propertyHelper.js` and `isConstructor.js` among them, executes as a
    // separate classic script in the same VM realm.
    let unknown: Vec<_> = request
        .includes
        .iter()
        .filter(|name| !matches!(name.as_str(), "sta.js" | "assert.js"))
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
    // GC stress mode: a tiny nursery makes nearly every allocation a collection
    // point, so a native function that leaves an object unrooted across a
    // later allocation fails deterministically instead of by timing luck.
    apply_gc_stress_overrides(&mut config, |name| std::env::var(name).ok());
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
    vm.set_json_module_sources(request.module_json_sources.clone());
    vm.set_text_module_sources(request.module_text_sources.clone());
    vm.set_bytes_module_sources(request.module_bytes_sources.clone());
    vm.set_dynamic_module_sources(dynamic_sources);
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
    // A referrer context is needed whenever the harness detected a dynamic
    // import at all (`request.module_path` is set), even when it resolves
    // only `.json`/other non-`.js` fixtures and `module_codes` ends up
    // empty -- an empty registry still fixes `dynamic_import`'s referrer to
    // this test's own path, rather than the resolution-breaking `"<script>"`
    // default `resolve_module_request` falls back to otherwise.
    if request.mode != "module" && (request.module_path.is_some() || !module_codes.is_empty()) {
        vm.set_module_loader_context(
            request
                .module_path
                .clone()
                .unwrap_or_else(|| "<script>".to_string()),
            module_codes.clone(),
        );
    }
    // Harness includes never get the synthetic "strict mode" wrapper --
    // only the test body does (`source`, above). Each include runs as its
    // own top-level script; wrapping it in a `"use strict"` this adapter
    // invented (rather than one the include's own source declares) makes
    // every function defined at that script's top level strict too,
    // breaking any harness helper that relies on an unprefixed *internal*
    // `eval` to observe sloppy-mode-only behavior (direct eval inherits
    // strictness from its immediately enclosing script, not just from its
    // own text) -- e.g. sm/non262-strict-shell.js's `testLenientAndStrict`
    // and sm/non262-expressions-shell.js's `testDestructuringArrayDefault`.
    for source in request.harness_sources {
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

fn serve(input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    writeln!(output, "{{\"ready\":1}}")?;
    output.flush()?;
    for line in input.lines() {
        let reply = match serde_json::from_str::<Request>(&line?) {
            Ok(request) => evaluate(request),
            Err(error) => json!({"kind":"harness_error", "message":error.to_string()}),
        };
        writeln!(output, "{reply}")?;
        output.flush()?;
    }
    Ok(())
}

fn main() -> io::Result<()> {
    serve(io::stdin().lock(), io::stdout().lock())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval_request(mode: &str, harness_sources: Vec<String>, source: &str) -> Value {
        evaluate(Request {
            source: source.to_string(),
            mode: mode.to_string(),
            includes: Vec::new(),
            harness_sources,
            module_path: None,
            module_sources: HashMap::new(),
            module_json_sources: HashMap::new(),
            module_text_sources: HashMap::new(),
            module_bytes_sources: HashMap::new(),
            speculative_module_sources: std::collections::HashSet::new(),
            module_dynamic_sources: HashMap::new(),
            module_source_requests: Vec::new(),
            asynchronous: false,
            parse_only: false,
            is_html_dda: false,
            bytecode_limit: None,
            instruction_budget: None,
            heap_limit: None,
            regex_timeout_ms: None,
            string_limit: None,
        })
    }

    // Mirrors the shape of sm/non262-strict-shell.js's `testLenientAndStrict`
    // and sm/non262-expressions-shell.js's `testDestructuringArrayDefault`:
    // a harness include whose own helper relies on an *unprefixed* internal
    // `eval` to observe sloppy-mode-only behavior (here, an implicit global
    // created by an undeclared assignment).
    const SLOPPY_ONLY_HARNESS: &str = r#"
        globalThis.isSloppyImplicitGlobal = function(name) {
            try {
                eval(name + " = 1;");
            } catch (e) {
                return false;
            }
            return typeof globalThis[name] !== "undefined";
        };
    "#;

    fn req(value: Value) -> Value {
        evaluate(serde_json::from_value(value).expect("valid request"))
    }

    fn kind(reply: &Value) -> &str {
        reply["kind"].as_str().expect("reply has a kind")
    }

    #[test]
    fn runtime_errors_map_to_their_reply_kinds() {
        let unsupported = runtime(RuntimeError::Unsupported("thing"));
        assert_eq!(unsupported["kind"], json!("unsupported"));
        assert_eq!(unsupported["reason"], json!("thing"));
        let missing_hook = runtime(RuntimeError::ReferenceError("$262".into()));
        assert_eq!(missing_hook["reason"], json!("missing Test262 host hook"));
        assert_eq!(
            runtime(RuntimeError::RegexWorker("w".into()))["kind"],
            json!("worker_error")
        );
        assert_eq!(
            runtime(RuntimeError::RegexTimeout)["kind"],
            json!("timeout")
        );
        assert_eq!(
            runtime(RuntimeError::InstructionLimit)["kind"],
            json!("resource_error")
        );
        assert_eq!(
            runtime(RuntimeError::StringLimit { limit: 1 })["kind"],
            json!("resource_error")
        );
        let resolution = runtime(RuntimeError::ModuleResolution("m".into()));
        assert_eq!(resolution["kind"], json!("SyntaxError"));
        assert_eq!(resolution["phase"], json!("resolution"));
        for (error, name) in [
            (RuntimeError::TypeError("m".into()), "TypeError"),
            (RuntimeError::RangeError("m".into()), "RangeError"),
            (RuntimeError::SyntaxError("m".into()), "SyntaxError"),
            (RuntimeError::ReferenceError("m".into()), "ReferenceError"),
            (RuntimeError::Test262("m".into()), "Test262Error"),
            (
                RuntimeError::Thrown(blueice_bluejs::Value::Null),
                "ThrownValue",
            ),
        ] {
            assert_eq!(runtime(error)["kind"], json!(name));
        }
    }

    #[test]
    fn thrown_objects_report_their_name_only_when_it_is_a_usable_string() {
        assert_eq!(
            kind(&req(
                json!({"source":"throw new RangeError('x')","mode":"sloppy"})
            )),
            "RangeError"
        );
        // A non-string `name`, and a name that is not valid UTF-8 (a lone
        // surrogate), both fall back to the generic thrown-value reply.
        assert_eq!(
            kind(&req(json!({"source":"throw {name: 1}","mode":"sloppy"}))),
            "ThrownValue"
        );
        assert_eq!(
            kind(&req(
                json!({"source":"throw {name: '\\ud800'}","mode":"sloppy"})
            )),
            "ThrownValue"
        );
    }

    #[test]
    fn parse_and_compile_failures_are_classified() {
        let parse = |known_syntax, resource| {
            parse_error(blueice_bluejs::ParseError {
                message: "m".into(),
                resource,
                known_syntax,
            })
        };
        assert_eq!(parse(true, None)["kind"], json!("SyntaxError"));
        assert_eq!(
            parse(false, None)["kind"],
            json!("unclassified_parse_error")
        );
        let resource = parse(false, Some(RuntimeError::InstructionLimit));
        assert_eq!(resource["kind"], json!("resource_error"));
        assert_eq!(resource["phase"], json!("parse"));

        assert_eq!(
            compile_error(CompileError::Unsupported("u"))["reason"],
            json!("u")
        );
        assert_eq!(
            compile_error(CompileError::ProgramTooLarge)["kind"],
            json!("resource_error")
        );
        assert_eq!(
            compile_error(CompileError::InvalidSyntax("s"))["kind"],
            json!("SyntaxError")
        );

        let reply = req(json!({"source":"a b","mode":"sloppy"}));
        assert_eq!(
            (kind(&reply), &reply["phase"]),
            ("SyntaxError", &json!("parse"))
        );
        let reply = req(json!({"source":"1;","mode":"sloppy","bytecode_limit":1}));
        assert_eq!(kind(&reply), "resource_error");
    }

    #[test]
    fn required_module_failures_are_resolution_errors_when_they_are_syntax_errors() {
        let syntax = |known_syntax| blueice_bluejs::ParseError {
            message: "m".into(),
            resource: None,
            known_syntax,
        };
        assert_eq!(
            required_module_parse_error(syntax(true))["phase"],
            json!("resolution")
        );
        assert_eq!(
            required_module_parse_error(syntax(false))["kind"],
            json!("unclassified_parse_error")
        );
        for error in [
            CompileError::DuplicateBinding("d".into()),
            CompileError::InvalidSyntax("i"),
        ] {
            let reply = required_module_compile_error(error);
            assert_eq!(
                (kind(&reply), &reply["phase"]),
                ("SyntaxError", &json!("resolution"))
            );
        }
        assert_eq!(
            required_module_compile_error(CompileError::ProgramTooLarge)["kind"],
            json!("resource_error")
        );
    }

    #[test]
    fn module_entry_failures_surface_before_any_execution() {
        assert_eq!(
            req(json!({"source":"@","mode":"module"}))["phase"],
            json!("parse")
        );
        assert_eq!(
            kind(&req(
                json!({"source":"1;","mode":"module","bytecode_limit":1})
            )),
            "resource_error"
        );
    }

    #[test]
    fn module_mode_links_required_and_speculative_module_sources() {
        let base = |sources: Value, speculative: Value| {
            req(json!({
                "source": "import './dep.js'; assert.sameValue(1, 1);",
                "mode": "module",
                "module_path": "entry.js",
                "module_sources": sources,
                "speculative_module_sources": speculative,
            }))
        };
        // The entry's own path in `module_sources` is skipped, not compiled twice.
        assert_eq!(
            kind(&base(
                json!({"entry.js":"@", "dep.js":"export {};"}),
                json!([])
            )),
            "ok"
        );
        // Speculative candidates that do not parse or compile are dropped.
        assert_eq!(
            kind(&base(
                json!({"dep.js":"export {};", "bad.js":"@"}),
                json!(["bad.js"])
            )),
            "ok"
        );
        assert_eq!(
            kind(&base(
                json!({"dep.js":"export {};", "dup.js":"let a; let a;"}),
                json!(["dup.js"])
            )),
            "ok"
        );
        // A speculative candidate that only fails to compile is dropped too; a required one is reported.
        let big = "export var a = 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8;";
        let run = |speculative: Value| {
            req(json!({
                "source": "",
                "mode": "module",
                "module_path": "entry.js",
                "module_sources": {"dep.js": big},
                "speculative_module_sources": speculative,
                "bytecode_limit": 6,
            }))
        };
        assert_eq!(kind(&run(json!(["dep.js"]))), "ok");
        assert_eq!(kind(&run(json!([]))), "resource_error");
        // A required source with a syntax error is a resolution-phase SyntaxError.
        for bad in ["@", "let a; let a;", "export {x};"] {
            let reply = base(json!({"dep.js": bad}), json!([]));
            assert_eq!(kind(&reply), "SyntaxError", "{bad}: {reply}");
            assert_eq!(reply["phase"], json!("resolution"), "{bad}: {reply}");
        }
    }

    #[test]
    fn script_mode_registers_module_sources_for_dynamic_import() {
        let run = |sources: Value, speculative: Value, limit: Value| {
            req(json!({
                "source": "",
                "mode": "sloppy",
                "module_path": "self.js",
                "module_sources": sources,
                "speculative_module_sources": speculative,
                "bytecode_limit": limit,
            }))
        };
        // The script's own path is only kept as dynamic-import source text.
        assert_eq!(
            kind(&run(json!({"self.js":"@"}), json!([]), Value::Null)),
            "ok"
        );
        assert_eq!(
            kind(&run(json!({"m.js":"export {};"}), json!([]), Value::Null)),
            "ok"
        );
        assert_eq!(
            kind(&run(json!({"m.js":"@"}), json!(["m.js"]), Value::Null)),
            "ok"
        );
        assert_eq!(
            run(json!({"m.js":"@"}), json!([]), Value::Null)["phase"],
            json!("parse")
        );
        assert_eq!(
            kind(&run(
                json!({"m.js":"let a; let a;"}),
                json!(["m.js"]),
                Value::Null
            )),
            "ok"
        );
        let big = "export var a = 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8;";
        // Speculation forgives a compile failure too; without it the failure is reported.
        assert_eq!(
            kind(&run(json!({"m.js":big}), json!(["m.js"]), json!(6))),
            "ok"
        );
        assert_eq!(
            kind(&run(json!({"m.js":big}), json!([]), json!(6))),
            "resource_error"
        );
    }

    #[test]
    fn script_without_a_module_path_imports_relative_to_the_script_default() {
        let reply = req(json!({
            "source": "import('m.js').then(() => $DONE(), $DONE)",
            "mode": "sloppy",
            "asynchronous": true,
            "module_sources": {"m.js": "export {};"},
        }));
        assert_eq!(kind(&reply), "ok", "{reply}");
    }

    #[test]
    fn parse_only_stops_before_running_anything() {
        let reply = req(json!({"source":"throw 1","mode":"sloppy","parse_only":true}));
        assert_eq!((kind(&reply), &reply["phase"]), ("ok", &json!("parse")));
    }

    #[test]
    fn unknown_includes_need_harness_sources() {
        let reply = req(json!({"source":"1","mode":"sloppy","includes":["propertyHelper.js"]}));
        assert_eq!(kind(&reply), "unsupported");
        let reply = req(json!({"source":"1","mode":"sloppy","includes":["assert.js","sta.js"]}));
        assert_eq!(kind(&reply), "ok");
    }

    #[test]
    fn resource_limits_and_gc_stress_are_applied_to_the_vm() {
        let mut config = VmConfig::default();
        let overrides = [
            ("BLUEJS_TEST262_NURSERY_CAPACITY", "4096"),
            ("BLUEJS_TEST262_MAJOR_THRESHOLD", "8192"),
        ];
        apply_gc_stress_overrides(&mut config, |name| {
            let (_, value) = overrides.iter().find(|(key, _)| *key == name)?;
            Some(value.to_string())
        });
        assert_eq!(config.heap.nursery_capacity, 4096);
        assert_eq!(config.heap.major_threshold_bytes, 8192);
        let before = VmConfig::default();
        let mut unchanged = VmConfig::default();
        apply_gc_stress_overrides(&mut unchanged, |_| Some("not a number".into()));
        assert_eq!(
            unchanged.heap.nursery_capacity,
            before.heap.nursery_capacity
        );
        apply_gc_stress_overrides(&mut unchanged, |_| None);
        assert_eq!(
            unchanged.heap.major_threshold_bytes,
            before.heap.major_threshold_bytes
        );

        let reply = req(json!({"source":"for(;;);","mode":"sloppy","instruction_budget":100}));
        assert_eq!(kind(&reply), "resource_error");
        let reply = req(
            json!({"source":"var a=[]; for(;;) a.push({x:1,y:'abc'+a.length});","mode":"sloppy","heap_limit":3_000_000,"instruction_budget":100_000_000}),
        );
        assert_eq!(kind(&reply), "resource_error", "{reply}");
        let reply = req(json!({"source":"'x'.repeat(1000)","mode":"sloppy","string_limit":10}));
        assert_eq!(kind(&reply), "resource_error", "{reply}");
        let reply = req(json!({"source":"1","mode":"sloppy","regex_timeout_ms":5000}));
        assert_eq!(kind(&reply), "ok", "{reply}");
    }

    #[test]
    fn host_hook_installation_failures_are_harness_errors() {
        // Raw mode skips the harness, so a heap this small still builds the VM
        // and then fails while installing each optional host hook.
        for hook in ["asynchronous", "is_html_dda"] {
            let reply = req(json!({"source":"","mode":"raw", hook:true, "heap_limit":100_000}));
            assert_eq!(kind(&reply), "harness_error", "{hook}: {reply}");
        }
        let reply = req(json!({"source":"","mode":"sloppy","heap_limit":100_000}));
        assert_eq!(kind(&reply), "harness_error", "{reply}");
        let reply = req(json!({"source":"","mode":"raw","heap_limit":0}));
        assert_eq!(kind(&reply), "harness_error", "{reply}");
    }

    #[test]
    fn host_hooks_install_according_to_the_request() {
        let done = req(json!({"source":"$DONE()","mode":"sloppy","asynchronous":true}));
        assert_eq!(kind(&done), "ok", "{done}");
        let failed =
            req(json!({"source":"$DONE(new TypeError('x'))","mode":"sloppy","asynchronous":true}));
        assert_eq!(kind(&failed), "TypeError", "{failed}");
        let silent = req(json!({"source":"1","mode":"sloppy","asynchronous":true}));
        assert_eq!(kind(&silent), "timeout");
        let job_error = req(json!({
            "source":"Promise.resolve().then(() => { for(;;); })",
            "mode":"sloppy","asynchronous":true,"instruction_budget":2000,
        }));
        assert_eq!(kind(&job_error), "resource_error", "{job_error}");
        let dda = req(
            json!({"source":"assert.sameValue(typeof $262.IsHTMLDDA, 'undefined')","mode":"sloppy","is_html_dda":true}),
        );
        assert_eq!(kind(&dda), "ok", "{dda}");
        let raw = req(json!({"source":"$262","mode":"raw"}));
        assert_eq!(raw["reason"], json!("missing Test262 host hook"));
    }

    #[test]
    fn harness_sources_that_do_not_build_or_run_are_reported() {
        let reply = eval_request("sloppy", vec!["@".into()], "1");
        assert_eq!(reply["reason"], json!("harness source parse unsupported"));
        let reply = req(
            json!({"source":"","mode":"sloppy","harness_sources":["1+2+3+4+5+6+7+8;"],"bytecode_limit":6}),
        );
        assert_eq!(
            reply["reason"],
            json!("harness source compile unsupported"),
            "{reply}"
        );
        let reply = eval_request("sloppy", vec!["throw new RangeError('h')".into()], "1");
        assert_eq!(kind(&reply), "RangeError");
    }

    #[test]
    fn a_failing_agent_only_replaces_a_successful_reply() {
        let source = r#"$262.agent.start("throw 1"); $262.agent.sleep(200);"#;
        let reply = req(json!({"source":source,"mode":"sloppy"}));
        assert_eq!(kind(&reply), "Test262Error", "{reply}");
        let source = format!("{source} throw new RangeError('first')");
        let reply = req(json!({"source":source,"mode":"sloppy"}));
        assert_eq!(kind(&reply), "RangeError", "{reply}");
    }

    #[test]
    fn serve_announces_readiness_then_answers_one_reply_per_line() {
        let input = "{\"source\":\"1\",\"mode\":\"sloppy\"}\nnot json\n";
        let mut output = Vec::new();
        serve(input.as_bytes(), &mut output).unwrap();
        let lines: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines[0], json!({"ready":1}));
        assert_eq!(kind(&lines[1]), "ok");
        assert_eq!(kind(&lines[2]), "harness_error");
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn harness_includes_stay_sloppy_under_a_strict_mode_test_body() {
        // Only the test body gets Test262's synthetic "use strict" prefix
        // for the "strict" execution mode -- never a harness include. A
        // harness's own internal `eval` must keep observing sloppy-mode
        // behavior regardless of the test body's own mode, exactly like
        // real Test262 fixtures that depend on this (e.g.
        // `staging/sm/strict/10.6.js`, `staging/sm/expressions/
        // destructuring-array-default-simple.js`).
        let reply = eval_request(
            "strict",
            vec![SLOPPY_ONLY_HARNESS.to_string()],
            "assert.sameValue(isSloppyImplicitGlobal('bluejsCorpusProblemProbe'), true);",
        );
        assert_eq!(reply["kind"], json!("ok"), "reply was: {reply}");
    }

    #[test]
    fn harness_includes_stay_sloppy_under_a_sloppy_mode_test_body_too() {
        let reply = eval_request(
            "sloppy",
            vec![SLOPPY_ONLY_HARNESS.to_string()],
            "assert.sameValue(isSloppyImplicitGlobal('bluejsCorpusProblemProbeSloppy'), true);",
        );
        assert_eq!(reply["kind"], json!("ok"), "reply was: {reply}");
    }

    #[test]
    fn the_test_body_itself_is_still_strict_prefixed() {
        // The fix must not stop prefixing the test body -- only harness
        // includes. A bare implicit global in the *test body* itself must
        // still throw ReferenceError under "strict" mode.
        let reply = eval_request("strict", Vec::new(), "bluejsCorpusProblemProbeBody = 1;");
        assert_eq!(reply["kind"], json!("ReferenceError"), "reply was: {reply}");
    }
}
