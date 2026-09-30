// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn contract_root_kind_resolves_local_definitions_without_exposing_a_plan() {
    let plan = ContractPlan {
        id: "private".to_string(),
        root: Contract::Reference("Alias".to_string()),
        definitions: BTreeMap::from([
            (
                "Alias".to_string(),
                Contract::Reference("Shape".to_string()),
            ),
            ("Shape".to_string(), Contract::Record(Vec::new())),
        ]),
        fingerprint: "private".to_string(),
    };
    assert_eq!(
        debugger_contract_root_kind(&plan),
        DebuggerStaticMetadataContractRootKind::Record
    );
    let cyclic = ContractPlan {
        definitions: BTreeMap::from([(
            "Alias".to_string(),
            Contract::Reference("Alias".to_string()),
        )]),
        ..plan
    };
    assert_eq!(
        debugger_contract_root_kind(&cyclic),
        DebuggerStaticMetadataContractRootKind::Reference
    );
}

#[test]
fn child_bluets_debug_metadata_is_bound_to_its_live_realm_generation() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![blue_ts_classic(0, "const answer: number = 42;")]),
        }),
        PageHostReply::Synchronized { reports, .. }
            if reports == vec![script_report(
                7,
                1,
                0,
                PageHostScriptLanguage::BlueTs,
                PageHostScriptKind::Classic,
                PageHostScriptOutcome::Executed,
            )]
    ));
    assert_eq!(host.debug_registry.len(), 1);
    let first_handle = host
        .documents
        .get(&7)
        .expect("the first child realm remains live")
        .debugger_programs
        .values()
        .map(|record| record.runtime_handle)
        .find(|handle| {
            host.debug_registry
                .get(host.runtime.program_registry(), *handle)
                .is_ok()
        })
        .expect("the BlueTS program retains static metadata");
    let first_static_info = host
        .debug_registry
        .get(host.runtime.program_registry(), first_handle)
        .expect("the exact live generation resolves its metadata")
        .static_info();
    assert!(first_static_info
        .sources
        .iter()
        .any(|source| source.module == "blueice://page/inline-0.ts"));
    assert!(!first_static_info.types.is_empty());

    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                2,
                vec![blue_ts_classic(0, "const answer: string = 'next';")]
            ),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert_eq!(host.debug_registry.len(), 1);
    assert!(host
        .debug_registry
        .get(host.runtime.program_registry(), first_handle)
        .is_err());

    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 7,
            document_generation: 2,
        }),
        PageHostReply::RealmClosed {
            tab_id: 7,
            document_generation: 2,
        }
    ));
    assert!(host.debug_registry.is_empty());
}

#[test]
fn child_private_root_slots_require_exact_classic_program_and_metadata() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    classic(0, "globalThis.other = true;"),
                    blue_ts_classic(1, "const typedAnswer: number = 42;"),
                ],
            ),
        }),
        PageHostReply::Synchronized { .. }
    ));
    let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected child programs, got {reply:?}"),
    };
    assert_eq!(programs.len(), 2);
    let mut typed = None;
    let mut other = None;
    for program in programs {
        let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata,
            reply => panic!("expected metadata inventory, got {reply:?}"),
        };
        if let [metadata] = metadata.as_slice() {
            typed = Some((program, *metadata));
        } else {
            assert!(metadata.is_empty());
            other = Some(program);
        }
    }
    let (program, metadata) = typed.unwrap();
    let other = other.unwrap();
    let [slot] = host
        .live_bluets_root_symbol_slots(7, 1, program, metadata)
        .unwrap()
    else {
        panic!("the typed root declaration has one verified slot");
    };
    assert_eq!(slot.code_unit.ordinal(), 0);
    assert!(host
        .live_bluets_root_symbol_slots(7, 1, other, metadata)
        .is_err());
    assert!(host
        .live_bluets_root_symbol_slots(
            7,
            1,
            program,
            PageHostDebuggerMetadataHandle {
                metadata_handle: metadata.metadata_handle + 1,
                ..metadata
            },
        )
        .is_err());
    assert!(host
        .live_bluets_root_symbol_slots(
            7,
            1,
            PageHostDebuggerProgram {
                program_generation: program.program_generation + 1,
                ..program
            },
            metadata,
        )
        .is_err());

    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![blue_ts_classic(0, "const next: number = 7;")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(host
        .live_bluets_root_symbol_slots(7, 1, program, metadata)
        .is_err());
}

