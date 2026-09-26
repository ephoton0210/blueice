// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn launcher_relates_classic_and_module_root_scopes_without_value_authority() {
    for (mime, label) in [
        ("application/x-blueice-typescript", "classic"),
        ("application/x-blueice-typescript-module", "module"),
    ] {
        let gatekeeper_socket = clearing_gatekeeper();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let fixture = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = format!(
                "<main>{label}-static-scope</main><script type=\"{mime}\">const rootValue: number = 9; globalThis.answer = rootValue;</script>"
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
        });
        let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
            &gatekeeper_socket,
            StaticMetadataPolicy {
                inventory: true,
                type_inventory: true,
                symbol_inventory: true,
                static_scope_relation: true,
                ..Default::default()
            },
        );
        let mut browser = launcher.connect_browser();
        blueice_ipc::client_handshake(&mut browser).unwrap();
        navigate(&mut browser, &url);
        fixture.join().unwrap();
        let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
        let manifest = DebuggerMetadataCapabilityManifest::opaque_selected(
            DebuggerMetadataCapabilitySelection {
                static_scope_relation: true,
                ..Default::default()
            },
        );
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
        let realm = one_realm(debugger_request(
            &mut debugger,
            DebuggerRequest::ListPageRealms,
        ));
        let program = one_program(
            debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
            realm,
        );
        let DebuggerReply::Capabilities(capabilities) = debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeCapabilities { realm },
        ) else {
            panic!("{label} must advertise debugger capabilities");
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::StaticScopeRelation
                && report.state == DebuggerCapabilityState::Available
        }));
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::BoundedValues
                && report.state == DebuggerCapabilityState::Planned
        }));
        let DebuggerReply::StaticMetadata(metadata) = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadata { program },
        ) else {
            panic!("{label} must expose its opaque metadata handle");
        };
        assert_eq!(metadata.len(), 1);
        let metadata = metadata[0];
        let DebuggerReply::StaticMetadataTypes(types) = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadataTypes { metadata },
        ) else {
            panic!("{label} must expose receipted type IDs");
        };
        let DebuggerReply::StaticMetadataSymbols(symbols) = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadataSymbols { metadata },
        ) else {
            panic!("{label} must expose receipted symbol IDs");
        };
        assert!(!types.is_empty() && !symbols.is_empty());
        let root_point = safe_points(
            debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
            program,
        )
        .into_iter()
        .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset != 0)
        .expect("typed root must have a non-entry safe point");
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::ArmRootSafePointBreakpoint {
                    safe_point: root_point,
                },
            ),
            DebuggerReply::RootSafePointBreakpointArmed {
                safe_point: root_point,
            }
        );
        await_paused_execution(&mut debugger, program, root_point);
        let mut point = root_point;
        let mut found = None;
        for _ in 0..64 {
            let DebuggerReply::Scopes(scopes) = debugger_request(
                &mut debugger,
                DebuggerRequest::GetScopes {
                    program,
                    frame: None,
                    frame_index: 0,
                    expected_safe_point: point,
                    max_scope_entries: 256,
                },
            ) else {
                panic!("{label} must expose root lexical slots");
            };
            for scope_entry in scopes.entries {
                let value = DebuggerValueTarget {
                    program,
                    frame: None,
                    frame_index: 0,
                    safe_point: point,
                    scope_entry,
                };
                let target = blueice_ipc::debugger::DebuggerStaticScopeTarget::Ordinary {
                    metadata,
                    target: value,
                };
                match debugger_request(
                    &mut debugger,
                    DebuggerRequest::GetStaticScopeRelation { target },
                ) {
                    DebuggerReply::StaticScopeRelation(relation) => {
                        assert_eq!(relation.target, target);
                        assert!(relation.is_well_formed());
                        assert!(types.contains(&relation.static_type));
                        assert!(symbols.contains(&relation.symbol));
                        found = Some(value);
                        break;
                    }
                    DebuggerReply::Error {
                        code:
                            DebuggerErrorCode::InvalidTarget | DebuggerErrorCode::InvalidExecutionState,
                        ..
                    } => {}
                    other => panic!("{label} root relation must be typed: {other:?}"),
                }
            }
            if found.is_some() {
                break;
            }
            assert_eq!(
                debugger_request(
                    &mut debugger,
                    DebuggerRequest::StepRootInstruction { program }
                ),
                DebuggerReply::ExecutionStepRequested { program }
            );
            point = await_step_paused_execution(&mut debugger, program, point);
        }
        let value = found.expect("classic/module root must expose a bound static slot");
        assert!(matches!(
            debugger_request(&mut debugger, DebuggerRequest::GetValue { target: value }),
            DebuggerReply::Error {
                code: DebuggerErrorCode::CapabilityUnavailable,
                ..
            }
        ));
        let old_relation = blueice_ipc::debugger::DebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: value,
        };
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::StepRootInstruction { program }
            ),
            DebuggerReply::ExecutionStepRequested { program }
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
                } if safe_point != point => break,
                DebuggerReply::ExecutionState {
                    state: DebuggerExecutionState::Completed,
                    ..
                } => break,
                DebuggerReply::ExecutionState {
                    state: DebuggerExecutionState::Stepping,
                    ..
                } if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                other => panic!("{label} root did not advance after step: {other:?}"),
            }
        }
        assert!(matches!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::GetStaticScopeRelation {
                    target: old_relation,
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget | DebuggerErrorCode::InvalidExecutionState,
                ..
            }
        ));
        drop(debugger);
        let mut foreign = UnixStream::connect(&launcher.debugger_socket).unwrap();
        let foreign_manifest = DebuggerMetadataCapabilityManifest::opaque_selected(
            DebuggerMetadataCapabilitySelection {
                static_scope_relation: true,
                ..Default::default()
            },
        );
        assert_eq!(
            debugger_request(
                &mut foreign,
                DebuggerRequest::Hello {
                    protocol_version: DEBUGGER_PROTOCOL_VERSION,
                    requested_bounded_values: false,
                    requested_metadata_capabilities: foreign_manifest.clone(),
                },
            ),
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_bounded_values: false,
                granted_metadata_capabilities: foreign_manifest,
            }
        );
        let foreign_target = blueice_ipc::debugger::DebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: value,
        };
        assert!(matches!(
            debugger_request(
                &mut foreign,
                DebuggerRequest::GetStaticScopeRelation {
                    target: foreign_target,
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget | DebuggerErrorCode::CapabilityUnavailable,
                ..
            }
        ));
        drop(foreign);
        launcher.shutdown();
        let _ = std::fs::remove_file(gatekeeper_socket);
    }
}

