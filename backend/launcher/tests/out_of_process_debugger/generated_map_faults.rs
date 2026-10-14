// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::debugger::{DebuggerGeneratedBreakpoint, DebuggerGeneratedBreakpointTarget};
use blueice_ipc::page_host::{self, PageHostReply, PageHostRequest};
use std::os::unix::fs::PermissionsExt;

const PAGE_SOURCE: &str = "const answer: number = 42;";
const SANITIZED_PUBLIC_FAILURE: &str =
    "native debugger program locations require an enabled live JavaScript page realm";

pub(super) fn assert_closed_map_failure(reply: DebuggerReply, mode: &str) {
    assert_eq!(
        reply,
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            message: SANITIZED_PUBLIC_FAILURE.to_string(),
        },
        "{mode}: child map fault must disclose no source or private error detail"
    );
}

pub(super) struct FaultLaunchDir(PathBuf);

impl FaultLaunchDir {
    pub(super) fn new(child_entry: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "blueice-map-fault-launch-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        let normal_launcher = Path::new(env!("CARGO_BIN_EXE_blueice-launcher"));
        let normal_core = normal_launcher.parent().unwrap().join("blueice-core");
        assert!(normal_core.is_file(), "the real core sibling must be built");
        std::fs::copy(normal_launcher, path.join("blueice-launcher")).unwrap();
        std::fs::copy(normal_core, path.join("blueice-core")).unwrap();
        let wrapper = path.join("blueice-bluejs-host");
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  case \"$1\" in\n    --socket) BLUEICE_TEST_MAP_FAULT_SOCKET=\"$2\"; shift 2;;\n    --session-token) BLUEICE_TEST_MAP_FAULT_TOKEN=\"$2\"; shift 2;;\n    *) shift;;\n  esac\ndone\nexport BLUEICE_TEST_MAP_FAULT_SOCKET BLUEICE_TEST_MAP_FAULT_TOKEN\nexec \"$BLUEICE_TEST_MAP_FAULT_EXECUTABLE\" --exact {child_entry} --nocapture\n"
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&wrapper).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&wrapper, permissions).unwrap();
        Self(path)
    }

    pub(super) fn launcher(&self) -> PathBuf {
        self.0.join("blueice-launcher")
    }
}

impl Drop for FaultLaunchDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("the test-owned launch directory is disposable");
    }
}

#[test]
fn fault_child_entry() {
    let Ok(socket) = std::env::var("BLUEICE_TEST_MAP_FAULT_SOCKET") else {
        return;
    };
    let token = std::env::var("BLUEICE_TEST_MAP_FAULT_TOKEN").unwrap();
    let mode = std::env::var("BLUEICE_TEST_MAP_FAULT_MODE").unwrap();
    let mappings = match mode.as_str() {
        "bad_vlq" => "!",
        "non_monotonic_column" => "AAAA,AAAA",
        "source_index" => "ACAA",
        "name_index" => "AAAAA",
        _ => panic!("unknown child-retained map corruption: {mode}"),
    };
    let listener =
        blueice_launcher::bluejs_host::bind_bluejs_host_socket(Path::new(&socket)).unwrap();
    let mut host = blueice_launcher::bluejs_host::BlueJsChildHost::default();
    for incoming in listener.incoming() {
        let mut stream = incoming.unwrap();
        let hello = page_host::read_page_host_request(&mut stream).unwrap();
        let reply = page_host::negotiate(&hello, &token);
        let accepted = matches!(reply, PageHostReply::HelloAck { .. });
        page_host::write_page_host_reply(&mut stream, &reply).unwrap();
        if !accepted {
            continue;
        }
        let mut injected = false;
        loop {
            let request = match page_host::read_page_host_request(&mut stream) {
                Ok(request) => request,
                Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(error) => panic!("private child request failed: {error}"),
            };
            let shutting_down = matches!(request, PageHostRequest::Shutdown);
            let inject_at = match &request {
                PageHostRequest::ResolveDebuggerEmittedSafePoint {
                    tab_id,
                    document_generation,
                    program,
                    ..
                } if !injected => Some((*tab_id, *document_generation, *program)),
                _ => None,
            };
            let reply = host.handle_request(request);
            if let Some((tab_id, document_generation, program)) = inject_at {
                assert!(
                    matches!(
                        reply,
                        PageHostReply::DebuggerEmittedSafePoint {
                            safe_point: Some(_),
                            ..
                        }
                    ),
                    "the first live lookup must prove an actual mapped safe point: {reply:?}"
                );
                host.corrupt_retained_emitted_map_for_test(
                    tab_id,
                    document_generation,
                    program,
                    mappings,
                );
                injected = true;
            }
            page_host::write_page_host_reply(&mut stream, &reply).unwrap();
            if shutting_down {
                return;
            }
        }
    }
}

