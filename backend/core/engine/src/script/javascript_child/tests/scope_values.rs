// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn core_rejects_malformed_child_stack_shapes_and_scope_budgets() {
    use blueice_ipc::page_host::{PageHostDebuggerScopeEntry, PageHostDebuggerStackFrame};

    let child = PageHostDebuggerFrame {
        tab_id: 7,
        document_generation: 1,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        code_unit_ordinal: 1,
        invocation_serial: 17,
    };
    let entry = PageHostDebuggerScopeEntry {
        slot_ordinal: 2,
        scope_depth: 0,
    };
    let top = PageHostDebuggerStackFrame {
        code_unit_ordinal: 1,
        bytecode_offset: 4,
        scope_entries: vec![entry],
        scope_truncated: true,
    };
    let snapshot = PageHostDebuggerStackSnapshot {
        frames: vec![top.clone()],
        stack_truncated: true,
    };
    assert!(valid_child_debugger_stack_snapshot(
        &snapshot,
        Some(child),
        1,
        4,
        1,
        1
    ));
    let mut malformed = snapshot.clone();
    malformed.frames[0].bytecode_offset = 5;
    assert!(!valid_child_debugger_stack_snapshot(
        &malformed,
        Some(child),
        1,
        4,
        1,
        1
    ));
    malformed = snapshot.clone();
    malformed.stack_truncated = false;
    assert!(!valid_child_debugger_stack_snapshot(
        &malformed,
        Some(child),
        1,
        4,
        1,
        1
    ));
    malformed = snapshot.clone();
    malformed.frames[0].scope_entries.push(entry);
    assert!(!valid_child_debugger_stack_snapshot(
        &malformed,
        Some(child),
        1,
        4,
        1,
        1
    ));
    malformed = snapshot.clone();
    malformed.frames[0].scope_entries.clear();
    assert!(!valid_child_debugger_stack_snapshot(
        &malformed,
        Some(child),
        1,
        4,
        1,
        1
    ));
    malformed = snapshot.clone();
    malformed.frames[0]
        .scope_entries
        .push(PageHostDebuggerScopeEntry {
            slot_ordinal: 3,
            scope_depth: 0,
        });
    malformed.frames[0].scope_entries[0].scope_depth = 1;
    assert!(!valid_child_debugger_stack_snapshot(
        &malformed,
        Some(child),
        1,
        4,
        1,
        2
    ));
    let root = PageHostDebuggerStackFrame {
        code_unit_ordinal: 0,
        bytecode_offset: 8,
        scope_entries: Vec::new(),
        scope_truncated: false,
    };
    let full = PageHostDebuggerStackSnapshot {
        frames: vec![top, root],
        stack_truncated: false,
    };
    assert!(valid_child_debugger_stack_snapshot(
        &full,
        Some(child),
        1,
        4,
        2,
        1
    ));
    assert!(!valid_child_debugger_stack_snapshot(
        &full, None, 0, 8, 2, 1
    ));
    malformed = full.clone();
    malformed.frames[1].code_unit_ordinal = 1;
    assert!(!valid_child_debugger_stack_snapshot(
        &malformed,
        Some(child),
        1,
        4,
        2,
        1
    ));
}

struct SnapshotReplyChild {
    reply: PageHostReply,
    value_reply: Option<PageHostReply>,
}

impl PageHostClient for SnapshotReplyChild {
    fn synchronize_document(&mut self, _: PageHostDocument) -> io::Result<PageHostReply> {
        unreachable!("the test installs a core-owned identity directly")
    }

    fn close_realm(&mut self, _: u64, _: u64) -> io::Result<PageHostReply> {
        unreachable!("the test does not close the test realm")
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn debugger_stack_snapshot_available(&self) -> bool {
        true
    }

    fn debugger_value_snapshot_available(&self) -> bool {
        self.value_reply.is_some()
    }

    fn debugger_value_snapshot(
        &mut self,
        _: PageHostDebuggerValueTarget,
    ) -> io::Result<PageHostReply> {
        Ok(self.value_reply.clone().unwrap())
    }

    fn debugger_execution_state(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerExecutionState {
            tab_id,
            document_generation,
            program,
            state: PageHostDebuggerExecutionState::Paused {
                safe_point: PageHostDebuggerSafePoint {
                    program,
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                },
            },
        })
    }

