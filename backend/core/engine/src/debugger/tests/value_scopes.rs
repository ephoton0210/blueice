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

#[test]
fn private_static_scope_helper_and_staged_receipts_recheck_both_pauses() {
    use crate::script::javascript::{
        JavaScriptPageDebuggerLinkedScopeSnapshot, JavaScriptPageDebuggerLinkedStackFrame,
        JavaScriptPageDebuggerStackFrame, JavaScriptPageDebuggerStackSnapshot,
        JavaScriptPageDebuggerStaticMetadata, JavaScriptPageDebuggerStaticMetadataSymbolType,
    };

    struct StaticLocations {
        tab_id: TabId,
        generation: u64,
        ordinary: JavaScriptPageDebuggerStackSnapshot,
        linked: JavaScriptPageDebuggerLinkedStackSnapshot,
        linked_scope: JavaScriptPageDebuggerLinkedScopeSnapshot,
        scopes_available: bool,
        relation_calls: usize,
        changed_echo: Option<JavaScriptPageDebuggerStaticScopeTarget>,
    }
    impl PageJavaScriptDebuggerLocations for StaticLocations {
        fn debugger_has_live_realm(&mut self, tab_id: TabId, generation: u64) -> bool {
            tab_id == self.tab_id && generation == self.generation
        }

        fn debugger_execution_control_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_inventory_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_type_inventory_available(&self) -> bool {
            true
        }

        fn debugger_static_metadata_symbol_inventory_available(&self) -> bool {
            true
        }

        fn max_debugger_safe_points_per_program(&self) -> usize {
            1
        }

        fn debugger_programs(
            &mut self,
            _: TabId,
            _: u64,
        ) -> Result<Vec<JavaScriptPageDebuggerProgram>, JavaScriptPageDebuggerError> {
            Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
        }

        fn debugger_safe_points(
            &mut self,
            _: TabId,
            _: u64,
            _: u64,
            _: u64,
        ) -> Result<Vec<JavaScriptPageDebuggerSafePoint>, JavaScriptPageDebuggerError> {
            Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
        }

        fn validate_debugger_safe_point(
            &mut self,
            _: TabId,
            _: u64,
            _: u64,
            _: u64,
            _: u32,
            _: u32,
        ) -> Result<(), JavaScriptPageDebuggerError> {
            Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
        }

        fn debugger_stack_snapshot(
            &mut self,
            _: TabId,
            _: u64,
            _: JavaScriptPageDebuggerProgram,
            _: Option<JavaScriptPageDebuggerFrame>,
            _: u32,
            _: u32,
        ) -> Result<JavaScriptPageDebuggerStackSnapshot, JavaScriptPageDebuggerError> {
            Ok(self.ordinary.clone())
        }

        fn debugger_linked_frames_available(&self) -> bool {
            true
        }

        fn debugger_scopes_available(&self) -> bool {
            self.scopes_available
        }

        fn debugger_linked_stack_snapshot(
            &mut self,
            _: JavaScriptPageDebuggerFrame,
            _: u32,
        ) -> Result<JavaScriptPageDebuggerLinkedStackSnapshot, JavaScriptPageDebuggerError>
        {
            Ok(self.linked)
        }

        fn debugger_linked_scope_snapshot(
            &mut self,
            _: JavaScriptPageDebuggerLinkedStackSnapshot,
        ) -> Result<JavaScriptPageDebuggerLinkedScopeSnapshot, JavaScriptPageDebuggerError>
        {
            Ok(self.linked_scope.clone())
        }

        fn debugger_static_scope_relation(
            &mut self,
            _: TabId,
            _: u64,
            target: JavaScriptPageDebuggerStaticScopeTarget,
        ) -> Result<JavaScriptPageDebuggerStaticScopeRelation, JavaScriptPageDebuggerError>
        {
            self.relation_calls += 1;
            Ok(JavaScriptPageDebuggerStaticScopeRelation {
                target: self.changed_echo.unwrap_or(target),
                symbol_type: JavaScriptPageDebuggerStaticMetadataSymbolType {
                    symbol_id: 2,
                    type_id: 1,
                },
            })
        }
    }

    let (tabs, realm) = loaded_tabs();
    let tab_id = TabId::from_u64(realm.tab_id);
    let entry = JavaScriptPageDebuggerProgram {
        program_handle: 7,
        program_generation: 3,
    };
    let slot = JavaScriptPageDebuggerScopeEntry {
        slot_ordinal: 0,
        scope_depth: 0,
    };
    let metadata = JavaScriptPageDebuggerStaticMetadata {
        metadata_handle: 11,
        metadata_generation: 12,
    };
    let ordinary = JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
        metadata,
        target: JavaScriptPageDebuggerValueTarget {
            program: entry,
            frame: None,
            frame_index: 0,
            safe_point: JavaScriptPageDebuggerSafePoint {
                code_unit_ordinal: 0,
                bytecode_offset: 41,
            },
            scope_entry: slot,
        },
    };
    let frame =
        |program: JavaScriptPageDebuggerProgram, ordinal, handle| JavaScriptPageDebuggerFrame {
            tab_id,
            document_generation: realm.realm_generation,
            program_handle: program.program_handle,
            program_generation: program.program_generation,
            code_unit_ordinal: ordinal,
            core_instance: [7; 16],
            frame_handle: handle,
        };
    let dependency = JavaScriptPageDebuggerProgram {
        program_handle: 17,
        program_generation: 4,
    };
    let linked_stack = JavaScriptPageDebuggerLinkedStackSnapshot {
        frames: [
            JavaScriptPageDebuggerLinkedStackFrame {
                frame: frame(dependency, 1, 19),
                safe_point: JavaScriptPageDebuggerSafePoint {
                    code_unit_ordinal: 1,
                    bytecode_offset: 5,
                },
            },
            JavaScriptPageDebuggerLinkedStackFrame {
                frame: frame(entry, 0, 20),
                safe_point: JavaScriptPageDebuggerSafePoint {
                    code_unit_ordinal: 0,
                    bytecode_offset: 41,
                },
            },
        ],
    };
    let linked = JavaScriptPageDebuggerStaticScopeTarget::Linked {
        metadata,
        expected_stack: linked_stack,
        frame_index: 1,
        scope_entry: slot,
    };
    let mut locations = StaticLocations {
        tab_id,
        generation: realm.realm_generation,
        ordinary: JavaScriptPageDebuggerStackSnapshot {
            frames: vec![JavaScriptPageDebuggerStackFrame {
                code_unit_ordinal: 0,
                bytecode_offset: 41,
                scope_entries: vec![slot],
                scope_truncated: false,
            }],
            stack_truncated: false,
        },
        linked: linked_stack,
        linked_scope: JavaScriptPageDebuggerLinkedScopeSnapshot {
            stack: linked_stack,
            scope_entries: vec![slot],
        },
        scopes_available: true,
        relation_calls: 0,
        changed_echo: None,
    };
    for target in [ordinary, linked] {
        assert_eq!(
            private_core_static_scope_relation(&tabs, &mut locations, realm, target)
                .unwrap()
                .target,
            target
        );
    }
    assert_eq!(locations.relation_calls, 2);
    let JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
        target: mut ordinary_slot,
        ..
    } = ordinary
    else {
        unreachable!();
    };
    ordinary_slot.scope_entry.slot_ordinal = 9;
    let forged = JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
        metadata,
        target: ordinary_slot,
    };
    assert!(private_core_static_scope_relation(&tabs, &mut locations, realm, forged).is_err());
    assert_eq!(locations.relation_calls, 2);
    locations.ordinary.frames[0].scope_entries.push(slot);
    assert!(private_core_static_scope_relation(&tabs, &mut locations, realm, ordinary).is_err());
    locations.ordinary.frames[0].scope_entries.pop();
    assert_eq!(locations.relation_calls, 2);
    let stale_realm = DebuggerPageRealm {
        realm_generation: realm.realm_generation + 1,
        ..realm
    };
    assert!(
        private_core_static_scope_relation(&tabs, &mut locations, stale_realm, ordinary).is_err()
    );
    assert_eq!(locations.relation_calls, 2);
    locations.linked.frames[1].safe_point.bytecode_offset += 1;
    assert!(private_core_static_scope_relation(&tabs, &mut locations, realm, linked).is_err());
    locations.linked = linked_stack;
    assert_eq!(locations.relation_calls, 2);
    locations.changed_echo = Some(ordinary);
    assert!(private_core_static_scope_relation(&tabs, &mut locations, realm, linked).is_err());
    assert_eq!(locations.relation_calls, 3);
    locations.changed_echo = None;

    let public_entry = DebuggerProgram {
        realm,
        program_handle: entry.program_handle,
        program_generation: entry.program_generation,
    };
    let public_metadata = DebuggerStaticMetadataHandle {
        program: public_entry,
        metadata_handle: metadata.metadata_handle,
        metadata_generation: metadata.metadata_generation,
    };
    let public_slot = DebuggerScopeEntry {
        slot_ordinal: slot.slot_ordinal,
        scope_depth: slot.scope_depth,
    };
    let public_value = DebuggerValueTarget {
        program: public_entry,
        frame: None,
        frame_index: 0,
        safe_point: DebuggerSafePoint {
            program: public_entry,
            code_unit_ordinal: 0,
            bytecode_offset: 41,
        },
        scope_entry: public_slot,
    };
    let public_linked = public_linked_stack(tab_id, realm, linked_stack).unwrap();
    let public_targets = [
        DebuggerStaticScopeTarget::Ordinary {
            metadata: public_metadata,
            target: public_value,
        },
        DebuggerStaticScopeTarget::Linked {
            metadata: public_metadata,
            target: blueice_ipc::debugger::DebuggerLinkedScopeTarget {
                stack: public_linked,
                frame_index: 1,
                scope_entry: public_slot,
            },
        },
    ];
    let manifest = blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_selected(
        blueice_ipc::debugger::DebuggerMetadataCapabilitySelection {
            type_inventory: true,
            symbol_inventory: true,
            ..Default::default()
        },
    );
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities: manifest.clone(),
    };
    let ack = blueice_ipc::debugger::negotiate(&hello, &manifest);
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &ack).unwrap();
    let other = blueice_ipc::debugger::metadata_session_authorization(&hello, &ack).unwrap();
    assert!(!session.permits_bounded_values());
    for target in public_targets {
        assert!(staged_child_static_scope_relation(
            &tabs,
            &mut locations,
            Some(&session),
            false,
            1,
            target,
        )
        .is_err());
        assert!(staged_child_static_scope_relation(
            &tabs,
            &mut locations,
            Some(&session),
            true,
            1,
            target,
        )
        .is_err());
    }
    assert_eq!(locations.relation_calls, 3);
    assert!(session.observe_scopes(
        &DebuggerScopeSnapshot {
            program: public_entry,
            frame: None,
            frame_index: 0,
            safe_point: public_value.safe_point,
            entries: vec![public_slot],
            scope_truncated: false,
        },
        1
    ));
    assert!(session.observe_linked_scopes(
        &blueice_ipc::debugger::DebuggerLinkedScopeSnapshot {
            stack: public_linked,
            frame_index: 1,
            entries: vec![public_slot],
            scope_truncated: false,
            max_scope_entries: 4,
        },
        1
    ));
    for target in public_targets {
        assert!(staged_child_static_scope_relation(
            &tabs,
            &mut locations,
            Some(&session),
            true,
            1,
            target,
        )
        .is_err());
    }
    assert_eq!(locations.relation_calls, 3);
    assert!(session.observe_metadata(&[public_metadata]));
    let symbol = DebuggerStaticMetadataSymbolId {
        metadata: public_metadata,
        symbol_id: 2,
    };
    let static_type = DebuggerStaticMetadataTypeId {
        metadata: public_metadata,
        type_id: 1,
    };
    for target in public_targets {
        assert!(staged_child_static_scope_relation(
            &tabs,
            &mut locations,
            Some(&session),
            true,
            1,
            target,
        )
        .is_err());
    }
    assert!(session.observe_symbols(&[symbol]));
    for target in public_targets {
        assert!(staged_child_static_scope_relation(
            &tabs,
            &mut locations,
            Some(&session),
            true,
            1,
            target,
        )
        .is_err());
    }
    assert!(session.observe_types(&[static_type]));
    for target in public_targets {
        assert_eq!(
            staged_child_static_scope_relation(
                &tabs,
                &mut locations,
                Some(&session),
                true,
                1,
                target,
            )
            .unwrap(),
            DebuggerStaticScopeRelation {
                target,
                symbol,
                static_type,
            }
        );
        assert!(staged_child_static_scope_relation(
            &tabs,
            &mut locations,
            Some(&other),
            true,
            1,
            target,
        )
        .is_err());
        assert!(staged_child_static_scope_relation(
            &tabs,
            &mut locations,
            Some(&session),
            true,
            2,
            target,
        )
        .is_err());
    }
    locations.changed_echo = Some(ordinary);
    assert!(staged_child_static_scope_relation(
        &tabs,
        &mut locations,
        Some(&session),
        true,
        1,
        public_targets[1],
    )
    .is_err());

    let expected = public_linked_stack(tab_id, realm, linked_stack).unwrap();
    let complete = staged_child_linked_scopes(&tabs, &mut locations, expected, 2).unwrap();
    assert_eq!(complete.stack, expected);
    assert_eq!(complete.entries, vec![public_slot]);
    assert!(!complete.scope_truncated);
    assert_eq!(complete.max_scope_entries, 2);
    assert!(staged_child_linked_scopes(&tabs, &mut locations, expected, 0).is_err());
    assert!(
        staged_child_linked_scopes(&tabs, &mut locations, expected, MAX_SCOPE_BINDINGS + 1)
            .is_err()
    );
    let mut forged = expected;
    forged.frames[0].safe_point.bytecode_offset += 1;
    assert!(staged_child_linked_scopes(&tabs, &mut locations, forged, 2).is_err());
    locations
        .linked_scope
        .scope_entries
        .push(JavaScriptPageDebuggerScopeEntry {
            slot_ordinal: 1,
            scope_depth: 1,
        });
    let bounded = staged_child_linked_scopes(&tabs, &mut locations, expected, 1).unwrap();
    assert_eq!(bounded.entries, vec![public_slot]);
    assert!(bounded.scope_truncated);
    locations.linked_scope.scope_entries.push(slot);
    assert!(staged_child_linked_scopes(&tabs, &mut locations, expected, 2).is_err());
    locations.linked_scope.scope_entries.pop();
    locations.linked_scope.stack.frames[1]
        .safe_point
        .bytecode_offset += 1;
    assert!(staged_child_linked_scopes(&tabs, &mut locations, expected, 2).is_err());
    locations.linked_scope.stack = linked_stack;
    locations.scopes_available = false;
    assert!(staged_child_linked_scopes(&tabs, &mut locations, expected, 2).is_err());

    locations.scopes_available = true;
    locations.changed_echo = None;
    locations.linked_scope.scope_entries = vec![slot];
    let route_manifest = blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_selected(
        blueice_ipc::debugger::DebuggerMetadataCapabilitySelection {
            static_scope_relation: true,
            ..Default::default()
        },
    );
    let route_hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities: route_manifest.clone(),
    };
    let route_ack = blueice_ipc::debugger::negotiate(&route_hello, &route_manifest);
    let routed =
        blueice_ipc::debugger::metadata_session_authorization(&route_hello, &route_ack).unwrap();
    let routed_other =
        blueice_ipc::debugger::metadata_session_authorization(&route_hello, &route_ack).unwrap();
    assert!(!routed.permits_bounded_values());
    let DebuggerReply::Capabilities(capabilities) =
        describe_child_location_capabilities(&tabs, &mut locations, Some(&routed), realm)
    else {
        panic!("live child must describe its separately granted capabilities");
    };
    assert!(capabilities
        .authorize_metadata(
            &routed,
            DebuggerMetadataCapability::OpaqueStaticScopeRelation
        )
        .is_some());
    let DebuggerReply::Capabilities(no_grant_capabilities) =
        describe_child_location_capabilities(&tabs, &mut locations, Some(&session), realm)
    else {
        panic!("ungranted stream should still describe planned capabilities");
    };
    assert!(no_grant_capabilities
        .authorize_metadata(
            &session,
            DebuggerMetadataCapability::OpaqueStaticScopeRelation
        )
        .is_none());
    let relation_request = |target| DebuggerRequest::GetStaticScopeRelation { target };
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&session),
            1,
            relation_request(public_targets[0]),
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
            Some(&routed),
            1,
            relation_request(public_targets[1]),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    locations
        .linked_scope
        .scope_entries
        .push(JavaScriptPageDebuggerScopeEntry {
            slot_ordinal: 1,
            scope_depth: 1,
        });
    let DebuggerReply::LinkedScopes(truncated) =
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&routed),
            1,
            DebuggerRequest::GetLinkedScopes {
                expected_stack: expected,
                max_scope_entries: 1,
            },
        )
    else {
        panic!("linked scopes should allow a visibly truncated read");
    };
    assert!(truncated.scope_truncated);
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&routed),
            1,
            relation_request(public_targets[1]),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    locations.linked_scope.scope_entries.pop();
    let DebuggerReply::LinkedScopes(complete) =
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&routed),
            1,
            DebuggerRequest::GetLinkedScopes {
                expected_stack: expected,
                max_scope_entries: 2,
            },
        )
    else {
        panic!("complete linked scopes should be available");
    };
    assert!(!complete.scope_truncated);
    let DebuggerReply::Scopes(_) = handle_debugger_request_with_child_locations_and_pause(
        &tabs,
        &mut locations,
        Some(&routed),
        1,
        DebuggerRequest::GetScopes {
            program: public_entry,
            frame: None,
            frame_index: 0,
            expected_safe_point: public_value.safe_point,
            max_scope_entries: 2,
        },
    ) else {
        panic!("ordinary scopes should be available without Value authority");
    };
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&routed),
            1,
            DebuggerRequest::GetValue {
                target: public_value,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    assert!(routed.observe_metadata(&[public_metadata]));
    assert!(routed.observe_symbols(&[symbol]));
    assert!(routed.observe_types(&[static_type]));
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&routed_other),
            1,
            relation_request(public_targets[1]),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    let malformed = DebuggerStaticScopeTarget::Linked {
        metadata: DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                program_generation: public_entry.program_generation + 1,
                ..public_entry
            },
            ..public_metadata
        },
        target: blueice_ipc::debugger::DebuggerLinkedScopeTarget {
            stack: expected,
            frame_index: 1,
            scope_entry: public_slot,
        },
    };
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&routed),
            1,
            relation_request(malformed),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    for target in public_targets {
        assert_eq!(
            handle_debugger_request_with_child_locations_and_pause(
                &tabs,
                &mut locations,
                Some(&routed),
                1,
                relation_request(target),
            ),
            DebuggerReply::StaticScopeRelation(Box::new(DebuggerStaticScopeRelation {
                target,
                symbol,
                static_type,
            }))
        );
        assert!(matches!(
            handle_debugger_request_with_child_locations_and_pause(
                &tabs,
                &mut locations,
                Some(&routed),
                2,
                relation_request(target),
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
    }
    locations.linked.frames[1].safe_point.bytecode_offset += 1;
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&routed),
            1,
            relation_request(public_targets[1]),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    locations.linked = linked_stack;
    locations.changed_echo = Some(ordinary);
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&routed),
            1,
            relation_request(public_targets[1]),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));

    let full =
        blueice_ipc::debugger::metadata_session_authorization(&route_hello, &route_ack).unwrap();
    for batch in 0..16 {
        assert!(full.observe_scopes(
            &DebuggerScopeSnapshot {
                program: public_entry,
                frame: None,
                frame_index: 0,
                safe_point: public_value.safe_point,
                entries: (0..256)
                    .map(|index| DebuggerScopeEntry {
                        slot_ordinal: batch * 256 + index,
                        scope_depth: 0,
                    })
                    .collect(),
                scope_truncated: false,
            },
            1,
        ));
    }
    assert!(matches!(
        handle_debugger_request_with_child_locations_and_pause(
            &tabs,
            &mut locations,
            Some(&full),
            1,
            DebuggerRequest::GetLinkedScopes {
                expected_stack: expected,
                max_scope_entries: 2,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            ..
        }
    ));
}
