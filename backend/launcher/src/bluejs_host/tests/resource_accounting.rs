// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn long_abort_deadlines_use_bounded_task_and_frame_wake_replies() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![classic(0, "true;")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    host.runtime
        .configure_realm_bindings(7, |bindings| {
            bindings.install_web_file_api()?;
            bindings.install_host_timers()?;
            bindings.install_host_animation_frames()
        })
        .unwrap();
    let origin = blueice_bluejs::BlueJsPageOrigin::new("https://example.test").unwrap();
    let handle=host.runtime.install_program(
        7,&origin,
        blueice_bluejs::BlueJsSourceIdentity::new("page:///long-abort.js","sha256:long-abort").unwrap(),
        &blueice_bluejs::BlueJsProgramV1::Script(blueice_bluejs::parse("var pending=AbortSignal.timeout(9007199254740991);pending.onabort=function(){};requestAnimationFrame(function(){});").unwrap()),
    ).unwrap();
    host.runtime.execute_program(7, handle).unwrap();
    assert!(
        matches!(host.handle_request(PageHostRequest::FlushDragTasks { tab_id:7,document_generation:1 }),PageHostReply::DragTasksFlushed { next_wake_in_ms:Some(delay),animation_frame_pending:true,.. } if delay==i32::MAX as u64)
    );
    assert!(
        matches!(host.handle_request(PageHostRequest::RunAnimationFrame { tab_id:7,document_generation:1 }),PageHostReply::AnimationFrameRendered { next_wake_in_ms:Some(delay),animation_frame_pending:false,.. } if delay==i32::MAX as u64)
    );
}

#[test]
fn task_and_frame_replies_release_the_real_vm_timer_payload_charge() {
    assert_eq!(
        blueice_bluejs::MAX_HOST_SCHEDULING_BYTES as u64,
        page_host::PAGE_HOST_SCHEDULING_PAYLOAD_MAX_BYTES
    );
    let socket_path = std::path::PathBuf::from("/tmp")
        .join(format!("bi-sched-ledger-{}.sock", std::process::id()));
    let listener = UnixListener::bind(&socket_path).unwrap();
    let capability = "a".repeat(script::SCRIPT_SESSION_TOKEN_HEX_BYTES);
    let server_capability = capability.clone();
    let server = std::thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        assert_eq!(
            script::read_script_request(&mut peer).unwrap(),
            ScriptRequest::Hello {
                protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                session_token: server_capability
            }
        );
        script::write_script_reply(
            &mut peer,
            &ScriptReply::HelloAck {
                protocol_version: script::SCRIPT_PROTOCOL_VERSION,
            },
        )
        .unwrap();
        let ScriptRequest::Call {
            request_id,
            request,
        } = script::read_script_request(&mut peer).unwrap()
        else {
            panic!("profile admission needs the live owner document");
        };
        let target = request.document_target().unwrap();
        assert_eq!(target.tab_id, 7);
        assert_eq!(target.document_generation, 1);
        assert!(matches!(*request, ScriptRequest::GetDocument { .. }));
        script::write_script_reply(
            &mut peer,
            &ScriptReply::CallResult {
                request_id,
                target,
                reply: Box::new(ScriptReply::NodeCreated { node: 1 }),
            },
        )
        .unwrap();
    });
    let mut host = BlueJsChildHost::default();
    host.configure_script_file_dom_capability(socket_path.clone(), capability)
        .unwrap();
    let PageHostReply::Synchronized { reports, .. } = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(1, vec![classic(0, "function noop() {} function clearOwned() { clearTimeout(owned); } var owned = setTimeout(noop, 2147483647, 'x'.repeat(400000)); requestAnimationFrame(clearOwned);")]),
    }) else { panic!("fixed File DOM profile must admit the timer fixture"); };
    assert!(matches!(
        reports.as_slice(),
        [PageHostScriptReport {
            outcome: PageHostScriptOutcome::Executed,
            ..
        }]
    ));
    let PageHostReply::RealmStats(before) = host.handle_request(PageHostRequest::GetRealmStats {
        tab_id: 7,
        document_generation: 1,
    }) else {
        panic!("retained timer payload must be observable");
    };
    assert!(before.scheduling_payload_bytes >= 800000);
    assert!(
        matches!(host.handle_request(PageHostRequest::FlushDragTasks { tab_id: 7, document_generation: 1 }), PageHostReply::DragTasksFlushed { scheduling_payload_bytes, animation_frame_pending: true, .. } if scheduling_payload_bytes == before.scheduling_payload_bytes)
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::RunAnimationFrame {
            tab_id: 7,
            document_generation: 1
        }),
        PageHostReply::AnimationFrameRendered {
            scheduling_payload_bytes: 0,
            next_wake_in_ms: None,
            animation_frame_pending: false,
            ..
        }
    ));
    let PageHostReply::RealmStats(after) = host.handle_request(PageHostRequest::GetRealmStats {
        tab_id: 7,
        document_generation: 1,
    }) else {
        panic!("completed frame must release its timer storage");
    };
    assert_eq!(after.scheduling_payload_bytes, 0);
    drop(host);
    server.join().unwrap();
    std::fs::remove_file(socket_path).unwrap();
}

