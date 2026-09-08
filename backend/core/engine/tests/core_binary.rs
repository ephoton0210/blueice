// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Exercises the actual compiled `blueice-core` binary as a real
//! subprocess -- `main`'s own argument parsing, socket binding,
//! accept, and shutdown/cleanup wiring, none of which
//! `src/bin/blueice-core.rs`'s unit tests touch (those cover
//! `parse_args` in isolation; `blueice_engine::session`'s own tests
//! cover the message loop over an in-process pipe). This is the
//! "e2e test through the real public interface" the project's
//! Definition of Done asks for applied to a binary rather than a
//! library: `Command::new(env!("CARGO_BIN_EXE_blueice-core"))`, not a
//! function call, is the real public interface here. Manually running
//! this exact binary under a real windowed frontend (WSLg) additionally
//! confirmed the end-to-end pixels look right; this test covers the
//! process-lifecycle contract repeatably in CI, which a manual run
//! can't.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn unique_socket_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("blueice-core-binary-test-{label}-{}.sock", std::process::id()))
}

/// A gated `Navigate`/`OpenTab{url}` sent to the real subprocess needs
/// *some* `ai-gatekeeper` behind the `--gatekeeper-socket` path it's
/// given -- a genuinely unreachable one fails closed
/// (`phase-7-local-ai/PLAN.md`'s "Wiring design"), which would turn
/// this file's pre-existing "navigation always succeeds" assertions
/// false. Spins up a real listener running the actual minimal-slice
/// stub logic (`blueice_ai_gatekeeper::handle_one_check`, always
/// clears) bound to a fresh path unique to this call, mirroring
/// `blueice_engine::session`'s own test-module helper of the same
/// name/purpose.
fn clearing_gatekeeper(label: &str) -> PathBuf {
    let path = unique_socket_path(label);
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let _ = blueice_ai_gatekeeper::handle_one_check(&mut stream);
        }
    });
    path
}

fn wait_for(path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn missing_socket_flag_exits_with_failure_and_no_socket_is_created() {
    let output = Command::new(env!("CARGO_BIN_EXE_blueice-core")).output().expect("failed to run blueice-core");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--socket"));
}

#[test]
fn real_subprocess_serves_navigate_resize_and_shutdown_over_a_real_socket() {
    let socket_path = unique_socket_path("full-session");
    let frame_dir = std::env::temp_dir().join(format!("blueice-core-binary-test-frames-{}", std::process::id()));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("fs-gk"); // short: Unix socket paths are capped at ~100 bytes total

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = "<p>from a real subprocess</p>";
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--width",
            "300",
            "--height",
            "150",
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)), "blueice-core never created its socket");
    let mut stream = UnixStream::connect(&socket_path).expect("failed to connect to the real subprocess");
    blueice_ipc::client_handshake(&mut stream).expect("the real subprocess must complete the protocol_version handshake");

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Navigate { url: url.clone() }).unwrap();
    let navigated = blueice_ipc::read_server_message(&mut stream).unwrap();
    assert_eq!(navigated, blueice_ipc::ServerMessage::Navigated { url });

    let frame = blueice_ipc::read_server_message(&mut stream).unwrap();
    let (shm_path, width, height) = match frame {
        blueice_ipc::ServerMessage::FrameReady { shm_path, width, height, generation: 1 } => (shm_path, width, height),
        other => panic!("expected the first FrameReady, got {other:?}"),
    };
    assert_eq!((width, height), (300, 150));
    let mapped = blueice_ipc::shm::map_frame(std::path::Path::new(&shm_path)).expect("the real subprocess's frame file must be mappable");
    assert_eq!(mapped.len() as u32, width * height * 4, "RGBA8 frame bytes must match the requested viewport size");

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Resize { width: 100, height: 80 }).unwrap();
    let resized = blueice_ipc::read_server_message(&mut stream).unwrap();
    assert!(matches!(resized, blueice_ipc::ServerMessage::FrameReady { width: 100, height: 80, generation: 2, .. }));

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    let status = child.wait().expect("failed to wait for blueice-core to exit");
    assert!(status.success(), "blueice-core must exit cleanly after Shutdown");

    assert!(!socket_path.exists(), "blueice-core must remove its own socket file on exit");
    assert!(!frame_dir.exists(), "blueice-core must remove its own frame directory on exit");
}

