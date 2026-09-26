// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn child_wrapper_rechecks_core_liveness_and_does_not_survive_realm_replacement() {
    let socket_path = std::env::temp_dir().join(format!(
        "bi-dom-wrapper-lifetime-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).unwrap();
    let capability = "a".repeat(script::SCRIPT_SESSION_TOKEN_HEX_BYTES);
    let server_capability = capability.clone();
    let server = std::thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        assert_eq!(
            script::read_script_request(&mut peer).unwrap(),
            ScriptRequest::Hello {
                protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                session_token: server_capability,
            }
        );
        script::write_script_reply(
            &mut peer,
            &ScriptReply::HelloAck {
                protocol_version: script::SCRIPT_PROTOCOL_VERSION,
            },
        )
        .unwrap();
        let target = ScriptDocumentTarget {
            tab_id: 7,
            document_generation: 1,
        };
        for (request_id, operation, reply) in [
            (
                1,
                ScriptRequest::GetElementById {
                    target,
                    id: "present".to_string(),
                },
                ScriptReply::Node { node: Some(11) },
            ),
            (
                2,
                ScriptRequest::ValidateNode { target, node: 11 },
                ScriptReply::Ack,
            ),
            (
                3,
                ScriptRequest::ValidateNode { target, node: 11 },
                ScriptReply::Error {
                    message: "unknown node 11".to_string(),
                },
            ),
        ] {
            assert_eq!(
                script::read_script_request(&mut peer).unwrap(),
                ScriptRequest::Call {
                    request_id,
                    request: Box::new(operation),
                }
            );
            script::write_script_reply(
                &mut peer,
                &ScriptReply::CallResult {
                    request_id,
                    target,
                    reply: Box::new(reply),
                },
            )
            .unwrap();
        }
        assert_eq!(
            script::read_script_request(&mut peer).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    });

    let mut host = BlueJsChildHost::default();
    host.configure_script_dom_capability(
        socket_path.clone(),
        capability,
        true,
        false,
        false,
        false,
    )
    .unwrap();
    let outcomes = |reply: PageHostReply| match reply {
        PageHostReply::Synchronized { reports, .. } => reports
            .into_iter()
            .map(|report| report.outcome)
            .collect::<Vec<_>>(),
        other => panic!("expected a synchronized child document, got {other:?}"),
    };
    let first = outcomes(host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![
                classic(
                    0,
                    "globalThis.saved = blueiceTestGetElementById('present'); if (!saved.blueiceTestRequireLive()) throw 'not live';",
                ),
                classic(1, "saved.blueiceTestRequireLive();"),
            ],
        ),
    }));
    assert!(matches!(
        first.as_slice(),
        [
            PageHostScriptOutcome::Executed,
            PageHostScriptOutcome::Rejected { .. }
        ]
    ));
    let second = outcomes(host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            2,
            vec![classic(
                0,
                "if (typeof saved !== 'undefined') throw 'old wrapper';",
            )],
        ),
    }));
    assert_eq!(second, vec![PageHostScriptOutcome::Executed]);
    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 7,
            document_generation: 2,
        }),
        PageHostReply::RealmClosed { .. }
    ));
    let third = outcomes(host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            3,
            vec![classic(
                0,
                "if (typeof saved !== 'undefined') throw 'closed wrapper';",
            )],
        ),
    }));
    assert_eq!(third, vec![PageHostScriptOutcome::Executed]);
    server.join().unwrap();
    std::fs::remove_file(socket_path).unwrap();
}

#[test]
fn mutation_profile_uses_exact_core_creation_and_append_calls() {
    let socket_path =
        std::env::temp_dir().join(format!("bi-dom-mutation-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).unwrap();
    let capability = "b".repeat(script::SCRIPT_SESSION_TOKEN_HEX_BYTES);
    let server_capability = capability.clone();
    let server = std::thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        assert_eq!(
            script::read_script_request(&mut peer).unwrap(),
            ScriptRequest::Hello {
                protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                session_token: server_capability,
            }
        );
        script::write_script_reply(
            &mut peer,
            &ScriptReply::HelloAck {
                protocol_version: script::SCRIPT_PROTOCOL_VERSION,
            },
        )
        .unwrap();
        let target = ScriptDocumentTarget {
            tab_id: 7,
            document_generation: 1,
        };
        for (request_id, operation, reply) in [
            (
                1,
                ScriptRequest::GetElementById {
                    target,
                    id: "target".to_string(),
                },
                ScriptReply::Node { node: Some(11) },
            ),
            (
                2,
                ScriptRequest::CreateElement {
                    target,
                    tag_name: "span".to_string(),
                },
                ScriptReply::NodeCreated { node: 12 },
            ),
            (
                3,
                ScriptRequest::CreateTextNode {
                    target,
                    data: "new text".to_string(),
                },
                ScriptReply::NodeCreated { node: 13 },
            ),
            (
                4,
                ScriptRequest::AppendChild {
                    target,
                    parent: 12,
                    child: 13,
                },
                ScriptReply::Ack,
            ),
            (
                5,
                ScriptRequest::AppendChild {
                    target,
                    parent: 11,
                    child: 12,
                },
                ScriptReply::Ack,
            ),
            (
                6,
                ScriptRequest::GetTextContent { target, node: 11 },
                ScriptReply::Text {
                    value: "new text".to_string(),
                },
            ),
        ] {
            assert_eq!(
                script::read_script_request(&mut peer).unwrap(),
                ScriptRequest::Call {
                    request_id,
                    request: Box::new(operation),
                }
            );
            script::write_script_reply(
                &mut peer,
                &ScriptReply::CallResult {
                    request_id,
                    target,
                    reply: Box::new(reply),
                },
            )
            .unwrap();
        }
        assert_eq!(
            script::read_script_request(&mut peer).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    });

    let mut host = BlueJsChildHost::default();
    host.configure_script_dom_capability(
        socket_path.clone(),
        capability,
        false,
        false,
        true,
        false,
    )
    .unwrap();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![classic(
                0,
                "let parent = document.getElementById('target'); \
                 let child = document.createElement('span'); \
                 let text = document.createTextNode('new text'); \
                 if (child.appendChild(text) !== text) throw 'text identity'; \
                 if (parent.appendChild(child) !== child) throw 'child identity'; \
                 if (parent.textContent !== 'new text') throw 'mutation'; \
                 try { parent.appendChild({}); throw 'forged accepted'; } \
                 catch (error) { if (!(error instanceof TypeError)) throw error; }",
            )],
        ),
    });
    let PageHostReply::Synchronized { reports, .. } = reply else {
        panic!("expected synchronized document");
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    drop(host);
    server.join().unwrap();
    std::fs::remove_file(socket_path).unwrap();
}

#[test]
fn event_profile_dispatches_only_live_unremoved_exact_document_listeners() {
    let socket_path =
        std::env::temp_dir().join(format!("bi-dom-event-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).unwrap();
    let capability = "c".repeat(script::SCRIPT_SESSION_TOKEN_HEX_BYTES);
    let server_capability = capability.clone();
    let server = std::thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        assert_eq!(
            script::read_script_request(&mut peer).unwrap(),
            ScriptRequest::Hello {
                protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                session_token: server_capability,
            }
        );
        script::write_script_reply(
            &mut peer,
            &ScriptReply::HelloAck {
                protocol_version: script::SCRIPT_PROTOCOL_VERSION,
            },
        )
        .unwrap();
        let target = ScriptDocumentTarget {
            tab_id: 7,
            document_generation: 1,
        };
        for (request_id, id, node) in [(1, "live", 11), (2, "removed", 12)] {
            assert_eq!(
                script::read_script_request(&mut peer).unwrap(),
                ScriptRequest::Call {
                    request_id,
                    request: Box::new(ScriptRequest::GetElementById {
                        target,
                        id: id.to_string(),
                    }),
                }
            );
            script::write_script_reply(
                &mut peer,
                &ScriptReply::CallResult {
                    request_id,
                    target,
                    reply: Box::new(ScriptReply::Node { node: Some(node) }),
                },
            )
            .unwrap();
        }
        assert_eq!(
            script::read_script_request(&mut peer).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    });

    let mut host = BlueJsChildHost::default();
    host.configure_script_dom_capability(
        socket_path.clone(),
        capability,
        false,
        false,
        false,
        true,
    )
    .unwrap();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![classic(
                0,
                "let live = document.getElementById('live'); \
                 let removed = document.getElementById('removed'); \
                 function cancel(event) { \
                   event.preventDefault(); \
                   Promise.resolve().then(function() { \
                     if (globalThis.ranLater) live.removeEventListener('click', cancel); \
                   }); \
                   throw 'listener failure'; \
                 } \
                 live.addEventListener('click', cancel); \
                 live.addEventListener('click', function() { globalThis.ranLater = true; }); \
                 removed.addEventListener('click', cancel); \
                 removed.removeEventListener('click', cancel);",
            )],
        ),
    });
    let PageHostReply::Synchronized { reports, .. } = reply else {
        panic!("expected synchronized event document");
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    host.documents
        .get_mut(&7)
        .unwrap()
        .pending_click_tasks
        .push_back(PendingClickTask {
            generation: 1,
            node_id: 11,
        });
    assert_eq!(
        host.handle_request(PageHostRequest::DispatchClick {
            tab_id: 7,
            document_generation: 1,
            node_id: 11,
        }),
        resource_limit()
    );
    host.documents
        .get_mut(&7)
        .unwrap()
        .pending_click_tasks
        .pop_front();
    assert_eq!(
        host.handle_request(PageHostRequest::DispatchClick {
            tab_id: 7,
            document_generation: 1,
            node_id: 11,
        }),
        PageHostReply::ClickDispatched {
            tab_id: 7,
            document_generation: 1,
            default_prevented: true,
        }
    );
    // The first callback threw after canceling, but the next callback
    // still ran. Its state is visible to the microtask checkpoint, which
    // removes the canceler before core can apply a default action.
    assert_eq!(
        host.handle_request(PageHostRequest::DispatchClick {
            tab_id: 7,
            document_generation: 1,
            node_id: 11,
        }),
        PageHostReply::ClickDispatched {
            tab_id: 7,
            document_generation: 1,
            default_prevented: false,
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::DispatchClick {
            tab_id: 7,
            document_generation: 1,
            node_id: 12,
        }),
        PageHostReply::ClickDispatched {
            tab_id: 7,
            document_generation: 1,
            default_prevented: false,
        }
    );
    host.documents
        .get_mut(&7)
        .unwrap()
        .pending_click_tasks
        .push_back(PendingClickTask {
            generation: 1,
            node_id: 11,
        });
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(host
        .documents
        .get(&7)
        .unwrap()
        .pending_click_tasks
        .is_empty());
    assert_eq!(
        host.handle_request(PageHostRequest::DispatchClick {
            tab_id: 7,
            document_generation: 1,
            node_id: 11,
        }),
        stale_document()
    );
    assert_eq!(
        host.handle_request(PageHostRequest::DispatchClick {
            tab_id: 7,
            document_generation: 2,
            node_id: 11,
        }),
        PageHostReply::ClickDispatched {
            tab_id: 7,
            document_generation: 2,
            default_prevented: false,
        }
    );
    drop(host);
    server.join().unwrap();
    std::fs::remove_file(socket_path).unwrap();
}

