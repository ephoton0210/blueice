// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn symbol_breakpoint_candidate_requires_executable_kind_and_contained_exact_span() {
    let program = DebuggerProgram {
        realm: realm(),
        program_handle: 11,
        program_generation: 12,
    };
    let metadata = DebuggerStaticMetadataHandle {
        program,
        metadata_handle: 21,
        metadata_generation: 22,
    };
    let source = DebuggerStaticMetadataSourceId {
        metadata,
        source_id: 2,
    };
    let symbol = DebuggerStaticMetadataSymbolId {
        metadata,
        symbol_id: 3,
    };
    let coordinates = DebuggerSourceCoordinates {
        start_line: 0,
        start_column_utf16: 10,
        end_line: 0,
        end_column_utf16: 20,
    };
    let location = DebuggerStaticMetadataSymbolLocation {
        symbol,
        source,
        start_byte: 10,
        end_byte: 20,
        coordinates,
    };
    let display = DebuggerStaticMetadataSymbolDisplay {
        symbol,
        display: "answer".into(),
        kind: DebuggerStaticMetadataSymbolKind::Variable,
        exported: true,
    };
    let point = DebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: 4,
    };
    let binding = DebuggerStaticMetadataSourceBreakpoint {
        target: DebuggerStaticMetadataSourceBreakpointTarget {
            source,
            source_byte: location.start_byte,
        },
        safe_point: Some(point),
    };
    let span = DebuggerStaticMetadataSafePointSpan {
        safe_point: point,
        source,
        start_byte: 10,
        end_byte: 20,
        coordinates,
    };
    assert_eq!(
        location.executable_breakpoint_candidate(&display, binding, span),
        Some(point)
    );
    for kind in [
        DebuggerStaticMetadataSymbolKind::Interface,
        DebuggerStaticMetadataSymbolKind::TypeAlias,
        DebuggerStaticMetadataSymbolKind::Import,
    ] {
        assert_eq!(
            location.executable_breakpoint_candidate(
                &DebuggerStaticMetadataSymbolDisplay {
                    kind,
                    ..display.clone()
                },
                binding,
                span,
            ),
            None
        );
    }
    assert_eq!(
        location.executable_breakpoint_candidate(
            &display,
            DebuggerStaticMetadataSourceBreakpoint {
                safe_point: None,
                ..binding
            },
            span,
        ),
        None
    );
    assert_eq!(
        location.executable_breakpoint_candidate(
            &display,
            binding,
            DebuggerStaticMetadataSafePointSpan {
                start_byte: 21,
                end_byte: 25,
                ..span
            },
        ),
        None
    );
    assert_eq!(
        location.executable_breakpoint_candidate(
            &display,
            DebuggerStaticMetadataSourceBreakpoint {
                target: DebuggerStaticMetadataSourceBreakpointTarget {
                    source_byte: 11,
                    ..binding.target
                },
                ..binding
            },
            span,
        ),
        None
    );
    assert_eq!(
        location.executable_breakpoint_candidate(
            &display,
            binding,
            DebuggerStaticMetadataSafePointSpan {
                source: DebuggerStaticMetadataSourceId {
                    source_id: 4,
                    ..source
                },
                ..span
            },
        ),
        None
    );
}

