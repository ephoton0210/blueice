// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::debugger::{
    DebuggerGeneratedBreakpoint, DebuggerGeneratedBreakpointTarget,
    DebuggerGeneratedOriginalPosition, DebuggerGeneratedPausedLocation,
    DebuggerGeneratedPausedSourceLocation, DebuggerGeneratedStepTarget,
};

#[derive(Clone, Copy)]
enum FrozenProfile {
    Legacy,
    Metadata,
    Es5,
}

impl FrozenProfile {
    fn options(self) -> blueice_bluets::CompilerOptions {
        blueice_bluets::CompilerOptions {
            target: if matches!(self, Self::Es5) {
                blueice_bluets::EcmaTarget::Es5
            } else {
                blueice_bluets::EcmaTarget::Es2022
            },
            experimental_decorators: !matches!(self, Self::Es5),
            emit_decorator_metadata: matches!(self, Self::Metadata),
            source_map: true,
            inline_sources: true,
            ..Default::default()
        }
    }

    fn policy(self) -> StaticMetadataPolicy {
        StaticMetadataPolicy {
            emitted_blue_ts_classic_page_console_ambient: true,
            emitted_blue_ts_classic_legacy_decorators: matches!(self, Self::Legacy),
            emitted_blue_ts_classic_legacy_metadata: matches!(self, Self::Metadata),
            emitted_blue_ts_classic_es5: matches!(self, Self::Es5),
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        }
    }
}

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
    profile: FrozenProfile,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let loader = match profile {
            FrozenProfile::Legacy => "var seen=0;var console={log:function(a){var expected=['method','class',42];if(a!==expected[seen++])throw 'legacy result';}};",
            FrozenProfile::Metadata => "var seen=0;var console={log:function(a,b){var expected=['parameter','method',42];if(a!==expected[seen]||(seen===0&&b!==0))throw 'metadata result';seen++;}};",
            FrozenProfile::Es5 => "var seen=0;var console={log:function(a){if(a!==3)throw 'ES5 result';seen++;}};",
        };
        let body = format!(
            "<main>frozen classic</main><script>{loader}</script><script type=\"application/x-blueice-typescript\">{source}</script>"
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
fn public_exact_frozen_legacy_and_es5_helpers_bind_only_source_anchored_calls() {
    let legacy = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/legacy_decorated.ts"
    ));
    let metadata = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/legacy_parameter_metadata.ts"
    ));
    let es5 = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/object_spread_helper.ts"
    ));
    for (
        case,
        source,
        profile,
        generated_prefix,
        generated_token,
        original_token,
        anchor_end,
        unmapped,
    ) in [
        (
            "legacy-member",
            legacy,
            FrozenProfile::Legacy,
            "",
            "__bluetsDecorate([methodDecor",
            "method()",
            false,
            &["var __bluetsDecorate", "], Example.prototype"][..],
        ),
        (
            "legacy-class",
            legacy,
            FrozenProfile::Legacy,
            "",
            "__bluetsDecorate([classDecor",
            "class Example",
            true,
            &["var __bluetsDecorate", "], Example.prototype"][..],
        ),
        (
            "legacy-parameter-metadata",
            metadata,
            FrozenProfile::Metadata,
            "",
            "__bluetsParam(0, parameterDecor)",
            "@parameterDecor",
            false,
            &["var __bluetsParam", "__bluetsMetadata(\"design:type\""][..],
        ),
        (
            "es5-object-spread",
            es5,
            FrozenProfile::Es5,
            "var merged",
            "__blueice_target_object_properties_",
            "merged",
            true,
            &["function __blueice_target_object_properties_"][..],
        ),
    ] {
        let module = "memory:///k11-frozen-classic.ts";
        let compilation = blueice_bluets::compile(
            module,
            &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(module, source)]),
            profile.options(),
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
        let prefix_start = generated.find(generated_prefix).unwrap() + generated_prefix.len();
        let generated_byte =
            prefix_start + generated[prefix_start..].find(generated_token).unwrap();
        let generated_position = coordinate(generated, generated_byte);
        let original_byte = source.find(original_token).unwrap()
            + if anchor_end {
                original_token.len()
            } else if original_token.starts_with('@') {
                1
            } else {
                0
            };
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
            .unwrap_or_else(|| panic!("{case} mapped helper call is missing"));
        assert_eq!(
            (
                mapped.source_index,
                mapped.line as u32,
                mapped.column_utf16 as u32
            ),
            (0, original_position.0, original_position.1),
            "{case} frozen source anchor"
        );
        let unmapped_positions = unmapped
            .iter()
            .map(|token| {
                let at = coordinate(generated, generated.find(token).unwrap());
                assert_eq!(
                    map.original_position_at(
                        generated,
                        at.0 as usize,
                        at.1 as usize,
                        200_000,
                        16 * 1024 * 1024,
                    )
                    .unwrap(),
                    None,
                    "{case} generated {token} must not claim user source"
                );
                at
            })
            .collect::<Vec<_>>();

        let gatekeeper_socket = clearing_gatekeeper();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let fixture = serve_frozen_document(listener, source, profile);
        let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
            &gatekeeper_socket,
            profile.policy(),
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
            "{case} debugger capability grant"
        );
        hold_next_document(&mut debugger, 1);
        navigate(&mut browser, &url);
        let realm = one_realm(debugger_request(
            &mut debugger,
            DebuggerRequest::ListPageRealms,
        ));
        let inventory = debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm });
        let DebuggerReply::Programs(programs) = inventory else {
            panic!("{case} must install loader and exact frozen classic program: {inventory:?}")
        };
        assert_eq!(programs.len(), 2, "{case}");
        let program = programs[1];
        for (line, column) in unmapped_positions {
            let target = DebuggerGeneratedBreakpointTarget {
                program,
                generated_line: line,
                generated_column_utf16: column,
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
                "{case} generated helper body must stay unbound"
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
            panic!("{case} exact source-anchored helper call must bind: {binding:?}")
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
            "{case} exact frozen source projection"
        );
        let DebuggerReply::Stack(stack) = debugger_request(
            &mut debugger,
            DebuggerRequest::GetStack {
                program,
                frame: None,
                max_frames: 1,
            },
        ) else {
            panic!("{case} must retain its one-frame root stack")
        };
        assert_eq!(stack.safe_points, vec![safe_point], "{case}");
        assert!(!stack.stack_truncated, "{case}");
        assert!(matches!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::StepGeneratedInstruction {
                    target: DebuggerGeneratedStepTarget::Single {
                        program,
                        frame: None
                    },
                }
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
            panic!("{case} step must retain its root stack")
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
            "{case} interior instruction must not inherit its helper call source"
        );
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
