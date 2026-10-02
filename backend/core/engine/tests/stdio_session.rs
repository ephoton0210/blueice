// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_engine::session::{message_pipe::MessagePipe, ReadTimeout};
use blueice_ipc::{ClientMessage, ServerMessage};
use std::io::{self, Cursor, Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn a_partial_pipe_frame_survives_session_polling() {
    let (sender, receiver) = std::sync::mpsc::channel();
    struct Chunks(std::sync::mpsc::Receiver<Vec<u8>>, Cursor<Vec<u8>>);
    impl Read for Chunks {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.1.position() == self.1.get_ref().len() as u64 {
                self.1 = Cursor::new(self.0.recv().unwrap_or_default());
            }
            self.1.read(out)
        }
    }
    let mut bytes = Vec::new();
    blueice_ipc::write_client_message(
        &mut bytes,
        &ClientMessage::Hello {
            protocol_version: 2,
        },
    )
    .unwrap();
    let mut pipe = MessagePipe::new(Chunks(receiver, Cursor::new(Vec::new())), Vec::<u8>::new());
    pipe.set_read_timeout(Some(Duration::from_millis(5)))
        .unwrap();
    sender.send(bytes[..2].to_vec()).unwrap();
    let error = blueice_ipc::read_client_message(&mut pipe).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    sender.send(bytes[2..].to_vec()).unwrap();
    pipe.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    assert_eq!(
        blueice_ipc::read_client_message(&mut pipe).unwrap(),
        ClientMessage::Hello {
            protocol_version: 2
        }
    );
}

#[test]
fn oversized_control_frames_are_rejected_before_allocation() {
    let bytes = ((blueice_ipc::MAX_FRAME_BYTES + 1) as u32).to_le_bytes();
    let mut pipe = MessagePipe::new(Cursor::new(bytes), Vec::<u8>::new());
    assert_eq!(
        blueice_ipc::read_client_message(&mut pipe)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn actual_stdio_core_renders_and_closes_its_owned_frame_directory() {
    let root = std::env::temp_dir().join(format!("blueice-stdio-public-{}", std::process::id()));
    assert!(!root.exists());
    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--stdio",
            "--width",
            "320",
            "--height",
            "200",
            "--frame-dir",
        ])
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    blueice_ipc::write_client_message(
        &mut input,
        &ClientMessage::Hello {
            protocol_version: blueice_ipc::PROTOCOL_VERSION,
        },
    )
    .unwrap();
    input.flush().unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut output).unwrap(),
        ServerMessage::Hello {
            protocol_version: 2
        }
    ));
    blueice_ipc::write_client_message_with_ids(
        &mut input,
        Some(1),
        Some(42),
        &ClientMessage::Navigate {
            url: "about:credits".into(),
        },
    )
    .unwrap();
    input.flush().unwrap();
    let mut rendered = false;
    let mut navigated = false;
    while !(rendered && navigated) {
        let (tab, request, message) =
            blueice_ipc::read_server_message_with_ids(&mut output).unwrap();
        assert_eq!(tab, Some(1));
        assert_eq!(request, Some(42));
        match message {
            ServerMessage::FrameReady {
                shm_path,
                width,
                height,
                ..
            } => {
                assert_eq!((width, height), (320, 200));
                let pixels = blueice_ipc::shm::map_frame(std::path::Path::new(&shm_path)).unwrap();
                assert_eq!(pixels.len(), 320 * 200 * 4);
                assert!(pixels.chunks_exact(4).any(|pixel| pixel[0] != 255));
                rendered = true;
            }
            ServerMessage::Navigated { url } => {
                assert_eq!(url, "about:credits");
                navigated = true;
            }
            other => panic!("unexpected reply: {other:?}"),
        }
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    blueice_ipc::write_client_message_with_ids(
        &mut input,
        Some(1),
        Some(43),
        &ClientMessage::Navigate {
            url: format!("http://{}/", listener.local_addr().unwrap()),
        },
    )
    .unwrap();
    let (tab, request, message) = blueice_ipc::read_server_message_with_ids(&mut output).unwrap();
    assert_eq!((tab, request), (Some(1), Some(43)));
    assert!(matches!(message, ServerMessage::GatekeeperBlocked { .. }));
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    blueice_ipc::write_client_message(&mut input, &ClientMessage::Shutdown).unwrap();
    input.flush().unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!root.exists());
}

#[test]
fn stdio_refuses_to_reuse_or_delete_an_existing_directory() {
    let root = std::env::temp_dir().join(format!("blueice-stdio-existing-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("keep.txt"), "owned by another session").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args(["--stdio", "--frame-dir"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(
        std::fs::read_to_string(root.join("keep.txt")).unwrap(),
        "owned by another session"
    );
    std::fs::remove_dir_all(root).unwrap();
}