#[test]
fn linked_stack_shapes_require_two_distinct_programs_and_source_attachments() {
    let programs = [11, 21].map(|handle| DebuggerProgram {
        realm: realm(),
        program_handle: handle,
        program_generation: handle + 1,
    });
    let points = [
        DebuggerSafePoint {
            program: programs[0],
            code_unit_ordinal: 1,
            bytecode_offset: 0,
        },
        DebuggerSafePoint {
            program: programs[1],
            code_unit_ordinal: 0,
            bytecode_offset: 8,
        },
    ];
    let stack = DebuggerLinkedStackSnapshot {
        frames: [0, 1].map(|index| DebuggerLinkedStackFrame {
            frame: DebuggerLinkedFrame {
                program: programs[index],
                code_unit_ordinal: points[index].code_unit_ordinal,
                core_instance: [7; 16],
                frame_handle: index as u64 + 1,
            },
            safe_point: points[index],
        }),
    };
    assert!(stack.is_well_formed());
    let arm = DebuggerLinkedArmTarget {
        entry: programs[1],
        dependency_safe_point: points[0],
    };
    assert!(arm.is_well_formed());
    assert!(!DebuggerLinkedArmTarget {
        entry: programs[0],
        ..arm
    }
    .is_well_formed());
    assert!(!DebuggerLinkedArmTarget {
        dependency_safe_point: points[1],
        ..arm
    }
    .is_well_formed());
    for state in [
        DebuggerLinkedExecutionState::Pending,
        DebuggerLinkedExecutionState::Paused { stack },
        DebuggerLinkedExecutionState::Resuming {
            frame: stack.frames[0].frame,
        },
        DebuggerLinkedExecutionState::Completed,
    ] {
        assert!(state.is_well_formed(programs[1]));
    }
    assert!(!DebuggerLinkedExecutionState::Paused { stack }.is_well_formed(programs[0]));
    assert!(!DebuggerLinkedExecutionState::Resuming {
        frame: stack.frames[1].frame,
    }
    .is_well_formed(programs[1]));
    assert!(!DebuggerLinkedStackSnapshot {
        frames: [stack.frames[1], stack.frames[0]],
    }
    .is_well_formed());
    assert!(!DebuggerLinkedStackSnapshot {
        frames: [
            stack.frames[0],
            DebuggerLinkedStackFrame {
                frame: DebuggerLinkedFrame {
                    frame_handle: stack.frames[0].frame.frame_handle,
                    ..stack.frames[1].frame
                },
                ..stack.frames[1]
            },
        ],
    }
    .is_well_formed());
    let sources = [0, 1].map(|index| DebuggerStaticMetadataSourceId {
        metadata: DebuggerStaticMetadataHandle {
            program: programs[index],
            metadata_handle: index as u64 + 31,
            metadata_generation: index as u64 + 41,
        },
        source_id: index as u32,
    });
    let target = DebuggerLinkedStackCoordinatesTarget {
        expected_stack: stack,
        sources,
    };
    assert!(target.is_well_formed());
    assert!(!DebuggerLinkedStackCoordinatesTarget {
        sources: [sources[1], sources[0]],
        ..target
    }
    .is_well_formed());
    let spans = [0, 1].map(|index| DebuggerStaticMetadataSafePointSpan {
        safe_point: points[index],
        source: sources[index],
        start_byte: 0,
        end_byte: 1,
        coordinates: DebuggerSourceCoordinates {
            start_line: 0,
            start_column_utf16: 0,
            end_line: 0,
            end_column_utf16: 1,
        },
    });
    let result = DebuggerLinkedStackCoordinates { stack, spans };
    assert!(result.is_well_formed());
    assert!(!DebuggerLinkedStackCoordinates {
        spans: [spans[1], spans[0]],
        ..result
    }
    .is_well_formed());
    let encoded = serde_json::to_vec(&target).unwrap();
    assert_eq!(
        serde_json::from_slice::<DebuggerLinkedStackCoordinatesTarget>(&encoded).unwrap(),
        target
    );
    let encoded = serde_json::to_vec(&DebuggerLinkedExecutionState::Paused { stack }).unwrap();
    assert_eq!(
        serde_json::from_slice::<DebuggerLinkedExecutionState>(&encoded).unwrap(),
        DebuggerLinkedExecutionState::Paused { stack }
    );
}

