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
    DebuggerErrorCode, DebuggerExecutionState, DebuggerLinkedArmTarget,
    DebuggerLinkedExecutionState, DebuggerLinkedStackCoordinatesTarget,
    DebuggerMetadataCapabilityManifest, DebuggerMetadataCapabilitySelection, DebuggerPageRealm,
    DebuggerProgram, DebuggerReply, DebuggerRequest, DebuggerSafePoint,
    DebuggerStackCoordinatesTarget, DebuggerStaticMetadataSafePointSpanTarget,
    DebuggerStaticMetadataSourceBreakpointTarget, DebuggerStaticMetadataSourceId,
    DebuggerStaticMetadataSymbolKind, DebuggerStaticMetadataSymbolLocationTarget,
    DebuggerValuePreview, DebuggerValueTarget, DEBUGGER_PROTOCOL_VERSION,
};
use blueice_ipc::owner_bootstrap::{
    OwnerHttpOriginRule, OwnerHttpPolicyBootstrap, OwnerHttpResource,
};
use blueice_ipc::{read_server_message, write_client_message, ClientMessage, ServerMessage};
use blueice_launcher::control::{
    read_control_reply, write_control_request, ControlReply, ControlRequest,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};
#[path = "out_of_process_debugger/exceptions.rs"]
mod exceptions;
#[path = "out_of_process_debugger/linked_sources.rs"]
mod linked_sources;
#[path = "out_of_process_debugger/metadata_policy.rs"]
mod metadata_policy;
#[path = "out_of_process_debugger/module_lifecycle.rs"]
mod module_lifecycle;
#[path = "out_of_process_debugger/nested_frames.rs"]
mod nested_frames;
#[path = "out_of_process_debugger/runtime_values.rs"]
mod runtime_values;
#[path = "out_of_process_debugger/scope_relations.rs"]
mod scope_relations;
#[path = "out_of_process_debugger/source_breakpoints.rs"]
mod source_breakpoints;
#[path = "out_of_process_debugger/spans_stack.rs"]
mod spans_stack;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
/// Each test owns a full launcher/core/child tree. Running several at once
/// can exhaust the debugger's intentionally short pending-admission window.
static LAUNCHER_DEBUGGER_TEST_LOCK: Mutex<()> = Mutex::new(());
const FIRST_BLUETS_SOURCE: &str = "export interface PrivateContract { enabled: boolean; }\r\n/* 🚀 */ const privateBlueTsMetadata: number = 42;";
const SOURCE_STEP_BLUETS_SOURCE: &str =
    "const first: number = 1; const second: number = first + 1; const third: number = second + 1;";
const MODULE_BLUETS_SOURCE: &str = "/* 🚀 */\r\nexport const moduleAnswer: number = 42;";
const MODULE_STEP_BLUETS_SOURCE: &str =
    "let first: number = 1; let second: number = first + 1; export const answer: number = second + 1;";
const STACK_COORDINATE_BLUETS_SOURCE: &str = "/* 🚀 */ function inner(a: number): number { let value: number = a + 1; return value; } globalThis.answer = inner(3);";
const MODULE_SYMBOL_BREAKPOINT_BLUETS_SOURCE: &str = "export interface Shape { enabled: boolean; } const rootValue: number = 1; function inner(): number { return 41; } globalThis.answer = inner() + rootValue;";
const BOUNDED_VALUE_BLUETS_SOURCE: &str = "let rootValue: number = 9; function inner(a: number): number { let childValue: number = a + 1; return childValue; } globalThis.answer = inner(3) + rootValue;";
const CLASSIC_THROW_BLUETS_SOURCE: &str = "/* 🚀 */ function fail(): number { throw 7; } fail();";
const MODULE_THROW_BLUETS_SOURCE: &str =
    "/* 🚀 */ function fail(): number { throw 9; } export const answer: number = fail();";
const CAUGHT_THROW_BLUETS_SOURCE: &str = "function fail(): number { throw 11; } globalThis.fail = fail; const evaluate = eval; evaluate('try { globalThis.fail(); } catch (error) { globalThis.caughtBlueTsThrow = error === 11; }'); globalThis.caughtBlueTsThrow;";
const LINKED_ENTRY_SOURCE: &str =
    "import { inner } from './linked-dependency.ts'; export const answer: number = inner() + 1;";
const LINKED_DEPENDENCY_SOURCE: &str = "export function inner(): number { return 41; }";

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
    bounded_values: bool,
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
    static_scope_relation: bool,
}

