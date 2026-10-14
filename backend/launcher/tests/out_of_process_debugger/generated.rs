// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::debugger::{
    DebuggerGeneratedBreakpoint, DebuggerGeneratedBreakpointTarget,
    DebuggerGeneratedLinkedStackLocations, DebuggerGeneratedOriginalPosition,
    DebuggerGeneratedPausedLocation, DebuggerGeneratedPausedSourceLocation,
};

fn serve_generated_documents(listener: TcpListener, count: usize) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for source in [
            "const answer: number = 42; answer;",
            "const replacement: number = 43; replacement;",
        ]
        .into_iter()
        .take(count)
        {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = format!(
                "<main>emitted</main><script type=\"application/x-blueice-typescript\">{source}</script>"
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
        }
    })
}

fn serve_generated_module_document(
    listener: TcpListener,
    source: &'static str,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            "<main>emitted-module</main><script type=\"application/x-blueice-typescript-module\">{source}</script>"
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
    })
}

#[test]
fn public_generated_module_breakpoint_projects_one_owner_selected_emitted_graph() {
    const SOURCE: &str = "const marker: number = 0; export const answer: number = 42;";
    let compilation = blueice_bluets::compile(
        "memory:///generated-module.ts",
        &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(
            "memory:///generated-module.ts",
            SOURCE,
        )]),
        blueice_bluets::CompilerOptions {
            source_map: true,
            inline_sources: true,
            ..Default::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let generated = &output.artifacts["memory:///generated-module.ts"].javascript;
    let marker_byte = generated.find("const marker").unwrap();
    let before_marker = &generated[..marker_byte];
    let generated_line = before_marker.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let generated_column_utf16 = before_marker
        .rsplit('\n')
        .next()
        .unwrap()
        .encode_utf16()
        .count() as u32;

    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_generated_module_document(listener, SOURCE);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            emitted_blue_ts_module: true,
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    let manifest =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        });
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: manifest,
        }
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
        generated_line,
        generated_column_utf16,
    };
    let binding = debugger_request(
        &mut debugger,
        DebuggerRequest::ResolveGeneratedBreakpoint { target },
    );
    let DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
        safe_point: Some(safe_point),
        ..
    }) = binding
    else {
        panic!("the owner-selected emitted module must bind its mapped root: {binding:?}")
    };
    assert_eq!(safe_point.program, program);
    assert_eq!(safe_point.code_unit_ordinal, 0);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmGeneratedBreakpoint { target },
        ),
        DebuggerReply::RootSafePointBreakpointArmed { safe_point }
    );
    await_paused_execution(&mut debugger, program, safe_point);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetGeneratedPausedLocation { program },
        ),
        DebuggerReply::GeneratedPausedLocation(DebuggerGeneratedPausedLocation {
            program,
            safe_point,
            location: Some(DebuggerGeneratedPausedSourceLocation {
                generated_line,
                generated_column_utf16,
                original: Some(DebuggerGeneratedOriginalPosition {
                    source_index: 0,
                    line: 0,
                    column_utf16: SOURCE.find("const marker").unwrap() as u32,
                }),
            }),
        })
    );
    assert!(matches!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { .. }
    ));
    await_completed_execution(&mut debugger, program);
    drop(debugger);
    launcher.shutdown();
    fixture.join().unwrap();
}

#[test]
fn emitted_module_route_does_not_grant_generated_debugger_capabilities() {
    const SOURCE: &str = "const marker: number = 0; export const answer: number = 42;";
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_generated_module_document(listener, SOURCE);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            emitted_blue_ts_module: true,
            ..Default::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities:
                    DebuggerMetadataCapabilityManifest::opaque_selected(
                        DebuggerMetadataCapabilitySelection {
                            generated_breakpoint: true,
                            generated_paused_location: true,
                            ..Default::default()
                        },
                    ),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
        }
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
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint { target },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmGeneratedBreakpoint { target },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetGeneratedPausedLocation { program },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    drop(debugger);
    launcher.shutdown();
    fixture.join().unwrap();
}