#[test]
fn child_revokes_dom_streams_on_navigation_and_close_and_rejects_old_replies() {
    let socket_path =
        std::env::temp_dir().join(format!("bi-dom-revoke-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let capability = "a".repeat(script::SCRIPT_SESSION_TOKEN_HEX_BYTES);
    let server_capability = capability.clone();
    let server = std::thread::spawn(move || {
        // Both successors reuse call ID 1 on a fresh stream. First give
        // each an old-document result, then a valid result after the
        // child fails closed and reconnects.
        for (request_generation, reply_generation) in [(1, 1), (2, 1), (2, 2), (3, 2), (3, 3)] {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut peer = loop {
                match listener.accept() {
                    Ok((peer, _)) => break peer,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "child did not open DOM stream");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("failed to accept child DOM stream: {error}"),
                }
            };
            peer.set_nonblocking(false).unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            assert_eq!(
                script::read_script_request(&mut peer).unwrap(),
                ScriptRequest::Hello {
                    protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                    session_token: server_capability.clone(),
                }
            );
            script::write_script_reply(
                &mut peer,
                &ScriptReply::HelloAck {
                    protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                },
            )
            .unwrap();
            assert_eq!(
                script::read_script_request(&mut peer).unwrap(),
                ScriptRequest::Call {
                    request_id: 1,
                    request: Box::new(ScriptRequest::GetElementById {
                        target: ScriptDocumentTarget {
                            tab_id: 7,
                            document_generation: request_generation,
                        },
                        id: "present".to_string(),
                    }),
                }
            );
            script::write_script_reply(
                &mut peer,
                &ScriptReply::CallResult {
                    request_id: 1,
                    target: ScriptDocumentTarget {
                        tab_id: 7,
                        document_generation: reply_generation,
                    },
                    reply: Box::new(ScriptReply::Node { node: Some(11) }),
                },
            )
            .unwrap();
            assert_eq!(
                script::read_script_request(&mut peer).unwrap_err().kind(),
                io::ErrorKind::UnexpectedEof,
                "navigation, tab close, or a stale reply must close the old DOM stream"
            );
        }
    });

    let mut host = BlueJsChildHost::default();
    host.configure_script_dom_capability(
        socket_path.clone(),
        capability,
        true,
        false,
        false,
        false,
    )
    .unwrap();
    let script = |ordinal| {
        classic(
            ordinal,
            "if (!blueiceTestHasElementById('present')) throw 'missing';",
        )
    };
    let outcomes = |reply: PageHostReply| match reply {
        PageHostReply::Synchronized { reports, .. } => reports
            .into_iter()
            .map(|report| report.outcome)
            .collect::<Vec<_>>(),
        other => panic!("expected a synchronized child document, got {other:?}"),
    };
    assert_eq!(
        outcomes(host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![script(0)]),
        })),
        vec![PageHostScriptOutcome::Executed]
    );
    let second_outcomes = outcomes(host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(2, vec![script(0), script(1)]),
    }));
    assert!(matches!(
        second_outcomes.as_slice(),
        [
            PageHostScriptOutcome::Rejected { .. },
            PageHostScriptOutcome::Executed
        ]
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 7,
            document_generation: 2,
        }),
        PageHostReply::RealmClosed {
            tab_id: 7,
            document_generation: 2,
        }
    );
    let third_outcomes = outcomes(host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(3, vec![script(0), script(1)]),
    }));
    assert!(matches!(
        third_outcomes.as_slice(),
        [
            PageHostScriptOutcome::Rejected { .. },
            PageHostScriptOutcome::Executed
        ]
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 7,
            document_generation: 3,
        }),
        PageHostReply::RealmClosed {
            tab_id: 7,
            document_generation: 3,
        }
    );
    server.join().unwrap();
    std::fs::remove_file(socket_path).unwrap();
}

#[test]
fn child_rejects_unbound_or_mismatched_dom_replies_and_closes_the_stream() {
    let target = ScriptDocumentTarget {
        tab_id: 7,
        document_generation: 9,
    };
    for reply in [
        ScriptReply::CallResult {
            request_id: 2,
            target,
            reply: Box::new(ScriptReply::Node { node: Some(11) }),
        },
        ScriptReply::CallResult {
            request_id: 1,
            target: ScriptDocumentTarget {
                document_generation: 10,
                ..target
            },
            reply: Box::new(ScriptReply::Node { node: Some(11) }),
        },
        ScriptReply::Node { node: Some(11) },
        ScriptReply::CallResult {
            request_id: 1,
            target,
            reply: Box::new(ScriptReply::CallResult {
                request_id: 1,
                target,
                reply: Box::new(ScriptReply::Node { node: Some(11) }),
            }),
        },
    ] {
        let (client_stream, mut fake_core) = UnixStream::pair().unwrap();
        let mut client = ScriptDomClient {
            capability: ScriptDomCapability {
                socket_path: PathBuf::new(),
                session_token: String::new(),
                enable_lookup_probe: true,
                enable_dom_text_profile: false,
                enable_dom_mutation_profile: false,
                enable_dom_event_profile: false,
            },
            stream: Some(client_stream),
            next_call_id: 1,
        };
        let fake = std::thread::spawn(move || {
            assert_eq!(
                script::read_script_request(&mut fake_core).unwrap(),
                ScriptRequest::Call {
                    request_id: 1,
                    request: Box::new(ScriptRequest::GetElementById {
                        target,
                        id: "target".to_string(),
                    }),
                }
            );
            script::write_script_reply(&mut fake_core, &reply).unwrap();
        });
        assert_eq!(
            client
                .has_element_by_id(target, "target".to_string())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert!(client.stream.is_none());
        fake.join().unwrap();
    }
}

fn graph(entry: &str, modules: Vec<PageHostSource>) -> PageHostModuleGraph {
    PageHostModuleGraph {
        entry: entry.to_string(),
        modules,
        resolutions: Vec::new(),
        resolver_fingerprint: "core-page-loader-v1".to_string(),
    }
}

fn document(generation: u64, scripts: Vec<PageHostScript>) -> PageHostDocument {
    document_with_snapshot(
        generation,
        "test document snapshot".to_string(),
        "https://example.test".to_string(),
        scripts,
    )
}

fn document_with_snapshot(
    generation: u64,
    document_text: String,
    document_origin: String,
    scripts: Vec<PageHostScript>,
) -> PageHostDocument {
    PageHostDocument {
        tab_id: 7,
        document_generation: generation,
        snapshot: PageHostDocumentSnapshot {
            document_text,
            document_origin,
        },
        debugger_execution_control: false,
        scripts,
    }
}

fn debugger_document(generation: u64, scripts: Vec<PageHostScript>) -> PageHostDocument {
    let mut document = document(generation, scripts);
    document.debugger_execution_control = true;
    document
}

#[test]
fn private_child_classic_scheduler_steps_only_its_paused_nested_invocation() {
    let mut host = BlueJsChildHost::default();
    let source =
        "var calls = 0; function inner() { calls++; return 4; } globalThis.answer = inner() + 1;";
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![classic(0, source)]),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected one private program: {reply:?}"),
    };
    let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .expect("the inner function has a first verified instruction"),
        reply => panic!("expected child safe points: {reply:?}"),
    };
    host.arm_debugger_nested_target(7, 1, target).unwrap();
    assert!(host.arm_debugger_nested_target(7, 1, target).is_err());
    let root = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 0)
            .unwrap(),
        reply => panic!("expected root safe points: {reply:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: root,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let frame = match host.documents[&7].debugger_execution_states[&program] {
        ChildDebuggerExecutionStatus::NestedPaused { frame, safe_point } => {
            assert_eq!(safe_point, target);
            frame
        }
        status => panic!("the actual inner invocation must pause: {status:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::NestedPaused {
                frame: wire_frame,
                safe_point,
            },
            ..
        } if wire_frame == child_debugger_frame(7, 1, program, frame)
            && safe_point == target
    ));
    assert!(host
        .request_debugger_nested_advance(7, 2, frame, false)
        .is_err());
    let mut returned = false;
    for _ in 0..96 {
        host.request_debugger_nested_advance(7, 1, frame, false)
            .unwrap();
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        match host.documents[&7].debugger_execution_states[&program] {
            ChildDebuggerExecutionStatus::NestedPaused {
                frame: same_frame,
                safe_point,
            } => {
                assert_eq!(same_frame, frame);
                host.exact_debugger_safe_point(7, 1, safe_point).unwrap();
            }
            ChildDebuggerExecutionStatus::Paused(root) => {
                assert_eq!(root.code_unit_ordinal, 0);
                host.exact_debugger_safe_point(7, 1, root).unwrap();
                returned = true;
                break;
            }
            status => panic!("unexpected nested step: {status:?}"),
        }
    }
    assert!(returned);
    assert!(host
        .request_debugger_nested_advance(7, 1, frame, false)
        .is_err());
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if reports.len() == 1 && reports[0].outcome == PageHostScriptOutcome::Executed
    ));
}

#[test]
fn child_nested_wire_requires_the_exact_live_frame_and_reports_step_state() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![classic(0, "function inner() { return 4; } inner();")],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected a child program: {reply:?}"),
    };
    let point = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected child points: {reply:?}"),
    };
    assert_eq!(
        host.handle_request(PageHostRequest::ArmDebuggerNestedSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: point,
        }),
        PageHostReply::DebuggerNestedSafePointBreakpointArmed {
            tab_id: 7,
            document_generation: 1,
            safe_point: point,
        }
    );
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
        program,
    }) {
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::NestedPaused { frame, safe_point },
            ..
        } => {
            assert_eq!(safe_point, point);
            frame
        }
        reply => panic!("expected active frame state: {reply:?}"),
    };
    assert!(frame.is_well_formed());
    let snapshot = |frame: Option<PageHostDebuggerFrame>, max_frames, max_scope_entries| {
        PageHostRequest::GetDebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 1,
            program,
            frame,
            max_frames,
            max_scope_entries,
        }
    };
    assert!(matches!(
        host.handle_request(snapshot(Some(frame), 1, 1)),
        PageHostReply::DebuggerStackSnapshot {
            frame: Some(reply_frame),
            snapshot: PageHostDebuggerStackSnapshot { frames, stack_truncated: true },
            ..
        } if reply_frame == frame && frames.len() == 1 && frames[0].code_unit_ordinal == 1
    ));
    assert!(matches!(
        host.handle_request(snapshot(Some(frame), 2, 256)),
        PageHostReply::DebuggerStackSnapshot {
            snapshot: PageHostDebuggerStackSnapshot { frames, stack_truncated: false },
            ..
        } if frames.len() == 2 && frames[0].code_unit_ordinal == 1 && frames[1].code_unit_ordinal == 0
    ));
    assert!(matches!(
        host.handle_request(snapshot(None, 2, 256)),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(snapshot(Some(frame), 0, 1)),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(snapshot(Some(frame), 2, 257)),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 1,
            program: PageHostDebuggerProgram {
                program_generation: program.program_generation + 1,
                ..program
            },
            frame: Some(frame),
            max_frames: 2,
            max_scope_entries: 256,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerStackSnapshot {
            tab_id: 7,
            document_generation: 2,
            program,
            frame: Some(frame),
            max_frames: 2,
            max_scope_entries: 256,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    for wrong in [
        PageHostDebuggerFrame {
            invocation_serial: frame.invocation_serial + 1,
            ..frame
        },
        PageHostDebuggerFrame {
            code_unit_ordinal: frame.code_unit_ordinal + 1,
            ..frame
        },
        PageHostDebuggerFrame {
            document_generation: frame.document_generation + 1,
            ..frame
        },
        PageHostDebuggerFrame { tab_id: 8, ..frame },
    ] {
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerNestedInstruction { frame: wrong }),
            PageHostReply::Error { .. }
        ));
        assert!(matches!(
            host.handle_request(snapshot(Some(wrong), 2, 256)),
            PageHostReply::Error { .. }
        ));
    }
    assert_eq!(
        host.handle_request(PageHostRequest::StepDebuggerNestedInstruction { frame }),
        PageHostReply::DebuggerNestedStepRequested { frame }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::NestedStepping {
                frame: same_frame,
            },
            ..
        } if same_frame == frame
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerNestedInstruction { frame }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(snapshot(Some(frame), 2, 256)),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::NestedPaused { frame: same, .. },
            ..
        } if same == frame
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerNestedExecution {
            frame: PageHostDebuggerFrame {
                invocation_serial: frame.invocation_serial + 1,
                ..frame
            },
        }),
        PageHostReply::Error { .. }
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::ResumeDebuggerNestedExecution { frame }),
        PageHostReply::DebuggerNestedResumeRequested { frame }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::NestedResuming { frame: same },
            ..
        } if same == frame
    ));
    assert!(matches!(
        host.handle_request(snapshot(Some(frame), 2, 256)),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::Paused { safe_point },
            ..
        } if safe_point.code_unit_ordinal == 0
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerNestedExecution { frame }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(snapshot(None, 2, 256)),
        PageHostReply::DebuggerStackSnapshot { frame: None, snapshot: PageHostDebuggerStackSnapshot { frames, stack_truncated: false }, .. }
            if frames.len() == 1 && frames[0].code_unit_ordinal == 0
    ));
}

