// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Process-boundary coverage for BlueTS's JSON-lines test interface.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

#[test]
fn interface_uses_the_bluejs_ready_and_json_lines_contract() {
    let mut adapter = Adapter::start();
    let compiled = adapter.request(json!({
        "source": "import type { Envelope } from '../types/envelope.d.ts';\nconst envelope: Envelope<string> = { payload: 'Ada' };",
        "mode": "module",
        "module_path": "src/main.ts",
        "module_sources": {
            "types/envelope.d.ts": "export interface Envelope<T> { payload: T }"
        },
        "source_map": true,
        "declaration": true,
    }));
    assert_eq!(compiled["kind"], "ok");
    assert_eq!(compiled["phase"], "compile");
    assert_eq!(compiled["language_version"], "blue-ts-0.1");
    assert_eq!(compiled["artifacts"].as_array().unwrap().len(), 1);
    assert_eq!(compiled["artifacts"][0]["source_map"], true);
    assert_eq!(compiled["artifacts"][0]["declaration"], true);

    let type_error = adapter.request(json!({
        "source": "function takesNumber(value: number): number { return value; } const value: string = takesNumber('wrong');",
        "mode": "strict",
    }));
    assert_eq!(type_error["kind"], "TypeError");
    assert_eq!(type_error["phase"], "type");
    assert_eq!(type_error["code"], "BTS3003");
    assert_eq!(type_error["span"]["module"], "memory:///entry.ts");

    let limited = adapter.request(json!({
        "source": "const answer: number = 1;",
        "limits": { "max_tokens": 2 },
    }));
    assert_eq!(limited["kind"], "resource_error");
    assert_eq!(limited["code"], "BTS9000");

    let malformed = adapter.raw_request("{");
    assert_eq!(malformed["kind"], "harness_error");
}

struct Adapter {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Adapter {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_bluets-test-interface"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut ready = String::new();
        output.read_line(&mut ready).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&ready).unwrap(),
            json!({"ready": 1})
        );
        Self {
            child,
            input,
            output,
        }
    }

    fn request(&mut self, request: Value) -> Value {
        self.raw_request(&request.to_string())
    }

    fn raw_request(&mut self, request: &str) -> Value {
        writeln!(self.input, "{request}").unwrap();
        self.input.flush().unwrap();
        let mut reply = String::new();
        self.output.read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap()
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