fn javascript_module(ordinal: u32, source: &str) -> PageHostScript {
    let id = format!("blueice://page/deferred-{ordinal}.js");
    PageHostScript {
        ordinal,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Module,
        graph: graph(&id, vec![PageHostSource::new(id.clone(), source)]),
    }
}

#[test]
fn child_deferred_queue_counts_all_pending_payload_and_releases_empty_capacity() {
    let bluets_id = "blueice://page/deferred-module.ts";
    let bluets_module = PageHostScript {
        ordinal: 2,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            bluets_id,
            vec![PageHostSource::new(
                bluets_id,
                "export const answer: number = 42;",
            )],
        ),
    };
    let mut host = BlueJsChildHost::default();
    let first = debugger_document(
        1,
        vec![
            javascript_module(0, "export const value = 1;"),
            blue_ts_classic(1, "const classicValue: number = 2; classicValue;"),
            bluets_module,
        ],
    );
    let mut second = debugger_document(
        1,
        vec![javascript_module(0, "export const otherValue = 3;")],
    );
    second.tab_id = 8;
    for document in [first, second] {
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument { document }),
            PageHostReply::Synchronized { reports, .. } if reports.is_empty()
        ));
    }
    let first_slots = host.documents[&7].pending_debugger_executions.capacity()
        * std::mem::size_of::<PendingDebuggerExecution>();
    let first_components = host.retained_deferred_source_graph_bytes(7, 1).unwrap()
        + host.retained_deferred_javascript_ast_bytes(7, 1).unwrap()
        + host
            .retained_deferred_bluets_attachment_bytes(7, 1)
            .unwrap();
    assert!(first_slots > 0 && first_components > 0);
    assert_eq!(
        host.retained_deferred_payload_bytes(7, 1).unwrap(),
        first_slots + first_components
    );
    let second_bytes = host.retained_deferred_payload_bytes(8, 1).unwrap();
    assert!(second_bytes > 0);
    let PageHostReply::RealmStats(first_stats) =
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        })
    else {
        panic!("the first live child realm must report bounded accounting")
    };
    let PageHostReply::RealmStats(second_stats) =
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 8,
            document_generation: 1,
        })
    else {
        panic!("the second live child realm must report bounded accounting")
    };
    assert_eq!(
        first_stats.static_metadata_bytes,
        u64::try_from(host.retained_static_payload_bytes(7, 1).unwrap()).unwrap()
    );
    assert!(first_stats.static_metadata_bytes > 0);
    assert_eq!(
        first_stats.deferred_payload_bytes,
        u64::try_from(first_slots + first_components).unwrap()
    );
    assert_eq!(second_stats.static_metadata_bytes, 0);
    assert_eq!(
        second_stats.deferred_payload_bytes,
        u64::try_from(second_bytes).unwrap()
    );

    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if reports.len() == 3
                && reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
    ));
    assert_eq!(host.retained_deferred_payload_bytes(7, 1).unwrap(), 0);
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmStats(PageHostRealmStats {
            deferred_payload_bytes: 0,
            ..
        })
    ));
    assert_eq!(host.documents[&7].pending_debugger_executions.capacity(), 0);
    assert_eq!(
        host.retained_deferred_payload_bytes(8, 1).unwrap(),
        second_bytes
    );

    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                2,
                vec![javascript_module(0, "export const replacement = 4;")],
            ),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(host.retained_deferred_payload_bytes(7, 2).unwrap() > 0);
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(3, Vec::new()),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(host.retained_deferred_payload_bytes(7, 2).is_err());
    assert_eq!(host.retained_deferred_payload_bytes(7, 3).unwrap(), 0);
    assert_eq!(
        host.retained_deferred_payload_bytes(8, 1).unwrap(),
        second_bytes
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 8,
            document_generation: 1,
        }),
        PageHostReply::RealmClosed { .. }
    ));
    assert!(host.retained_deferred_payload_bytes(8, 1).is_err());
}