#[test]
fn private_child_bluets_classic_uses_the_same_nested_frame_scheduler() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![blue_ts_classic(
                    0,
                    "function inner(): number { return 4; } globalThis.answer = inner() + 1;",
                )],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected one BlueTS program: {reply:?}"),
    };
    let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected BlueTS child points: {reply:?}"),
    };
    host.arm_debugger_nested_target(7, 1, target).unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let frame = match host.documents[&7].debugger_execution_states[&program] {
        ChildDebuggerExecutionStatus::NestedPaused { frame, .. } => frame,
        status => panic!("BlueTS child must pause: {status:?}"),
    };
    host.request_debugger_nested_advance(7, 1, frame, false)
        .unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert!(matches!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::NestedPaused { frame: same, .. } if same == frame
    ));
}

#[test]
fn private_child_classic_rejects_an_unsupported_deeper_nested_target() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![classic(
                    0,
                    "function inner() { return 4; } function outer() { return inner(); } outer();",
                )],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected one private program: {reply:?}"),
    };
    let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected child safe points: {reply:?}"),
    };
    host.arm_debugger_nested_target(7, 1, target).unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if matches!(reports.as_slice(), [PageHostScriptReport {
                outcome: PageHostScriptOutcome::Rejected { .. }, ..
            }])
    ));
    assert_eq!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::Completed
    );
}

#[test]
fn private_child_bluets_module_nested_frame_rejoins_its_linked_entry_once() {
    let entry = "blueice://page/nested-entry.ts";
    let dependency = "blueice://page/nested-dependency.ts";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { value } from './nested-dependency.ts'; function inner(): number { return value + 1; } globalThis.moduleAnswer = inner();",
                ),
                PageHostSource::new(
                    dependency,
                    "globalThis.depRuns = (globalThis.depRuns || 0) + 1; export const value: number = 40;",
                ),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./nested-dependency.ts".to_string(),
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
    let program = pending.program.unwrap();
    let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected module child points: {reply:?}"),
    };
    host.arm_debugger_nested_target(7, 1, target).unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let frame = match host.documents[&7].debugger_execution_states[&program] {
        ChildDebuggerExecutionStatus::NestedPaused { frame, safe_point } => {
            assert_eq!(safe_point, target);
            frame
        }
        status => panic!("the module entry child must pause: {status:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::Error { .. }
    ));
    host.request_debugger_nested_advance(7, 1, frame, false)
        .unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert!(matches!(
        host.documents[&7].debugger_execution_states[&program],
        ChildDebuggerExecutionStatus::NestedPaused { frame: same, .. } if same == frame
    ));
    let child_frame = child_debugger_frame(7, 1, program, frame);
    assert_eq!(
        host.handle_request(PageHostRequest::ResumeDebuggerNestedExecution { frame: child_frame }),
        PageHostReply::DebuggerNestedResumeRequested { frame: child_frame }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let ChildDebuggerExecutionStatus::Paused(root) =
        host.documents[&7].debugger_execution_states[&program]
    else {
        panic!("module child resume must park its original root");
    };
    assert_eq!(root.code_unit_ordinal, 0);
    host.exact_debugger_safe_point(7, 1, root).unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerNestedExecution { frame: child_frame }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if reports.len() == 1 && reports[0].outcome == PageHostScriptOutcome::Executed
    ));
    let origin = BlueJsPageOrigin::new("https://example.test").unwrap();
    let probe = host
        .runtime
        .install_program(
            7,
            &origin,
            BlueJsSourceIdentity::new("page:///nested-probe.js", "sha256:nested-probe").unwrap(),
            &BlueJsProgramV1::Script(
                parse("globalThis.depRuns + globalThis.moduleAnswer").unwrap(),
            ),
        )
        .unwrap();
    assert_eq!(
        host.runtime.execute_program(7, probe),
        Ok(Value::Number(42.0))
    );
}

fn classic(ordinal: u32, source: &str) -> PageHostScript {
    let id = format!("blueice://page/inline-{ordinal}.js");
    PageHostScript {
        ordinal,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Classic,
        graph: graph(&id, vec![PageHostSource::new(id.clone(), source)]),
    }
}

fn blue_ts_classic(ordinal: u32, source: &str) -> PageHostScript {
    let id = format!("blueice://page/inline-{ordinal}.ts");
    PageHostScript {
        ordinal,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Classic,
        graph: graph(&id, vec![PageHostSource::new(id.clone(), source)]),
    }
}

#[test]
fn private_child_snapshots_only_terminal_uncaught_bluets_locations() {
    let mut host = BlueJsChildHost::default();
    let synchronized = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: debugger_document(
            1,
            vec![
                blue_ts_classic(0, "function fail(): number { throw 7; } fail();"),
                classic(1, "globalThis.successor = 1;"),
                blue_ts_classic(
                    2,
                    "function unused(): number { throw 8; } const handled: number = 1;",
                ),
            ],
        ),
    });
    assert!(
        matches!(&synchronized, PageHostReply::Synchronized { reports, .. } if reports.is_empty()),
        "unexpected synchronization reply: {synchronized:?}"
    );
    let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected three private programs, got {reply:?}"),
    };
    assert_eq!(programs.len(), 3);
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program: programs[0],
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected private BlueTS attachment, got {reply:?}"),
    };
    let request = PageHostRequest::DescribeDebuggerBlueTsExceptionLocation {
        tab_id: 7,
        document_generation: 1,
        program: programs[0],
        metadata,
    };
    assert!(matches!(
        host.handle_request(request.clone()),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.len() == 3
    ));
    let first = host.documents[&7].debugger_programs[&programs[0].program_handle]
        .exception_location
        .expect("first uncaught BlueTS throw must survive successor execution");
    assert_eq!(first.safe_point.program, programs[0]);
    assert_eq!(first.safe_point.code_unit_ordinal, 1);
    assert!(first
        .span
        .coordinates
        .is_well_formed_for_range(first.span.start_byte, first.span.end_byte));
    assert_eq!(first.span.start_byte, 0);
    assert!(first.span.end_byte > first.span.start_byte);
    assert_eq!(
        host.handle_request(request.clone()),
        PageHostReply::DebuggerBlueTsExceptionLocation {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            metadata,
            location: PageHostDebuggerBlueTsExceptionLocation {
                safe_point: first.safe_point,
                span: first.span,
            },
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerBlueTsExceptionLocation {
            tab_id: 7,
            document_generation: 1,
            program: programs[1],
            metadata,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerBlueTsExceptionLocation {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            metadata: PageHostDebuggerMetadataHandle {
                metadata_generation: metadata.metadata_generation + 1,
                ..metadata
            },
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    for program in &programs[1..] {
        assert!(
            host.documents[&7].debugger_programs[&program.program_handle]
                .exception_location
                .is_none()
        );
    }
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(2, vec![blue_ts_classic(0, "const next = 1;")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(!host.documents[&7]
        .debugger_programs
        .contains_key(&programs[0].program_handle));
    assert!(matches!(
        host.handle_request(request),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn private_child_maps_exact_nested_module_throw_sites_without_guessing() {
    for source in [
        "function inner(): number { throw 9; } export const result: number = inner();",
        "function inner(): number { throw 'x'; } export const result: number = inner();",
    ] {
        let entry = "blueice://page/throwing-module.ts";
        let module = PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::BlueTs,
            kind: PageHostScriptKind::Module,
            graph: graph(entry, vec![PageHostSource::new(entry, source)]),
        };
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: debugger_document(1, vec![module]),
            }),
            PageHostReply::Synchronized { reports, .. } if reports.is_empty()
        ));
        let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
            reply => panic!("expected private module program, got {reply:?}"),
        };
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.len() == 1
        ));
        let location = host.documents[&7].debugger_programs[&program.program_handle]
            .exception_location
            .expect("exact module throw must retain a private location");
        assert_eq!(location.safe_point.program, program);
        assert_eq!(location.safe_point.code_unit_ordinal, 1);
        assert!(location.span.start_byte < location.span.end_byte);
        assert!(location
            .span
            .coordinates
            .is_well_formed_for_range(location.span.start_byte, location.span.end_byte));
        let runtime_handle =
            host.documents[&7].debugger_programs[&program.program_handle].runtime_handle;
        let retained = host
            .debug_registry
            .get(host.runtime.program_registry(), runtime_handle)
            .unwrap();
        assert!(exact_bluets_span_for_site(
            retained,
            PageHostDebuggerSafePoint {
                bytecode_offset: u32::MAX,
                ..location.safe_point
            }
        )
        .is_none());
    }
}

#[test]
fn private_child_attributes_dependency_throw_to_its_own_bluets_program() {
    let entry = "blueice://page/entry-throw.ts";
    let dependency = "blueice://page/dependency-throw.ts";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { answer } from './dependency-throw.ts'; export const result: number = answer;",
                ),
                PageHostSource::new(
                    dependency,
                    "function fail(): number { throw 9; } export const answer: number = fail();",
                ),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./dependency-throw.ts".to_string(),
        canonical_target: dependency.to_string(),
    });
    let mut host = BlueJsChildHost::default();
    let synchronized = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: debugger_document(1, vec![module]),
    });
    assert!(
        matches!(&synchronized, PageHostReply::Synchronized { reports, .. } if reports.is_empty()),
        "unexpected synchronization reply: {synchronized:?}"
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.len() == 1
    ));
    let locations = host.documents[&7]
        .debugger_programs
        .iter()
        .filter_map(|(handle, record)| record.exception_location.map(|location| (handle, location)))
        .collect::<Vec<_>>();
    assert_eq!(locations.len(), 1);
    let (handle, location) = locations[0];
    assert_eq!(location.safe_point.program.program_handle, *handle);
    assert_eq!(location.safe_point.code_unit_ordinal, 1);
    let runtime_handle = host.documents[&7].debugger_programs[handle].runtime_handle;
    assert_eq!(
        host.runtime
            .program_registry()
            .get(runtime_handle)
            .unwrap()
            .source()
            .canonical_module_id(),
        dependency
    );
}

fn paused_value_targets(
    host: &mut BlueJsChildHost,
    program: PageHostDebuggerProgram,
    frame: Option<PageHostDebuggerFrame>,
    frame_index: usize,
) -> Vec<PageHostDebuggerValueTarget> {
    let snapshot = match host.handle_request(PageHostRequest::GetDebuggerStackSnapshot {
        tab_id: 7,
        document_generation: 1,
        program,
        frame,
        max_frames: 2,
        max_scope_entries: 256,
    }) {
        PageHostReply::DebuggerStackSnapshot { snapshot, .. } => snapshot,
        reply => panic!("expected active stack: {reply:?}"),
    };
    let selected = &snapshot.frames[frame_index];
    selected
        .scope_entries
        .iter()
        .copied()
        .map(|scope_entry| PageHostDebuggerValueTarget {
            tab_id: 7,
            document_generation: 1,
            program,
            frame,
            frame_index: frame_index as u32,
            safe_point: PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: selected.code_unit_ordinal,
                bytecode_offset: selected.bytecode_offset,
            },
            scope_entry,
        })
        .collect()
}

