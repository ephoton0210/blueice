// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn public_value_targets_require_exact_frame_and_scope_identity() {
    let program = DebuggerProgram {
        realm: realm(),
        program_handle: 12,
        program_generation: 5,
    };
    let frame = DebuggerFrame {
        program,
        code_unit_ordinal: 2,
        core_instance: [1; 16],
        frame_handle: 9,
    };
    let root = DebuggerValueTarget {
        program,
        frame: None,
        frame_index: 0,
        safe_point: DebuggerSafePoint {
            program,
            code_unit_ordinal: 0,
            bytecode_offset: 8,
        },
        scope_entry: DebuggerScopeEntry {
            slot_ordinal: 512,
            scope_depth: 1,
        },
    };
    assert!(root.is_well_formed());
    let snapshot = DebuggerValueSnapshot {
        target: root,
        preview: DebuggerValuePreview::Undefined,
    };
    assert!(snapshot.is_well_formed());
    assert_eq!(
        serde_json::from_slice::<DebuggerValueSnapshot>(&serde_json::to_vec(&snapshot).unwrap())
            .unwrap(),
        snapshot
    );
    let request = DebuggerRequest::GetValue { target: root };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_debugger_request(&mut writer, &request).unwrap();
    assert_eq!(read_debugger_request(&mut reader).unwrap(), request);
    let reply = DebuggerReply::Value(Box::new(snapshot));
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_debugger_reply(&mut writer, &reply).unwrap();
    assert_eq!(read_debugger_reply(&mut reader).unwrap(), reply);
    assert!(!DebuggerValueTarget {
        frame_index: 1,
        ..root
    }
    .is_well_formed());
    assert!(!DebuggerValueTarget {
        safe_point: DebuggerSafePoint {
            code_unit_ordinal: 2,
            ..root.safe_point
        },
        ..root
    }
    .is_well_formed());
    assert!(!DebuggerValueTarget {
        safe_point: DebuggerSafePoint {
            program: DebuggerProgram {
                program_generation: 6,
                ..program
            },
            ..root.safe_point
        },
        ..root
    }
    .is_well_formed());
    let nested = DebuggerValueTarget {
        frame: Some(frame),
        safe_point: DebuggerSafePoint {
            code_unit_ordinal: 2,
            ..root.safe_point
        },
        ..root
    };
    assert!(nested.is_well_formed());
    assert!(!DebuggerValueTarget {
        frame: Some(DebuggerFrame {
            core_instance: [0; 16],
            ..frame
        }),
        ..nested
    }
    .is_well_formed());
    assert!(!DebuggerValueTarget {
        frame_index: DEBUGGER_MAX_STACK_FRAMES,
        ..nested
    }
    .is_well_formed());
    assert!(DebuggerValueTarget {
        frame_index: 1,
        safe_point: root.safe_point,
        ..nested
    }
    .is_well_formed());
}

#[test]
fn public_value_preview_rejects_every_excess_budget_and_duplicate_keys() {
    let preview = DebuggerValuePreview::Record(vec![(
        vec![0xd800],
        DebuggerValuePreview::Array(vec![
            Some(DebuggerValuePreview::NumberBits((-0.0_f64).to_bits())),
            None,
            Some(DebuggerValuePreview::StringUnits(vec![0xdc00])),
        ]),
    )]);
    assert!(preview.is_well_formed());
    assert_eq!(
        serde_json::from_slice::<DebuggerValuePreview>(&serde_json::to_vec(&preview).unwrap())
            .unwrap(),
        preview
    );
    let mut too_deep = DebuggerValuePreview::Null;
    for _ in 0..5 {
        too_deep = DebuggerValuePreview::Array(vec![Some(too_deep)]);
    }
    assert!(!too_deep.is_well_formed());
    assert!(!DebuggerValuePreview::Array(vec![None; 33]).is_well_formed());
    assert!(!DebuggerValuePreview::Array(vec![
        Some(DebuggerValuePreview::Array(vec![None; 32]));
        8
    ])
    .is_well_formed());
    assert!(!DebuggerValuePreview::BigIntBytes(vec![0; 4_097]).is_well_formed());
    assert!(DebuggerValuePreview::StringUnits(vec![0; 2_048]).is_well_formed());
    assert!(!DebuggerValuePreview::Record(vec![
        (
            vec![b'a' as u16],
            DebuggerValuePreview::StringUnits(vec![0; 1_024])
        ),
        (
            vec![b'b' as u16],
            DebuggerValuePreview::StringUnits(vec![0; 1_024])
        ),
    ])
    .is_well_formed());
    assert!(!DebuggerValuePreview::Record(vec![(
        vec![b'x' as u16; 2_049],
        DebuggerValuePreview::Null,
    )])
    .is_well_formed());
    assert!(!DebuggerValuePreview::Record(vec![
        (vec![b'a' as u16], DebuggerValuePreview::Null),
        (vec![b'a' as u16], DebuggerValuePreview::Bool(true)),
    ])
    .is_well_formed());
}