#[test]
fn child_pending_javascript_source_graphs_follow_exact_document_lifetimes() {
    let mut host = BlueJsChildHost::default();
    let first = debugger_document(1, vec![javascript_module(0, "export const value = 1;")]);
    let mut second = debugger_document(
        1,
        vec![javascript_module(
            0,
            "export const muchLongerRetainedSourceName = 'a longer source payload';",
        )],
    );
    second.tab_id = 8;
    for document in [first, second] {
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument { document }),
            PageHostReply::Synchronized { reports, .. } if reports.is_empty()
        ));
    }
    let first_bytes = host.retained_deferred_source_graph_bytes(7, 1).unwrap();
    let second_bytes = host.retained_deferred_source_graph_bytes(8, 1).unwrap();
    assert!(first_bytes > 0);
    assert!(second_bytes > first_bytes);
    assert_eq!(
        host.retained_deferred_source_graph_bytes(7, 1).unwrap(),
        first_bytes
    );

    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if matches!(reports.as_slice(), [PageHostScriptReport {
                outcome: PageHostScriptOutcome::Executed,
                ..
            }])
    ));
    assert_eq!(host.retained_deferred_source_graph_bytes(7, 1).unwrap(), 0);
    assert_eq!(
        host.retained_deferred_source_graph_bytes(8, 1).unwrap(),
        second_bytes
    );

    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                2,
                vec![javascript_module(0, "export const replaced = 2;")]
            ),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(host.retained_deferred_source_graph_bytes(7, 2).unwrap() > 0);
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(3, Vec::new()),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(host.retained_deferred_source_graph_bytes(7, 2).is_err());
    assert_eq!(host.retained_deferred_source_graph_bytes(7, 3).unwrap(), 0);
    assert_eq!(
        host.retained_deferred_source_graph_bytes(8, 1).unwrap(),
        second_bytes
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 8,
            document_generation: 1,
        }),
        PageHostReply::RealmClosed { .. }
    ));
    assert!(host.retained_deferred_source_graph_bytes(8, 1).is_err());
}

