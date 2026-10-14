// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::generated_map_faults::{assert_closed_map_failure, FaultLaunchDir};
use super::*;
use blueice_ipc::debugger::{DebuggerGeneratedBreakpoint, DebuggerGeneratedBreakpointTarget};
use blueice_ipc::page_host::{self, PageHostErrorCode, PageHostReply, PageHostRequest};

const PAGE_SOURCE: &str = "const answer: number = 42;";

fn independently_compiled_map(source: &str) -> (String, blueice_bluets::SourceMap) {
    let module = "memory:///asserted-map.ts";
    let compilation = blueice_bluets::compile(
        module,
        &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(module, source)]),
        blueice_bluets::CompilerOptions {
            source_map: true,
            inline_sources: true,
            ..Default::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    (
        artifact.javascript.clone(),
        artifact.source_map.clone().unwrap(),
    )
}

fn private_outcome(mode: &str) -> PageHostErrorCode {
    match mode {
        "other_generation" => PageHostErrorCode::StaleDocument,
        "other_javascript_hash" => PageHostErrorCode::InvalidRequest,
        "external_assertion" => PageHostErrorCode::UnauthorizedArtifact,
        _ => panic!("unknown substituted map mode: {mode}"),
    }
}

#[test]
fn substituted_child_entry() {
    let Ok(socket) = std::env::var("BLUEICE_TEST_MAP_FAULT_SOCKET") else {
        return;
    };
    let token = std::env::var("BLUEICE_TEST_MAP_FAULT_TOKEN").unwrap();
    let mode = std::env::var("BLUEICE_TEST_MAP_FAULT_MODE").unwrap();
    let expected = private_outcome(&mode);
    let (other_javascript, other_map) = independently_compiled_map("const answer: number = 43;");
    let (_, asserted_map) = independently_compiled_map(PAGE_SOURCE);
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
        let mut captured = false;
        let mut substituted = false;
        loop {
            let request = match page_host::read_page_host_request(&mut stream) {
                Ok(request) => request,
                Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(error) => panic!("private child request failed: {error}"),
            };
            let shutting_down = matches!(request, PageHostRequest::Shutdown);
            let first_mapped_lookup = !captured
                && matches!(
                    request,
                    PageHostRequest::ResolveDebuggerEmittedSafePoint { .. }
                );
            let first_identity = match &request {
                PageHostRequest::ResolveDebuggerEmittedSafePoint {
                    tab_id,
                    document_generation,
                    program,
                    ..
                } if first_mapped_lookup => Some((*tab_id, *document_generation, *program)),
                _ => None,
            };
            let replacement_document = match &request {
                PageHostRequest::SynchronizeDocument { document }
                    if mode == "other_generation" && captured && !substituted =>
                {
                    Some((document.tab_id, document.document_generation))
                }
                _ => None,
            };
            let inspect_substituted_lookup = substituted
                && matches!(
                    request,
                    PageHostRequest::ResolveDebuggerEmittedOriginalPosition { .. }
                        | PageHostRequest::ResolveDebuggerEmittedSafePoint { .. }
                        | PageHostRequest::GetDebuggerEmittedPausedRootLocation { .. }
                );
            let reply = host.handle_request(request);
            if let Some((tab_id, document_generation, program)) = first_identity {
                assert!(
                    matches!(
                        reply,
                        PageHostReply::DebuggerEmittedSafePoint {
                            safe_point: Some(_),
                            ..
                        }
                    ),
                    "{mode}: the live compiler map must first resolve: {reply:?}"
                );
                if mode == "other_generation" {
                    host.capture_retained_emitted_map_for_test(
                        tab_id,
                        document_generation,
                        program,
                    );
                } else if mode == "other_javascript_hash" {
                    host.substitute_other_javascript_emitted_map_for_test(
                        tab_id,
                        document_generation,
                        program,
                        other_javascript.clone(),
                        other_map.clone(),
                    );
                    substituted = true;
                } else {
                    host.substitute_unadmitted_emitted_map_for_test(
                        tab_id,
                        document_generation,
                        program,
                        asserted_map.clone(),
                    );
                    substituted = true;
                }
                captured = true;
            }
            if let Some((tab_id, document_generation)) = replacement_document {
                assert!(
                    matches!(reply, PageHostReply::Synchronized { .. }),
                    "{mode}"
                );
                host.substitute_prior_generation_emitted_map_for_test(tab_id, document_generation);
                substituted = true;
            }
            if inspect_substituted_lookup {
                assert!(
                    matches!(reply, PageHostReply::Error { code, .. } if code == expected),
                    "{mode}: private child must distinguish stale, mismatched and unadmitted maps: {reply:?}"
                );
            }
            page_host::write_page_host_reply(&mut stream, &reply).unwrap();
            if shutting_down {
                return;
            }
        }
    }
}

#[test]
fn public_substituted_maps_fail_closed_with_distinct_private_outcomes() {
    let fault_binaries = FaultLaunchDir::new("generated_map_substitution::substituted_child_entry");
    for mode in [
        "other_generation",
        "other_javascript_hash",
        "external_assertion",
    ] {
        let gatekeeper_socket = clearing_gatekeeper();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let fetches = if mode == "other_generation" { 2 } else { 1 };
        let fixture = thread::spawn(move || {
            for _ in 0..fetches {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                let body = format!(
                    "<main>map substitution</main><script type=\"application/x-blueice-typescript\">{PAGE_SOURCE}</script>"
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
            }
        });
        let mut launcher = LauncherProcess::spawn_with_binary(
            &gatekeeper_socket,
            StaticMetadataPolicy {
                emitted_blue_ts_classic: true,
                generated_breakpoint: true,
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
            safe_point: Some(_),
            ..
        }) = debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint { target },
        )
        else {
            panic!("{mode}: initial compiler-owned map must resolve")
        };
        let (realm, program, target) = if mode == "other_generation" {
            release_next_document_hold(&mut debugger, 1);
            hold_next_document(&mut debugger, 1);
            navigate(&mut browser, &url);
            let new_realm = one_realm(debugger_request(
                &mut debugger,
                DebuggerRequest::ListPageRealms,
            ));
            let new_program = one_program(
                debugger_request(
                    &mut debugger,
                    DebuggerRequest::ListPrograms { realm: new_realm },
                ),
                new_realm,
            );
            assert_ne!(new_program, program);
            (
                new_realm,
                new_program,
                DebuggerGeneratedBreakpointTarget {
                    program: new_program,
                    generated_line: 0,
                    generated_column_utf16: 0,
                },
            )
        } else {
            (realm, program, target)
        };
        for request in [
            DebuggerRequest::ResolveGeneratedBreakpoint { target },
            DebuggerRequest::ArmGeneratedBreakpoint { target },
        ] {
            assert_closed_map_failure(debugger_request(&mut debugger, request), mode);
        }
        assert_eq!(
            debugger_request(&mut debugger, DebuggerRequest::ListBreakpoints { realm }),
            DebuggerReply::Breakpoints(Vec::new()),
            "{mode}: refused map may not retain a breakpoint"
        );
        release_next_document_hold(&mut debugger, 1);
        await_completed_execution(&mut debugger, program);
        assert_eq!(
            debugger_request(&mut debugger, DebuggerRequest::ListBreakpoints { realm }),
            DebuggerReply::Breakpoints(Vec::new()),
            "{mode}: no breakpoint may appear after execution"
        );
        drop(debugger);
        launcher.shutdown();
        fixture.join().unwrap();
    }
}
