// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::debugger::{
    DebuggerGeneratedBreakpoint, DebuggerGeneratedBreakpointTarget,
    DebuggerGeneratedOriginalPosition, DebuggerGeneratedPausedLocation,
    DebuggerGeneratedPausedSourceLocation,
};

fn composed_source() -> String {
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/decorated_multiple.ts"
    ))
    .replace("console.log(\"outer\");", "")
    .replace("console.log(\"inner\");", "")
    .replace("console.log(\"class\");", "")
    .replace(
        "console.log(new Example().method());",
        "globalThis.composedDecoratorResult = new Example().method();",
    )
}

fn serve_composed_document(listener: TcpListener, source: String) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            "<main>composed</main><script type=\"application/x-blueice-typescript\">{source}</script>"
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
fn public_composed_class_and_two_member_decorators_keep_each_relocated_token() {
    let source = composed_source();
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
            ..Default::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    let generated = &artifact.javascript;
    let map = artifact.source_map.as_ref().unwrap();
    let method_list =
        generated.find("_method_decorators = [").unwrap() + "_method_decorators = [".len();
    let class_list = generated.find("_classDecorators = [").unwrap() + "_classDecorators = [".len();
    let unmapped = position(generated, generated.find("var __bluetsEsDecorate").unwrap());
    assert_eq!(
        map.original_position_at(
            generated,
            unmapped.0 as usize,
            unmapped.1 as usize,
            200_000,
            16 * 1024 * 1024,
        )
        .unwrap(),
        None,
    );
    for (case, generated_start, token, original_token) in [
        ("outer-member", method_list, "outer", "@outer"),
        ("inner-member", method_list, "inner", "@inner"),
        ("class", class_list, "classDecor", "@classDecor"),
    ] {
        let generated_byte = generated_start + generated[generated_start..].find(token).unwrap();
        let generated_position = position(generated, generated_byte);
        let original_position = position(&source, source.find(original_token).unwrap() + 1);
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
            "{case} must retain its original decorator token"
        );

        let gatekeeper_socket = clearing_gatekeeper();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let fixture = serve_composed_document(listener, source.clone());
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
            "{case} must retain independent generated debugger grants"
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
        let helper_target = DebuggerGeneratedBreakpointTarget {
            program,
            generated_line: unmapped.0,
            generated_column_utf16: unmapped.1,
        };
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ResolveGeneratedBreakpoint {
                    target: helper_target,
                }
            ),
            DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
                target: helper_target,
                safe_point: None,
            }),
            "{case} must not bind generated helper body"
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
            panic!("{case} must bind an exact relocated safe point: {binding:?}")
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
            "{case} must project its own original decorator token"
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
                reply => panic!("{case} must rejoin the root after nested resume: {reply:?}"),
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
