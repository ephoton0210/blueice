// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn private_static_scope_adapter_requires_valid_exact_echo_for_both_pause_shapes() {
    use blueice_ipc::page_host::{
        PageHostDebuggerLinkedStackFrame, PageHostDebuggerStaticScopeRelation,
        PageHostDebuggerStaticScopeTarget,
    };

    struct DenyChild;
    impl PageHostClient for DenyChild {
        fn synchronize_document(&mut self, _: PageHostDocument) -> io::Result<PageHostReply> {
            unreachable!()
        }
        fn close_realm(&mut self, _: u64, _: u64) -> io::Result<PageHostReply> {
            unreachable!()
        }
    }

    struct ReplyChild {
        reply: PageHostReply,
        seen: Option<PageHostDebuggerStaticScopeTarget>,
    }
    impl PageHostClient for ReplyChild {
        fn synchronize_document(&mut self, _: PageHostDocument) -> io::Result<PageHostReply> {
            unreachable!()
        }
        fn close_realm(&mut self, _: u64, _: u64) -> io::Result<PageHostReply> {
            unreachable!()
        }
        fn debugger_static_scope_relation(
            &mut self,
            target: PageHostDebuggerStaticScopeTarget,
        ) -> io::Result<PageHostReply> {
            self.seen = Some(target);
            Ok(self.reply.clone())
        }
    }

    let program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let dependency = PageHostDebuggerProgram {
        program_handle: 17,
        program_generation: 19,
    };
    let metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 23,
        metadata_generation: 29,
    };
    let entry = PageHostDebuggerScopeEntry {
        slot_ordinal: 2,
        scope_depth: 0,
    };
    let root_point = PageHostDebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: 4,
    };
    let ordinary = PageHostDebuggerStaticScopeTarget::Ordinary {
        metadata,
        target: PageHostDebuggerValueTarget {
            tab_id: 7,
            document_generation: 3,
            program,
            frame: None,
            frame_index: 0,
            safe_point: root_point,
            scope_entry: entry,
        },
    };
    let linked = PageHostDebuggerStaticScopeTarget::Linked {
        frame: PageHostDebuggerLinkedFrame {
            tab_id: 7,
            document_generation: 3,
            entry_program: program,
            dependency_program: dependency,
            code_unit_ordinal: 1,
            invocation_serial: 31,
        },
        expected_stack: Box::new(PageHostDebuggerLinkedStackSnapshot {
            frames: [
                PageHostDebuggerLinkedStackFrame {
                    safe_point: PageHostDebuggerSafePoint {
                        program: dependency,
                        code_unit_ordinal: 1,
                        bytecode_offset: 2,
                    },
                    scope_entries: vec![],
                    scope_truncated: false,
                },
                PageHostDebuggerLinkedStackFrame {
                    safe_point: root_point,
                    scope_entries: vec![entry],
                    scope_truncated: false,
                },
            ],
            stack_truncated: false,
            max_scope_entries: 2,
        }),
        frame_index: 1,
        metadata,
        scope_entry: entry,
    };
    let symbol_type = PageHostDebuggerBlueTsMetadataSymbolType {
        symbol_id: 0,
        type_id: 1,
    };
    assert_eq!(
        ChildStaticScopeAdapter {
            child: &mut DenyChild,
        }
        .describe(ordinary.clone()),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    for target in [ordinary, linked] {
        assert!(target.is_well_formed());
        let relation = |target| {
            PageHostReply::DebuggerStaticScopeRelation(Box::new(
                PageHostDebuggerStaticScopeRelation {
                    target,
                    symbol_type,
                },
            ))
        };
        let mut child = ReplyChild {
            reply: relation(target.clone()),
            seen: None,
        };
        assert_eq!(
            ChildStaticScopeAdapter { child: &mut child }.describe(target.clone()),
            Ok(symbol_type)
        );
        assert_eq!(child.seen, Some(target.clone()));
        child.reply = relation(match &target {
            PageHostDebuggerStaticScopeTarget::Ordinary { target, .. } => {
                PageHostDebuggerStaticScopeTarget::Ordinary {
                    metadata: PageHostDebuggerMetadataHandle {
                        metadata_generation: metadata.metadata_generation + 1,
                        ..metadata
                    },
                    target: *target,
                }
            }
            PageHostDebuggerStaticScopeTarget::Linked {
                frame,
                expected_stack,
                frame_index,
                scope_entry,
                ..
            } => PageHostDebuggerStaticScopeTarget::Linked {
                frame: *frame,
                expected_stack: expected_stack.clone(),
                frame_index: *frame_index,
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_generation: metadata.metadata_generation + 1,
                    ..metadata
                },
                scope_entry: *scope_entry,
            },
        });
        assert_eq!(
            ChildStaticScopeAdapter { child: &mut child }.describe(target.clone()),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
        child.reply = relation(match &target {
            PageHostDebuggerStaticScopeTarget::Ordinary { metadata, target } => {
                PageHostDebuggerStaticScopeTarget::Ordinary {
                    metadata: *metadata,
                    target: PageHostDebuggerValueTarget {
                        safe_point: PageHostDebuggerSafePoint {
                            bytecode_offset: target.safe_point.bytecode_offset + 1,
                            ..target.safe_point
                        },
                        ..*target
                    },
                }
            }
            PageHostDebuggerStaticScopeTarget::Linked {
                frame,
                expected_stack,
                frame_index,
                metadata,
                scope_entry,
            } => {
                let mut moved = expected_stack.clone();
                moved.frames[1].safe_point.bytecode_offset += 1;
                PageHostDebuggerStaticScopeTarget::Linked {
                    frame: *frame,
                    expected_stack: moved,
                    frame_index: *frame_index,
                    metadata: *metadata,
                    scope_entry: *scope_entry,
                }
            }
        });
        assert_eq!(
            ChildStaticScopeAdapter { child: &mut child }.describe(target.clone()),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
        child.reply = PageHostReply::ShutdownAck;
        assert_eq!(
            ChildStaticScopeAdapter { child: &mut child }.describe(target.clone()),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
        child.reply = PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            message: "fixed child error".into(),
        };
        assert_eq!(
            ChildStaticScopeAdapter { child: &mut child }.describe(target.clone()),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        child.seen = None;
        let malformed = match target {
            PageHostDebuggerStaticScopeTarget::Ordinary { target, .. } => {
                PageHostDebuggerStaticScopeTarget::Ordinary {
                    metadata: PageHostDebuggerMetadataHandle {
                        metadata_handle: 0,
                        ..metadata
                    },
                    target,
                }
            }
            PageHostDebuggerStaticScopeTarget::Linked {
                frame,
                expected_stack,
                metadata,
                scope_entry,
                ..
            } => PageHostDebuggerStaticScopeTarget::Linked {
                frame,
                expected_stack,
                frame_index: 0,
                metadata,
                scope_entry,
            },
        };
        assert_eq!(
            ChildStaticScopeAdapter { child: &mut child }.describe(malformed),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        assert_eq!(child.seen, None);
    }
}

#[test]
fn linked_child_adapter_rejects_moved_or_partial_private_replies() {
    use blueice_ipc::debugger::DebuggerSourceCoordinates;
    use blueice_ipc::page_host::{
        PageHostDebuggerBlueTsSafePointSpan, PageHostDebuggerLinkedStackFrame,
    };

    #[derive(Clone)]
    struct LinkedReplies {
        arm: PageHostReply,
        state: PageHostReply,
        stack: PageHostReply,
        spans: PageHostReply,
        resume: PageHostReply,
    }

    impl PageHostClient for LinkedReplies {
        fn synchronize_document(&mut self, _: PageHostDocument) -> io::Result<PageHostReply> {
            Err(io::Error::new(io::ErrorKind::Unsupported, "unused"))
        }

        fn close_realm(&mut self, _: u64, _: u64) -> io::Result<PageHostReply> {
            Err(io::Error::new(io::ErrorKind::Unsupported, "unused"))
        }

        fn arm_debugger_linked_nested_safe_point_breakpoint(
            &mut self,
            _: u64,
            _: u64,
            _: PageHostDebuggerProgram,
            _: PageHostDebuggerSafePoint,
        ) -> io::Result<PageHostReply> {
            Ok(self.arm.clone())
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
            Ok(self.spans.clone())
        }

        fn resume_debugger_linked_nested_execution(
            &mut self,
            _: PageHostDebuggerLinkedFrame,
        ) -> io::Result<PageHostReply> {
            Ok(self.resume.clone())
        }
    }

    let entry = PageHostDebuggerProgram {
        program_handle: 1,
        program_generation: 2,
    };
    let dependency = PageHostDebuggerProgram {
        program_handle: 3,
        program_generation: 4,
    };
    let frame = PageHostDebuggerLinkedFrame {
        tab_id: 7,
        document_generation: 1,
        entry_program: entry,
        dependency_program: dependency,
        code_unit_ordinal: 1,
        invocation_serial: 5,
    };
    let child_point = PageHostDebuggerSafePoint {
        program: dependency,
        code_unit_ordinal: 1,
        bytecode_offset: 0,
    };
    let root_point = PageHostDebuggerSafePoint {
        program: entry,
        code_unit_ordinal: 0,
        bytecode_offset: 8,
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
                metadata_handle: 11,
                metadata_generation: 12,
            },
            source_id: 13,
        },
        PageHostDebuggerLinkedSource {
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 14,
                metadata_generation: 15,
            },
            source_id: 16,
        },
    ];
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
    let spans = [span(13), span(16)];
    let mut replies = LinkedReplies {
        arm: PageHostReply::DebuggerLinkedNestedSafePointBreakpointArmed {
            tab_id: 7,
            document_generation: 1,
            entry_program: entry,
            safe_point: child_point,
        },
        state: PageHostReply::DebuggerLinkedExecutionState {
            frame,
            state: PageHostDebuggerLinkedExecutionState::Paused {
                safe_point: child_point,
            },
        },
        stack: PageHostReply::DebuggerLinkedStackSnapshot {
            frame,
            snapshot: Box::new(snapshot.clone()),
        },
        spans: PageHostReply::DebuggerLinkedStackSpans {
            frame,
            snapshot: Box::new(snapshot.clone()),
            spans: Box::new(spans),
        },
        resume: PageHostReply::DebuggerLinkedNestedResumeRequested { frame },
    };
    {
        let mut adapter = ChildLinkedDebuggerAdapter {
            child: &mut replies,
        };
        assert_eq!(adapter.arm(7, 1, entry, child_point), Ok(()));
        assert_eq!(
            adapter.state(7, 1, entry, None),
            Ok((
                frame,
                PageHostDebuggerLinkedExecutionState::Paused {
                    safe_point: child_point
                }
            ))
        );
        assert_eq!(adapter.stack(frame, 256), Ok(snapshot.clone()));
        assert_eq!(adapter.spans(frame, snapshot.clone(), sources), Ok(spans));
        assert_eq!(adapter.resume(frame), Ok(()));
    }
    let moved = PageHostDebuggerLinkedFrame {
        invocation_serial: frame.invocation_serial + 1,
        ..frame
    };
    replies.arm = PageHostReply::DebuggerLinkedNestedSafePointBreakpointArmed {
        tab_id: 7,
        document_generation: 1,
        entry_program: entry,
        safe_point: root_point,
    };
    replies.state = PageHostReply::DebuggerLinkedExecutionState {
        frame: moved,
        state: PageHostDebuggerLinkedExecutionState::Paused {
            safe_point: child_point,
        },
    };
    replies.stack = PageHostReply::DebuggerLinkedStackSnapshot {
        frame: moved,
        snapshot: Box::new(snapshot.clone()),
    };
    replies.spans = PageHostReply::DebuggerLinkedStackSpans {
        frame,
        snapshot: Box::new(snapshot.clone()),
        spans: Box::new([span(13), span(u32::MAX)]),
    };
    replies.resume = PageHostReply::DebuggerLinkedNestedResumeRequested { frame: moved };
    let mut adapter = ChildLinkedDebuggerAdapter {
        child: &mut replies,
    };
    assert_eq!(
        adapter.arm(7, 1, entry, child_point),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert_eq!(
        adapter.state(7, 1, entry, Some(frame)),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert_eq!(
        adapter.stack(frame, 256),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert_eq!(
        adapter.spans(frame, snapshot, sources),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert_eq!(
        adapter.resume(frame),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}