#[test]
fn child_private_linked_root_slots_refuse_swapped_metadata_and_cutover() {
    let entry = "blueice://page/entry.ts";
    let dependency = "blueice://page/dependency.ts";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { value } from './dependency.ts'; export const answer: number = value + 1;",
                ),
                PageHostSource::new(dependency, "export const value: number = 41;"),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./dependency.ts".to_string(),
        canonical_target: dependency.to_string(),
    });
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![module]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected linked child programs, got {reply:?}"),
    };
    assert_eq!(programs.len(), 2);
    let mut pairs = Vec::new();
    for program in programs {
        let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata,
            reply => panic!("expected linked metadata inventory, got {reply:?}"),
        };
        let [metadata] = metadata.as_slice() else {
            panic!("each linked module needs its exact metadata handle");
        };
        pairs.push((program, *metadata));
    }
    let first_slot = host
        .live_bluets_root_symbol_slots(7, 1, pairs[0].0, pairs[0].1)
        .unwrap()[0];
    let second_slot = host
        .live_bluets_root_symbol_slots(7, 1, pairs[1].0, pairs[1].1)
        .unwrap()[0];
    assert_ne!(first_slot.program, second_slot.program);
    assert!(host
        .live_bluets_root_symbol_slots(7, 1, pairs[0].0, pairs[1].1)
        .is_err());
    assert!(host
        .live_bluets_root_symbol_slots(7, 1, pairs[1].0, pairs[0].1)
        .is_err());

    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmClosed { .. }
    ));
    for (program, metadata) in pairs {
        assert!(host
            .live_bluets_root_symbol_slots(7, 1, program, metadata)
            .is_err());
    }
    assert!(host.debug_registry.is_empty());
}

