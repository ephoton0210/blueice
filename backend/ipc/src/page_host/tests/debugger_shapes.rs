// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn private_value_target_requires_exact_root_or_nested_frame_shape() {
    let program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let frame = PageHostDebuggerFrame {
        tab_id: 7,
        document_generation: 3,
        program,
        code_unit_ordinal: 1,
        invocation_serial: 19,
    };
    let mut target = PageHostDebuggerValueTarget {
        tab_id: 7,
        document_generation: 3,
        program,
        frame: None,
        frame_index: 0,
        safe_point: PageHostDebuggerSafePoint {
            program,
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        },
        scope_entry: PageHostDebuggerScopeEntry {
            slot_ordinal: 2,
            scope_depth: 0,
        },
    };
    assert!(target.is_well_formed());
    let snapshot = PageHostDebuggerValueSnapshot {
        target,
        preview: PageHostDebuggerValuePreview::Undefined,
    };
    assert!(snapshot.is_well_formed());
    assert_eq!(
        serde_json::from_slice::<PageHostDebuggerValueSnapshot>(
            &serde_json::to_vec(&snapshot).unwrap()
        )
        .unwrap(),
        snapshot
    );
    target.frame_index = 1;
    assert!(!target.is_well_formed());
    target.frame_index = 0;
    target.frame = Some(frame);
    assert!(!target.is_well_formed());
    target.safe_point.code_unit_ordinal = 1;
    assert!(target.is_well_formed());
    target.frame_index = 1;
    assert!(!target.is_well_formed());
    target.safe_point.code_unit_ordinal = 0;
    assert!(target.is_well_formed());
    target.document_generation += 1;
    assert!(!target.is_well_formed());
    target.document_generation -= 1;
    target.safe_point.program.program_generation += 1;
    assert!(!target.is_well_formed());
    target.safe_point.program.program_generation -= 1;
    target.frame_index = 2;
    assert!(!target.is_well_formed());
    assert_eq!(PAGE_HOST_PROTOCOL_VERSION, 41);
}

#[test]
fn private_static_scope_targets_reject_non_root_and_incomplete_linked_stacks() {
    let entry_program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let dependency_program = PageHostDebuggerProgram {
        program_handle: 17,
        program_generation: 19,
    };
    let metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 23,
        metadata_generation: 29,
    };
    let scope_entry = PageHostDebuggerScopeEntry {
        slot_ordinal: 2,
        scope_depth: 0,
    };
    let value_target = PageHostDebuggerValueTarget {
        tab_id: 7,
        document_generation: 3,
        program: entry_program,
        frame: None,
        frame_index: 0,
        safe_point: PageHostDebuggerSafePoint {
            program: entry_program,
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        },
        scope_entry,
    };
    let ordinary = PageHostDebuggerStaticScopeTarget::Ordinary {
        metadata,
        target: value_target,
    };
    assert!(ordinary.is_well_formed());
    assert!(PageHostDebuggerStaticScopeTarget::Ordinary {
        metadata,
        target: PageHostDebuggerValueTarget {
            frame: Some(PageHostDebuggerFrame {
                tab_id: 7,
                document_generation: 3,
                program: entry_program,
                code_unit_ordinal: 1,
                invocation_serial: 31,
            }),
            frame_index: 1,
            ..value_target
        },
    }
    .is_well_formed());
    assert!(!PageHostDebuggerStaticScopeTarget::Ordinary {
        metadata,
        target: PageHostDebuggerValueTarget {
            safe_point: PageHostDebuggerSafePoint {
                code_unit_ordinal: 1,
                ..value_target.safe_point
            },
            ..value_target
        },
    }
    .is_well_formed());
    assert!(!PageHostDebuggerStaticScopeTarget::Ordinary {
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 0,
            ..metadata
        },
        target: value_target,
    }
    .is_well_formed());

    let frame = PageHostDebuggerLinkedFrame {
        tab_id: 7,
        document_generation: 3,
        entry_program,
        dependency_program,
        code_unit_ordinal: 1,
        invocation_serial: 37,
    };
    let stack = PageHostDebuggerLinkedStackSnapshot {
        frames: [
            PageHostDebuggerLinkedStackFrame {
                safe_point: PageHostDebuggerSafePoint {
                    program: dependency_program,
                    code_unit_ordinal: 1,
                    bytecode_offset: 2,
                },
                scope_entries: vec![],
                scope_truncated: false,
            },
            PageHostDebuggerLinkedStackFrame {
                safe_point: value_target.safe_point,
                scope_entries: vec![scope_entry],
                scope_truncated: false,
            },
        ],
        stack_truncated: false,
        max_scope_entries: 2,
    };
    let linked = PageHostDebuggerStaticScopeTarget::Linked {
        frame,
        expected_stack: Box::new(stack.clone()),
        frame_index: 1,
        metadata,
        scope_entry,
    };
    assert!(linked.is_well_formed());
    for (changed_stack, frame_index, entry) in [
        (stack.clone(), 0, scope_entry),
        (
            stack.clone(),
            1,
            PageHostDebuggerScopeEntry {
                slot_ordinal: 9,
                ..scope_entry
            },
        ),
        (
            {
                let mut s = stack.clone();
                s.frames[1].scope_truncated = true;
                s
            },
            1,
            scope_entry,
        ),
        (
            {
                let mut s = stack.clone();
                s.frames[0].scope_truncated = true;
                s
            },
            1,
            scope_entry,
        ),
        (
            {
                let mut s = stack.clone();
                s.frames[1].scope_entries.push(scope_entry);
                s
            },
            1,
            scope_entry,
        ),
        (
            {
                let mut s = stack.clone();
                s.frames[1].safe_point.program = dependency_program;
                s
            },
            1,
            scope_entry,
        ),
        (
            {
                let mut s = stack.clone();
                s.max_scope_entries = 0;
                s
            },
            1,
            scope_entry,
        ),
        (
            {
                let mut s = stack.clone();
                s.stack_truncated = true;
                s
            },
            1,
            scope_entry,
        ),
        (
            {
                let mut s = stack.clone();
                s.max_scope_entries = PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES + 1;
                s
            },
            1,
            scope_entry,
        ),
    ] {
        assert!(!PageHostDebuggerStaticScopeTarget::Linked {
            frame,
            expected_stack: Box::new(changed_stack),
            frame_index,
            metadata,
            scope_entry: entry,
        }
        .is_well_formed());
    }
    assert!(!PageHostDebuggerStaticScopeTarget::Linked {
        frame: PageHostDebuggerLinkedFrame {
            entry_program: dependency_program,
            ..frame
        },
        expected_stack: Box::new(stack.clone()),
        frame_index: 1,
        metadata,
        scope_entry,
    }
    .is_well_formed());
    assert!(!PageHostDebuggerStaticScopeTarget::Linked {
        frame,
        expected_stack: Box::new(stack),
        frame_index: 1,
        metadata: PageHostDebuggerMetadataHandle {
            metadata_generation: 0,
            ..metadata
        },
        scope_entry,
    }
    .is_well_formed());
}