#[test]
fn emitted_module_dependency_binds_only_its_exact_generated_nested_point() {
    const ENTRY: &str = LINKED_ENTRY_SOURCE;
    const DEPENDENCY: &str =
        "export function inner(): number { return 41 + (globalThis as any).offset; }";
    const DEPENDENCY_INTEGRITY: &str =
        "sha256:ba6694c4fcb6d9a5272aa6cebd643e759c7e2b201a880e94c4ef6d1fbf086ef1";
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let entry_url = format!("{origin}/linked-entry.ts");
    let dependency_url = format!("{origin}/linked-dependency.ts");
    let compilation = blueice_bluets::compile(
        &entry_url,
        &blueice_bluets::MapLoader::from([
            blueice_bluets::ModuleSource::new(&entry_url, ENTRY),
            blueice_bluets::ModuleSource::new(&dependency_url, DEPENDENCY),
        ]),
        blueice_bluets::CompilerOptions {
            source_map: true,
            inline_sources: true,
            ..Default::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let generated = &output.artifacts[&dependency_url].javascript;
    let identifier_byte = generated.find("globalThis").unwrap();
    let before_identifier = &generated[..identifier_byte];
    let generated_line = before_identifier
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count() as u32;
    let generated_column_utf16 = before_identifier
        .rsplit('\n')
        .next()
        .unwrap()
        .encode_utf16()
        .count() as u32;
    let policy = OwnerHttpPolicyBootstrap {
        origin_rule: OwnerHttpOriginRule::SameDocumentOrigin,
        resources: vec![
            OwnerHttpResource {
                canonical_url: entry_url,
                integrity:
                    "sha256:0e2a801c62c1a42cfef5afc39c6ad266dba586c6a834679a43adf674cf89f942".into(),
            },
            OwnerHttpResource {
                canonical_url: dependency_url,
                integrity: DEPENDENCY_INTEGRITY.into(),
            },
        ],
    };
    let policy_file = unique_path("generated-linked-owner-policy");
    std::fs::write(&policy_file, serde_json::to_vec(&policy).unwrap()).unwrap();
    let (stop_tx, stop_rx) = mpsc::channel();
    let fixture = thread::spawn(move || {
        const DOCUMENT: &str = "<script type=\"application/x-blueice-typescript-module\" src=\"/linked-entry.ts\"></script>";
        while matches!(stop_rx.try_recv(), Err(mpsc::TryRecvError::Empty)) {
            let Ok((mut stream, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(10));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0_u8; 2048];
            let count = stream.read(&mut request).unwrap();
            let path = std::str::from_utf8(&request[..count])
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap();
            let (mime, body) = match path {
                "/" => ("text/html", DOCUMENT),
                "/linked-entry.ts" => ("text/typescript", ENTRY),
                "/linked-dependency.ts" => ("text/typescript", DEPENDENCY),
                other => panic!("unexpected generated linked resource {other}"),
            };
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
    });
    let mut launcher = LauncherProcess::spawn_with_owner_http_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            emitted_blue_ts_module: true,
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        },
        Some(&policy_file),
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    let manifest =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        });
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: manifest,
        }
    );
    hold_next_document(&mut debugger, 1);
    navigate(&mut browser, &format!("{origin}/"));
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let DebuggerReply::Programs(programs) =
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm })
    else {
        panic!("generated linked graph must publish two opaque programs")
    };
    assert_eq!(programs.len(), 2);
    let binding = programs.iter().find_map(|program| {
        let target = DebuggerGeneratedBreakpointTarget {
            program: *program,
            generated_line,
            generated_column_utf16,
        };
        let reply = debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint { target },
        );
        match reply {
            DebuggerReply::GeneratedBreakpoint(binding)
                if binding
                    .safe_point
                    .is_some_and(|point| point.code_unit_ordinal != 0) =>
            {
                Some((*program, target, binding.safe_point.unwrap()))
            }
            _ => None,
        }
    });
    let (dependency, target, safe_point) =
        binding.expect("exact generated dependency token must bind a nested safe point");
    let entry = *programs
        .iter()
        .find(|program| **program != dependency)
        .unwrap();
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint {
                target: DebuggerGeneratedBreakpointTarget {
                    program: DebuggerProgram {
                        program_generation: dependency.program_generation + 1,
                        ..dependency
                    },
                    ..target
                },
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    let arm = DebuggerLinkedArmTarget {
        entry,
        dependency_safe_point: safe_point,
    };
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmLinkedNestedSafePointBreakpoint { target: arm },
        ),
        DebuggerReply::LinkedNestedSafePointBreakpointArmed { target: arm }
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let stack = loop {
        match debugger_request(
            &mut debugger,
            DebuggerRequest::GetLinkedExecutionState { entry },
        ) {
            DebuggerReply::LinkedExecutionState {
                entry: actual,
                state,
            } if actual == entry => match *state {
                DebuggerLinkedExecutionState::Paused { stack } => break stack,
                DebuggerLinkedExecutionState::Pending if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                other => panic!("generated linked graph did not pause: {other:?}"),
            },
            other => panic!("generated linked graph did not return its state: {other:?}"),
        }
    };
    assert_eq!(stack.frames[0].safe_point, safe_point);
    assert_eq!(stack.frames[0].frame.program, dependency);
    assert_eq!(stack.frames[1].frame.program, entry);
    assert_eq!(target.program, dependency);
    let DebuggerReply::GeneratedLinkedStackLocations(DebuggerGeneratedLinkedStackLocations {
        stack: projected_stack,
        locations,
    }) = debugger_request(
        &mut debugger,
        DebuggerRequest::GetGeneratedLinkedStackLocations {
            expected_stack: stack,
        },
    )
    else {
        panic!("both emitted modules must project one exact linked pause")
    };
    assert_eq!(projected_stack, stack);
    assert_eq!(
        locations[0].unwrap().original,
        Some(DebuggerGeneratedOriginalPosition {
            source_index: 0,
            line: 0,
            column_utf16: DEPENDENCY.find("globalThis").unwrap() as u32,
        })
    );
    // The suspended entry caller has no unique parser/bytecode token at its
    // saved instruction; its frame must remain explicit but unprojected.
    assert_eq!(locations[1], None);
    let mut altered_stack = stack;
    altered_stack.frames[0].frame.frame_handle += 1_000;
    assert!(altered_stack.is_well_formed());
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetGeneratedLinkedStackLocations {
                expected_stack: altered_stack,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetGeneratedPausedLocation {
                program: dependency
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResumeLinkedNestedExecution {
                top_frame: stack.frames[0].frame,
            },
        ),
        DebuggerReply::LinkedNestedResumeRequested {
            top_frame: stack.frames[0].frame,
        }
    );
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetGeneratedLinkedStackLocations {
                expected_stack: stack,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    drop(debugger);
    let mut denied = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut denied,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
        }
    );
    assert_eq!(
        one_realm(debugger_request(
            &mut denied,
            DebuggerRequest::ListPageRealms
        )),
        realm
    );
    assert_eq!(
        debugger_request(&mut denied, DebuggerRequest::ListPrograms { realm }),
        DebuggerReply::Programs(programs.clone())
    );
    assert!(matches!(
        debugger_request(
            &mut denied,
            DebuggerRequest::GetGeneratedLinkedStackLocations {
                expected_stack: stack,
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    drop(denied);
    let mut unreceipted = UnixStream::connect(&launcher.debugger_socket).unwrap();
    let location_grant =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            generated_paused_location: true,
            ..Default::default()
        });
    assert_eq!(
        debugger_request(
            &mut unreceipted,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: location_grant.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: location_grant,
        }
    );
    assert!(matches!(
        debugger_request(
            &mut unreceipted,
            DebuggerRequest::GetGeneratedLinkedStackLocations {
                expected_stack: stack,
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    drop(unreceipted);
    launcher.shutdown();
    stop_tx.send(()).unwrap();
    fixture.join().unwrap();
    std::fs::remove_file(policy_file).unwrap();
}

fn decorated_generated_source() -> String {
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/decorated.ts"
    ))
    .replace(
        "console.log(new Example().method());",
        "const finalAnswer: number = new Example().method();",
    )
}

fn serve_decorated_generated_document(listener: TcpListener) -> thread::JoinHandle<()> {
    serve_decorated_generated_document_with_source(listener, decorated_generated_source())
}

fn serve_decorated_generated_document_with_source(
    listener: TcpListener,
    source: String,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            "<main>decorated</main><script type=\"application/x-blueice-typescript\">{source}</script>"
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
    })
}

fn relocated_decorator_coordinates() -> (u32, u32, u32, u32) {
    let source = decorated_generated_source();
    relocated_decorator_coordinates_for_source(&source)
}

fn relocated_decorator_coordinates_for_source(source: &str) -> (u32, u32, u32, u32) {
    let module = "memory:///main.ts";
    let compilation = blueice_bluets::compile(
        module,
        &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(
            module,
            source.to_string(),
        )]),
        blueice_bluets::CompilerOptions {
            source_map: true,
            inline_sources: true,
            ..Default::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    let decorator_list_start = artifact.javascript.find("_method_decorators = [").unwrap();
    let relocated_byte = decorator_list_start
        + artifact.javascript[decorator_list_start..]
            .find("decoSite")
            .unwrap();
    let position = |source: &str, byte: usize| {
        let prefix = &source[..byte];
        (
            prefix
                .bytes()
                .filter(|character| *character == b'\n')
                .count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        )
    };
    let (generated_line, generated_column) = position(&artifact.javascript, relocated_byte);
    let decorator_start = source.find('@').unwrap();
    let original_byte = decorator_start + source[decorator_start..].find("decoSite").unwrap();
    let (original_line, original_column) = position(source, original_byte);
    let mapped = artifact
        .source_map
        .as_ref()
        .unwrap()
        .original_position_at(
            &artifact.javascript,
            generated_line,
            generated_column,
            200_000,
            16 * 1024 * 1024,
        )
        .unwrap()
        .unwrap();
    assert_eq!(mapped.source_index, 0);
    assert_eq!(
        (mapped.line, mapped.column_utf16),
        (original_line, original_column)
    );
    (
        generated_line as u32,
        generated_column as u32,
        original_line as u32,
        original_column as u32,
    )
}

#[test]
fn public_generated_breakpoint_and_paused_projection_bind_one_real_emitted_artifact() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_generated_documents(listener, 2);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            emitted_blue_ts_classic: true,
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    let manifest =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        });
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: manifest,
        }
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
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint {
                target: DebuggerGeneratedBreakpointTarget {
                    generated_line: blueice_ipc::debugger::DEBUGGER_GENERATED_MAX_COORDINATE + 1,
                    ..target
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    let binding = debugger_request(
        &mut debugger,
        DebuggerRequest::ResolveGeneratedBreakpoint { target },
    );
    let DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
        target: reply_target,
        safe_point: Some(safe_point),
    }) = binding
    else {
        panic!("emitted statement must bind one exact root: {binding:?}")
    };
    assert_eq!(reply_target, target);
    assert_eq!(safe_point.program, program);
    assert_eq!(safe_point.code_unit_ordinal, 0);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint {
                target: DebuggerGeneratedBreakpointTarget {
                    generated_column_utf16: 6,
                    ..target
                }
            },
        ),
        DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
            target: DebuggerGeneratedBreakpointTarget {
                generated_column_utf16: 6,
                ..target
            },
            safe_point: None,
        }),
        "an inner token must not borrow the statement-start safe point"
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmGeneratedBreakpoint { target },
        ),
        DebuggerReply::RootSafePointBreakpointArmed { safe_point }
    );
    await_paused_execution(&mut debugger, program, safe_point);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetGeneratedPausedLocation { program },
        ),
        DebuggerReply::GeneratedPausedLocation(DebuggerGeneratedPausedLocation {
            program,
            safe_point,
            location: Some(DebuggerGeneratedPausedSourceLocation {
                generated_line: 0,
                generated_column_utf16: 0,
                original: Some(DebuggerGeneratedOriginalPosition {
                    source_index: 0,
                    line: 0,
                    column_utf16: 0,
                }),
            }),
        })
    );
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepRootInstruction { program },
        ),
        DebuggerReply::ExecutionStepRequested { .. }
    ));
    let stepped = await_step_paused_execution(&mut debugger, program, safe_point);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetGeneratedPausedLocation { program },
        ),
        DebuggerReply::GeneratedPausedLocation(DebuggerGeneratedPausedLocation {
            program,
            safe_point: stepped,
            location: None,
        }),
        "an interior instruction has no exact generated token coordinate"
    );
    let stale = DebuggerGeneratedBreakpointTarget {
        program: DebuggerProgram {
            program_generation: program.program_generation + 1,
            ..program
        },
        ..target
    };
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint { target: stale },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint {
                target: DebuggerGeneratedBreakpointTarget {
                    program: DebuggerProgram {
                        program_handle: program.program_handle + 1,
                        ..program
                    },
                    ..target
                },
            },
        ),
        DebuggerReply::Unsupported { .. }
    ));

    hold_next_document(&mut debugger, 1);
    navigate(&mut browser, &url);
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint { target },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        } | DebuggerReply::Unsupported { .. }
    ));
    drop(debugger);
    launcher.shutdown();
    fixture.join().unwrap();
}

