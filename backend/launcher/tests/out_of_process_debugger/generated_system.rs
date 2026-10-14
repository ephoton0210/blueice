// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::debugger::{DebuggerGeneratedBreakpoint, DebuggerGeneratedBreakpointTarget};

const PAGE_SYSTEM_LOADER: &str = r#"
    var console = { log: function(value) {
        if (value !== 42) throw 'System console result';
    } };
    var System = { register: function(dependencies, declare) {
        if (dependencies.length !== 0) throw 'unexpected System dependency';
        var exports = {};
        var body = declare(function(name, value) {
            exports[name] = value;
            return value;
        }, { id: 'main' });
        if (body.setters.length !== 0) throw 'unexpected System setter';
        body.execute();
        if (exports.answer !== 42) throw 'System export result';
    } };
"#;

fn system_source() -> String {
    let frozen = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/system_wrapper.ts"
    ));
    format!("{frozen}\ndeclare const console: {{ log(value: number): void }};\n")
}

fn serve_system_document(listener: TcpListener, source: String) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            "<main>system</main><script>{PAGE_SYSTEM_LOADER}</script><script type=\"application/x-blueice-typescript\">{source}</script>"
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
fn public_system_wrapper_and_foreign_loader_target_fail_closed() {
    let source = system_source();
    let module = "memory:///main.ts";
    let compilation = blueice_bluets::compile(
        module,
        &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(
            module,
            source.clone(),
        )]),
        blueice_bluets::CompilerOptions {
            module_kind: blueice_bluets::ModuleKind::System,
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
    let wrapper = position(generated, generated.find("System.register").unwrap());
    let call = position(generated, generated.rfind("console.log(").unwrap());
    let original = position(&source, source.find("console.log(answer)").unwrap());
    assert_eq!(
        map.original_position_at(
            generated,
            wrapper.0 as usize,
            wrapper.1 as usize,
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
    let fixture = serve_system_document(listener, source);
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            emitted_blue_ts_classic_system: true,
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
    let inventory = debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm });
    let DebuggerReply::Programs(programs) = inventory else {
        panic!("page loader and emitted System wrapper must both be installed: {inventory:?}")
    };
    assert_eq!(programs.len(), 2);
    let program = programs[1];
    let raw_target = safe_points(
        debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
        program,
    )
    .into_iter()
    .find(|point| point.code_unit_ordinal != 0)
    .expect("System execute body must have a nested safe point");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmNestedSafePointBreakpoint {
                safe_point: raw_target,
            }
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "debugger operation is not valid for the program execution state".to_string(),
        },
        "raw safe-point arming must not bypass System's bounded-frame refusal"
    );
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ListBreakpoints { realm }),
        DebuggerReply::Breakpoints(Vec::new()),
        "refused raw System arming must not retain a partial breakpoint"
    );
    let wrapper_target = DebuggerGeneratedBreakpointTarget {
        program,
        generated_line: wrapper.0,
        generated_column_utf16: wrapper.1,
    };
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedBreakpoint {
                target: wrapper_target,
            }
        ),
        DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
            target: wrapper_target,
            safe_point: None,
        }),
        "System.register scaffolding must not bind a user breakpoint"
    );
    let target = DebuggerGeneratedBreakpointTarget {
        program,
        generated_line: call.0,
        generated_column_utf16: call.1,
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
        "a mapped System execute token cannot promise a pause across a foreign loader"
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmGeneratedBreakpoint { target }
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidSafePoint,
            message: "generated position has no exact executable safe point".to_string(),
        }
    );
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ListBreakpoints { realm }),
        DebuggerReply::Breakpoints(Vec::new()),
        "refused generated System arming must not retain a partial breakpoint"
    );
    drop(debugger);
    launcher.shutdown();
    fixture.join().unwrap();
}
