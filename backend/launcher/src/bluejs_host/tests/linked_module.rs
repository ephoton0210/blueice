// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn linked_module_child_keeps_distinct_programs_and_resumes_its_entry() {
    let entry = "blueice://page/linked-entry.ts";
    let dependency = "blueice://page/linked-dependency.ts";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { inner } from './linked-dependency.ts'; export const answer: number = inner() + 1;",
                ),
                PageHostSource::new(
                    dependency,
                    "export function inner(): number { return 41; }",
                ),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./linked-dependency.ts".to_string(),
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
    let entry_handle = attachment.entry.handle;
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
    assert_ne!(entry_program, dependency_program);
    let point = host
        .runtime
        .safe_points(7, dependency_handle, 1024)
        .unwrap()
        .into_iter()
        .find(|point| point.code_unit.ordinal() == 1 && point.bytecode_offset == 0)
        .unwrap();
    let target = PageHostDebuggerSafePoint {
        program: dependency_program,
        code_unit_ordinal: point.code_unit.ordinal(),
        bytecode_offset: point.bytecode_offset,
    };
    assert!(matches!(
        host.handle_request(
            PageHostRequest::ArmDebuggerLinkedNestedSafePointBreakpoint {
                tab_id: 7,
                document_generation: 1,
                entry_program: dependency_program,
                safe_point: target,
            }
        ),
        PageHostReply::Error { .. }
    ));
    assert_eq!(
        host.handle_request(
            PageHostRequest::ArmDebuggerLinkedNestedSafePointBreakpoint {
                tab_id: 7,
                document_generation: 1,
                entry_program,
                safe_point: target,
            }
        ),
        PageHostReply::DebuggerLinkedNestedSafePointBreakpointArmed {
            tab_id: 7,
            document_generation: 1,
            entry_program,
            safe_point: target,
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let ChildDebuggerExecutionStatus::LinkedPaused { frame, safe_point } =
        host.documents[&7].debugger_execution_states[&entry_program]
    else {
        panic!("the linked child must be paused");
    };
    assert_eq!(frame.entry_program(), entry_handle);
    assert_eq!(frame.dependency_program(), dependency_handle);
    assert_eq!(safe_point.program, dependency_program);
    let stack = host
        .debugger_linked_stack_snapshot(7, 1, entry_program, frame, 256)
        .unwrap();
    assert_eq!(stack.frames[0].safe_point.program, dependency_program);
    assert_eq!(stack.frames[1].safe_point.program, entry_program);
    let wire_frame = child_debugger_linked_frame(7, 1, entry_program, dependency_program, frame);
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
        }),
        PageHostReply::DebuggerLinkedExecutionState {
            frame: wire_frame,
            state: PageHostDebuggerLinkedExecutionState::Paused { safe_point },
        }
    );
    let wire_stack = match host.handle_request(PageHostRequest::GetDebuggerLinkedStackSnapshot {
        frame: wire_frame,
        max_scope_entries: 256,
    }) {
        PageHostReply::DebuggerLinkedStackSnapshot {
            frame: returned,
            snapshot,
        } if returned == wire_frame => *snapshot,
        reply => panic!("expected private linked stack, got {reply:?}"),
    };
    assert_eq!(wire_stack.frames[0].safe_point.program, dependency_program);
    assert_eq!(wire_stack.frames[1].safe_point.program, entry_program);
    let mut source_inventory = |program| {
        let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
            reply => panic!("expected per-program metadata, got {reply:?}"),
        };
        let source_id =
            match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
            }) {
                PageHostReply::DebuggerBlueTsMetadataSources { sources, .. } => {
                    sources[0].source_id
                }
                reply => panic!("expected per-program source IDs, got {reply:?}"),
            };
        (metadata, source_id)
    };
    let child_source = source_inventory(dependency_program);
    let entry_source = source_inventory(entry_program);
    assert_ne!(child_source.0, entry_source.0);
    let spans = host
        .debugger_linked_source_spans(
            7,
            1,
            entry_program,
            frame,
            &stack,
            [child_source, entry_source],
        )
        .unwrap();
    assert_eq!(spans[0].source_id, child_source.1);
    assert_eq!(spans[1].source_id, entry_source.1);
    let wire_sources = [
        PageHostDebuggerLinkedSource {
            metadata: child_source.0,
            source_id: child_source.1,
        },
        PageHostDebuggerLinkedSource {
            metadata: entry_source.0,
            source_id: entry_source.1,
        },
    ];
    assert_eq!(
        host.handle_request(PageHostRequest::DescribeDebuggerLinkedStackSpans {
            frame: wire_frame,
            expected_stack: wire_stack.clone(),
            sources: wire_sources,
        }),
        PageHostReply::DebuggerLinkedStackSpans {
            frame: wire_frame,
            snapshot: Box::new(wire_stack.clone()),
            spans: Box::new(spans),
        }
    );
    for rejected_sources in [
        [wire_sources[1], wire_sources[0]],
        [
            PageHostDebuggerLinkedSource {
                source_id: u32::MAX,
                ..wire_sources[0]
            },
            wire_sources[1],
        ],
        [
            wire_sources[0],
            PageHostDebuggerLinkedSource {
                source_id: u32::MAX,
                ..wire_sources[1]
            },
        ],
    ] {
        assert!(matches!(
            host.handle_request(PageHostRequest::DescribeDebuggerLinkedStackSpans {
                frame: wire_frame,
                expected_stack: wire_stack.clone(),
                sources: rejected_sources,
            }),
            PageHostReply::Error { .. }
        ));
    }
    for (program, (metadata, source_id), expected_module) in [
        (dependency_program, child_source, dependency),
        (entry_program, entry_source, entry),
    ] {
        let provenance =
            match host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadataSource {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                source_id,
            }) {
                PageHostReply::DebuggerBlueTsMetadataSourceProvenance { provenance, .. } => {
                    provenance
                }
                reply => panic!("expected exact source provenance, got {reply:?}"),
            };
        assert_eq!(provenance.module, expected_module);
    }
    assert!(host
        .debugger_linked_source_spans(
            7,
            1,
            entry_program,
            frame,
            &stack,
            [entry_source, child_source],
        )
        .is_err());
    assert!(host
        .debugger_linked_source_spans(
            7,
            1,
            entry_program,
            frame,
            &stack,
            [(child_source.0, u32::MAX), entry_source],
        )
        .is_err());
    assert!(host
        .debugger_linked_source_spans(
            7,
            1,
            entry_program,
            frame,
            &stack,
            [child_source, (entry_source.0, u32::MAX)],
        )
        .is_err());
    let mut wrong_stack = stack.clone();
    wrong_stack.frames[0].safe_point = wrong_stack.frames[1].safe_point;
    assert!(host
        .debugger_linked_source_spans(
            7,
            1,
            entry_program,
            frame,
            &wrong_stack,
            [child_source, entry_source],
        )
        .is_err());
    assert!(host
        .debugger_linked_stack_snapshot(7, 1, dependency_program, frame, 256)
        .is_err());
    assert!(host
        .debugger_linked_stack_snapshot(7, 2, entry_program, frame, 256)
        .is_err());
    let mut moved_stack = wire_stack.clone();
    moved_stack.frames[0].safe_point.bytecode_offset += 1;
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerLinkedStackSpans {
            frame: wire_frame,
            expected_stack: moved_stack,
            sources: wire_sources,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
            frame: None,
            max_frames: 2,
            max_scope_entries: 256,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        }
    ));
    let wrong_serial = PageHostDebuggerLinkedFrame {
        invocation_serial: wire_frame.invocation_serial + 1,
        ..wire_frame
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerLinkedStackSnapshot {
            frame: wrong_serial,
            max_scope_entries: 256,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerLinkedNestedExecution {
            frame: wrong_serial,
        }),
        PageHostReply::Error { .. }
    ));
    let stale_document = PageHostDebuggerLinkedFrame {
        document_generation: 2,
        ..wire_frame
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerLinkedStackSnapshot {
            frame: stale_document,
            max_scope_entries: 256,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(host
        .request_debugger_linked_resume(7, 1, dependency_program, frame)
        .is_err());
    assert_eq!(
        host.handle_request(PageHostRequest::ResumeDebuggerLinkedNestedExecution {
            frame: wire_frame,
        }),
        PageHostReply::DebuggerLinkedNestedResumeRequested { frame: wire_frame }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
        }),
        PageHostReply::DebuggerLinkedExecutionState {
            frame: wire_frame,
            state: PageHostDebuggerLinkedExecutionState::Resuming,
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let ChildDebuggerExecutionStatus::Paused(root_point) =
        host.documents[&7].debugger_execution_states[&entry_program]
    else {
        panic!("the linked child must return to its entry");
    };
    assert_eq!(root_point.program, entry_program);
    assert_eq!(root_point.code_unit_ordinal, 0);
    assert!(host
        .debugger_linked_stack_snapshot(7, 1, entry_program, frame, 256)
        .is_err());
    assert!(host
        .request_debugger_linked_resume(7, 1, entry_program, frame)
        .is_err());
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerLinkedNestedExecution {
            frame: wire_frame,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program: entry_program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { .. }
    ));
    assert_eq!(
        host.documents[&7].debugger_execution_states[&entry_program],
        ChildDebuggerExecutionStatus::Completed
    );
}