#[test]
fn launcher_relates_nested_parent_roots_but_not_child_local_slots() {
    for (mime, label) in [
        ("application/x-blueice-typescript", "classic"),
        ("application/x-blueice-typescript-module", "module"),
    ] {
        let gatekeeper_socket = clearing_gatekeeper();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let fixture = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = format!(
                "<main>{label}-nested-scope</main><script type=\"{mime}\">{BOUNDED_VALUE_BLUETS_SOURCE}</script>"
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
        });
        let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
            &gatekeeper_socket,
            StaticMetadataPolicy {
                inventory: true,
                type_inventory: true,
                symbol_inventory: true,
                static_scope_relation: true,
                ..Default::default()
            },
        );
        let mut browser = launcher.connect_browser();
        blueice_ipc::client_handshake(&mut browser).unwrap();
        navigate(&mut browser, &url);
        fixture.join().unwrap();
        let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
        let manifest = DebuggerMetadataCapabilityManifest::opaque_selected(
            DebuggerMetadataCapabilitySelection {
                static_scope_relation: true,
                ..Default::default()
            },
        );
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
        let realm = one_realm(debugger_request(
            &mut debugger,
            DebuggerRequest::ListPageRealms,
        ));
        let program = one_program(
            debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
            realm,
        );
        let (paused_program, frame) = arm_first_nested_frame(&mut debugger, realm);
        assert_eq!(paused_program, program);
        let DebuggerReply::StaticMetadata(metadata) = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadata { program },
        ) else {
            panic!("{label} nested program must expose metadata");
        };
        assert_eq!(metadata.len(), 1);
        let metadata = metadata[0];
        let DebuggerReply::StaticMetadataTypes(types) = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadataTypes { metadata },
        ) else {
            panic!("{label} nested program must expose type IDs");
        };
        let DebuggerReply::StaticMetadataSymbols(symbols) = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadataSymbols { metadata },
        ) else {
            panic!("{label} nested program must expose symbol IDs");
        };
        let DebuggerReply::Stack(stack) = debugger_request(
            &mut debugger,
            DebuggerRequest::GetStack {
                program,
                frame: Some(frame),
                max_frames: 2,
            },
        ) else {
            panic!("{label} nested pause must expose both frames");
        };
        assert_eq!(stack.safe_points.len(), 2);
        let mut child_point = stack.safe_points[0];
        let parent_point = stack.safe_points[1];
        let mut child_entry = None;
        for _ in 0..96 {
            let DebuggerReply::Scopes(child_scopes) = debugger_request(
                &mut debugger,
                DebuggerRequest::GetScopes {
                    program,
                    frame: Some(frame),
                    frame_index: 0,
                    expected_safe_point: child_point,
                    max_scope_entries: 256,
                },
            ) else {
                panic!("{label} nested child must expose source-free slots");
            };
            if let Some(entry) = child_scopes.entries.first() {
                child_entry = Some(*entry);
                break;
            }
            assert_eq!(
                debugger_request(
                    &mut debugger,
                    DebuggerRequest::StepNestedInstruction { frame },
                ),
                DebuggerReply::NestedStepRequested { frame }
            );
            let (same_frame, next_point) = await_nested_paused_execution(&mut debugger, program);
            assert_eq!(same_frame, frame);
            child_point = next_point;
        }
        let child_entry = child_entry.expect("nested child must eventually expose an active slot");
        let child_local = blueice_ipc::debugger::DebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: DebuggerValueTarget {
                program,
                frame: Some(frame),
                frame_index: 0,
                safe_point: child_point,
                scope_entry: child_entry,
            },
        };
        assert!(matches!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::GetStaticScopeRelation {
                    target: child_local,
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
        let DebuggerReply::Scopes(parent_scopes) = debugger_request(
            &mut debugger,
            DebuggerRequest::GetScopes {
                program,
                frame: Some(frame),
                frame_index: 1,
                expected_safe_point: parent_point,
                max_scope_entries: 256,
            },
        ) else {
            panic!("{label} nested caller must expose parent-root slots");
        };
        let mut found = None;
        let mut denials = Vec::new();
        for scope_entry in parent_scopes.entries {
            let target = blueice_ipc::debugger::DebuggerStaticScopeTarget::Ordinary {
                metadata,
                target: DebuggerValueTarget {
                    program,
                    frame: Some(frame),
                    frame_index: 1,
                    safe_point: parent_point,
                    scope_entry,
                },
            };
            match debugger_request(
                &mut debugger,
                DebuggerRequest::GetStaticScopeRelation { target },
            ) {
                DebuggerReply::StaticScopeRelation(relation) => {
                    assert_eq!(relation.target, target);
                    assert!(relation.is_well_formed());
                    assert!(types.contains(&relation.static_type));
                    assert!(symbols.contains(&relation.symbol));
                    found = Some(target);
                    break;
                }
                reply @ DebuggerReply::Error {
                    code:
                        DebuggerErrorCode::InvalidTarget | DebuggerErrorCode::InvalidExecutionState,
                    ..
                } => denials.push((scope_entry, reply)),
                other => panic!("{label} parent relation must be typed: {other:?}"),
            }
        }
        let found = found
            .unwrap_or_else(|| panic!("nested caller must retain a bound root slot: {denials:?}"));
        let blueice_ipc::debugger::DebuggerStaticScopeTarget::Ordinary { target, .. } = found
        else {
            unreachable!();
        };
        assert!(matches!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::GetStaticScopeRelation {
                    target: blueice_ipc::debugger::DebuggerStaticScopeTarget::Ordinary {
                        metadata,
                        target: DebuggerValueTarget {
                            scope_entry: blueice_ipc::debugger::DebuggerScopeEntry {
                                slot_ordinal: u32::MAX,
                                ..target.scope_entry
                            },
                            ..target
                        },
                    },
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
        launcher.shutdown();
        let _ = std::fs::remove_file(gatekeeper_socket);
    }
}