#[test]
fn public_generated_breakpoint_is_default_denied_on_a_real_child() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_generated_documents(listener, 1);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            emitted_blue_ts_classic: true,
            ..Default::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities:
                    DebuggerMetadataCapabilityManifest::opaque_generated_breakpoint(),
            }
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
        }
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
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint { target },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmGeneratedBreakpoint { target },
        ),
        DebuggerReply::Unsupported { .. }
    ));
    drop(debugger);
    launcher.shutdown();
    fixture.join().unwrap();
}

#[test]
fn public_generated_helper_pause_does_not_disclose_user_source() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_decorated_generated_document(listener);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            emitted_blue_ts_classic: true,
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    let manifest =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        });
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
        }
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
    let (generated_line, generated_column_utf16, _, _) = relocated_decorator_coordinates();
    let relocated_target = DebuggerGeneratedBreakpointTarget {
        program,
        generated_line,
        generated_column_utf16,
    };
    let relocated = debugger_request(
        &mut debugger,
        DebuggerRequest::ResolveGeneratedBreakpoint {
            target: relocated_target,
        },
    );
    let DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
        target: reply_target,
        safe_point: Some(relocated_point),
    }) = relocated
    else {
        panic!("the depth-two decorator must bind its exact safe point: {relocated:?}");
    };
    assert_eq!(reply_target, relocated_target);
    assert_ne!(relocated_point.code_unit_ordinal, 0);
    let root_points = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    );
    let helper_point = root_points
        .into_iter()
        .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset == 27)
        .expect("generated helper must have a validated root safe point");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint {
                target: DebuggerGeneratedBreakpointTarget {
                    program,
                    generated_line: 0,
                    generated_column_utf16: 0,
                }
            }
        ),
        DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
            target: DebuggerGeneratedBreakpointTarget {
                program,
                generated_line: 0,
                generated_column_utf16: 0,
            },
            safe_point: None,
        }),
        "the emitted helper body must not bind as a user breakpoint"
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmRootSafePointBreakpoint {
                safe_point: helper_point,
            },
        ),
        DebuggerReply::RootSafePointBreakpointArmed {
            safe_point: helper_point
        }
    );
    await_paused_execution(&mut debugger, program, helper_point);
    let reply = debugger_request(
        &mut debugger,
        DebuggerRequest::GetGeneratedPausedLocation { program },
    );
    let DebuggerReply::GeneratedPausedLocation(DebuggerGeneratedPausedLocation {
        safe_point,
        location: Some(location),
        ..
    }) = reply
    else {
        panic!("generated helper pause must have only its generated coordinate: {reply:?}")
    };
    assert_eq!(safe_point, helper_point);
    assert_eq!(location.original, None);
    drop(debugger);
    launcher.shutdown();
    fixture.join().unwrap();
}