#[test]
fn private_static_scope_linked_entry_root_requires_complete_live_stack_and_owner() {
    let entry = "blueice://page/static-entry.ts";
    let dependency = "blueice://page/static-dependency.ts";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { inner } from './static-dependency.ts'; const seed: number = 7; export const answer: number = seed + inner();",
                ),
                PageHostSource::new(
                    dependency,
                    "export function inner(): number { let local: number = 41; return local; }",
                ),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./static-dependency.ts".to_string(),
        canonical_target: dependency.to_string(),
    });
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
    let entry_program = pending.program.unwrap();
    let DeferredChildExecution::BlueTsModule { attachment, .. } = &pending.execution else {
        panic!("the linked module remains pending");
    };
    let dependency_handle = attachment.modules[dependency].handle;
    let dependency_program = host.documents[&7]
        .debugger_programs
        .iter()
        .find(|(_, record)| record.runtime_handle == dependency_handle)
        .map(|(handle, record)| PageHostDebuggerProgram {
            program_handle: *handle,
            program_generation: record.program_generation,
        })
        .unwrap();
    let metadata_for = |host: &mut BlueJsChildHost, program| match host.handle_request(
        PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program,
        },
    ) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected per-program metadata: {reply:?}"),
    };
    let entry_metadata = metadata_for(&mut host, entry_program);
    let dependency_metadata = metadata_for(&mut host, dependency_program);
    let point = host
        .runtime
        .safe_points(7, dependency_handle, 1024)
        .unwrap()
        .into_iter()
        .find(|point| point.code_unit.ordinal() == 1 && point.bytecode_offset == 0)
        .unwrap();
    assert!(matches!(
        host.handle_request(
            PageHostRequest::ArmDebuggerLinkedNestedSafePointBreakpoint {
                tab_id: 7,
                document_generation: 1,
                entry_program,
                safe_point: PageHostDebuggerSafePoint {
                    program: dependency_program,
                    code_unit_ordinal: 1,
                    bytecode_offset: point.bytecode_offset,
                },
            }
        ),
        PageHostReply::DebuggerLinkedNestedSafePointBreakpointArmed { .. }
    ));
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
        program: entry_program,
    }) {
        PageHostReply::DebuggerLinkedExecutionState {
            frame,
            state: PageHostDebuggerLinkedExecutionState::Paused { .. },
        } => frame,
        reply => panic!("expected linked pause: {reply:?}"),
    };
    let stack = match host.handle_request(PageHostRequest::GetDebuggerLinkedStackSnapshot {
        frame,
        max_scope_entries: 256,
    }) {
        PageHostReply::DebuggerLinkedStackSnapshot { snapshot, .. } => *snapshot,
        reply => panic!("expected complete linked stack: {reply:?}"),
    };
    let slots = host
        .live_bluets_root_symbol_slots(7, 1, entry_program, entry_metadata)
        .unwrap();
    let (slot, scope_entry) = stack.frames[1]
        .scope_entries
        .iter()
        .find_map(|entry| {
            slots
                .iter()
                .find(|slot| slot.slot_ordinal == entry.slot_ordinal)
                .map(|slot| (*slot, *entry))
        })
        .expect("the linked entry root retains a compiler-bound slot");
    let target = PageHostDebuggerStaticScopeTarget::Linked {
        frame,
        expected_stack: Box::new(stack.clone()),
        frame_index: 1,
        metadata: entry_metadata,
        scope_entry,
    };
    assert!(target.is_well_formed());
    let request = |target| PageHostRequest::DescribeDebuggerStaticScopeRelation {
        target: Box::new(target),
    };
    assert_eq!(
        host.handle_request(request(target.clone())),
        PageHostReply::DebuggerStaticScopeRelation(Box::new(PageHostDebuggerStaticScopeRelation {
            target: target.clone(),
            symbol_type: PageHostDebuggerBlueTsMetadataSymbolType {
                symbol_id: slot.symbol_id.0,
                type_id: slot.type_id.0,
            },
        }))
    );
    for denied in [
        PageHostDebuggerStaticScopeTarget::Linked {
            frame,
            expected_stack: Box::new(stack.clone()),
            frame_index: 1,
            metadata: dependency_metadata,
            scope_entry,
        },
        PageHostDebuggerStaticScopeTarget::Linked {
            frame,
            expected_stack: Box::new(stack.clone()),
            frame_index: 1,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_generation: entry_metadata.metadata_generation + 1,
                ..entry_metadata
            },
            scope_entry,
        },
        PageHostDebuggerStaticScopeTarget::Linked {
            frame,
            expected_stack: Box::new(stack.clone()),
            frame_index: 0,
            metadata: dependency_metadata,
            scope_entry,
        },
        PageHostDebuggerStaticScopeTarget::Linked {
            frame: PageHostDebuggerLinkedFrame {
                entry_program: dependency_program,
                dependency_program: entry_program,
                ..frame
            },
            expected_stack: Box::new(stack.clone()),
            frame_index: 1,
            metadata: entry_metadata,
            scope_entry,
        },
        PageHostDebuggerStaticScopeTarget::Linked {
            frame: PageHostDebuggerLinkedFrame {
                invocation_serial: frame.invocation_serial + 1,
                ..frame
            },
            expected_stack: Box::new(stack.clone()),
            frame_index: 1,
            metadata: entry_metadata,
            scope_entry,
        },
        PageHostDebuggerStaticScopeTarget::Linked {
            frame,
            expected_stack: Box::new({
                let mut moved = stack.clone();
                moved.frames[1].safe_point.bytecode_offset += 1;
                moved
            }),
            frame_index: 1,
            metadata: entry_metadata,
            scope_entry,
        },
        PageHostDebuggerStaticScopeTarget::Linked {
            frame,
            expected_stack: Box::new(stack.clone()),
            frame_index: 1,
            metadata: entry_metadata,
            scope_entry: PageHostDebuggerScopeEntry {
                slot_ordinal: scope_entry.slot_ordinal + 256,
                ..scope_entry
            },
        },
        PageHostDebuggerStaticScopeTarget::Linked {
            frame,
            expected_stack: Box::new({
                let mut moved = stack.clone();
                moved.frames[0].safe_point.bytecode_offset += 1;
                moved
            }),
            frame_index: 1,
            metadata: entry_metadata,
            scope_entry,
        },
    ] {
        assert!(matches!(
            host.handle_request(request(denied)),
            PageHostReply::Error { .. }
        ));
    }
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(2, vec![]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(request(target)),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}
