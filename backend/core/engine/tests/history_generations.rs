// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

use blueice_ipc::{
    client_handshake, read_server_message, write_client_message, ClientMessage, ServerMessage,
};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn snapshot_history_frames_keep_increasing_across_back_and_forward() {
    let directory = std::env::temp_dir().join(format!("bi-history-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let socket = directory.join("core.sock");
    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .arg("--socket")
        .arg(&socket)
        .arg("--frame-dir")
        .arg(directory.join("frames"))
        .args(["--width", "320", "--height", "200", "--history-snapshots"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let result = std::panic::catch_unwind(|| {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut wire = loop {
            match UnixStream::connect(&socket) {
                Ok(wire) => break wire,
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => panic!("core did not accept connections: {error}"),
            }
        };
        wire.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        client_handshake(&mut wire).unwrap();
        let mut previous_generation = 0;
        for (request, expected_url) in [
            (
                ClientMessage::Navigate {
                    url: "about:credits".into(),
                },
                "about:credits",
            ),
            (
                ClientMessage::Navigate {
                    url: "about:blank".into(),
                },
                "about:blank",
            ),
            (ClientMessage::GoBack, "about:credits"),
            (ClientMessage::GoForward, "about:blank"),
            (ClientMessage::GoBack, "about:credits"),
        ] {
            write_client_message(&mut wire, &request).unwrap();
            assert_eq!(
                read_server_message(&mut wire).unwrap(),
                ServerMessage::Navigated {
                    url: expected_url.into(),
                }
            );
            let ServerMessage::FrameReady { generation, .. } =
                read_server_message(&mut wire).unwrap()
            else {
                panic!("expected a frame after {request:?}");
            };
            assert!(generation > previous_generation,
                "{request:?} reused or decreased frame generation: {previous_generation} -> {generation}");
            previous_generation = generation;
            write_client_message(&mut wire, &ClientMessage::GetRepresentation).unwrap();
            let ServerMessage::Representation(snapshot) = read_server_message(&mut wire).unwrap()
            else {
                panic!("expected the restored document's representation");
            };
            assert_eq!(snapshot.generation, generation);
        }
        write_client_message(&mut wire, &ClientMessage::Shutdown).unwrap();
    });
    let _ = child.kill();
    let _ = child.wait();
    std::fs::remove_dir_all(directory).unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
