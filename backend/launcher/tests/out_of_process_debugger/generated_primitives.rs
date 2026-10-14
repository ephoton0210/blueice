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

fn serve_primitive_document(
    listener: TcpListener,
    source: String,
    expected: u32,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            "<main>primitive map</main><script>var console = {{ log: function(value) {{ if (value !== {expected}) throw 'wrong primitive result'; }} }};</script><script type=\"application/x-blueice-typescript-module\">{source}</script>"
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

fn run_primitive_row(
    name: &str,
    frozen: &str,
    expected: u32,
    entry_token: &str,
    paused_token: &str,
) {
    let source = frozen.to_string();
    let module = "memory:///k11-primitive.ts";
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
    assert!(
        !compilation.has_errors(),
        "{name}: {:#?}",
        compilation.diagnostics
    );
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    assert_eq!(
        artifact.source_map.as_ref().unwrap().sources_content,
        vec![frozen.to_owned()],
        "{name} exact fixture must be the full source-map content"
    );
    let generated = &artifact.javascript;
    let generated_console = coordinate(generated, generated.rfind("console.log(").unwrap());
    let original_console = coordinate(&source, source.find("console.log(").unwrap());
    let mapped = artifact
        .source_map
        .as_ref()
        .unwrap()
        .original_position_at(
            generated,
            generated_console.0 as usize,
            generated_console.1 as usize,
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
        (0, original_console.0, original_console.1),
        "{name} emitted console token must map to the frozen source prefix"
    );
    let generated_token = coordinate(generated, generated.find(entry_token).unwrap());
    let original_token = coordinate(&source, source.find(entry_token).unwrap());
    let mapped_entry = artifact
        .source_map
        .as_ref()
        .unwrap()
        .original_position_at(
            generated,
            generated_token.0 as usize,
            generated_token.1 as usize,
            200_000,
            16 * 1024 * 1024,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            mapped_entry.source_index,
            mapped_entry.line as u32,
            mapped_entry.column_utf16 as u32,
        ),
        (0, original_token.0, original_token.1),
        "{name} module entry must map to its frozen source token"
    );
    let generated_paused = coordinate(generated, generated.find(paused_token).unwrap());
    let original_paused = coordinate(&source, source.find(paused_token).unwrap());
    let mapped_paused = artifact
        .source_map
        .as_ref()
        .unwrap()
        .original_position_at(
            generated,
            generated_paused.0 as usize,
            generated_paused.1 as usize,
            200_000,
            16 * 1024 * 1024,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            mapped_paused.source_index,
            mapped_paused.line as u32,
            mapped_paused.column_utf16 as u32,
        ),
        (0, original_paused.0, original_paused.1),
        "{name} actual paused instruction must map to its frozen token"
    );

    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_primitive_document(listener, source, expected);
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
    let inventory = debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm });
    let DebuggerReply::Programs(programs) = inventory else {
        panic!("{name} must install the page loader and emitted module: {inventory:?}")
    };
    assert_eq!(programs.len(), 2, "{name}");
    let program = programs[1];
    let target = DebuggerGeneratedBreakpointTarget {
        program,
        generated_line: generated_token.0,
        generated_column_utf16: generated_token.1,
    };
    let armed = debugger_request(
        &mut debugger,
        DebuggerRequest::ArmGeneratedBreakpoint { target },
    );
    let DebuggerReply::RootSafePointBreakpointArmed { safe_point } = armed else {
        panic!("{name} mapped module entry must arm before its evaluation: {armed:?}")
    };
    assert_eq!(safe_point.code_unit_ordinal, 0, "{name}");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint { target }
        ),
        DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
            target,
            safe_point: Some(safe_point),
        })
    );
    if name == "erased" {
        let hoisted = coordinate(generated, generated.find("export function double").unwrap());
        let hoisted_target = DebuggerGeneratedBreakpointTarget {
            program,
            generated_line: hoisted.0,
            generated_column_utf16: hoisted.1,
        };
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ResolveGeneratedBreakpoint {
                    target: hoisted_target,
                }
            ),
            DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
                target: hoisted_target,
                safe_point: None,
            }),
            "a hoisted function declaration must not borrow the later evaluation entry"
        );
    }
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
                generated_line: generated_paused.0,
                generated_column_utf16: generated_paused.1,
                original: Some(DebuggerGeneratedOriginalPosition {
                    source_index: 0,
                    line: original_paused.0,
                    column_utf16: original_paused.1,
                }),
            }),
        }),
        "{name} paused token must reverse-project without a nearest-token guess"
    );
    let DebuggerReply::Stack(stack) = debugger_request(
        &mut debugger,
        DebuggerRequest::GetStack {
            program,
            frame: None,
            max_frames: 1,
        },
    ) else {
        panic!("{name} mapped module entry must expose its root stack");
    };
    assert_eq!(stack.safe_points, vec![safe_point], "{name}");
    assert!(!stack.stack_truncated, "{name}");
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepGeneratedInstruction {
                target: DebuggerGeneratedStepTarget::Single {
                    program,
                    frame: None,
                },
            },
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
        panic!("{name} first root instruction step must retain its stack");
    };
    assert_eq!(stepped_stack.safe_points, vec![stepped], "{name}");
    assert!(!stepped_stack.stack_truncated, "{name}");
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
        "{name} interior step must not inherit the mapped module-entry token"
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
fn public_frozen_basic_erasure_and_astral_prefixes_bind_and_project() {
    for (name, source, expected, entry_token, paused_token) in [
        (
            "basic",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../bluets/tests/fixtures/k11_source_maps/basic.ts"
            )),
            3,
            "export const alpha",
            "export const alpha",
        ),
        (
            "erased",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../bluets/tests/fixtures/k11_source_maps/erased.ts"
            )),
            42,
            "export const result",
            "double(21)",
        ),
        (
            "astral",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../bluets/tests/fixtures/k11_source_maps/crlf_astral.ts"
            )),
            2,
            "export const marker",
            "export const marker",
        ),
    ] {
        run_primitive_row(name, source, expected, entry_token, paused_token);
    }
}
