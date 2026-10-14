// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn public_value_dispatch_requires_granted_scopes_receipt_and_current_pause() {
    let (tabs, realm) = loaded_tabs();
    let program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let frame = DebuggerFrame {
        program,
        code_unit_ordinal: 1,
        core_instance: [7; 16],
        frame_handle: 19,
    };
    let safe_point = DebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: 41,
    };
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: true,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::empty(),
    };
    let manifest = blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::empty();
    let granted_ack = blueice_ipc::debugger::negotiate_with_values(&hello, &manifest, true);
    let denied_ack = blueice_ipc::debugger::negotiate_with_values(&hello, &manifest, false);
    let granted =
        blueice_ipc::debugger::metadata_session_authorization(&hello, &granted_ack).unwrap();
    let separate =
        blueice_ipc::debugger::metadata_session_authorization(&hello, &granted_ack).unwrap();
    let denied =
        blueice_ipc::debugger::metadata_session_authorization(&hello, &denied_ack).unwrap();
    let mut locations = StackCoordinateLocations {
        moved_root_offset: 41,
        unbound_root: false,
        span_calls: 0,
        value_preview: Some(JavaScriptPageDebuggerValuePreview::Array(vec![
            Some(JavaScriptPageDebuggerValuePreview::NumberBits(
                7.0_f64.to_bits(),
            )),
            None,
        ])),
        value_calls: 0,
    };
    let scope_request = DebuggerRequest::GetScopes {
        program,
        frame: Some(frame),
        frame_index: 1,
        expected_safe_point: safe_point,
        max_scope_entries: 1,
    };
    let target = DebuggerValueTarget {
        program,
        frame: Some(frame),
        frame_index: 1,
        safe_point,
        scope_entry: DebuggerScopeEntry {
            slot_ordinal: 3,
            scope_depth: 0,
        },
    };
    let value_request = DebuggerRequest::GetValue { target };
    let available = handle_debugger_request_with_child_locations_and_pause(
        &tabs,
        &mut locations,
        Some(&granted),
        1,
        DebuggerRequest::DescribeCapabilities { realm },
    );
    assert!(
        matches!(available, DebuggerReply::Capabilities(capabilities)
        if capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::BoundedValues
                && report.state == DebuggerCapabilityState::Available
        }))
    );
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&granted),
            1,
            value_request.clone(),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&denied),
            1,
            scope_request.clone(),
        ),
        DebuggerReply::Scopes(_)
    ));
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&denied),
            1,
            value_request.clone(),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&granted),
            1,
            scope_request,
        ),
        DebuggerReply::Scopes(_)
    ));
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&separate),
            1,
            value_request.clone(),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    for guessed_program in [
        DebuggerProgram {
            program_handle: program.program_handle + 1,
            ..program
        },
        DebuggerProgram {
            realm: DebuggerPageRealm {
                tab_id: realm.tab_id + 1,
                ..realm
            },
            ..program
        },
    ] {
        let guessed = DebuggerValueTarget {
            program: guessed_program,
            frame: Some(DebuggerFrame {
                program: guessed_program,
                ..frame
            }),
            safe_point: DebuggerSafePoint {
                program: guessed_program,
                ..safe_point
            },
            ..target
        };
        assert!(guessed.is_well_formed());
        assert!(matches!(
            handle_debugger_request_with_child_locations_and_pause(
                &tabs,
                &mut locations,
                Some(&granted),
                1,
                DebuggerRequest::GetValue { target: guessed },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
    }
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&granted),
            2,
            value_request.clone(),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    assert_eq!(locations.value_calls, 0);
    let reply = handle_debugger_request_with_child_locations_and_pause(
        &tabs,
        &mut locations,
        Some(&granted),
        1,
        value_request.clone(),
    );
    assert_eq!(
        reply,
        DebuggerReply::Value(Box::new(DebuggerValueSnapshot {
            target,
            preview: DebuggerValuePreview::Array(vec![
                Some(DebuggerValuePreview::NumberBits(7.0_f64.to_bits())),
                None,
            ]),
        }))
    );
    assert_eq!(locations.value_calls, 1);
    locations.moved_root_offset = 42;
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&granted),
            1,
            value_request,
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert_eq!(locations.value_calls, 1);
}

#[test]
fn core_value_remint_enforces_all_tree_budgets_before_reply() {
    let remint = |value| remint_core_debugger_value(value, 0, &mut ValueRemintBudget::default());
    let mut too_deep = JavaScriptPageDebuggerValuePreview::Null;
    for _ in 0..5 {
        too_deep = JavaScriptPageDebuggerValuePreview::Array(vec![Some(too_deep)]);
    }
    assert!(remint(too_deep).is_none());
    assert!(remint(JavaScriptPageDebuggerValuePreview::Array(vec![None; 33])).is_none());
    assert!(remint(JavaScriptPageDebuggerValuePreview::Array(vec![
        Some(
            JavaScriptPageDebuggerValuePreview::Array(vec![None; 32])
        );
        8
    ]))
    .is_none());
    assert!(remint(JavaScriptPageDebuggerValuePreview::StringUnits(vec![
        0;
        2_049
    ]))
    .is_none());
    assert!(remint(JavaScriptPageDebuggerValuePreview::Record(vec![
        (vec![b'a' as u16], JavaScriptPageDebuggerValuePreview::Null),
        (vec![b'a' as u16], JavaScriptPageDebuggerValuePreview::Null),
    ]))
    .is_none());
}

