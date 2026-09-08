// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Integration test for [`blueice_launcher::SpawnedCore`] -- spawns the
//! *real* `blueice-core` binary and drives it over the real internal
//! Unix socket, the same strategy `blueice-mcp-server`'s own
//! `tests/core_process.rs` uses for its equivalent `CoreProcess`.
//! Headless and display-free -- there is no reason this can't run in CI.

use blueice_launcher::SpawnedCore;

#[test]
fn spawn_connects_to_a_real_core_and_cleans_up_on_drop() {
    let dir = std::env::temp_dir().join(format!("blueice-launcher-test-spawn-core-{}", std::process::id()));
    let core = SpawnedCore::spawn(320.0, 200.0, &dir).expect("blueice-core must spawn and accept a connection");

    let mut stream = core.stream.try_clone().unwrap();
    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Navigate { url: "about:blank".to_string() }).unwrap();
    let navigated = blueice_ipc::read_server_message(&mut stream).unwrap();
    assert_eq!(navigated, blueice_ipc::ServerMessage::Navigated { url: "about:blank".to_string() });

    drop(core); // must not panic or hang
    let _ = std::fs::remove_dir_all(&dir);
}
