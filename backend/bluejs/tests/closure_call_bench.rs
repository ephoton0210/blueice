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
    let mut s = String::from("(function () {\n");
    for i in 0..captured {
        s += &format!("let v{i} = {i};\n");
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
