// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

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

fn spawn_private_bluejs_host(label: &str) -> (PathBuf, String, thread::JoinHandle<()>) {
    // The default macOS temporary directory can exceed the Unix-domain socket
    // pathname limit once this test's descriptive filename is appended.
    let path = PathBuf::from("/private/tmp")
        .join(format!("blueice-oop-{label}-{}.sock", std::process::id()));
    let token = "0123456789abcdef0123456789abcdef".to_string();
    let listener = blueice_launcher::bluejs_host::bind_bluejs_host_socket(&path)
        .expect("test child host must bind its private socket");
    let child_token = token.clone();
    let child = thread::spawn(move || {
        let mut host = blueice_launcher::bluejs_host::BlueJsChildHost::default();
        blueice_launcher::bluejs_host::serve_bluejs_host_listener(listener, child_token, &mut host)
            .expect("test child host must serve its private protocol");
    });
    (path, token, child)
}

fn shutdown_private_bluejs_host(path: &std::path::Path, token: &str) {
    let mut stream =
        UnixStream::connect(path).expect("must connect to test child host for shutdown");
    blueice_ipc::page_host::write_page_host_request(
        &mut stream,
        &blueice_ipc::page_host::PageHostRequest::Hello {
            protocol_version: blueice_ipc::page_host::PAGE_HOST_PROTOCOL_VERSION,
            session_token: token.to_string(),
        },
    )
    .expect("must send child-host shutdown handshake");
    assert!(matches!(
        blueice_ipc::page_host::read_page_host_reply(&mut stream)
            .expect("child host must acknowledge shutdown handshake"),
        blueice_ipc::page_host::PageHostReply::HelloAck { .. }
    ));
    blueice_ipc::page_host::write_page_host_request(
        &mut stream,
        &blueice_ipc::page_host::PageHostRequest::Shutdown,
    )
    .expect("must request child-host shutdown");
    assert_eq!(
        blueice_ipc::page_host::read_page_host_reply(&mut stream)
            .expect("child host must acknowledge shutdown"),
        blueice_ipc::page_host::PageHostReply::ShutdownAck
    );
}

fn unique_socket_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "blueice-core-binary-test-{label}-{}.sock",
        std::process::id()
    ))
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
    let output = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .output()
        .expect("failed to run blueice-core");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--socket"));
}

#[test]
fn real_subprocess_routes_a_handshaken_script_connection_through_the_core_session() {
    let socket_path = unique_socket_path("script-core");
    let script_socket_path = unique_socket_path("script-host");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-script-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&script_socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--script-socket",
            script_socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(
        wait_for(&socket_path, Duration::from_secs(5)),
        "blueice-core never created its frontend socket"
    );
    assert!(
        wait_for(&script_socket_path, Duration::from_secs(5)),
        "blueice-core never created its script socket"
    );
    let mut frontend = UnixStream::connect(&socket_path)
        .expect("failed to connect to the real core frontend socket");
    blueice_ipc::client_handshake(&mut frontend)
        .expect("the real subprocess must complete the frontend handshake");

    let mut invalid = UnixStream::connect(&script_socket_path)
        .expect("failed to connect an unhandshaken script client");
    blueice_ipc::script::write_script_request(
        &mut invalid,
        &blueice_ipc::script::ScriptRequest::CreateTextNode {
            tab_id: 1,
            data: "must not run".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::script::read_script_reply(&mut invalid).unwrap(),
        blueice_ipc::script::ScriptReply::Error { .. }
    ));
    drop(invalid);

    let mut script =
        UnixStream::connect(&script_socket_path).expect("failed to connect the real script host");
    blueice_ipc::script::write_script_request(
        &mut script,
        &blueice_ipc::script::ScriptRequest::Hello,
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::script::read_script_reply(&mut script).unwrap(),
        blueice_ipc::script::ScriptReply::HelloAck
    );
    blueice_ipc::script::write_script_request(
        &mut script,
        &blueice_ipc::script::ScriptRequest::CreateTextNode {
            tab_id: 1,
            data: "from script socket".to_string(),
        },
    )
    .unwrap();
    let node = match blueice_ipc::script::read_script_reply(&mut script).unwrap() {
        blueice_ipc::script::ScriptReply::NodeCreated { node } => node,
        reply => panic!("expected a created script node, got {reply:?}"),
    };
    blueice_ipc::script::write_script_request(
        &mut script,
        &blueice_ipc::script::ScriptRequest::GetTextContent { tab_id: 1, node },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::script::read_script_reply(&mut script).unwrap(),
        blueice_ipc::script::ScriptReply::Text {
            value: "from script socket".to_string(),
        }
    );

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    let status = child
        .wait()
        .expect("failed to wait for blueice-core to exit");
    assert!(
        status.success(),
        "blueice-core must exit cleanly after Shutdown"
    );
    assert!(
        !script_socket_path.exists(),
        "blueice-core must remove its script socket on exit"
    );
    assert!(
        !frame_dir.exists(),
        "blueice-core must remove its script frame directory on exit"
    );
}