#[test]
fn scope_receipts_are_exact_stream_local_and_pause_bound() {
    let hello = hello(DebuggerMetadataCapabilityManifest::empty());
    let ack = negotiate(&hello, &DebuggerMetadataCapabilityManifest::empty());
    let session = metadata_session_authorization(&hello, &ack).unwrap();
    let other = metadata_session_authorization(&hello, &ack).unwrap();
    let program = DebuggerProgram {
        realm: realm(),
        program_handle: 12,
        program_generation: 5,
    };
    let snapshot = DebuggerScopeSnapshot {
        program,
        frame: None,
        frame_index: 0,
        safe_point: DebuggerSafePoint {
            program,
            code_unit_ordinal: 0,
            bytecode_offset: 8,
        },
        entries: vec![DebuggerScopeEntry {
            slot_ordinal: 512,
            scope_depth: 1,
        }],
        scope_truncated: false,
    };
    let target = snapshot
        .receipt_targets()
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert!(!session.observed_scope(target, 1));
    assert!(session.observe_scopes(&snapshot, 1));
    assert!(session.observed_scope(target, 1));
    assert!(session.clone().observed_scope(target, 1));
    assert!(!other.observed_scope(target, 1));
    assert!(!session.observed_scope(
        DebuggerValueTarget {
            scope_entry: DebuggerScopeEntry {
                scope_depth: 2,
                ..target.scope_entry
            },
            ..target
        },
        1
    ));
    assert!(!session.observed_scope(target, 2));
    assert!(session.observe_scopes(&snapshot, 2));
    assert!(session.observed_scope(target, 2));
    assert!(!session.observe_scopes(&snapshot, 1));
    assert!(!session.observed_scope(target, 1));
    assert!(!session.observe_scopes(
        &DebuggerScopeSnapshot {
            entries: vec![target.scope_entry, target.scope_entry],
            ..snapshot.clone()
        },
        2
    ));
    assert!(!session.observe_scopes(&snapshot, 0));
    assert!(!session.observed_scope(target, 0));
}

