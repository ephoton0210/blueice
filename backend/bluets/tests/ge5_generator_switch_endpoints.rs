// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public generator protocol and provenance boundaries across all frozen products.
use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn generator_switch_all_source_endpoints_keep_native_phases() {
    run("verify_ge5_generator_switch_endpoints.cjs");
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn generator_switch_borrowed_methods_keep_the_target_receiver_abi() {
    run("verify_ge5_generator_switch_borrowed.cjs");
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn generator_switch_getters_and_abrupt_completion_keep_the_target_abi() {
    run("verify_ge5_generator_switch_runtime.cjs");
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn generator_switch_borrowed_callers_preserve_source_this_and_arguments() {
    run("verify_ge5_generator_switch_capture.cjs");
}

fn run(name: &str) {
    let compiler = std::env::var_os("BLUEICE_GE5_CLI_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into());
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools")
        .join(name);
    let mut command = Command::new("node");
    command
        .arg("--experimental-vm-modules")
        .arg(script)
        .arg(compiler);
    if let Some(hash) = std::env::var_os("BLUEICE_GE5_EXPECTED_COMPILER_SHA256") {
        command.arg(hash);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
