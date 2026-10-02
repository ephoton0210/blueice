// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn an_unrecognized_argument_exits_with_failure_and_creates_no_socket() {
    let _guard = broker_test_guard();
    let rendezvous_socket = unique_path("bad-args-rendezvous.sock");
    let _ = std::fs::remove_file(&rendezvous_socket);

    let output = Command::new(env!("CARGO_BIN_EXE_blueice-launcher"))
        .args(["--socket", rendezvous_socket.to_str().unwrap(), "--bogus"])
        .output()
        .expect("failed to run blueice-launcher");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--bogus"));
    assert!(
        !rendezvous_socket.exists(),
        "a launcher that failed argument parsing must never spawn core or bind a socket"
    );
}
