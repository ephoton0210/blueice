// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! C3.1.3.5.2: independent static type and runtime value views of one slot.

use super::*;
use blueice_ipc::debugger::{
    DebuggerStaticMetadataHandle, DebuggerStaticMetadataSymbolId, DebuggerStaticMetadataTypeId,
    DebuggerStaticScopeRelation, DebuggerStaticScopeTarget,
};

pub(super) fn static_manifest() -> DebuggerMetadataCapabilityManifest {
    DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
        type_display: true,
        static_scope_relation: true,
        ..Default::default()
    })
}

fn open_debugger(
    socket: &Path,
    bounded_values: bool,
    manifest: DebuggerMetadataCapabilityManifest,
) -> UnixStream {
    let mut debugger = UnixStream::connect(socket).expect("public debugger socket must accept");
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: bounded_values,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: bounded_values,
            granted_metadata_capabilities: manifest,
        }
    );
    debugger
}

pub(super) fn static_receipts(
    debugger: &mut UnixStream,
    program: DebuggerProgram,
) -> (
    DebuggerStaticMetadataHandle,
    Vec<DebuggerStaticMetadataTypeId>,
    Vec<DebuggerStaticMetadataSymbolId>,
) {
    let DebuggerReply::StaticMetadata(metadata) =
        debugger_request(debugger, DebuggerRequest::ListStaticMetadata { program })
    else {
        panic!("the live BlueTS program must expose one metadata receipt")
    };
    assert_eq!(metadata.len(), 1);
    let metadata = metadata[0];
    let DebuggerReply::StaticMetadataTypes(types) = debugger_request(
        debugger,
        DebuggerRequest::ListStaticMetadataTypes { metadata },
    ) else {
        panic!("the stream must receipt compiler type IDs")
    };
    let DebuggerReply::StaticMetadataSymbols(symbols) = debugger_request(
        debugger,
        DebuggerRequest::ListStaticMetadataSymbols { metadata },
    ) else {
        panic!("the stream must receipt compiler symbol IDs")
    };
    assert!(!types.is_empty() && !symbols.is_empty());
    (metadata, types, symbols)
}

fn active_scopes(
    debugger: &mut UnixStream,
    program: DebuggerProgram,
    point: DebuggerSafePoint,
) -> blueice_ipc::debugger::DebuggerScopeSnapshot {
    let DebuggerReply::Scopes(scopes) = debugger_request(
        debugger,
        DebuggerRequest::GetScopes {
            program,
            frame: None,
            frame_index: 0,
            expected_safe_point: point,
            max_scope_entries: 256,
        },
    ) else {
        panic!("the paused root must expose active lexical slots")
    };
    assert_eq!(scopes.program, program);
    assert_eq!(scopes.safe_point, point);
    scopes
}

fn static_pair(
    debugger: &mut UnixStream,
    metadata: DebuggerStaticMetadataHandle,
    target: DebuggerValueTarget,
    types: &[DebuggerStaticMetadataTypeId],
    symbols: &[DebuggerStaticMetadataSymbolId],
) -> DebuggerStaticScopeRelation {
    let selector = DebuggerStaticScopeTarget::Ordinary { metadata, target };
    let DebuggerReply::StaticScopeRelation(relation) = debugger_request(
        debugger,
        DebuggerRequest::GetStaticScopeRelation { target: selector },
    ) else {
        panic!("the selected active root slot must have a checked static relation")
    };
    assert_eq!(relation.target, selector);
    assert!(types.contains(&relation.static_type));
    assert!(symbols.contains(&relation.symbol));
    let DebuggerReply::StaticMetadataType(display) = debugger_request(
        debugger,
        DebuggerRequest::DescribeStaticMetadataType {
            static_type: relation.static_type,
        },
    ) else {
        panic!("the separately granted compiler type display must resolve")
    };
    assert_eq!(display.static_type, relation.static_type);
    assert_eq!(display.display, "number");
    *relation
}