#[test]
fn real_subprocess_serves_two_independently_addressed_tabs_without_cross_contamination() {
    // `phase-16-multi-tab-and-tab-groups/PLAN.md`'s minimal-first-slice
    // proof, one layer up from `session.rs`'s own in-process tests: the
    // real compiled `blueice-core` binary, driven over a real socket,
    // must keep two tabs' navigation and representation fully
    // independent.
    let socket_path = unique_socket_path("multi-tab");
    let frame_dir = std::env::temp_dir().join(format!("blueice-core-binary-test-frames-multi-tab-{}", std::process::id()));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("mt-gk"); // short: Unix socket paths are capped at ~100 bytes total

    let listener_one = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_one = listener_one.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_one.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = "<p>first tab content</p>";
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
    });
    let listener_two = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_two = listener_two.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_two.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = "<p>second tab content</p>";
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--width",
            "300",
            "--height",
            "150",
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)), "blueice-core never created its socket");
    let mut stream = UnixStream::connect(&socket_path).expect("failed to connect to the real subprocess");
    blueice_ipc::client_handshake(&mut stream).expect("the real subprocess must complete the protocol_version handshake");

    // Navigate the default (first) tab.
    let url_one = format!("http://{addr_one}");
    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Navigate { url: url_one.clone() }).unwrap();
    assert_eq!(blueice_ipc::read_server_message(&mut stream).unwrap(), blueice_ipc::ServerMessage::Navigated { url: url_one });
    assert!(matches!(blueice_ipc::read_server_message(&mut stream).unwrap(), blueice_ipc::ServerMessage::FrameReady { .. }));

    // Open a second tab navigated straight to a different URL.
    let url_two = format!("http://{addr_two}");
    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::OpenTab { url: Some(url_two.clone()) }).unwrap();
    let (tab_two, tab_two_url) = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::TabOpened { tab_id, url } => (tab_id, url),
        other => panic!("expected TabOpened, got {other:?}"),
    };
    assert_eq!(tab_two_url, Some(url_two));
    assert!(matches!(blueice_ipc::read_server_message(&mut stream).unwrap(), blueice_ipc::ServerMessage::FrameReady { .. }));

    // The default tab's own representation must still show only its
    // own content -- opening and navigating a second tab must not have
    // touched it.
    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::GetRepresentation).unwrap();
    let default_tab_snapshot = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::Representation(snapshot) => snapshot,
        other => panic!("expected Representation, got {other:?}"),
    };
    assert!(default_tab_snapshot.nodes.iter().any(|n| n.name.as_deref() == Some("first tab content")));
    assert!(!default_tab_snapshot.nodes.iter().any(|n| n.name.as_deref() == Some("second tab content")));

    // The second tab's representation, addressed explicitly, must show
    // only *its* content.
    blueice_ipc::write_client_message_with_ids(&mut stream, Some(tab_two), None, &blueice_ipc::ClientMessage::GetRepresentation).unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut stream).unwrap();
    assert_eq!(reply_tab, Some(tab_two));
    let tab_two_snapshot = match reply {
        blueice_ipc::ServerMessage::Representation(snapshot) => snapshot,
        other => panic!("expected Representation, got {other:?}"),
    };
    assert_eq!(tab_two_snapshot.tab_id, tab_two);
    assert!(tab_two_snapshot.nodes.iter().any(|n| n.name.as_deref() == Some("second tab content")));
    assert!(!tab_two_snapshot.nodes.iter().any(|n| n.name.as_deref() == Some("first tab content")));

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    let status = child.wait().expect("failed to wait for blueice-core to exit");
    assert!(status.success(), "blueice-core must exit cleanly after Shutdown");

    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn a_client_disconnecting_without_shutdown_still_lets_the_subprocess_exit_cleanly() {
    let socket_path = unique_socket_path("disconnect");
    let frame_dir = std::env::temp_dir().join(format!("blueice-core-binary-test-frames-disconnect-{}", std::process::id()));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args(["--socket", socket_path.to_str().unwrap(), "--frame-dir", frame_dir.to_str().unwrap()])
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let stream = UnixStream::connect(&socket_path).unwrap();
    drop(stream); // disconnect without ever sending Shutdown

    let status = child.wait_timeout_or_kill();
    assert!(status.success(), "blueice-core must exit cleanly when its one client just disconnects");
}

trait WaitTimeoutOrKill {
    fn wait_timeout_or_kill(&mut self) -> std::process::ExitStatus;
}

impl WaitTimeoutOrKill for std::process::Child {
    fn wait_timeout_or_kill(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.try_wait().expect("failed to poll child status") {
                return status;
            }
            if Instant::now() >= deadline {
                let _ = self.kill();
                panic!("blueice-core did not exit within the timeout");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}
