// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// A hostile authenticated child still cannot acknowledge a different
/// realm and make core treat the requested document as eligible for any
/// debugger lifecycle record.
struct MismatchedSynchronizationChild;

impl PageHostClient for MismatchedSynchronizationChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        Ok(PageHostReply::Synchronized {
            tab_id: document.tab_id.saturating_add(1),
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
}

#[test]
fn mismatched_child_synchronization_cannot_seed_oop_debugger_lifecycle() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>let childLifecycleSecret = 1;</script>",
        "https://example.test/hostile-child.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
        MismatchedSynchronizationChild,
    );
    executor.synchronize_and_execute(&tabs).unwrap();

    assert!(!executor.debugger_has_live_realm(tab_id, 1));
    assert!(executor.debugger_execution_deferrals.is_empty());
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![JavaScriptPageExecutionReport::Rejected {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 0,
            kind: BlueJsPageScriptKind::Classic,
            category: "out-of-process JavaScript host is unavailable",
        }]
    );
}

struct MismatchedDebuggerChild;

impl PageHostClient for MismatchedDebuggerChild {
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

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::RealmStats(page_host::PageHostRealmStats {
            tab_id,
            document_generation,
            program_count: 1,
            bytecode_bytes: 0,
            heap_bytes: 0,
        }))
    }

    fn debugger_programs(
        &mut self,
        _tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerPrograms {
            // A peer must never be able to retarget core's tab merely by
            // returning a plausible private record under another tuple.
            tab_id: 99,
            document_generation,
            programs: vec![PageHostDebuggerProgram {
                program_handle: 1,
                program_generation: 1,
            }],
        })
    }
}

#[test]
fn core_rejects_a_child_debugger_reply_with_a_mismatched_realm_tuple() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>let noChildSourceLeak = true;</script>",
        "https://example.test/app.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(MismatchedDebuggerChild);
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(executor.debugger_has_live_realm(tab_id, 1));
    let realm = blueice_ipc::debugger::DebuggerPageRealm {
        browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: tab_id.as_u64(),
        realm_generation: 1,
    };
    let capabilities = crate::debugger::handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        blueice_ipc::debugger::DebuggerRequest::DescribeCapabilities { realm },
    );
    let blueice_ipc::debugger::DebuggerReply::Capabilities(capabilities) = capabilities else {
        panic!("expected child debugger capability report");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::ProgramLocations
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::BreakpointConfiguration
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Planned
    }));
    assert_eq!(
        executor.debugger_programs(tab_id, 1),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

/// A hostile private peer can know its own protocol shape, so core must
/// reject malformed breakpoint acknowledgements rather than treating an
/// authenticated socket as a source of public debugger identities.
struct MalformedBreakpointChild;

impl PageHostClient for MalformedBreakpointChild {
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

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::RealmStats(page_host::PageHostRealmStats {
            tab_id,
            document_generation,
            program_count: 1,
            bytecode_bytes: 0,
            heap_bytes: 0,
        }))
    }

    fn debugger_programs(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerPrograms {
            tab_id,
            document_generation,
            programs: vec![PageHostDebuggerProgram {
                program_handle: 1,
                program_generation: 1,
            }],
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

    fn set_debugger_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerBreakpointSet {
            // The reply is otherwise plausible, but a core must bind it
            // to the original tab and never silently retarget a request.
            tab_id: tab_id + 1,
            document_generation,
            safe_point,
        })
    }

    fn debugger_breakpoints(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerBreakpoints {
            tab_id,
            // A generation mismatch must not reveal or remint a record.
            document_generation: document_generation + 1,
            safe_points: Vec::new(),
        })
    }

    fn clear_debugger_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerBreakpointCleared {
            tab_id,
            document_generation,
            // A response must echo the exact child-private safe point,
            // not merely one with a valid private program ID.
            safe_point: PageHostDebuggerSafePoint {
                bytecode_offset: safe_point.bytecode_offset.saturating_add(1),
                ..safe_point
            },
            was_present: true,
        })
    }
}