#[test]
fn private_static_scope_relation_round_trips_without_values_or_displays() {
    let program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let target = PageHostDebuggerStaticScopeTarget::Ordinary {
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        target: PageHostDebuggerValueTarget {
            tab_id: 7,
            document_generation: 3,
            program,
            frame: None,
            frame_index: 0,
            safe_point: PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
            scope_entry: PageHostDebuggerScopeEntry {
                slot_ordinal: 2,
                scope_depth: 0,
            },
        },
    };
    let request = PageHostRequest::DescribeDebuggerStaticScopeRelation {
        target: Box::new(target.clone()),
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_request(&mut writer, &request).unwrap();
    assert_eq!(read_page_host_request(&mut reader).unwrap(), request);

    let relation = PageHostDebuggerStaticScopeRelation {
        target,
        symbol_type: PageHostDebuggerBlueTsMetadataSymbolType {
            symbol_id: 0,
            type_id: 0,
        },
    };
    assert!(relation.is_well_formed());
    let reply = PageHostReply::DebuggerStaticScopeRelation(Box::new(relation));
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);
    let serialized = serde_json::to_string(&reply).unwrap();
    for forbidden in ["preview", "display", "source_id", "runtime_type"] {
        assert!(!serialized.contains(forbidden));
    }

    let entry_program = program;
    let dependency_program = PageHostDebuggerProgram {
        program_handle: 31,
        program_generation: 37,
    };
    let frame = PageHostDebuggerLinkedFrame {
        tab_id: 7,
        document_generation: 3,
        entry_program,
        dependency_program,
        code_unit_ordinal: 1,
        invocation_serial: 41,
    };
    let scope_entry = PageHostDebuggerScopeEntry {
        slot_ordinal: 2,
        scope_depth: 0,
    };
    let linked = PageHostDebuggerStaticScopeTarget::Linked {
        frame,
        expected_stack: Box::new(PageHostDebuggerLinkedStackSnapshot {
            frames: [
                PageHostDebuggerLinkedStackFrame {
                    safe_point: PageHostDebuggerSafePoint {
                        program: dependency_program,
                        code_unit_ordinal: 1,
                        bytecode_offset: 2,
                    },
                    scope_entries: vec![],
                    scope_truncated: false,
                },
                PageHostDebuggerLinkedStackFrame {
                    safe_point: PageHostDebuggerSafePoint {
                        program: entry_program,
                        code_unit_ordinal: 0,
                        bytecode_offset: 4,
                    },
                    scope_entries: vec![scope_entry],
                    scope_truncated: false,
                },
            ],
            stack_truncated: false,
            max_scope_entries: 1,
        }),
        frame_index: 1,
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        scope_entry,
    };
    assert!(linked.is_well_formed());
    let request = PageHostRequest::DescribeDebuggerStaticScopeRelation {
        target: Box::new(linked.clone()),
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_request(&mut writer, &request).unwrap();
    assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
    let reply =
        PageHostReply::DebuggerStaticScopeRelation(Box::new(PageHostDebuggerStaticScopeRelation {
            target: linked,
            symbol_type: PageHostDebuggerBlueTsMetadataSymbolType {
                symbol_id: 0,
                type_id: 0,
            },
        }));
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);
    assert_eq!(PAGE_HOST_PROTOCOL_VERSION, 41);
}

