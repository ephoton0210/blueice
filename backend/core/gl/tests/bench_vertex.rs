// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Developer measurement, not a regression test: how long one vertex-shader run
//! takes for a shader dumped from a real page (`BI_DUMP_GLSL`).
//! (`BI_VERT`; `BI_FRAG` does the same for a fragment shader.)
//! `BI_VERT=path.vert cargo test -p blueice-gl --release --test bench_vertex -- --ignored --nocapture`

use blueice_gl::glsl::builtins::NoTextures;
use blueice_gl::glsl::interp::{Machine, Program};

#[test]
#[ignore = "needs a dumped shader; run by hand"]
fn vertex_shader_cost() {
    let Ok(path) = std::env::var("BI_VERT") else {
        return;
    };
    let source = std::fs::read_to_string(path).unwrap();
    let program = Program::compile(&source).expect("compiles");
    let mut machine = Machine::new(&program, &NoTextures).expect("machine");
    let n = 20_000;
    let start = std::time::Instant::now();
    for _ in 0..n {
        machine.run_main().expect("runs");
    }
    let each = start.elapsed() / n;
    eprintln!(
        "one vertex run: {each:?}  ({} per second)",
        1_000_000_000 / each.as_nanos().max(1)
    );
}

#[test]
#[ignore = "needs a dumped shader; run by hand"]
fn fragment_shader_cost() {
    let Ok(path) = std::env::var("BI_FRAG") else {
        return;
    };
    let source = std::fs::read_to_string(path).unwrap();
    let program = Program::compile(&source).expect("compiles");
    let mut machine = Machine::new(&program, &NoTextures).expect("machine");
    let n = 20_000;
    let start = std::time::Instant::now();
    for _ in 0..n {
        machine.run_main().expect("runs");
    }
    let each = start.elapsed() / n;
    eprintln!(
        "one fragment run: {each:?}  ({} per second)",
        1_000_000_000 / each.as_nanos().max(1)
    );
}
