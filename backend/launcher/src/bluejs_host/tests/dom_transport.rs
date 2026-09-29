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