#[test]
fn linked_module_private_wire_preserves_two_programs_and_sources() {
    let entry_program = PageHostDebuggerProgram {
        program_handle: 7,
        program_generation: 9,
    };
    let dependency_program = PageHostDebuggerProgram {
        program_handle: 8,
        program_generation: 10,
    };
    let frame = PageHostDebuggerLinkedFrame {
        tab_id: 3,
        document_generation: 4,
        entry_program,
        dependency_program,
        code_unit_ordinal: 1,
        invocation_serial: 11,
    };
    let child_point = PageHostDebuggerSafePoint {
        program: dependency_program,
        code_unit_ordinal: 1,
        bytecode_offset: 5,
    };
    let root_point = PageHostDebuggerSafePoint {
        program: entry_program,
        code_unit_ordinal: 0,
        bytecode_offset: 13,
    };
    let snapshot = PageHostDebuggerLinkedStackSnapshot {
        frames: [
            PageHostDebuggerLinkedStackFrame {
                safe_point: child_point,
                scope_entries: vec![],
                scope_truncated: false,
            },
            PageHostDebuggerLinkedStackFrame {
                safe_point: root_point,
                scope_entries: vec![],
                scope_truncated: false,
            },
        ],
        stack_truncated: false,
        max_scope_entries: 256,
    };
    let sources = [
        PageHostDebuggerLinkedSource {
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 100,
                metadata_generation: 101,
            },
            source_id: 1,
        },
        PageHostDebuggerLinkedSource {
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 102,
                metadata_generation: 103,
            },
            source_id: 2,
        },
    ];
    assert!(snapshot.is_well_formed(frame));
    assert!(!snapshot.is_well_formed(PageHostDebuggerLinkedFrame {
        dependency_program: entry_program,
        ..frame
    }));
    for request in [
        PageHostRequest::ArmDebuggerLinkedNestedSafePointBreakpoint {
            tab_id: 3,
            document_generation: 4,
            entry_program,
            safe_point: child_point,
        },
        PageHostRequest::GetDebuggerLinkedStackSnapshot {
            frame,
            max_scope_entries: 256,
        },
        PageHostRequest::DescribeDebuggerLinkedStackSpans {
            frame,
            expected_stack: snapshot.clone(),
            sources,
        },
        PageHostRequest::ResumeDebuggerLinkedNestedExecution { frame },
    ] {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_request(&mut writer, &request).unwrap();
        assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
    }
    let span = PageHostDebuggerBlueTsSafePointSpan {
        source_id: 1,
        start_byte: 0,
        end_byte: 1,
        coordinates: DebuggerSourceCoordinates {
            start_line: 1,
            start_column_utf16: 0,
            end_line: 1,
            end_column_utf16: 1,
        },
    };
    for reply in [
        PageHostReply::DebuggerLinkedNestedSafePointBreakpointArmed {
            tab_id: 3,
            document_generation: 4,
            entry_program,
            safe_point: child_point,
        },
        PageHostReply::DebuggerLinkedExecutionState {
            frame,
            state: PageHostDebuggerLinkedExecutionState::Paused {
                safe_point: child_point,
            },
        },
        PageHostReply::DebuggerLinkedStackSnapshot {
            frame,
            snapshot: Box::new(snapshot.clone()),
        },
        PageHostReply::DebuggerLinkedStackSpans {
            frame,
            snapshot: Box::new(snapshot),
            spans: Box::new([
                span,
                PageHostDebuggerBlueTsSafePointSpan {
                    source_id: 2,
                    ..span
                },
            ]),
        },
        PageHostReply::DebuggerLinkedNestedResumeRequested { frame },
    ] {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);
    }
}