    fn validate_debugger_safe_point(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerSafePointValidated {
            tab_id,
            document_generation,
            safe_point,
        })
    }

    fn debugger_stack_snapshot(
        &mut self,
        _: u64,
        _: u64,
        _: PageHostDebuggerProgram,
        _: Option<PageHostDebuggerFrame>,
        _: u32,
        _: u32,
    ) -> io::Result<PageHostReply> {
        Ok(self.reply.clone())
    }
}

#[test]
fn core_rejects_private_snapshot_reply_identity_mismatches() {
    use blueice_ipc::page_host::PageHostDebuggerStackFrame;

    let tab_id = TabId::from_u64(7);
    let child_program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let reply = PageHostReply::DebuggerStackSnapshot {
        tab_id: 7,
        document_generation: 1,
        program: child_program,
        frame: None,
        snapshot: PageHostDebuggerStackSnapshot {
            frames: vec![PageHostDebuggerStackFrame {
                code_unit_ordinal: 0,
                bytecode_offset: 4,
                scope_entries: Vec::new(),
                scope_truncated: false,
            }],
            stack_truncated: false,
        },
    };
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(SnapshotReplyChild {
        reply: reply.clone(),
        value_reply: None,
    });
    executor.enable_debugger_execution_control();
    executor.live_documents.insert(
        tab_id,
        LiveDocument {
            document_generation: 1,
            origin: "https://example.test".into(),
        },
    );
    executor.debugger_programs.insert(
        tab_id,
        BTreeMap::from([(
            child_program,
            CoreDebuggerProgram {
                program_handle: 101,
                program_generation: 103,
            },
        )]),
    );
    let request = |executor: &mut OutOfProcessJavaScriptPageExecutor<SnapshotReplyChild>| {
        executor.debugger_stack_snapshot(
            tab_id,
            1,
            JavaScriptPageDebuggerProgram {
                program_handle: 101,
                program_generation: 103,
            },
            None,
            2,
            2,
        )
    };
    assert_eq!(request(&mut executor).unwrap().frames.len(), 1);
    let PageHostReply::DebuggerStackSnapshot { snapshot, .. } = &reply else {
        unreachable!();
    };
    let snapshot = snapshot.clone();
    for malformed in [
        PageHostReply::DebuggerStackSnapshot {
            tab_id: 8,
            document_generation: 1,
            program: child_program,
            frame: None,
            snapshot: snapshot.clone(),
        },
        PageHostReply::DebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 2,
            program: child_program,
            frame: None,
            snapshot: snapshot.clone(),
        },
        PageHostReply::DebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 1,
            program: PageHostDebuggerProgram {
                program_generation: 14,
                ..child_program
            },
            frame: None,
            snapshot: snapshot.clone(),
        },
        PageHostReply::DebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 1,
            program: child_program,
            frame: Some(PageHostDebuggerFrame {
                tab_id: 7,
                document_generation: 1,
                program: child_program,
                code_unit_ordinal: 1,
                invocation_serial: 17,
            }),
            snapshot,
        },
    ] {
        executor.child.reply = malformed;
        assert_eq!(
            request(&mut executor),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
    }
}