#[test]
fn real_subprocess_routes_exact_debugger_locations_through_the_live_core_session() {
    // The debugger protocol must remain separate from both frontend and DOM
    // script IPC, but it still has to validate a target against the session's
    // actual document lifecycle. The narrow program-location and exact
    // breakpoint-configuration operations are installed only for the explicit
    // in-process JavaScript host; breakpoint interruption and every
    // VM-inspection operation remain deliberately planned.
    let socket_path = unique_socket_path("debugger-core");
    let debugger_socket_path = unique_socket_path("debugger-host");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-debugger-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&debugger_socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("dbg-gk");

    let listener_one = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_one = listener_one.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_one.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>first debugger document</main>",
            "<script>const firstDebuggerLocation = 42;</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    let listener_two = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_two = listener_two.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_two.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>replacement debugger document</main>",
            "<script>const replacementDebuggerLocation = 43;</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--debugger-socket",
            debugger_socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluejs",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    assert!(wait_for(&debugger_socket_path, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&socket_path).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{addr_one}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { generation: 1, .. }
    ));

    let mut invalid_debugger = UnixStream::connect(&debugger_socket_path).unwrap();
    blueice_ipc::debugger::write_debugger_request(
        &mut invalid_debugger,
        &blueice_ipc::debugger::DebuggerRequest::DescribeCapabilities {
            realm: blueice_ipc::debugger::DebuggerPageRealm {
                browser_context_id: blueice_engine::debugger::DEFAULT_BROWSER_CONTEXT_ID,
                tab_id: 1,
                realm_generation: 1,
            },
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::debugger::read_debugger_reply(&mut invalid_debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::ProtocolVersion,
            ..
        }
    ));
    drop(invalid_debugger);

    let mut debugger = UnixStream::connect(&debugger_socket_path).unwrap();
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::Hello {
            protocol_version: blueice_ipc::debugger::DEBUGGER_PROTOCOL_VERSION,
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::HelloAck {
            protocol_version: blueice_ipc::debugger::DEBUGGER_PROTOCOL_VERSION,
        }
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListPageRealms,
    )
    .unwrap();
    let first_realm = match blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap() {
        blueice_ipc::debugger::DebuggerReply::PageRealms(realms) => {
            assert_eq!(realms.len(), 1);
            realms[0]
        }
        other => panic!("expected debugger page realms, got {other:?}"),
    };
    assert_eq!(
        first_realm,
        blueice_ipc::debugger::DebuggerPageRealm {
            browser_context_id: blueice_engine::debugger::DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: 1,
            realm_generation: 1,
        }
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::DescribeCapabilities { realm: first_realm },
    )
    .unwrap();
    let first_capabilities =
        match blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap() {
            blueice_ipc::debugger::DebuggerReply::Capabilities(capabilities) => capabilities,
            other => panic!("expected debugger capabilities, got {other:?}"),
        };
    assert_eq!(first_capabilities.realm, first_realm);
    assert!(first_capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::ProgramLocations
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(first_capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::BreakpointConfiguration
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
            && report.detail.contains("does not interrupt execution")
    }));
    assert!(first_capabilities
        .reports
        .iter()
        .filter(|report| {
            !matches!(
                report.capability,
                blueice_ipc::debugger::DebuggerCapability::ProgramLocations
                    | blueice_ipc::debugger::DebuggerCapability::BreakpointConfiguration
            )
        })
        .all(|report| { report.state == blueice_ipc::debugger::DebuggerCapabilityState::Planned }));

    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListPrograms { realm: first_realm },
    )
    .unwrap();
    let first_program = match blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap() {
        blueice_ipc::debugger::DebuggerReply::Programs(programs) => {
            assert_eq!(programs.len(), 1);
            programs[0]
        }
        other => panic!("expected opaque debugger programs, got {other:?}"),
    };
    assert_eq!(first_program.realm, first_realm);
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListSafePoints {
            program: first_program,
        },
    )
    .unwrap();
    let first_safe_point = match blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap()
    {
        blueice_ipc::debugger::DebuggerReply::SafePoints(safe_points) => *safe_points
            .first()
            .expect("the compiled page program has a safe point"),
        other => panic!("expected verified debugger safe points, got {other:?}"),
    };
    assert_eq!(first_safe_point.program, first_program);
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ValidateSafePoint {
            safe_point: first_safe_point,
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::SafePointValidated {
            safe_point: first_safe_point,
        }
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::SetBreakpoint {
            safe_point: first_safe_point,
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::BreakpointSet {
            safe_point: first_safe_point,
        }
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListBreakpoints { realm: first_realm },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::Breakpoints(vec![first_safe_point])
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ClearBreakpoint {
            safe_point: first_safe_point,
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::BreakpointCleared {
            safe_point: first_safe_point,
            was_present: true,
        }
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ValidateSafePoint {
            safe_point: blueice_ipc::debugger::DebuggerSafePoint {
                bytecode_offset: u32::MAX,
                ..first_safe_point
            },
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::InvalidSafePoint,
            ..
        }
    ));

    // Keep one configuration record until navigation so the next request
    // proves it cannot survive the document-generation transition.
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::SetBreakpoint {
            safe_point: first_safe_point,
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::BreakpointSet { .. }
    ));

    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{addr_two}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { generation: 2, .. }
    ));

    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListPageRealms,
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::PageRealms(vec![
            blueice_ipc::debugger::DebuggerPageRealm {
                realm_generation: 2,
                ..first_realm
            },
        ])
    );

    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListSafePoints {
            program: first_program,
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListBreakpoints { realm: first_realm },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::StaleRealm,
            ..
        }
    ));

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!debugger_socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_serves_navigate_resize_and_shutdown_over_a_real_socket() {
    let socket_path = unique_socket_path("full-session");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-frames-{}",
        std::process::id()
    ));
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
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
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

    assert!(
        wait_for(&socket_path, Duration::from_secs(5)),
        "blueice-core never created its socket"
    );
    let mut stream =
        UnixStream::connect(&socket_path).expect("failed to connect to the real subprocess");
    blueice_ipc::client_handshake(&mut stream)
        .expect("the real subprocess must complete the protocol_version handshake");

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut stream).unwrap();
    assert_eq!(navigated, blueice_ipc::ServerMessage::Navigated { url });

    let frame = blueice_ipc::read_server_message(&mut stream).unwrap();
    let (shm_path, width, height) = match frame {
        blueice_ipc::ServerMessage::FrameReady {
            shm_path,
            width,
            height,
            generation: 1,
        } => (shm_path, width, height),
        other => panic!("expected the first FrameReady, got {other:?}"),
    };
    assert_eq!((width, height), (300, 150));
    let mapped = blueice_ipc::shm::map_frame(std::path::Path::new(&shm_path))
        .expect("the real subprocess's frame file must be mappable");
    assert_eq!(
        mapped.len() as u32,
        width * height * 4,
        "RGBA8 frame bytes must match the requested viewport size"
    );

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Resize {
            width: 100,
            height: 80,
        },
    )
    .unwrap();
    let resized = blueice_ipc::read_server_message(&mut stream).unwrap();
    assert!(matches!(
        resized,
        blueice_ipc::ServerMessage::FrameReady {
            width: 100,
            height: 80,
            generation: 2,
            ..
        }
    ));

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    let status = child
        .wait()
        .expect("failed to wait for blueice-core to exit");
    assert!(
        status.success(),
        "blueice-core must exit cleanly after Shutdown"
    );

    assert!(
        !socket_path.exists(),
        "blueice-core must remove its own socket file on exit"
    );
    assert!(
        !frame_dir.exists(),
        "blueice-core must remove its own frame directory on exit"
    );
}

