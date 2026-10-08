// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

use std::process::Command;

#[test]
fn page_dom_requires_an_owned_isolated_host_before_spawning_services() {
    let output = Command::new(env!("CARGO_BIN_EXE_blueice-launcher"))
        .args(["--page-dom-bindings"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("--page-dom-bindings requires --out-of-process-bluejs"));
}