#[test]
fn core_remints_only_exact_bounded_paused_child_values() {
    let tab_id = TabId::from_u64(7);
    let child_program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let core_program = JavaScriptPageDebuggerProgram {
        program_handle: 101,
        program_generation: 103,
    };
    let child_target = PageHostDebuggerValueTarget {
        tab_id: 7,
        document_generation: 1,
        program: child_program,
        frame: None,
        frame_index: 0,
        safe_point: PageHostDebuggerSafePoint {
            program: child_program,
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        },
        scope_entry: PageHostDebuggerScopeEntry {
            slot_ordinal: 0,
            scope_depth: 0,
        },
    };
    let preview = PageHostDebuggerValuePreview::Record(vec![(
        vec![0xd800],
        PageHostDebuggerValuePreview::Array(vec![
            None,
            Some(PageHostDebuggerValuePreview::NumberBits(
                (-0.0_f64).to_bits(),
            )),
        ]),
    )]);
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(SnapshotReplyChild {
        reply: PageHostReply::DebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 1,
            program: child_program,
            frame: None,
            snapshot: PageHostDebuggerStackSnapshot {
                frames: vec![page_host::PageHostDebuggerStackFrame {
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                    scope_entries: vec![child_target.scope_entry],
                    scope_truncated: false,
                }],
                stack_truncated: false,
            },
        },
        value_reply: Some(PageHostReply::DebuggerValueSnapshot(Box::new(
            page_host::PageHostDebuggerValueSnapshot {
                target: child_target,
                preview: preview.clone(),
            },
        ))),
    });
    executor.enable_debugger_execution_control();
    executor.live_documents.insert(
        tab_id,
        LiveDocument {
            document_generation: 1,
            origin: "https://example.test".into(),
        },
    );
    executor.debugger_programs.insert(
        tab_id,
        BTreeMap::from([(
            child_program,
            CoreDebuggerProgram {
                program_handle: core_program.program_handle,
                program_generation: core_program.program_generation,
            },
        )]),
    );
    let target = JavaScriptPageDebuggerValueTarget {
        program: core_program,
        frame: None,
        frame_index: 0,
        safe_point: JavaScriptPageDebuggerSafePoint {
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        },
        scope_entry: JavaScriptPageDebuggerScopeEntry {
            slot_ordinal: 0,
            scope_depth: 0,
        },
    };
    assert_eq!(
        executor.debugger_value_snapshot(tab_id, 1, target),
        Ok(JavaScriptPageDebuggerValuePreview::Record(vec![(
            vec![0xd800],
            JavaScriptPageDebuggerValuePreview::Array(vec![
                None,
                Some(JavaScriptPageDebuggerValuePreview::NumberBits(
                    (-0.0_f64).to_bits()
                )),
            ]),
        )]))
    );
    for invalid in [
        JavaScriptPageDebuggerValueTarget {
            safe_point: JavaScriptPageDebuggerSafePoint {
                bytecode_offset: 5,
                ..target.safe_point
            },
            ..target
        },
        JavaScriptPageDebuggerValueTarget {
            scope_entry: JavaScriptPageDebuggerScopeEntry {
                slot_ordinal: 1,
                ..target.scope_entry
            },
            ..target
        },
        JavaScriptPageDebuggerValueTarget {
            frame_index: 1,
            ..target
        },
    ] {
        assert_eq!(
            executor.debugger_value_snapshot(tab_id, 1, invalid),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
    }
    for malformed in [
        page_host::PageHostDebuggerValueSnapshot {
            target: PageHostDebuggerValueTarget {
                document_generation: 2,
                ..child_target
            },
            preview: preview.clone(),
        },
        page_host::PageHostDebuggerValueSnapshot {
            target: child_target,
            preview: PageHostDebuggerValuePreview::StringUnits(vec![0; 2_049]),
        },
        page_host::PageHostDebuggerValueSnapshot {
            target: child_target,
            preview: PageHostDebuggerValuePreview::Record(vec![
                (vec![1], PageHostDebuggerValuePreview::Null),
                (vec![1], PageHostDebuggerValuePreview::Null),
            ]),
        },
    ] {
        executor.child.value_reply =
            Some(PageHostReply::DebuggerValueSnapshot(Box::new(malformed)));
        assert_eq!(
            executor.debugger_value_snapshot(tab_id, 1, target),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
    }
}

struct StaticScopeCoreChild {
    state: PageHostDebuggerExecutionState,
    stack: PageHostDebuggerStackSnapshot,
    relation_reply: Option<PageHostReply>,
    seen: Option<PageHostDebuggerStaticScopeTarget>,
}

impl PageHostClient for StaticScopeCoreChild {
    fn synchronize_document(&mut self, _: PageHostDocument) -> io::Result<PageHostReply> {
        unreachable!("the test installs core identities directly")
    }

    fn close_realm(&mut self, _: u64, _: u64) -> io::Result<PageHostReply> {
        unreachable!("the test does not close the realm")
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn debugger_nested_frames_available(&self) -> bool {
        true
    }

    fn debugger_stack_snapshot_available(&self) -> bool {
        true
    }

    fn debugger_execution_state(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerExecutionState {
            tab_id,
            document_generation,
            program,
            state: self.state,
        })
    }

    fn validate_debugger_safe_point(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerSafePointValidated {
            tab_id,
            document_generation,
            safe_point,
        })
    }

    fn debugger_stack_snapshot(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        frame: Option<PageHostDebuggerFrame>,
        _: u32,
        _: u32,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerStackSnapshot {
            tab_id,
            document_generation,
            program,
            frame,
            snapshot: self.stack.clone(),
        })
    }

    fn debugger_static_scope_relation(
        &mut self,
        target: PageHostDebuggerStaticScopeTarget,
    ) -> io::Result<PageHostReply> {
        self.seen = Some(target.clone());
        Ok(self.relation_reply.clone().unwrap_or_else(|| {
            PageHostReply::DebuggerStaticScopeRelation(Box::new(
                page_host::PageHostDebuggerStaticScopeRelation {
                    target,
                    symbol_type: PageHostDebuggerBlueTsMetadataSymbolType {
                        symbol_id: 2,
                        type_id: 1,
                    },
                },
            ))
        }))
    }
}

