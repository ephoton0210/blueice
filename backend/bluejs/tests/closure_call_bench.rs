// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Developer measurement, not a regression test: the cost of calling a closure that captures
//! many bindings, which is how a bundled page script is shaped (every function sees the
//! module's bindings). `cargo test -p blueice-bluejs --release --test closure_call_bench --
//! --ignored --nocapture`.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};
use std::time::Instant;

fn script(captured: usize, calls: usize) -> String {
    script_with(captured, calls, "{}")
}

/// A caller that holds `captured` object-valued bindings: each one is a root the VM keeps
/// alive for the whole of every call the caller makes.
fn script_with(captured: usize, calls: usize, init: &str) -> String {
    let mut s = String::from("(function () {\n");
    for i in 0..captured {
        s += &format!(
            "let v{i} = {};\n",
            if init == "{}" {
                "{}".to_string()
            } else {
                i.to_string()
            }
        );
    }
    let uses: Vec<String> = (0..captured).map(|i| format!("v{i}")).collect();
    s += &format!(
        "function f(x) {{ if (x < 0) return x + {}; return x; }}\n",
        uses.join(" + ")
    );
    s +=
        &format!("let t = 0; for (let i = 0; i < {calls}; i++) t = f(1) % 1000;\nreturn t;\n}})()");
    s
}

#[test]
#[ignore = "a measurement; run by hand"]
fn calls_of_a_closure_with_many_captures() {
    for captured in [4usize, 40, 200] {
        let source = script(captured, 20_000);
        let program = compile(&parse(&source).unwrap()).unwrap();
        let mut best = f64::INFINITY;
        let mut result = Value::Undefined;
        for _ in 0..3 {
            let mut vm = Vm::new(VmConfig {
                instruction_budget: u64::MAX / 2,
                ..VmConfig::default()
            })
            .unwrap();
            let start = Instant::now();
            result = vm.execute(&program).unwrap();
            best = best.min(start.elapsed().as_secs_f64());
        }
        println!(
            "{captured:>4} captured: {:>8.0} ns per call  (result {result:?})",
            best / 20_000.0 * 1e9
        );
    }
}

/// The caller's own bindings are rooted on every call, whether or not the callee touches
/// them, so the cost of a call grows with how many the caller holds: a bundled page script's
/// top-level function has thousands.
#[test]
#[ignore = "a measurement; run by hand"]
fn calls_made_by_a_caller_that_holds_many_objects() {
    for held in [10usize, 400, 4000] {
        let mut source = String::from("(function () {\n");
        for i in 0..held {
            source += &format!("let v{i} = {{}};\n");
        }
        source += "function f(x) { return x; }\nlet t = 0;\n";
        source += "for (let i = 0; i < 20000; i++) t = f(1);\n";
        // Keep every binding live without calling through them.
        source += &format!("return t + (v0 === v{} ? 0 : 0);\n}})()", held - 1);
        let program = compile(&parse(&source).unwrap()).unwrap();
        let mut best = f64::INFINITY;
        for _ in 0..3 {
            let mut vm = Vm::new(VmConfig {
                instruction_budget: u64::MAX / 2,
                ..VmConfig::default()
            })
            .unwrap();
            let start = Instant::now();
            vm.execute(&program).unwrap();
            best = best.min(start.elapsed().as_secs_f64());
        }
        println!("{held:>5} held: {:>8.0} ns per call", best / 20_000.0 * 1e9);
    }
}