#[test]
fn core_value_read_requires_same_stream_pause_and_live_scope() {
    let (tabs, realm) = loaded_tabs();
    let program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let frame = DebuggerFrame {
        program,
        code_unit_ordinal: 1,
        core_instance: [7; 16],
        frame_handle: 19,
    };
    let safe_point = DebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: 41,
    };
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities:
            blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::empty(),
    };
    let ack = blueice_ipc::debugger::negotiate(
        &hello,
        &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::empty(),
    );
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &ack).unwrap();
    let separate = blueice_ipc::debugger::metadata_session_authorization(&hello, &ack).unwrap();
    let mut locations = StackCoordinateLocations {
        moved_root_offset: 41,
        unbound_root: false,
        span_calls: 0,
        value_preview: Some(JavaScriptPageDebuggerValuePreview::Record(vec![(
            vec![0xd800],
            JavaScriptPageDebuggerValuePreview::Array(vec![
                Some(JavaScriptPageDebuggerValuePreview::NumberBits(
                    (-0.0_f64).to_bits(),
                )),
                None,
                Some(JavaScriptPageDebuggerValuePreview::StringUnits(vec![
                    0xdc00,
                ])),
            ]),
        )])),
        value_calls: 0,
    };
    let DebuggerReply::Scopes(scopes) = child_scopes(
        &tabs,
        &mut locations,
        program,
        Some(frame),
        1,
        safe_point,
        MAX_SCOPE_BINDINGS,
    ) else {
        panic!("mock must expose the paused caller-root scope");
    };
    let target = DebuggerValueTarget {
        program,
        frame: Some(frame),
        frame_index: 1,
        safe_point,
        scope_entry: scopes.entries[0],
    };
    assert!(session.observe_scopes(&scopes, 1));
    assert!(matches!(
        child_value_snapshot(&tabs, &mut locations, Some(&session), false, 1, target)
            .map_err(|reply| *reply),
        Err(DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        })
    ));
    assert!(matches!(
        child_value_snapshot(&tabs, &mut locations, Some(&separate), true, 1, target)
            .map_err(|reply| *reply),
        Err(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        })
    ));
    assert!(matches!(
        child_value_snapshot(&tabs, &mut locations, Some(&session), true, 2, target)
            .map_err(|reply| *reply),
        Err(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        })
    ));
    assert!(matches!(
        child_value_snapshot(
            &tabs,
            &mut locations,
            Some(&session),
            true,
            1,
            DebuggerValueTarget {
                scope_entry: DebuggerScopeEntry {
                    slot_ordinal: 4,
                    ..target.scope_entry
                },
                ..target
            }
        )
        .map_err(|reply| *reply),
        Err(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        })
    ));
    assert_eq!(locations.value_calls, 0);
    let snapshot = child_value_snapshot(&tabs, &mut locations, Some(&session), true, 1, target)
        .expect("exact same-stream paused slot must be reminted");
    assert_eq!(snapshot.target, target);
    assert_eq!(
        snapshot.preview,
        DebuggerValuePreview::Record(vec![(
            vec![0xd800],
            DebuggerValuePreview::Array(vec![
                Some(DebuggerValuePreview::NumberBits((-0.0_f64).to_bits())),
                None,
                Some(DebuggerValuePreview::StringUnits(vec![0xdc00])),
            ]),
        )])
    );
    assert_eq!(locations.value_calls, 1);
    locations.moved_root_offset = 42;
    assert!(matches!(
        child_value_snapshot(&tabs, &mut locations, Some(&session), true, 1, target)
            .map_err(|reply| *reply),
        Err(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        })
    ));
    assert_eq!(locations.value_calls, 1);
    locations.moved_root_offset = 41;
    locations.value_preview = Some(JavaScriptPageDebuggerValuePreview::BigIntBytes(vec![
        0;
        4_097
    ]));
    assert!(matches!(
        child_value_snapshot(&tabs, &mut locations, Some(&session), true, 1, target)
            .map_err(|reply| *reply),
        Err(DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            ..
        })
    ));
    locations.value_preview = Some(JavaScriptPageDebuggerValuePreview::Record(vec![
        (vec![b'a' as u16], JavaScriptPageDebuggerValuePreview::Null),
        (vec![b'a' as u16], JavaScriptPageDebuggerValuePreview::Null),
    ]));
    assert!(matches!(
        child_value_snapshot(&tabs, &mut locations, Some(&session), true, 1, target)
            .map_err(|reply| *reply),
        Err(DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            ..
        })
    ));
}