#[test]
fn staged_static_scope_shapes_bind_one_exact_root_and_metadata_owner() {
    let dependency = DebuggerProgram {
        realm: realm(),
        program_handle: 11,
        program_generation: 12,
    };
    let entry = DebuggerProgram {
        realm: realm(),
        program_handle: 21,
        program_generation: 22,
    };
    let points = [
        DebuggerSafePoint {
            program: dependency,
            code_unit_ordinal: 1,
            bytecode_offset: 4,
        },
        DebuggerSafePoint {
            program: entry,
            code_unit_ordinal: 0,
            bytecode_offset: 8,
        },
    ];
    let stack = DebuggerLinkedStackSnapshot {
        frames: [0, 1].map(|index| DebuggerLinkedStackFrame {
            frame: DebuggerLinkedFrame {
                program: points[index].program,
                code_unit_ordinal: points[index].code_unit_ordinal,
                core_instance: [7; 16],
                frame_handle: index as u64 + 1,
            },
            safe_point: points[index],
        }),
    };
    let slot = DebuggerScopeEntry {
        slot_ordinal: 0,
        scope_depth: 0,
    };
    let linked_scopes = DebuggerLinkedScopeSnapshot {
        stack,
        frame_index: 1,
        entries: vec![slot],
        scope_truncated: false,
        max_scope_entries: 4,
    };
    assert!(linked_scopes.is_well_formed());
    assert!(!DebuggerLinkedScopeSnapshot {
        frame_index: 0,
        ..linked_scopes.clone()
    }
    .is_well_formed());
    assert!(!DebuggerLinkedScopeSnapshot {
        entries: vec![
            slot,
            DebuggerScopeEntry {
                scope_depth: 1,
                ..slot
            }
        ],
        ..linked_scopes.clone()
    }
    .is_well_formed());
    assert!(!DebuggerLinkedScopeSnapshot {
        max_scope_entries: 0,
        ..linked_scopes.clone()
    }
    .is_well_formed());
    assert!(!DebuggerLinkedScopeSnapshot {
        scope_truncated: true,
        ..linked_scopes.clone()
    }
    .is_well_formed());
    let linked_target = DebuggerLinkedScopeTarget {
        stack,
        frame_index: 1,
        scope_entry: slot,
    };
    assert!(linked_target.is_well_formed());
    let entry_metadata = DebuggerStaticMetadataHandle {
        program: entry,
        metadata_handle: 31,
        metadata_generation: 32,
    };
    let dependency_metadata = DebuggerStaticMetadataHandle {
        program: dependency,
        metadata_handle: 41,
        metadata_generation: 42,
    };
    let linked = DebuggerStaticScopeTarget::Linked {
        metadata: entry_metadata,
        target: linked_target,
    };
    assert!(linked.is_well_formed());
    assert!(!DebuggerStaticScopeTarget::Linked {
        metadata: dependency_metadata,
        target: linked_target,
    }
    .is_well_formed());
    assert!(!DebuggerStaticScopeTarget::Linked {
        metadata: entry_metadata,
        target: DebuggerLinkedScopeTarget {
            frame_index: 0,
            ..linked_target
        },
    }
    .is_well_formed());
    let ordinary_slot = DebuggerValueTarget {
        program: entry,
        frame: None,
        frame_index: 0,
        safe_point: points[1],
        scope_entry: slot,
    };
    let ordinary = DebuggerStaticScopeTarget::Ordinary {
        metadata: entry_metadata,
        target: ordinary_slot,
    };
    assert!(ordinary.is_well_formed());
    assert!(!DebuggerStaticScopeTarget::Ordinary {
        metadata: dependency_metadata,
        target: ordinary_slot,
    }
    .is_well_formed());
    assert!(!DebuggerStaticScopeTarget::Ordinary {
        metadata: entry_metadata,
        target: DebuggerValueTarget {
            safe_point: DebuggerSafePoint {
                code_unit_ordinal: 1,
                ..points[1]
            },
            ..ordinary_slot
        },
    }
    .is_well_formed());
    let relation = DebuggerStaticScopeRelation {
        target: linked,
        symbol: DebuggerStaticMetadataSymbolId {
            metadata: entry_metadata,
            symbol_id: 2,
        },
        static_type: DebuggerStaticMetadataTypeId {
            metadata: entry_metadata,
            type_id: 1,
        },
    };
    assert!(relation.is_well_formed());
    assert!(!DebuggerStaticScopeRelation {
        static_type: DebuggerStaticMetadataTypeId {
            metadata: dependency_metadata,
            type_id: 1,
        },
        ..relation
    }
    .is_well_formed());
    for target in [ordinary, linked] {
        let encoded = serde_json::to_vec(&target).unwrap();
        assert_eq!(
            serde_json::from_slice::<DebuggerStaticScopeTarget>(&encoded).unwrap(),
            target
        );
    }
    let encoded = serde_json::to_vec(&linked_scopes).unwrap();
    assert_eq!(
        serde_json::from_slice::<DebuggerLinkedScopeSnapshot>(&encoded).unwrap(),
        linked_scopes
    );
    let encoded = serde_json::to_vec(&relation).unwrap();
    assert_eq!(
        serde_json::from_slice::<DebuggerStaticScopeRelation>(&encoded).unwrap(),
        relation
    );
}