#[test]
fn launcher_pairs_independent_static_type_and_runtime_value_for_classic_and_module_roots() {
    for (mime, label) in [
        ("application/x-blueice-typescript", "classic"),
        ("application/x-blueice-typescript-module", "module"),
    ] {
        let gatekeeper_socket = clearing_gatekeeper();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let fixture = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                let body = format!(
                    "<main>{label}-static-runtime</main><script type=\"{mime}\">const rootValue: number = 9; globalThis.answer = rootValue;</script>"
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
        });
        let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
            &gatekeeper_socket,
            StaticMetadataPolicy {
                bounded_values: true,
                inventory: true,
                type_inventory: true,
                type_display: true,
                symbol_inventory: true,
                static_scope_relation: true,
                ..Default::default()
            },
        );
        let mut browser = launcher.connect_browser();
        blueice_ipc::client_handshake(&mut browser).unwrap();
        // Negotiate before navigation so Hello cannot consume the pending
        // execution window after the new realm becomes visible.
        let mut static_only = open_debugger(&launcher.debugger_socket, false, static_manifest());
        navigate(&mut browser, &url);

        // First stream has static authority only. Arm before metadata queries
        // so the real child cannot finish the root while inventories are read.
        let realm = one_realm(debugger_request(
            &mut static_only,
            DebuggerRequest::ListPageRealms,
        ));
        let program = one_program(
            debugger_request(&mut static_only, DebuggerRequest::ListPrograms { realm }),
            realm,
        );
        let root_point = safe_points(
            debugger_request(
                &mut static_only,
                DebuggerRequest::ListSafePoints { program },
            ),
            program,
        )
        .into_iter()
        .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset != 0)
        .expect("typed root must have a resumable safe point");
        assert_eq!(
            debugger_request(
                &mut static_only,
                DebuggerRequest::GetExecutionState { program },
            ),
            DebuggerReply::ExecutionState {
                program,
                state: DebuggerExecutionState::Pending,
            },
            "{label} must remain pending until its root breakpoint is armed"
        );
        assert_eq!(
            debugger_request(
                &mut static_only,
                DebuggerRequest::ArmRootSafePointBreakpoint {
                    safe_point: root_point,
                },
            ),
            DebuggerReply::RootSafePointBreakpointArmed {
                safe_point: root_point,
            }
        );
        await_paused_execution(&mut static_only, program, root_point);
        let (metadata, types, symbols) = static_receipts(&mut static_only, program);
        let mut point = root_point;
        let mut selected = None;
        for _ in 0..64 {
            for scope_entry in active_scopes(&mut static_only, program, point).entries {
                let target = DebuggerValueTarget {
                    program,
                    frame: None,
                    frame_index: 0,
                    safe_point: point,
                    scope_entry,
                };
                let selector = DebuggerStaticScopeTarget::Ordinary { metadata, target };
                match debugger_request(
                    &mut static_only,
                    DebuggerRequest::GetStaticScopeRelation { target: selector },
                ) {
                    DebuggerReply::StaticScopeRelation(relation) => {
                        assert_eq!(relation.target, selector);
                        selected = Some(target);
                        break;
                    }
                    DebuggerReply::Error {
                        code:
                            DebuggerErrorCode::InvalidTarget | DebuggerErrorCode::InvalidExecutionState,
                        ..
                    } => {}
                    other => panic!("{label} relation must be typed: {other:?}"),
                }
            }
            if selected.is_some() {
                break;
            }
            assert_eq!(
                debugger_request(
                    &mut static_only,
                    DebuggerRequest::StepRootInstruction { program },
                ),
                DebuggerReply::ExecutionStepRequested { program }
            );
            point = await_step_paused_execution(&mut static_only, program, point);
        }
        let static_target = selected.expect("the root must expose one checked slot");
        let relation = static_pair(&mut static_only, metadata, static_target, &types, &symbols);
        assert!(matches!(
            debugger_request(
                &mut static_only,
                DebuggerRequest::GetValue {
                    target: static_target,
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::CapabilityUnavailable,
                ..
            }
        ));
        drop(static_only);

        // A static relation does not itself authorize rendering its type.
        let relation_manifest = DebuggerMetadataCapabilityManifest::opaque_selected(
            DebuggerMetadataCapabilitySelection {
                static_scope_relation: true,
                ..Default::default()
            },
        );
        let mut relation_only = open_debugger(&launcher.debugger_socket, false, relation_manifest);
        let (relation_metadata, relation_types, relation_symbols) =
            static_receipts(&mut relation_only, program);
        assert!(active_scopes(&mut relation_only, program, point)
            .entries
            .contains(&static_target.scope_entry));
        let relation_only_target = DebuggerStaticScopeTarget::Ordinary {
            metadata: relation_metadata,
            target: static_target,
        };
        let DebuggerReply::StaticScopeRelation(relation_only_reply) = debugger_request(
            &mut relation_only,
            DebuggerRequest::GetStaticScopeRelation {
                target: relation_only_target,
            },
        ) else {
            panic!("{label} relation-only stream must resolve the receipted slot")
        };
        assert!(relation_types.contains(&relation_only_reply.static_type));
        assert!(relation_symbols.contains(&relation_only_reply.symbol));
        assert!(matches!(
            debugger_request(
                &mut relation_only,
                DebuggerRequest::DescribeStaticMetadataType {
                    static_type: relation_only_reply.static_type,
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::CapabilityUnavailable,
                ..
            }
        ));
        drop(relation_only);

        // A separate stream has Value authority, but no static relation or
        // type-display authority. It advances to the initialized root slot;
        // a visible Scopes slot before its initializer is not yet readable.
        let mut value_only = open_debugger(
            &launcher.debugger_socket,
            true,
            DebuggerMetadataCapabilityManifest::empty(),
        );
        let mut selected_value = None;
        for _ in 0..64 {
            for scope_entry in active_scopes(&mut value_only, program, point).entries {
                let candidate = DebuggerValueTarget {
                    program,
                    frame: None,
                    frame_index: 0,
                    safe_point: point,
                    scope_entry,
                };
                match debugger_request(
                    &mut value_only,
                    DebuggerRequest::GetValue { target: candidate },
                ) {
                    DebuggerReply::Value(snapshot)
                        if snapshot.preview
                            == DebuggerValuePreview::NumberBits(9.0_f64.to_bits()) =>
                    {
                        selected_value = Some((candidate, snapshot));
                        break;
                    }
                    DebuggerReply::Value(_) => {}
                    DebuggerReply::Error {
                        code:
                            DebuggerErrorCode::InvalidExecutionState | DebuggerErrorCode::InvalidTarget,
                        ..
                    } => {}
                    other => panic!("{label} Value lookup must be typed: {other:?}"),
                }
            }
            if selected_value.is_some() {
                break;
            }
            assert_eq!(
                debugger_request(
                    &mut value_only,
                    DebuggerRequest::StepRootInstruction { program },
                ),
                DebuggerReply::ExecutionStepRequested { program }
            );
            point = await_step_paused_execution(&mut value_only, program, point);
        }
        let (target, value_only_snapshot) =
            selected_value.expect("the initialized root slot must preview the value 9");
        assert_eq!(value_only_snapshot.target, target);
        assert!(matches!(
            debugger_request(
                &mut value_only,
                DebuggerRequest::GetStaticScopeRelation {
                    target: relation.target,
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::CapabilityUnavailable,
                ..
            }
        ));
        drop(value_only);

        // Both disclosures can be composed only after this stream obtains
        // its own inventories and Scopes receipt for the exact same target.
        let mut combined = open_debugger(&launcher.debugger_socket, true, static_manifest());
        let (combined_metadata, combined_types, combined_symbols) =
            static_receipts(&mut combined, program);
        assert_eq!(combined_metadata, metadata);
        assert!(active_scopes(&mut combined, program, point)
            .entries
            .contains(&target.scope_entry));
        let combined_relation = static_pair(
            &mut combined,
            combined_metadata,
            target,
            &combined_types,
            &combined_symbols,
        );
        assert_eq!(combined_relation.static_type, relation.static_type);
        let DebuggerReply::Value(combined_snapshot) =
            debugger_request(&mut combined, DebuggerRequest::GetValue { target })
        else {
            panic!("{label} combined stream must return its separately granted preview")
        };
        assert_eq!(combined_snapshot.target, target);
        assert_eq!(combined_snapshot.preview, value_only_snapshot.preview);

        assert_eq!(
            debugger_request(
                &mut combined,
                DebuggerRequest::StepRootInstruction { program },
            ),
            DebuggerReply::ExecutionStepRequested { program }
        );
        let _ = await_step_paused_execution(&mut combined, program, point);
        for request in [
            DebuggerRequest::GetStaticScopeRelation {
                target: combined_relation.target,
            },
            DebuggerRequest::GetValue { target },
        ] {
            assert!(matches!(
                debugger_request(&mut combined, request),
                DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget
                        | DebuggerErrorCode::InvalidExecutionState,
                    ..
                }
            ));
        }

        navigate(&mut browser, &url);
        fixture.join().unwrap();
        for request in [
            DebuggerRequest::GetStaticScopeRelation {
                target: combined_relation.target,
            },
            DebuggerRequest::GetValue { target },
            DebuggerRequest::DescribeStaticMetadataType {
                static_type: combined_relation.static_type,
            },
        ] {
            let reply = debugger_request(&mut combined, request);
            assert!(
                matches!(
                    reply,
                    DebuggerReply::Error {
                        code: DebuggerErrorCode::StaleRealm
                            | DebuggerErrorCode::CapabilityUnavailable
                            | DebuggerErrorCode::InvalidTarget,
                        ..
                    }
                ),
                "{label} old document target must have a typed no-payload refusal: {reply:?}"
            );
        }
        launcher.shutdown();
        let _ = std::fs::remove_file(gatekeeper_socket);
    }
}