#[test]
fn private_static_scope_wire_rejects_missing_realm() {
    let mut host = BlueJsChildHost::default();
    let program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let target = page_host::PageHostDebuggerStaticScopeTarget::Ordinary {
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        target: PageHostDebuggerValueTarget {
            tab_id: 7,
            document_generation: 3,
            program,
            frame: None,
            frame_index: 0,
            safe_point: PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
            scope_entry: PageHostDebuggerScopeEntry {
                slot_ordinal: 2,
                scope_depth: 0,
            },
        },
    };
    assert!(target.is_well_formed());
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerStaticScopeRelation {
            target: Box::new(target),
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::UnknownRealm,
            ..
        }
    ));
}

#[test]
fn private_static_scope_ordinary_root_relates_only_one_live_compiler_slot() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![blue_ts_classic(0, "let answer: number = 41;")],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = host.documents[&7]
        .pending_debugger_executions
        .front()
        .unwrap()
        .program
        .unwrap();
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected metadata attachment: {reply:?}"),
    };
    let slot = host
        .live_bluets_root_symbol_slots(7, 1, program, metadata)
        .unwrap()[0];
    let halt = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .filter(|point| point.code_unit_ordinal == 0)
            .max_by_key(|point| point.bytecode_offset)
            .unwrap(),
        reply => panic!("expected root safe points: {reply:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: halt,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let target = paused_value_targets(&mut host, program, None, 0)
        .into_iter()
        .find(|target| target.scope_entry.slot_ordinal == slot.slot_ordinal)
        .expect("the compiled root declaration occupies an active slot");
    let static_target = page_host::PageHostDebuggerStaticScopeTarget::Ordinary { metadata, target };
    let request = |target| PageHostRequest::DescribeDebuggerStaticScopeRelation {
        target: Box::new(target),
    };
    assert_eq!(
        host.handle_request(request(static_target.clone())),
        PageHostReply::DebuggerStaticScopeRelation(Box::new(
            page_host::PageHostDebuggerStaticScopeRelation {
                target: static_target.clone(),
                symbol_type: PageHostDebuggerBlueTsMetadataSymbolType {
                    symbol_id: slot.symbol_id.0,
                    type_id: slot.type_id.0,
                },
            }
        ))
    );
    for forged in [
        page_host::PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata: PageHostDebuggerMetadataHandle {
                metadata_generation: metadata.metadata_generation + 1,
                ..metadata
            },
            target,
        },
        page_host::PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: PageHostDebuggerValueTarget {
                scope_entry: PageHostDebuggerScopeEntry {
                    slot_ordinal: target.scope_entry.slot_ordinal + 1,
                    ..target.scope_entry
                },
                ..target
            },
        },
        page_host::PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: PageHostDebuggerValueTarget {
                scope_entry: PageHostDebuggerScopeEntry {
                    scope_depth: target.scope_entry.scope_depth + 1,
                    ..target.scope_entry
                },
                ..target
            },
        },
        page_host::PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: PageHostDebuggerValueTarget {
                safe_point: PageHostDebuggerSafePoint {
                    bytecode_offset: target.safe_point.bytecode_offset + 1,
                    ..target.safe_point
                },
                ..target
            },
        },
    ] {
        assert!(matches!(
            host.handle_request(request(forged)),
            PageHostReply::Error { .. }
        ));
    }
    assert!(matches!(
        host.handle_request(request(
            page_host::PageHostDebuggerStaticScopeTarget::Ordinary {
                metadata,
                target: PageHostDebuggerValueTarget {
                    document_generation: 2,
                    ..target
                },
            }
        )),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(2, vec![blue_ts_classic(0, "let next: number = 7;")],),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(request(static_target)),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn private_static_scope_ordinary_nested_pause_selects_only_parent_root() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![blue_ts_classic(
                    0,
                    "let seed: number = 7; function inner(value: number): number { let local: number = value; return local; } inner(seed);",
                )],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = host.documents[&7]
        .pending_debugger_executions
        .front()
        .unwrap()
        .program
        .unwrap();
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected metadata attachment: {reply:?}"),
    };
    let point = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected nested safe point: {reply:?}"),
    };
    host.arm_debugger_nested_target(7, 1, point).unwrap();
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
        program,
    }) {
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::NestedPaused { frame, .. },
            ..
        } => frame,
        reply => panic!("expected nested pause: {reply:?}"),
    };
    for _ in 0..64 {
        if !paused_value_targets(&mut host, program, Some(frame), 0).is_empty() {
            break;
        }
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerNestedInstruction { frame }),
            PageHostReply::DebuggerNestedStepRequested { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
    }
    let slots = host
        .live_bluets_root_symbol_slots(7, 1, program, metadata)
        .unwrap()
        .to_vec();
    let (slot, parent) = paused_value_targets(&mut host, program, Some(frame), 1)
        .into_iter()
        .find_map(|target| {
            slots
                .iter()
                .find(|slot| slot.slot_ordinal == target.scope_entry.slot_ordinal)
                .map(|slot| (*slot, target))
        })
        .expect("one compiled parent-root declaration remains active");
    let parent_target = PageHostDebuggerStaticScopeTarget::Ordinary {
        metadata,
        target: parent,
    };
    assert_eq!(
        host.handle_request(PageHostRequest::DescribeDebuggerStaticScopeRelation {
            target: Box::new(parent_target.clone()),
        }),
        PageHostReply::DebuggerStaticScopeRelation(Box::new(PageHostDebuggerStaticScopeRelation {
            target: parent_target,
            symbol_type: PageHostDebuggerBlueTsMetadataSymbolType {
                symbol_id: slot.symbol_id.0,
                type_id: slot.type_id.0,
            },
        }))
    );
    let child = paused_value_targets(&mut host, program, Some(frame), 0)
        .into_iter()
        .next()
        .expect("the child has an active local or capture slot");
    for denied in [
        PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: child,
        },
        PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: PageHostDebuggerValueTarget {
                frame_index: 0,
                safe_point: PageHostDebuggerSafePoint {
                    code_unit_ordinal: 1,
                    ..parent.safe_point
                },
                ..parent
            },
        },
        PageHostDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: PageHostDebuggerValueTarget {
                frame: Some(PageHostDebuggerFrame {
                    invocation_serial: frame.invocation_serial + 1,
                    ..frame
                }),
                ..parent
            },
        },
    ] {
        assert!(matches!(
            host.handle_request(PageHostRequest::DescribeDebuggerStaticScopeRelation {
                target: Box::new(denied),
            }),
            PageHostReply::Error { .. }
        ));
    }
}

#[test]
fn private_child_bluets_classic_returns_only_exact_paused_plain_values() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![blue_ts_classic(
                    0,
                    "let answer: number = 41; let data = { answer: answer };",
                )],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = host.documents[&7]
        .pending_debugger_executions
        .front()
        .unwrap()
        .program
        .unwrap();
    let halt = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .filter(|point| point.code_unit_ordinal == 0)
            .max_by_key(|point| point.bytecode_offset)
            .unwrap(),
        reply => panic!("expected BlueTS root points: {reply:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: halt,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let targets = paused_value_targets(&mut host, program, None, 0);
    let replies: Vec<_> = targets
        .iter()
        .map(|target| {
            host.handle_request(PageHostRequest::GetDebuggerValueSnapshot { target: *target })
        })
        .collect();
    assert!(replies.iter().any(|reply| matches!(
        reply,
        PageHostReply::DebuggerValueSnapshot(snapshot)
            if matches!(&snapshot.preview, PageHostDebuggerValuePreview::NumberBits(bits) if *bits == 41.0_f64.to_bits())
    )));
    assert!(replies.iter().any(|reply| matches!(
        reply,
        PageHostReply::DebuggerValueSnapshot(snapshot)
            if matches!(&snapshot.preview, PageHostDebuggerValuePreview::Record(entries)
                if entries == &vec![("answer".encode_utf16().collect(), PageHostDebuggerValuePreview::NumberBits(41.0_f64.to_bits()))])
    )));
    let target = targets[0];
    for invalid in [
        PageHostDebuggerValueTarget {
            safe_point: PageHostDebuggerSafePoint {
                bytecode_offset: target.safe_point.bytecode_offset + 1,
                ..target.safe_point
            },
            ..target
        },
        PageHostDebuggerValueTarget {
            scope_entry: PageHostDebuggerScopeEntry {
                slot_ordinal: u32::MAX,
                ..target.scope_entry
            },
            ..target
        },
    ] {
        assert!(matches!(
            host.handle_request(PageHostRequest::GetDebuggerValueSnapshot { target: invalid }),
            PageHostReply::Error { .. }
        ));
    }
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerValueSnapshot {
            target: PageHostDebuggerValueTarget {
                document_generation: 2,
                ..target
            },
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn private_child_bluets_nested_preview_requires_its_active_frame() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![blue_ts_classic(
                    0,
                    "let seed: number = 7; function inner(value: number): number { let local: number = value; return local; } inner(seed);",
                )],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = host.documents[&7]
        .pending_debugger_executions
        .front()
        .unwrap()
        .program
        .unwrap();
    let point = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected BlueTS nested points: {reply:?}"),
    };
    host.arm_debugger_nested_target(7, 1, point).unwrap();
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
        program,
    }) {
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::NestedPaused { frame, .. },
            ..
        } => frame,
        reply => panic!("expected nested pause: {reply:?}"),
    };
    let mut selected = None;
    for _ in 0..64 {
        for target in paused_value_targets(&mut host, program, Some(frame), 0) {
            if matches!(
                host.handle_request(PageHostRequest::GetDebuggerValueSnapshot { target }),
                PageHostReply::DebuggerValueSnapshot(snapshot)
                    if matches!(&snapshot.preview, PageHostDebuggerValuePreview::NumberBits(bits) if *bits == 7.0_f64.to_bits())
            ) {
                selected = Some(target);
                break;
            }
        }
        if selected.is_some() {
            break;
        }
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerNestedInstruction { frame }),
            PageHostReply::DebuggerNestedStepRequested { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
    }
    let target = selected.expect("one active child slot must hold the parameter value");
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerValueSnapshot {
            target: PageHostDebuggerValueTarget {
                frame: None,
                ..target
            }
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerValueSnapshot {
            target: PageHostDebuggerValueTarget {
                frame: Some(PageHostDebuggerFrame {
                    invocation_serial: frame.invocation_serial + 1,
                    ..frame
                }),
                ..target
            }
        }),
        PageHostReply::Error { .. }
    ));
}