#[test]
fn public_static_scope_grant_and_wire_are_independent_of_values() {
    let manifest =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            static_scope_relation: true,
            ..Default::default()
        });
    assert_eq!(DEBUGGER_PROTOCOL_VERSION, 41);
    assert_eq!(DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION, 5);
    assert_eq!(
        manifest.capabilities,
        vec![
            DebuggerMetadataCapability::OpaqueInventory,
            DebuggerMetadataCapability::OpaqueTypeInventory,
            DebuggerMetadataCapability::OpaqueSymbolInventory,
            DebuggerMetadataCapability::OpaqueStaticScopeRelation,
        ]
    );
    for capabilities in [
        vec![DebuggerMetadataCapability::OpaqueStaticScopeRelation],
        vec![
            DebuggerMetadataCapability::OpaqueInventory,
            DebuggerMetadataCapability::OpaqueStaticScopeRelation,
        ],
    ] {
        assert!(!DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities,
        }
        .is_well_formed());
    }
    let hello = hello(manifest.clone());
    let ack = negotiate(&hello, &manifest);
    let session = metadata_session_authorization(&hello, &ack).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueStaticScopeRelation));
    assert!(!session.permits_bounded_values());
    let no_owner_ack = negotiate(&hello, &DebuggerMetadataCapabilityManifest::empty());
    assert!(!metadata_session_authorization(&hello, &no_owner_ack)
        .unwrap()
        .permits(DebuggerMetadataCapability::OpaqueStaticScopeRelation));

    let dependency = DebuggerProgram {
        realm: realm(),
        program_handle: 11,
        program_generation: 12,
    };
    let entry = DebuggerProgram {
        realm: realm(),
        program_handle: 21,
        program_generation: 22,
    };
    let points = [
        DebuggerSafePoint {
            program: dependency,
            code_unit_ordinal: 1,
            bytecode_offset: 4,
        },
        DebuggerSafePoint {
            program: entry,
            code_unit_ordinal: 0,
            bytecode_offset: 8,
        },
    ];
    let stack = DebuggerLinkedStackSnapshot {
        frames: [0, 1].map(|index| DebuggerLinkedStackFrame {
            frame: DebuggerLinkedFrame {
                program: points[index].program,
                code_unit_ordinal: points[index].code_unit_ordinal,
                core_instance: [7; 16],
                frame_handle: index as u64 + 1,
            },
            safe_point: points[index],
        }),
    };
    let scopes = DebuggerLinkedScopeSnapshot {
        stack,
        frame_index: 1,
        entries: vec![DebuggerScopeEntry {
            slot_ordinal: 0,
            scope_depth: 0,
        }],
        scope_truncated: false,
        max_scope_entries: 4,
    };
    let target = DebuggerStaticScopeTarget::Linked {
        metadata: DebuggerStaticMetadataHandle {
            program: entry,
            metadata_handle: 31,
            metadata_generation: 41,
        },
        target: DebuggerLinkedScopeTarget {
            stack,
            frame_index: 1,
            scope_entry: scopes.entries[0],
        },
    };
    let relation = DebuggerStaticScopeRelation {
        target,
        symbol: DebuggerStaticMetadataSymbolId {
            metadata: target.metadata(),
            symbol_id: 2,
        },
        static_type: DebuggerStaticMetadataTypeId {
            metadata: target.metadata(),
            type_id: 1,
        },
    };
    for request in [
        DebuggerRequest::GetLinkedScopes {
            expected_stack: stack,
            max_scope_entries: 4,
        },
        DebuggerRequest::GetStaticScopeRelation { target },
    ] {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_debugger_request(&mut sender, &request).unwrap();
        assert_eq!(read_debugger_request(&mut receiver).unwrap(), request);
        assert!(matches!(
            negotiate(&request, &manifest),
            DebuggerReply::Error {
                code: DebuggerErrorCode::ProtocolVersion,
                ..
            }
        ));
    }
    for reply in [
        DebuggerReply::LinkedScopes(Box::new(scopes)),
        DebuggerReply::StaticScopeRelation(Box::new(relation)),
    ] {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_debugger_reply(&mut sender, &reply).unwrap();
        assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);
    }
}

