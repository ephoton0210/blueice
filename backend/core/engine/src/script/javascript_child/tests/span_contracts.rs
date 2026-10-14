// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn core_child_transport_resolves_only_live_exact_bluets_safe_point_spans() {
    let (path, token, child) = spawn_child();
    let (mut tabs, tab_id) = loaded_tabs(
        "<script type=\"application/x-blueice-typescript\">const mapped: number = 42;</script>",
        "https://example.test/mapped.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let child_stats = executor.child_stats().unwrap();
    assert_eq!(child_stats.realm_count, 1);
    assert!(child_stats.is_well_formed());
    assert!(executor.child.debugger_bluets_safe_point_span_available());

    let PageHostReply::DebuggerPrograms { programs, .. } = executor
        .child
        .debugger_programs(tab_id.as_u64(), 1)
        .unwrap()
    else {
        panic!("expected a child-private BlueTS program");
    };
    let program = programs[0];
    let PageHostReply::DebuggerBlueTsMetadata { metadata, .. } = executor
        .child
        .debugger_bluets_metadata(tab_id.as_u64(), 1, program)
        .unwrap()
    else {
        panic!("expected a child-private metadata attachment");
    };
    let metadata = metadata[0];
    let PageHostReply::DebuggerSafePoints { safe_points, .. } = executor
        .child
        .debugger_safe_points(tab_id.as_u64(), 1, program)
        .unwrap()
    else {
        panic!("expected child-private safe points");
    };
    let (safe_point, span) = safe_points
        .into_iter()
        .find_map(|safe_point| {
            match executor
                .child
                .debugger_bluets_safe_point_span(tab_id.as_u64(), 1, metadata, safe_point)
                .unwrap()
            {
                PageHostReply::DebuggerBlueTsSafePointSpan {
                    safe_point: echoed,
                    span,
                    ..
                } if echoed == safe_point => Some((safe_point, span)),
                PageHostReply::Error {
                    code: page_host::PageHostErrorCode::InvalidRequest,
                    ..
                } => None,
                reply => panic!("unexpected exact-span reply: {reply:?}"),
            }
        })
        .expect("one verified safe point must have a retained BlueTS span");
    assert!(span.start_byte < span.end_byte);
    assert!(!format!("{span:?}").contains("mapped"));

    let public_program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    let public_metadata = executor
        .debugger_static_metadata(
            tab_id,
            1,
            public_program.program_handle,
            public_program.program_generation,
        )
        .unwrap()[0];
    let public_sources = executor
        .debugger_static_metadata_sources(
            tab_id,
            1,
            public_program.program_handle,
            public_program.program_generation,
            public_metadata.metadata_handle,
            public_metadata.metadata_generation,
        )
        .unwrap();
    assert!(public_sources
        .iter()
        .any(|source| source.source_id == span.source_id));
    let target = JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
        program_handle: public_program.program_handle,
        program_generation: public_program.program_generation,
        metadata_handle: public_metadata.metadata_handle,
        metadata_generation: public_metadata.metadata_generation,
        source_id: span.source_id,
        code_unit_ordinal: safe_point.code_unit_ordinal,
        bytecode_offset: safe_point.bytecode_offset,
    };
    assert_eq!(
        executor
            .debugger_static_metadata_safe_point_span(tab_id, 1, target)
            .unwrap(),
        JavaScriptPageDebuggerStaticMetadataSafePointSpan {
            source_id: span.source_id,
            start_byte: span.start_byte,
            end_byte: span.end_byte,
            coordinates: span.coordinates,
        }
    );
    assert!(executor.debugger_static_metadata_source_breakpoint_available());
    let breakpoint_target = JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget {
        program_handle: public_program.program_handle,
        program_generation: public_program.program_generation,
        metadata_handle: public_metadata.metadata_handle,
        metadata_generation: public_metadata.metadata_generation,
        source_id: span.source_id,
        source_byte: span.start_byte,
    };
    assert_eq!(
        executor
            .debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target)
            .unwrap(),
        Some(JavaScriptPageDebuggerSafePoint {
            code_unit_ordinal: safe_point.code_unit_ordinal,
            bytecode_offset: safe_point.bytecode_offset,
        })
    );
    assert_eq!(
        executor
            .debugger_static_metadata_source_breakpoint(
                tab_id,
                1,
                JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget {
                    source_byte: span.end_byte,
                    ..breakpoint_target
                },
            )
            .unwrap(),
        None,
    );
    assert_eq!(
        executor.debugger_static_metadata_source_breakpoint(
            tab_id,
            1,
            JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget {
                metadata_generation: breakpoint_target.metadata_generation + 1,
                ..breakpoint_target
            },
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    assert_eq!(
        executor.debugger_static_metadata_safe_point_span(
            tab_id,
            1,
            JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                source_id: u32::MAX,
                ..target
            },
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    assert_eq!(
        executor.debugger_static_metadata_safe_point_span(
            tab_id,
            1,
            JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                metadata_generation: target.metadata_generation + 1,
                ..target
            },
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    assert_eq!(
        executor.debugger_static_metadata_safe_point_span(
            tab_id,
            1,
            JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                bytecode_offset: u32::MAX,
                ..target
            },
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script>const successor = true;</script>",
        Some("https://example.test/successor.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.debugger_static_metadata_safe_point_span(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert_eq!(
        executor.debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert!(matches!(
        executor
            .child
            .debugger_bluets_safe_point_span(tab_id.as_u64(), 1, metadata, safe_point)
            .unwrap(),
        PageHostReply::Error {
            code: page_host::PageHostErrorCode::StaleDocument,
            ..
        }
    ));

    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

struct MalformedSafePointSpanChild {
    lifecycle: RecordingChild,
    reply: PageHostReply,
}

struct ExceptionLocationChild {
    lifecycle: RecordingChild,
    exception_reply: PageHostReply,
    point_reply: PageHostReply,
    span_reply: PageHostReply,
}

impl PageHostClient for ExceptionLocationChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        self.lifecycle.synchronize_document(document)
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        self.lifecycle.close_realm(tab_id, document_generation)
    }

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.lifecycle
            .debugger_realm_stats(tab_id, document_generation)
    }

    fn debugger_bluets_metadata_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_sources_available(&self) -> bool {
        true
    }

    fn debugger_bluets_safe_point_span_available(&self) -> bool {
        true
    }

    fn debugger_bluets_exception_location_available(&self) -> bool {
        true
    }

    fn debugger_bluets_exception_location(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        Ok(self.exception_reply.clone())
    }

    fn validate_debugger_safe_point(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(self.point_reply.clone())
    }

    fn debugger_bluets_safe_point_span(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _metadata: PageHostDebuggerMetadataHandle,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(self.span_reply.clone())
    }
}

#[test]
fn core_exception_adapter_remints_only_a_revalidated_live_child_location() {
    let (tabs, tab_id) = loaded_tabs(
        "<script type=\"application/x-blueice-typescript\">const typed: number = 1;</script>",
        "https://example.test/exception-adapter.html",
    );
    let child_program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let child_metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 17,
        metadata_generation: 19,
    };
    let child_point = PageHostDebuggerSafePoint {
        program: child_program,
        code_unit_ordinal: 1,
        bytecode_offset: 4,
    };
    let span = page_host::PageHostDebuggerBlueTsSafePointSpan {
        source_id: 3,
        start_byte: 2,
        end_byte: 8,
        coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
            start_line: 0,
            start_column_utf16: 2,
            end_line: 0,
            end_column_utf16: 8,
        },
    };
    let exception_reply = PageHostReply::DebuggerBlueTsExceptionLocation {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        program: child_program,
        metadata: child_metadata,
        location: page_host::PageHostDebuggerBlueTsExceptionLocation {
            safe_point: child_point,
            span,
        },
    };
    let point_reply = PageHostReply::DebuggerSafePointValidated {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        safe_point: child_point,
    };
    let span_reply = PageHostReply::DebuggerBlueTsSafePointSpan {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        metadata: child_metadata,
        safe_point: child_point,
        span,
    };
    let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
        ExceptionLocationChild {
            lifecycle: RecordingChild::default(),
            exception_reply: exception_reply.clone(),
            point_reply: point_reply.clone(),
            span_reply: span_reply.clone(),
        },
    );
    executor.synchronize_and_execute(&tabs).unwrap();
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
    executor.debugger_static_metadata.insert(
        tab_id,
        BTreeMap::from([(
            child_metadata,
            CoreDebuggerStaticMetadata {
                program: child_program,
                metadata_handle: 107,
                metadata_generation: 109,
            },
        )]),
    );
    let target = JavaScriptPageDebuggerExceptionLocationTarget {
        program_handle: 101,
        program_generation: 103,
        metadata_handle: 107,
        metadata_generation: 109,
        source_id: 3,
    };
    assert!(executor.debugger_exception_location_available());
    assert_eq!(
        executor.debugger_exception_location(tab_id, 1, target),
        Ok(JavaScriptPageDebuggerExceptionLocation {
            source_id: 3,
            code_unit_ordinal: 1,
            bytecode_offset: 4,
            start_byte: 2,
            end_byte: 8,
            coordinates: span.coordinates,
        })
    );

    executor.child.exception_reply = PageHostReply::DebuggerBlueTsExceptionLocation {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        program: PageHostDebuggerProgram {
            program_generation: child_program.program_generation + 1,
            ..child_program
        },
        metadata: child_metadata,
        location: page_host::PageHostDebuggerBlueTsExceptionLocation {
            safe_point: child_point,
            span,
        },
    };
    assert_eq!(
        executor.debugger_exception_location(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    executor.child.exception_reply = PageHostReply::DebuggerBlueTsExceptionLocation {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        program: child_program,
        metadata: child_metadata,
        location: page_host::PageHostDebuggerBlueTsExceptionLocation {
            span: page_host::PageHostDebuggerBlueTsSafePointSpan {
                source_id: 4,
                ..span
            },
            safe_point: child_point,
        },
    };
    assert_eq!(
        executor.debugger_exception_location(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    executor.child.exception_reply = exception_reply.clone();
    executor.child.point_reply = PageHostReply::DebuggerSafePointValidated {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        safe_point: PageHostDebuggerSafePoint {
            bytecode_offset: 5,
            ..child_point
        },
    };
    assert_eq!(
        executor.debugger_exception_location(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    executor.child.point_reply = point_reply;
    executor.child.span_reply = PageHostReply::DebuggerBlueTsSafePointSpan {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        metadata: child_metadata,
        safe_point: child_point,
        span: page_host::PageHostDebuggerBlueTsSafePointSpan {
            end_byte: 9,
            ..span
        },
    };
    assert_eq!(
        executor.debugger_exception_location(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    executor.child.exception_reply = PageHostReply::Error {
        code: PageHostErrorCode::InvalidDebuggerState,
        message: "not completed".to_string(),
    };
    assert_eq!(
        executor.debugger_exception_location(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert!(executor
        .debugger_exception_location(tab_id, 2, target)
        .is_err());

    let mut denied = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
    assert!(!denied.debugger_exception_location_available());
    assert_eq!(
        denied.debugger_exception_location(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

impl PageHostClient for MalformedSafePointSpanChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        self.lifecycle.synchronize_document(document)
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        self.lifecycle.close_realm(tab_id, document_generation)
    }

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.lifecycle
            .debugger_realm_stats(tab_id, document_generation)
    }

    fn debugger_bluets_metadata_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_sources_available(&self) -> bool {
        true
    }

    fn debugger_bluets_safe_point_span_available(&self) -> bool {
        true
    }

    fn debugger_bluets_source_breakpoint_available(&self) -> bool {
        true
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn debugger_stepping_available(&self) -> bool {
        true
    }

    fn debugger_bluets_source_span_step_available(&self) -> bool {
        true
    }

    fn step_debugger_bluets_source_span(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _metadata: PageHostDebuggerMetadataHandle,
        _source_id: u32,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(self.reply.clone())
    }

    fn debugger_bluets_safe_point_span(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _metadata: PageHostDebuggerMetadataHandle,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(self.reply.clone())
    }

    fn debugger_bluets_source_breakpoint(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
        _source_id: u32,
        _source_byte: u32,
    ) -> io::Result<PageHostReply> {
        Ok(self.reply.clone())
    }
}

#[test]
fn core_forwards_zero_based_bluets_source_id_but_rejects_mismatched_step_echo() {
    let (tabs, tab_id) = loaded_tabs(
        "<script type=\"application/x-blueice-typescript\">const zero: number = 1;</script>",
        "https://example.test/zero-source.html",
    );
    let child_program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let child_metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 17,
        metadata_generation: 19,
    };
    let child_safe_point = PageHostDebuggerSafePoint {
        program: child_program,
        code_unit_ordinal: 0,
        bytecode_offset: 4,
    };
    let reply = PageHostReply::DebuggerBlueTsSourceStepRequested {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        metadata: child_metadata,
        source_id: 0,
        safe_point: child_safe_point,
    };
    let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
        MalformedSafePointSpanChild {
            lifecycle: RecordingChild::default(),
            reply: reply.clone(),
        },
    );
    executor.synchronize_and_execute(&tabs).unwrap();
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
    executor.debugger_static_metadata.insert(
        tab_id,
        BTreeMap::from([(
            child_metadata,
            CoreDebuggerStaticMetadata {
                program: child_program,
                metadata_handle: 107,
                metadata_generation: 109,
            },
        )]),
    );
    let target = JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
        program_handle: 101,
        program_generation: 103,
        metadata_handle: 107,
        metadata_generation: 109,
        source_id: 0,
        code_unit_ordinal: 0,
        bytecode_offset: 4,
    };
    assert!(executor.debugger_source_span_stepping_available());
    assert_eq!(
        executor.step_debugger_bluets_source_span(tab_id, 1, target),
        Ok(())
    );
    executor.child.reply = PageHostReply::DebuggerBlueTsSourceStepRequested {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        metadata: child_metadata,
        source_id: 1,
        safe_point: child_safe_point,
    };
    assert_eq!(
        executor.step_debugger_bluets_source_span(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_mismatched_child_safe_point_span_envelopes() {
    let (tabs, tab_id) = loaded_tabs(
        "<script type=\"application/x-blueice-typescript\">const mapped: number = 42;</script>",
        "https://example.test/forged-span.html",
    );
    let child_program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let child_metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 17,
        metadata_generation: 19,
    };
    let child_safe_point = PageHostDebuggerSafePoint {
        program: child_program,
        code_unit_ordinal: 0,
        bytecode_offset: 4,
    };
    let span = page_host::PageHostDebuggerBlueTsSafePointSpan {
        source_id: 3,
        start_byte: 0,
        end_byte: 5,
        coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
            start_line: 0,
            start_column_utf16: 0,
            end_line: 0,
            end_column_utf16: 5,
        },
    };
    let span_reply = |reply_tab_id, reply_generation, metadata, safe_point, span| {
        PageHostReply::DebuggerBlueTsSafePointSpan {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            metadata,
            safe_point,
            span,
        }
    };
    let reply = span_reply(tab_id.as_u64(), 1, child_metadata, child_safe_point, span);
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(MalformedSafePointSpanChild {
        lifecycle: RecordingChild::default(),
        reply: reply.clone(),
    });
    executor.synchronize_and_execute(&tabs).unwrap();
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
    executor.debugger_static_metadata.insert(
        tab_id,
        BTreeMap::from([(
            child_metadata,
            CoreDebuggerStaticMetadata {
                program: child_program,
                metadata_handle: 107,
                metadata_generation: 109,
            },
        )]),
    );
    let target = JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
        program_handle: 101,
        program_generation: 103,
        metadata_handle: 107,
        metadata_generation: 109,
        source_id: 3,
        code_unit_ordinal: 0,
        bytecode_offset: 4,
    };
    assert_eq!(
        executor
            .debugger_static_metadata_safe_point_span(tab_id, 1, target)
            .unwrap(),
        JavaScriptPageDebuggerStaticMetadataSafePointSpan {
            source_id: 3,
            start_byte: 0,
            end_byte: 5,
            coordinates: span.coordinates,
        }
    );
    for malformed in [
        span_reply(
            tab_id.as_u64() + 1,
            1,
            child_metadata,
            child_safe_point,
            span,
        ),
        span_reply(tab_id.as_u64(), 2, child_metadata, child_safe_point, span),
        span_reply(
            tab_id.as_u64(),
            1,
            PageHostDebuggerMetadataHandle {
                metadata_generation: 20,
                ..child_metadata
            },
            child_safe_point,
            span,
        ),
        span_reply(
            tab_id.as_u64(),
            1,
            child_metadata,
            PageHostDebuggerSafePoint {
                bytecode_offset: 5,
                ..child_safe_point
            },
            span,
        ),
        span_reply(
            tab_id.as_u64(),
            1,
            child_metadata,
            child_safe_point,
            page_host::PageHostDebuggerBlueTsSafePointSpan {
                end_byte: span.start_byte,
                ..span
            },
        ),
        span_reply(
            tab_id.as_u64(),
            1,
            child_metadata,
            child_safe_point,
            page_host::PageHostDebuggerBlueTsSafePointSpan {
                end_byte: DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES + 1,
                ..span
            },
        ),
        span_reply(
            tab_id.as_u64(),
            1,
            child_metadata,
            child_safe_point,
            page_host::PageHostDebuggerBlueTsSafePointSpan {
                coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                    start_column_utf16: span.end_byte + 1,
                    ..span.coordinates
                },
                ..span
            },
        ),
    ] {
        executor.child.reply = malformed;
        assert_eq!(
            executor.debugger_static_metadata_safe_point_span(tab_id, 1, target),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
    }
    executor.child.reply = span_reply(
        tab_id.as_u64(),
        1,
        child_metadata,
        child_safe_point,
        page_host::PageHostDebuggerBlueTsSafePointSpan {
            source_id: 4,
            ..span
        },
    );
    assert_eq!(
        executor.debugger_static_metadata_safe_point_span(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );

    let breakpoint_target = JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget {
        program_handle: 101,
        program_generation: 103,
        metadata_handle: 107,
        metadata_generation: 109,
        source_id: 3,
        source_byte: 2,
    };
    let breakpoint_reply =
        |reply_tab_id, reply_generation, program, metadata, source_id, source_byte, safe_point| {
            PageHostReply::DebuggerBlueTsSourceBreakpoint {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                program,
                metadata,
                source_id,
                source_byte,
                safe_point,
            }
        };
    executor.child.reply = breakpoint_reply(
        tab_id.as_u64(),
        1,
        child_program,
        child_metadata,
        3,
        2,
        Some(child_safe_point),
    );
    assert_eq!(
        executor
            .debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target)
            .unwrap(),
        Some(JavaScriptPageDebuggerSafePoint {
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        })
    );
    executor.child.reply = breakpoint_reply(
        tab_id.as_u64(),
        1,
        child_program,
        child_metadata,
        3,
        2,
        None,
    );
    assert_eq!(
        executor
            .debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target)
            .unwrap(),
        None
    );
    for malformed in [
        breakpoint_reply(8, 1, child_program, child_metadata, 3, 2, None),
        breakpoint_reply(
            tab_id.as_u64(),
            2,
            child_program,
            child_metadata,
            3,
            2,
            None,
        ),
        breakpoint_reply(
            tab_id.as_u64(),
            1,
            PageHostDebuggerProgram {
                program_generation: 14,
                ..child_program
            },
            child_metadata,
            3,
            2,
            None,
        ),
        breakpoint_reply(
            tab_id.as_u64(),
            1,
            child_program,
            PageHostDebuggerMetadataHandle {
                metadata_generation: 20,
                ..child_metadata
            },
            3,
            2,
            None,
        ),
        breakpoint_reply(
            tab_id.as_u64(),
            1,
            child_program,
            child_metadata,
            4,
            2,
            None,
        ),
        breakpoint_reply(
            tab_id.as_u64(),
            1,
            child_program,
            child_metadata,
            3,
            3,
            None,
        ),
        breakpoint_reply(
            tab_id.as_u64(),
            1,
            child_program,
            child_metadata,
            3,
            2,
            Some(PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_generation: 14,
                    ..child_program
                },
                ..child_safe_point
            }),
        ),
    ] {
        executor.child.reply = malformed;
        assert_eq!(
            executor.debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
    }
}