#[test]
fn public_relocated_decorator_breakpoint_pauses_at_its_original_token() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_decorated_generated_document(listener);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            emitted_blue_ts_classic: true,
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    let manifest =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        });
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
        }
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
    let (generated_line, generated_column_utf16, original_line, original_column_utf16) =
        relocated_decorator_coordinates();
    let target = DebuggerGeneratedBreakpointTarget {
        program,
        generated_line,
        generated_column_utf16,
    };
    let binding = debugger_request(
        &mut debugger,
        DebuggerRequest::ResolveGeneratedBreakpoint { target },
    );
    let DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
        target: returned,
        safe_point: Some(safe_point),
    }) = binding
    else {
        panic!("relocated decorator must have its own safe point: {binding:?}");
    };
    assert_eq!(returned, target);
    assert_ne!(safe_point.code_unit_ordinal, 0);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmGeneratedBreakpoint { target }
        ),
        DebuggerReply::NestedSafePointBreakpointArmed { safe_point }
    );
    let (frame, paused) = await_nested_paused_execution(&mut debugger, program);
    assert_eq!(paused, safe_point);
    let DebuggerReply::Stack(stack) = debugger_request(
        &mut debugger,
        DebuggerRequest::GetStack {
            program,
            frame: Some(frame),
            max_frames: 3,
        },
    ) else {
        panic!("the relocated decorator must retain its complete three-frame stack");
    };
    assert_eq!(stack.safe_points.len(), 3);
    assert_eq!(stack.safe_points[0], safe_point);
    assert_ne!(stack.safe_points[1].code_unit_ordinal, 0);
    assert_eq!(stack.safe_points[2].code_unit_ordinal, 0);
    assert!(!stack.stack_truncated);
    let DebuggerReply::Stack(truncated) = debugger_request(
        &mut debugger,
        DebuggerRequest::GetStack {
            program,
            frame: Some(frame),
            max_frames: 2,
        },
    ) else {
        panic!("the public stack must support a bounded two-frame projection");
    };
    assert_eq!(truncated.safe_points, stack.safe_points[..2]);
    assert!(truncated.stack_truncated);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetGeneratedPausedLocation { program },
        ),
        DebuggerReply::GeneratedPausedLocation(DebuggerGeneratedPausedLocation {
            program,
            safe_point,
            location: Some(DebuggerGeneratedPausedSourceLocation {
                generated_line,
                generated_column_utf16,
                original: Some(DebuggerGeneratedOriginalPosition {
                    source_index: 0,
                    line: original_line,
                    column_utf16: original_column_utf16,
                }),
            }),
        })
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepNestedInstruction { frame }
        ),
        DebuggerReply::NestedStepRequested { frame }
    );
    let (stepped_frame, stepped) = await_nested_paused_execution(&mut debugger, program);
    assert_eq!(stepped_frame, frame);
    assert_ne!(stepped, safe_point);
    let DebuggerReply::Stack(stepped_stack) = debugger_request(
        &mut debugger,
        DebuggerRequest::GetStack {
            program,
            frame: Some(frame),
            max_frames: 3,
        },
    ) else {
        panic!("one nested instruction step must retain all three paused frames");
    };
    assert_eq!(stepped_stack.safe_points.len(), 3);
    assert_eq!(stepped_stack.safe_points[0], stepped);
    assert_eq!(stepped_stack.safe_points[1], stack.safe_points[1]);
    assert_eq!(stepped_stack.safe_points[2], stack.safe_points[2]);
    assert!(!stepped_stack.stack_truncated);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetGeneratedPausedLocation { program },
        ),
        DebuggerReply::GeneratedPausedLocation(DebuggerGeneratedPausedLocation {
            program,
            safe_point: stepped,
            location: None,
        }),
        "an interior nested instruction must not inherit the decorator source"
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResumeNestedExecution { frame }
        ),
        DebuggerReply::NestedResumeRequested { frame }
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match debugger_request(
            &mut debugger,
            DebuggerRequest::GetExecutionState { program },
        ) {
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::Paused { safe_point },
                ..
            } if safe_point.code_unit_ordinal == 0 => break,
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::NestedResuming { frame: active },
                ..
            } if active == frame && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            reply => panic!("decorator's three-frame continuation must rejoin its root: {reply:?}"),
        }
    }
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    await_completed_execution(&mut debugger, program);
    drop(debugger);
    launcher.shutdown();
    fixture.join().unwrap();
}