fn ordinary_static_scope_executor(
    frame: Option<PageHostDebuggerFrame>,
) -> (
    OutOfProcessJavaScriptPageExecutor<StaticScopeCoreChild>,
    JavaScriptPageDebuggerProgram,
    JavaScriptPageDebuggerStaticMetadata,
) {
    use blueice_ipc::page_host::PageHostDebuggerStackFrame;

    let tab_id = TabId::from_u64(7);
    let child_program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let core_program = JavaScriptPageDebuggerProgram {
        program_handle: 101,
        program_generation: 103,
    };
    let child_metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 17,
        metadata_generation: 19,
    };
    let core_metadata = JavaScriptPageDebuggerStaticMetadata {
        metadata_handle: 301,
        metadata_generation: 303,
    };
    let scope_entry = PageHostDebuggerScopeEntry {
        slot_ordinal: 0,
        scope_depth: 0,
    };
    let root = PageHostDebuggerStackFrame {
        code_unit_ordinal: 0,
        bytecode_offset: 4,
        scope_entries: vec![scope_entry],
        scope_truncated: false,
    };
    let stack = PageHostDebuggerStackSnapshot {
        frames: match frame {
            None => vec![root],
            Some(_) => vec![
                PageHostDebuggerStackFrame {
                    code_unit_ordinal: 1,
                    bytecode_offset: 2,
                    scope_entries: vec![PageHostDebuggerScopeEntry {
                        slot_ordinal: 1,
                        scope_depth: 0,
                    }],
                    scope_truncated: false,
                },
                root,
            ],
        },
        stack_truncated: false,
    };
    let state = match frame {
        None => PageHostDebuggerExecutionState::Paused {
            safe_point: PageHostDebuggerSafePoint {
                program: child_program,
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
        },
        Some(frame) => PageHostDebuggerExecutionState::NestedPaused {
            frame,
            safe_point: PageHostDebuggerSafePoint {
                program: child_program,
                code_unit_ordinal: 1,
                bytecode_offset: 2,
            },
        },
    };
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(StaticScopeCoreChild {
        state,
        stack,
        relation_reply: None,
        seen: None,
    });
    executor.enable_debugger_execution_control();
    executor.live_documents.insert(
        tab_id,
        LiveDocument {
            document_generation: 1,
            origin: "https://example.test".into(),
        },
    );
    executor.debugger_programs.insert(
        tab_id,
        BTreeMap::from([(
            child_program,
            CoreDebuggerProgram {
                program_handle: core_program.program_handle,
                program_generation: core_program.program_generation,
            },
        )]),
    );
    executor.debugger_static_metadata.insert(
        tab_id,
        BTreeMap::from([(
            child_metadata,
            CoreDebuggerStaticMetadata {
                program: child_program,
                metadata_handle: core_metadata.metadata_handle,
                metadata_generation: core_metadata.metadata_generation,
            },
        )]),
    );
    (executor, core_program, core_metadata)
}