#[test]
fn real_subprocess_executes_an_opted_in_inline_bluets_profile_and_reports_source_free_outcomes() {
    // This proves the process seam, rather than just the in-process runner:
    // parsed classic/module declarations travel through real HTTP navigation,
    // the core-owned profile executes them before the navigation reply, and
    // the frontend can observe only bounded outcome metadata.
    let socket_path = unique_socket_path("inline-bluets");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-inline-bluets-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("ib-gk");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>inline BlueTS process proof</main>",
            "<script type=\"application/x-blueice-typescript\">",
            "blueiceDocumentText();",
            "</script>",
            "<script type=\"application/x-blueice-typescript-module\">",
            "blueiceDocumentText();",
            "</script>",
            "<script type=\"application/x-blueice-typescript\">",
            "blueiceDocumentText(1);",
            "</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluets-profile",
            "core-script-document-text-v1",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(
        wait_for(&socket_path, Duration::from_secs(5)),
        "blueice-core never created its socket"
    );
    let mut stream =
        UnixStream::connect(&socket_path).expect("failed to connect to the real subprocess");
    blueice_ipc::client_handshake(&mut stream)
        .expect("the real subprocess must complete the protocol_version handshake");

    let url = format!("http://{addr}");
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
        blueice_ipc::ServerMessage::FrameReady { generation: 1, .. }
    ));

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    let reports = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::BlueTsScriptReports(reports) => reports,
        other => panic!("expected BlueTsScriptReports, got {other:?}"),
    };
    assert_eq!(
        reports,
        vec![
            blueice_ipc::BlueTsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 0,
                kind: blueice_ipc::BlueTsScriptKind::Classic,
                outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueTsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 1,
                kind: blueice_ipc::BlueTsScriptKind::Module,
                outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueTsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 2,
                kind: blueice_ipc::BlueTsScriptKind::Classic,
                outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Rejected {
                    category: "BlueTS compilation rejected the page script".to_string(),
                },
            },
        ]
    );
    assert!(reports.iter().all(|report| match &report.outcome {
        blueice_ipc::BlueTsScriptExecutionOutcome::Executed => true,
        blueice_ipc::BlueTsScriptExecutionOutcome::Rejected { category } => {
            !category.contains("blueiceDocumentText(1)")
        }
    }));

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    let status = child
        .wait()
        .expect("failed to wait for blueice-core to exit");
    assert!(
        status.success(),
        "blueice-core must exit cleanly after Shutdown"
    );
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_executes_opted_in_standard_javascript_and_reports_source_free_outcomes() {
    // This is the Phase 13 page-host process proof: ordinary HTML scripts
    // travel through real HTTP navigation into the compiled core binary, share
    // its tab/document realm lifecycle, and expose only bounded outcomes.
    let socket_path = unique_socket_path("inline-bluejs");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-inline-bluejs-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("ij-gk");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>inline JavaScript process proof</main>",
            "<script>blueiceDocumentText(); blueiceDocumentOrigin();</script>",
            "<script type=\"module\">export const moduleAnswer = 43;</script>",
            "<script>const = malformed;</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluejs",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let mut stream = UnixStream::connect(&socket_path).unwrap();
    blueice_ipc::client_handshake(&mut stream).unwrap();
    let url = format!("http://{addr}");
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
        blueice_ipc::ServerMessage::FrameReady { generation: 1, .. }
    ));

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueJsScriptReports,
    )
    .unwrap();
    let reports = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::BlueJsScriptReports(reports) => reports,
        other => panic!("expected BlueJsScriptReports, got {other:?}"),
    };
    assert_eq!(
        reports,
        vec![
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 0,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 1,
                kind: blueice_ipc::BlueJsScriptKind::Module,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 2,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Rejected {
                    category: "JavaScript parsing rejected the page script".to_string(),
                },
            },
        ]
    );
    assert!(reports.iter().all(|report| match &report.outcome {
        blueice_ipc::BlueJsScriptExecutionOutcome::Executed => true,
        blueice_ipc::BlueJsScriptExecutionOutcome::Rejected { category } => {
            !category.contains("const = malformed")
        }
    }));

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_routes_an_explicit_page_lifecycle_to_the_private_bluejs_host() {
    // Exercise the production core binary on its explicit child-host route.
    // The server is the launcher's real child-host state machine; unlike the
    // regular in-process `--inline-bluejs` fixture, core reaches it only over
    // the capability-authenticated page-host protocol.
    let socket_path = unique_socket_path("oopj");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-out-of-process-bluejs-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("oopj-gk");
    let (host_socket, host_token, host) = spawn_private_bluejs_host("out-of-process-host");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>out-of-process JavaScript process proof</main>",
            "<script>globalThis.answer = 42;</script>",
            "<script type=\"module\">export const moduleAnswer = 43;</script>",
            "<script src=\"untrusted.js\"></script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--out-of-process-bluejs-socket",
            host_socket.to_str().unwrap(),
            "--out-of-process-bluejs-token",
            &host_token,
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let mut stream = UnixStream::connect(&socket_path).unwrap();
    blueice_ipc::client_handshake(&mut stream).unwrap();
    let url = format!("http://{addr}");
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
        blueice_ipc::ServerMessage::FrameReady { generation: 1, .. }
    ));

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueJsScriptReports,
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::BlueJsScriptReports(vec![
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 0,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 1,
                kind: blueice_ipc::BlueJsScriptKind::Module,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 2,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Rejected {
                    category: "external JavaScript declarations require an authorized loader"
                        .to_string(),
                },
            },
        ])
    );

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(core.wait().unwrap().success());
    shutdown_private_bluejs_host(&host_socket, &host_token);
    host.join().unwrap();
    let _ = std::fs::remove_file(&host_socket);
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_rebinds_inline_javascript_document_context_after_replacement() {
    // The current document origin is a copied, realm-local value. Exercise two
    // real navigations so a stale first-document callback would turn the second
    // document's explicit origin assertion into a source-free runtime failure.
    let socket_path = unique_socket_path("ij-repl");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-inline-bluejs-replacement-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("ijr-gk");

    let listener_one = TcpListener::bind("127.0.0.1:0").unwrap();
    let url_one = format!("http://{}", listener_one.local_addr().unwrap());
    let first_document_origin = url_one.clone();
    thread::spawn(move || {
        let (mut stream, _) = listener_one.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = format!(
            "<main>first JavaScript replacement document</main>\
             <script>if (blueiceDocumentOrigin() !== '{first_document_origin}') \
             {{ throw 'stale origin'; }}</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let listener_two = TcpListener::bind("127.0.0.1:0").unwrap();
    let url_two = format!("http://{}", listener_two.local_addr().unwrap());
    let second_document_origin = url_two.clone();
    thread::spawn(move || {
        let (mut stream, _) = listener_two.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = format!(
            "<main>second JavaScript replacement document</main>\
             <script>if (blueiceDocumentOrigin() !== '{second_document_origin}') \
             {{ throw 'stale origin'; }}</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluejs",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let mut stream = UnixStream::connect(&socket_path).unwrap();
    blueice_ipc::client_handshake(&mut stream).unwrap();

    for (expected_generation, url) in [(1, url_one), (2, url_two)] {
        blueice_ipc::write_client_message(
            &mut stream,
            &blueice_ipc::ClientMessage::Navigate { url },
        )
        .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut stream).unwrap(),
            blueice_ipc::ServerMessage::Navigated { .. }
        ));
        assert!(matches!(
            blueice_ipc::read_server_message(&mut stream).unwrap(),
            blueice_ipc::ServerMessage::FrameReady {
                generation,
                ..
            } if generation == expected_generation
        ));

        blueice_ipc::write_client_message(
            &mut stream,
            &blueice_ipc::ClientMessage::GetBlueJsScriptReports,
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut stream).unwrap(),
            blueice_ipc::ServerMessage::BlueJsScriptReports(vec![
                blueice_ipc::BlueJsScriptExecutionReport {
                    tab_id: 1,
                    document_generation: expected_generation,
                    ordinal: 0,
                    kind: blueice_ipc::BlueJsScriptKind::Classic,
                    outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
                },
            ])
        );
    }

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_rejects_an_oversized_document_text_binding_before_inline_admission() {
    // The profile's document-text boundary has a core-selected 1 MiB contract
    // limit. Exercise it through real navigation and process IPC so the page
    // cannot turn a rejection into either source/diagnostic disclosure or a
    // partially admitted BlueJS program.
    let socket_path = unique_socket_path("inline-contract");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-inline-contract-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("ic-gk");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let oversized_text = "x".repeat(1_048_577);
        let body = format!("<main>{oversized_text}</main>",)
            + concat!(
                "<script type=\"application/x-blueice-typescript\">",
                "blueiceDocumentText();",
                "</script>"
            );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluets-profile",
            "core-script-document-text-v1",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let mut stream = UnixStream::connect(&socket_path).unwrap();
    blueice_ipc::client_handshake(&mut stream).unwrap();
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{addr}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { generation: 1, .. }
    ));

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    let reports = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::BlueTsScriptReports(reports) => reports,
        other => panic!("expected BlueTsScriptReports, got {other:?}"),
    };
    assert_eq!(reports.len(), 1);
    let blueice_ipc::BlueTsScriptExecutionOutcome::Rejected { category } = &reports[0].outcome
    else {
        panic!("the oversized document snapshot must reject before admission")
    };
    assert_eq!(category, "host binding contract rejected the page script");
    assert!(!category.contains('x'));

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_keeps_opted_in_inline_bluets_reports_isolated_by_tab() {
    // The report query is an observation boundary, so prove it through the
    // compiled process rather than relying only on the executor's queue test:
    // two independently navigated tabs may execute, but draining one must not
    // disclose or discard the other tab's report.
    let socket_path = unique_socket_path("inline-bluets-tabs");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-inline-bluets-tabs-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("ibt-gk");

    let listener_one = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_one = listener_one.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_one.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>first inline page</main>",
            "<script type=\"application/x-blueice-typescript\">",
            "blueiceDocumentText();",
            "</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    let listener_two = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_two = listener_two.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_two.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>second inline page</main>",
            "<script type=\"application/x-blueice-typescript\">",
            "blueiceDocumentText();",
            "</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluets-profile",
            "core-script-document-text-v1",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let mut stream = UnixStream::connect(&socket_path).unwrap();
    blueice_ipc::client_handshake(&mut stream).unwrap();

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{addr_one}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::OpenTab {
            url: Some(format!("http://{addr_two}")),
        },
    )
    .unwrap();
    let tab_two = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::TabOpened { tab_id, .. } => tab_id,
        other => panic!("expected TabOpened, got {other:?}"),
    };
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    let (reply_tab, _, first_reports) =
        blueice_ipc::read_server_message_with_ids(&mut stream).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(
        first_reports,
        blueice_ipc::ServerMessage::BlueTsScriptReports(vec![
            blueice_ipc::BlueTsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 0,
                kind: blueice_ipc::BlueTsScriptKind::Classic,
                outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Executed,
            },
        ])
    );

    blueice_ipc::write_client_message_with_ids(
        &mut stream,
        Some(tab_two),
        None,
        &blueice_ipc::ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    let (reply_tab, _, second_reports) =
        blueice_ipc::read_server_message_with_ids(&mut stream).unwrap();
    assert_eq!(reply_tab, Some(tab_two));
    assert_eq!(
        second_reports,
        blueice_ipc::ServerMessage::BlueTsScriptReports(vec![
            blueice_ipc::BlueTsScriptExecutionReport {
                tab_id: tab_two,
                document_generation: 1,
                ordinal: 0,
                kind: blueice_ipc::BlueTsScriptKind::Classic,
                outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Executed,
            },
        ])
    );

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_reexecutes_opted_in_inline_bluets_for_a_replacement_document() {
    // A replacement must receive a new page generation and a fresh execution
    // observation. Drive that lifecycle through the binary so it covers the
    // fetch, gatekeeper, session, realm-owner, and frontend IPC boundaries.
    // Keep the Unix-domain socket leaf short enough for macOS's
    // `sockaddr_un::sun_path` limit.
    let socket_path = unique_socket_path("ib-repl");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-inline-bluets-replacement-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("ibr-gk");

    let listener_one = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_one = listener_one.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_one.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>first replacement document</main>",
            "<script type=\"application/x-blueice-typescript\">",
            "const text: string = blueiceDocumentText(); text;",
            "</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    let listener_two = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_two = listener_two.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_two.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>second replacement document</main>",
            "<script type=\"application/x-blueice-typescript\">",
            "const text: string = blueiceDocumentText(); text;",
            "</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluets-profile",
            "core-script-document-text-v1",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let mut stream = UnixStream::connect(&socket_path).unwrap();
    blueice_ipc::client_handshake(&mut stream).unwrap();

    for (expected_generation, address) in [(1, addr_one), (2, addr_two)] {
        blueice_ipc::write_client_message(
            &mut stream,
            &blueice_ipc::ClientMessage::Navigate {
                url: format!("http://{address}"),
            },
        )
        .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut stream).unwrap(),
            blueice_ipc::ServerMessage::Navigated { .. }
        ));
        assert!(matches!(
            blueice_ipc::read_server_message(&mut stream).unwrap(),
            blueice_ipc::ServerMessage::FrameReady {
                generation,
                ..
            } if generation == expected_generation
        ));

        blueice_ipc::write_client_message(
            &mut stream,
            &blueice_ipc::ClientMessage::GetBlueTsScriptReports,
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut stream).unwrap(),
            blueice_ipc::ServerMessage::BlueTsScriptReports(vec![
                blueice_ipc::BlueTsScriptExecutionReport {
                    tab_id: 1,
                    document_generation: expected_generation,
                    ordinal: 0,
                    kind: blueice_ipc::BlueTsScriptKind::Classic,
                    outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Executed,
                },
            ])
        );
    }

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_serves_two_independently_addressed_tabs_without_cross_contamination() {
    // `phase-16-multi-tab-and-tab-groups/PLAN.md`'s minimal-first-slice
    // proof, one layer up from `session.rs`'s own in-process tests: the
    // real compiled `blueice-core` binary, driven over a real socket,
    // must keep two tabs' navigation and representation fully
    // independent.
    let socket_path = unique_socket_path("multi-tab");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-frames-multi-tab-{}",
        std::process::id()
    ));
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
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    let listener_two = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_two = listener_two.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_two.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = "<p>second tab content</p>";
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
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

    assert!(
        wait_for(&socket_path, Duration::from_secs(5)),
        "blueice-core never created its socket"
    );
    let mut stream =
        UnixStream::connect(&socket_path).expect("failed to connect to the real subprocess");
    blueice_ipc::client_handshake(&mut stream)
        .expect("the real subprocess must complete the protocol_version handshake");

    // Navigate the default (first) tab.
    let url_one = format!("http://{addr_one}");
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate {
            url: url_one.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url: url_one }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    // Open a second tab navigated straight to a different URL.
    let url_two = format!("http://{addr_two}");
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::OpenTab {
            url: Some(url_two.clone()),
        },
    )
    .unwrap();
    let (tab_two, tab_two_url) = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::TabOpened { tab_id, url } => (tab_id, url),
        other => panic!("expected TabOpened, got {other:?}"),
    };
    assert_eq!(tab_two_url, Some(url_two));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    // The default tab's own representation must still show only its
    // own content -- opening and navigating a second tab must not have
    // touched it.
    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::GetRepresentation)
        .unwrap();
    let default_tab_snapshot = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::Representation(snapshot) => snapshot,
        other => panic!("expected Representation, got {other:?}"),
    };
    assert!(default_tab_snapshot
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("first tab content")));
    assert!(!default_tab_snapshot
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("second tab content")));

    // The second tab's representation, addressed explicitly, must show
    // only *its* content.
    blueice_ipc::write_client_message_with_ids(
        &mut stream,
        Some(tab_two),
        None,
        &blueice_ipc::ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut stream).unwrap();
    assert_eq!(reply_tab, Some(tab_two));
    let tab_two_snapshot = match reply {
        blueice_ipc::ServerMessage::Representation(snapshot) => snapshot,
        other => panic!("expected Representation, got {other:?}"),
    };
    assert_eq!(tab_two_snapshot.tab_id, tab_two);
    assert!(tab_two_snapshot
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("second tab content")));
    assert!(!tab_two_snapshot
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("first tab content")));

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    let status = child
        .wait()
        .expect("failed to wait for blueice-core to exit");
    assert!(
        status.success(),
        "blueice-core must exit cleanly after Shutdown"
    );

    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn a_client_disconnecting_without_shutdown_still_lets_the_subprocess_exit_cleanly() {
    let socket_path = unique_socket_path("disconnect");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-frames-disconnect-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let stream = UnixStream::connect(&socket_path).unwrap();
    drop(stream); // disconnect without ever sending Shutdown

    let status = child.wait_timeout_or_kill();
    assert!(
        status.success(),
        "blueice-core must exit cleanly when its one client just disconnects"
    );
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
