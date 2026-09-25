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

use blueice_ipc::compiler::CompilerContractValue;
use blueice_ipc::debugger::{
    read_debugger_reply, write_debugger_request, DebuggerCapability, DebuggerCapabilityState,
    DebuggerErrorCode, DebuggerExecutionState, DebuggerMetadataCapabilityManifest,
    DebuggerMetadataCapabilitySelection, DebuggerPageRealm, DebuggerProgram, DebuggerReply,
    DebuggerRequest, DebuggerSafePoint, DebuggerStaticMetadataSymbolKind,
    DEBUGGER_PROTOCOL_VERSION,
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
const FIRST_BLUETS_SOURCE: &str = "export interface PrivateContract { enabled: boolean; }\r\n/* 🚀 */ const privateBlueTsMetadata: number = 42;";
const SOURCE_STEP_BLUETS_SOURCE: &str =
    "const first: number = 1; const second: number = first + 1; const third: number = second + 1;";
const MODULE_BLUETS_SOURCE: &str = "/* 🚀 */\r\nexport const moduleAnswer: number = 42;";
const MODULE_STEP_BLUETS_SOURCE: &str =
    "let first: number = 1; let second: number = first + 1; export const answer: number = second + 1;";

fn unique_path(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "bi-oopdbg-{label}-{}-{suffix}.sock",
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

/// Every metadata operation remains separately default-denied. Keeping these
/// launch-time choices named prevents a positional boolean from accidentally
/// widening the public debugger test policy as capabilities are added.
#[derive(Default)]
struct StaticMetadataPolicy {
    inventory: bool,
    summary: bool,
    source_inventory: bool,
    source_provenance: bool,
    type_inventory: bool,
    type_display: bool,
    symbol_inventory: bool,
    contract_inventory: bool,
    symbol_display: bool,
    symbol_location: bool,
    safe_point_span: bool,
    source_breakpoint: bool,
    source_span_step: bool,
    contract_location: bool,
    symbol_type: bool,
    symbol_contract: bool,
    contract_display: bool,
    contract_validation: bool,
    lowering_summary: bool,
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
        Self::spawn_with_static_metadata_policy(gatekeeper_socket, StaticMetadataPolicy::default())
    }

    fn spawn_with_static_metadata_policy(
        gatekeeper_socket: &Path,
        policy: StaticMetadataPolicy,
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
        if policy.inventory {
            command.arg("--debugger-static-metadata-inventory");
        }
        if policy.summary {
            command.arg("--debugger-static-metadata-summary");
        }
        if policy.source_inventory {
            command.arg("--debugger-static-metadata-source-inventory");
        }
        if policy.source_provenance {
            command.arg("--debugger-static-metadata-source-provenance");
        }
        if policy.type_inventory {
            command.arg("--debugger-static-metadata-type-inventory");
        }
        if policy.type_display {
            command.arg("--debugger-static-metadata-type-display");
        }
        if policy.symbol_inventory {
            command.arg("--debugger-static-metadata-symbol-inventory");
        }
        if policy.contract_inventory {
            command.arg("--debugger-static-metadata-contract-inventory");
        }
        if policy.symbol_display {
            command.arg("--debugger-static-metadata-symbol-display");
        }
        if policy.symbol_location {
            command.arg("--debugger-static-metadata-symbol-location");
        }
        if policy.safe_point_span {
            command.arg("--debugger-static-metadata-safe-point-span");
        }
        if policy.source_breakpoint {
            command.arg("--debugger-static-metadata-source-breakpoint");
        }
        if policy.source_span_step {
            command.arg("--debugger-static-metadata-source-span-step");
        }
        if policy.contract_location {
            command.arg("--debugger-static-metadata-contract-location");
        }
        if policy.symbol_type {
            command.arg("--debugger-static-metadata-symbol-type");
        }
        if policy.symbol_contract {
            command.arg("--debugger-static-metadata-symbol-contract");
        }
        if policy.contract_display {
            command.arg("--debugger-static-metadata-contract-display");
        }
        if policy.contract_validation {
            command.arg("--debugger-static-metadata-contract-validation");
        }
        if policy.lowering_summary {
            command.arg("--debugger-static-metadata-lowering-summary");
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

fn await_completed_execution(debugger: &mut UnixStream, program: DebuggerProgram) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match debugger_request(debugger, DebuggerRequest::GetExecutionState { program }) {
            DebuggerReply::ExecutionState {
                program: reply_program,
                state: blueice_ipc::debugger::DebuggerExecutionState::Completed,
            } if reply_program == program => return,
            DebuggerReply::ExecutionState {
                program: reply_program,
                state: blueice_ipc::debugger::DebuggerExecutionState::Resuming,
            } if reply_program == program && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            other => panic!("expected bounded root-frame completion, got {other:?}"),
        }
    }
}

fn await_paused_execution(
    debugger: &mut UnixStream,
    program: DebuggerProgram,
    safe_point: DebuggerSafePoint,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match debugger_request(debugger, DebuggerRequest::GetExecutionState { program }) {
            DebuggerReply::ExecutionState {
                program: reply_program,
                state:
                    blueice_ipc::debugger::DebuggerExecutionState::Paused {
                        safe_point: reply_safe_point,
                    },
            } if reply_program == program && reply_safe_point == safe_point => return,
            DebuggerReply::ExecutionState {
                program: reply_program,
                state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
            } if reply_program == program && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            other => panic!("expected bounded root-frame pause, got {other:?}"),
        }
    }
}

fn await_step_paused_execution(
    debugger: &mut UnixStream,
    program: DebuggerProgram,
    previous: DebuggerSafePoint,
) -> DebuggerSafePoint {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match debugger_request(debugger, DebuggerRequest::GetExecutionState { program }) {
            DebuggerReply::ExecutionState {
                program: reply_program,
                state: blueice_ipc::debugger::DebuggerExecutionState::Paused { safe_point },
            } if reply_program == program && safe_point != previous => return safe_point,
            DebuggerReply::ExecutionState {
                program: reply_program,
                state: blueice_ipc::debugger::DebuggerExecutionState::Stepping,
            } if reply_program == program && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            other => panic!("expected bounded debugger step pause, got {other:?}"),
        }
    }
}

fn await_nested_paused_execution(
    debugger: &mut UnixStream,
    program: DebuggerProgram,
) -> (blueice_ipc::debugger::DebuggerFrame, DebuggerSafePoint) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match debugger_request(debugger, DebuggerRequest::GetExecutionState { program }) {
            DebuggerReply::ExecutionState {
                program: reply_program,
                state: DebuggerExecutionState::NestedPaused { frame, safe_point },
            } if reply_program == program => return (frame, safe_point),
            DebuggerReply::ExecutionState {
                program: reply_program,
                state:
                    DebuggerExecutionState::Pending | DebuggerExecutionState::NestedStepping { .. },
            } if reply_program == program && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            other => panic!("expected bounded nested-frame pause, got {other:?}"),
        }
    }
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
            (
                "first",
                FIRST_BLUETS_SOURCE,
            ),
            (
                "second",
                "interface SuccessorContract { enabled: boolean; } const successorBlueTsMetadata: number = 43;",
            ),
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

fn serve_two_bluets_source_step_documents(listener: TcpListener) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("local HTTP fixture must connect");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = format!(
                "<script type=\"application/x-blueice-typescript\">{SOURCE_STEP_BLUETS_SOURCE}</script>"
            );
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .expect("fixture must reply with typed source");
        }
    })
}

