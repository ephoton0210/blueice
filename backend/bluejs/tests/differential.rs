// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A deliberately small, scoped Node-vs-BlueJS corpus. It compares the public
//! binaries, not two in-process evaluators, and records wall-clock timing in
//! assertion diagnostics for the performance baseline Phase 13 calls for.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn run(binary: &str, args: &[&str]) -> (std::process::Output, Duration) {
    let start = Instant::now();
    let output = Command::new(binary)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("failed to run {binary}: {error}"));
    (output, start.elapsed())
}

#[test]
fn mvp_corpus_matches_nodes_console_output_and_exit_status() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let bluejs = env!("CARGO_BIN_EXE_bluejs");
    for name in ["arithmetic.js", "closure.js", "destructuring.js", "loop.js"] {
        let script = corpus.join(name);
        let script = script.to_str().unwrap();
        let (node, node_elapsed) = run("node", &[script]);
        let (bluejs, bluejs_elapsed) = run(bluejs, &["--stdout-only", script]);

        assert_eq!(
            bluejs.status.code(),
            node.status.code(),
            "{name}: exit mismatch; node={node_elapsed:?}, bluejs={bluejs_elapsed:?}, bluejs stderr={}",
            String::from_utf8_lossy(&bluejs.stderr)
        );
        assert_eq!(
            bluejs.stdout,
            node.stdout,
            "{name}: stdout mismatch; node={node_elapsed:?}, bluejs={bluejs_elapsed:?}, bluejs stderr={}",
            String::from_utf8_lossy(&bluejs.stderr)
        );
    }
}