#[test]
fn public_legacy_parameter_helper_binds_only_its_source_anchored_call() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/legacy_parameter_metadata.ts"
    ))
    .replace("console.log(\"method\");", "")
    .replace("console.log(\"parameter\", index);", "")
    .replace(
        "console.log(new Example().method(41));",
        "globalThis.legacyParameterResult = new Example().method(41);",
    );
    let module = "memory:///main.ts";
    let compilation = blueice_bluets::compile(
        module,
        &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(
            module,
            source.clone(),
        )]),
        blueice_bluets::CompilerOptions {
            source_map: true,
            inline_sources: true,
            experimental_decorators: true,
            emit_decorator_metadata: true,
            ..Default::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    let position = |text: &str, byte: usize| {
        let prefix = &text[..byte];
        (
            prefix
                .bytes()
                .filter(|character| *character == b'\n')
                .count() as u32,
            prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
        )
    };
    let generated = &artifact.javascript;
    let parameter_call = generated.find("__bluetsParam(0, parameterDecor)").unwrap();
    let parameter_target = position(generated, parameter_call);
    let original_parameter = source.find("@parameterDecor").unwrap() + 1;
    let original = position(&source, original_parameter);
    let map = artifact.source_map.as_ref().unwrap();
    let mapped = map
        .original_position_at(
            generated,
            parameter_target.0 as usize,
            parameter_target.1 as usize,
            200_000,
            16 * 1024 * 1024,
        )
        .unwrap()
        .unwrap();
    assert_eq!((mapped.line as u32, mapped.column_utf16 as u32), original);
    let unmapped = [
        position(generated, generated.find("var __bluetsParam").unwrap()),
        position(
            generated,
            generated.find("__bluetsMetadata(\"design:type\"").unwrap(),
        ),
    ];
    for (line, column) in unmapped {
        assert_eq!(
            map.original_position_at(
                generated,
                line as usize,
                column as usize,
                200_000,
                16 * 1024 * 1024,
            )
            .unwrap(),
            None,
        );
    }

    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_decorated_generated_document_with_source(listener, source);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            emitted_blue_ts_classic_legacy_metadata: true,
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    let manifest =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        });
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: manifest,
        }
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
    for (generated_line, generated_column_utf16) in unmapped {
        let target = DebuggerGeneratedBreakpointTarget {
            program,
            generated_line,
            generated_column_utf16,
        };
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ResolveGeneratedBreakpoint { target },
            ),
            DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
                target,
                safe_point: None,
            })
        );
    }
    let target = DebuggerGeneratedBreakpointTarget {
        program,
        generated_line: parameter_target.0,
        generated_column_utf16: parameter_target.1,
    };
    let binding = debugger_request(
        &mut debugger,
        DebuggerRequest::ResolveGeneratedBreakpoint { target },
    );
    let DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
        safe_point: Some(safe_point),
        ..
    }) = binding
    else {
        panic!("legacy parameter helper call must bind exactly: {binding:?}")
    };
    assert_eq!(safe_point.code_unit_ordinal, 0);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmGeneratedBreakpoint { target },
        ),
        DebuggerReply::RootSafePointBreakpointArmed { safe_point }
    );
    await_paused_execution(&mut debugger, program, safe_point);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetGeneratedPausedLocation { program },
        ),
        DebuggerReply::GeneratedPausedLocation(DebuggerGeneratedPausedLocation {
            program,
            safe_point,
            location: Some(DebuggerGeneratedPausedSourceLocation {
                generated_line: parameter_target.0,
                generated_column_utf16: parameter_target.1,
                original: Some(DebuggerGeneratedOriginalPosition {
                    source_index: 0,
                    line: original.0,
                    column_utf16: original.1,
                }),
            }),
        })
    );
    let DebuggerReply::Stack(stack) = debugger_request(
        &mut debugger,
        DebuggerRequest::GetStack {
            program,
            frame: None,
            max_frames: 1,
        },
    ) else {
        panic!("legacy parameter helper call must retain its root stack");
    };
    assert_eq!(stack.safe_points, vec![safe_point]);
    assert!(!stack.stack_truncated);
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepRootInstruction { program },
        ),
        DebuggerReply::ExecutionStepRequested { .. }
    ));
    let stepped = await_step_paused_execution(&mut debugger, program, safe_point);
    let DebuggerReply::Stack(stepped_stack) = debugger_request(
        &mut debugger,
        DebuggerRequest::GetStack {
            program,
            frame: None,
            max_frames: 1,
        },
    ) else {
        panic!("legacy parameter helper step must retain its root stack");
    };
    assert_eq!(stepped_stack.safe_points, vec![stepped]);
    assert!(!stepped_stack.stack_truncated);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::GetGeneratedPausedLocation { program }
        ),
        DebuggerReply::GeneratedPausedLocation(DebuggerGeneratedPausedLocation {
            program,
            safe_point: stepped,
            location: None,
        }),
        "interior legacy parameter helper step must not inherit its source anchor"
    );
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    await_completed_execution(&mut debugger, program);
    drop(debugger);
    launcher.shutdown();
    fixture.join().unwrap();
}

