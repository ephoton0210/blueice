// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Developer measurement, not a regression test: nanoseconds per run of `main` for each
//! corpus shader. `cargo test -p blueice-glsl --release --test bench -- --ignored --nocapture`
//! (`BI_RUNS=n` sets the runs per shader; `BI_ONLY=name` runs one).

mod common;

use blueice_glsl::interp::Machine;
use common::{compile, randomize_inputs, Rng, TestTextures, CORPUS};
use std::time::Instant;

#[test]
#[ignore = "a measurement; run by hand"]
fn nanoseconds_per_run() {
    let runs: usize = std::env::var("BI_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000);
    let only = std::env::var("BI_ONLY").ok();
    let mut total = 0.0;
    println!("{:<28}{:>14}", "shader", "ns per run");
    for entry in CORPUS
        .iter()
        .filter(|e| only.as_deref().is_none_or(|n| e.name == n))
    {
        let program = compile(entry);
        let mut m = Machine::new(&program, &TestTextures).unwrap();
        let mut rng = Rng(42);
        randomize_inputs(&mut m, &mut rng);
        // Warm up, then take the best of three timings (the machine is noisy).
        for _ in 0..runs / 10 {
            m.run_main().unwrap();
        }
        let best = (0..3)
            .map(|_| {
                let start = Instant::now();
                for _ in 0..runs {
                    m.run_main().unwrap();
                }
                start.elapsed().as_nanos() as f64 / runs as f64
            })
            .fold(f64::INFINITY, f64::min);
        total += best;
        println!("{:<28}{:>14.0}", entry.name, best);
    }
    println!("{:<28}{:>14.0}", "total", total);
}