#[test]
fn core_ordinary_static_scope_relation_requires_live_root_slot_and_owner() {
    let tab_id = TabId::from_u64(7);
    let (mut executor, program, metadata) = ordinary_static_scope_executor(None);
    let target = JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
        metadata,
        target: JavaScriptPageDebuggerValueTarget {
            program,
            frame: None,
            frame_index: 0,
            safe_point: JavaScriptPageDebuggerSafePoint {
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
            scope_entry: JavaScriptPageDebuggerScopeEntry {
                slot_ordinal: 0,
                scope_depth: 0,
            },
        },
    };
    let relation = executor
        .debugger_static_scope_relation(tab_id, 1, target)
        .unwrap();
    assert_eq!(relation.target, target);
    assert_eq!(relation.symbol_type.symbol_id, 2);
    assert_eq!(relation.symbol_type.type_id, 1);
    let PageHostDebuggerStaticScopeTarget::Ordinary {
        metadata: child_metadata,
        target: child_target,
    } = executor.child.seen.clone().unwrap()
    else {
        panic!("ordinary target must remain ordinary across the boundary");
    };
    assert_eq!(child_target.program.program_handle, 11);
    assert_eq!(child_target.program.program_generation, 13);
    assert_eq!(child_metadata.metadata_handle, 17);
    assert_eq!(child_metadata.metadata_generation, 19);
    assert_eq!(child_target.scope_entry.slot_ordinal, 0);

    let JavaScriptPageDebuggerStaticScopeTarget::Ordinary { target: slot, .. } = target else {
        unreachable!();
    };
    for invalid in [
        JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: JavaScriptPageDebuggerValueTarget {
                safe_point: JavaScriptPageDebuggerSafePoint {
                    bytecode_offset: 5,
                    ..slot.safe_point
                },
                ..slot
            },
        },
        JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: JavaScriptPageDebuggerValueTarget {
                scope_entry: JavaScriptPageDebuggerScopeEntry {
                    slot_ordinal: 9,
                    ..slot.scope_entry
                },
                ..slot
            },
        },
        JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: JavaScriptPageDebuggerValueTarget {
                scope_entry: JavaScriptPageDebuggerScopeEntry {
                    scope_depth: slot.scope_entry.scope_depth + 1,
                    ..slot.scope_entry
                },
                ..slot
            },
        },
        JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
            metadata: JavaScriptPageDebuggerStaticMetadata {
                metadata_generation: metadata.metadata_generation + 1,
                ..metadata
            },
            target: slot,
        },
        JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: JavaScriptPageDebuggerValueTarget {
                program: JavaScriptPageDebuggerProgram {
                    program_generation: program.program_generation + 1,
                    ..program
                },
                ..slot
            },
        },
    ] {
        assert!(executor
            .debugger_static_scope_relation(tab_id, 1, invalid)
            .is_err());
    }
    assert!(executor
        .debugger_static_scope_relation(tab_id, 2, target)
        .is_err());
    executor
        .debugger_static_metadata
        .get_mut(&tab_id)
        .unwrap()
        .insert(
            PageHostDebuggerMetadataHandle {
                metadata_handle: 27,
                metadata_generation: 29,
            },
            CoreDebuggerStaticMetadata {
                program: PageHostDebuggerProgram {
                    program_handle: 21,
                    program_generation: 23,
                },
                metadata_handle: 401,
                metadata_generation: 403,
            },
        );
    assert!(executor
        .debugger_static_scope_relation(
            tab_id,
            1,
            JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
                metadata: JavaScriptPageDebuggerStaticMetadata {
                    metadata_handle: 401,
                    metadata_generation: 403,
                },
                target: slot,
            },
        )
        .is_err());
    executor.child.stack.frames[0].scope_truncated = true;
    assert!(executor
        .debugger_static_scope_relation(tab_id, 1, target)
        .is_err());
    executor.child.stack.frames[0].scope_truncated = false;
    executor.child.relation_reply = Some(PageHostReply::DebuggerStaticScopeRelation(Box::new(
        page_host::PageHostDebuggerStaticScopeRelation {
            target: PageHostDebuggerStaticScopeTarget::Ordinary {
                metadata: child_metadata,
                target: PageHostDebuggerValueTarget {
                    safe_point: PageHostDebuggerSafePoint {
                        bytecode_offset: 5,
                        ..child_target.safe_point
                    },
                    ..child_target
                },
            },
            symbol_type: PageHostDebuggerBlueTsMetadataSymbolType {
                symbol_id: 2,
                type_id: 1,
            },
        },
    )));
    assert!(executor
        .debugger_static_scope_relation(tab_id, 1, target)
        .is_err());
}

