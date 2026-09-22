// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fs;
use std::process::Command;

#[test]
fn batch_shell_executes_a_file_and_prints_console_then_completion() {
    let directory = std::env::temp_dir().join(format!("bluejs-cli-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let script = directory.join("program.js");
    fs::write(&script, "console.log('hello'); 2 + 3;").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bluejs"))
        .arg(&script)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "hello\n5\n");
    fs::remove_file(script).unwrap();
    fs::remove_dir(directory).unwrap();
}

#[test]
fn stdout_only_mode_matches_nodes_console_only_batch_contract() {
    let directory = std::env::temp_dir().join(format!("bluejs-cli-stdout-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let script = directory.join("program.js");
    fs::write(&script, "console.log('hello'); 2 + 3;").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bluejs"))
        .args(["--stdout-only", script.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "hello\n");
    fs::remove_file(script).unwrap();
    fs::remove_dir(directory).unwrap();
}
