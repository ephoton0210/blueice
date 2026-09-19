// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Process-boundary coverage for `bluets-test-interface`.
//!
//! Unlike a long-lived interactive session that is killed when the test ends,
//! every test here closes the adapter's stdin and waits for it to exit
//! normally, so the process flushes its coverage counters and its clean-exit
//! contract (status 0 at end of input) is itself checked.

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

/// Runs one adapter process over `requests` (one JSON document per line),
/// closes stdin, and returns the ready banner plus one reply per request.
fn session(lines: &[String]) -> (Value, Vec<Value>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bluets-test-interface"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    {
        let mut input = child.stdin.take().unwrap();
        for line in lines {
            writeln!(input, "{line}").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "adapter must exit cleanly at EOF");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let mut replies = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap());
    let ready = replies.next().expect("ready banner");
    let replies: Vec<Value> = replies.collect();
    assert_eq!(replies.len(), lines.len(), "one reply per request");
    (ready, replies)
}

fn requests(values: &[Value]) -> (Value, Vec<Value>) {
    session(&values.iter().map(Value::to_string).collect::<Vec<_>>())
}

fn one(request: Value) -> Value {
    let (ready, mut replies) = requests(&[request]);
    assert_eq!(ready, json!({"ready": 1}));
    replies.remove(0)
}

#[test]
fn exits_cleanly_with_only_the_ready_banner_when_no_request_arrives() {
    let (ready, replies) = session(&[]);
    assert_eq!(ready, json!({"ready": 1}));
    assert!(replies.is_empty());
}

#[test]
fn every_mode_is_accepted_and_unknown_modes_are_harness_errors() {
    for mode in ["raw", "sloppy", "strict", "module"] {
        let reply = one(json!({"source": "const answer: number = 1;", "mode": mode}));
        assert_eq!(reply["kind"], "ok", "{mode}: {reply}");
        assert_eq!(reply["phase"], "compile");
        assert_eq!(reply["language_version"], "blue-ts-0.1");
        assert!(reply["fingerprint"].as_str().unwrap().starts_with("bts-"));
    }
    // The default mode is `raw`.
    assert_eq!(one(json!({"source": "const a: number = 1;"}))["kind"], "ok");
    let reply = one(json!({"source": "const a: number = 1;", "mode": "async"}));
    assert_eq!(reply["kind"], "harness_error");
    assert!(reply["message"].as_str().unwrap().contains("`async`"));
}

#[test]
fn runtime_policies_are_selected_by_name() {
    for policy in ["checked", "transpile-only", "strict-runtime"] {
        let reply = one(json!({
            "source": "export function id(value: number): number { return value; }",
            "runtime_policy": policy,
        }));
        assert_eq!(reply["kind"], "ok", "{policy}: {reply}");
    }
    let reply = one(json!({
        "source": "const a: number = 1;",
        "runtime_policy": "yolo",
    }));
    assert_eq!(reply["kind"], "harness_error");
    assert!(reply["message"].as_str().unwrap().contains("`yolo`"));
}

#[test]
fn parse_only_requests_report_the_parse_phase() {
    let reply = one(json!({"source": "const a: number = 1;", "parse_only": true}));
    assert_eq!(reply["kind"], "ok");
    assert_eq!(reply["phase"], "parse");
    // Syntax errors are still reported while only parsing.
    let reply = one(json!({"source": "const = ;", "parse_only": true}));
    assert_eq!(reply["kind"], "SyntaxError");
    assert_eq!(reply["phase"], "parse");
}

