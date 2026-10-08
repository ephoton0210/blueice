// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn page_host_connection_waits_for_a_bound_socket_to_start_listening() {
    use std::os::unix::net::UnixListener;

    let path = unique_socket_path("bound-before-listen");
    let stale_listener = UnixListener::bind(&path).unwrap();
    drop(stale_listener);
    assert_eq!(
        UnixStream::connect(&path).unwrap_err().kind(),
        io::ErrorKind::ConnectionRefused
    );

    let child_path = path.clone();
    let child = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        std::fs::remove_file(&child_path).unwrap();
        let listener = UnixListener::bind(&child_path).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        assert_eq!(
            page_host::read_page_host_request(&mut stream).unwrap(),
            PageHostRequest::Hello {
                protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
                session_token: "owner-capability".to_string(),
            }
        );
        page_host::write_page_host_reply(
            &mut stream,
            &PageHostReply::HelloAck {
                protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
            },
        )
        .unwrap();
    });

    let connection = PageHostConnection::connect(&path, "owner-capability").unwrap();
    drop(connection);
    child.join().unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn page_host_transport_pumps_before_the_child_replies() {
    let (core, mut child) = UnixStream::pair().unwrap();
    let (resume_sender, resume_receiver) = mpsc::sync_channel(1);
    let child_task = thread::spawn(move || {
        assert_eq!(
            page_host::read_page_host_request(&mut child).unwrap(),
            PageHostRequest::Shutdown
        );
        resume_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        page_host::write_page_host_reply(&mut child, &PageHostReply::ShutdownAck).unwrap();
    });
    let session_thread = thread::current().id();
    let mut pump_calls = 0;
    let reply = PageHostConnection { stream: core }
        .request_while_pumping_script(PageHostRequest::Shutdown, &mut || {
            assert_eq!(thread::current().id(), session_thread);
            pump_calls += 1;
            if pump_calls == 1 {
                resume_sender.send(()).unwrap();
            }
            Ok(())
        })
        .unwrap();
    child_task.join().unwrap();
    assert_eq!(reply, PageHostReply::ShutdownAck);
    assert!(pump_calls > 0);
}

#[test]
fn private_exception_location_wrapper_preserves_the_exact_child_tuple() {
    let (core, mut child) = UnixStream::pair().unwrap();
    let program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 1 << 63,
        metadata_generation: 1 << 63,
    };
    let expected = PageHostReply::DebuggerBlueTsExceptionLocation {
        tab_id: 7,
        document_generation: 3,
        program,
        metadata,
        location: page_host::PageHostDebuggerBlueTsExceptionLocation {
            safe_point: PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: 1,
                bytecode_offset: 4,
            },
            span: page_host::PageHostDebuggerBlueTsSafePointSpan {
                source_id: 0,
                start_byte: 2,
                end_byte: 8,
                coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: 2,
                    end_line: 0,
                    end_column_utf16: 8,
                },
            },
        },
    };
    let child_reply = expected.clone();
    let child_task = thread::spawn(move || {
        assert_eq!(
            page_host::read_page_host_request(&mut child).unwrap(),
            PageHostRequest::DescribeDebuggerBlueTsExceptionLocation {
                tab_id: 7,
                document_generation: 3,
                program,
                metadata,
            }
        );
        page_host::write_page_host_reply(&mut child, &child_reply).unwrap();
    });
    let mut connection = PageHostConnection { stream: core };
    assert!(connection.debugger_bluets_exception_location_available());
    assert_eq!(
        connection
            .debugger_bluets_exception_location(7, 3, program, metadata)
            .unwrap(),
        expected
    );
    child_task.join().unwrap();
}

#[test]
fn page_host_transport_times_out_and_poison_closes_a_stalled_child() {
    let (core, mut child) = UnixStream::pair().unwrap();
    let (release_sender, release_receiver) = mpsc::sync_channel(1);
    let child_task = thread::spawn(move || {
        assert_eq!(
            page_host::read_page_host_request(&mut child).unwrap(),
            PageHostRequest::Shutdown
        );
        release_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
    });
    let mut connection = PageHostConnection { stream: core };
    let started = Instant::now();
    let error = connection
        .request_while_pumping_script_with_timeout(
            PageHostRequest::Shutdown,
            &mut || Ok(()),
            Duration::from_millis(100),
        )
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(connection.request(PageHostRequest::Shutdown).is_err());
    release_sender.send(()).unwrap();
    child_task.join().unwrap();
}

