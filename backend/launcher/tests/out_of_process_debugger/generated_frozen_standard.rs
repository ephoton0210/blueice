// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::debugger::{
    DebuggerGeneratedBreakpoint, DebuggerGeneratedBreakpointTarget,
    DebuggerGeneratedOriginalPosition, DebuggerGeneratedPausedLocation,
    DebuggerGeneratedPausedSourceLocation, DebuggerGeneratedStepTarget,
};

fn coordinate(source: &str, byte: usize) -> (u32, u32) {
    let prefix = &source[..byte];
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count() as u32,
        prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
    )
}

fn serve_frozen_document(
    listener: TcpListener,
    source: &'static str,
    composed: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let loader = if composed {
            "var expected = ['inner', 'outer', 'class', 42]; var next = 0; var console = { log: function(value) { if (value !== expected[next++]) throw 'wrong composed result'; } };"
        } else {
            "var console = { log: function(value) { if (value !== 42) throw 'wrong decorator result'; } };"
        };
        let body = format!(
            "<main>frozen standard decorator</main><script>{loader}</script><script type=\"application/x-blueice-typescript-module\">{source}</script>"
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
    })
}

#[test]
fn public_exact_frozen_standard_decorators_keep_relocated_tokens() {
    for (case, source, list, token, original_marker, expected_frames) in [
        (
            "standard-method",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../bluets/tests/fixtures/k11_source_maps/decorated.ts"
            )),
            "_method_decorators = [",
            "decoSite",
            "@decoSite",
            3,
        ),
        (
            "compacted-relocation",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../bluets/tests/fixtures/k11_source_maps/decorated_parenthesized.ts"
            )),
            "_method_decorators = [",
            "decoSite",
            "@(  decoSite",
            3,
        ),
        (
            "erased-relocation",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../bluets/tests/fixtures/k11_source_maps/decorated_erased_assertion.ts"
            )),
            "_method_decorators = [",
            "decoSite",
            "@(decoSite",
            3,
        ),
        (
            "composed-outer",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../bluets/tests/fixtures/k11_source_maps/decorated_multiple.ts"
            )),
            "_method_decorators = [",
            "outer",
            "@outer",
            3,
        ),
        (
            "composed-inner",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../bluets/tests/fixtures/k11_source_maps/decorated_multiple.ts"
            )),
            "_method_decorators = [",
            "inner",
            "@inner",
            3,
        ),
        (
            "composed-class",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../bluets/tests/fixtures/k11_source_maps/decorated_multiple.ts"
            )),
            "_classDecorators = [",
            "classDecor",
            "@classDecor",
            2,
        ),
    ] {
        let module = "memory:///k11-standard-decorator.ts";
        let compilation = blueice_bluets::compile(
            module,
            &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(module, source)]),
            blueice_bluets::CompilerOptions {
                source_map: true,
                inline_sources: true,
                ..Default::default()
            },
        );
        assert!(
            !compilation.has_errors(),
            "{case}: {:#?}",
            compilation.diagnostics
        );
        let output = compilation.output.unwrap();
        let artifact = &output.artifacts[module];
        let generated = &artifact.javascript;
        let map = artifact.source_map.as_ref().unwrap();
        assert_eq!(map.sources_content, vec![source.to_owned()], "{case}");

        let list_start = generated.find(list).unwrap() + list.len();
        let generated_byte = list_start + generated[list_start..].find(token).unwrap();
        let generated_position = coordinate(generated, generated_byte);
        let original_byte =
            source.find(original_marker).unwrap() + original_marker.find(token).unwrap();
        let original_position = coordinate(source, original_byte);
        let mapped = map
            .original_position_at(
                generated,
                generated_position.0 as usize,
                generated_position.1 as usize,
                200_000,
                16 * 1024 * 1024,
            )
            .unwrap()
            .unwrap_or_else(|| {
                panic!("{case} generated {token} at {generated_position:?} is unmapped")
            });
        assert_eq!(
            (
                mapped.source_index,
                mapped.line as u32,
                mapped.column_utf16 as u32
            ),
            (0, original_position.0, original_position.1),
            "{case} exact frozen decorator location"
        );
        let helper_position =
            coordinate(generated, generated.find("var __bluetsEsDecorate").unwrap());
        assert_eq!(
            map.original_position_at(
                generated,
                helper_position.0 as usize,
                helper_position.1 as usize,
                200_000,
                16 * 1024 * 1024,
            )
            .unwrap(),
            None,
            "{case} helper definition has no user source"
        );

        let gatekeeper_socket = clearing_gatekeeper();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let fixture = serve_frozen_document(listener, source, case.starts_with("composed-"));
        let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
            &gatekeeper_socket,
            StaticMetadataPolicy {
                emitted_blue_ts_module: true,
                emitted_blue_ts_module_page_console_ambient: true,
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
            "{case} capability grant"
        );
        hold_next_document(&mut debugger, 1);
        navigate(&mut browser, &url);
        let realm = one_realm(debugger_request(
            &mut debugger,
            DebuggerRequest::ListPageRealms,
        ));
        let inventory = debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm });
        let DebuggerReply::Programs(programs) = inventory else {
            panic!("{case} expected loader and emitted module: {inventory:?}")
        };
        assert_eq!(programs.len(), 2, "{case}");
        let program = programs[1];
        let helper = DebuggerGeneratedBreakpointTarget {
            program,
            generated_line: helper_position.0,
            generated_column_utf16: helper_position.1,
        };
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ResolveGeneratedBreakpoint { target: helper }
            ),
            DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
                target: helper,
                safe_point: None,
            }),
            "{case} helper body must stay unbound"
        );
        let target = DebuggerGeneratedBreakpointTarget {
            program,
            generated_line: generated_position.0,
            generated_column_utf16: generated_position.1,
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
            panic!("{case} frozen relocated token must bind: {binding:?}")
        };
        assert_ne!(safe_point.code_unit_ordinal, 0, "{case}");
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ArmGeneratedBreakpoint { target }
            ),
            DebuggerReply::NestedSafePointBreakpointArmed { safe_point },
            "{case}"
        );
        let (frame, paused) = await_nested_paused_execution(&mut debugger, program);
        assert_eq!(paused, safe_point, "{case}");
        let DebuggerReply::Stack(stack) = debugger_request(
            &mut debugger,
            DebuggerRequest::GetStack {
                program,
                frame: Some(frame),
                max_frames: 3,
            },
        ) else {
            panic!("{case} must expose the complete decorator stack")
        };
        assert_eq!(stack.safe_points.len(), expected_frames, "{case}");
        assert_eq!(stack.safe_points[0], safe_point, "{case}");
        assert_eq!(
            stack.safe_points.last().unwrap().code_unit_ordinal,
            0,
            "{case}"
        );
        assert!(!stack.stack_truncated, "{case}");
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::GetGeneratedPausedLocation { program }
            ),
            DebuggerReply::GeneratedPausedLocation(DebuggerGeneratedPausedLocation {
                program,
                safe_point,
                location: Some(DebuggerGeneratedPausedSourceLocation {
                    generated_line: generated_position.0,
                    generated_column_utf16: generated_position.1,
                    original: Some(DebuggerGeneratedOriginalPosition {
                        source_index: 0,
                        line: original_position.0,
                        column_utf16: original_position.1,
                    }),
                }),
            }),
            "{case} must project the frozen original position"
        );
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::StepGeneratedInstruction {
                    target: DebuggerGeneratedStepTarget::Single {
                        program,
                        frame: Some(frame)
                    },
                }
            ),
            DebuggerReply::NestedStepRequested { frame },
            "{case}"
        );
        let (stepped_frame, stepped) = await_nested_paused_execution(&mut debugger, program);
        assert_eq!(stepped_frame, frame, "{case}");
        assert_ne!(stepped, safe_point, "{case}");
        let DebuggerReply::Stack(stepped_stack) = debugger_request(
            &mut debugger,
            DebuggerRequest::GetStack {
                program,
                frame: Some(frame),
                max_frames: 3,
            },
        ) else {
            panic!("{case} one nested step must retain its stack")
        };
        assert_eq!(stepped_stack.safe_points.len(), expected_frames, "{case}");
        assert_eq!(stepped_stack.safe_points[0], stepped, "{case}");
        assert_eq!(
            stepped_stack.safe_points[1..],
            stack.safe_points[1..],
            "{case}"
        );
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
            "{case} interior step must not inherit the decorator source"
        );
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ResumeNestedExecution { frame }
            ),
            DebuggerReply::NestedResumeRequested { frame },
            "{case}"
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
                reply => panic!("{case} must rejoin its root: {reply:?}"),
            }
        }
        assert_eq!(
            debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
            DebuggerReply::ExecutionResumed { program },
            "{case}"
        );
        await_completed_execution(&mut debugger, program);
        drop(debugger);
        launcher.shutdown();
        fixture.join().unwrap();
    }
}
