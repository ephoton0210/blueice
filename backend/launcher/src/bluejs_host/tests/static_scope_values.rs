// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn private_static_scope_wire_rejects_missing_realm() {
    let mut host = BlueJsChildHost::default();
    let program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let target = page_host::PageHostDebuggerStaticScopeTarget::Ordinary {
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
    assert!(target.is_well_formed());
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerStaticScopeRelation {
            target: Box::new(target),
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::UnknownRealm,
            ..
        }
    ));
}

#[test]
fn private_static_scope_ordinary_root_relates_only_one_live_compiler_slot() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![blue_ts_classic(0, "let answer: number = 41;")],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = host.documents[&7]
        .pending_debugger_executions
        .front()
        .unwrap()
        .program
        .unwrap();
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected metadata attachment: {reply:?}"),
    };
    let slot = host
        .live_bluets_root_symbol_slots(7, 1, program, metadata)
        .unwrap()[0];
    let halt = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .filter(|point| point.code_unit_ordinal == 0)
            .max_by_key(|point| point.bytecode_offset)
            .unwrap(),
        reply => panic!("expected root safe points: {reply:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: halt,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let target = paused_value_targets(&mut host, program, None, 0)
        .into_iter()
        .find(|target| target.scope_entry.slot_ordinal == slot.slot_ordinal)
        .expect("the compiled root declaration occupies an active slot");
    let static_target = page_host::PageHostDebuggerStaticScopeTarget::Ordinary { metadata, target };
    let request = |target| PageHostRequest::DescribeDebuggerStaticScopeRelation {
        target: Box::new(target),
    };
    assert_eq!(
        host.handle_request(request(static_target.clone())),
        PageHostReply::DebuggerStaticScopeRelation(Box::new(
            page_host::PageHostDebuggerStaticScopeRelation {
                target: static_target.clone(),
                symbol_type: PageHostDebuggerBlueTsMetadataSymbolType {
                    symbol_id: slot.symbol_id.0,
                    type_id: slot.type_id.0,
                },
            }
        ))
    );
    for forged in [
        page_host::PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata: PageHostDebuggerMetadataHandle {
                metadata_generation: metadata.metadata_generation + 1,
                ..metadata
            },
            target,
        },
        page_host::PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: PageHostDebuggerValueTarget {
                scope_entry: PageHostDebuggerScopeEntry {
                    slot_ordinal: target.scope_entry.slot_ordinal + 1,
                    ..target.scope_entry
                },
                ..target
            },
        },
        page_host::PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: PageHostDebuggerValueTarget {
                scope_entry: PageHostDebuggerScopeEntry {
                    scope_depth: target.scope_entry.scope_depth + 1,
                    ..target.scope_entry
                },
                ..target
            },
        },
        page_host::PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: PageHostDebuggerValueTarget {
                safe_point: PageHostDebuggerSafePoint {
                    bytecode_offset: target.safe_point.bytecode_offset + 1,
                    ..target.safe_point
                },
                ..target
            },
        },
    ] {
        assert!(matches!(
            host.handle_request(request(forged)),
            PageHostReply::Error { .. }
        ));
    }
    assert!(matches!(
        host.handle_request(request(
            page_host::PageHostDebuggerStaticScopeTarget::Ordinary {
                metadata,
                target: PageHostDebuggerValueTarget {
                    document_generation: 2,
                    ..target
                },
            }
        )),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(2, vec![blue_ts_classic(0, "let next: number = 7;")],),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(request(static_target)),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn private_static_scope_ordinary_nested_pause_selects_only_parent_root() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![blue_ts_classic(
                    0,
                    "let seed: number = 7; function inner(value: number): number { let local: number = value; return local; } inner(seed);",
                )],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = host.documents[&7]
        .pending_debugger_executions
        .front()
        .unwrap()
        .program
        .unwrap();
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected metadata attachment: {reply:?}"),
    };
    let point = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected nested safe point: {reply:?}"),
    };
    host.arm_debugger_nested_target(7, 1, point).unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let frame = match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::NestedPaused { frame, .. },
            ..
        } => frame,
        reply => panic!("expected nested pause: {reply:?}"),
    };
    for _ in 0..64 {
        if !paused_value_targets(&mut host, program, Some(frame), 0).is_empty() {
            break;
        }
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerNestedInstruction { frame }),
            PageHostReply::DebuggerNestedStepRequested { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
    }
    let slots = host
        .live_bluets_root_symbol_slots(7, 1, program, metadata)
        .unwrap()
        .to_vec();
    let (slot, parent) = paused_value_targets(&mut host, program, Some(frame), 1)
        .into_iter()
        .find_map(|target| {
            slots
                .iter()
                .find(|slot| slot.slot_ordinal == target.scope_entry.slot_ordinal)
                .map(|slot| (*slot, target))
        })
        .expect("one compiled parent-root declaration remains active");
    let parent_target = PageHostDebuggerStaticScopeTarget::Ordinary {
        metadata,
        target: parent,
    };
    assert_eq!(
        host.handle_request(PageHostRequest::DescribeDebuggerStaticScopeRelation {
            target: Box::new(parent_target.clone()),
        }),
        PageHostReply::DebuggerStaticScopeRelation(Box::new(PageHostDebuggerStaticScopeRelation {
            target: parent_target,
            symbol_type: PageHostDebuggerBlueTsMetadataSymbolType {
                symbol_id: slot.symbol_id.0,
                type_id: slot.type_id.0,
            },
        }))
    );
    let child = paused_value_targets(&mut host, program, Some(frame), 0)
        .into_iter()
        .next()
        .expect("the child has an active local or capture slot");
    for denied in [
        PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: child,
        },
        PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: PageHostDebuggerValueTarget {
                frame_index: 0,
                safe_point: PageHostDebuggerSafePoint {
                    code_unit_ordinal: 1,
                    ..parent.safe_point
                },
                ..parent
            },
        },
        PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: PageHostDebuggerValueTarget {
                frame: Some(PageHostDebuggerFrame {
                    invocation_serial: frame.invocation_serial + 1,
                    ..frame
                }),
                ..parent
            },
        },
    ] {
        assert!(matches!(
            host.handle_request(PageHostRequest::DescribeDebuggerStaticScopeRelation {
                target: Box::new(denied),
            }),
            PageHostReply::Error { .. }
        ));
    }
}

