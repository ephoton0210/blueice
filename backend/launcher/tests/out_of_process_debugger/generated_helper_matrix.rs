// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::debugger::{
    DebuggerGeneratedBreakpoint, DebuggerGeneratedBreakpointTarget,
    DebuggerGeneratedPausedLocation, DebuggerGeneratedStepTarget,
    DebuggerGeneratedSystemBreakpoint, DebuggerGeneratedSystemBreakpointTarget,
};

fn position(source: &str, byte: usize) -> (u32, u32) {
    let prefix = &source[..byte];
    (
        prefix
            .bytes()
            .filter(|character| *character == b'\n')
            .count() as u32,
        prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
    )
}

fn serve_document(
    listener: TcpListener,
    source: String,
    module: bool,
    system: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let loader = if system {
            generated_system::PAGE_SYSTEM_LOADER
        } else {
            "var console = { log: function() {} };"
        };
        let kind = if module {
            "application/x-blueice-typescript-module"
        } else {
            "application/x-blueice-typescript"
        };
        let body = format!(
            "<main>frozen helper matrix</main><script>{loader}</script><script type=\"{kind}\">{source}</script>"
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
fn frozen_eleven_row_helper_frames_never_claim_user_source() {
    let matrix = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bluets/tests/fixtures/k11_source_maps/source-map-tokens-checker-matrix.tsv"
    ));
    let rows = matrix.lines().skip(1).collect::<Vec<_>>();
    assert_eq!(
        rows.len(),
        11,
        "the frozen K.11 matrix must stay eleven rows"
    );
    assert_eq!(
        rows.iter()
            .map(|row| row.split('\t').next().unwrap())
            .collect::<Vec<_>>(),
        [
            "k11-basic-copy",
            "k11-erased-annotation",
            "k11-standard-method",
            "k11-compacted-relocation",
            "k11-erased-relocation",
            "k11-composed-decorators",
            "k11-legacy-class-member",
            "k11-legacy-parameter-metadata",
            "k11-nondecorator-helper",
            "k11-system-wrapper",
            "k11-crlf-astral",
        ],
        "the helper-frame test must exercise every frozen row exactly once"
    );
    let mut helper_rows = 0;
    for row in rows {
        let fields = row.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 11, "invalid frozen row: {row}");
        let case = fields[0];
        let mut source = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../bluets/tests/fixtures/k11_source_maps")
                .join(fields[2]),
        )
        .unwrap();
        if fields[3] == "crlf" {
            source = source.replace('\n', "\r\n");
        } else {
            assert_eq!(fields[3], "lf", "{case}");
        }
        let target = blueice_bluets::EcmaTarget::parse(fields[4]).unwrap();
        let system = fields[5] == "System";
        assert!(system || fields[5] == "ES2022", "{case}");
        let legacy = fields[6] == "true";
        let metadata = fields[7] == "true";
        let module = !system && !legacy && target != blueice_bluets::EcmaTarget::Es5;
        let module_id = "memory:///k11-helper-matrix.ts";
        let compilation = blueice_bluets::compile(
            module_id,
            &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(
                module_id,
                source.clone(),
            )]),
            blueice_bluets::CompilerOptions {
                target,
                module_kind: if system {
                    blueice_bluets::ModuleKind::System
                } else {
                    blueice_bluets::ModuleKind::Esm
                },
                experimental_decorators: legacy,
                emit_decorator_metadata: metadata,
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
        let artifact = &output.artifacts[module_id];
        let generated = &artifact.javascript;
        let map = artifact.source_map.as_ref().unwrap();
        let marker = match case {
            "k11-basic-copy" | "k11-erased-annotation" | "k11-crlf-astral" => None,
            "k11-standard-method"
            | "k11-compacted-relocation"
            | "k11-erased-relocation"
            | "k11-composed-decorators" => Some("var __bluetsEsDecorate"),
            "k11-legacy-class-member" | "k11-legacy-parameter-metadata" => {
                Some("var __bluetsDecorate")
            }
            "k11-nondecorator-helper" => Some("function __blueice_target_object_properties_"),
            "k11-system-wrapper" => Some("System.register"),
            other => panic!("unexpected frozen K.11 row {other}"),
        };
        let Some(marker) = marker else {
            assert!(!generated.contains("__bluets"), "{case}: unexpected helper");
            assert!(
                !generated.contains("__blueice_target_object_properties_"),
                "{case}: unexpected helper"
            );
            assert!(!generated.contains("System.register"), "{case}");
            continue;
        };
        helper_rows += 1;
        let helper_position = position(generated, generated.find(marker).unwrap());
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
            "{case}: compiler map must leave the helper body unbound"
        );

        let gatekeeper_socket = clearing_gatekeeper();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let fixture = serve_document(listener, source, module, system);
        let policy = StaticMetadataPolicy {
            emitted_blue_ts_module: module,
            emitted_blue_ts_module_page_console_ambient: module,
            emitted_blue_ts_classic_page_console_ambient: !module && !system,
            emitted_blue_ts_classic_es5: target == blueice_bluets::EcmaTarget::Es5,
            emitted_blue_ts_classic_legacy_decorators: legacy && !metadata,
            emitted_blue_ts_classic_legacy_metadata: metadata,
            emitted_blue_ts_classic_system: system,
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        };
        let mut launcher =
            LauncherProcess::spawn_with_static_metadata_policy(&gatekeeper_socket, policy);
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
                },
            ),
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_bounded_values: false,
                granted_metadata_capabilities: manifest,
            },
            "{case}"
        );
        hold_next_document(&mut debugger, 1);
        navigate(&mut browser, &url);
        let realm = one_realm(debugger_request(
            &mut debugger,
            DebuggerRequest::ListPageRealms,
        ));
        let inventory = debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm });
        let DebuggerReply::Programs(programs) = inventory else {
            panic!("{case}: expected loader and emitted program: {inventory:?}")
        };
        assert_eq!(programs.len(), 2, "{case}");
        let loader = programs[0];
        let program = programs[1];
        let helper_target = DebuggerGeneratedBreakpointTarget {
            program,
            generated_line: helper_position.0,
            generated_column_utf16: helper_position.1,
        };
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ResolveGeneratedBreakpoint {
                    target: helper_target,
                },
            ),
            DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
                target: helper_target,
                safe_point: None,
            }),
            "{case}: helper definition cannot be bound as a user breakpoint"
        );
        if system {
            let target = DebuggerGeneratedSystemBreakpointTarget {
                emitted: DebuggerGeneratedBreakpointTarget {
                    program,
                    generated_line: position(generated, generated.rfind("console.log(").unwrap()).0,
                    generated_column_utf16: position(
                        generated,
                        generated.rfind("console.log(").unwrap(),
                    )
                    .1,
                },
                loader,
            };
            let binding = debugger_request(
                &mut debugger,
                DebuggerRequest::ArmGeneratedSystemBreakpoint { target },
            );
            let DebuggerReply::GeneratedSystemBreakpointArmed(DebuggerGeneratedSystemBreakpoint {
                safe_point: Some(bound),
                ..
            }) = binding
            else {
                panic!("{case}: System execute token did not arm: {binding:?}")
            };
            let (frame, paused) = await_nested_paused_execution(&mut debugger, program);
            assert_eq!(paused, bound, "{case}");
            let projection = debugger_request(
                &mut debugger,
                DebuggerRequest::GetGeneratedSystemStackLocations { frame, loader },
            );
            let DebuggerReply::GeneratedSystemStackLocations(projection) = projection else {
                panic!("{case}: complete System stack must project: {projection:?}")
            };
            assert!(projection.is_well_formed(), "{case}");
            assert_eq!(projection.locations[1], None, "{case}: loader frame");
            assert_eq!(projection.locations[2], None, "{case}: wrapper frame");
            assert!(projection.locations[0]
                .and_then(|location| location.original)
                .is_some());
        } else {
            // Fixed identities from the frozen emitted artifacts, confirmed
            // by live helper pauses. Standard decorators execute the emitted
            // class IIFE (6/8); legacy and ES5 execute their helper closure.
            // A layout change must revalidate these identities, not silently
            // choose a different nested function as the helper.
            let helper_ordinal = match case {
                "k11-standard-method" | "k11-compacted-relocation" | "k11-erased-relocation" => 6,
                "k11-composed-decorators" => 8,
                "k11-legacy-class-member" | "k11-legacy-parameter-metadata" => 3,
                "k11-nondecorator-helper" => 1,
                _ => unreachable!("only helper-bearing non-System rows reach this branch"),
            };
            let points = safe_points(
                debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
                program,
            );
            let helper = points
                .into_iter()
                .find(|point| {
                    point.code_unit_ordinal == helper_ordinal && point.bytecode_offset == 0
                })
                .unwrap_or_else(|| panic!("{case}: helper code unit has no entry safe point"));
            assert_eq!(
                debugger_request(
                    &mut debugger,
                    DebuggerRequest::ArmNestedSafePointBreakpoint { safe_point: helper },
                ),
                DebuggerReply::NestedSafePointBreakpointArmed { safe_point: helper },
                "{case}: raw helper entry must be reachable"
            );
            let (frame, paused) = await_nested_paused_execution(&mut debugger, program);
            assert_eq!(paused, helper, "{case}");
            let stack = debugger_request(
                &mut debugger,
                DebuggerRequest::GetStack {
                    program,
                    frame: Some(frame),
                    max_frames: 3,
                },
            );
            let DebuggerReply::Stack(stack) = stack else {
                panic!("{case}: helper pause must expose its stack: {stack:?}")
            };
            assert_eq!(stack.safe_points[0], helper, "{case}");
            assert!(stack.safe_points.len() >= 2, "{case}: helper has a caller");
            assert_eq!(
                stack.safe_points.last().unwrap().code_unit_ordinal,
                0,
                "{case}: helper stack must return to the emitted root"
            );
            assert!(!stack.stack_truncated, "{case}");
            let location = debugger_request(
                &mut debugger,
                DebuggerRequest::GetGeneratedPausedLocation { program },
            );
            let DebuggerReply::GeneratedPausedLocation(DebuggerGeneratedPausedLocation {
                safe_point,
                location,
                ..
            }) = location
            else {
                panic!("{case}: helper location lookup failed: {location:?}")
            };
            assert_eq!(safe_point, helper, "{case}");
            assert!(
                location.and_then(|location| location.original).is_none(),
                "{case}: helper-body frame claimed a user-source coordinate: {location:?}"
            );
            assert_eq!(
                debugger_request(
                    &mut debugger,
                    DebuggerRequest::StepGeneratedInstruction {
                        target: DebuggerGeneratedStepTarget::Single {
                            program,
                            frame: Some(frame),
                        },
                    },
                ),
                DebuggerReply::NestedStepRequested { frame },
                "{case}: helper frame must permit an exact instruction step"
            );
            let (stepped_frame, stepped) = await_nested_paused_execution(&mut debugger, program);
            assert_eq!(stepped_frame, frame, "{case}");
            assert_ne!(stepped, helper, "{case}");
            let stepped_location = debugger_request(
                &mut debugger,
                DebuggerRequest::GetGeneratedPausedLocation { program },
            );
            let DebuggerReply::GeneratedPausedLocation(DebuggerGeneratedPausedLocation {
                safe_point,
                location,
                ..
            }) = stepped_location
            else {
                panic!("{case}: stepped helper location lookup failed: {stepped_location:?}")
            };
            assert_eq!(safe_point, stepped, "{case}");
            assert!(
                location.and_then(|location| location.original).is_none(),
                "{case}: helper-body instruction claimed a user-source coordinate: {location:?}"
            );
        }
        drop(debugger);
        launcher.shutdown();
        fixture.join().unwrap();
    }
    assert_eq!(
        helper_rows, 8,
        "all helper-bearing frozen rows must be exercised"
    );
}