#[test]
fn public_compacted_and_erased_decorator_positions_keep_their_original_tokens() {
    for (case, original) in [
        (
            "compacted-parentheses",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../bluets/tests/fixtures/k11_source_maps/decorated_parenthesized.ts"
            )),
        ),
        (
            "erased-assertion",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../bluets/tests/fixtures/k11_source_maps/decorated_erased_assertion.ts"
            )),
        ),
    ] {
        let source = original.replace(
            "console.log(new Example().method());",
            "const finalAnswer: number = new Example().method();",
        );
        let (generated_line, generated_column_utf16, original_line, original_column_utf16) =
            relocated_decorator_coordinates_for_source(&source);
        let gatekeeper_socket = clearing_gatekeeper();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let fixture = serve_decorated_generated_document_with_source(listener, source);
        let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
            &gatekeeper_socket,
            StaticMetadataPolicy {
                emitted_blue_ts_classic: true,
                generated_breakpoint: true,
                generated_paused_location: true,
                ..Default::default()
            },
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
            "{case} must negotiate the exact generated debugger grants"
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
            generated_line,
            generated_column_utf16,
        };
        let binding = debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint { target },
        );
        let DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
            safe_point: Some(safe_point),
            ..
        }) = binding
        else {
            panic!("{case} must bind its relocated token exactly: {binding:?}")
        };
        assert_ne!(safe_point.code_unit_ordinal, 0, "{case}");
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ArmGeneratedBreakpoint { target },
            ),
            DebuggerReply::NestedSafePointBreakpointArmed { safe_point },
            "{case} must arm only the verified nested safe point"
        );
        let (frame, paused) = await_nested_paused_execution(&mut debugger, program);
        assert_eq!(paused, safe_point, "{case}");
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::GetGeneratedPausedLocation { program },
            ),
            DebuggerReply::GeneratedPausedLocation(DebuggerGeneratedPausedLocation {
                program,
                safe_point,
                location: Some(DebuggerGeneratedPausedSourceLocation {
                    generated_line,
                    generated_column_utf16,
                    original: Some(DebuggerGeneratedOriginalPosition {
                        source_index: 0,
                        line: original_line,
                        column_utf16: original_column_utf16,
                    }),
                }),
            }),
            "{case} must retain the original decorator token after relocation"
        );
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ResumeNestedExecution { frame },
            ),
            DebuggerReply::NestedResumeRequested { frame }
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match debugger_request(
                &mut debugger,
                DebuggerRequest::GetExecutionState { program },
            ) {
                DebuggerReply::ExecutionState {
                    state: DebuggerExecutionState::Paused { safe_point },
                    ..
                } if safe_point.code_unit_ordinal == 0 => break,
                DebuggerReply::ExecutionState {
                    state: DebuggerExecutionState::NestedResuming { frame: active },
                    ..
                } if active == frame && Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                reply => panic!("{case} must rejoin the paused root: {reply:?}"),
            }
        }
        assert_eq!(
            debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
            DebuggerReply::ExecutionResumed { program }
        );
        await_completed_execution(&mut debugger, program);
        drop(debugger);
        launcher.shutdown();
        fixture.join().unwrap();
    }
}
