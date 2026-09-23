// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

//! Real-process coverage for the launcher-supervised child debugger route.
//!
//! This test intentionally uses only the launcher's public browser, control,
//! and debugger sockets.  It neither opens the launcher-created child socket
//! nor supplies a child capability.  Two local HTTP navigations establish
//! that the public debugger inventory and execution control are source-free,
//! and that every opaque realm, program, and safe-point identity expires with
//! its document.

use blueice_ipc::debugger::{
    read_debugger_reply, write_debugger_request, DebuggerErrorCode,
    DebuggerMetadataCapabilityManifest, DebuggerPageRealm, DebuggerProgram, DebuggerReply,
    DebuggerRequest, DebuggerSafePoint, DEBUGGER_PROTOCOL_VERSION,
};
use blueice_ipc::{read_server_message, write_client_message, ClientMessage, ServerMessage};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

fn unique_path(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blueice-launcher-oop-debugger-{label}-{}-{suffix}.sock",
        std::process::id()
    ))
}

fn unique_frame_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blueice-launcher-oop-debugger-frames-{}-{suffix}",
        std::process::id()
    ))
}

/// Provides the already-required core authorization answer without adding a
/// test-only navigation bypass.  The page still crosses the real launcher's
/// browser route, core HTTP loader, and supervised child route.
fn clearing_gatekeeper() -> PathBuf {
    let path = unique_path("gatekeeper");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("test gatekeeper must bind");
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let _ = blueice_ai_gatekeeper::handle_one_check(&mut stream);
        }
    });
    path
}

struct LauncherProcess {
    child: Child,
    rendezvous_socket: PathBuf,
    control_socket: PathBuf,
    debugger_socket: PathBuf,
    frame_dir: PathBuf,
}

impl LauncherProcess {
    fn spawn(gatekeeper_socket: &Path) -> Self {
        Self::spawn_with_static_metadata_policy(gatekeeper_socket, false, false)
    }

    fn spawn_with_static_metadata_policy(
        gatekeeper_socket: &Path,
        static_metadata_inventory: bool,
        static_metadata_summary: bool,
    ) -> Self {
        let rendezvous_socket = unique_path("rendezvous");
        let control_socket = unique_path("control");
        let debugger_socket = unique_path("debugger");
        let frame_dir = unique_frame_dir();
        let _ = std::fs::remove_file(&rendezvous_socket);
        let _ = std::fs::remove_file(&control_socket);
        let _ = std::fs::remove_file(&debugger_socket);
        let _ = std::fs::remove_dir_all(&frame_dir);

        let mut command = Command::new(env!("CARGO_BIN_EXE_blueice-launcher"));
        command.args([
            "--socket",
            rendezvous_socket.to_str().unwrap(),
            "--control-socket",
            control_socket.to_str().unwrap(),
            "--debugger-socket",
            debugger_socket.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_socket.to_str().unwrap(),
            "--out-of-process-bluejs",
            "--width",
            "320",
            "--height",
            "200",
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ]);
        if static_metadata_inventory {
            command.arg("--debugger-static-metadata-inventory");
        }
        if static_metadata_summary {
            command.arg("--debugger-static-metadata-summary");
        }
        let child = command.spawn().expect("blueice-launcher must spawn");
        let mut process = Self {
            child,
            rendezvous_socket,
            control_socket,
            debugger_socket,
            frame_dir,
        };
        process.wait_for_socket(&process.rendezvous_socket.clone(), "browser rendezvous");
        process.wait_for_socket(&process.control_socket.clone(), "control");
        process.wait_for_socket(&process.debugger_socket.clone(), "debugger");
        process
    }

