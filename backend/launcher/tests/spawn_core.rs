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
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
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

#[test]
fn supervised_child_script_completes_a_synchronous_core_dom_lookup() {
    let gatekeeper_path = std::env::temp_dir().join(format!("bi-dom-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&gatekeeper_path);
    let gatekeeper = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in gatekeeper.incoming() {
            let Ok(mut stream) = incoming else { break };
            let _ = blueice_ai_gatekeeper::handle_one_check(&mut stream);
        }
    });

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/lookup.html", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request).unwrap();
        let body = concat!(
            "<div id='target'>live core node</div>",
            "<script>",
            "if (!blueiceTestHasElementById('target')) throw 'missing live node';",
            "if (blueiceTestHasElementById('absent')) throw 'invented node';",
            "globalThis.domLookupCompleted = true;",
            "</script>",
            "<script>if (!globalThis.domLookupCompleted) throw 'lookup did not complete';</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-launcher-dom-probe-frames-{}",
        std::process::id()
    ));
    let core = SpawnedCore::spawn_with_options(
        320.0,
        200.0,
        &frame_dir,
        CoreLaunchOptions::default()
            .with_gatekeeper_socket(gatekeeper_path.clone())
            .supervise_out_of_process_bluejs_with_dom_lookup_probe_fixture(),
    )
    .expect("launcher must supervise the real core and child");
    let mut stream = core.stream.try_clone().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueJsScriptReports,
    )
    .unwrap();
    let blueice_ipc::ServerMessage::BlueJsScriptReports(reports) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected page script reports");
    };
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|report| matches!(
        report.outcome,
        blueice_ipc::BlueJsScriptExecutionOutcome::Executed
    )));
    server.join().unwrap();
    drop(core);
    let _ = std::fs::remove_file(&gatekeeper_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
}