fn serve_bluets_module_document(listener: TcpListener) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .expect("local module fixture must connect");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            "<script type=\"application/x-blueice-typescript-module\">{MODULE_BLUETS_SOURCE}</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .expect("fixture must reply with a local typed module");
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
    let root_safe_point = *first_safe_points
        .iter()
        .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0)
        .expect("classic program must expose a resumable non-entry root safe point");
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
    await_paused_execution(&mut debugger, first_program, root_safe_point);
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
    await_completed_execution(&mut debugger, first_program);

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
fn public_socket_steps_one_real_bluets_nested_frame_and_rejects_root_aliasing() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let source = "function inner(): number { let value: number = 4; return value; } globalThis.answer = inner() + 1;";
        let body = format!(
            "<main>nested-bluets</main><script type=\"application/x-blueice-typescript\">{source}</script>"
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
    let mut launcher = LauncherProcess::spawn(&gatekeeper_socket);
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    navigate(&mut browser, &url);

    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
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
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let DebuggerReply::Capabilities(capabilities) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeCapabilities { realm },
    ) else {
        panic!("expected public debugger capabilities");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::NestedFrames
            && report.state == DebuggerCapabilityState::Available
    }));
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    let target = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    )
    .into_iter()
    .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
    .expect("BlueTS inner function must have its first safe point");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmNestedSafePointBreakpoint { safe_point: target },
        ),
        DebuggerReply::NestedSafePointBreakpointArmed { safe_point: target }
    );
    let (frame, first) = await_nested_paused_execution(&mut debugger, program);
    assert_eq!(first, target);
    assert!(frame.matches_safe_point(first));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepRootInstruction { program }
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepNestedInstruction { frame },
        ),
        DebuggerReply::NestedStepRequested { frame }
    );
    let (successor_frame, successor) = await_nested_paused_execution(&mut debugger, program);
    assert_eq!(successor_frame, frame);
    assert_ne!(successor, first);
    assert_eq!(successor.code_unit_ordinal, first.code_unit_ordinal);

    let mut current = successor;
    let mut returned = false;
    for _ in 0..96 {
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::StepNestedInstruction { frame },
            ),
            DebuggerReply::NestedStepRequested { frame }
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match debugger_request(
                &mut debugger,
                DebuggerRequest::GetExecutionState { program },
            ) {
                DebuggerReply::ExecutionState {
                    state:
                        DebuggerExecutionState::NestedPaused {
                            frame: same,
                            safe_point,
                        },
                    ..
                } => {
                    assert_eq!(same, frame);
                    assert_ne!(safe_point, current);
                    current = safe_point;
                    break;
                }
                DebuggerReply::ExecutionState {
                    state: DebuggerExecutionState::Paused { safe_point },
                    ..
                } => {
                    assert_eq!(safe_point.code_unit_ordinal, 0);
                    returned = true;
                    break;
                }
                DebuggerReply::ExecutionState {
                    state: DebuggerExecutionState::NestedStepping { frame: same },
                    ..
                } if same == frame && Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                reply => panic!("unexpected nested successor: {reply:?}"),
            }
        }
        if returned {
            break;
        }
    }
    assert!(
        returned,
        "nested frame must return to its root continuation"
    );
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepNestedInstruction { frame }
        ),
        DebuggerReply::Error { .. }
    ));
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    await_completed_execution(&mut debugger, program);
    write_client_message(&mut browser, &ClientMessage::GetBlueTsScriptReports).unwrap();
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::BlueTsScriptReports(reports)
            if reports.len() == 1
                && reports[0].kind == blueice_ipc::BlueTsScriptKind::Classic
                && reports[0].outcome == blueice_ipc::BlueTsScriptExecutionOutcome::Executed
    ));

    fixture.join().unwrap();
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn public_socket_keeps_unsupported_deeper_bluets_call_shape_unavailable() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let source = "function inner(): number { return 4; } function outer(): number { return inner(); } outer();";
        let body = format!(
            "<main>unsupported-nested-bluets</main><script type=\"application/x-blueice-typescript\">{source}</script>"
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
    let mut launcher = LauncherProcess::spawn(&gatekeeper_socket);
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    navigate(&mut browser, &url);

    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            },
        ),
        DebuggerReply::HelloAck { .. }
    ));
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    let target = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    )
    .into_iter()
    .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
    .expect("nested BlueTS function has an exact static point");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmNestedSafePointBreakpoint { safe_point: target },
        ),
        DebuggerReply::NestedSafePointBreakpointArmed { safe_point: target }
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program },
        ) {
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::Completed,
                ..
            } => break,
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::Pending,
                ..
            } if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            reply => panic!("unsupported deeper call must not expose a frame: {reply:?}"),
        }
    }
    write_client_message(&mut browser, &ClientMessage::GetBlueTsScriptReports).unwrap();
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::BlueTsScriptReports(reports)
            if reports.len() == 1
                && reports[0].kind == blueice_ipc::BlueTsScriptKind::Classic
                && matches!(reports[0].outcome, blueice_ipc::BlueTsScriptExecutionOutcome::Rejected { .. })
    ));

    fixture.join().unwrap();
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_inventories_a_pending_bluets_module_before_execution() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("fixture must receive navigation");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            "<main>pending-module</main><script type=\"application/x-blueice-typescript-module\">{MODULE_BLUETS_SOURCE}</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .expect("fixture must reply with a module document");
    });
    let mut launcher = LauncherProcess::spawn(&gatekeeper_socket);
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);

    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
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
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program }
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
        }
    );
    let points = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    );
    assert!(points.iter().any(|point| point.code_unit_ordinal == 0));

    blueice_ipc::write_client_message(&mut browser, &blueice_ipc::ClientMessage::GetDom).unwrap();
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::Dom(_)
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program }
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Completed,
        }
    );

    fixture.join().unwrap();
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_pauses_and_resumes_a_real_bluets_module_entry() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("fixture must receive navigation");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            "<main>module-entry-debugger</main><script type=\"application/x-blueice-typescript-module\">{MODULE_BLUETS_SOURCE}</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .expect("fixture must reply with the module document");
    });
    let mut launcher = LauncherProcess::spawn(&gatekeeper_socket);
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);

    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
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
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let DebuggerReply::Programs(programs) =
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm })
    else {
        panic!("expected the module and following classic programs");
    };
    if programs.len() != 1 {
        write_client_message(&mut browser, &ClientMessage::GetBlueTsScriptReports).unwrap();
        let bluets = read_server_message(&mut browser).unwrap();
        write_client_message(&mut browser, &ClientMessage::GetBlueJsScriptReports).unwrap();
        let bluejs = read_server_message(&mut browser).unwrap();
        panic!("expected one module program, got {programs:?}; {bluets:?}; {bluejs:?}");
    }
    let program = programs[0];
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program }
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
        }
    );
    let points = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    );
    let mut armed = None;
    for point in points
        .into_iter()
        .filter(|point| point.code_unit_ordinal == 0)
    {
        match debugger_request(
            &mut debugger,
            DebuggerRequest::ArmRootSafePointBreakpoint { safe_point: point },
        ) {
            DebuggerReply::RootSafePointBreakpointArmed { safe_point } => {
                armed = Some(safe_point);
                break;
            }
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidSafePoint | DebuggerErrorCode::InvalidExecutionState,
                ..
            } => {}
            reply => panic!("unexpected module arm reply: {reply:?}"),
        }
    }
    let target = armed.expect("the first entry evaluate-body root point must arm");
    await_paused_execution(&mut debugger, program, target);
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program }
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Resuming,
        }
    );
    await_completed_execution(&mut debugger, program);
    write_client_message(&mut browser, &ClientMessage::GetBlueTsScriptReports).unwrap();
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::BlueTsScriptReports(reports)
            if reports.len() == 1
                && reports[0].kind == blueice_ipc::BlueTsScriptKind::Module
                && reports[0].outcome == blueice_ipc::BlueTsScriptExecutionOutcome::Executed
    ));
    write_client_message(&mut browser, &ClientMessage::GetDom).unwrap();
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::Dom(dom) if format!("{dom:?}").contains("module-entry-debugger")
    ));

    fixture.join().unwrap();
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_steps_a_real_bluets_module_then_rejects_stale_generation() {
    use blueice_ipc::debugger::DebuggerStaticMetadataSafePointSpanTarget;

    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("fixture must receive navigation");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = format!(
                "<main>module-step-debugger</main><script type=\"application/x-blueice-typescript-module\">{MODULE_STEP_BLUETS_SOURCE}</script>"
            );
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .expect("fixture must reply with the module document");
        }
    });
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            source_inventory: true,
            safe_point_span: true,
            source_span_step: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);

    let manifest = DebuggerMetadataCapabilityManifest::opaque_source_span_step();
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: manifest,
        }
    );
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    let DebuggerReply::StaticMetadata(metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("the BlueTS module needs one opaque metadata attachment");
    };
    let metadata = metadata[0];
    let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources { metadata },
    ) else {
        panic!("the same debugger stream must receive source IDs");
    };
    let points = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    );
    let mut armed = None;
    for point in points
        .iter()
        .copied()
        .filter(|point| point.code_unit_ordinal == 0)
    {
        match debugger_request(
            &mut debugger,
            DebuggerRequest::ArmRootSafePointBreakpoint { safe_point: point },
        ) {
            DebuggerReply::RootSafePointBreakpointArmed { safe_point } => {
                armed = Some(safe_point);
                break;
            }
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidSafePoint | DebuggerErrorCode::InvalidExecutionState,
                ..
            } => {}
            reply => panic!("unexpected module arm reply: {reply:?}"),
        }
    }
    let target = armed.expect("the module entry evaluate-body point must arm");
    await_paused_execution(&mut debugger, program, target);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepRootInstruction { program }
        ),
        DebuggerReply::ExecutionStepRequested { program }
    );
    let mut current = await_step_paused_execution(&mut debugger, program, target);
    assert!(points.contains(&current));
    let bound_span = |debugger: &mut UnixStream, point: DebuggerSafePoint| {
        sources.iter().find_map(|source| {
            let target = DebuggerStaticMetadataSafePointSpanTarget {
                safe_point: point,
                source: *source,
            };
            match debugger_request(
                debugger,
                DebuggerRequest::DescribeStaticMetadataSafePointSpan { target },
            ) {
                DebuggerReply::StaticMetadataSafePointSpan(span) => Some((target, span)),
                _ => None,
            }
        })
    };
    let mut origin = None;
    for _ in 0..128 {
        if let Some(span) = bound_span(&mut debugger, current) {
            origin = Some(span);
            break;
        }
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::StepRootInstruction { program }
            ),
            DebuggerReply::ExecutionStepRequested { program }
        );
        current = await_step_paused_execution(&mut debugger, program, current);
        assert!(points.contains(&current));
    }
    let (source_target, original_span) =
        origin.expect("module instruction step must reach a bound span");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepStaticMetadataSourceSpan {
                target: source_target,
            },
        ),
        DebuggerReply::ExecutionSourceSpanStepRequested {
            safe_point: current,
        }
    );
    let successor = await_step_paused_execution(&mut debugger, program, current);
    assert!(points.contains(&successor));
    let (_, successor_span) = bound_span(&mut debugger, successor)
        .expect("source step stops at another bound module span");
    assert_ne!(
        (original_span.start_byte, original_span.end_byte),
        (successor_span.start_byte, successor_span.end_byte)
    );
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    await_completed_execution(&mut debugger, program);
    write_client_message(&mut browser, &ClientMessage::GetBlueTsScriptReports).unwrap();
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::BlueTsScriptReports(reports)
            if reports.len() == 1
                && reports[0].kind == blueice_ipc::BlueTsScriptKind::Module
                && reports[0].outcome == blueice_ipc::BlueTsScriptExecutionOutcome::Executed
    ));

    navigate(&mut browser, &url);
    fixture.join().unwrap();
    for request in [
        DebuggerRequest::ArmRootSafePointBreakpoint { safe_point: target },
        DebuggerRequest::StepRootInstruction { program },
        DebuggerRequest::StepStaticMetadataSourceSpan {
            target: source_target,
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
    assert_eq!(successor_realm.tab_id, realm.tab_id);
    assert_ne!(successor_realm.realm_generation, realm.realm_generation);
    let _ = one_program(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ListPrograms {
                realm: successor_realm,
            },
        ),
        successor_realm,
    );
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_exposes_bluets_metadata_while_its_root_frame_is_pending_and_paused() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_two_bluets_documents(listener);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            summary: true,
            ..StaticMetadataPolicy::default()
        },
    );

    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);

    let mut debugger = UnixStream::connect(&launcher.debugger_socket)
        .expect("launcher public debugger endpoint must accept a peer");
    let metadata_capabilities =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            summary: true,
            ..DebuggerMetadataCapabilitySelection::default()
        });
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_metadata_capabilities: metadata_capabilities.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: metadata_capabilities,
        }
    );
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    let DebuggerReply::StaticMetadata(metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("pending BlueTS program must expose an authorized opaque metadata handle")
    };
    assert_eq!(metadata.len(), 1);
    let metadata = metadata[0];
    let DebuggerReply::StaticMetadataSummary(summary) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadata { metadata },
    ) else {
        panic!("pending BlueTS program must expose an authorized bounded summary")
    };
    assert_eq!(summary.metadata, metadata);
    assert!(summary.symbol_count > 0);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
        }
    );
    let target = *safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    )
    .iter()
    .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset != 0)
    .expect("typed classic program must have a non-entry root safe point");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmRootSafePointBreakpoint { safe_point: target },
        ),
        DebuggerReply::RootSafePointBreakpointArmed { safe_point: target }
    );
    await_paused_execution(&mut debugger, program, target);
    let paused_summary = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadata { metadata },
    );
    assert_eq!(
        paused_summary,
        DebuggerReply::StaticMetadataSummary(summary)
    );
    assert!(!format!("{paused_summary:?}").contains("PrivateContract"));
    assert!(!format!("{paused_summary:?}").contains("privateBlueTsMetadata"));
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Resuming,
        }
    );
    await_completed_execution(&mut debugger, program);

    navigate(&mut browser, &url);
    fixture.join().expect("fixture must serve both documents");
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadata { metadata },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_owner_policy_exposes_only_handle_bound_bluets_metadata_after_negotiation() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_two_bluets_documents(listener);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            summary: true,
            source_inventory: true,
            source_provenance: true,
            type_inventory: true,
            type_display: true,
            symbol_inventory: true,
            contract_inventory: true,
            symbol_display: true,
            symbol_location: true,
            safe_point_span: false,
            source_breakpoint: false,
            source_span_step: false,
            contract_location: true,
            symbol_type: true,
            symbol_contract: true,
            contract_display: true,
            contract_validation: true,
            lowering_summary: true,
        },
    );

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
                requested_metadata_capabilities:
                    DebuggerMetadataCapabilityManifest::opaque_selected(
                        DebuggerMetadataCapabilitySelection {
                            summary: true,
                            source_inventory: true,
                            source_provenance: true,
                            type_inventory: true,
                            type_display: true,
                            symbol_inventory: true,
                            contract_inventory: true,
                            symbol_display: true,
                            symbol_location: true,
                            safe_point_span: false,
                            source_breakpoint: true,
                            source_span_step: false,
                            contract_location: true,
                            symbol_type: true,
                            symbol_contract: true,
                            contract_display: true,
                            contract_validation: true,
                            lowering_summary: true,
                        },
                    ),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::opaque_selected(
                DebuggerMetadataCapabilitySelection {
                    summary: true,
                    source_inventory: true,
                    source_provenance: true,
                    type_inventory: true,
                    type_display: true,
                    symbol_inventory: true,
                    contract_inventory: true,
                    symbol_display: true,
                    symbol_location: true,
                    safe_point_span: false,
                    source_breakpoint: false,
                    source_span_step: false,
                    contract_location: true,
                    symbol_type: true,
                    symbol_contract: true,
                    contract_display: true,
                    contract_validation: true,
                    lowering_summary: true,
                },
            ),
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
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSymbolDisplay
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSymbolLocation
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataContractLocation
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSymbolType
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSymbolContract
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataContractDisplay
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataContractValidation
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataLoweringSummary
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataContractInventory
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataTypeInventory
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataTypeDisplay
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSymbolInventory
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSummary
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSourceInventory
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSourceProvenance
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSourceBreakpoint
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Planned
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
    let guessed_metadata = blueice_ipc::debugger::DebuggerStaticMetadataHandle {
        metadata_handle: typed_metadata.metadata_handle + 1,
        ..typed_metadata
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataLoweringSummary {
                metadata: guessed_metadata,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    let lowering_summary_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadataLoweringSummary {
            metadata: typed_metadata,
        },
    );
    let DebuggerReply::StaticMetadataLoweringSummary(lowering_summary) = lowering_summary_reply
    else {
        panic!("expected a bounded direct BlueTS-to-BlueJS lowering summary")
    };
    assert_eq!(lowering_summary.metadata, typed_metadata);
    assert_eq!(
        lowering_summary.safe_point_map_abi,
        blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_SAFE_POINT_MAP_ABI_V1
    );
    assert_eq!(
        lowering_summary.program_abi,
        blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_PROGRAM_ABI_V1
    );
    assert!(lowering_summary
        .source_set_hash
        .starts_with(blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_SOURCE_SET_HASH_PREFIX));
    assert!(lowering_summary.bound_safe_point_count > 0);
    assert!(
        !format!("{lowering_summary:?}").contains("inline-0.ts")
            && !format!("{lowering_summary:?}").contains("bytecode_offset")
            && !format!("{lowering_summary:?}").contains("privateBlueTsMetadata"),
        "the public lowering summary must not expose source identities, map entries, bytecode offsets, or static records"
    );
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
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataType {
                static_type: blueice_ipc::debugger::DebuggerStaticMetadataTypeId {
                    metadata: typed_metadata,
                    type_id: 0,
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataContract {
                contract: blueice_ipc::debugger::DebuggerStaticMetadataContractId {
                    metadata: typed_metadata,
                    contract_id: 0,
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ValidateStaticMetadataContract {
                contract: blueice_ipc::debugger::DebuggerStaticMetadataContractId {
                    metadata: typed_metadata,
                    contract_id: 0,
                },
                value: CompilerContractValue::Boolean(true),
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbol {
                symbol: blueice_ipc::debugger::DebuggerStaticMetadataSymbolId {
                    metadata: typed_metadata,
                    symbol_id: 0,
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    let unreceipted_relation = blueice_ipc::debugger::DebuggerStaticMetadataSymbolType {
        symbol: blueice_ipc::debugger::DebuggerStaticMetadataSymbolId {
            metadata: typed_metadata,
            symbol_id: 0,
        },
        static_type: blueice_ipc::debugger::DebuggerStaticMetadataTypeId {
            metadata: typed_metadata,
            type_id: 0,
        },
    };
    let unreceipted_contract_relation =
        blueice_ipc::debugger::DebuggerStaticMetadataSymbolContract {
            symbol: blueice_ipc::debugger::DebuggerStaticMetadataSymbolId {
                metadata: typed_metadata,
                symbol_id: 0,
            },
            contract: blueice_ipc::debugger::DebuggerStaticMetadataContractId {
                metadata: typed_metadata,
                contract_id: 0,
            },
        };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolType {
                target: unreceipted_relation,
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolContract {
                target: unreceipted_contract_relation,
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let type_inventory_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataTypes {
            metadata: typed_metadata,
        },
    );
    let DebuggerReply::StaticMetadataTypes(types) = type_inventory_reply else {
        panic!("expected bounded public static metadata type-record IDs")
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolType {
                target: unreceipted_relation,
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert_eq!(types.len(), usize::try_from(summary.type_count).unwrap());
    assert!(types
        .iter()
        .all(|static_type| static_type.metadata == typed_metadata));
    assert!(
        !format!("{types:?}").contains("privateBlueTsMetadata")
            && !format!("{types:?}").contains("number"),
        "type IDs must not contain static type displays or compiler-record payload"
    );
    for static_type in &types {
        let type_display_reply = debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataType {
                static_type: *static_type,
            },
        );
        let DebuggerReply::StaticMetadataType(type_display) = type_display_reply else {
            panic!("expected bounded public static metadata type display")
        };
        assert_eq!(type_display.static_type, *static_type);
        assert!(!type_display.display.is_empty());
        assert!(
            !format!("{type_display:?}").contains("privateBlueTsMetadata"),
            "type display must not expose source text"
        );
    }
    let symbol_inventory_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSymbols {
            metadata: typed_metadata,
        },
    );
    let DebuggerReply::StaticMetadataSymbols(symbols) = symbol_inventory_reply else {
        panic!("expected bounded public static metadata symbol-record IDs")
    };
    assert_eq!(
        symbols.len(),
        usize::try_from(summary.symbol_count).unwrap()
    );
    assert!(symbols
        .iter()
        .all(|symbol| symbol.metadata == typed_metadata));
    assert!(
        !format!("{symbols:?}").contains("privateBlueTsMetadata")
            && !format!("{symbols:?}").contains("number"),
        "symbol IDs must not contain names, type displays, or compiler-record payload"
    );
    let mut saw_interface = false;
    let mut saw_variable = false;
    let mut interface_symbol = None;
    let mut variable_symbol = None;
    for symbol in &symbols {
        let symbol_display_reply = debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbol { symbol: *symbol },
        );
        let DebuggerReply::StaticMetadataSymbol(symbol_display) = symbol_display_reply else {
            panic!("expected bounded public static metadata symbol display")
        };
        assert_eq!(symbol_display.symbol, *symbol);
        assert!(!symbol_display.display.is_empty());
        match symbol_display.display.as_str() {
            "PrivateContract" => {
                assert_eq!(
                    symbol_display.kind,
                    DebuggerStaticMetadataSymbolKind::Interface
                );
                assert!(symbol_display.exported);
                saw_interface = true;
                interface_symbol = Some(*symbol);
            }
            "privateBlueTsMetadata" => {
                assert_eq!(
                    symbol_display.kind,
                    DebuggerStaticMetadataSymbolKind::Variable
                );
                assert!(!symbol_display.exported);
                saw_variable = true;
                variable_symbol = Some(*symbol);
            }
            _ => {}
        }
        assert!(
            !symbol_display.display.contains("const ")
                && !symbol_display.display.contains(": number")
                && !symbol_display.display.contains("= 42")
                && !symbol_display.display.contains("inline-0.ts"),
            "symbol display may expose its authorized name, never declaration source, type, initializer, or module identity"
        );
    }
    assert!(saw_interface && saw_variable);
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolContract {
                target: unreceipted_contract_relation,
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let contract_inventory_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataContracts {
            metadata: typed_metadata,
        },
    );
    let DebuggerReply::StaticMetadataContracts(contracts) = contract_inventory_reply else {
        panic!("expected bounded public static metadata contract IDs")
    };
    assert_eq!(
        contracts.len(),
        usize::try_from(summary.contract_count).unwrap()
    );
    assert!(contracts
        .iter()
        .all(|contract| contract.metadata == typed_metadata));
    assert!(
        !format!("{contracts:?}").contains("privateBlueTsMetadata")
            && !format!("{contracts:?}").contains("number"),
        "contract IDs must not contain names, plans, validation, or compiler-record payload"
    );
    for contract in &contracts {
        let contract_display_reply = debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataContract {
                contract: *contract,
            },
        );
        let DebuggerReply::StaticMetadataContract(contract_display) = contract_display_reply else {
            panic!("expected bounded public static metadata contract display")
        };
        assert_eq!(contract_display.contract, *contract);
        assert!(!contract_display.display.is_empty());
        assert_eq!(
            contract_display.root_kind,
            blueice_ipc::debugger::DebuggerStaticMetadataContractRootKind::Record
        );
        assert!(
            !contract_display.display.contains("interface ")
                && !contract_display.display.contains("enabled")
                && !contract_display.display.contains("boolean")
                && !contract_display.display.contains("inline-0.ts"),
            "contract display may expose its authorized name, never declaration source, field, type, or module identity"
        );
    }
    let valid_contract_validation = debugger_request(
        &mut debugger,
        DebuggerRequest::ValidateStaticMetadataContract {
            contract: contracts[0],
            value: CompilerContractValue::Object(
                [("enabled".to_string(), CompilerContractValue::Boolean(true))]
                    .into_iter()
                    .collect(),
            ),
        },
    );
    let DebuggerReply::StaticMetadataContractValidation(valid_contract_validation) =
        valid_contract_validation
    else {
        panic!("expected bounded public static contract validation result")
    };
    assert_eq!(valid_contract_validation.contract, contracts[0]);
    assert!(valid_contract_validation.valid);
    let invalid_contract_validation = debugger_request(
        &mut debugger,
        DebuggerRequest::ValidateStaticMetadataContract {
            contract: contracts[0],
            value: CompilerContractValue::Object(
                [(
                    "enabled".to_string(),
                    CompilerContractValue::String("not-a-boolean".to_string()),
                )]
                .into_iter()
                .collect(),
            ),
        },
    );
    let DebuggerReply::StaticMetadataContractValidation(invalid_contract_validation) =
        invalid_contract_validation
    else {
        panic!("expected a redacted invalid static contract validation result")
    };
    assert_eq!(invalid_contract_validation.contract, contracts[0]);
    assert!(!invalid_contract_validation.valid);
    assert!(
        !format!("{invalid_contract_validation:?}").contains("enabled")
            && !format!("{invalid_contract_validation:?}").contains("string"),
        "contract validation must not reflect caller input or structural failure detail"
    );
    assert!(
        !format!("{summary:?}").contains("privateBlueTsMetadata")
            && !format!("{summary:?}").contains("inline-0.ts")
            && !format!("{summary:?}").contains("number"),
        "public summary must not contain a BlueTS metadata record payload"
    );
    let source_inventory_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources {
            metadata: typed_metadata,
        },
    );
    let DebuggerReply::StaticMetadataSources(sources) = source_inventory_reply else {
        panic!("expected bounded public static metadata source-record IDs")
    };
    assert_eq!(
        sources.len(),
        usize::try_from(summary.source_count).unwrap()
    );
    assert!(sources
        .iter()
        .all(|source| source.metadata == typed_metadata));
    assert!(
        !format!("{sources:?}").contains("privateBlueTsMetadata")
            && !format!("{sources:?}").contains("inline-0.ts")
            && !format!("{sources:?}").contains("number"),
        "public source IDs must not contain source or compiler-record payload"
    );
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
                    source: sources[0],
                    source_byte: 0,
                },
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
                    source: sources[0],
                    source_byte: 0,
                },
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let initial_contract_location_target =
        blueice_ipc::debugger::DebuggerStaticMetadataContractLocationTarget {
            contract: contracts[0],
            source: sources[0],
        };
    let guessed_contract_source = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadataContractLocation {
            target: blueice_ipc::debugger::DebuggerStaticMetadataContractLocationTarget {
                source: blueice_ipc::debugger::DebuggerStaticMetadataSourceId {
                    source_id: u32::MAX,
                    ..sources[0]
                },
                ..initial_contract_location_target
            },
        },
    );
    assert!(matches!(
        guessed_contract_source,
        DebuggerReply::Unsupported { .. }
    ));
    let mut matching_contract_locations = Vec::new();
    let mut wrong_source_count = 0;
    for source in &sources {
        let target = blueice_ipc::debugger::DebuggerStaticMetadataContractLocationTarget {
            contract: contracts[0],
            source: *source,
        };
        match debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataContractLocation { target },
        ) {
            DebuggerReply::StaticMetadataContractLocation(location) => {
                matching_contract_locations.push((target, location));
            }
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            } => wrong_source_count += 1,
            reply => panic!("unexpected contract/source location reply: {reply:?}"),
        }
    }
    assert_eq!(matching_contract_locations.len(), 1);
    assert_eq!(wrong_source_count + 1, sources.len());
    let (contract_location_target, contract_location) = matching_contract_locations[0];
    assert_eq!(contract_location.contract, contracts[0]);
    assert_eq!(contract_location.source, contract_location_target.source);
    assert!(contract_location.start_byte < contract_location.end_byte);
    assert!(
        contract_location.end_byte
            <= blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
    );
    assert_eq!(contract_location.coordinates.start_line, 0);
    assert_eq!(contract_location.coordinates.end_line, 0);
    assert!(!format!("{contract_location:?}").contains("PrivateContract"));
    assert!(!format!("{contract_location:?}").contains("enabled"));
    for source in &sources {
        let provenance_reply = debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSource { source: *source },
        );
        let DebuggerReply::StaticMetadataSourceProvenance(provenance) = provenance_reply else {
            panic!("expected source-free debugger provenance for an inventoried source")
        };
        assert_eq!(provenance.source, *source);
        assert!(!provenance.module.is_empty());
        assert!(
            !provenance.module.starts_with('/'),
            "compiler provenance must expose a canonical module identity, not a filesystem path"
        );
        assert!(provenance.content_hash.starts_with("bts-sha256:"));
        assert_eq!(provenance.content_hash.len(), "bts-sha256:".len() + 64);
        assert!(
            !format!("{provenance:?}").contains("privateBlueTsMetadata")
                && !format!("{provenance:?}").contains("number"),
            "provenance must contain no source text or static-record payload"
        );
    }
    let location_target = blueice_ipc::debugger::DebuggerStaticMetadataSymbolLocationTarget {
        symbol: interface_symbol.expect("fixture must retain its interface symbol"),
        source: contract_location_target.source,
    };
    let guessed_location_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadataSymbolLocation {
            target: blueice_ipc::debugger::DebuggerStaticMetadataSymbolLocationTarget {
                source: blueice_ipc::debugger::DebuggerStaticMetadataSourceId {
                    source_id: u32::MAX,
                    ..contract_location_target.source
                },
                ..location_target
            },
        },
    );
    assert!(
        matches!(guessed_location_reply, DebuggerReply::Unsupported { .. },),
        "a guessed source ID must fail before the child: {guessed_location_reply:?}"
    );
    let location_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadataSymbolLocation {
            target: location_target,
        },
    );
    let DebuggerReply::StaticMetadataSymbolLocation(location) = location_reply else {
        panic!("expected a bounded public static symbol location")
    };
    assert_eq!(location.symbol, location_target.symbol);
    assert_eq!(location.source, location_target.source);
    assert!(location.start_byte < location.end_byte);
    assert!(
        location.end_byte <= blueice_ipc::debugger::DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
    );
    assert_eq!(location.coordinates.start_line, 0);
    assert_eq!(location.coordinates.end_line, 0);
    assert!(
        !format!("{location:?}").contains("privateBlueTsMetadata")
            && !format!("{location:?}").contains("inline-0.ts")
            && !format!("{location:?}").contains("number"),
        "symbol location must expose only opaque IDs and a bounded byte range"
    );
    let variable_symbol = variable_symbol.expect("fixture must retain its local variable");
    let mut variable_locations = Vec::new();
    for source in &sources {
        match debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolLocation {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSymbolLocationTarget {
                    symbol: variable_symbol,
                    source: *source,
                },
            },
        ) {
            DebuggerReply::StaticMetadataSymbolLocation(location) => {
                variable_locations.push(location);
            }
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget | DebuggerErrorCode::CapabilityUnavailable,
                ..
            } => {}
            reply => panic!("unexpected variable/source location reply: {reply:?}"),
        }
    }
    assert_eq!(variable_locations.len(), 1);
    let variable_location = variable_locations[0];
    assert_eq!(variable_location.coordinates.start_line, 1);
    assert_eq!(
        variable_location.coordinates.start_column_utf16,
        "/* 🚀 */ ".encode_utf16().count() as u32
    );
    assert_eq!(variable_location.coordinates.end_line, 1);
    let guessed_type_relation = blueice_ipc::debugger::DebuggerStaticMetadataSymbolType {
        symbol: symbols[0],
        static_type: blueice_ipc::debugger::DebuggerStaticMetadataTypeId {
            type_id: u32::MAX,
            ..types[0]
        },
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolType {
                target: guessed_type_relation,
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let mut verified_relation = None;
    for symbol in &symbols {
        for static_type in &types {
            let target = blueice_ipc::debugger::DebuggerStaticMetadataSymbolType {
                symbol: *symbol,
                static_type: *static_type,
            };
            match debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeStaticMetadataSymbolType { target },
            ) {
                DebuggerReply::StaticMetadataSymbolType(relation) => {
                    assert_eq!(relation, target);
                    verified_relation = Some(relation);
                    break;
                }
                DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    ..
                } => {}
                reply => panic!("unexpected symbol/type relation reply: {reply:?}"),
            }
        }
        if verified_relation.is_some() {
            break;
        }
    }
    let verified_relation =
        verified_relation.expect("the typed fixture has a symbol/type relation");
    assert!(verified_relation.is_well_formed());
    assert!(
        !format!("{verified_relation:?}").contains("privateBlueTsMetadata")
            && !format!("{verified_relation:?}").contains("number")
            && !format!("{verified_relation:?}").contains("inline-0.ts"),
        "the relation must repeat only opaque IDs"
    );
    let guessed_contract_relation = blueice_ipc::debugger::DebuggerStaticMetadataSymbolContract {
        symbol: symbols[0],
        contract: blueice_ipc::debugger::DebuggerStaticMetadataContractId {
            contract_id: u32::MAX,
            ..contracts[0]
        },
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolContract {
                target: guessed_contract_relation,
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let mut verified_contract_relation = None;
    for symbol in &symbols {
        for contract in &contracts {
            let target = blueice_ipc::debugger::DebuggerStaticMetadataSymbolContract {
                symbol: *symbol,
                contract: *contract,
            };
            match debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeStaticMetadataSymbolContract { target },
            ) {
                DebuggerReply::StaticMetadataSymbolContract(relation) => {
                    assert_eq!(relation, target);
                    verified_contract_relation = Some(relation);
                    break;
                }
                DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    ..
                } => {}
                reply => panic!("unexpected symbol/contract relation reply: {reply:?}"),
            }
        }
        if verified_contract_relation.is_some() {
            break;
        }
    }
    let verified_contract_relation = verified_contract_relation
        .expect("the interface fixture has a reifiable symbol/contract relation");
    assert!(verified_contract_relation.is_well_formed());
    let unrelated_symbol = symbols
        .iter()
        .copied()
        .find(|symbol| *symbol != verified_contract_relation.symbol)
        .expect("the fixture also retains a non-contract value symbol");
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSymbolContract {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSymbolContract {
                    symbol: unrelated_symbol,
                    contract: verified_contract_relation.contract,
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    assert!(
        !format!("{verified_contract_relation:?}").contains("PrivateContract")
            && !format!("{verified_contract_relation:?}").contains("enabled")
            && !format!("{verified_contract_relation:?}").contains("inline-0.ts"),
        "the relation must repeat only opaque IDs, not a contract plan or name"
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
            DebuggerRequest::DescribeStaticMetadataContract {
                contract: contracts[0],
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
            DebuggerRequest::ValidateStaticMetadataContract {
                contract: contracts[0],
                value: CompilerContractValue::Boolean(true),
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
            DebuggerRequest::DescribeStaticMetadataSymbol { symbol: symbols[0] },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataContractLocation {
                target: contract_location_target,
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
            DebuggerRequest::DescribeStaticMetadataSymbolLocation {
                target: location_target,
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
            DebuggerRequest::DescribeStaticMetadataSymbolType {
                target: verified_relation,
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
            DebuggerRequest::DescribeStaticMetadataSymbolContract {
                target: verified_contract_relation,
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
            DebuggerRequest::ListStaticMetadataSources {
                metadata: typed_metadata,
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
            DebuggerRequest::ListStaticMetadataTypes {
                metadata: typed_metadata,
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
            DebuggerRequest::DescribeStaticMetadataType {
                static_type: types[0],
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
            DebuggerRequest::ListStaticMetadataSymbols {
                metadata: typed_metadata,
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
            DebuggerRequest::ListStaticMetadataContracts {
                metadata: typed_metadata,
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
            DebuggerRequest::DescribeStaticMetadataSource { source: sources[0] },
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
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataLoweringSummary {
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

#[test]
fn launcher_exposes_exact_bluets_safe_point_spans_only_after_same_stream_source_receipts() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_two_bluets_documents(listener);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            source_inventory: true,
            safe_point_span: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);

    let manifest = DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: manifest.clone(),
        }
    );
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    let DebuggerReply::Capabilities(capabilities) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeCapabilities { realm },
    ) else {
        panic!("the live child must report debugger capabilities");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSafePointSpan
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    let DebuggerReply::StaticMetadata(metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("the direct BlueTS program must have one opaque metadata attachment");
    };
    let metadata = metadata[0];
    let safe_points = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    );
    let guessed = blueice_ipc::debugger::DebuggerStaticMetadataSafePointSpanTarget {
        safe_point: safe_points[0],
        source: blueice_ipc::debugger::DebuggerStaticMetadataSourceId {
            metadata,
            source_id: 0,
        },
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSafePointSpan { target: guessed },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources { metadata },
    ) else {
        panic!("the exact stream must receive compiler source IDs");
    };
    assert!(!sources.is_empty());
    let mut mapped = None;
    for safe_point in safe_points {
        if safe_point.code_unit_ordinal != 0 || safe_point.bytecode_offset == 0 {
            continue;
        }
        for source in &sources {
            let target = blueice_ipc::debugger::DebuggerStaticMetadataSafePointSpanTarget {
                safe_point,
                source: *source,
            };
            match debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeStaticMetadataSafePointSpan { target },
            ) {
                DebuggerReply::StaticMetadataSafePointSpan(span) => {
                    mapped = Some((target, span));
                    break;
                }
                DebuggerReply::Error {
                    code:
                        DebuggerErrorCode::CapabilityUnavailable | DebuggerErrorCode::InvalidTarget,
                    ..
                } => {}
                reply => panic!("unexpected safe-point span lookup result: {reply:?}"),
            }
        }
        if mapped.is_some() {
            break;
        }
    }
    let (target, span) = mapped.expect("one verified typed instruction has an exact source span");
    assert_eq!(span.safe_point, target.safe_point);
    assert_eq!(span.source, target.source);
    assert!(span.is_well_formed());
    assert!(usize::try_from(span.end_byte).unwrap() <= FIRST_BLUETS_SOURCE.len());
    let source_span = FIRST_BLUETS_SOURCE
        .get(span.start_byte as usize..span.end_byte as usize)
        .expect("the verified span must have original UTF-8 boundaries");
    assert!(source_span.contains("privateBlueTsMetadata"));
    assert_eq!(span.coordinates.start_line, 1);
    assert_eq!(span.coordinates.start_column_utf16, 9);
    assert_eq!(span.coordinates.end_line, 1);
    assert!(span.coordinates.end_column_utf16 > 9);
    assert!(!format!("{span:?}").contains("privateBlueTsMetadata"));
    assert!(!format!("{span:?}").contains("inline-0.ts"));

    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSafePointSpan {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSafePointSpanTarget {
                    source: blueice_ipc::debugger::DebuggerStaticMetadataSourceId {
                        source_id: target.source.source_id + 1,
                        ..target.source
                    },
                    ..target
                },
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));

    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program }
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
        }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmRootSafePointBreakpoint {
                safe_point: target.safe_point,
            },
        ),
        DebuggerReply::RootSafePointBreakpointArmed {
            safe_point: target.safe_point,
        }
    );
    await_paused_execution(&mut debugger, program, target.safe_point);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSafePointSpan { target },
        ),
        DebuggerReply::StaticMetadataSafePointSpan(span),
        "the paused BlueJS frame must retain the same original BlueTS position"
    );
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepStaticMetadataSourceSpan { target },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    await_completed_execution(&mut debugger, program);

    navigate(&mut browser, &url);
    fixture.join().expect("fixture must serve both documents");
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataSafePointSpan { target },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    drop(debugger);
    let mut separate = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert!(matches!(
        debugger_request(
            &mut separate,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_metadata_capabilities: manifest,
            },
        ),
        DebuggerReply::HelloAck { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut separate,
            DebuggerRequest::DescribeStaticMetadataSafePointSpan { target },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    drop(separate);
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_maps_a_real_bluets_module_safe_point_to_original_coordinates() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_bluets_module_document(listener);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            source_inventory: true,
            safe_point_span: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);
    fixture.join().expect("fixture must serve the typed module");

    let manifest = DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: manifest,
        }
    );
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    let DebuggerReply::StaticMetadata(metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("the live BlueTS module must retain static metadata")
    };
    let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources {
            metadata: metadata[0],
        },
    ) else {
        panic!("the module's original source must be receipted")
    };
    let points = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    );
    let mut mapped = None;
    for safe_point in points {
        for source in &sources {
            let target = blueice_ipc::debugger::DebuggerStaticMetadataSafePointSpanTarget {
                safe_point,
                source: *source,
            };
            if let DebuggerReply::StaticMetadataSafePointSpan(span) = debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeStaticMetadataSafePointSpan { target },
            ) {
                mapped = Some(span);
                break;
            }
        }
        if mapped.is_some() {
            break;
        }
    }
    let span = mapped.expect("module root must map to an original BlueTS source span");
    // HTML normalizes CRLF before handing inline script text to BlueTS.
    let normalized_source = MODULE_BLUETS_SOURCE.replace("\r\n", "\n");
    let source_span = normalized_source
        .get(span.start_byte as usize..span.end_byte as usize)
        .expect("module span must have original UTF-8 boundaries");
    assert!(source_span.contains("moduleAnswer"));
    assert!(span.is_well_formed());
    let line_start = normalized_source.find("export").unwrap();
    assert_eq!(span.coordinates.start_line, 1);
    assert_eq!(span.coordinates.end_line, 1);
    assert_eq!(span.coordinates.start_column_utf16, 0);
    assert_eq!(span.start_byte as usize, line_start);
    assert_eq!(
        span.coordinates.end_column_utf16,
        normalized_source[line_start..span.end_byte as usize]
            .encode_utf16()
            .count() as u32
    );
    assert!(!format!("{span:?}").contains("moduleAnswer"));

    drop(debugger);
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_resolves_receipted_bluets_source_positions_to_live_breakpoints() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_two_bluets_documents(listener);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            source_inventory: true,
            source_breakpoint: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);

    let manifest = DebuggerMetadataCapabilityManifest::opaque_source_breakpoint();
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: manifest.clone(),
        }
    );
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    let DebuggerReply::Capabilities(capabilities) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeCapabilities { realm },
    ) else {
        panic!("the live child must report source-breakpoint capabilities");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability
            == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSourceBreakpoint
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::StaticMetadataSafePointSpan
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Planned
    }));
    let DebuggerReply::StaticMetadata(metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("the BlueTS program must have one opaque metadata attachment");
    };
    let metadata = metadata[0];
    let source_byte = u32::try_from(
        FIRST_BLUETS_SOURCE
            .find("const privateBlueTsMetadata")
            .expect("fixture must contain its executable TypeScript declaration"),
    )
    .unwrap();
    let guessed = blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
        source: blueice_ipc::debugger::DebuggerStaticMetadataSourceId {
            metadata,
            source_id: 0,
        },
        source_byte,
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target: guessed },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target: guessed },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources { metadata },
    ) else {
        panic!("the same stream must receive compiler source IDs");
    };
    let mut armed = None;
    for source in sources {
        let candidate = blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
            source,
            source_byte,
        };
        match debugger_request(
            &mut debugger,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target: candidate },
        ) {
            DebuggerReply::RootSafePointBreakpointArmed { safe_point } => {
                armed = Some((candidate, safe_point));
                break;
            }
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidSafePoint | DebuggerErrorCode::InvalidTarget,
                ..
            } => assert_eq!(
                debugger_request(
                    &mut debugger,
                    DebuggerRequest::GetExecutionState { program }
                ),
                DebuggerReply::ExecutionState {
                    program,
                    state: blueice_ipc::debugger::DebuggerExecutionState::Pending,
                },
                "an unbound source must leave the classic declaration pending"
            ),
            reply => panic!("unexpected atomic source-breakpoint arm reply: {reply:?}"),
        }
    }
    let (target, safe_point) = armed.expect("one original BlueTS source must arm its root point");
    assert_eq!(safe_point.program, program);
    assert_eq!(safe_point.code_unit_ordinal, 0);
    assert_ne!(safe_point.bytecode_offset, 0);
    await_paused_execution(&mut debugger, program, safe_point);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
                    source_byte: FIRST_BLUETS_SOURCE.len() as u32,
                    ..target
                },
            },
        ),
        DebuggerReply::StaticMetadataSourceBreakpoint(
            blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpoint {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
                    source_byte: FIRST_BLUETS_SOURCE.len() as u32,
                    ..target
                },
                safe_point: None,
            }
        )
    );
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
                    source: blueice_ipc::debugger::DebuggerStaticMetadataSourceId {
                        source_id: u32::MAX,
                        ..target.source
                    },
                    ..target
                },
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint {
                target: blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpointTarget {
                    source_byte: FIRST_BLUETS_SOURCE.len() as u32,
                    ..target
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidSafePoint,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program }
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Paused { safe_point },
        },
        "an unbound arm must not release the paused root frame"
    );
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target },
        ),
        DebuggerReply::StaticMetadataSourceBreakpoint(
            blueice_ipc::debugger::DebuggerStaticMetadataSourceBreakpoint {
                target,
                safe_point: Some(safe_point),
            }
        )
    );
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    await_completed_execution(&mut debugger, program);

    navigate(&mut browser, &url);
    fixture.join().expect("fixture must serve both documents");
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    drop(debugger);
    let mut separate = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert!(matches!(
        debugger_request(
            &mut separate,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_metadata_capabilities: manifest,
            },
        ),
        DebuggerReply::HelloAck { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut separate,
            DebuggerRequest::ResolveStaticMetadataSourceBreakpoint { target },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut separate,
            DebuggerRequest::ArmStaticMetadataSourceBreakpoint { target },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    drop(separate);
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_steps_only_receipted_paused_bluets_source_spans() {
    use blueice_ipc::debugger::{
        DebuggerCapability, DebuggerCapabilityState, DebuggerExecutionState,
        DebuggerStaticMetadataSafePointSpanTarget, DebuggerStaticMetadataSourceId,
    };

    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP fixture must bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_two_bluets_source_step_documents(listener);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            source_inventory: true,
            safe_point_span: true,
            source_span_step: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).expect("public browser handshake must succeed");
    navigate(&mut browser, &url);

    let manifest = DebuggerMetadataCapabilityManifest::opaque_source_span_step();
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_metadata_capabilities: manifest.clone(),
        }
    );
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    let DebuggerReply::Capabilities(capabilities) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeCapabilities { realm },
    ) else {
        panic!("the live child must report debugger capabilities");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::StaticMetadataSourceSpanStep
            && report.state == DebuggerCapabilityState::Available
    }));
    let DebuggerReply::StaticMetadata(metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("BlueTS program must have metadata");
    };
    let metadata = metadata[0];
    let points = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    );
    let guessed = DebuggerStaticMetadataSafePointSpanTarget {
        safe_point: points[0],
        source: DebuggerStaticMetadataSourceId {
            metadata,
            source_id: 0,
        },
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepStaticMetadataSourceSpan { target: guessed },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataSources { metadata },
    ) else {
        panic!("the stream must receive source receipts");
    };
    let mut mapped = Vec::new();
    for safe_point in points {
        if safe_point.code_unit_ordinal != 0 || safe_point.bytecode_offset == 0 {
            continue;
        }
        for source in &sources {
            let target = DebuggerStaticMetadataSafePointSpanTarget {
                safe_point,
                source: *source,
            };
            if let DebuggerReply::StaticMetadataSafePointSpan(span) = debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeStaticMetadataSafePointSpan { target },
            ) {
                mapped.push((target, span));
                break;
            }
        }
    }
    let (target, original_span) = mapped
        .iter()
        .find(|(_, span)| {
            mapped.iter().any(|(_, later)| {
                later.safe_point.bytecode_offset > span.safe_point.bytecode_offset
                    && (later.start_byte, later.end_byte) != (span.start_byte, span.end_byte)
            })
        })
        .copied()
        .expect("fixture must expose two distinct bound root spans");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmRootSafePointBreakpoint {
                safe_point: target.safe_point,
            },
        ),
        DebuggerReply::RootSafePointBreakpointArmed {
            safe_point: target.safe_point,
        }
    );
    await_paused_execution(&mut debugger, program, target.safe_point);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepStaticMetadataSourceSpan { target },
        ),
        DebuggerReply::ExecutionSourceSpanStepRequested {
            safe_point: target.safe_point,
        }
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let successor = loop {
        match debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program },
        ) {
            DebuggerReply::ExecutionState {
                program: reply_program,
                state: DebuggerExecutionState::Paused { safe_point },
            } if reply_program == program => break safe_point,
            DebuggerReply::ExecutionState {
                program: reply_program,
                state: DebuggerExecutionState::Stepping,
            } if reply_program == program && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            other => panic!("source step must pause at a new bound span: {other:?}"),
        }
    };
    let successor_target = DebuggerStaticMetadataSafePointSpanTarget {
        safe_point: successor,
        source: target.source,
    };
    let DebuggerReply::StaticMetadataSafePointSpan(successor_span) = debugger_request(
        &mut debugger,
        DebuggerRequest::DescribeStaticMetadataSafePointSpan {
            target: successor_target,
        },
    ) else {
        panic!("source-step stop must remain compiler-bound");
    };
    assert_ne!(
        (original_span.start_byte, original_span.end_byte),
        (successor_span.start_byte, successor_span.end_byte)
    );
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    await_completed_execution(&mut debugger, program);
    drop(debugger);
    let mut separate = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert!(matches!(
        debugger_request(
            &mut separate,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_metadata_capabilities: manifest,
            },
        ),
        DebuggerReply::HelloAck { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut separate,
            DebuggerRequest::StepStaticMetadataSourceSpan { target },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert_eq!(
        one_program(
            debugger_request(&mut separate, DebuggerRequest::ListPrograms { realm }),
            realm,
        ),
        program
    );
    let DebuggerReply::StaticMetadata(second_metadata) = debugger_request(
        &mut separate,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("second stream must inventory live metadata independently");
    };
    assert_eq!(second_metadata.len(), 1);
    assert_ne!(
        second_metadata[0], metadata,
        "metadata handles are stream-local"
    );
    let DebuggerReply::StaticMetadataSources(second_sources) = debugger_request(
        &mut separate,
        DebuggerRequest::ListStaticMetadataSources {
            metadata: second_metadata[0],
        },
    ) else {
        panic!("second stream must inventory source IDs independently");
    };
    let second_source = *second_sources
        .iter()
        .find(|source| source.source_id == target.source.source_id)
        .expect("same compiler source remains attached under a new stream-local handle");
    let second_target = DebuggerStaticMetadataSafePointSpanTarget {
        source: second_source,
        ..target
    };
    navigate(&mut browser, &url);
    fixture.join().expect("fixture must serve both documents");
    assert!(matches!(
        debugger_request(
            &mut separate,
            DebuggerRequest::StepStaticMetadataSourceSpan {
                target: second_target,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    drop(separate);
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}