struct LauncherProcess {
    _test_guard: MutexGuard<'static, ()>,
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
        Self::spawn_with_owner_http_policy(gatekeeper_socket, policy, None)
    }

    fn spawn_with_owner_http_policy(
        gatekeeper_socket: &Path,
        policy: StaticMetadataPolicy,
        owner_http_policy: Option<&Path>,
    ) -> Self {
        let test_guard = LAUNCHER_DEBUGGER_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        if policy.bounded_values {
            command.arg("--debugger-bounded-values");
        }
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
        if policy.static_scope_relation {
            command.arg("--debugger-static-scope-relation");
        }
        if let Some(path) = owner_http_policy {
            command.arg("--page-http-policy-file").arg(path);
        }
        let child = command.spawn().expect("blueice-launcher must spawn");
        let mut process = Self {
            _test_guard: test_guard,
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

fn assert_exception_refusal(reply: DebuggerReply, expected: DebuggerErrorCode) {
    let DebuggerReply::Error { code, message } = reply else {
        panic!("exception location must be refused without a partial position: {reply:?}")
    };
    assert_eq!(code, expected);
    assert!(
        !message.contains("throw 7")
            && !message.contains("throw 11")
            && !message.contains("function fail")
    );
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

fn one_receipted_typed_program(
    debugger: &mut UnixStream,
    stage: &str,
) -> (DebuggerProgram, Vec<DebuggerStaticMetadataSourceId>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let (program, metadata) = loop {
        let realm = one_realm(debugger_request(debugger, DebuggerRequest::ListPageRealms));
        let DebuggerReply::Programs(programs) =
            debugger_request(debugger, DebuggerRequest::ListPrograms { realm })
        else {
            panic!("page must expose its program inventory")
        };
        let mut typed = Vec::new();
        let mut inventory = format!("programs={programs:?}");
        for program in programs {
            let reply = debugger_request(debugger, DebuggerRequest::ListStaticMetadata { program });
            inventory.push_str(&format!(" {program:?}={reply:?}"));
            match reply {
                DebuggerReply::StaticMetadata(metadata) if metadata.is_empty() => {}
                DebuggerReply::StaticMetadata(metadata) => typed.push((program, metadata)),
                reply => panic!("program metadata inventory must remain typed: {reply:?}"),
            }
        }
        if typed.len() == 1 {
            break typed.pop().unwrap();
        }
        assert!(typed.is_empty(), "fixture must contain one BlueTS program");
        assert!(
            Instant::now() < deadline,
            "{stage} fixture did not install its BlueTS program: {inventory}"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(metadata.len(), 1);
    let source_reply = debugger_request(
        debugger,
        DebuggerRequest::ListStaticMetadataSources {
            metadata: metadata[0],
        },
    );
    let DebuggerReply::StaticMetadataSources(sources) = source_reply else {
        panic!("{stage} BlueTS program must expose source IDs: {source_reply:?}")
    };
    assert!(!sources.is_empty());
    (program, sources)
}

fn await_terminal_bluets_execution(debugger: &mut UnixStream, program: DebuggerProgram) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match debugger_request(debugger, DebuggerRequest::GetExecutionState { program }) {
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::Completed,
                ..
            } => break,
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::Pending,
                ..
            } if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            reply => panic!("BlueTS execution did not complete: {reply:?}"),
        }
    }
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

fn arm_first_nested_frame(
    debugger: &mut UnixStream,
    realm: DebuggerPageRealm,
) -> (DebuggerProgram, blueice_ipc::debugger::DebuggerFrame) {
    let program = one_program(
        debugger_request(debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    let target = safe_points(
        debugger_request(debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    )
    .into_iter()
    .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
    .expect("direct BlueTS child has its first verified instruction");
    let armed = debugger_request(
        debugger,
        DebuggerRequest::ArmNestedSafePointBreakpoint { safe_point: target },
    );
    assert_eq!(
        armed,
        DebuggerReply::NestedSafePointBreakpointArmed { safe_point: target },
        "execution state: {:?}",
        debugger_request(debugger, DebuggerRequest::GetExecutionState { program })
    );
    let (frame, paused) = await_nested_paused_execution(debugger, program);
    assert_eq!(paused, target);
    (program, frame)
}

fn find_number_value(
    debugger: &mut UnixStream,
    program: DebuggerProgram,
    frame: Option<blueice_ipc::debugger::DebuggerFrame>,
    frame_index: u32,
    safe_point: DebuggerSafePoint,
    expected: f64,
) -> Option<DebuggerValueTarget> {
    let DebuggerReply::Scopes(scopes) = debugger_request(
        debugger,
        DebuggerRequest::GetScopes {
            program,
            frame,
            frame_index,
            expected_safe_point: safe_point,
            max_scope_entries: 256,
        },
    ) else {
        panic!("the exact paused frame must expose its scope slots");
    };
    assert_eq!(scopes.safe_point, safe_point);
    for scope_entry in scopes.entries {
        let target = DebuggerValueTarget {
            program,
            frame,
            frame_index,
            safe_point,
            scope_entry,
        };
        match debugger_request(debugger, DebuggerRequest::GetValue { target }) {
            DebuggerReply::Value(snapshot) => {
                assert_eq!(snapshot.target, target);
                assert!(snapshot.is_well_formed());
                if snapshot.preview == DebuggerValuePreview::NumberBits(expected.to_bits()) {
                    return Some(target);
                }
            }
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidExecutionState,
                ..
            } => {}
            other => panic!("exact receipted BlueTS value read must not fail: {other:?}"),
        }
    }
    None
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