#[test]
fn core_ordinary_static_scope_parent_requires_same_nested_invocation() {
    let tab_id = TabId::from_u64(7);
    let child_frame = PageHostDebuggerFrame {
        tab_id: 7,
        document_generation: 1,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        code_unit_ordinal: 1,
        invocation_serial: 37,
    };
    let (mut executor, program, metadata) = ordinary_static_scope_executor(Some(child_frame));
    let core_frame = executor
        .remint_core_debugger_frame(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            child_frame,
        )
        .unwrap();
    let parent = JavaScriptPageDebuggerValueTarget {
        program,
        frame: Some(core_frame),
        frame_index: 1,
        safe_point: JavaScriptPageDebuggerSafePoint {
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        },
        scope_entry: JavaScriptPageDebuggerScopeEntry {
            slot_ordinal: 0,
            scope_depth: 0,
        },
    };
    let target = JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
        metadata,
        target: parent,
    };
    assert_eq!(
        executor
            .debugger_static_scope_relation(tab_id, 1, target)
            .unwrap()
            .symbol_type,
        JavaScriptPageDebuggerStaticMetadataSymbolType {
            symbol_id: 2,
            type_id: 1,
        }
    );
    let PageHostDebuggerStaticScopeTarget::Ordinary {
        target: child_target,
        ..
    } = executor.child.seen.clone().unwrap()
    else {
        unreachable!();
    };
    assert_eq!(child_target.frame, Some(child_frame));
    assert_eq!(child_target.frame_index, 1);
    for forged in [
        JavaScriptPageDebuggerValueTarget {
            frame_index: 0,
            safe_point: JavaScriptPageDebuggerSafePoint {
                code_unit_ordinal: 1,
                bytecode_offset: 2,
            },
            scope_entry: JavaScriptPageDebuggerScopeEntry {
                slot_ordinal: 1,
                scope_depth: 0,
            },
            ..parent
        },
        JavaScriptPageDebuggerValueTarget {
            frame: Some(JavaScriptPageDebuggerFrame {
                frame_handle: core_frame.frame_handle + 1,
                ..core_frame
            }),
            ..parent
        },
    ] {
        assert!(executor
            .debugger_static_scope_relation(
                tab_id,
                1,
                JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
                    metadata,
                    target: forged,
                },
            )
            .is_err());
    }
    executor.child.state = PageHostDebuggerExecutionState::NestedPaused {
        frame: child_frame,
        safe_point: PageHostDebuggerSafePoint {
            program: child_frame.program,
            code_unit_ordinal: 1,
            bytecode_offset: 3,
        },
    };
    assert!(executor
        .debugger_static_scope_relation(tab_id, 1, target)
        .is_err());
    executor.child.state = PageHostDebuggerExecutionState::NestedPaused {
        frame: PageHostDebuggerFrame {
            invocation_serial: child_frame.invocation_serial + 1,
            ..child_frame
        },
        safe_point: PageHostDebuggerSafePoint {
            program: child_frame.program,
            code_unit_ordinal: 1,
            bytecode_offset: 2,
        },
    };
    assert!(executor
        .debugger_static_scope_relation(tab_id, 1, target)
        .is_err());
}

#[test]
fn child_symbol_location_rejects_malformed_original_coordinates() {
    let location = PageHostDebuggerBlueTsMetadataSymbolLocation {
        symbol_id: 3,
        source_id: 5,
        start_byte: 8,
        end_byte: 31,
        coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
            start_line: 0,
            start_column_utf16: 8,
            end_line: 1,
            end_column_utf16: 9,
        },
    };
    assert!(valid_child_static_metadata_symbol_location(location, 3, 5));
    assert!(!valid_child_static_metadata_symbol_location(
        PageHostDebuggerBlueTsMetadataSymbolLocation {
            coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                end_line: 0,
                end_column_utf16: 8,
                ..location.coordinates
            },
            ..location
        },
        3,
        5,
    ));
    assert!(!valid_child_static_metadata_symbol_location(
        PageHostDebuggerBlueTsMetadataSymbolLocation {
            coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                end_line: 32,
                ..location.coordinates
            },
            ..location
        },
        3,
        5,
    ));
}