#[test]
fn page_host_transport_fails_promptly_when_the_child_disconnects() {
    let (core, mut child) = UnixStream::pair().unwrap();
    let child_task = thread::spawn(move || {
        assert_eq!(
            page_host::read_page_host_request(&mut child).unwrap(),
            PageHostRequest::Shutdown
        );
    });
    let started = Instant::now();
    let error = PageHostConnection { stream: core }
        .request_while_pumping_script_with_timeout(
            PageHostRequest::Shutdown,
            &mut || Ok(()),
            Duration::from_secs(3),
        )
        .unwrap_err();
    child_task.join().unwrap();
    assert_ne!(error.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn nested_child_wait_rejects_calls_after_its_total_budget() {
    let (mut tabs, tab_id) = loaded_tabs(
        "<div id='target'>before</div>",
        "https://example.test/nested-budget.html",
    );
    let target = ScriptDocumentTarget {
        tab_id: tab_id.as_u64(),
        document_generation: tabs.get(tab_id).unwrap().document_generation(),
    };
    let (sender, receiver) = crate::script::script_request_channel();
    let first = thread::spawn({
        let sender = sender.clone();
        move || {
            sender.request(blueice_ipc::script::ScriptRequest::GetElementById {
                target,
                id: "target".to_string(),
            })
        }
    });
    let mut remaining = 1;
    let deadline = Instant::now() + Duration::from_secs(1);
    while remaining > 0 {
        pump_script_requests_during_child_wait(&receiver, &mut tabs, target, &mut remaining)
            .unwrap();
        assert!(
            Instant::now() < deadline,
            "first nested request did not arrive"
        );
        thread::yield_now();
    }
    let ScriptReply::Node { node: Some(node) } = first.join().unwrap().unwrap() else {
        panic!("the first request must resolve the target node");
    };
    pump_script_requests_during_child_wait(&receiver, &mut tabs, target, &mut remaining).unwrap();
    let before = tabs.get(tab_id).unwrap().dom_dump();
    let excess = thread::spawn(move || {
        sender.request(blueice_ipc::script::ScriptRequest::SetTextContent {
            target,
            node,
            value: "over budget".to_string(),
        })
    });
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if pump_script_requests_during_child_wait(&receiver, &mut tabs, target, &mut remaining)
            .is_err()
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "excess nested request did not arrive"
        );
        thread::yield_now();
    }
    assert!(matches!(
        excess.join().unwrap().unwrap(),
        ScriptReply::Error { .. }
    ));
    assert_eq!(tabs.get(tab_id).unwrap().dom_dump(), before);
}

struct ReentrantScriptChild {
    script_sender: crate::script::ScriptRequestSender,
}

fn nested_dom_write(
    sender: crate::script::ScriptRequestSender,
    target: ScriptDocumentTarget,
    value: &'static str,
    pump: &mut dyn FnMut() -> io::Result<()>,
) -> io::Result<()> {
    let (done_sender, done_receiver) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let result = (|| {
            let ScriptReply::Node { node: Some(node) } =
                sender.request(blueice_ipc::script::ScriptRequest::GetElementById {
                    target,
                    id: "target".to_string(),
                })?
            else {
                return Err(io::Error::other("target was not found"));
            };
            sender.request(blueice_ipc::script::ScriptRequest::SetTextContent {
                target,
                node,
                value: value.to_string(),
            })
        })();
        done_sender.send(result).unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let script_reply = loop {
        pump()?;
        match done_receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(result) => break result?,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::other("script worker disconnected"));
            }
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "nested DOM call stalled",
                ));
            }
        }
    };
    worker.join().unwrap();
    assert_eq!(script_reply, ScriptReply::Ack);
    Ok(())
}

impl PageHostClient for ReentrantScriptChild {
    fn synchronize_document(&mut self, _document: PageHostDocument) -> io::Result<PageHostReply> {
        panic!("a session-owned child sync must use the nested script pump");
    }

    fn synchronize_document_with_script_pump(
        &mut self,
        document: PageHostDocument,
        pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        let target = ScriptDocumentTarget {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
        };
        nested_dom_write(
            self.script_sender.clone(),
            target,
            "from nested script",
            pump,
        )?;
        Ok(PageHostReply::Synchronized {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
            already_current: false,
            reports: Vec::new(),
        })
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        Ok(PageHostReply::RealmClosed {
            tab_id,
            document_generation,
        })
    }