#[test]
fn core_fails_closed_on_malformed_child_breakpoint_replies() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>let childBreakpointSecret = 1;</script>",
        "https://example.test/app.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(MalformedBreakpointChild);
    executor.synchronize_and_execute(&tabs).unwrap();
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    let result = executor.set_debugger_breakpoint(
        tab_id,
        1,
        program.program_handle,
        program.program_generation,
        0,
        0,
    );
    assert_eq!(result, Err(JavaScriptPageDebuggerError::NoLiveRealm));
    assert_eq!(
        executor.debugger_breakpoints(tab_id, 1),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert_eq!(
        executor.clear_debugger_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            0,
            0,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_validates_and_installs_source_free_document_snapshots_in_the_real_child() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
        concat!(
            "<p>core snapshot marker</p>",
            "<script>",
            "if (blueiceDocumentOrigin() !== 'https://example.test') throw 'origin';",
            "if (blueiceDocumentText() === '') throw 'text';",
            "if (typeof document !== 'undefined' || typeof fetch !== 'undefined') throw 'ambient';",
            "</script>"
        ),
        "https://example.test/app/index.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![JavaScriptPageExecutionReport::Executed {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 0,
            kind: BlueJsPageScriptKind::Classic,
        }]
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn over_budget_core_snapshot_is_never_sent_to_the_child() {
    let oversized = "x".repeat(blueice_ipc::page_host::PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES + 1);
    let html = format!("<p>{oversized}</p><script>blueiceDocumentText();</script>");
    let (tabs, tab_id) = loaded_tabs(&html, "https://example.test/app/index.html");
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![JavaScriptPageExecutionReport::Rejected {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 0,
            kind: BlueJsPageScriptKind::Classic,
            category: "host binding contract rejected the page script",
        }]
    );
    let child = executor.into_child();
    assert!(child.documents.is_empty());
    assert!(child.closes.is_empty());
}

#[test]
fn core_routes_interleaved_bluets_and_javascript_to_one_child_realm() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script>globalThis.beforeBlueTs = true;</script>",
                "<script type=\"application/x-blueice-typescript\">const sharedAnswer: number = 42;</script>",
                "<script>if (!globalThis.beforeBlueTs || sharedAnswer !== 42) throw 'shared realm failed';</script>",
                "<script type=\"application/x-blueice-typescript\">blueiceDocumentText();</script>",
                "<script type=\"application/x-blueice-typescript\" src=\"untrusted.ts\"></script>"
            ),
            "https://example.test/app/index.html",
        );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Classic,
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 2,
                kind: BlueJsPageScriptKind::Classic,
            },
        ]
    );
    assert_eq!(
        executor.drain_blue_ts_reports_for_tab(tab_id),
        vec![
            BlueTsPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 1,
                kind: DirectPageScriptKind::Classic,
            },
            BlueTsPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 3,
                kind: DirectPageScriptKind::Classic,
            },
            BlueTsPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 4,
                kind: DirectPageScriptKind::Classic,
                category: "external BlueTS declarations require an authorized loader",
            },
        ]
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn navigation_replaces_the_child_document_and_close_releases_its_realm() {
    let (path, token, child) = spawn_child();
    let (mut tabs, tab_id) = loaded_tabs(
        "<script>globalThis.firstSnapshot = blueiceDocumentText();</script>",
        "https://example.test/first",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(matches!(
        executor.drain_reports_for_tab(tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Executed {
            document_generation: 1,
            ..
        }]
    ));
    tabs.get_mut(tab_id).unwrap().load_html_str(
        concat!(
            "<script>",
            "if (typeof globalThis.firstSnapshot !== 'undefined') throw 'stale realm';",
            "if (blueiceDocumentOrigin() !== 'https://second.example.test') throw 'origin';",
            "globalThis.second = true;",
            "</script>"
        ),
        Some("https://second.example.test/second".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(matches!(
        executor.drain_reports_for_tab(tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Executed {
            document_generation: 2,
            ..
        }]
    ));
    assert!(tabs.close_tab(tab_id));
    executor.synchronize_and_execute(&tabs).unwrap();
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}