#[test]
fn linked_control_family_round_trips_on_one_real_socket() {
    let dependency = DebuggerProgram {
        realm: realm(),
        program_handle: 11,
        program_generation: 12,
    };
    let entry = DebuggerProgram {
        realm: realm(),
        program_handle: 21,
        program_generation: 22,
    };
    let points = [
        DebuggerSafePoint {
            program: dependency,
            code_unit_ordinal: 1,
            bytecode_offset: 0,
        },
        DebuggerSafePoint {
            program: entry,
            code_unit_ordinal: 0,
            bytecode_offset: 8,
        },
    ];
    let stack = DebuggerLinkedStackSnapshot {
        frames: [0, 1].map(|index| DebuggerLinkedStackFrame {
            frame: DebuggerLinkedFrame {
                program: points[index].program,
                code_unit_ordinal: points[index].code_unit_ordinal,
                core_instance: [7; 16],
                frame_handle: index as u64 + 1,
            },
            safe_point: points[index],
        }),
    };
    let sources = [0, 1].map(|index| DebuggerStaticMetadataSourceId {
        metadata: DebuggerStaticMetadataHandle {
            program: points[index].program,
            metadata_handle: index as u64 + 31,
            metadata_generation: index as u64 + 41,
        },
        source_id: index as u32,
    });
    let arm = DebuggerLinkedArmTarget {
        entry,
        dependency_safe_point: points[0],
    };
    let target = DebuggerLinkedStackCoordinatesTarget {
        expected_stack: stack,
        sources,
    };
    let requests = [
        DebuggerRequest::ArmLinkedNestedSafePointBreakpoint { target: arm },
        DebuggerRequest::GetLinkedExecutionState { entry },
        DebuggerRequest::GetLinkedStack {
            top_frame: stack.frames[0].frame,
        },
        DebuggerRequest::ResumeLinkedNestedExecution {
            top_frame: stack.frames[0].frame,
        },
        DebuggerRequest::GetLinkedStackCoordinates { target },
    ];
    let spans = [0, 1].map(|index| DebuggerStaticMetadataSafePointSpan {
        safe_point: points[index],
        source: sources[index],
        start_byte: 0,
        end_byte: 1,
        coordinates: DebuggerSourceCoordinates {
            start_line: 0,
            start_column_utf16: 0,
            end_line: 0,
            end_column_utf16: 1,
        },
    });
    let replies = [
        DebuggerReply::LinkedNestedSafePointBreakpointArmed { target: arm },
        DebuggerReply::LinkedExecutionState {
            entry,
            state: Box::new(DebuggerLinkedExecutionState::Paused { stack }),
        },
        DebuggerReply::LinkedStack(Box::new(stack)),
        DebuggerReply::LinkedNestedResumeRequested {
            top_frame: stack.frames[0].frame,
        },
        DebuggerReply::LinkedStackCoordinates(Box::new(DebuggerLinkedStackCoordinates {
            stack,
            spans,
        })),
    ];
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    for request in requests {
        write_debugger_request(&mut writer, &request).unwrap();
        assert_eq!(read_debugger_request(&mut reader).unwrap(), request);
    }
    for reply in replies {
        write_debugger_reply(&mut writer, &reply).unwrap();
        assert_eq!(read_debugger_reply(&mut reader).unwrap(), reply);
    }
}