    fn advance_debugger_execution_with_script_pump(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        nested_dom_write(
            self.script_sender.clone(),
            ScriptDocumentTarget {
                tab_id,
                document_generation,
            },
            "from resumed script",
            pump,
        )?;
        Ok(PageHostReply::DebuggerExecutionAdvanced {
            tab_id,
            document_generation,
            reports: Vec::new(),
        })
    }
}

#[test]
fn child_document_sync_serves_dom_calls_on_the_owning_session_thread() {
    let (mut tabs, tab_id) = loaded_tabs(
        "<div id='target'>before</div><script>let page = 1;</script>",
        "https://example.test/nested-dom.html",
    );
    let (script_sender, script_receiver) = crate::script::script_request_channel();
    let mut executor =
        OutOfProcessJavaScriptPageExecutor::new(ReentrantScriptChild { script_sender });
    executor
        .synchronize_and_execute_serving_script(&mut tabs, &script_receiver)
        .unwrap();
    assert!(tabs
        .get(tab_id)
        .unwrap()
        .dom_dump()
        .contains("from nested script"));
    assert_eq!(executor.live_documents.len(), 1);
}

#[test]
fn debugger_resume_serves_dom_calls_on_the_owning_session_thread() {
    let (mut tabs, tab_id) = loaded_tabs(
        "<div id='target'>before</div><script>let page = 1;</script>",
        "https://example.test/nested-debugger-dom.html",
    );
    let (script_sender, script_receiver) = crate::script::script_request_channel();
    let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
        ReentrantScriptChild { script_sender },
    );
    executor
        .synchronize_and_execute_serving_script(&mut tabs, &script_receiver)
        .unwrap();
    executor
        .synchronize_and_execute_serving_script(&mut tabs, &script_receiver)
        .unwrap();
    assert!(tabs
        .get(tab_id)
        .unwrap()
        .dom_dump()
        .contains("from resumed script"));
    assert_eq!(executor.live_documents.len(), 1);
}

/// Records only the lifecycle advance requests needed to prove that a
/// debugger discovery peer receives a finite grace budget rather than an
/// execution lease. It deliberately implements no debugger inspection
/// operation, so no VM/source/bytecode surface is added by this test.
#[derive(Default)]
struct DeferralBudgetChild {
    advances: Vec<(u64, u64)>,
}

impl PageHostClient for DeferralBudgetChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        Ok(PageHostReply::Synchronized {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
            already_current: false,
            reports: Vec::new(),
        })
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        Ok(PageHostReply::RealmClosed {
            tab_id,
            document_generation,
        })
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn advance_debugger_execution(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.advances.push((tab_id, document_generation));
        Ok(PageHostReply::DebuggerExecutionAdvanced {
            tab_id,
            document_generation,
            reports: Vec::new(),
        })
    }
}

#[test]
fn oop_debugger_discovery_deferrals_are_finite_per_document() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>let pendingDebuggerAdmission = true;</script>",
        "https://example.test/pending-debugger.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
        DeferralBudgetChild::default(),
    );
    executor.synchronize_and_execute(&tabs).unwrap();

    // One hold is accepted for each of the fixed number of session turns,
    // matching a debugger peer that asks one discovery/configuration
    // question per core tick. The next request cannot keep the document
    // pending; core advances it through the child lifecycle instead.
    for _ in 0..MAX_OOP_DEBUGGER_EXECUTION_DEFERRALS_PER_DOCUMENT {
        executor.hold_pending_debugger_execution_once();
        executor.synchronize_and_execute(&tabs).unwrap();
    }
    assert!(executor.child.advances.is_empty());

    executor.hold_pending_debugger_execution_once();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.child.advances, vec![(tab_id.as_u64(), 1)]);
}