#[test]
fn private_child_bluets_module_root_preview_reads_retained_binding() {
    let entry = "blueice://page/private-values.ts";
    let module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![PageHostSource::new(
                entry,
                "let answer: number = 41; export const result: number = answer + 1;",
            )],
        ),
    };
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
    let program = pending.program.unwrap();
    let DeferredChildExecution::BlueTsModule { attachment, .. } = &pending.execution else {
        panic!("the checked module must be pending");
    };
    let point = host
        .runtime
        .module_evaluate_entry_safe_point(7, attachment.entry.handle)
        .unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: 0,
                bytecode_offset: point.bytecode_offset,
            },
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let mut selected = None;
    for _ in 0..64 {
        for target in paused_value_targets(&mut host, program, None, 0) {
            if matches!(
                host.handle_request(PageHostRequest::GetDebuggerValueSnapshot { target }),
                PageHostReply::DebuggerValueSnapshot(snapshot)
                    if matches!(&snapshot.preview, PageHostDebuggerValuePreview::NumberBits(bits) if *bits == 41.0_f64.to_bits())
            ) {
                selected = Some(target);
                break;
            }
        }
        if selected.is_some() {
            break;
        }
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionStepRequested { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
    }
    let target = selected.expect("module root must expose its initialized binding");
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerValueSnapshot { target }),
        PageHostReply::DebuggerValueSnapshot(snapshot)
            if matches!(&snapshot.preview, PageHostDebuggerValuePreview::NumberBits(bits) if *bits == 41.0_f64.to_bits())
    ));
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
fn executes_authorized_classic_and_rejects_a_later_parse_failure_independently() {
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![
                classic(0, "globalThis.answer = 42;"),
                classic(1, "const = ;"),
            ],
        ),
    });
    let PageHostReply::Synchronized { reports, .. } = reply else {
        panic!("expected synchronized reply");
    };
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    assert!(matches!(
        reports[1].outcome,
        PageHostScriptOutcome::Rejected { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmStats(PageHostRealmStats {
            program_count: 1,
            ..
        })
    ));
}

#[test]
fn private_debugger_locations_are_generation_and_tab_bound_without_runtime_leaks() {
    let mut host = BlueJsChildHost::default();
    let first = document(1, vec![classic(0, "let answer = 42;")]);
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument { document: first }),
        PageHostReply::Synchronized { .. }
    ));
    let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected source-free child debugger programs, got {reply:?}"),
    };
    assert_eq!(programs.len(), 1);
    let program = programs[0];
    let safe_points = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points,
        reply => panic!("expected source-free child debugger safe points, got {reply:?}"),
    };
    let safe_point = *safe_points
        .first()
        .expect("a retained classic program has a root safe point");
    assert_eq!(
        host.handle_request(PageHostRequest::ValidateDebuggerSafePoint {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }),
        PageHostReply::DebuggerSafePointValidated {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }),
        PageHostReply::DebuggerBreakpointSet {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }
    );
    // Retries are idempotent and cannot consume a second bounded record.
    assert_eq!(
        host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }),
        PageHostReply::DebuggerBreakpointSet {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::ListDebuggerBreakpoints {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerBreakpoints {
            tab_id: 7,
            document_generation: 1,
            safe_points: vec![safe_point],
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::ClearDebuggerBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }),
        PageHostReply::DebuggerBreakpointCleared {
            tab_id: 7,
            document_generation: 1,
            safe_point,
            was_present: true,
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::ClearDebuggerBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point,
        }),
        PageHostReply::DebuggerBreakpointCleared {
            tab_id: 7,
            document_generation: 1,
            safe_point,
            was_present: false,
        }
    );

    let mut other = document(1, vec![classic(0, "let other = 7;")]);
    other.tab_id = 9;
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument { document: other }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerSafePoints {
            tab_id: 9,
            document_generation: 1,
            program,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));

    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![classic(0, "let successor = 1;")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::ListDebuggerBreakpoints {
            tab_id: 7,
            document_generation: 2,
        }),
        PageHostReply::DebuggerBreakpoints {
            tab_id: 7,
            document_generation: 2,
            safe_points: Vec::new(),
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    let reply = format!(
        "{:?}",
        host.handle_request(PageHostRequest::ListDebuggerSafePoints {
            tab_id: 7,
            document_generation: 2,
            program,
        })
    );
    assert!(
        !reply.contains("answer") && !reply.contains("bytecode") && !reply.contains("Value"),
        "private debugger errors must remain source/value-free"
    );
}

#[test]
fn root_classic_breakpoint_pauses_and_resumes_without_vm_disclosure() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![classic(0, "let first = 1; first += 1; globalThis.answer = first;")],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected private classic program, got {reply:?}"),
    };
    let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0)
            .expect("fixture must have a non-entry root safe point"),
        reply => panic!("expected source-free safe points, got {reply:?}"),
    };
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
            state: PageHostDebuggerExecutionState::Pending,
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: target,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { safe_point, .. }
            if safe_point == target
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
            state: PageHostDebuggerExecutionState::Paused { safe_point: target },
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionStepRequested {
            tab_id: 7,
            document_generation: 1,
            program,
        }
    );
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
            state: PageHostDebuggerExecutionState::Stepping,
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::Error { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let successor = match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::Paused { safe_point },
            ..
        } => safe_point,
        reply => panic!("one child root step must pause at its successor: {reply:?}"),
    };
    assert_ne!(successor, target);
    assert_eq!(successor.program, program);
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if reports == vec![script_report(
                7,
                1,
                0,
                PageHostScriptLanguage::JavaScript,
                PageHostScriptKind::Classic,
                PageHostScriptOutcome::Executed,
            )]
    ));
    let reply = format!(
        "{:?}",
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        })
    );
    assert!(reply.contains("Completed"));
    assert!(
        !reply.contains("answer") && !reply.contains("Value") && !reply.contains("Vm"),
        "execution state remains source/value/VM-free"
    );
}

#[test]
fn root_classic_steps_revisit_loop_boundaries_without_releasing_the_queue() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![classic(
                    0,
                    "let index = 0; while (index < 2) { index++; } globalThis.done = index;",
                )],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected queued classic program: {reply:?}"),
    };
    let safe_points = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points,
        reply => panic!("expected verified root safe points: {reply:?}"),
    };
    let target = *safe_points
        .iter()
        .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset != 0)
        .unwrap();
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: target,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let mut seen_offsets = Vec::new();
    let mut completion_reports = None;
    for _ in 0..256 {
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionStepRequested { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Stepping,
                ..
            }
        ));
        let reports = match host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } => reports,
            reply => panic!("expected one bounded child advance: {reply:?}"),
        };
        match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Paused { safe_point },
                ..
            } => {
                assert!(safe_points.contains(&safe_point));
                seen_offsets.push(safe_point.bytecode_offset);
                assert!(reports.is_empty());
            }
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Completed,
                ..
            } => {
                completion_reports = Some(reports);
                break;
            }
            reply => panic!("unexpected child step state: {reply:?}"),
        }
    }
    assert_eq!(
        completion_reports,
        Some(vec![script_report(
            7,
            1,
            0,
            PageHostScriptLanguage::JavaScript,
            PageHostScriptKind::Classic,
            PageHostScriptOutcome::Executed,
        )])
    );
    assert!(
        seen_offsets
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            < seen_offsets.len(),
        "the loop must revisit a real root boundary"
    );
}

#[test]
fn private_debugger_breakpoint_configuration_is_idempotent_and_bounded() {
    let mut host = BlueJsChildHost::default();
    let source = (0..=PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
        .map(|index| format!("let breakpoint_{index} = {index};"))
        .collect::<String>();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![classic(0, &source)]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected child debugger program, got {reply:?}"),
    };
    let safe_points = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points,
        reply => panic!("expected child debugger safe points, got {reply:?}"),
    };
    let max = usize::try_from(PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM)
        .expect("page-host breakpoint cap fits usize");
    assert!(
        safe_points.len() > max,
        "fixture needs one point over the cap"
    );
    for safe_point in safe_points.iter().copied().take(max) {
        assert!(matches!(
            host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 1,
                safe_point,
            }),
            PageHostReply::DebuggerBreakpointSet { .. }
        ));
    }
    let overflow = safe_points[max];
    assert!(matches!(
        host.handle_request(PageHostRequest::SetDebuggerBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: overflow,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::ResourceLimit,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBreakpoints {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerBreakpoints { safe_points, .. } if safe_points.len() == max
    ));
}

#[test]
fn direct_bluets_and_javascript_execute_in_document_order_in_one_realm() {
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![
                classic(0, "globalThis.beforeBlueTs = true;"),
                // The TypeScript annotation makes this invalid JavaScript
                // source. Success therefore proves the child used direct
                // BlueTS lowering rather than emitted-JavaScript reparse.
                blue_ts_classic(1, "const sharedAnswer: number = 42;"),
                classic(
                    2,
                    "if (!globalThis.beforeBlueTs || sharedAnswer !== 42) throw 'realm/order failed';",
                ),
            ],
        ),
    });
    let PageHostReply::Synchronized { reports, .. } = reply else {
        panic!("expected synchronized reply");
    };
    assert_eq!(
        reports
            .iter()
            .map(|report| (report.ordinal, report.language, &report.outcome))
            .collect::<Vec<_>>(),
        vec![
            (
                0,
                PageHostScriptLanguage::JavaScript,
                &PageHostScriptOutcome::Executed
            ),
            (
                1,
                PageHostScriptLanguage::BlueTs,
                &PageHostScriptOutcome::Executed
            ),
            (
                2,
                PageHostScriptLanguage::JavaScript,
                &PageHostScriptOutcome::Executed
            ),
        ]
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmStats(PageHostRealmStats {
            program_count: 3,
            ..
        })
    ));
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

#[test]
fn child_bluets_metadata_inventory_mints_only_opaque_live_attachment_handles() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    classic(0, "globalThis.javaScriptOnly = true;"),
                    blue_ts_classic(1, "const typedAnswer: number = 42;"),
                ],
            ),
        }),
        PageHostReply::Synchronized { reports, .. }
            if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
    ));
    let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected private program inventory, got {reply:?}"),
    };
    assert_eq!(programs.len(), 2);

    let mut blue_ts = None;
    for program in programs {
        let reply = host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program,
        });
        let PageHostReply::DebuggerBlueTsMetadata { metadata, .. } = &reply else {
            panic!("expected private BlueTS metadata inventory, got {reply:?}");
        };
        // The child exposes no source/module/name/type/span/contract data:
        // only the bounded list's length and its opaque values are visible.
        assert!(!format!("{reply:?}").contains("typedAnswer"));
        assert!(!format!("{reply:?}").contains("inline-1.ts"));
        assert!(!format!("{reply:?}").contains("number"));
        if let [metadata] = metadata.as_slice() {
            blue_ts = Some((program, *metadata));
        } else {
            assert!(
                metadata.is_empty(),
                "only the JavaScript program is ineligible"
            );
        }
    }
    let (typed_program, first_metadata) = blue_ts.expect("the live BlueTS attachment exists");
    assert!(first_metadata.is_well_formed());
    assert_ne!(
        first_metadata.metadata_handle, typed_program.program_handle,
        "metadata IDs must not reuse the child program namespace"
    );
    assert!(
        first_metadata.metadata_handle >= CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START,
        "metadata IDs have a child-private namespace separate from program IDs"
    );
    let summary_reply = host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program: typed_program,
        metadata: first_metadata,
    });
    let PageHostReply::DebuggerBlueTsMetadataSummary {
        metadata, summary, ..
    } = summary_reply
    else {
        panic!("expected bounded private BlueTS metadata summary")
    };
    assert_eq!(metadata, first_metadata);
    assert_eq!(summary.language_version, "blue-ts-0.1");
    assert!(summary.source_count > 0);
    assert!(summary.type_count > 0);
    assert!(summary.symbol_count > 0);
    // The summary intentionally reveals no source/module/name/type/span/
    // contract record. A separate future capability would be needed for
    // every individual record family.
    assert!(!format!("{summary:?}").contains("typedAnswer"));
    assert!(!format!("{summary:?}").contains("inline-1.ts"));
    assert!(!format!("{summary:?}").contains("number"));
    let source_inventory_reply =
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: first_metadata,
        });
    let PageHostReply::DebuggerBlueTsMetadataSources {
        program,
        metadata,
        sources,
        ..
    } = source_inventory_reply
    else {
        panic!("expected bounded private BlueTS source-record identity inventory")
    };
    assert_eq!(program, typed_program);
    assert_eq!(metadata, first_metadata);
    assert_eq!(
        sources.len(),
        usize::try_from(summary.source_count).unwrap()
    );
    assert_eq!(
        sources
            .iter()
            .map(|source| source.source_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        sources.len(),
        "one metadata attachment cannot repeat a compiler source-record ID"
    );
    // The opaque inventory conveys source-record cardinality and IDs only;
    // no module, source, hash, span, or compiler-record field crosses it.
    assert!(!format!("{sources:?}").contains("typedAnswer"));
    assert!(!format!("{sources:?}").contains("inline-1.ts"));
    assert!(!format!("{sources:?}").contains("number"));
    let type_inventory_reply =
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: first_metadata,
        });
    let PageHostReply::DebuggerBlueTsMetadataTypes {
        program,
        metadata,
        types,
        ..
    } = type_inventory_reply
    else {
        panic!("expected bounded private BlueTS type-record identity inventory")
    };
    assert_eq!(program, typed_program);
    assert_eq!(metadata, first_metadata);
    assert_eq!(types.len(), usize::try_from(summary.type_count).unwrap());
    assert_eq!(
        types
            .iter()
            .map(|static_type| static_type.type_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        types.len(),
        "one metadata attachment cannot repeat a compiler type-record ID"
    );
    assert!(!format!("{types:?}").contains("typedAnswer"));
    assert!(!format!("{types:?}").contains("inline-1.ts"));
    assert!(!format!("{types:?}").contains("number"));
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: first_metadata.metadata_handle,
                metadata_generation: first_metadata.metadata_generation + 1,
            },
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: first_metadata.metadata_handle,
                metadata_generation: first_metadata.metadata_generation + 1,
            },
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: first_metadata.metadata_handle,
                metadata_generation: first_metadata.metadata_generation + 1,
            },
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));

    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
        }),
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. }
            if metadata == vec![first_metadata]
    ));

    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                2,
                vec![blue_ts_classic(0, "const replacement: string = 'next';")]
            ),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: first_metadata,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: first_metadata,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
            tab_id: 7,
            document_generation: 1,
            program: typed_program,
            metadata: first_metadata,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    let replacement_program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 2,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => *programs
            .first()
            .expect("the replacement BlueTS program remains live"),
        reply => panic!("expected replacement private program inventory, got {reply:?}"),
    };
    let replacement_metadata =
        match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 2,
            program: replacement_program,
        }) {
            PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => *metadata
                .first()
                .expect("the replacement live BlueTS attachment remains eligible"),
            reply => panic!("expected replacement metadata inventory, got {reply:?}"),
        };
    assert_ne!(replacement_metadata, first_metadata);

    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 7,
            document_generation: 2,
        }),
        PageHostReply::RealmClosed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 2,
            program: replacement_program,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::UnknownRealm,
            ..
        }
    ));
}

