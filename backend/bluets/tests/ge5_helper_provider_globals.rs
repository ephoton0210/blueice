// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime helper provider routing through the shared real module loader.
use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "requires pinned tslib 2.8.1 and Node VM modules"]
fn private_ambient_and_imported_helpers_keep_their_own_bindings() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_helper_provider_globals.cjs",
    );
    let output = Command::new("node")
        .arg("--experimental-vm-modules")
        .arg(script)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