#[test]
fn reserved_oop_tab_does_not_delay_an_unreserved_pending_tab_or_spend_its_budget() {
    let (mut tabs, reserved_tab) = loaded_tabs(
        "<script>let first = 1;</script>",
        "https://example.test/first.html",
    );
    let other_tab = tabs.open_tab();
    tabs.get_mut(other_tab).unwrap().load_html_str(
        "<script>let second = 2;</script>",
        Some("https://example.test/second.html".to_string()),
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
        DeferralBudgetChild::default(),
    );
    executor.synchronize_and_execute(&tabs).unwrap();

    PageJavaScriptExecutor::hold_reserved_debugger_execution_once(&mut executor, reserved_tab);
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.child.advances, vec![(other_tab.as_u64(), 1)]);
    assert_eq!(
        executor.debugger_execution_deferrals[&reserved_tab].remaining,
        MAX_OOP_DEBUGGER_EXECUTION_DEFERRALS_PER_DOCUMENT
    );

    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.child.advances,
        vec![
            (other_tab.as_u64(), 1),
            (reserved_tab.as_u64(), 1),
            (other_tab.as_u64(), 1),
        ]
    );
}

/// Every `PageHostConnection` debugger dispatch method is a thin wrapper
/// around `request`/`request_while_pumping_script`: it forwards whatever the
/// child replies without matching on the reply's variant (that validation
/// lives one layer up, in `child_reply_validation.rs`). So a single fixed
/// canned reply, echoed back once per request in call order, is sufficient
/// to exercise every wrapper's own line without needing a realistic reply
/// shape per method — the per-method request/reply *shape* pairing is
/// already covered by more targeted tests elsewhere in this file (e.g.
/// `private_exception_location_wrapper_preserves_the_exact_child_tuple`).
#[test]
fn every_debugger_dispatch_method_forwards_its_reply_and_every_capability_flag_is_true() {
    use blueice_ipc::page_host::PageHostDebuggerLinkedStackFrame;

    let (core, mut child) = UnixStream::pair().unwrap();
    let canned = PageHostReply::ShutdownAck;
    let expected_calls = 45;
    let child_task = thread::spawn(move || {
        for _ in 0..expected_calls {
            page_host::read_page_host_request(&mut child).unwrap();
            page_host::write_page_host_reply(&mut child, &PageHostReply::ShutdownAck).unwrap();
        }
    });

    let mut connection = PageHostConnection { stream: core };

    assert!(connection.debugger_breakpoint_configuration_available());
    assert!(connection.debugger_bluets_metadata_available());
    assert!(connection.debugger_bluets_metadata_summary_available());
    assert!(connection.debugger_bluets_metadata_lowering_summary_available());
    assert!(connection.debugger_bluets_metadata_sources_available());
    assert!(connection.debugger_bluets_metadata_source_provenance_available());
    assert!(connection.debugger_bluets_metadata_types_available());
    assert!(connection.debugger_bluets_metadata_type_display_available());
    assert!(connection.debugger_bluets_metadata_symbols_available());
    assert!(connection.debugger_bluets_metadata_contracts_available());
    assert!(connection.debugger_bluets_metadata_contract_display_available());
    assert!(connection.debugger_bluets_metadata_contract_validation_available());
    assert!(connection.debugger_bluets_metadata_symbol_display_available());
    assert!(connection.debugger_bluets_metadata_symbol_location_available());
    assert!(connection.debugger_bluets_metadata_contract_location_available());
    assert!(connection.debugger_bluets_safe_point_span_available());
    assert!(connection.debugger_bluets_exception_location_available());
    assert!(connection.debugger_bluets_source_breakpoint_available());
    assert!(connection.debugger_bluets_metadata_symbol_type_available());
    assert!(connection.debugger_bluets_metadata_symbol_contract_available());
    assert!(connection.debugger_execution_control_available());
    assert!(connection.debugger_nested_frames_available());
    assert!(connection.debugger_linked_frames_available());
    assert!(connection.debugger_stack_snapshot_available());
    assert!(connection.debugger_value_snapshot_available());
    assert!(connection.debugger_stepping_available());
    assert!(connection.debugger_bluets_source_span_step_available());

    let program = PageHostDebuggerProgram {
        program_handle: 1,
        program_generation: 1,
    };
    let dependency_program = PageHostDebuggerProgram {
        program_handle: 2,
        program_generation: 2,
    };
    let metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 1,
        metadata_generation: 1,
    };
    let safe_point = PageHostDebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: 0,
    };
    let scope_entry = PageHostDebuggerScopeEntry {
        slot_ordinal: 0,
        scope_depth: 0,
    };
    let value_target = PageHostDebuggerValueTarget {
        tab_id: 1,
        document_generation: 1,
        program,
        frame: None,
        frame_index: 0,
        safe_point,
        scope_entry,
    };
    let frame = PageHostDebuggerFrame {
        tab_id: 1,
        document_generation: 1,
        program,
        code_unit_ordinal: 1,
        invocation_serial: 1,
    };
    let linked_frame = PageHostDebuggerLinkedFrame {
        tab_id: 1,
        document_generation: 1,
        entry_program: program,
        dependency_program,
        code_unit_ordinal: 1,
        invocation_serial: 1,
    };
    let linked_stack_frame = PageHostDebuggerLinkedStackFrame {
        safe_point,
        scope_entries: Vec::new(),
        scope_truncated: false,
    };
    let linked_snapshot = PageHostDebuggerLinkedStackSnapshot {
        frames: [linked_stack_frame.clone(), linked_stack_frame],
        stack_truncated: false,
        max_scope_entries: 1,
    };
    let linked_source = PageHostDebuggerLinkedSource {
        metadata,
        source_id: 0,
    };
    let static_scope_target = PageHostDebuggerStaticScopeTarget::Ordinary {
        metadata,
        target: value_target,
    };
    let linked_value_target = PageHostDebuggerLinkedValueTarget {
        frame: linked_frame,
        expected_stack: Box::new(linked_snapshot.clone()),
        frame_index: 1,
        scope_entry,
    };

    assert_eq!(
        connection.debugger_realm_stats(1, 1).unwrap(),
        canned.clone()
    );
    assert_eq!(connection.child_stats().unwrap(), canned.clone());
    assert_eq!(
        connection.debugger_programs(1, 1).unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection.debugger_bluets_metadata(1, 1, program).unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_summary(1, 1, program, metadata)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_lowering_summary(1, 1, program, metadata)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_sources(1, 1, program, metadata)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_source_provenance(1, 1, program, metadata, 0)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_types(1, 1, program, metadata)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_type_display(1, 1, program, metadata, 0)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_symbols(1, 1, program, metadata)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_contracts(1, 1, program, metadata)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_contract_display(1, 1, program, metadata, 0)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_contract_validation(
                1,
                1,
                program,
                metadata,
                0,
                CompilerContractValue::Null,
            )
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_symbol_display(1, 1, program, metadata, 0)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_symbol_location(1, 1, program, metadata, 0)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_contract_location(1, 1, program, metadata, 0)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_safe_point_span(1, 1, metadata, safe_point)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_exception_location(1, 1, program, metadata)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_source_breakpoint(1, 1, program, metadata, 0, 0)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_symbol_type(1, 1, program, metadata, 0, 0)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_bluets_metadata_symbol_contract(1, 1, program, metadata, 0, 0)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection.debugger_safe_points(1, 1, program).unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .validate_debugger_safe_point(1, 1, safe_point)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .set_debugger_breakpoint(1, 1, safe_point)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection.debugger_breakpoints(1, 1).unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .clear_debugger_breakpoint(1, 1, safe_point)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_stack_snapshot(1, 1, program, None, 1, 1)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection.debugger_value_snapshot(value_target).unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_linked_value_snapshot(linked_value_target)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_static_scope_relation(static_scope_target)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .arm_debugger_nested_safe_point_breakpoint(1, 1, safe_point)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection.step_debugger_nested_instruction(frame).unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .resume_debugger_nested_execution(frame)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .arm_debugger_linked_nested_safe_point_breakpoint(1, 1, program, safe_point)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_linked_stack_snapshot(linked_frame, 1)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_linked_stack_spans(linked_frame, linked_snapshot, [linked_source, linked_source])
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .resume_debugger_linked_nested_execution(linked_frame)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .arm_debugger_root_safe_point_breakpoint(1, 1, safe_point)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .debugger_execution_state(1, 1, program)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection.resume_debugger_execution(1, 1, program).unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .step_debugger_root_instruction(1, 1, program)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .step_debugger_bluets_source_span(1, 1, metadata, 0, safe_point)
            .unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection.advance_debugger_execution(1, 1).unwrap(),
        canned.clone()
    );
    assert_eq!(
        connection
            .advance_debugger_execution_with_script_pump(1, 1, &mut || Ok(()))
            .unwrap(),
        canned
    );

    child_task.join().unwrap();
}