#[test]
fn child_bluets_symbol_contract_verifies_only_one_live_reifiable_pair() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![blue_ts_classic(
                    0,
                    "interface PrivateContract { enabled: boolean; } const typedAnswer: number = 42;",
                )],
            ),
        }),
        PageHostReply::Synchronized { reports, .. }
            if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected live child program, got {reply:?}"),
    };
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected live child metadata, got {reply:?}"),
    };
    let symbols = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
        tab_id: 7,
        document_generation: 1,
        program,
        metadata,
    }) {
        PageHostReply::DebuggerBlueTsMetadataSymbols { symbols, .. } => symbols,
        reply => panic!("expected child symbol IDs, got {reply:?}"),
    };
    let contracts =
        match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataContracts {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
        }) {
            PageHostReply::DebuggerBlueTsMetadataContracts { contracts, .. } => contracts,
            reply => panic!("expected child contract IDs, got {reply:?}"),
        };
    assert!(!symbols.is_empty() && !contracts.is_empty());
    let mut verified = None;
    for symbol in &symbols {
        for contract in &contracts {
            let reply = host.handle_request(
                PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                    metadata,
                    symbol_id: symbol.symbol_id,
                    contract_id: contract.contract_id,
                },
            );
            match reply {
                PageHostReply::DebuggerBlueTsMetadataSymbolContract {
                    symbol_contract, ..
                } => {
                    assert_eq!(symbol_contract.symbol_id, symbol.symbol_id);
                    assert_eq!(symbol_contract.contract_id, contract.contract_id);
                    verified = Some(symbol_contract);
                    break;
                }
                PageHostReply::Error {
                    code: PageHostErrorCode::InvalidRequest,
                    ..
                } => {}
                reply => panic!("unexpected private relation reply: {reply:?}"),
            }
        }
        if verified.is_some() {
            break;
        }
    }
    let verified = verified.expect("the interface has one reifiable contract relation");
    assert!(!format!("{verified:?}").contains("PrivateContract"));
    assert!(!format!("{verified:?}").contains("enabled"));
    let sources = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
        tab_id: 7,
        document_generation: 1,
        program,
        metadata,
    }) {
        PageHostReply::DebuggerBlueTsMetadataSources { sources, .. } => sources,
        reply => panic!("expected child source IDs, got {reply:?}"),
    };
    let location = match host.handle_request(
        PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
            contract_id: verified.contract_id,
        },
    ) {
        PageHostReply::DebuggerBlueTsMetadataContractLocation { location, .. } => location,
        reply => panic!("expected child contract location, got {reply:?}"),
    };
    assert_eq!(location.contract_id, verified.contract_id);
    assert!(sources
        .iter()
        .any(|source| source.source_id == location.source_id));
    assert!(location.start_byte < location.end_byte);
    assert!(location.end_byte <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES);
    assert_eq!(location.coordinates.start_line, 0);
    assert_eq!(location.coordinates.end_line, 0);
    assert!(!format!("{location:?}").contains("PrivateContract"));
    assert!(!format!("{location:?}").contains("enabled"));
    assert!(matches!(
        host.handle_request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                contract_id: u32::MAX,
            }
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                symbol_id: verified.symbol_id,
                contract_id: u32::MAX,
            }
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![blue_ts_classic(0, "const successor: number = 1;")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                symbol_id: verified.symbol_id,
                contract_id: verified.contract_id,
            }
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                contract_id: verified.contract_id,
            }
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn child_bluets_symbol_location_is_live_bound_and_source_text_free() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![blue_ts_classic(0, "const typedAnswer: number = 42;")]),
        }),
        PageHostReply::Synchronized { reports, .. }
            if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => *programs
            .first()
            .expect("the one BlueTS program remains live"),
        reply => panic!("expected private debugger program inventory, got {reply:?}"),
    };
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => *metadata
            .first()
            .expect("the BlueTS program retains one static attachment"),
        reply => panic!("expected private BlueTS metadata inventory, got {reply:?}"),
    };
    let sources = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
        tab_id: 7,
        document_generation: 1,
        program,
        metadata,
    }) {
        PageHostReply::DebuggerBlueTsMetadataSources { sources, .. } => sources,
        reply => panic!("expected private source-ID inventory, got {reply:?}"),
    };
    let symbols = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
        tab_id: 7,
        document_generation: 1,
        program,
        metadata,
    }) {
        PageHostReply::DebuggerBlueTsMetadataSymbols { symbols, .. } => symbols,
        reply => panic!("expected private symbol-ID inventory, got {reply:?}"),
    };
    let symbol = symbols
        .into_iter()
        .find(|candidate| {
            matches!(
                host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbol {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                    metadata,
                    symbol_id: candidate.symbol_id,
                }),
                PageHostReply::DebuggerBlueTsMetadataSymbol { symbol, .. }
                    if symbol.display == "typedAnswer"
            )
        })
        .expect("the page declaration must retain its own symbol ID");
    let reply = host.handle_request(
        PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
            symbol_id: symbol.symbol_id,
        },
    );
    let PageHostReply::DebuggerBlueTsMetadataSymbolLocation { location, .. } = reply else {
        panic!("expected bounded private symbol location")
    };
    assert_eq!(location.symbol_id, symbol.symbol_id);
    assert!(sources
        .iter()
        .any(|source| source.source_id == location.source_id));
    assert!(location.start_byte < location.end_byte);
    assert!(location.end_byte <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES);
    assert_eq!(location.coordinates.start_line, 0);
    assert_eq!(location.coordinates.end_line, 0);
    // The result is deliberately only ID/range/coordinate structure, even inside the
    // private bridge: no source text, module identity, name, or type leaks.
    assert!(!format!("{location:?}").contains("typedAnswer"));
    assert!(!format!("{location:?}").contains("inline-0.ts"));
    assert!(!format!("{location:?}").contains("number"));
    let types = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
        tab_id: 7,
        document_generation: 1,
        program,
        metadata,
    }) {
        PageHostReply::DebuggerBlueTsMetadataTypes { types, .. } => types,
        reply => panic!("expected private type-ID inventory, got {reply:?}"),
    };
    assert!(!types.is_empty());
    let matching = types
        .iter()
        .filter_map(|static_type| {
            let reply =
                host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
                    tab_id: 7,
                    document_generation: 1,
                    program,
                    metadata,
                    symbol_id: symbol.symbol_id,
                    type_id: static_type.type_id,
                });
            match reply {
                PageHostReply::DebuggerBlueTsMetadataSymbolType { symbol_type, .. } => {
                    Some(symbol_type)
                }
                _ => None,
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].symbol_id, symbol.symbol_id);
    assert!(types
        .iter()
        .any(|static_type| static_type.type_id == matching[0].type_id));
    assert!(!format!("{:?}", matching[0]).contains("typedAnswer"));
    assert!(!format!("{:?}", matching[0]).contains("number"));
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
            symbol_id: symbol.symbol_id,
            type_id: u32::MAX,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                symbol_id: u32::MAX,
            }
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                2,
                vec![blue_ts_classic(0, "const replacement: number = 1;")]
            ),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                symbol_id: symbol.symbol_id,
            }
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
            symbol_id: symbol.symbol_id,
            type_id: matching[0].type_id,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn child_bluets_safe_point_span_requires_an_exact_live_metadata_attachment() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    blue_ts_classic(0, "const first: number = 1;"),
                    blue_ts_classic(1, "const second: number = 2;"),
                ],
            ),
        }),
        PageHostReply::Synchronized { reports, .. }
            if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
    ));
    let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected two private BlueTS programs, got {reply:?}"),
    };
    assert_eq!(programs.len(), 2);
    let program = programs[0];
    let other_program = programs[1];
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected exact private metadata handle, got {reply:?}"),
    };
    let entry = {
        let handle = host.documents[&7].debugger_programs[&program.program_handle].runtime_handle;
        host.debug_registry
            .get(host.runtime.program_registry(), handle)
            .unwrap()
            .safe_point_map()
            .entries[0]
            .clone()
    };
    let safe_point = PageHostDebuggerSafePoint {
        program,
        code_unit_ordinal: entry.code_unit.ordinal(),
        bytecode_offset: entry.bytecode_offset,
    };
    let request = PageHostRequest::DescribeDebuggerBlueTsSafePointSpan {
        tab_id: 7,
        document_generation: 1,
        metadata,
        safe_point,
    };
    let PageHostReply::DebuggerBlueTsSafePointSpan {
        span,
        safe_point: echoed,
        ..
    } = host.handle_request(request.clone())
    else {
        panic!("an exact retained safe point must have its original BlueTS span")
    };
    assert_eq!(echoed, safe_point);
    assert_eq!(
        (span.start_byte, span.end_byte),
        (
            u32::try_from(entry.start_byte).unwrap(),
            u32::try_from(entry.end_byte).unwrap()
        )
    );
    assert!(span.start_byte < span.end_byte);
    assert!(span.end_byte <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES);
    assert!(span
        .coordinates
        .is_well_formed_for_range(span.start_byte, span.end_byte));
    assert!(!format!("{span:?}").contains("const first"));
    assert!(!format!("{span:?}").contains("inline-0.ts"));

    for (metadata, safe_point) in [
        (
            metadata,
            PageHostDebuggerSafePoint {
                program: other_program,
                ..safe_point
            },
        ),
        (
            PageHostDebuggerMetadataHandle {
                metadata_generation: metadata.metadata_generation + 1,
                ..metadata
            },
            safe_point,
        ),
        (
            metadata,
            PageHostDebuggerSafePoint {
                bytecode_offset: u32::MAX,
                ..safe_point
            },
        ),
    ] {
        assert!(matches!(
            host.handle_request(PageHostRequest::DescribeDebuggerBlueTsSafePointSpan {
                tab_id: 7,
                document_generation: 1,
                metadata,
                safe_point,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
    }
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![blue_ts_classic(0, "const successor = 3;")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(request),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn child_bluets_source_breakpoint_is_bound_to_a_live_source_and_generation() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    blue_ts_classic(
                        0,
                        "const first: number = 1; const second: number = 2; second;"
                    ),
                    blue_ts_classic(1, "const other: number = 3;"),
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
        reply => panic!("expected private BlueTS programs, got {reply:?}"),
    };
    let program = programs[0];
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected private BlueTS metadata, got {reply:?}"),
    };
    let (source_id, first, second) = {
        let handle = host.documents[&7].debugger_programs[&program.program_handle].runtime_handle;
        let retained = host
            .debug_registry
            .get(host.runtime.program_registry(), handle)
            .unwrap();
        let mut entries = retained.safe_point_map().entries.iter().collect::<Vec<_>>();
        entries.sort_by_key(|entry| (entry.start_byte, entry.bytecode_offset));
        let first = entries[0];
        let second = entries
            .iter()
            .copied()
            .find(|entry| entry.source == first.source && entry.start_byte >= first.end_byte)
            .expect("the next distinct declaration has a bound entry");
        (
            retained
                .static_info()
                .sources
                .iter()
                .find(|source| source.module == entries[0].source)
                .expect("the lowered source must have a compiler source ID")
                .id
                .0,
            first.clone(),
            second.clone(),
        )
    };
    let request = |source_byte| PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
        tab_id: 7,
        document_generation: 1,
        program,
        metadata,
        source_id,
        source_byte,
    };
    for (source_byte, entry) in [
        (u32::try_from(first.start_byte).unwrap(), &first),
        (u32::try_from(first.end_byte).unwrap(), &second),
    ] {
        assert_eq!(
            host.handle_request(request(source_byte)),
            PageHostReply::DebuggerBlueTsSourceBreakpoint {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                source_id,
                source_byte,
                safe_point: Some(PageHostDebuggerSafePoint {
                    program,
                    code_unit_ordinal: entry.code_unit.ordinal(),
                    bytecode_offset: entry.bytecode_offset,
                }),
            }
        );
    }
    assert_eq!(
        host.handle_request(request(u32::try_from(second.end_byte).unwrap() + 10)),
        PageHostReply::DebuggerBlueTsSourceBreakpoint {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
            source_id,
            source_byte: u32::try_from(second.end_byte).unwrap() + 10,
            safe_point: None,
        }
    );
    for forged in [
        PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
            tab_id: 7,
            document_generation: 1,
            program: programs[1],
            metadata,
            source_id,
            source_byte: 0,
        },
        PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
            source_id: u32::MAX,
            source_byte: 0,
        },
        PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_generation: metadata.metadata_generation + 1,
                ..metadata
            },
            source_id,
            source_byte: 0,
        },
        request(DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES + 1),
    ] {
        assert!(matches!(
            host.handle_request(forged),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
    }
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![blue_ts_classic(0, "const successor = 4;")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(request(0)),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn child_installs_only_fixed_core_snapshot_callbacks_for_javascript() {
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document_with_snapshot(
            1,
            "private document snapshot".to_string(),
            "https://example.test".to_string(),
            vec![
                classic(
                    0,
                    concat!(
                        "if (blueiceDocumentText() !== 'private document snapshot') throw 'text';",
                        "if (blueiceDocumentOrigin() !== 'https://example.test') throw 'origin';",
                        "if (typeof document !== 'undefined' || typeof fetch !== 'undefined' || typeof blueiceTestHasElementById !== 'undefined' || typeof blueiceTestGetElementById !== 'undefined') throw 'ambient';"
                    ),
                ),
                classic(1, "blueiceDocumentText(1);"),
            ],
        ),
    });
    let PageHostReply::Synchronized { reports, .. } = reply else {
        panic!("expected synchronized reply");
    };
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    assert!(matches!(
        reports[1].outcome,
        PageHostScriptOutcome::Rejected { .. }
    ));
    // Result records remain source/value-free even when a callback
    // returned a private snapshot inside the child VM.
    assert!(!format!("{:?}", reports).contains("private document snapshot"));
}

#[test]
fn snapshot_validation_happens_before_realm_creation_or_replacement() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document_with_snapshot(
                1,
                "snapshot".to_string(),
                "HTTP://EXAMPLE.test".to_string(),
                vec![classic(0, "globalThis.answer = 42;")],
            ),
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::UnknownRealm,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document_with_snapshot(
                1,
                "x".repeat(PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES + 1),
                "https://example.test".to_string(),
                vec![classic(0, "globalThis.answer = 42;")],
            ),
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::ResourceLimit,
            ..
        }
    ));
}