    fn wait_for_socket(&mut self, path: &Path, name: &str) {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            if path.exists() {
                return;
            }
            if let Some(status) = self
                .child
                .try_wait()
                .expect("must be able to poll blueice-launcher")
            {
                panic!("launcher exited before creating {name} socket: {status}");
            }
            if Instant::now() >= deadline {
                self.wait_or_kill(Duration::from_secs(1));
                panic!("launcher did not create {name} socket within {STARTUP_TIMEOUT:?}");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn connect_browser(&self) -> UnixStream {
        UnixStream::connect(&self.rendezvous_socket)
            .expect("launcher browser rendezvous must accept a public client")
    }

    fn wait_or_kill(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn shutdown(&mut self) {
        if let Ok(mut browser) = self.connect_browser_result() {
            if blueice_ipc::client_handshake(&mut browser).is_ok() {
                let _ = write_client_message(&mut browser, &ClientMessage::Shutdown);
            }
        }
        self.wait_or_kill(Duration::from_secs(5));
    }

    fn connect_browser_result(&self) -> std::io::Result<UnixStream> {
        UnixStream::connect(&self.rendezvous_socket)
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

fn navigate(browser: &mut UnixStream, url: &str) {
    write_client_message(
        browser,
        &ClientMessage::Navigate {
            url: url.to_string(),
        },
    )
    .expect("public browser client must submit local HTTP navigation");
    assert_eq!(
        read_server_message(browser).expect("core must acknowledge navigation"),
        ServerMessage::Navigated {
            url: url.to_string()
        }
    );
    assert!(matches!(
        read_server_message(browser).expect("core must render navigated document"),
        ServerMessage::FrameReady { .. }
    ));
}

fn debugger_request(stream: &mut UnixStream, request: DebuggerRequest) -> DebuggerReply {
    write_debugger_request(stream, &request).expect("public debugger request must frame");
    read_debugger_reply(stream).expect("public debugger reply must frame")
}

fn one_realm(reply: DebuggerReply) -> DebuggerPageRealm {
    let DebuggerReply::PageRealms(realms) = reply else {
        panic!("expected source-free page realm inventory")
    };
    assert_eq!(realms.len(), 1, "fixture has one loaded public tab");
    assert!(
        realms[0].is_well_formed(),
        "realm must be an opaque live identity"
    );
    realms[0]
}

fn one_program(reply: DebuggerReply, realm: DebuggerPageRealm) -> DebuggerProgram {
    let DebuggerReply::Programs(programs) = reply else {
        panic!("expected source-free program inventory")
    };
    assert_eq!(programs.len(), 1, "fixture has one classic declaration");
    assert!(programs[0].is_well_formed());
    assert_eq!(programs[0].realm, realm);
    programs[0]
}

fn safe_points(reply: DebuggerReply, program: DebuggerProgram) -> Vec<DebuggerSafePoint> {
    let DebuggerReply::SafePoints(safe_points) = reply else {
        panic!("expected opaque compiler-verified safe-point inventory")
    };
    assert!(
        !safe_points.is_empty(),
        "classic program must expose at least one executable boundary"
    );
    assert!(safe_points
        .iter()
        .all(|safe_point| { safe_point.is_well_formed() && safe_point.program == program }));
    safe_points
}

fn serve_two_classic_documents(listener: TcpListener) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener
                .accept()
                .expect("fixture must receive local HTTP navigation");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = concat!(
                "<main>launcher-supervised-debugger</main>",
                "<script>const localCounter = 1; globalThis.localCounter = localCounter;</script>"
            );
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .expect("fixture must reply with local classic document");
        }
    })
}

fn serve_two_bluets_documents(listener: TcpListener) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for (ordinal, source) in [
            ("first", "const privateBlueTsMetadata: number = 42;"),
            ("second", "const successorBlueTsMetadata: number = 43;"),
        ] {
            let (mut stream, _) = listener
                .accept()
                .expect("fixture must receive local HTTP navigation");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = format!(
                concat!(
                    "<main>launcher-supervised-metadata-{ordinal}</main>",
                    "<script type=\"application/x-blueice-typescript\">{source}</script>"
                ),
                ordinal = ordinal,
                source = source,
            );
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .expect("fixture must reply with a local typed document");
        }
    })
}