#[test]
fn public_malformed_map_faults_fail_closed_through_real_launcher_core_and_child() {
    let fault_binaries = FaultLaunchDir::new("generated_map_faults::fault_child_entry");
    for mode in [
        "bad_vlq",
        "non_monotonic_column",
        "source_index",
        "name_index",
    ] {
        let gatekeeper_socket = clearing_gatekeeper();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let fixture = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = format!(
                "<main>map fault</main><script type=\"application/x-blueice-typescript\">{PAGE_SOURCE}</script>"
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
        let mut launcher = LauncherProcess::spawn_with_binary(
            &gatekeeper_socket,
            StaticMetadataPolicy {
                emitted_blue_ts_classic: true,
                generated_breakpoint: true,
                generated_paused_location: true,
                ..Default::default()
            },
            None,
            &fault_binaries.launcher(),
            Some(mode),
        );
        let mut browser = launcher.connect_browser();
        blueice_ipc::client_handshake(&mut browser).unwrap();
        let manifest = DebuggerMetadataCapabilityManifest::opaque_selected(
            DebuggerMetadataCapabilitySelection {
                generated_breakpoint: true,
                generated_paused_location: true,
                ..Default::default()
            },
        );
        let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::Hello {
                    protocol_version: DEBUGGER_PROTOCOL_VERSION,
                    requested_bounded_values: false,
                    requested_metadata_capabilities: manifest.clone(),
                }
            ),
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_bounded_values: false,
                granted_metadata_capabilities: manifest,
            },
            "{mode}"
        );
        hold_next_document(&mut debugger, 1);
        navigate(&mut browser, &url);
        let realm = one_realm(debugger_request(
            &mut debugger,
            DebuggerRequest::ListPageRealms,
        ));
        let program = one_program(
            debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
            realm,
        );
        let target = DebuggerGeneratedBreakpointTarget {
            program,
            generated_line: 0,
            generated_column_utf16: 0,
        };
        let DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
            safe_point: Some(safe_point),
            ..
        }) = debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint { target },
        )
        else {
            panic!("{mode}: first uncorrupted lookup must identify a mapped root safe point")
        };
        for request in [
            DebuggerRequest::ResolveGeneratedBreakpoint { target },
            DebuggerRequest::ArmGeneratedBreakpoint { target },
        ] {
            let reply = debugger_request(&mut debugger, request);
            assert_closed_map_failure(reply, mode);
        }
        assert_eq!(
            debugger_request(&mut debugger, DebuggerRequest::ListBreakpoints { realm }),
            DebuggerReply::Breakpoints(Vec::new()),
            "{mode}: generated arming must retain no breakpoint"
        );
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ArmRootSafePointBreakpoint { safe_point }
            ),
            DebuggerReply::RootSafePointBreakpointArmed { safe_point },
            "{mode}: raw pause should remain independently available"
        );
        await_paused_execution(&mut debugger, program, safe_point);
        let projection = debugger_request(
            &mut debugger,
            DebuggerRequest::GetGeneratedPausedLocation { program },
        );
        assert_closed_map_failure(projection, mode);
        assert!(matches!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ClearBreakpoint { safe_point }
            ),
            DebuggerReply::BreakpointCleared { .. }
        ));
        assert_eq!(
            debugger_request(&mut debugger, DebuggerRequest::ListBreakpoints { realm }),
            DebuggerReply::Breakpoints(Vec::new()),
            "{mode}: no breakpoint may survive the fault checks"
        );
        assert_eq!(
            debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
            DebuggerReply::ExecutionResumed { program },
            "{mode}"
        );
        await_completed_execution(&mut debugger, program);
        write_client_message(&mut browser, &ClientMessage::GetBlueTsScriptReports).unwrap();
        assert!(
            matches!(
                read_server_message(&mut browser).unwrap(),
                ServerMessage::BlueTsScriptReports(reports)
                    if reports.len() == 1
                        && reports[0].outcome == blueice_ipc::BlueTsScriptExecutionOutcome::Executed
            ),
            "{mode}: page must still execute to completion"
        );
        drop(debugger);
        launcher.shutdown();
        fixture.join().unwrap();
    }
}