#[test]
fn child_pending_javascript_asts_follow_exact_document_lifetimes() {
    let mut host = BlueJsChildHost::default();
    let first = debugger_document(1, vec![javascript_module(0, "export const value = 1;")]);
    let mut second = debugger_document(
        1,
        vec![javascript_module(
            0,
            "export function elaborate(value) { const doubled = value * 2; return doubled + 1; }",
        )],
    );
    second.tab_id = 8;
    for document in [first, second] {
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument { document }),
            PageHostReply::Synchronized { reports, .. } if reports.is_empty()
        ));
    }
    let first_bytes = host.retained_deferred_javascript_ast_bytes(7, 1).unwrap();
    let second_bytes = host.retained_deferred_javascript_ast_bytes(8, 1).unwrap();
    assert!(first_bytes > 0);
    assert!(second_bytes > first_bytes);
    assert_eq!(
        host.retained_deferred_bluets_attachment_bytes(7, 1)
            .unwrap(),
        0
    );

    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if matches!(reports.as_slice(), [PageHostScriptReport {
                outcome: PageHostScriptOutcome::Executed,
                ..
            }])
    ));
    assert_eq!(
        host.retained_deferred_javascript_ast_bytes(7, 1).unwrap(),
        0
    );
    assert_eq!(
        host.retained_deferred_javascript_ast_bytes(8, 1).unwrap(),
        second_bytes
    );

    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                2,
                vec![javascript_module(0, "export const replaced = 2;")]
            ),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(host.retained_deferred_javascript_ast_bytes(7, 2).unwrap() > 0);
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(3, Vec::new()),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(host.retained_deferred_javascript_ast_bytes(7, 2).is_err());
    assert_eq!(
        host.retained_deferred_javascript_ast_bytes(7, 3).unwrap(),
        0
    );
    assert_eq!(
        host.retained_deferred_javascript_ast_bytes(8, 1).unwrap(),
        second_bytes
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 8,
            document_generation: 1,
        }),
        PageHostReply::RealmClosed { .. }
    ));
    assert!(host.retained_deferred_javascript_ast_bytes(8, 1).is_err());
}

#[test]
fn child_pending_bluets_attachments_follow_exact_document_lifetimes() {
    fn module(id: &str) -> PageHostScript {
        PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::BlueTs,
            kind: PageHostScriptKind::Module,
            graph: graph(
                id,
                vec![PageHostSource::new(id, "export const value: number = 1;")],
            ),
        }
    }

    let mut host = BlueJsChildHost::default();
    let first = debugger_document(1, vec![module("blueice://page/a.ts")]);
    let mut second = debugger_document(
        1,
        vec![module(
            "blueice://page/a-much-longer-retained-module-identity.ts",
        )],
    );
    second.tab_id = 8;
    for document in [first, second] {
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument { document }),
            PageHostReply::Synchronized { reports, .. } if reports.is_empty()
        ));
    }
    let first_bytes = host
        .retained_deferred_bluets_attachment_bytes(7, 1)
        .unwrap();
    let second_bytes = host
        .retained_deferred_bluets_attachment_bytes(8, 1)
        .unwrap();
    assert!(first_bytes > 0);
    assert!(second_bytes > first_bytes);
    assert_eq!(host.retained_deferred_source_graph_bytes(7, 1).unwrap(), 0);

    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if matches!(reports.as_slice(), [PageHostScriptReport {
                outcome: PageHostScriptOutcome::Executed,
                ..
            }])
    ));
    assert_eq!(
        host.retained_deferred_bluets_attachment_bytes(7, 1)
            .unwrap(),
        0
    );
    assert_eq!(
        host.retained_deferred_bluets_attachment_bytes(8, 1)
            .unwrap(),
        second_bytes
    );

    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(2, vec![module("blueice://page/replaced.ts")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(
        host.retained_deferred_bluets_attachment_bytes(7, 2)
            .unwrap()
            > 0
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(3, Vec::new()),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(host
        .retained_deferred_bluets_attachment_bytes(7, 2)
        .is_err());
    assert_eq!(
        host.retained_deferred_bluets_attachment_bytes(7, 3)
            .unwrap(),
        0
    );
    assert_eq!(
        host.retained_deferred_bluets_attachment_bytes(8, 1)
            .unwrap(),
        second_bytes
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 8,
            document_generation: 1,
        }),
        PageHostReply::RealmClosed { .. }
    ));
    assert!(host
        .retained_deferred_bluets_attachment_bytes(8, 1)
        .is_err());
}

