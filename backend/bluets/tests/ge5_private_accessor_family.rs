// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete frozen private-accessor products and their public CLI boundary.
use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn private_accessor_public_cli_phases() {
    run("verify_ge5_private_accessor_public.cjs");
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and complete Native roles"]
fn private_accessor_all_frozen_products() {
    run("verify_ge5_private_accessor_roles.cjs");
}

fn run(script: &str) {
    let compiler = std::env::var_os("BLUEICE_GE5_CLI_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into());
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools")
        .join(script);
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