#[test]
fn linked_static_scope_receipts_cannot_be_used_as_value_receipts() {
    let hello = hello(DebuggerMetadataCapabilityManifest::empty());
    let ack = negotiate(&hello, &DebuggerMetadataCapabilityManifest::empty());
    let session = metadata_session_authorization(&hello, &ack).unwrap();
    let other = metadata_session_authorization(&hello, &ack).unwrap();
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
    let stack = DebuggerLinkedStackSnapshot {
        frames: [(dependency, 1, 4, 31), (entry, 0, 8, 32)].map(
            |(program, ordinal, offset, handle)| DebuggerLinkedStackFrame {
                frame: DebuggerLinkedFrame {
                    program,
                    code_unit_ordinal: ordinal,
                    core_instance: [7; 16],
                    frame_handle: handle,
                },
                safe_point: DebuggerSafePoint {
                    program,
                    code_unit_ordinal: ordinal,
                    bytecode_offset: offset,
                },
            },
        ),
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
    let metadata = DebuggerStaticMetadataHandle {
        program: entry,
        metadata_handle: 41,
        metadata_generation: 42,
    };
    let linked_target = DebuggerLinkedScopeTarget {
        stack,
        frame_index: 1,
        scope_entry: slot,
    };
    let linked = DebuggerStaticScopeTarget::Linked {
        metadata,
        target: linked_target,
    };
    let ordinary_value = DebuggerValueTarget {
        program: entry,
        frame: None,
        frame_index: 0,
        safe_point: stack.frames[1].safe_point,
        scope_entry: slot,
    };
    let ordinary = DebuggerStaticScopeTarget::Ordinary {
        metadata,
        target: ordinary_value,
    };
    assert!(!session.observed_static_scope(linked, 1));
    assert!(session.observe_linked_scopes(&linked_scopes, 1));
    assert!(session.observed_static_scope(linked, 1));
    assert!(!session.observed_static_scope(ordinary, 1));
    assert!(!session.observed_scope(ordinary_value, 1));
    assert!(!other.observed_static_scope(linked, 1));
    let ordinary_scopes = DebuggerScopeSnapshot {
        program: entry,
        frame: None,
        frame_index: 0,
        safe_point: stack.frames[1].safe_point,
        entries: vec![slot],
        scope_truncated: false,
    };
    assert!(session.observe_scopes(&ordinary_scopes, 1));
    assert!(session.observed_static_scope(ordinary, 1));
    assert!(session.observed_scope(ordinary_value, 1));
    assert!(!session.observed_static_scope(
        DebuggerStaticScopeTarget::Linked {
            metadata,
            target: DebuggerLinkedScopeTarget {
                scope_entry: DebuggerScopeEntry {
                    slot_ordinal: 9,
                    ..slot
                },
                ..linked_target
            },
        },
        1
    ));
    assert!(!session.observed_static_scope(linked, 2));
    let truncated = DebuggerLinkedScopeSnapshot {
        scope_truncated: true,
        max_scope_entries: 1,
        ..linked_scopes.clone()
    };
    assert!(truncated.is_well_formed());
    assert!(!session.observe_linked_scopes(&truncated, 2));
    assert!(!session.observed_static_scope(linked, 1));
    assert!(!session.observed_static_scope(ordinary, 2));
    assert!(session.observe_linked_scopes(&linked_scopes, 2));
    assert!(session.observed_static_scope(linked, 2));
    assert!(!session.observe_linked_scopes(&linked_scopes, 1));
    for batch in 0..(DEBUGGER_SESSION_MAX_OBSERVED_SCOPE_ENTRIES / 256) {
        let snapshot = DebuggerScopeSnapshot {
            entries: (0..256)
                .map(|index| DebuggerScopeEntry {
                    slot_ordinal: (10_000 + batch * 256 + index) as u32,
                    scope_depth: 0,
                })
                .collect(),
            ..ordinary_scopes.clone()
        };
        if batch + 1 == DEBUGGER_SESSION_MAX_OBSERVED_SCOPE_ENTRIES / 256 {
            assert!(!session.observe_scopes(&snapshot, 2));
            assert!(!session.observed_scope(
                DebuggerValueTarget {
                    scope_entry: snapshot.entries[0],
                    ..ordinary_value
                },
                2
            ));
        } else {
            assert!(session.observe_scopes(&snapshot, 2));
        }
    }
    assert!(session.observed_static_scope(linked, 2));
}

#[test]
fn scope_receipt_budget_rejects_a_whole_new_snapshot() {
    let hello = hello(DebuggerMetadataCapabilityManifest::empty());
    let ack = negotiate(&hello, &DebuggerMetadataCapabilityManifest::empty());
    let session = metadata_session_authorization(&hello, &ack).unwrap();
    let program = DebuggerProgram {
        realm: realm(),
        program_handle: 12,
        program_generation: 5,
    };
    let mut snapshot = DebuggerScopeSnapshot {
        program,
        frame: None,
        frame_index: 0,
        safe_point: DebuggerSafePoint {
            program,
            code_unit_ordinal: 0,
            bytecode_offset: 8,
        },
        entries: Vec::new(),
        scope_truncated: true,
    };
    for batch in 0..(DEBUGGER_SESSION_MAX_OBSERVED_SCOPE_ENTRIES / 256) {
        snapshot.entries = (0..256)
            .map(|index| DebuggerScopeEntry {
                slot_ordinal: (batch * 256 + index) as u32,
                scope_depth: 0,
            })
            .collect();
        assert!(session.observe_scopes(&snapshot, 1));
    }
    snapshot.entries = vec![DebuggerScopeEntry {
        slot_ordinal: DEBUGGER_SESSION_MAX_OBSERVED_SCOPE_ENTRIES as u32,
        scope_depth: 0,
    }];
    let excess = snapshot
        .receipt_targets()
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert!(!session.observe_scopes(&snapshot, 1));
    assert!(!session.observed_scope(excess, 1));
    assert!(session.observe_scopes(&snapshot, 2));
    assert!(session.observed_scope(excess, 2));
}

#[test]
fn malformed_debugger_frames_fail_without_a_panic() {
    let mut bytes = Vec::new();
    let malformed = b"not json";
    bytes.extend_from_slice(&(malformed.len() as u32).to_le_bytes());
    bytes.extend_from_slice(malformed);
    assert!(read_debugger_request(&mut std::io::Cursor::new(bytes)).is_err());
}