#[test]
fn child_static_bluets_metadata_is_charged_to_its_exact_live_document() {
    let mut host = BlueJsChildHost::default();
    let first = document(
        1,
        vec![blue_ts_classic(
            0,
            "interface ShortShape { value: string; } const shortName: string = 'one'; shortName;",
        )],
    );
    let mut second = document(
        1,
        vec![blue_ts_classic(
            0,
            "interface MuchLongerRetainedContractShape { value: string; } const muchLongerRetainedSymbolName: string = 'two'; muchLongerRetainedSymbolName;",
        )],
    );
    second.tab_id = 8;
    for document in [first, second] {
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument { document }),
            PageHostReply::Synchronized { .. }
        ));
    }
    let first_bytes = host.retained_static_payload_bytes(7, 1).unwrap();
    let second_bytes = host.retained_static_payload_bytes(8, 1).unwrap();
    assert!(first_bytes > 0);
    assert!(second_bytes > first_bytes);
    assert_eq!(
        host.retained_static_payload_bytes(7, 1).unwrap(),
        first_bytes
    );

    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, Vec::new()),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(host.retained_static_payload_bytes(7, 1).is_err());
    assert_eq!(host.retained_static_payload_bytes(7, 2).unwrap(), 0);
    assert_eq!(
        host.retained_static_payload_bytes(8, 1).unwrap(),
        second_bytes
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 8,
            document_generation: 1,
        }),
        PageHostReply::RealmClosed { .. }
    ));
    assert!(host.retained_static_payload_bytes(8, 1).is_err());
}

#[test]
fn child_wide_reservations_reject_new_tabs_but_allow_replacement_and_release() {
    for constrained_resource in ["programs", "bytecode", "heap"] {
        let heap_per_realm = BlueJsHostRuntimeLimits::default().max_heap_bytes_per_realm;
        let mut limits = BlueJsHostRuntimeLimits {
            max_realms: 2,
            max_programs_per_realm: 2,
            max_bytecode_bytes_per_realm: 4096,
            max_heap_bytes_per_realm: heap_per_realm,
            max_reserved_programs: 4,
            max_reserved_bytecode_bytes: 8192,
            max_reserved_heap_bytes: heap_per_realm.saturating_mul(2),
        };
        match constrained_resource {
            "programs" => limits.max_reserved_programs = limits.max_programs_per_realm,
            "bytecode" => {
                limits.max_reserved_bytecode_bytes = limits.max_bytecode_bytes_per_realm;
            }
            "heap" => limits.max_reserved_heap_bytes = limits.max_heap_bytes_per_realm,
            _ => unreachable!(),
        }
        let mut host = BlueJsChildHost::with_runtime_limits(limits).unwrap();
        let first = document(1, vec![classic(0, "globalThis.answer = 42;")]);
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument { document: first }),
            PageHostReply::Synchronized { reports, .. }
                if matches!(reports.as_slice(), [PageHostScriptReport {
                    outcome: PageHostScriptOutcome::Executed,
                    ..
                }])
        ));

        let mut second = document(1, vec![classic(0, "globalThis.other = 7;")]);
        second.tab_id = 8;
        assert!(
            matches!(
                host.handle_request(PageHostRequest::SynchronizeDocument {
                    document: second.clone()
                }),
                PageHostReply::Error {
                    code: PageHostErrorCode::ResourceLimit,
                    ..
                }
            ),
            "{constrained_resource}"
        );
        assert!(
            matches!(
                host.handle_request(PageHostRequest::SynchronizeDocument {
                    document: document(2, vec![classic(0, "globalThis.answer = 43;")])
                }),
                PageHostReply::Synchronized { reports, .. }
                    if matches!(reports.as_slice(), [PageHostScriptReport {
                        outcome: PageHostScriptOutcome::Executed,
                        ..
                    }])
            ),
            "{constrained_resource}"
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 2,
            }),
            PageHostReply::RealmClosed { .. }
        ));
        assert!(
            matches!(
                host.handle_request(PageHostRequest::SynchronizeDocument { document: second }),
                PageHostReply::Synchronized { reports, .. }
                    if matches!(reports.as_slice(), [PageHostScriptReport {
                        outcome: PageHostScriptOutcome::Executed,
                        ..
                    }])
            ),
            "{constrained_resource}"
        );
    }
}

