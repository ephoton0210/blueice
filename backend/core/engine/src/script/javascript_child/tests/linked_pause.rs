// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn core_linked_pause_remints_both_frames_and_gates_all_spans() {
    use blueice_ipc::debugger::DebuggerSourceCoordinates;
    use blueice_ipc::page_host::{
        PageHostDebuggerBlueTsSafePointSpan, PageHostDebuggerLinkedStackFrame,
    };

    struct LinkedCoreChild {
        state: PageHostReply,
        stack: PageHostReply,
        spans: PageHostReply,
        resume: PageHostReply,
        span_calls: usize,
        resume_calls: usize,
        relation_reply: Option<PageHostReply>,
        relation_calls: usize,
        relation_target: Option<PageHostDebuggerStaticScopeTarget>,
    }
    impl PageHostClient for LinkedCoreChild {
        fn synchronize_document(&mut self, _: PageHostDocument) -> io::Result<PageHostReply> {
            Err(io::Error::new(io::ErrorKind::Unsupported, "unused"))
        }

        fn close_realm(&mut self, _: u64, _: u64) -> io::Result<PageHostReply> {
            Err(io::Error::new(io::ErrorKind::Unsupported, "unused"))
        }

        fn debugger_execution_control_available(&self) -> bool {
            true
        }

        fn debugger_linked_frames_available(&self) -> bool {
            true
        }

        fn debugger_execution_state(
            &mut self,
            _: u64,
            _: u64,
            _: PageHostDebuggerProgram,
        ) -> io::Result<PageHostReply> {
            Ok(self.state.clone())
        }

        fn debugger_linked_stack_snapshot(
            &mut self,
            _: PageHostDebuggerLinkedFrame,
            max_scope_entries: u32,
        ) -> io::Result<PageHostReply> {
            let mut reply = self.stack.clone();
            if let PageHostReply::DebuggerLinkedStackSnapshot { snapshot, .. } = &mut reply {
                snapshot.max_scope_entries = max_scope_entries;
            }
            Ok(reply)
        }

        fn debugger_linked_stack_spans(
            &mut self,
            _: PageHostDebuggerLinkedFrame,
            _: PageHostDebuggerLinkedStackSnapshot,
            _: [PageHostDebuggerLinkedSource; 2],
        ) -> io::Result<PageHostReply> {
            self.span_calls += 1;
            Ok(self.spans.clone())
        }

        fn resume_debugger_linked_nested_execution(
            &mut self,
            _: PageHostDebuggerLinkedFrame,
        ) -> io::Result<PageHostReply> {
            self.resume_calls += 1;
            Ok(self.resume.clone())
        }

        fn debugger_static_scope_relation(
            &mut self,
            target: PageHostDebuggerStaticScopeTarget,
        ) -> io::Result<PageHostReply> {
            self.relation_calls += 1;
            self.relation_target = Some(target.clone());
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

    let tab_id = TabId::from_u64(7);
    let entry = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 12,
    };
    let dependency = PageHostDebuggerProgram {
        program_handle: 21,
        program_generation: 22,
    };
    let child_frame = PageHostDebuggerLinkedFrame {
        tab_id: 7,
        document_generation: 1,
        entry_program: entry,
        dependency_program: dependency,
        code_unit_ordinal: 1,
        invocation_serial: 31,
    };
    let points = [
        PageHostDebuggerSafePoint {
            program: dependency,
            code_unit_ordinal: 1,
            bytecode_offset: 0,
        },
        PageHostDebuggerSafePoint {
            program: entry,
            code_unit_ordinal: 0,
            bytecode_offset: 8,
        },
    ];
    let stack = PageHostDebuggerLinkedStackSnapshot {
        frames: points.map(|safe_point| PageHostDebuggerLinkedStackFrame {
            safe_point,
            scope_entries: if safe_point.program == entry {
                vec![PageHostDebuggerScopeEntry {
                    slot_ordinal: 0,
                    scope_depth: 0,
                }]
            } else {
                vec![]
            },
            scope_truncated: false,
        }),
        stack_truncated: false,
        max_scope_entries: 4,
    };
    let span = |source_id| PageHostDebuggerBlueTsSafePointSpan {
        source_id,
        start_byte: 0,
        end_byte: 1,
        coordinates: DebuggerSourceCoordinates {
            start_line: 0,
            start_column_utf16: 0,
            end_line: 0,
            end_column_utf16: 1,
        },
    };
    let child = LinkedCoreChild {
        state: PageHostReply::DebuggerLinkedExecutionState {
            frame: child_frame,
            state: PageHostDebuggerLinkedExecutionState::Paused {
                safe_point: points[0],
            },
        },
        stack: PageHostReply::DebuggerLinkedStackSnapshot {
            frame: child_frame,
            snapshot: Box::new(stack.clone()),
        },
        spans: PageHostReply::DebuggerLinkedStackSpans {
            frame: child_frame,
            snapshot: Box::new(stack.clone()),
            spans: Box::new([span(13), span(16)]),
        },
        resume: PageHostReply::DebuggerLinkedNestedResumeRequested { frame: child_frame },
        span_calls: 0,
        resume_calls: 0,
        relation_reply: None,
        relation_calls: 0,
        relation_target: None,
    };
    let mut executor =
        OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(child);
    executor.live_documents.insert(
        tab_id,
        LiveDocument {
            document_generation: 1,
            origin: "https://example.test".into(),
        },
    );
    let public_dependency = CoreDebuggerProgram {
        program_handle: 101,
        program_generation: 102,
    };
    let public_entry = CoreDebuggerProgram {
        program_handle: 201,
        program_generation: 202,
    };
    executor.debugger_programs.insert(
        tab_id,
        BTreeMap::from([(dependency, public_dependency), (entry, public_entry)]),
    );
    let child_metadata = [
        PageHostDebuggerMetadataHandle {
            metadata_handle: 41,
            metadata_generation: 42,
        },
        PageHostDebuggerMetadataHandle {
            metadata_handle: 51,
            metadata_generation: 52,
        },
    ];
    executor.debugger_static_metadata.insert(
        tab_id,
        BTreeMap::from([
            (
                child_metadata[0],
                CoreDebuggerStaticMetadata {
                    program: dependency,
                    metadata_handle: 301,
                    metadata_generation: 302,
                },
            ),
            (
                child_metadata[1],
                CoreDebuggerStaticMetadata {
                    program: entry,
                    metadata_handle: 401,
                    metadata_generation: 402,
                },
            ),
        ]),
    );
    let public_entry = JavaScriptPageDebuggerProgram {
        program_handle: public_entry.program_handle,
        program_generation: public_entry.program_generation,
    };
    let frames = executor
        .capture_core_linked_pause(tab_id, 1, public_entry, 4)
        .unwrap();
    assert_eq!(
        frames[0].frame.program_handle,
        public_dependency.program_handle
    );
    assert_eq!(frames[1].frame.program_handle, public_entry.program_handle);
    assert_ne!(frames[0].frame.frame_handle, frames[1].frame.frame_handle);
    assert_eq!(frames[0].safe_point.bytecode_offset, 0);
    assert_eq!(frames[1].safe_point.bytecode_offset, 8);
    assert_eq!(
        executor.capture_core_linked_pause(tab_id, 1, public_entry, 4),
        Ok(frames)
    );
    assert_eq!(
        executor.debugger_linked_execution_state(tab_id, 1, public_entry),
        Ok(JavaScriptPageDebuggerLinkedExecutionState::Paused {
            stack: JavaScriptPageDebuggerLinkedStackSnapshot { frames }
        })
    );
    assert_eq!(
        executor.debugger_linked_stack_snapshot(frames[0].frame, 4),
        Ok(JavaScriptPageDebuggerLinkedStackSnapshot { frames })
    );
    let targets = [
        JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
            program_handle: public_dependency.program_handle,
            program_generation: public_dependency.program_generation,
            metadata_handle: 301,
            metadata_generation: 302,
            source_id: 13,
            code_unit_ordinal: 1,
            bytecode_offset: 0,
        },
        JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
            program_handle: public_entry.program_handle,
            program_generation: public_entry.program_generation,
            metadata_handle: 401,
            metadata_generation: 402,
            source_id: 16,
            code_unit_ordinal: 0,
            bytecode_offset: 8,
        },
    ];
    let access = JavaScriptPageDebuggerLinkedSpanAccess {
        granted: true,
        metadata_receipted: [true; 2],
        source_receipted: [true; 2],
        targets,
    };
    let expected_stack = JavaScriptPageDebuggerLinkedStackSnapshot { frames };
    let static_target = JavaScriptPageDebuggerStaticScopeTarget::Linked {
        metadata: JavaScriptPageDebuggerStaticMetadata {
            metadata_handle: 401,
            metadata_generation: 402,
        },
        expected_stack,
        frame_index: 1,
        scope_entry: JavaScriptPageDebuggerScopeEntry {
            slot_ordinal: 0,
            scope_depth: 0,
        },
    };
    assert_eq!(
        executor
            .debugger_static_scope_relation(tab_id, 1, static_target)
            .unwrap(),
        JavaScriptPageDebuggerStaticScopeRelation {
            target: static_target,
            symbol_type: JavaScriptPageDebuggerStaticMetadataSymbolType {
                symbol_id: 2,
                type_id: 1,
            },
        }
    );
    let PageHostDebuggerStaticScopeTarget::Linked {
        frame: private_frame,
        expected_stack: private_stack,
        frame_index: private_index,
        metadata: private_metadata,
        scope_entry: private_entry,
    } = executor.child.relation_target.clone().unwrap()
    else {
        panic!("linked core target must stay linked across the private boundary");
    };
    assert_eq!(private_frame, child_frame);
    assert_eq!(*private_stack, stack);
    assert_eq!(private_index, 1);
    assert_eq!(private_metadata, child_metadata[1]);
    assert_eq!(private_entry.slot_ordinal, 0);
    assert_eq!(
        executor.debugger_linked_scope_snapshot(expected_stack),
        Ok(JavaScriptPageDebuggerLinkedScopeSnapshot {
            stack: expected_stack,
            scope_entries: vec![JavaScriptPageDebuggerScopeEntry {
                slot_ordinal: 0,
                scope_depth: 0,
            }],
        })
    );
    assert_eq!(
        executor.capture_core_linked_pause(tab_id, 1, public_entry, 4),
        Ok(frames)
    );
    let mut moved_stack = expected_stack;
    moved_stack.frames[1].safe_point.bytecode_offset += 1;
    assert!(executor
        .debugger_linked_scope_snapshot(moved_stack)
        .is_err());
    let mut swapped_stack = expected_stack;
    swapped_stack.frames.swap(0, 1);
    let linked_target = |metadata, expected_stack, frame_index, scope_entry| {
        JavaScriptPageDebuggerStaticScopeTarget::Linked {
            metadata,
            expected_stack,
            frame_index,
            scope_entry,
        }
    };
    let JavaScriptPageDebuggerStaticScopeTarget::Linked {
        metadata,
        expected_stack,
        frame_index,
        scope_entry,
    } = static_target
    else {
        unreachable!();
    };
    for denied in [
        linked_target(
            JavaScriptPageDebuggerStaticMetadata {
                metadata_handle: 301,
                metadata_generation: 302,
            },
            expected_stack,
            frame_index,
            scope_entry,
        ),
        linked_target(metadata, expected_stack, 0, scope_entry),
        linked_target(
            metadata,
            expected_stack,
            frame_index,
            JavaScriptPageDebuggerScopeEntry {
                slot_ordinal: 9,
                scope_depth: 0,
            },
        ),
        linked_target(metadata, moved_stack, frame_index, scope_entry),
        linked_target(metadata, swapped_stack, frame_index, scope_entry),
    ] {
        assert!(executor
            .debugger_static_scope_relation(tab_id, 1, denied)
            .is_err());
    }
    assert_eq!(executor.child.relation_calls, 1);
    executor.child.relation_reply = Some(PageHostReply::DebuggerStaticScopeRelation(Box::new(
        page_host::PageHostDebuggerStaticScopeRelation {
            target: PageHostDebuggerStaticScopeTarget::Linked {
                frame: private_frame,
                expected_stack: private_stack,
                frame_index: 1,
                metadata: child_metadata[0],
                scope_entry: private_entry,
            },
            symbol_type: PageHostDebuggerBlueTsMetadataSymbolType {
                symbol_id: 2,
                type_id: 1,
            },
        },
    )));
    assert!(executor
        .debugger_static_scope_relation(tab_id, 1, static_target)
        .is_err());
    executor.child.relation_reply = None;
    assert!(executor
        .debugger_static_scope_relation(tab_id, 2, static_target)
        .is_err());
    if let PageHostReply::DebuggerLinkedStackSnapshot { snapshot, .. } = &mut executor.child.stack {
        snapshot.frames[1].scope_entries[0].scope_depth = 1;
    }
    assert!(executor
        .debugger_linked_scope_snapshot(expected_stack)
        .is_err());
    assert!(executor
        .debugger_static_scope_relation(tab_id, 1, static_target)
        .is_err());
    assert_eq!(executor.child.relation_calls, 2);
    executor.child.stack = PageHostReply::DebuggerLinkedStackSnapshot {
        frame: child_frame,
        snapshot: Box::new(stack.clone()),
    };
    assert_eq!(
        executor.capture_core_linked_pause(tab_id, 1, public_entry, 4),
        Ok(frames)
    );
    for malformed in [
        PageHostDebuggerLinkedStackSnapshot {
            frames: [
                stack.frames[0].clone(),
                PageHostDebuggerLinkedStackFrame {
                    scope_entries: vec![
                        PageHostDebuggerScopeEntry {
                            slot_ordinal: 0,
                            scope_depth: 0,
                        },
                        PageHostDebuggerScopeEntry {
                            slot_ordinal: 0,
                            scope_depth: 1,
                        },
                    ],
                    ..stack.frames[1].clone()
                },
            ],
            ..stack.clone()
        },
        PageHostDebuggerLinkedStackSnapshot {
            frames: [
                stack.frames[0].clone(),
                PageHostDebuggerLinkedStackFrame {
                    scope_truncated: true,
                    ..stack.frames[1].clone()
                },
            ],
            ..stack.clone()
        },
    ] {
        executor.child.stack = PageHostReply::DebuggerLinkedStackSnapshot {
            frame: child_frame,
            snapshot: Box::new(malformed),
        };
        assert_eq!(
            executor.capture_core_linked_pause(tab_id, 1, public_entry, 4),
            Ok(frames)
        );
        assert!(executor
            .debugger_linked_scope_snapshot(expected_stack)
            .is_err());
    }
    executor.child.stack = PageHostReply::DebuggerLinkedStackSnapshot {
        frame: child_frame,
        snapshot: Box::new(stack.clone()),
    };
    assert_eq!(
        executor.capture_core_linked_pause(tab_id, 1, public_entry, 4),
        Ok(frames)
    );
    for denied in [
        JavaScriptPageDebuggerLinkedSpanAccess {
            granted: false,
            ..access
        },
        JavaScriptPageDebuggerLinkedSpanAccess {
            metadata_receipted: [true, false],
            ..access
        },
        JavaScriptPageDebuggerLinkedSpanAccess {
            source_receipted: [false, true],
            ..access
        },
    ] {
        assert_eq!(
            executor.debugger_linked_stack_spans(expected_stack, denied),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
    }
    assert_eq!(executor.child.span_calls, 0);
    assert_eq!(
        executor.debugger_linked_stack_spans(
            expected_stack,
            JavaScriptPageDebuggerLinkedSpanAccess {
                targets: [
                    JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                        metadata_handle: targets[1].metadata_handle,
                        metadata_generation: targets[1].metadata_generation,
                        ..targets[0]
                    },
                    targets[1],
                ],
                ..access
            }
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    assert_eq!(executor.child.span_calls, 0);
    assert_eq!(
        executor.debugger_linked_stack_spans(
            expected_stack,
            JavaScriptPageDebuggerLinkedSpanAccess {
                targets: [targets[1], targets[0]],
                ..access
            }
        ),
        Err(JavaScriptPageDebuggerError::InvalidSafePoint)
    );
    assert_eq!(executor.child.span_calls, 0);
    let mut forged_stack = expected_stack;
    forged_stack.frames[1].safe_point.bytecode_offset += 1;
    assert_eq!(
        executor.debugger_linked_stack_spans(forged_stack, access),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert_eq!(executor.child.span_calls, 0);
    assert_eq!(
        executor.debugger_linked_stack_spans(expected_stack, access),
        Ok([span(13), span(16)].map(|span| {
            JavaScriptPageDebuggerStaticMetadataSafePointSpan {
                source_id: span.source_id,
                start_byte: span.start_byte,
                end_byte: span.end_byte,
                coordinates: span.coordinates,
            }
        }))
    );
    assert_eq!(executor.child.span_calls, 1);
    assert_eq!(
        executor.resume_debugger_linked_nested_execution(frames[0].frame),
        Ok(())
    );
    assert_eq!(executor.child.resume_calls, 1);

    let moved = PageHostDebuggerLinkedFrame {
        invocation_serial: 32,
        ..child_frame
    };
    executor.child.state = PageHostReply::DebuggerLinkedExecutionState {
        frame: moved,
        state: PageHostDebuggerLinkedExecutionState::Paused {
            safe_point: points[0],
        },
    };
    executor.child.stack = PageHostReply::DebuggerLinkedStackSnapshot {
        frame: moved,
        snapshot: Box::new(stack),
    };
    let moved_frames = executor
        .capture_core_linked_pause(tab_id, 1, public_entry, 4)
        .unwrap();
    assert_ne!(
        moved_frames[0].frame.frame_handle,
        frames[0].frame.frame_handle
    );
    assert_ne!(
        moved_frames[1].frame.frame_handle,
        frames[1].frame.frame_handle
    );
    assert_eq!(
        executor.debugger_linked_stack_snapshot(frames[0].frame, 4),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert_eq!(
        executor.resume_debugger_linked_nested_execution(frames[0].frame),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert_eq!(executor.child.resume_calls, 1);
    executor.child.resume = PageHostReply::DebuggerLinkedNestedResumeRequested { frame: moved };
    assert_eq!(
        executor.resume_debugger_linked_nested_execution(moved_frames[0].frame),
        Ok(())
    );
    assert_eq!(executor.child.resume_calls, 2);
    executor.child.state = PageHostReply::DebuggerLinkedExecutionState {
        frame: moved,
        state: PageHostDebuggerLinkedExecutionState::Resuming,
    };
    assert_eq!(
        executor.debugger_linked_execution_state(tab_id, 1, public_entry),
        Ok(JavaScriptPageDebuggerLinkedExecutionState::Resuming {
            frame: moved_frames[0].frame
        })
    );
    executor.child.state = PageHostReply::DebuggerLinkedExecutionState {
        frame: moved,
        state: PageHostDebuggerLinkedExecutionState::Paused {
            safe_point: points[0],
        },
    };
    assert_eq!(
        executor.debugger_linked_stack_spans(expected_stack, access),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert!(executor
        .debugger_static_scope_relation(tab_id, 1, static_target)
        .is_err());
    assert_eq!(executor.child.span_calls, 1);
    executor.child.stack = PageHostReply::DebuggerLinkedStackSnapshot {
        frame: child_frame,
        snapshot: Box::new(PageHostDebuggerLinkedStackSnapshot {
            frames: points.map(|safe_point| PageHostDebuggerLinkedStackFrame {
                safe_point,
                scope_entries: vec![],
                scope_truncated: false,
            }),
            stack_truncated: false,
            max_scope_entries: 4,
        }),
    };
    assert_eq!(
        executor.capture_core_linked_pause(tab_id, 1, public_entry, 4),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert!(!executor.debugger_linked_frames.contains_key(&tab_id));
    executor.child.state = PageHostReply::DebuggerExecutionState {
        tab_id: 7,
        document_generation: 1,
        program: entry,
        state: PageHostDebuggerExecutionState::Pending,
    };
    assert_eq!(
        executor.debugger_linked_execution_state(tab_id, 1, public_entry),
        Ok(JavaScriptPageDebuggerLinkedExecutionState::Pending)
    );
    executor.child.state = PageHostReply::DebuggerExecutionState {
        tab_id: 7,
        document_generation: 1,
        program: entry,
        state: PageHostDebuggerExecutionState::Completed,
    };
    assert_eq!(
        executor.debugger_linked_execution_state(tab_id, 1, public_entry),
        Ok(JavaScriptPageDebuggerLinkedExecutionState::Completed)
    );
    executor.child.state = PageHostReply::DebuggerLinkedExecutionState {
        frame: moved,
        state: PageHostDebuggerLinkedExecutionState::Paused {
            safe_point: points[0],
        },
    };
    executor.child.stack = PageHostReply::DebuggerLinkedStackSnapshot {
        frame: moved,
        snapshot: Box::new(PageHostDebuggerLinkedStackSnapshot {
            frames: points.map(|safe_point| PageHostDebuggerLinkedStackFrame {
                safe_point,
                scope_entries: vec![],
                scope_truncated: false,
            }),
            stack_truncated: false,
            max_scope_entries: 1,
        }),
    };
    executor
        .debugger_programs
        .get_mut(&tab_id)
        .unwrap()
        .remove(&dependency);
    assert_eq!(
        executor.debugger_linked_execution_state(tab_id, 1, public_entry),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    assert!(!executor.debugger_linked_frames.contains_key(&tab_id));
}

#[test]
fn core_linked_arm_requires_two_live_programs_and_exact_child_acknowledgement() {
    struct LinkedArmChild {
        arm_calls: usize,
        bad_acknowledgement: bool,
    }
    impl PageHostClient for LinkedArmChild {
        fn synchronize_document(&mut self, _: PageHostDocument) -> io::Result<PageHostReply> {
            Err(io::Error::new(io::ErrorKind::Unsupported, "unused"))
        }

        fn close_realm(&mut self, _: u64, _: u64) -> io::Result<PageHostReply> {
            Err(io::Error::new(io::ErrorKind::Unsupported, "unused"))
        }

        fn debugger_execution_control_available(&self) -> bool {
            true
        }

        fn debugger_linked_frames_available(&self) -> bool {
            true
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

        fn arm_debugger_linked_nested_safe_point_breakpoint(
            &mut self,
            tab_id: u64,
            document_generation: u64,
            entry_program: PageHostDebuggerProgram,
            safe_point: PageHostDebuggerSafePoint,
        ) -> io::Result<PageHostReply> {
            self.arm_calls += 1;
            Ok(
                PageHostReply::DebuggerLinkedNestedSafePointBreakpointArmed {
                    tab_id,
                    document_generation,
                    entry_program,
                    safe_point: if self.bad_acknowledgement {
                        PageHostDebuggerSafePoint {
                            bytecode_offset: safe_point.bytecode_offset + 1,
                            ..safe_point
                        }
                    } else {
                        safe_point
                    },
                },
            )
        }
    }
    let tab_id = TabId::from_u64(7);
    let child_entry = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 12,
    };
    let child_dependency = PageHostDebuggerProgram {
        program_handle: 21,
        program_generation: 22,
    };
    let public_entry = JavaScriptPageDebuggerProgram {
        program_handle: 101,
        program_generation: 102,
    };
    let public_dependency = JavaScriptPageDebuggerProgram {
        program_handle: 201,
        program_generation: 202,
    };
    let point = JavaScriptPageDebuggerSafePoint {
        code_unit_ordinal: 1,
        bytecode_offset: 0,
    };
    let mut executor =
        OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(LinkedArmChild {
            arm_calls: 0,
            bad_acknowledgement: false,
        });
    assert!(executor.debugger_linked_frames_available());
    executor.live_documents.insert(
        tab_id,
        LiveDocument {
            document_generation: 1,
            origin: "https://example.test".into(),
        },
    );
    executor.debugger_programs.insert(
        tab_id,
        BTreeMap::from([
            (
                child_entry,
                CoreDebuggerProgram {
                    program_handle: public_entry.program_handle,
                    program_generation: public_entry.program_generation,
                },
            ),
            (
                child_dependency,
                CoreDebuggerProgram {
                    program_handle: public_dependency.program_handle,
                    program_generation: public_dependency.program_generation,
                },
            ),
        ]),
    );
    assert_eq!(
        executor.arm_debugger_linked_nested_safe_point_breakpoint(
            tab_id,
            1,
            public_entry,
            public_dependency,
            point,
        ),
        Ok(())
    );
    assert_eq!(executor.child.arm_calls, 1);
    assert_eq!(
        executor.arm_debugger_linked_nested_safe_point_breakpoint(
            tab_id,
            1,
            public_entry,
            public_entry,
            point,
        ),
        Err(JavaScriptPageDebuggerError::InvalidSafePoint)
    );
    assert_eq!(
        executor.arm_debugger_linked_nested_safe_point_breakpoint(
            tab_id,
            2,
            public_entry,
            public_dependency,
            point,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert_eq!(executor.child.arm_calls, 1);
    executor.child.bad_acknowledgement = true;
    assert_eq!(
        executor.arm_debugger_linked_nested_safe_point_breakpoint(
            tab_id,
            1,
            public_entry,
            public_dependency,
            point,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert_eq!(executor.child.arm_calls, 2);
}

#[test]
fn real_child_linked_modules_return_two_exact_original_spans() {
    use blueice_ipc::debugger::{
        DebuggerLinkedArmTarget, DebuggerLinkedExecutionState,
        DebuggerLinkedStackCoordinatesTarget, DebuggerPageRealm, DebuggerProgram, DebuggerReply,
        DebuggerRequest, DebuggerSafePoint, DebuggerStaticMetadataHandle,
        DebuggerStaticMetadataSourceId,
    };

    struct LinkedGraphAuthorizer;

    impl OutOfProcessPageScriptSourceAuthorizer for LinkedGraphAuthorizer {
        fn authorize(
            &self,
            request: &OutOfProcessPageScriptSourceRequest,
        ) -> Result<
            AuthorizedOutOfProcessPageScriptGraph,
            OutOfProcessPageScriptSourceAuthorizationError,
        > {
            if request.document_generation != 1
                || request.ordinal != 0
                || request.language
                    != CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Module)
                || request.declared_src != "/assets/linked-entry.ts"
            {
                return Err(OutOfProcessPageScriptSourceAuthorizationError::new(
                    "unexpected linked graph request",
                ));
            }
            let entry = "https://cdn.example.test/assets/linked-entry.ts";
            let dependency = "https://cdn.example.test/assets/linked-dependency.ts";
            let loader = AuthorizedModuleLoader::new(
                    [
                        AuthorizedModule::new(
                            entry,
                            "import { inner } from './linked-dependency.ts'; export const answer: number = inner() + 1;",
                        ),
                        AuthorizedModule::new(
                            dependency,
                            "export function inner(): number { return 41; }",
                        ),
                    ],
                    [AuthorizedModuleResolution::new(
                        entry,
                        "./linked-dependency.ts",
                        dependency,
                    )],
                )
                .unwrap();
            Ok(AuthorizedOutOfProcessPageScriptGraph::BlueTs(
                AuthorizedPageScriptGraph::new(entry, loader, "core-linked-test-policy-v1")
                    .unwrap(),
            ))
        }
    }

    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
            "<script type=\"application/x-blueice-typescript-module\" src=\"/assets/linked-entry.ts\"></script>",
            "https://example.test/app/index.html",
        );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
        &path,
        &token,
        LinkedGraphAuthorizer,
    )
    .unwrap();
    executor.enable_debugger_execution_control();
    executor.synchronize_and_execute(&tabs).unwrap();
    let programs = executor.debugger_programs(tab_id, 1).unwrap();
    assert_eq!(programs.len(), 2);
    let (dependency, point) = programs
        .iter()
        .find_map(|program| {
            executor
                .debugger_safe_points(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .ok()?
                .into_iter()
                .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
                .map(|point| (*program, point))
        })
        .expect("the linked dependency has a verified function entry");
    let entry = *programs
        .iter()
        .find(|program| **program != dependency)
        .unwrap();
    let realm = DebuggerPageRealm {
        browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: tab_id.as_u64(),
        realm_generation: 1,
    };
    let public_entry = DebuggerProgram {
        realm,
        program_handle: entry.program_handle,
        program_generation: entry.program_generation,
    };
    let public_dependency = DebuggerProgram {
        realm,
        program_handle: dependency.program_handle,
        program_generation: dependency.program_generation,
    };
    let arm = DebuggerLinkedArmTarget {
        entry: public_entry,
        dependency_safe_point: DebuggerSafePoint {
            program: public_dependency,
            code_unit_ordinal: point.code_unit_ordinal,
            bytecode_offset: point.bytecode_offset,
        },
    };
    assert_eq!(
        crate::debugger::handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ArmLinkedNestedSafePointBreakpoint { target: arm },
        ),
        DebuggerReply::LinkedNestedSafePointBreakpointArmed { target: arm }
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    let DebuggerReply::LinkedExecutionState { state, .. } =
        crate::debugger::handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetLinkedExecutionState {
                entry: public_entry,
            },
        )
    else {
        panic!("the public route must return linked state")
    };
    let DebuggerLinkedExecutionState::Paused {
        stack: public_stack,
    } = *state
    else {
        panic!("the public route must return both paused frames")
    };
    assert_eq!(public_stack.frames[0].frame.program, public_dependency);
    assert_eq!(public_stack.frames[1].frame.program, public_entry);
    assert_eq!(
        crate::debugger::handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetLinkedStack {
                top_frame: public_stack.frames[0].frame,
            },
        ),
        DebuggerReply::LinkedStack(Box::new(public_stack))
    );
    let JavaScriptPageDebuggerLinkedExecutionState::Paused { stack } = executor
        .debugger_linked_execution_state(tab_id, 1, entry)
        .unwrap()
    else {
        panic!("the linked child must pause in its dependency")
    };
    assert_eq!(
        stack.frames[0].frame.program_handle,
        dependency.program_handle
    );
    assert_eq!(stack.frames[1].frame.program_handle, entry.program_handle);
    assert_eq!(
        executor
            .debugger_linked_stack_snapshot(stack.frames[0].frame, 256)
            .unwrap(),
        stack
    );
    let targets = [dependency, entry].map(|program| {
        let metadata = executor
            .debugger_static_metadata(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()[0];
        let source = executor
            .debugger_static_metadata_sources(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                metadata.metadata_handle,
                metadata.metadata_generation,
            )
            .unwrap()[0];
        (metadata, source)
    });
    assert_ne!(targets[0].0.metadata_handle, targets[1].0.metadata_handle);
    let entry_scope = executor
        .debugger_linked_scope_snapshot(stack)
        .unwrap()
        .scope_entries
        .first()
        .copied()
        .expect("linked entry root must retain an active lexical slot");
    let static_target = JavaScriptPageDebuggerStaticScopeTarget::Linked {
        metadata: targets[1].0,
        expected_stack: stack,
        frame_index: 1,
        scope_entry: JavaScriptPageDebuggerScopeEntry {
            slot_ordinal: entry_scope.slot_ordinal,
            scope_depth: entry_scope.scope_depth,
        },
    };
    assert_eq!(
        executor
            .debugger_static_scope_relation(tab_id, 1, static_target)
            .unwrap()
            .target,
        static_target
    );
    let scope_entry = JavaScriptPageDebuggerScopeEntry {
        slot_ordinal: entry_scope.slot_ordinal,
        scope_depth: entry_scope.scope_depth,
    };
    for denied in [
        JavaScriptPageDebuggerStaticScopeTarget::Linked {
            metadata: targets[0].0,
            expected_stack: stack,
            frame_index: 1,
            scope_entry,
        },
        JavaScriptPageDebuggerStaticScopeTarget::Linked {
            metadata: targets[1].0,
            expected_stack: stack,
            frame_index: 0,
            scope_entry,
        },
        JavaScriptPageDebuggerStaticScopeTarget::Linked {
            metadata: JavaScriptPageDebuggerStaticMetadata {
                metadata_generation: targets[1].0.metadata_generation + 1,
                ..targets[1].0
            },
            expected_stack: stack,
            frame_index: 1,
            scope_entry,
        },
        JavaScriptPageDebuggerStaticScopeTarget::Linked {
            metadata: targets[1].0,
            expected_stack: stack,
            frame_index: 1,
            scope_entry: JavaScriptPageDebuggerScopeEntry {
                slot_ordinal: scope_entry.slot_ordinal + 1,
                ..scope_entry
            },
        },
        JavaScriptPageDebuggerStaticScopeTarget::Linked {
            metadata: targets[1].0,
            expected_stack: JavaScriptPageDebuggerLinkedStackSnapshot {
                frames: [
                    stack.frames[0],
                    JavaScriptPageDebuggerLinkedStackFrame {
                        safe_point: JavaScriptPageDebuggerSafePoint {
                            bytecode_offset: stack.frames[1].safe_point.bytecode_offset + 1,
                            ..stack.frames[1].safe_point
                        },
                        ..stack.frames[1]
                    },
                ],
            },
            frame_index: 1,
            scope_entry,
        },
    ] {
        assert!(executor
            .debugger_static_scope_relation(tab_id, 1, denied)
            .is_err());
    }
    let access = JavaScriptPageDebuggerLinkedSpanAccess {
        granted: true,
        metadata_receipted: [true; 2],
        source_receipted: [true; 2],
        targets: [0, 1].map(|index| {
            let frame = stack.frames[index];
            let (metadata, source) = targets[index];
            JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                program_handle: frame.frame.program_handle,
                program_generation: frame.frame.program_generation,
                metadata_handle: metadata.metadata_handle,
                metadata_generation: metadata.metadata_generation,
                source_id: source.source_id,
                code_unit_ordinal: frame.safe_point.code_unit_ordinal,
                bytecode_offset: frame.safe_point.bytecode_offset,
            }
        }),
    };
    let public_sources = [0, 1].map(|index| DebuggerStaticMetadataSourceId {
        metadata: DebuggerStaticMetadataHandle {
            program: public_stack.frames[index].frame.program,
            metadata_handle: targets[index].0.metadata_handle,
            metadata_generation: targets[index].0.metadata_generation,
        },
        source_id: targets[index].1.source_id,
    });
    let ungranted = crate::debugger::handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::GetLinkedStackCoordinates {
            target: DebuggerLinkedStackCoordinatesTarget {
                expected_stack: public_stack,
                sources: public_sources,
            },
        },
    );
    assert!(
        matches!(ungranted, DebuggerReply::Unsupported { .. }),
        "ungranted linked coordinates returned {ungranted:?}"
    );
    let spans = executor.debugger_linked_stack_spans(stack, access).unwrap();
    assert!(spans.iter().all(|span| span
        .coordinates
        .is_well_formed_for_range(span.start_byte, span.end_byte)));
    assert_eq!(spans[0].source_id, targets[0].1.source_id);
    assert_eq!(spans[1].source_id, targets[1].1.source_id);
    assert_eq!(
        executor.debugger_linked_stack_spans(
            stack,
            JavaScriptPageDebuggerLinkedSpanAccess {
                targets: [access.targets[1], access.targets[0]],
                ..access
            }
        ),
        Err(JavaScriptPageDebuggerError::InvalidSafePoint)
    );
    assert_eq!(
        crate::debugger::handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ResumeLinkedNestedExecution {
                top_frame: public_stack.frames[0].frame,
            },
        ),
        DebuggerReply::LinkedNestedResumeRequested {
            top_frame: public_stack.frames[0].frame,
        }
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(executor
        .debugger_static_scope_relation(tab_id, 1, static_target)
        .is_err());
    assert!(matches!(
        executor.debugger_execution_state(
            tab_id,
            1,
            entry.program_handle,
            entry.program_generation
        ),
        Ok(JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal: 0,
            ..
        })
    ));
    executor
        .resume_debugger_execution(tab_id, 1, entry.program_handle, entry.program_generation)
        .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.debugger_linked_execution_state(tab_id, 1, entry),
        Ok(JavaScriptPageDebuggerLinkedExecutionState::Completed)
    );
    let mut tabs = tabs;
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<p>linked successor</p>",
        Some("https://example.test/app/successor.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.debugger_static_scope_relation(tab_id, 1, static_target),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}