#[test]
fn private_value_preview_validates_lossless_bounded_trees() {
    let preview = PageHostDebuggerValuePreview::Record(vec![(
        vec![0xd800],
        PageHostDebuggerValuePreview::Array(vec![
            Some(PageHostDebuggerValuePreview::NumberBits(
                (-0.0_f64).to_bits(),
            )),
            None,
            Some(PageHostDebuggerValuePreview::StringUnits(vec![0xdc00])),
        ]),
    )]);
    assert!(preview.is_well_formed());
    assert_eq!(
        serde_json::from_slice::<PageHostDebuggerValuePreview>(
            &serde_json::to_vec(&preview).unwrap()
        )
        .unwrap(),
        preview
    );
    let mut too_deep = PageHostDebuggerValuePreview::Null;
    for _ in 0..5 {
        too_deep = PageHostDebuggerValuePreview::Array(vec![Some(too_deep)]);
    }
    assert!(!too_deep.is_well_formed());
    assert!(!PageHostDebuggerValuePreview::Array(vec![None; 33]).is_well_formed());
    assert!(!PageHostDebuggerValuePreview::Array(vec![
        Some(PageHostDebuggerValuePreview::Array(
            vec![None; 32]
        ));
        9
    ])
    .is_well_formed());
    assert!(!PageHostDebuggerValuePreview::BigIntBytes(vec![0; 4_097]).is_well_formed());
    assert!(PageHostDebuggerValuePreview::StringUnits(vec![0; 2_048]).is_well_formed());
    assert!(!PageHostDebuggerValuePreview::Record(vec![
        (
            vec![b'a' as u16],
            PageHostDebuggerValuePreview::StringUnits(vec![0; 1_024])
        ),
        (
            vec![b'b' as u16],
            PageHostDebuggerValuePreview::StringUnits(vec![0; 1_024])
        ),
    ])
    .is_well_formed());
    assert!(!PageHostDebuggerValuePreview::Record(vec![(
        vec![b'x' as u16; 2_049],
        PageHostDebuggerValuePreview::Null,
    )])
    .is_well_formed());
    assert!(!PageHostDebuggerValuePreview::Record(vec![
        (vec![b'a' as u16], PageHostDebuggerValuePreview::Null),
        (vec![b'a' as u16], PageHostDebuggerValuePreview::Bool(true)),
    ])
    .is_well_formed());
}

#[test]
fn active_frame_wire_identity_is_exact_source_free_and_nonzero() {
    let program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let frame = PageHostDebuggerFrame {
        tab_id: 7,
        document_generation: 3,
        program,
        code_unit_ordinal: 1,
        invocation_serial: 19,
    };
    let point = PageHostDebuggerSafePoint {
        program,
        code_unit_ordinal: 1,
        bytecode_offset: 4,
    };
    assert!(frame.is_well_formed());
    assert!(frame.matches_safe_point(point));
    let wire = serde_json::to_vec(&frame).unwrap();
    assert_eq!(
        serde_json::from_slice::<PageHostDebuggerFrame>(&wire).unwrap(),
        frame
    );
    assert!(!String::from_utf8(wire).unwrap().contains("source"));

    for invalid in [
        PageHostDebuggerFrame { tab_id: 0, ..frame },
        PageHostDebuggerFrame {
            document_generation: 0,
            ..frame
        },
        PageHostDebuggerFrame {
            program: PageHostDebuggerProgram {
                program_generation: 0,
                ..program
            },
            ..frame
        },
        PageHostDebuggerFrame {
            code_unit_ordinal: 0,
            ..frame
        },
        PageHostDebuggerFrame {
            invocation_serial: 0,
            ..frame
        },
    ] {
        assert!(!invalid.is_well_formed());
        assert!(!invalid.matches_safe_point(point));
    }
    assert!(!frame.matches_safe_point(PageHostDebuggerSafePoint {
        code_unit_ordinal: 2,
        ..point
    }));
    assert!(!frame.matches_safe_point(PageHostDebuggerSafePoint {
        program: PageHostDebuggerProgram {
            program_handle: 12,
            ..program
        },
        ..point
    }));
    for different in [
        PageHostDebuggerFrame { tab_id: 8, ..frame },
        PageHostDebuggerFrame {
            document_generation: 4,
            ..frame
        },
        PageHostDebuggerFrame {
            program: PageHostDebuggerProgram {
                program_generation: 14,
                ..program
            },
            ..frame
        },
        PageHostDebuggerFrame {
            code_unit_ordinal: 2,
            ..frame
        },
        PageHostDebuggerFrame {
            invocation_serial: 20,
            ..frame
        },
    ] {
        assert!(different.is_well_formed());
        assert_ne!(different, frame);
    }
}

