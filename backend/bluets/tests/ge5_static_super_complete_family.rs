// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! All six static-super families require the complete module and runtime gate.
use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "requires pinned TypeScript/Acorn, an immutable CLI and its explicit SHA-256"]
fn static_super_all_frozen_products() {
    let compiler = std::env::var_os("BLUEICE_GE5_CLI_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into());
    let hash = std::env::var_os("BLUEICE_GE5_EXPECTED_COMPILER_SHA256")
        .expect("explicit immutable CLI SHA-256");
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_static_super_complete.cjs",
    );
    let output = Command::new("node")
        .arg("--experimental-vm-modules")
        .arg(script)
        .arg(compiler)
        .arg(hash)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
}
