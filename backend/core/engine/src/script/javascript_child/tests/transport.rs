// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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