#[test]
fn artifact_flags_are_reported_per_module() {
    let reply = one(json!({
        "source": "export const answer: number = 42;",
        "source_map": true,
        "declaration": true,
    }));
    assert_eq!(reply["kind"], "ok");
    let artifacts = reply["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0]["module"], "memory:///entry.ts");
    assert_eq!(artifacts[0]["source_map"], true);
    assert_eq!(artifacts[0]["declaration"], true);

    let reply = one(json!({"source": "export const answer: number = 42;"}));
    assert_eq!(reply["artifacts"][0]["source_map"], false);
    assert_eq!(reply["artifacts"][0]["declaration"], false);
}

#[test]
fn each_diagnostic_code_maps_to_a_stable_phase_and_kind() {
    let cases: Vec<(Value, &str, &str, &str)> = vec![
        (
            json!({"source": "const = ;"}),
            "SyntaxError",
            "parse",
            "BTS1000",
        ),
        (
            json!({"source": "enum Colour { Red }"}),
            "unsupported",
            "parse",
            "BTS1001",
        ),
        (
            json!({"source": "import './missing.ts'; const a: number = 1;"}),
            "SyntaxError",
            "resolution",
            "BTS2000",
        ),
        (
            json!({
                "source": "import './b.ts'; export const a: number = 1;",
                "module_path": "a.ts",
                "module_sources": {
                    "b.ts": "import './a.ts'; export const b: number = 2;",
                },
            }),
            "SyntaxError",
            "resolution",
            "BTS2001",
        ),
        (
            json!({
                "source": "import { create } from './types.d.ts'; create();",
                "module_sources": {
                    "types.d.ts": "export declare function create(): string;",
                },
            }),
            "TypeError",
            "type",
            "BTS2002",
        ),
        (
            json!({"source": "const dup: number = 1; const dup: number = 2;"}),
            "TypeError",
            "type",
            "BTS3000",
        ),
        (
            json!({"source": "const value: Missing = 1;"}),
            "TypeError",
            "type",
            "BTS3002",
        ),
        (
            json!({"source": "const value: string = 1;"}),
            "TypeError",
            "type",
            "BTS3003",
        ),
        (
            json!({"source": "function f(): number { return 'text'; }"}),
            "TypeError",
            "type",
            "BTS3004",
        ),
        (
            json!({"source": "const a: number = 1;", "limits": {"max_tokens": 2}}),
            "resource_error",
            "compile",
            "BTS9000",
        ),
    ];
    let inputs: Vec<Value> = cases.iter().map(|case| case.0.clone()).collect();
    let (_, replies) = requests(&inputs);
    for ((request, kind, phase, code), reply) in cases.iter().zip(&replies) {
        assert_eq!(reply["kind"], *kind, "{request}: {reply}");
        assert_eq!(reply["phase"], *phase, "{request}: {reply}");
        assert_eq!(reply["code"], *code, "{request}: {reply}");
        assert!(!reply["message"].as_str().unwrap().is_empty());
        assert!(
            reply["span"]["start_byte"].as_u64().unwrap()
                <= reply["span"]["end_byte"].as_u64().unwrap(),
            "{reply}"
        );
    }
}

#[test]
fn module_paths_are_canonicalised_into_memory_urls() {
    let bad = "const = ;";
    let paths = [
        ("entry.ts", "memory:///entry.ts"),
        ("./src/app.ts", "memory:///src/app.ts"),
        ("/rooted/app.ts", "memory:///rooted/app.ts"),
        ("nested/dir/app.ts", "memory:///nested/dir/app.ts"),
        ("https://example.test/app.ts", "https://example.test/app.ts"),
    ];
    let inputs: Vec<Value> = paths
        .iter()
        .map(|(path, _)| json!({"source": bad, "module_path": path}))
        .collect();
    let (_, replies) = requests(&inputs);
    for ((path, module), reply) in paths.iter().zip(&replies) {
        assert_eq!(reply["span"]["module"], *module, "{path}: {reply}");
    }
}

#[test]
fn imports_resolve_against_supplied_module_sources() {
    let reply = one(json!({
        "source": "import type { Envelope } from '../types/envelope.d.ts';\nexport const value: Envelope<string> = { payload: 'Ada' };",
        "module_path": "src/main.ts",
        "module_sources": {
            "types/envelope.d.ts": "export interface Envelope<T> { payload: T }",
        },
        "declaration": true,
    }));
    assert_eq!(reply["kind"], "ok", "{reply}");
    let modules: Vec<&str> = reply["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|artifact| artifact["module"].as_str().unwrap())
        .collect();
    assert_eq!(modules, ["memory:///src/main.ts"]);
}

#[test]
fn the_requested_source_replaces_a_same_named_module_source() {
    // `module_sources` also carries a (broken) copy of the entry module; the
    // request's own `source` is authoritative.
    let reply = one(json!({
        "source": "export const fine: number = 1;",
        "module_path": "app.ts",
        "module_sources": {"./app.ts": "const = ;", "other.ts": "export const x: number = 1;"},
    }));
    assert_eq!(reply["kind"], "ok", "{reply}");
}

#[test]
fn every_resource_limit_field_is_forwarded_to_the_compiler() {
    // Generous values leave a small program untouched.
    let reply = one(json!({
        "source": "export const value: number = 1;",
        "source_map": true,
        "limits": {
            "max_modules": 100,
            "max_module_edges": 100,
            "max_module_depth": 100,
            "max_total_source_bytes": 1_000_000,
            "max_source_bytes": 1_000_000,
            "max_tokens": 100_000,
            "max_type_depth": 100,
            "max_type_expansions": 100_000,
            "max_source_map_segments": 100_000,
        },
    }));
    assert_eq!(reply["kind"], "ok", "{reply}");

    // Each limit, tightened on its own, produces a resource error.
    let graph = |limits: Value| {
        json!({
            "source": "import './model.ts'; export const value: number = 1;",
            "module_path": "main.ts",
            "module_sources": {"model.ts": "export const model: number = 2;"},
            "limits": limits,
        })
    };
    let chain = |limits: Value| {
        json!({
            "source": "import './mid.ts'; export const value: number = 1;",
            "module_path": "main.ts",
            "module_sources": {
                "mid.ts": "import './leaf.ts'; export const mid: number = 2;",
                "leaf.ts": "export const leaf: number = 3;",
            },
            "limits": limits,
        })
    };
    let tight = [
        ("max_modules", graph(json!({"max_modules": 1}))),
        ("max_module_edges", graph(json!({"max_module_edges": 0}))),
        ("max_module_depth", chain(json!({"max_module_depth": 1}))),
        (
            "max_total_source_bytes",
            json!({"source": "const value: number = 1;", "limits": {"max_total_source_bytes": 8}}),
        ),
        (
            "max_source_bytes",
            json!({"source": "const value: number = 1;", "limits": {"max_source_bytes": 4}}),
        ),
        (
            "max_tokens",
            json!({"source": "const value: number = 1;", "limits": {"max_tokens": 2}}),
        ),
        (
            "max_type_depth",
            json!({
                "source": "type Deep = { value: { value: { value: string } } };",
                "limits": {"max_type_depth": 2},
            }),
        ),
        (
            "max_source_map_segments",
            json!({
                "source": "export const value: number = 1;",
                "source_map": true,
                "limits": {"max_source_map_segments": 0},
            }),
        ),
    ];
    let inputs: Vec<Value> = tight.iter().map(|(_, request)| request.clone()).collect();
    let (_, replies) = requests(&inputs);
    for ((name, _), reply) in tight.iter().zip(&replies) {
        assert_eq!(reply["kind"], "resource_error", "{name}: {reply}");
        assert_eq!(reply["code"], "BTS9000", "{name}: {reply}");
        assert_eq!(reply["phase"], "compile", "{name}: {reply}");
    }
}

#[test]
fn a_generic_expansion_limit_is_reported_as_a_resource_error() {
    let source = "type List<T> = T[]; const values: List<number> = [1];";
    let reply = one(json!({"source": source, "limits": {"max_type_expansions": 0}}));
    assert_eq!(reply["kind"], "resource_error", "{reply}");
    assert_eq!(reply["phase"], "compile");
    assert_eq!(reply["code"], "BTS9000");
    assert!(reply["message"]
        .as_str()
        .unwrap()
        .contains("generic-expansion limit"));
    // The same program compiles under a generous limit.
    let reply = one(json!({"source": source, "limits": {"max_type_expansions": 100}}));
    assert_eq!(reply["kind"], "ok", "{reply}");
}

#[test]
fn malformed_and_ill_typed_requests_do_not_end_the_session() {
    let lines = vec![
        "{".to_string(),
        "42".to_string(),
        "{}".to_string(),
        json!({"source": 3}).to_string(),
        json!({"source": "const a: number = 1;", "limits": {"max_tokens": "many"}}).to_string(),
        json!({"source": "const a: number = 1;"}).to_string(),
    ];
    let (ready, replies) = session(&lines);
    assert_eq!(ready, json!({"ready": 1}));
    for reply in &replies[..5] {
        assert_eq!(reply["kind"], "harness_error", "{reply}");
        assert!(!reply["message"].as_str().unwrap().is_empty());
    }
    assert_eq!(replies[5]["kind"], "ok");
}
