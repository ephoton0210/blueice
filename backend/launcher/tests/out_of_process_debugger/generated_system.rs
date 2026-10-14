// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::debugger::{
    DebuggerGeneratedBreakpoint, DebuggerGeneratedBreakpointTarget,
    DebuggerGeneratedSystemBreakpoint, DebuggerGeneratedSystemBreakpointTarget,
};

const PAGE_SYSTEM_LOADER: &str = r#"
    var systemLogCount = 0;
    var systemRegisterCount = 0;
    var console = { log: function(value) {
        systemLogCount++;
        if (value !== 42) throw 'System console result';
    } };
    var System = { register: function(dependencies, declare) {
        systemRegisterCount++;
        if (systemRegisterCount !== 1) throw 'System wrapper replay';
        if (dependencies.length !== 0) throw 'unexpected System dependency';
        var exports = {};
        var body = declare(function(name, value) {
            exports[name] = value;
            return value;
        }, { id: 'main' });
        if (body.setters.length !== 0) throw 'unexpected System setter';
        body.execute();
        if (systemLogCount !== 1) throw 'System execute replay';
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
fn public_system_requires_explicit_dual_program_breakpoint_and_stack_route() {
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
    let loader = programs[0];
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
    let system_wrapper = DebuggerGeneratedSystemBreakpointTarget {
        emitted: wrapper_target,
        loader,
    };
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResolveGeneratedSystemBreakpoint {
                target: system_wrapper,
            },
        ),
        DebuggerReply::GeneratedSystemBreakpoint(DebuggerGeneratedSystemBreakpoint {
            target: system_wrapper,
            safe_point: None,
        }),
        "explicit System lookup must keep the generated wrapper unbound"
    );
    let system_target = DebuggerGeneratedSystemBreakpointTarget {
        emitted: target,
        loader,
    };
    let binding = debugger_request(
        &mut debugger,
        DebuggerRequest::ResolveGeneratedSystemBreakpoint {
            target: system_target,
        },
    );
    let DebuggerReply::GeneratedSystemBreakpoint(DebuggerGeneratedSystemBreakpoint {
        safe_point: Some(bound),
        ..
    }) = binding
    else {
        panic!("mapped System execute token must bind exactly: {binding:?}")
    };
    assert_eq!(bound.program, program);
    assert_ne!(bound.code_unit_ordinal, 0);
    let expected_binding = DebuggerGeneratedSystemBreakpoint {
        target: system_target,
        safe_point: Some(bound),
    };
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmGeneratedSystemBreakpoint {
                target: system_wrapper,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidSafePoint,
            message: "generated System position has no exact executable safe point".to_string(),
        }
    );
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ListBreakpoints { realm }),
        DebuggerReply::Breakpoints(Vec::new())
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ArmGeneratedSystemBreakpoint {
                target: system_target,
            },
        ),
        DebuggerReply::GeneratedSystemBreakpointArmed(expected_binding)
    );
    let (frame, paused) = await_nested_paused_execution(&mut debugger, program);
    assert_eq!(paused, bound);
    let projection = debugger_request(
        &mut debugger,
        DebuggerRequest::GetGeneratedSystemStackLocations { frame, loader },
    );
    let DebuggerReply::GeneratedSystemStackLocations(projection) = projection else {
        panic!("explicit System stack must project all three frames: {projection:?}")
    };
    assert!(projection.is_well_formed());
    assert_eq!(projection.safe_points[0], bound);
    assert_eq!(projection.safe_points[1].program, loader);
    assert_eq!(projection.safe_points[2].program, program);
    assert_eq!(projection.locations[1], None);
    let location = projection.locations[0].expect("mapped System token has an exact location");
    assert_eq!(
        (location.generated_line, location.generated_column_utf16),
        call
    );
    let original_location = location
        .original
        .expect("mapped System token has an original");
    assert_eq!(
        (
            original_location.source_index,
            original_location.line,
            original_location.column_utf16
        ),
        (0, original.0, original.1)
    );
    assert_eq!(projection.locations[2], None);
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::StepNestedInstruction { frame },
        ),
        DebuggerReply::NestedStepRequested { frame }
    );
    let (stepped_frame, stepped) = await_nested_paused_execution(&mut debugger, program);
    assert_eq!(stepped_frame, frame);
    assert_ne!(stepped, bound);
    let stepped_projection = debugger_request(
        &mut debugger,
        DebuggerRequest::GetGeneratedSystemStackLocations { frame, loader },
    );
    let DebuggerReply::GeneratedSystemStackLocations(stepped_projection) = stepped_projection
    else {
        panic!("stepped System stack must remain exact: {stepped_projection:?}")
    };
    assert_eq!(stepped_projection.safe_points[0], stepped);
    assert_eq!(stepped_projection.safe_points[1], projection.safe_points[1]);
    assert_eq!(stepped_projection.safe_points[2], projection.safe_points[2]);
    assert_eq!(stepped_projection.locations[1], None);
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
            reply => panic!("System execute must rejoin its root: {reply:?}"),
        }
    }
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ResumeExecution { program }),
        DebuggerReply::ExecutionResumed { program }
    );
    await_completed_execution(&mut debugger, program);
    write_client_message(&mut browser, &ClientMessage::GetBlueTsScriptReports).unwrap();
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::BlueTsScriptReports(reports)
            if reports.len() == 1
                && reports[0].outcome == blueice_ipc::BlueTsScriptExecutionOutcome::Executed
    ));
    drop(debugger);
    launcher.shutdown();
    fixture.join().unwrap();
}