#[test]
fn launcher_supervised_child_debugger_execution_is_opaque_and_expires_after_http_reload() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_two_classic_documents(listener);
    let mut launcher = LauncherProcess::spawn(&gatekeeper_socket);

    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);

    let mut debugger = UnixStream::connect(&launcher.debugger_socket)
        .expect("launcher public debugger endpoint must accept a peer");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
        }
    );
    let first_realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let first_program = one_program(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListPrograms { realm: first_realm },
        ),
        first_realm,
    );
    let first_safe_points = safe_points(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListSafePoints {
                program: first_program,
            },
        ),
        first_program,
    );
    let first_safe_point = first_safe_points[0];
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ValidateSafePoint {
                safe_point: first_safe_point,
            },
        ),
        DebuggerReply::SafePointValidated {
            safe_point: first_safe_point,
        }
    );
    let root_safe_point = *first_safe_points
        .iter()
        .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0)
        .expect("classic program must expose a resumable non-entry root safe point");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState {
                program: first_program,
            },
        ),
        DebuggerReply::ExecutionState {
            program: first_program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
        }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmRootSafePointBreakpoint {
                safe_point: root_safe_point,
            },
        ),
        DebuggerReply::RootSafePointBreakpointArmed {
            safe_point: root_safe_point,
        }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState {
                program: first_program,
            },
        ),
        DebuggerReply::ExecutionState {
            program: first_program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Paused {
                safe_point: root_safe_point,
            },
        }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResumeExecution {
                program: first_program,
            },
        ),
        DebuggerReply::ExecutionResumed {
            program: first_program,
        }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState {
                program: first_program,
            },
        ),
        DebuggerReply::ExecutionState {
            program: first_program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Resuming,
        }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState {
                program: first_program,
            },
        ),
        DebuggerReply::ExecutionState {
            program: first_program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Completed,
        }
    );

    // Reload through the same public browser connection. The fixture accepts
    // exactly two requests so this is a real HTTP replacement, not a direct
    // page-host document injection.
    navigate(&mut browser, &url);
    fixture
        .join()
        .expect("local HTTP fixture must serve both documents");

    for request in [
        DebuggerRequest::ListPrograms { realm: first_realm },
        DebuggerRequest::ListSafePoints {
            program: first_program,
        },
        DebuggerRequest::ValidateSafePoint {
            safe_point: first_safe_point,
        },
        DebuggerRequest::ValidateSafePoint {
            safe_point: root_safe_point,
        },
    ] {
        assert!(matches!(
            debugger_request(&mut debugger, request),
            DebuggerReply::Error {
                code: DebuggerErrorCode::StaleRealm,
                ..
            }
        ));
    }

    let successor_realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    assert_eq!(
        successor_realm.browser_context_id,
        first_realm.browser_context_id
    );
    assert_eq!(successor_realm.tab_id, first_realm.tab_id);
    assert_ne!(
        successor_realm.realm_generation, first_realm.realm_generation,
        "a public debugger realm cannot survive an HTTP document replacement"
    );
    let successor_program = one_program(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListPrograms {
                realm: successor_realm,
            },
        ),
        successor_realm,
    );
    let _ = safe_points(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListSafePoints {
                program: successor_program,
            },
        ),
        successor_program,
    );

    launcher.shutdown();
    assert!(
        !launcher.debugger_socket.exists(),
        "launcher shutdown must remove its public debugger endpoint"
    );
    assert!(
        !launcher.frame_dir.exists(),
        "launcher shutdown must remove its generation frame state"
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_owner_policy_exposes_only_handle_bound_bluets_metadata_summary_after_negotiation() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_two_bluets_documents(listener);
    let mut launcher =
        LauncherProcess::spawn_with_static_metadata_policy(&gatekeeper_socket, true, true);

    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);

    let mut debugger = UnixStream::connect(&launcher.debugger_socket)
        .expect("launcher public debugger endpoint must accept a peer");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::opaque_summary(
                ),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::opaque_summary(),
        }
    );
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let DebuggerReply::Capabilities(capabilities) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeCapabilities { realm },
    ) else {
        panic!("typed fixture must expose debugger capabilities")
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataInventory
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSummary
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    let DebuggerReply::Programs(programs) =
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm })
    else {
        panic!("typed fixture must expose its opaque program inventory")
    };
    assert!(
        !programs.is_empty(),
        "typed fixture must retain at least one opaque program identity"
    );

    let mut typed_metadata = None;
    for program in programs {
        let reply = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadata { program },
        );
        assert!(
            !format!("{reply:?}").contains("privateBlueTsMetadata")
                && !format!("{reply:?}").contains("number"),
            "public reply must contain no BlueTS metadata payload"
        );
        match reply {
            DebuggerReply::StaticMetadata(handles) if handles.is_empty() => {}
            DebuggerReply::StaticMetadata(handles) => {
                assert_eq!(
                    handles.len(),
                    1,
                    "only the direct BlueTS program is eligible"
                );
                let handle = handles[0];
                assert_eq!(handle.program, program);
                assert!(handle.is_well_formed());
                typed_metadata = Some(handle);
            }
            other => panic!("expected a bounded opaque metadata inventory, got {other:?}"),
        }
    }
    let typed_metadata = typed_metadata.expect("fixture must include one direct BlueTS program");
    let summary_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadata {
            metadata: typed_metadata,
        },
    );
    let DebuggerReply::StaticMetadataSummary(summary) = summary_reply else {
        panic!("expected one bounded public static metadata summary")
    };
    assert_eq!(summary.metadata, typed_metadata);
    assert_eq!(summary.language_version, "blue-ts-0.1");
    assert!(summary.source_count > 0);
    assert!(summary.type_count > 0);
    assert!(summary.symbol_count > 0);
    assert!(
        !format!("{summary:?}").contains("privateBlueTsMetadata")
            && !format!("{summary:?}").contains("inline-0.ts")
            && !format!("{summary:?}").contains("number"),
        "public summary must not contain a BlueTS metadata record payload"
    );

    navigate(&mut browser, &url);
    fixture
        .join()
        .expect("local HTTP fixture must serve both documents");
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadata {
                program: typed_metadata.program,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadata {
                metadata: typed_metadata,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));

    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}
