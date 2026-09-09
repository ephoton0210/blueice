// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit opt-in: cargo test -p blueice-bluejs --test node_differential -- --ignored
//! Node is an independent oracle, not a runtime or default-test dependency.
use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires Node.js on PATH; run explicitly with --ignored"]
fn primitive_completions_and_error_classes_match_node() {
    let mut corpus = include_str!("fixtures/execution.txt").to_string();
    // Deterministic broad float coverage, including shortest-decimal
    // formatting. Every expected value still comes from Node, not Rust.
    let mut state = 0x83da_172c_d093_1b57u64;
    for _ in 0..512 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let n = f64::from_bits(state);
        if n.is_finite() {
            corpus.push_str(&format!("\n'' + ({n:e})"));
        }
    }
    let mut node = Command::new("node")
        .args(["-e", include_str!("fixtures/node_oracle.js")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("install Node.js to run the opt-in differential test");
    let mut stdin = node.stdin.take().unwrap();
    let input = corpus.clone();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = node.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(output.status.success(), "Node oracle failed: {}", String::from_utf8_lossy(&output.stderr));
    let expected = String::from_utf8(output.stdout).unwrap();
    let sources: Vec<_> = corpus.lines().filter(|line| !line.trim().is_empty() && !line.starts_with('#')).collect();
    let expected: Vec<_> = expected.lines().collect();
    assert_eq!(sources.len(), expected.len(), "oracle must return exactly one result per script");
    println!("Comparing {} isolated scripts against Node.js", sources.len());
    let mut vm = Vm::default();
    for (source, expected) in sources.into_iter().zip(expected) {
        let result = match parse(source) {
            Err(_) => "error:SyntaxError".into(),
            Ok(ast) => match compile(&ast) {
                Err(blueice_bluejs::CompileError::DuplicateBinding(_) | blueice_bluejs::CompileError::InvalidSyntax(_)) => "error:SyntaxError".into(),
                Err(error) => panic!("fixture {source} cannot execute: {error}"),
                Ok(code) => canonical(vm.execute(&code)),
            },
        };
        assert_eq!(result, expected, "{source}");
    }
}

fn canonical(result: Result<Value, RuntimeError>) -> String {
    match result {
        Ok(Value::Undefined) => "undefined".into(),
        Ok(Value::Null) => "null".into(),
        Ok(Value::Bool(b)) => format!("bool:{b}"),
        Ok(Value::Number(n)) if n.is_nan() => "number:NaN".into(),
        Ok(Value::Number(n)) => format!("number:{:016x}", n.to_bits()),
        Ok(Value::String(s)) => format!("string:{}", s.bytes().map(|b| format!("{b:02x}")).collect::<String>()),
        Err(RuntimeError::ReferenceError(_)) => "error:ReferenceError".into(),
        Err(RuntimeError::TypeError(_)) => "error:TypeError".into(),
        other => panic!("unexpected fixture result: {other:?}"),
    }
}