#[test]
fn private_child_bluets_classic_returns_only_exact_paused_plain_values() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![blue_ts_classic(
                    0,
                    "let answer: number = 41; let data = { answer: answer };",
                )],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = host.documents[&7]
        .pending_debugger_executions
        .front()
        .unwrap()
        .program
        .unwrap();
    let halt = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .filter(|point| point.code_unit_ordinal == 0)
            .max_by_key(|point| point.bytecode_offset)
            .unwrap(),
        reply => panic!("expected BlueTS root points: {reply:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: halt,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let targets = paused_value_targets(&mut host, program, None, 0);
    let replies: Vec<_> = targets
        .iter()
        .map(|target| {
            host.handle_request(PageHostRequest::GetDebuggerValueSnapshot { target: *target })
        })
        .collect();
    assert!(replies.iter().any(|reply| matches!(
        reply,
        PageHostReply::DebuggerValueSnapshot(snapshot)
            if matches!(&snapshot.preview, PageHostDebuggerValuePreview::NumberBits(bits) if *bits == 41.0_f64.to_bits())
    )));
    assert!(replies.iter().any(|reply| matches!(
        reply,
        PageHostReply::DebuggerValueSnapshot(snapshot)
            if matches!(&snapshot.preview, PageHostDebuggerValuePreview::Record(entries)
                if entries == &vec![("answer".encode_utf16().collect(), PageHostDebuggerValuePreview::NumberBits(41.0_f64.to_bits()))])
    )));
    let target = targets[0];
    for invalid in [
        PageHostDebuggerValueTarget {
            safe_point: PageHostDebuggerSafePoint {
                bytecode_offset: target.safe_point.bytecode_offset + 1,
                ..target.safe_point
            },
            ..target
        },
        PageHostDebuggerValueTarget {
            scope_entry: PageHostDebuggerScopeEntry {
                slot_ordinal: u32::MAX,
                ..target.scope_entry
            },
            ..target
        },
    ] {
        assert!(matches!(
            host.handle_request(PageHostRequest::GetDebuggerValueSnapshot { target: invalid }),
            PageHostReply::Error { .. }
        ));
    }
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerValueSnapshot {
            target: PageHostDebuggerValueTarget {
                document_generation: 2,
                ..target
            },
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn private_child_bluets_nested_preview_requires_its_active_frame() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![blue_ts_classic(
                    0,
                    "let seed: number = 7; function inner(value: number): number { let local: number = value; return local; } inner(seed);",
                )],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = host.documents[&7]
        .pending_debugger_executions
        .front()
        .unwrap()
        .program
        .unwrap();
    let point = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected BlueTS nested points: {reply:?}"),
    };
    host.arm_debugger_nested_target(7, 1, point).unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let frame = match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::NestedPaused { frame, .. },
            ..
        } => frame,
        reply => panic!("expected nested pause: {reply:?}"),
    };
    let mut selected = None;
    for _ in 0..64 {
        for target in paused_value_targets(&mut host, program, Some(frame), 0) {
            if matches!(
                host.handle_request(PageHostRequest::GetDebuggerValueSnapshot { target }),
                PageHostReply::DebuggerValueSnapshot(snapshot)
                    if matches!(&snapshot.preview, PageHostDebuggerValuePreview::NumberBits(bits) if *bits == 7.0_f64.to_bits())
            ) {
                selected = Some(target);
                break;
            }
        }
        if selected.is_some() {
            break;
        }
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerNestedInstruction { frame }),
            PageHostReply::DebuggerNestedStepRequested { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
    }
    let target = selected.expect("one active child slot must hold the parameter value");
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerValueSnapshot {
            target: PageHostDebuggerValueTarget {
                frame: None,
                ..target
            }
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerValueSnapshot {
            target: PageHostDebuggerValueTarget {
                frame: Some(PageHostDebuggerFrame {
                    invocation_serial: frame.invocation_serial + 1,
                    ..frame
                }),
                ..target
            }
        }),
        PageHostReply::Error { .. }
    ));
}