#[test]
fn bluets_uses_the_verified_snapshot_profile_and_rejects_untyped_globals() {
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![
                // The annotation is invalid JavaScript syntax, so this
                // success also preserves direct BlueTS-to-BlueJS lowering.
                blue_ts_classic(
                    0,
                    concat!(
                        "const snapshotText: string = blueiceDocumentText();",
                        "const snapshotOrigin: string = blueiceDocumentOrigin();"
                    ),
                ),
                classic(
                    1,
                    concat!(
                        "if (snapshotText !== 'test document snapshot') throw 'text';",
                        "if (snapshotOrigin !== 'https://example.test') throw 'origin';"
                    ),
                ),
                // The fixed declaration contains neither fetch nor a
                // general document object. All three compile attempts fail
                // before a program is admitted to the child realm.
                blue_ts_classic(2, "fetch('https://example.test/');"),
                blue_ts_classic(3, "blueiceDocumentText(1);"),
                blue_ts_classic(4, "document.getElementById('target');"),
            ],
        ),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if reports.len() == 5
            && reports[0].language == PageHostScriptLanguage::BlueTs
            && reports[0].outcome == PageHostScriptOutcome::Executed
            && reports[1].language == PageHostScriptLanguage::JavaScript
            && reports[1].outcome == PageHostScriptOutcome::Executed
            && matches!(reports[2].outcome, PageHostScriptOutcome::Rejected { .. })
            && matches!(reports[3].outcome, PageHostScriptOutcome::Rejected { .. })
            && matches!(reports[4].outcome, PageHostScriptOutcome::Rejected { .. })
    ));
}

#[test]
fn module_graph_uses_only_the_explicit_static_resolution_records() {
    let entry = "blueice://page/entry.js";
    let dependency = "blueice://page/dependency.js";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { answer } from './dependency.js'; export const result = answer;",
                ),
                PageHostSource::new(dependency, "export const answer = 42;"),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./dependency.js".to_string(),
        canonical_target: dependency.to_string(),
    });
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(1, vec![module]),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if reports[0].outcome == PageHostScriptOutcome::Executed
    ));
}

#[test]
fn direct_bluets_module_graph_uses_only_the_explicit_static_resolution_records() {
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
                    "import { answer } from './dependency.ts'; export const result: number = answer;",
                ),
                PageHostSource::new(dependency, "export const answer: number = 42;"),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./dependency.ts".to_string(),
        canonical_target: dependency.to_string(),
    });
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(1, vec![module]),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if matches!(
            reports.as_slice(),
            [PageHostScriptReport {
                language: PageHostScriptLanguage::BlueTs,
                kind: PageHostScriptKind::Module,
                outcome: PageHostScriptOutcome::Executed,
                ..
            }]
        )
    ));
    assert_eq!(host.debug_registry.len(), 2);
    let mut retained_modules: Vec<_> = host
        .documents
        .get(&7)
        .expect("the child realm remains live")
        .debugger_programs
        .values()
        .filter_map(|record| {
            host.debug_registry
                .get(host.runtime.program_registry(), record.runtime_handle)
                .ok()
        })
        .map(|metadata| {
            let sources = &metadata.static_info().sources;
            sources
                .iter()
                .find(|source| source.module == entry || source.module == dependency)
                .expect("each module keeps its own static source metadata")
                .module
                .clone()
        })
        .collect();
    retained_modules.sort();
    assert_eq!(
        retained_modules,
        vec![dependency.to_string(), entry.to_string()]
    );
}

#[test]
fn bluets_module_child_pauses_steps_and_resumes_the_exact_pending_entry() {
    let entry = "blueice://page/debug-entry.ts";
    let dependency = "blueice://page/debug-dependency.ts";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { answer } from './debug-dependency.ts'; export const result: number = answer + 1;",
                ),
                PageHostSource::new(dependency, "export const answer: number = 41;"),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./debug-dependency.ts".to_string(),
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
    let program = pending.program.unwrap();
    let DeferredChildExecution::BlueTsModule { attachment, .. } = &pending.execution else {
        panic!("the attached module is pending");
    };
    let entry_handle = attachment.entry.handle;
    let point = host
        .runtime
        .module_evaluate_entry_safe_point(7, entry_handle)
        .unwrap();
    let target = PageHostDebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: point.bytecode_offset,
    };
    let wrong_entry_point = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 0 && *point != target)
            .unwrap(),
        reply => panic!("expected entry safe points, got {reply:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: wrong_entry_point,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        }
    ));
    let dependency_program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => *programs
            .iter()
            .find(|candidate| **candidate != program)
            .unwrap(),
        reply => panic!("expected both module programs, got {reply:?}"),
    };
    let dependency_point = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program: dependency_program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 0)
            .unwrap(),
        reply => panic!("expected dependency safe points, got {reply:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: dependency_point,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: target,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    for _ in 0..2 {
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        assert_eq!(
            host.handle_request(PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionState {
                tab_id: 7,
                document_generation: 1,
                program,
                state: PageHostDebuggerExecutionState::Paused { safe_point: target },
            }
        );
    }
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 1,
            program: dependency_program,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionStepRequested { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let successor = match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::Paused { safe_point },
            ..
        } => safe_point,
        reply => panic!("expected verified module step successor, got {reply:?}"),
    };
    assert_eq!(successor.program, program);
    assert_eq!(successor.code_unit_ordinal, 0);
    assert_ne!(successor, target);
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
            state: PageHostDebuggerExecutionState::Paused {
                safe_point: successor
            },
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::ValidateDebuggerSafePoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: successor,
        }),
        PageHostReply::DebuggerSafePointValidated { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
            state: PageHostDebuggerExecutionState::Resuming,
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if reports.len() == 1 && reports[0].outcome == PageHostScriptOutcome::Executed
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::Completed,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(2, Vec::new()),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: target,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn bluets_module_source_span_step_stops_at_the_next_bound_statement() {
    let entry = "blueice://page/module-source-step.ts";
    let module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![PageHostSource::new(
                entry,
                "let first: number = 1; let second: number = first + 1; export const answer: number = second + 1;",
            )],
        ),
    };
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
    let program = pending.program.unwrap();
    let DeferredChildExecution::BlueTsModule { attachment, .. } = &pending.execution else {
        panic!("the checked entry module must be pending");
    };
    let point = host
        .runtime
        .module_evaluate_entry_safe_point(7, attachment.entry.handle)
        .unwrap();
    let target = PageHostDebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: point.bytecode_offset,
    };
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected entry metadata, got {reply:?}"),
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: target,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let mut current = target;
    let origin = loop {
        if let Some(span) = host
            .debugger_bluets_source_span_key(7, 1, metadata, current)
            .unwrap()
        {
            break span;
        }
        assert!(matches!(
            host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
                tab_id: 7,
                document_generation: 1,
                program,
            }),
            PageHostReply::DebuggerExecutionStepRequested { .. }
        ));
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        current = match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Paused { safe_point },
                ..
            } => safe_point,
            reply => panic!("module must remain paused until a bound span: {reply:?}"),
        };
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id: 7,
            document_generation: 1,
            metadata,
            source_id: origin.source_id + 1,
            safe_point: current,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id: 7,
            document_generation: 1,
            metadata,
            source_id: origin.source_id,
            safe_point: current,
        }),
        PageHostReply::DebuggerBlueTsSourceStepRequested { .. }
    ));
    let mut successor = None;
    for _ in 0..MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS {
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Stepping,
                ..
            } => {}
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Paused { safe_point },
                ..
            } => {
                successor = Some(safe_point);
                break;
            }
            reply => panic!("module source step did not find a new span: {reply:?}"),
        }
    }
    let successor = successor.expect("bounded module step must find another statement");
    assert_ne!(successor, current);
    assert_ne!(
        host.debugger_bluets_source_span_key(7, 1, metadata, successor)
            .unwrap(),
        Some(origin)
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if reports.len() == 1 && reports[0].outcome == PageHostScriptOutcome::Executed
    ));
}