#[test]
fn nested_frame_identity_commands_and_states_round_trip_source_free() {
    let program = DebuggerProgram {
        realm: realm(),
        program_handle: 11,
        program_generation: 13,
    };
    let safe_point = DebuggerSafePoint {
        program,
        code_unit_ordinal: 1,
        bytecode_offset: 4,
    };
    let frame = DebuggerFrame {
        program,
        code_unit_ordinal: 1,
        core_instance: [7; 16],
        frame_handle: 19,
    };
    assert!(frame.matches_safe_point(safe_point));
    assert!(!DebuggerFrame {
        frame_handle: 0,
        ..frame
    }
    .is_well_formed());
    assert!(!DebuggerFrame {
        core_instance: [0; 16],
        ..frame
    }
    .is_well_formed());
    assert!(!frame.matches_safe_point(DebuggerSafePoint {
        code_unit_ordinal: 2,
        ..safe_point
    }));
    for request in [
        DebuggerRequest::ArmNestedSafePointBreakpoint { safe_point },
        DebuggerRequest::StepNestedInstruction { frame },
        DebuggerRequest::ResumeNestedExecution { frame },
        DebuggerRequest::GetStack {
            program,
            frame: Some(frame),
            max_frames: 1,
        },
        DebuggerRequest::GetScopes {
            program,
            frame: Some(frame),
            frame_index: 0,
            expected_safe_point: safe_point,
            max_scope_entries: 1,
        },
    ] {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_debugger_request(&mut writer, &request).unwrap();
        assert_eq!(read_debugger_request(&mut reader).unwrap(), request);
    }
    for reply in [
        DebuggerReply::NestedSafePointBreakpointArmed { safe_point },
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::NestedPaused { frame, safe_point },
        },
        DebuggerReply::NestedStepRequested { frame },
        DebuggerReply::NestedResumeRequested { frame },
        DebuggerReply::Stack(DebuggerStackSnapshot {
            program,
            frame: Some(frame),
            safe_points: vec![safe_point],
            stack_truncated: true,
        }),
        DebuggerReply::Scopes(DebuggerScopeSnapshot {
            program,
            frame: Some(frame),
            frame_index: 0,
            safe_point,
            entries: vec![DebuggerScopeEntry {
                slot_ordinal: 2,
                scope_depth: 0,
            }],
            scope_truncated: true,
        }),
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::NestedStepping { frame },
        },
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::NestedResuming { frame },
        },
    ] {
        let bytes = serde_json::to_vec(&reply).unwrap();
        let json = String::from_utf8(bytes).unwrap();
        assert!(!json.contains("invocation_serial"));
        assert!(!json.contains("source_text"));
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_debugger_reply(&mut writer, &reply).unwrap();
        assert_eq!(read_debugger_reply(&mut reader).unwrap(), reply);
    }
}

