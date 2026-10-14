// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::debugger::{
    DebuggerGeneratedBreakpoint, DebuggerGeneratedBreakpointTarget,
    DebuggerGeneratedOriginalPosition, DebuggerGeneratedPausedLocation,
    DebuggerGeneratedPausedSourceLocation,
};

fn es5_spread_source() -> String {
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/object_spread_helper.ts"
    ))
    .replace(
        "console.log(merged.a + merged.b);",
        "if (merged.a + merged.b !== 3) throw 'ES5 object spread result';",
    )
}

fn serve_es5_document(listener: TcpListener, source: String) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            "<main>es5-spread</main><script type=\"application/x-blueice-typescript\">{source}</script>"
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
fn public_es5_object_spread_helper_call_maps_but_definition_is_unbound() {
    let source = es5_spread_source();
    let module = "memory:///main.ts";
    let compilation = blueice_bluets::compile(
        module,
        &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(
            module,
            source.clone(),
        )]),
        blueice_bluets::CompilerOptions {
            target: blueice_bluets::EcmaTarget::Es5,
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
    let definition = position(
        generated,
        generated
            .find("function __blueice_target_object_properties_")
            .unwrap(),
    );
    let body = generated.find("var merged").unwrap();
    let call = position(
        generated,
        body + generated[body..]
            .find("__blueice_target_object_properties_")
            .unwrap(),
    );
    let original = position(&source, source.find("merged").unwrap() + "merged".len());
    assert_eq!(
        map.original_position_at(
            generated,
            definition.0 as usize,
            definition.1 as usize,
            200_000,
            16 * 1024 * 1024,
        )
        .unwrap(),
        None,
    );
    let mapped = map
        .original_position_at(
            generated,
            call.0 as usize,
            call.1 as usize,
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
        (0, original.0, original.1)
    );

    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = serve_es5_document(listener, source);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            emitted_blue_ts_classic_es5: true,
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
    let definition_target = DebuggerGeneratedBreakpointTarget {
        program,
        generated_line: definition.0,
        generated_column_utf16: definition.1,
    };
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint {
                target: definition_target,
            }
        ),
        DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
            target: definition_target,
            safe_point: None,
        }),
        "the generated ES5 helper definition must remain unbound"
    );
    let target = DebuggerGeneratedBreakpointTarget {
        program,
        generated_line: call.0,
        generated_column_utf16: call.1,
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
        panic!("source-anchored ES5 helper call must bind: {binding:?}")
    };
    assert_eq!(safe_point.code_unit_ordinal, 0);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmGeneratedBreakpoint { target }
        ),
        DebuggerReply::RootSafePointBreakpointArmed { safe_point }
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
                generated_line: call.0,
                generated_column_utf16: call.1,
                original: Some(DebuggerGeneratedOriginalPosition {
                    source_index: 0,
                    line: original.0,
                    column_utf16: original.1,
                }),
            }),
        })
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
