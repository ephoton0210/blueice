// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public CLI source-boundary regression for the complete helper matrix.
use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn exported_async_generator_endpoints_all_frozen_products() {
    let compiler = std::env::var_os("BLUEICE_GE5_CLI_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_async_generator_g2_endpoints.cjs",
    );
    let output = Command::new("node")
        .arg(script)
        .arg(compiler)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node VM modules"]
fn borrowed_async_generator_methods_match_every_native_target_profile() {
    let compiler = std::env::var_os("BLUEICE_GE5_CLI_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_async_generator_g2_borrowed.cjs",
    );
    let output = Command::new("node")
        .arg("--experimental-vm-modules")
        .arg(script)
        .arg(compiler)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node VM modules"]
fn borrowed_methods_preserve_source_receiver_and_arguments() {
    let compiler = std::env::var_os("BLUEICE_GE5_CLI_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_async_generator_g2_capture.cjs",
    );
    let output = Command::new("node")
        .arg("--experimental-vm-modules")
        .arg(script)
        .arg(compiler)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