#[test]
fn stack_coordinate_values_require_one_exact_stack_and_one_metadata_parent() {
    let program = DebuggerProgram {
        realm: realm(),
        program_handle: 11,
        program_generation: 13,
    };
    let child = DebuggerSafePoint {
        program,
        code_unit_ordinal: 1,
        bytecode_offset: 0,
    };
    let root = DebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: 41,
    };
    let frame = DebuggerFrame {
        program,
        code_unit_ordinal: 1,
        core_instance: [7; 16],
        frame_handle: 19,
    };
    let stack = DebuggerStackSnapshot {
        program,
        frame: Some(frame),
        safe_points: vec![child, root],
        stack_truncated: false,
    };
    let metadata = DebuggerStaticMetadataHandle {
        program,
        metadata_handle: 23,
        metadata_generation: 29,
    };
    let first_source = DebuggerStaticMetadataSourceId {
        metadata,
        source_id: 0,
    };
    let second_source = DebuggerStaticMetadataSourceId {
        metadata,
        source_id: 1,
    };
    let target = DebuggerStackCoordinatesTarget {
        expected_stack: stack.clone(),
        sources: vec![first_source, second_source],
    };
    assert!(target.is_well_formed());
    assert_eq!(
        serde_json::from_slice::<DebuggerStackCoordinatesTarget>(
            &serde_json::to_vec(&target).unwrap()
        )
        .unwrap(),
        target
    );

    let mut malformed = target.clone();
    malformed.sources.pop();
    assert!(!malformed.is_well_formed());
    malformed = target.clone();
    malformed.sources[1].metadata.metadata_generation += 1;
    assert!(!malformed.is_well_formed());
    malformed = target.clone();
    malformed.expected_stack.frame = None;
    assert!(!malformed.is_well_formed());
    malformed = target.clone();
    malformed.expected_stack.safe_points[1]
        .program
        .program_generation += 1;
    assert!(!malformed.is_well_formed());
    malformed = target.clone();
    malformed.expected_stack.safe_points = vec![child; DEBUGGER_MAX_STACK_FRAMES as usize + 1];
    malformed.sources = vec![first_source; malformed.expected_stack.safe_points.len()];
    assert!(!malformed.is_well_formed());

    let span = |safe_point, source, start_byte, end_byte| DebuggerStaticMetadataSafePointSpan {
        safe_point,
        source,
        start_byte,
        end_byte,
        coordinates: DebuggerSourceCoordinates {
            start_line: 0,
            start_column_utf16: start_byte,
            end_line: 0,
            end_column_utf16: end_byte,
        },
    };
    let coordinates = DebuggerStackCoordinates {
        stack,
        spans: vec![
            span(child, first_source, 8, 40),
            span(root, second_source, 42, 60),
        ],
    };
    assert!(coordinates.is_well_formed());
    assert_eq!(
        serde_json::from_slice::<DebuggerStackCoordinates>(
            &serde_json::to_vec(&coordinates).unwrap()
        )
        .unwrap(),
        coordinates
    );
    let request = DebuggerRequest::GetStackCoordinates {
        target: target.clone(),
    };
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_request(&mut sender, &request).unwrap();
    assert_eq!(read_debugger_request(&mut receiver).unwrap(), request);
    let reply = DebuggerReply::StackCoordinates(coordinates.clone());
    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    write_debugger_reply(&mut sender, &reply).unwrap();
    assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);
    let mut malformed_reply = coordinates.clone();
    malformed_reply.spans.swap(0, 1);
    assert!(!malformed_reply.is_well_formed());
    malformed_reply = coordinates.clone();
    malformed_reply.spans[1].source.metadata.metadata_handle += 1;
    assert!(!malformed_reply.is_well_formed());
    malformed_reply = coordinates.clone();
    malformed_reply.spans.pop();
    assert!(!malformed_reply.is_well_formed());

    let root_only = DebuggerStackSnapshot {
        program,
        frame: None,
        safe_points: vec![root],
        stack_truncated: false,
    };
    assert!(root_only.is_well_formed());
    assert!(!DebuggerStackSnapshot {
        safe_points: vec![child],
        ..root_only.clone()
    }
    .is_well_formed());
    assert!(!DebuggerStackSnapshot {
        stack_truncated: true,
        ..root_only
    }
    .is_well_formed());
    assert!(!DebuggerStackSnapshot {
        safe_points: vec![],
        ..coordinates.stack.clone()
    }
    .is_well_formed());
    assert!(!DebuggerStackSnapshot {
        safe_points: vec![child],
        ..coordinates.stack.clone()
    }
    .is_well_formed());
    assert!(DebuggerStackSnapshot {
        safe_points: vec![child],
        stack_truncated: true,
        ..coordinates.stack
    }
    .is_well_formed());
}