#[test]
fn leading_bluets_classic_attaches_metadata_and_pauses_at_a_root_safe_point() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![
                    blue_ts_classic(
                        0,
                        "let first: number = 1; let deferredAnswer: number = first + 41;",
                    ),
                    classic(1, "globalThis.afterBlueTs = 2;"),
                ],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    assert_eq!(host.debug_registry.len(), 1);
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => {
            assert_eq!(programs.len(), 2);
            programs[0]
        }
        reply => panic!("expected attached BlueTS program, got {reply:?}"),
    };
    let target = match host.handle_request(PageHostRequest::ListDebuggerSafePoints {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerSafePoints { safe_points, .. } => safe_points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 0 && point.bytecode_offset != 0)
            .expect("typed classic fixture needs a non-entry root safe point"),
        reply => panic!("expected BlueTS root safe points, got {reply:?}"),
    };
    assert_eq!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
            state: PageHostDebuggerExecutionState::Pending,
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: target,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { safe_point, .. }
            if safe_point == target
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if reports.is_empty()
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::Paused { safe_point },
            ..
        } if safe_point == target
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionStepRequested { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionState {
            state: PageHostDebuggerExecutionState::Paused { safe_point },
            ..
        } if safe_point.program == program && safe_point != target
    ));
    assert_eq!(host.debug_registry.len(), 1);
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if reports == vec![
                script_report(
                    7,
                    1,
                    0,
                    PageHostScriptLanguage::BlueTs,
                    PageHostScriptKind::Classic,
                    PageHostScriptOutcome::Executed,
                ),
                script_report(
                    7,
                    1,
                    1,
                    PageHostScriptLanguage::JavaScript,
                    PageHostScriptKind::Classic,
                    PageHostScriptOutcome::Executed,
                ),
            ]
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmClosed { .. }
    ));
    assert!(host.debug_registry.is_empty());
}

#[test]
fn child_source_span_step_stops_at_the_next_bound_bluets_statement() {
    let mut host = BlueJsChildHost::default();
    let source = concat!(
        "let first: number = 1; ",
        "let middle: number = first + 1; ",
        "let last: number = middle + 1;"
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(
                1,
                vec![blue_ts_classic(0, source), classic(1, "globalThis.afterStep = 1;")],
            ),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected a BlueTS program, got {reply:?}"),
    };
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected retained BlueTS metadata, got {reply:?}"),
    };
    let (entries, source_id) = {
        let handle = host.documents[&7].debugger_programs[&program.program_handle].runtime_handle;
        let retained = host
            .debug_registry
            .get(host.runtime.program_registry(), handle)
            .unwrap();
        let entries = retained
            .safe_point_map()
            .entries
            .iter()
            .filter(|entry| entry.code_unit.ordinal() == 0)
            .cloned()
            .collect::<Vec<_>>();
        let source_id = retained
            .static_info()
            .sources
            .iter()
            .find(|source| source.module == entries[1].source)
            .unwrap()
            .id
            .0;
        (entries, source_id)
    };
    assert!(entries.len() >= 3, "fixture needs three bound root spans");
    let target_entry = &entries[1];
    let target = PageHostDebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: target_entry.bytecode_offset,
    };
    assert_ne!(target.bytecode_offset, 0);
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: target,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let request = PageHostRequest::StepDebuggerBlueTsSourceSpan {
        tab_id: 7,
        document_generation: 1,
        metadata,
        source_id,
        safe_point: target,
    };
    let wrong_source_reply = host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
        tab_id: 7,
        document_generation: 1,
        metadata,
        source_id: source_id + 1,
        safe_point: target,
    });
    assert!(
        matches!(
            wrong_source_reply,
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ),
        "{wrong_source_reply:?}"
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id: 7,
            document_generation: 1,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_generation: metadata.metadata_generation + 1,
                ..metadata
            },
            source_id,
            safe_point: target,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert_eq!(
        host.handle_request(request.clone()),
        PageHostReply::DebuggerBlueTsSourceStepRequested {
            tab_id: 7,
            document_generation: 1,
            metadata,
            source_id,
            safe_point: target,
        }
    );
    assert!(matches!(
        host.handle_request(request),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        }
    ));
    let mut successor = None;
    for _ in 0..MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS {
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        match host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        }) {
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Stepping,
                ..
            } => {}
            PageHostReply::DebuggerExecutionState {
                state: PageHostDebuggerExecutionState::Paused { safe_point },
                ..
            } => {
                successor = Some(safe_point);
                break;
            }
            reply => panic!("source step did not stop at a new bound span: {reply:?}"),
        }
    }
    let successor = successor.expect("source step must reach the third statement");
    assert_eq!(successor.bytecode_offset, entries[2].bytecode_offset);
    assert_eq!(host.debug_registry.len(), 1);
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.len() == 2
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(2, Vec::new()),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id: 7,
            document_generation: 1,
            metadata,
            source_id,
            safe_point: target,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn child_source_span_step_yields_at_its_fixed_root_instruction_limit() {
    let mut host = BlueJsChildHost::default();
    let expression = std::iter::repeat_n("first", 320)
        .collect::<Vec<_>>()
        .join(" + ");
    let source = format!(
        "let first: number = 1; let slow: number = {expression}; let last: number = slow + 1;"
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(1, vec![blue_ts_classic(0, &source)]),
        }),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
        reply => panic!("expected a BlueTS program, got {reply:?}"),
    };
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected retained BlueTS metadata, got {reply:?}"),
    };
    let (target, source_id) = {
        let handle = host.documents[&7].debugger_programs[&program.program_handle].runtime_handle;
        let retained = host
            .debug_registry
            .get(host.runtime.program_registry(), handle)
            .unwrap();
        let first_start = retained
            .safe_point_map()
            .entries
            .iter()
            .filter(|entry| entry.code_unit.ordinal() == 0)
            .map(|entry| entry.start_byte)
            .min()
            .expect("the first statement has a root entry");
        let entry = retained
            .safe_point_map()
            .entries
            .iter()
            .filter(|entry| entry.code_unit.ordinal() == 0 && entry.start_byte > first_start)
            .min_by_key(|entry| (entry.start_byte, entry.bytecode_offset))
            .expect("the long second statement must have a bound root entry");
        let source_id = retained
            .static_info()
            .sources
            .iter()
            .find(|source| source.module == entry.source)
            .unwrap()
            .id
            .0;
        (
            PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: 0,
                bytecode_offset: entry.bytecode_offset,
            },
            source_id,
        )
    };
    assert!(matches!(
        host.handle_request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 1,
            safe_point: target,
        }),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id: 7,
            document_generation: 1,
            metadata,
            source_id,
            safe_point: target,
        }),
        PageHostReply::DebuggerBlueTsSourceStepRequested { .. }
    ));
    for turn in 1..=MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS {
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
        ));
        let state = host.handle_request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 1,
            program,
        });
        if turn < MAX_BLUETS_SOURCE_STEP_ROOT_INSTRUCTIONS {
            assert!(
                matches!(
                    state,
                    PageHostReply::DebuggerExecutionState {
                        state: PageHostDebuggerExecutionState::Stepping,
                        ..
                    }
                ),
                "unexpected pre-limit state on turn {turn}: {state:?}"
            );
        } else {
            assert!(
                matches!(
                    state,
                    PageHostReply::DebuggerExecutionState {
                        state: PageHostDebuggerExecutionState::SourceStepLimitReached { safe_point },
                        ..
                    } if safe_point.program == program && safe_point != target
                ),
                "source step must yield at its exact budget: {state:?}"
            );
        }
    }
    assert!(matches!(
        host.handle_request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
            program,
        }),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.len() == 1
    ));
}

#[test]
fn a_missing_module_resolution_admits_no_partial_graph_programs() {
    let entry = "blueice://page/entry.js";
    let dependency = "blueice://page/dependency.js";
    let script = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(entry, "import './dependency.js';"),
                PageHostSource::new(dependency, "export const answer = 42;"),
            ],
        ),
    };
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(1, vec![script]),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if matches!(reports[0].outcome, PageHostScriptOutcome::Rejected { .. })
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmStats(PageHostRealmStats {
            program_count: 0,
            bytecode_bytes: 0,
            ..
        })
    ));
}

#[test]
fn document_generations_are_idempotent_and_cannot_close_a_successor() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![classic(0, "globalThis.first = true;")]),
        }),
        PageHostReply::Synchronized {
            already_current: false,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![classic(0, "throw new Error('must not replay');")]),
        }),
        PageHostReply::Synchronized {
            already_current: true,
            reports,
            ..
        } if reports.is_empty()
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![classic(0, "globalThis.second = true;")]),
        }),
        PageHostReply::Synchronized {
            already_current: false,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn source_hash_tampering_is_rejected_before_realm_program_admission() {
    let mut source = PageHostSource::new("blueice://page/main.js", "globalThis.answer = 42;");
    source.source_hash = "fnv1a64:0000000000000000".to_string();
    let script = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Classic,
        graph: graph("blueice://page/main.js", vec![source]),
    };
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(1, vec![script]),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if matches!(reports[0].outcome, PageHostScriptOutcome::Rejected { .. })
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmStats(PageHostRealmStats {
            program_count: 0,
            ..
        })
    ));
}

#[test]
fn document_source_budget_rejects_before_realm_replacement() {
    let source = PageHostSource::new(
        "blueice://page/too-large.js",
        "x".repeat(MAX_SOURCE_BYTES_PER_DOCUMENT + 1),
    );
    let script = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Classic,
        graph: graph("blueice://page/too-large.js", vec![source]),
    };
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![script]),
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::ResourceLimit,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::UnknownRealm,
            ..
        }
    ));
}

#[test]
fn duplicate_script_ordinals_are_rejected_before_realm_replacement() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    classic(0, "globalThis.first = true;"),
                    classic(0, "globalThis.second = true;"),
                ],
            ),
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::UnknownRealm,
            ..
        }
    ));
}

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

#[test]
fn private_socket_is_owner_only() {
    let path = PathBuf::from("/tmp").join(format!(
        "blueice-launcher-owner-only-test-{}.sock",
        std::process::id()
    ));
    let listener = bind_bluejs_host_socket(&path).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    drop(listener);
    let _ = fs::remove_file(path);
}