#[test]
fn nested_frame_commands_and_states_round_trip_on_private_wire() {
    let program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let safe_point = PageHostDebuggerSafePoint {
        program,
        code_unit_ordinal: 1,
        bytecode_offset: 4,
    };
    let frame = PageHostDebuggerFrame {
        tab_id: 7,
        document_generation: 3,
        program,
        code_unit_ordinal: 1,
        invocation_serial: 19,
    };
    let value_target = PageHostDebuggerValueTarget {
        tab_id: 7,
        document_generation: 3,
        program,
        frame: Some(frame),
        frame_index: 0,
        safe_point,
        scope_entry: PageHostDebuggerScopeEntry {
            slot_ordinal: 2,
            scope_depth: 0,
        },
    };
    for request in [
        PageHostRequest::ArmDebuggerNestedSafePointBreakpoint {
            tab_id: 7,
            document_generation: 3,
            safe_point,
        },
        PageHostRequest::StepDebuggerNestedInstruction { frame },
        PageHostRequest::ResumeDebuggerNestedExecution { frame },
        PageHostRequest::GetDebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 3,
            program,
            frame: Some(frame),
            max_frames: 1,
            max_scope_entries: 2,
        },
        PageHostRequest::GetDebuggerValueSnapshot {
            target: value_target,
        },
    ] {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_request(&mut writer, &request).unwrap();
        assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
    }
    for reply in [
        PageHostReply::DebuggerNestedSafePointBreakpointArmed {
            tab_id: 7,
            document_generation: 3,
            safe_point,
        },
        PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 3,
            program,
            state: PageHostDebuggerExecutionState::NestedPaused { frame, safe_point },
        },
        PageHostReply::DebuggerNestedStepRequested { frame },
        PageHostReply::DebuggerNestedResumeRequested { frame },
        PageHostReply::DebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 3,
            program,
            frame: Some(frame),
            snapshot: PageHostDebuggerStackSnapshot {
                frames: vec![PageHostDebuggerStackFrame {
                    code_unit_ordinal: 1,
                    bytecode_offset: 4,
                    scope_entries: vec![PageHostDebuggerScopeEntry {
                        slot_ordinal: 2,
                        scope_depth: 0,
                    }],
                    scope_truncated: true,
                }],
                stack_truncated: true,
            },
        },
        PageHostReply::DebuggerValueSnapshot(Box::new(PageHostDebuggerValueSnapshot {
            target: value_target,
            preview: PageHostDebuggerValuePreview::Array(vec![
                Some(PageHostDebuggerValuePreview::NumberBits(7.0_f64.to_bits())),
                None,
            ]),
        })),
        PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 3,
            program,
            state: PageHostDebuggerExecutionState::NestedStepping { frame },
        },
        PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 3,
            program,
            state: PageHostDebuggerExecutionState::NestedResuming { frame },
        },
    ] {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);
    }
}

#[test]
fn symbol_type_relation_round_trips_on_private_socket() {
    let program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 17,
        metadata_generation: 19,
    };
    let request = PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
        tab_id: 7,
        document_generation: 3,
        program,
        metadata,
        symbol_id: 1,
        type_id: 2,
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_request(&mut writer, &request).unwrap();
    assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
    let reply = PageHostReply::DebuggerBlueTsMetadataSymbolType {
        tab_id: 7,
        document_generation: 3,
        program,
        metadata,
        symbol_type: PageHostDebuggerBlueTsMetadataSymbolType {
            symbol_id: 1,
            type_id: 2,
        },
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);

    let click_reply = PageHostReply::ClickDispatched {
        tab_id: 7,
        document_generation: 3,
        default_prevented: true,
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &click_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), click_reply);
}

#[test]
fn symbol_contract_relation_round_trips_on_private_socket() {
    let program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 17,
        metadata_generation: 19,
    };
    let request = PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
        tab_id: 7,
        document_generation: 3,
        program,
        metadata,
        symbol_id: 1,
        contract_id: 2,
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_request(&mut writer, &request).unwrap();
    assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
    let reply = PageHostReply::DebuggerBlueTsMetadataSymbolContract {
        tab_id: 7,
        document_generation: 3,
        program,
        metadata,
        symbol_contract: PageHostDebuggerBlueTsMetadataSymbolContract {
            symbol_id: 1,
            contract_id: 2,
        },
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);
}