#[test]
fn child_wide_reservation_must_cover_one_full_realm() {
    let mut limits = BlueJsHostRuntimeLimits::default();
    limits.max_reserved_programs = limits.max_programs_per_realm - 1;
    assert!(limits.runtime_config().is_err());
    limits = BlueJsHostRuntimeLimits::default();
    limits.max_reserved_bytecode_bytes = limits.max_bytecode_bytes_per_realm - 1;
    assert!(limits.runtime_config().is_err());
    limits = BlueJsHostRuntimeLimits::default();
    limits.max_reserved_heap_bytes = limits.max_heap_bytes_per_realm - 1;
    assert!(limits.runtime_config().is_err());
}

#[test]
fn child_wide_actual_usage_tracks_live_realms_not_predecessors_or_reservations() {
    let mut host = BlueJsChildHost::default();
    assert_eq!(
        host.handle_request(PageHostRequest::GetChildStats),
        PageHostReply::ChildStats(PageHostChildStats {
            realm_count: 0,
            program_count: 0,
            bytecode_bytes: 0,
            heap_bytes: 0,
        })
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![classic(0, "globalThis.first = 1;")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    let mut second = document(1, vec![classic(0, "globalThis.second = 2;")]);
    second.tab_id = 8;
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument { document: second }),
        PageHostReply::Synchronized { .. }
    ));
    let PageHostReply::RealmStats(first) = host.handle_request(PageHostRequest::GetRealmStats {
        tab_id: 7,
        document_generation: 1,
    }) else {
        panic!("first child realm must remain live");
    };
    let PageHostReply::RealmStats(second) = host.handle_request(PageHostRequest::GetRealmStats {
        tab_id: 8,
        document_generation: 1,
    }) else {
        panic!("second child realm must remain live");
    };
    assert_eq!(
        host.handle_request(PageHostRequest::GetChildStats),
        PageHostReply::ChildStats(PageHostChildStats {
            realm_count: 2,
            program_count: u64::from(first.program_count + second.program_count),
            bytecode_bytes: first.bytecode_bytes + second.bytecode_bytes,
            heap_bytes: first.heap_bytes + second.heap_bytes,
        })
    );
    assert!(first.bytecode_bytes > 0 && second.bytecode_bytes > 0);
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, Vec::new()),
        }),
        PageHostReply::Synchronized { .. }
    ));
    let PageHostReply::RealmStats(replaced) = host.handle_request(PageHostRequest::GetRealmStats {
        tab_id: 7,
        document_generation: 2,
    }) else {
        panic!("replacement child realm must be live");
    };
    assert_eq!(replaced.program_count, 0);
    assert_eq!(
        host.handle_request(PageHostRequest::GetChildStats),
        PageHostReply::ChildStats(PageHostChildStats {
            realm_count: 2,
            program_count: u64::from(second.program_count),
            bytecode_bytes: second.bytecode_bytes,
            heap_bytes: replaced.heap_bytes + second.heap_bytes,
        })
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 8,
            document_generation: 1,
        }),
        PageHostReply::RealmClosed { .. }
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::GetChildStats),
        PageHostReply::ChildStats(PageHostChildStats {
            realm_count: 1,
            program_count: 0,
            bytecode_bytes: 0,
            heap_bytes: replaced.heap_bytes,
        })
    );
}
