// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

//! End-to-end coverage for the launcher-owned native debugger endpoint.
//!
//! The test drives the real `blueice-launcher` binary, its real
//! `blueice-core` child, and the private core debugger listener selected by
//! the launcher. It proves the public path stays stable and owner-only across
//! a core cutover, while an already accepted debugger connection is never
//! silently retargeted to the replacement core generation.

use blueice_ipc::debugger::{
    read_debugger_reply, write_debugger_request, DebuggerMetadataCapabilityManifest, DebuggerReply,
    DebuggerRequest, DEBUGGER_PROTOCOL_VERSION,
};
use blueice_launcher::control::{
    read_control_reply, write_control_request, ControlReply, ControlRequest,
};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn unique_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after Unix epoch")
        .as_nanos();
    PathBuf::from("/tmp").join(format!(
        "blueice-launcher-debugger-{label}-{}-{nonce}.sock",
        std::process::id()
    ))
}

fn unique_frame_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "blueice-launcher-debugger-frames-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after Unix epoch")
            .as_nanos()
    ))
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

struct LauncherProcess {
    child: Child,
    rendezvous_socket: PathBuf,
    control_socket: PathBuf,
    debugger_socket: PathBuf,
    frame_dir: PathBuf,
}

impl LauncherProcess {
    fn spawn() -> Self {
        let rendezvous_socket = unique_path("rendezvous");
        let control_socket = unique_path("control");
        let debugger_socket = unique_path("debugger");
        let frame_dir = unique_frame_dir();
        let _ = std::fs::remove_file(&rendezvous_socket);
        let _ = std::fs::remove_file(&control_socket);
        let _ = std::fs::remove_file(&debugger_socket);
        let _ = std::fs::remove_dir_all(&frame_dir);

        let child = Command::new(env!("CARGO_BIN_EXE_blueice-launcher"))
            .args([
                "--socket",
                rendezvous_socket.to_str().unwrap(),
                "--control-socket",
                control_socket.to_str().unwrap(),
                "--debugger-socket",
                debugger_socket.to_str().unwrap(),
                "--width",
                "320",
                "--height",
                "200",
                "--frame-dir",
                frame_dir.to_str().unwrap(),
            ])
            .spawn()
            .expect("blueice-launcher must spawn");
        assert!(
            wait_for(&rendezvous_socket, Duration::from_secs(5)),
            "launcher must create its browser rendezvous endpoint"
        );
        assert!(
            wait_for(&control_socket, Duration::from_secs(5)),
            "launcher must create its control endpoint"
        );
        assert!(
            wait_for(&debugger_socket, Duration::from_secs(5)),
            "the launcher-selected native debugger endpoint must be created"
        );

        Self {
            child,
            rendezvous_socket,
            control_socket,
            debugger_socket,
            frame_dir,
        }
    }

    fn shutdown(&mut self) {
        if let Ok(mut browser) = UnixStream::connect(&self.rendezvous_socket) {
            // Complete the public browser handshake before sending Shutdown.
            // It proves this is an ordinary launcher client rather than an
            // internal-core shortcut and ensures the broker has registered
            // the connection before the terminal request is written.
            if blueice_ipc::client_handshake(&mut browser).is_ok() {
                let _ = blueice_ipc::write_client_message(
                    &mut browser,
                    &blueice_ipc::ClientMessage::Shutdown,
                );
            }
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for LauncherProcess {
    fn drop(&mut self) {
        self.shutdown();
        let _ = std::fs::remove_file(&self.rendezvous_socket);
        let _ = std::fs::remove_file(&self.control_socket);
        let _ = std::fs::remove_file(&self.debugger_socket);
        let _ = std::fs::remove_dir_all(&self.frame_dir);
    }
}

fn open_debugger_session(
    path: &std::path::Path,
) -> (UnixStream, Vec<blueice_ipc::debugger::DebuggerPageRealm>) {
    let mut stream = UnixStream::connect(path).expect("debugger endpoint must accept a peer");
    write_debugger_request(
        &mut stream,
        &DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_bounded_values: false,
            requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
        },
    )
    .unwrap();
    assert_eq!(
        read_debugger_reply(&mut stream).unwrap(),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
        },
        "the fixed core profile must negotiate the native debugger protocol"
    );
    write_debugger_request(&mut stream, &DebuggerRequest::ListPageRealms).unwrap();
    let DebuggerReply::PageRealms(realms) = read_debugger_reply(&mut stream).unwrap() else {
        panic!("the fixed core profile must expose its page realm inventory")
    };
    assert!(
        realms.iter().all(|realm| realm.is_well_formed()),
        "the launcher must relay only core-owned opaque debugger identities"
    );
    (stream, realms)
}

#[test]
fn launcher_owns_a_stable_debugger_endpoint_across_cutover_without_retargeting_clients() {
    let mut launcher = LauncherProcess::spawn();
    let mode = std::fs::metadata(&launcher.debugger_socket)
        .expect("launcher must own debugger endpoint")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "debugger endpoint must be owner-only");

    let (mut v1, _) = open_debugger_session(&launcher.debugger_socket);

    // v2 receives a separate private debugger listener while this existing
    // public connection remains pinned to v1. The caller-selected public
    // endpoint itself remains owner-only and live.
    let mut control = UnixStream::connect(&launcher.control_socket).unwrap();
    write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
    let ControlReply::CutoverDone { .. } = read_control_reply(&mut control).unwrap() else {
        panic!("launcher-owned debugger relay must allow a prepared cutover")
    };
    assert!(launcher.debugger_socket.exists());
    assert_eq!(
        std::fs::metadata(&launcher.debugger_socket)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "cutover must preserve the stable owner-only public debugger endpoint"
    );

    // An accepted v1 debugger connection may reach its dying v1 long enough
    // to fail normally, or the relay/core may close it. It must never become
    // a v2 session: a successful PageRealms reply after cutover would let one
    // stream silently cross generation boundaries. A read deadline keeps a
    // broken relay from turning this safety assertion into a hung test.
    v1.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let old_result = write_debugger_request(&mut v1, &DebuggerRequest::ListPageRealms)
        .and_then(|_| read_debugger_reply(&mut v1));
    assert!(
        !matches!(old_result, Ok(DebuggerReply::PageRealms(_))),
        "a pre-cutover debugger connection must never be retargeted to v2"
    );

    let (_v2, v2_realms) = open_debugger_session(&launcher.debugger_socket);
    assert!(
        v2_realms.iter().all(|realm| realm.is_well_formed()),
        "a post-cutover debugger connection must negotiate the newly selected core"
    );

    launcher.shutdown();
    assert!(
        !launcher.debugger_socket.exists(),
        "launcher shutdown must not leave the selected debugger endpoint stale"
    );
    assert!(
        !launcher.frame_dir.exists(),
        "launcher-owned frame state must be cleaned with the core generation"
    );
}
