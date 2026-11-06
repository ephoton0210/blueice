// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original async operands keep their boundaries through colliding helper names.
use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and every frozen helper profile"]
fn helper_collision_headers_and_await_endpoints_all_frozen_products() {
    let compiler = std::env::var_os("BLUEICE_GE5_CLI_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_helper_collision_boundaries.cjs",
    );
    let mut command = Command::new("node");
    command.arg(script).arg(compiler);
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
