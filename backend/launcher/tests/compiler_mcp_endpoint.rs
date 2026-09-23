// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

//! End-to-end coverage for the launcher-owned compiler MCP endpoint.  This
//! drives the real `blueice-launcher` binary, its real `blueice-core` child,
//! and the core's separate v3 compiler listener. MCP's paired adapter is
//! covered from the MCP crate; this boundary proves the public launcher CLI
//! is the one that creates the fixed closed profile and safely owns its
//! caller-selected endpoint through shutdown and cutover refusal.

use blueice_ipc::compiler::{
    read_compiler_reply, write_compiler_request, CompilerErrorCode, CompilerProject, CompilerReply,
    CompilerRequest, CompilerStaticMetadataKind, COMPILER_PROTOCOL_VERSION,
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
        "blueice-launcher-compiler-mcp-{label}-{}-{nonce}.sock",
        std::process::id()
    ))
}

fn unique_frame_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "blueice-launcher-compiler-mcp-frames-{}-{}",
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
    compiler_socket: PathBuf,
    frame_dir: PathBuf,
}

impl LauncherProcess {
    fn spawn() -> Self {
        let rendezvous_socket = unique_path("rendezvous");
        let control_socket = unique_path("control");
        let compiler_socket = unique_path("compiler");
        let frame_dir = unique_frame_dir();
        let _ = std::fs::remove_file(&rendezvous_socket);
        let _ = std::fs::remove_file(&control_socket);
        let _ = std::fs::remove_file(&compiler_socket);
        let _ = std::fs::remove_dir_all(&frame_dir);

        let child = Command::new(env!("CARGO_BIN_EXE_blueice-launcher"))
            .args([
                "--socket",
                rendezvous_socket.to_str().unwrap(),
                "--control-socket",
                control_socket.to_str().unwrap(),
                "--compiler-mcp-socket",
                compiler_socket.to_str().unwrap(),
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
            wait_for(&compiler_socket, Duration::from_secs(5)),
            "the launcher-selected core compiler endpoint must be created"
        );

        Self {
            child,
            rendezvous_socket,
            control_socket,
            compiler_socket,
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
        let _ = std::fs::remove_file(&self.compiler_socket);
        let _ = std::fs::remove_dir_all(&self.frame_dir);
    }
}

fn open_fixed_core_profile(
    path: &std::path::Path,
) -> (UnixStream, blueice_ipc::compiler::CompilerCheck) {
    let mut stream = UnixStream::connect(path).expect("compiler endpoint must accept a peer");
    write_compiler_request(
        &mut stream,
        &CompilerRequest::Hello {
            protocol_version: COMPILER_PROTOCOL_VERSION,
        },
    )
    .unwrap();
    let CompilerReply::HelloAck {
        protocol_version,
        session_attestation,
        capability_manifest,
    } = read_compiler_reply(&mut stream).unwrap()
    else {
        panic!("fixed core profile must mint an attested compiler stream")
    };
    assert_eq!(protocol_version, COMPILER_PROTOCOL_VERSION);
    assert!(session_attestation.is_well_formed());
    assert!(capability_manifest.is_well_formed());
    write_compiler_request(
        &mut stream,
        &CompilerRequest::Check {
            project: CompilerProject { id: 1 },
        },
    )
    .unwrap();
    let CompilerReply::Check(check) = read_compiler_reply(&mut stream).unwrap() else {
        panic!("the launcher may expose only its fixed closed core profile")
    };
    assert!(
        !check.has_errors,
        "fixed core profile must check successfully"
    );
    assert!(check.static_metadata.is_some());
    (stream, check)
}

#[test]
fn launcher_owns_a_stable_fixed_compiler_endpoint_across_cutover_without_retargeting_clients() {
    let mut launcher = LauncherProcess::spawn();
    let mode = std::fs::metadata(&launcher.compiler_socket)
        .expect("core must own compiler endpoint")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "compiler endpoint must be owner-only");
    let (mut v1, v1_check) = open_fixed_core_profile(&launcher.compiler_socket);
    write_compiler_request(
        &mut v1,
        &CompilerRequest::ListStaticMetadata {
            generation: v1_check.generation,
            kind: CompilerStaticMetadataKind::Symbols,
            cursor: None,
            limit: Some(1),
        },
    )
    .unwrap();
    let CompilerReply::StaticMetadataPage(v1_symbols) = read_compiler_reply(&mut v1).unwrap()
    else {
        panic!("v1 fixed profile must expose bounded symbol inventory")
    };
    let v1_cursor = v1_symbols
        .next_cursor
        .expect("fixture must require a continuation cursor");

    // v2 gets its own private listener while v1's public connection remains
    // pinned to v1.  The public endpoint itself stays owner-only and live.
    let mut control = UnixStream::connect(&launcher.control_socket).unwrap();
    write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
    let ControlReply::CutoverDone { .. } = read_control_reply(&mut control).unwrap() else {
        panic!("launcher-owned compiler relay must allow a prepared cutover")
    };
    assert!(launcher.compiler_socket.exists());
    assert_eq!(
        std::fs::metadata(&launcher.compiler_socket)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "cutover must preserve the stable owner-only public endpoint"
    );

    // A stream accepted before cutover must not silently become a v2 stream.
    // It either reaches its dying v1 long enough to fail normally, or the
    // relay/core close it.  A successful v2 metadata page would prove an
    // unsafe cross-generation retarget.
    let old_result = write_compiler_request(
        &mut v1,
        &CompilerRequest::ListStaticMetadata {
            generation: v1_check.generation,
            kind: CompilerStaticMetadataKind::Symbols,
            cursor: Some(v1_cursor),
            limit: Some(1),
        },
    )
    .and_then(|_| read_compiler_reply(&mut v1));
    assert!(
        !matches!(old_result, Ok(CompilerReply::StaticMetadataPage(_))),
        "a pre-cutover compiler connection must never be retargeted to v2"
    );

    let (mut v2, v2_check) = open_fixed_core_profile(&launcher.compiler_socket);
    // A new connection after cutover is routed to the newly sealed catalog.
    // Even if its opaque number collides with v1's first cursor, v1's cursor
    // state is neither copied nor usable in v2.
    write_compiler_request(
        &mut v2,
        &CompilerRequest::ListStaticMetadata {
            generation: v2_check.generation,
            kind: CompilerStaticMetadataKind::Symbols,
            cursor: Some(v1_cursor),
            limit: Some(1),
        },
    )
    .unwrap();
    assert!(matches!(
        read_compiler_reply(&mut v2).unwrap(),
        CompilerReply::Error {
            code: CompilerErrorCode::InvalidMetadataCursor,
            ..
        }
    ));

    launcher.shutdown();
    assert!(
        !launcher.compiler_socket.exists(),
        "forced launcher/core shutdown must not leave the selected compiler endpoint stale"
    );
    assert!(
        !launcher.frame_dir.exists(),
        "launcher-owned frame state must be cleaned with the core generation"
    );
}