#[test]
fn private_child_bluets_module_root_preview_reads_retained_binding() {
    let entry = "blueice://page/private-values.ts";
    let module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![PageHostSource::new(
                entry,
                "let answer: number = 41; export const result: number = answer + 1;",
            )],
        ),
    };
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![module]),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let pending = host.documents[&7]
        .pending_debugger_executions
        .front()
        .unwrap();
    let program = pending.program.unwrap();
    let DeferredChildExecution::BlueTsModule { attachment, .. } = &pending.execution else {
        panic!("the checked module must be pending");
    };
    let point = host
        .runtime
        .module_evaluate_entry_safe_point(7, attachment.entry.handle)
        .unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: 0,
                bytecode_offset: point.bytecode_offset,
            },
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let mut selected = None;
    for _ in 0..64 {
        for target in paused_value_targets(&mut host, program, None, 0) {
            if matches!(
                host.handle_request(PageHostRequest::GetDebuggerValueSnapshot { target }),
                PageHostReply::DebuggerValueSnapshot(snapshot)
                    if matches!(&snapshot.preview, PageHostDebuggerValuePreview::NumberBits(bits) if *bits == 41.0_f64.to_bits())
            ) {
                selected = Some(target);
                break;
            }
        }
        if selected.is_some() {
            break;
        }
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionStepRequested { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
    }
    let target = selected.expect("module root must expose its initialized binding");
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerValueSnapshot { target }),
        PageHostReply::DebuggerValueSnapshot(snapshot)
            if matches!(&snapshot.preview, PageHostDebuggerValuePreview::NumberBits(bits) if *bits == 41.0_f64.to_bits())
    ));
}
