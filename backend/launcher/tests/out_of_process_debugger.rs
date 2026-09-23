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
    read_debugger_reply, write_debugger_request, DebuggerErrorCode,
    DebuggerMetadataCapabilityManifest, DebuggerMetadataCapabilitySelection, DebuggerPageRealm,
    DebuggerProgram, DebuggerReply, DebuggerRequest, DebuggerSafePoint, DEBUGGER_PROTOCOL_VERSION,
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
                "interface PrivateContract { enabled: boolean; } const privateBlueTsMetadata: number = 42;",
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
    let type_inventory_reply = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataTypes {
            metadata: typed_metadata,
        },
    );
    let DebuggerReply::StaticMetadataTypes(types) = type_inventory_reply else {
        panic!("expected bounded public static metadata type-record IDs")
    };
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
        assert!(
            !symbol_display.display.contains("const ")
                && !symbol_display.display.contains(": number")
                && !symbol_display.display.contains("= 42")
                && !symbol_display.display.contains("inline-0.ts"),
            "symbol display may expose its authorized name, never declaration source, type, initializer, or module identity"
        );
    }
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
