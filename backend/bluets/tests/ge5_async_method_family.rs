// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Two public builds of every frozen async-object-method product.
use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and complete Native method roles"]
fn async_object_method_all_frozen_products() {
    let compiler = std::env::var_os("BLUEICE_GE5_CLI_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_async_method_roles.cjs",
    );
    let mut command = Command::new("node");
    command
        .arg("--experimental-vm-modules")
        .arg(script)
        .arg(compiler);
    if let Some(hash) = std::env::var_os("BLUEICE_GE5_EXPECTED_COMPILER_SHA256") {
        command.arg(hash);
    }
    let output = command.output().unwrap();
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
