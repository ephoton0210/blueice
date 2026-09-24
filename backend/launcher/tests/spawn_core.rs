// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

//! Integration test for [`blueice_launcher::SpawnedCore`] -- spawns the
//! *real* `blueice-core` binary and drives it over the real internal
//! Unix socket, the same strategy `blueice-mcp-server`'s own
//! `tests/core_process.rs` uses for its equivalent `CoreProcess`.
//! Headless and display-free -- there is no reason this can't run in CI.

use blueice_launcher::{CoreLaunchOptions, SpawnedCore};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

#[test]
fn spawn_connects_to_a_real_core_and_cleans_up_on_drop() {
    let dir = std::env::temp_dir().join(format!(
        "blueice-launcher-test-spawn-core-{}",
        std::process::id()
    ));
    let core = SpawnedCore::spawn(320.0, 200.0, &dir)
        .expect("blueice-core must spawn and accept a connection");
    assert!(core.script_socket_path().is_none());

    let mut stream = core.stream.try_clone().unwrap();
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate {
            url: "about:blank".to_string(),
        },
    )
    .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut stream).unwrap();
    assert_eq!(
        navigated,
        blueice_ipc::ServerMessage::Navigated {
            url: "about:blank".to_string()
        }
    );

    drop(core); // must not panic or hang
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn supervised_child_gets_a_private_script_listener_that_denies_foreign_hello() {
    let dir = std::env::temp_dir().join(format!(
        "blueice-launcher-test-script-core-{}",
        std::process::id()
    ));
    let core = SpawnedCore::spawn_with_options(
        320.0,
        200.0,
        &dir,
        CoreLaunchOptions::default().supervise_out_of_process_bluejs(),
    )
    .expect("launcher must start one core and its supervised child");
    let path = core
        .script_socket_path()
        .expect("supervised core must have a private script listener")
        .to_path_buf();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut stream = loop {
        match UnixStream::connect(&path) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => panic!("script socket never accepted a connection: {error}"),
        }
    };
    blueice_ipc::script::write_script_request(
        &mut stream,
        &blueice_ipc::script::ScriptRequest::Hello {
            protocol_version: blueice_ipc::script::SCRIPT_PROTOCOL_VERSION,
            session_token: "f".repeat(64),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::script::read_script_reply(&mut stream).unwrap(),
        blueice_ipc::script::ScriptReply::Error { .. }
    ));
    assert!(blueice_ipc::script::read_script_reply(&mut stream).is_err());
    drop(core);
    assert!(
        !path.exists(),
        "launcher must remove its private script socket"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
