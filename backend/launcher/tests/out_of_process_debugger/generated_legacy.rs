// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::debugger::{
    DebuggerGeneratedBreakpoint, DebuggerGeneratedBreakpointTarget,
    DebuggerGeneratedOriginalPosition, DebuggerGeneratedPausedLocation,
    DebuggerGeneratedPausedSourceLocation,
};

fn legacy_class_member_source() -> String {
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/legacy_decorated.ts"
    ))
    .replace("console.log(\"class\");", "")
    .replace("console.log(\"method\");", "")
    .replace(
        "console.log(new Example().method());",
        "globalThis.legacyClassMemberResult = new Example().method();",
    )
}

fn serve_legacy_document(listener: TcpListener, source: String) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            "<main>legacy</main><script type=\"application/x-blueice-typescript\">{source}</script>"
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

fn position(text: &str, byte: usize) -> (u32, u32) {
    let prefix = &text[..byte];
    (
        prefix
            .bytes()
            .filter(|character| *character == b'\n')
            .count() as u32,
        prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
    )
}

#[test]
fn public_legacy_class_and_member_helper_calls_bind_without_metadata() {
    let source = legacy_class_member_source();
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
            ..Default::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    let generated = &artifact.javascript;
    let map = artifact.source_map.as_ref().unwrap();
    let unmapped = [
        position(generated, generated.find("var __bluetsDecorate").unwrap()),
        position(generated, generated.find("], Example.prototype").unwrap()),
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
    for (case, generated_token, original_token, anchor_at_token_end) in [
        ("member", "__bluetsDecorate([methodDecor", "method()", false),
        (
            "class",
            "__bluetsDecorate([classDecor",
            "class Example",
            true,
        ),
    ] {
        let generated_position = position(generated, generated.find(generated_token).unwrap());
        let original_byte = source.find(original_token).unwrap()
            + if anchor_at_token_end {
                original_token.len()
            } else {
                0
            };
        let original_position = position(&source, original_byte);
        let mapped = map
            .original_position_at(
                generated,
                generated_position.0 as usize,
                generated_position.1 as usize,
                200_000,
                16 * 1024 * 1024,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                mapped.source_index,
                mapped.line as u32,
                mapped.column_utf16 as u32
            ),
            (0, original_position.0, original_position.1),
            "{case} helper call must retain its measured source anchor"
        );

        let gatekeeper_socket = clearing_gatekeeper();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let fixture = serve_legacy_document(listener, source.clone());
        let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
            &gatekeeper_socket,
            StaticMetadataPolicy {
                emitted_blue_ts_classic_legacy_decorators: true,
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
            "{case} owner-selected profile must not expand debugger grants"
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
                    DebuggerRequest::ResolveGeneratedBreakpoint { target }
                ),
                DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
                    target,
                    safe_point: None,
                }),
                "{case} must not bind generated helper scaffolding"
            );
        }
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
            panic!("{case} helper call must bind an exact safe point: {binding:?}")
        };
        assert_eq!(safe_point.code_unit_ordinal, 0, "{case}");
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ArmGeneratedBreakpoint { target }
            ),
            DebuggerReply::RootSafePointBreakpointArmed { safe_point },
            "{case}"
        );
        await_paused_execution(&mut debugger, program, safe_point);
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
            "{case} must project its exact source anchor"
        );
        let DebuggerReply::Stack(stack) = debugger_request(
            &mut debugger,
            DebuggerRequest::GetStack {
                program,
                frame: None,
                max_frames: 1,
            },
        ) else {
            panic!("{case} mapped helper call must retain its root stack");
        };
        assert_eq!(stack.safe_points, vec![safe_point], "{case}");
        assert!(!stack.stack_truncated, "{case}");
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
            panic!("{case} first helper-call instruction step must retain its root stack");
        };
        assert_eq!(stepped_stack.safe_points, vec![stepped], "{case}");
        assert!(!stepped_stack.stack_truncated, "{case}");
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
            "{case} interior helper-call step must not inherit the mapped source anchor"
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
}
