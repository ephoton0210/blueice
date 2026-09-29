// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn real_core_negotiates_bounded_values_only_for_owner_and_client_opt_in() {
    use blueice_ipc::debugger::{
        read_debugger_reply, write_debugger_request, DebuggerMetadataCapabilityManifest,
        DebuggerReply, DebuggerRequest, DEBUGGER_PROTOCOL_VERSION,
    };

    let socket_path = unique_socket_path("value-grant-front");
    let debugger_path = unique_socket_path("value-grant-debug");
    let frame_dir =
        std::env::temp_dir().join(format!("blueice-value-grant-frames-{}", std::process::id()));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&debugger_path);
    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--debugger-socket",
            debugger_path.to_str().unwrap(),
            "--debugger-bounded-values",
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("core must start with explicit value owner policy");
    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    assert!(wait_for(&debugger_path, Duration::from_secs(5)));
    let mut frontend = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    for (requested, expected_grant) in [(false, false), (true, true)] {
        let mut debugger = connect_with_retry(&debugger_path, Duration::from_secs(5)).unwrap();
        debugger
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write_debugger_request(
            &mut debugger,
            &DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
                requested_bounded_values: requested,
            },
        )
        .unwrap();
        assert_eq!(
            read_debugger_reply(&mut debugger).unwrap(),
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
                granted_bounded_values: expected_grant,
            }
        );
    }
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!debugger_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn missing_socket_flag_exits_with_failure_and_no_socket_is_created() {
    let output = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .output()
        .expect("failed to run blueice-core");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--socket"));
}

#[test]
fn real_subprocess_routes_a_handshaken_script_connection_through_the_core_session() {
    const CURRENT_SCRIPT_CAPABILITY: &str =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PREDECESSOR_SCRIPT_CAPABILITY: &str =
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let socket_path = unique_socket_path("script-core");
    let script_socket_path = unique_socket_path("script-host");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-script-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&script_socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--script-socket",
            script_socket_path.to_str().unwrap(),
            "--script-session-token",
            CURRENT_SCRIPT_CAPABILITY,
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(
        wait_for(&socket_path, Duration::from_secs(15)),
        "blueice-core never created its frontend socket"
    );
    assert!(
        wait_for(&script_socket_path, Duration::from_secs(15)),
        "blueice-core never created its script socket"
    );
    let mut frontend = connect_with_retry(&socket_path, Duration::from_secs(5))
        .expect("failed to connect to the real core frontend socket");
    blueice_ipc::client_handshake(&mut frontend)
        .expect("the real subprocess must complete the frontend handshake");

    let mut invalid = connect_with_retry(&script_socket_path, Duration::from_secs(5))
        .expect("failed to connect an unhandshaken script client");
    blueice_ipc::script::write_script_request(
        &mut invalid,
        &blueice_ipc::script::ScriptRequest::CreateTextNode {
            target: blueice_ipc::script::ScriptDocumentTarget {
                tab_id: 1,
                document_generation: 0,
            },
            data: "must not run".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::script::read_script_reply(&mut invalid).unwrap(),
        blueice_ipc::script::ScriptReply::Error { .. }
    ));
    drop(invalid);

    let mut old_version = connect_with_retry(&script_socket_path, Duration::from_secs(5))
        .expect("failed to connect an obsolete script peer");
    blueice_ipc::script::write_script_request(
        &mut old_version,
        &blueice_ipc::script::ScriptRequest::Hello {
            protocol_version: blueice_ipc::script::SCRIPT_PROTOCOL_VERSION - 1,
            session_token: CURRENT_SCRIPT_CAPABILITY.to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::script::read_script_reply(&mut old_version).unwrap(),
        blueice_ipc::script::ScriptReply::Error { .. }
    ));
    drop(old_version);

    for capability in ["wrong", PREDECESSOR_SCRIPT_CAPABILITY] {
        let mut denied = connect_with_retry(&script_socket_path, Duration::from_secs(5))
            .expect("failed to connect a foreign script peer");
        blueice_ipc::script::write_script_request(
            &mut denied,
            &blueice_ipc::script::ScriptRequest::Hello {
                protocol_version: blueice_ipc::script::SCRIPT_PROTOCOL_VERSION,
                session_token: capability.to_string(),
            },
        )
        .unwrap();
        assert!(matches!(
            blueice_ipc::script::read_script_reply(&mut denied).unwrap(),
            blueice_ipc::script::ScriptReply::Error { .. }
        ));
        assert!(blueice_ipc::script::read_script_reply(&mut denied).is_err());
    }

    let idle = connect_with_retry(&script_socket_path, Duration::from_secs(5))
        .expect("failed to connect an idle unauthenticated peer");
    thread::sleep(Duration::from_millis(50));
    let mut script = connect_with_retry(&script_socket_path, Duration::from_secs(5))
        .expect("failed to connect the real script host");
    script
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    blueice_ipc::script::write_script_request(
        &mut script,
        &blueice_ipc::script::ScriptRequest::Hello {
            protocol_version: blueice_ipc::script::SCRIPT_PROTOCOL_VERSION,
            session_token: CURRENT_SCRIPT_CAPABILITY.to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::script::read_script_reply(&mut script).unwrap(),
        blueice_ipc::script::ScriptReply::HelloAck {
            protocol_version: blueice_ipc::script::SCRIPT_PROTOCOL_VERSION,
        }
    );
    script.set_read_timeout(None).unwrap();
    drop(idle);
    let mut script_call_id = 1;
    let target = blueice_ipc::script::ScriptDocumentTarget {
        tab_id: 1,
        document_generation: 0,
    };
    assert!(matches!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            blueice_ipc::script::ScriptRequest::CreateTextNode {
                target: blueice_ipc::script::ScriptDocumentTarget {
                    document_generation: 1,
                    ..target
                },
                data: "wrong-generation".to_string(),
            },
        ),
        blueice_ipc::script::ScriptReply::Error { .. }
    ));
    let node = match script_exchange(
        &mut script,
        &mut script_call_id,
        blueice_ipc::script::ScriptRequest::CreateTextNode {
            target,
            data: "from script socket".to_string(),
        },
    ) {
        blueice_ipc::script::ScriptReply::NodeCreated { node } => node,
        reply => panic!("expected a created script node, got {reply:?}"),
    };
    assert_eq!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            blueice_ipc::script::ScriptRequest::GetTextContent { target, node },
        ),
        blueice_ipc::script::ScriptReply::Text {
            value: "from script socket".to_string(),
        }
    );

    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: "about:blank".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated {
            url: "about:blank".to_string(),
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));
    assert!(matches!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            blueice_ipc::script::ScriptRequest::CreateTextNode {
                target,
                data: "stale socket must not mutate replacement".to_string(),
            },
        ),
        blueice_ipc::script::ScriptReply::Error { .. }
    ));
    assert!(matches!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            blueice_ipc::script::ScriptRequest::CreateTextNode {
                target: blueice_ipc::script::ScriptDocumentTarget {
                    document_generation: 1,
                    ..target
                },
                data: "new document".to_string(),
            },
        ),
        blueice_ipc::script::ScriptReply::NodeCreated { .. }
    ));
    drop(script);
    for invalid_call in [
        blueice_ipc::script::ScriptRequest::Call {
            request_id: 0,
            request: Box::new(blueice_ipc::script::ScriptRequest::CreateTextNode {
                target,
                data: "bad call ID".to_string(),
            }),
        },
        blueice_ipc::script::ScriptRequest::Call {
            request_id: 1,
            request: Box::new(blueice_ipc::script::ScriptRequest::Call {
                request_id: 1,
                request: Box::new(blueice_ipc::script::ScriptRequest::CreateTextNode {
                    target,
                    data: "nested call".to_string(),
                }),
            }),
        },
    ] {
        let mut invalid = connect_with_retry(&script_socket_path, Duration::from_secs(5))
            .expect("failed to connect a malformed script caller");
        invalid
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        blueice_ipc::script::write_script_request(
            &mut invalid,
            &blueice_ipc::script::ScriptRequest::Hello {
                protocol_version: blueice_ipc::script::SCRIPT_PROTOCOL_VERSION,
                session_token: CURRENT_SCRIPT_CAPABILITY.to_string(),
            },
        )
        .unwrap();
        assert!(matches!(
            blueice_ipc::script::read_script_reply(&mut invalid).unwrap(),
            blueice_ipc::script::ScriptReply::HelloAck { .. }
        ));
        blueice_ipc::script::write_script_request(&mut invalid, &invalid_call).unwrap();
        assert!(matches!(
            blueice_ipc::script::read_script_reply(&mut invalid).unwrap(),
            blueice_ipc::script::ScriptReply::Error { .. }
        ));
        assert!(blueice_ipc::script::read_script_reply(&mut invalid).is_err());
    }

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    let status = child
        .wait()
        .expect("failed to wait for blueice-core to exit");
    assert!(
        status.success(),
        "blueice-core must exit cleanly after Shutdown"
    );
    assert!(
        !script_socket_path.exists(),
        "blueice-core must remove its script socket on exit"
    );
    assert!(
        !frame_dir.exists(),
        "blueice-core must remove its script frame directory on exit"
    );

    // Reuse the pathname under a new core process to prove that the previous
    // generation's capability cannot authenticate to its successor. The
    // launcher uses a fresh random value (and a fresh pathname) on cutover;
    // this deliberately stronger path-reuse fixture isolates the token check.
    let successor_capability = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    let mut successor = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--script-socket",
            script_socket_path.to_str().unwrap(),
            "--script-session-token",
            successor_capability,
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn successor blueice-core");
    assert!(wait_for(&script_socket_path, Duration::from_secs(15)));
    let mut predecessor = connect_with_retry(&script_socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::script::write_script_request(
        &mut predecessor,
        &blueice_ipc::script::ScriptRequest::Hello {
            protocol_version: blueice_ipc::script::SCRIPT_PROTOCOL_VERSION,
            session_token: CURRENT_SCRIPT_CAPABILITY.to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::script::read_script_reply(&mut predecessor).unwrap(),
        blueice_ipc::script::ScriptReply::Error { .. }
    ));
    assert!(blueice_ipc::script::read_script_reply(&mut predecessor).is_err());

    let mut current = connect_with_retry(&script_socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::script::write_script_request(
        &mut current,
        &blueice_ipc::script::ScriptRequest::Hello {
            protocol_version: blueice_ipc::script::SCRIPT_PROTOCOL_VERSION,
            session_token: successor_capability.to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::script::read_script_reply(&mut current).unwrap(),
        blueice_ipc::script::ScriptReply::HelloAck {
            protocol_version: blueice_ipc::script::SCRIPT_PROTOCOL_VERSION,
        }
    );
    let mut successor_frontend = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut successor_frontend).unwrap();
    blueice_ipc::write_client_message(
        &mut successor_frontend,
        &blueice_ipc::ClientMessage::Shutdown,
    )
    .unwrap();
    assert!(successor.wait().unwrap().success());
    assert!(!script_socket_path.exists());
}

#[test]
fn real_script_socket_denials_preserve_each_live_dom_across_tabs_navigation_and_core_replacement() {
    use blueice_ipc::script::{
        ScriptDocumentTarget, ScriptReply, ScriptRequest, SCRIPT_PROTOCOL_VERSION,
    };

    const FIRST_CAPABILITY: &str =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SUCCESSOR_CAPABILITY: &str =
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    let socket_path = unique_socket_path("sd-core");
    let script_path = unique_socket_path("sd-host");
    let frame_dir = std::env::temp_dir().join(format!("bicb-script-denial-{}", std::process::id()));
    let gatekeeper_path = clearing_gatekeeper("sd-gk");
    let (first_addr, first_server) =
        serve_html_once("<main id=\"root\"><span id=\"label\">first</span></main>");
    let (second_addr, second_server) =
        serve_html_once("<main id=\"root\"><span id=\"label\">second</span></main>");
    let (replacement_addr, replacement_server) =
        serve_html_once("<main><span id=\"label\">replacement</span></main>");
    let (successor_addr, successor_server) =
        serve_html_once("<main><span id=\"label\">successor</span></main>");
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&script_path);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let spawn_core = |capability: &str| {
        Command::new(env!("CARGO_BIN_EXE_blueice-core"))
            .args([
                "--socket",
                socket_path.to_str().unwrap(),
                "--script-socket",
                script_path.to_str().unwrap(),
                "--script-session-token",
                capability,
                "--frame-dir",
                frame_dir.to_str().unwrap(),
                "--gatekeeper-socket",
                gatekeeper_path.to_str().unwrap(),
            ])
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let mut first_core = spawn_core(FIRST_CAPABILITY);
    assert!(wait_for(&socket_path, Duration::from_secs(15)));
    assert!(wait_for(&script_path, Duration::from_secs(15)));
    let mut frontend = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    frontend
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    navigate_default_tab(&mut frontend, format!("http://{first_addr}"));
    let first_dom = read_tab_dom(&mut frontend, 1, 1);
    assert!(first_dom.contains("first"));

    let mut script = connect_with_retry(&script_path, Duration::from_secs(5)).unwrap();
    let mut script_call_id = 1;
    assert_eq!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            ScriptRequest::Hello {
                protocol_version: SCRIPT_PROTOCOL_VERSION,
                session_token: FIRST_CAPABILITY.to_string(),
            }
        ),
        ScriptReply::HelloAck {
            protocol_version: SCRIPT_PROTOCOL_VERSION
        }
    );
    let first_target = ScriptDocumentTarget {
        tab_id: 1,
        document_generation: 1,
    };
    let first_node = match script_exchange(
        &mut script,
        &mut script_call_id,
        ScriptRequest::GetElementById {
            target: first_target,
            id: "label".to_string(),
        },
    ) {
        ScriptReply::Node { node: Some(node) } => node,
        reply => panic!("expected the first live label, got {reply:?}"),
    };

    blueice_ipc::write_client_message_with_ids(
        &mut frontend,
        None,
        Some(20),
        &blueice_ipc::ClientMessage::OpenTab {
            url: Some(format!("http://{second_addr}")),
        },
    )
    .unwrap();
    let second_tab = loop {
        let (_, request_id, reply) =
            blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
        if request_id == Some(20) {
            let blueice_ipc::ServerMessage::TabOpened { tab_id, .. } = reply else {
                panic!("expected a second tab, got {reply:?}");
            };
            break tab_id;
        }
        assert!(matches!(
            reply,
            blueice_ipc::ServerMessage::FrameReady { .. }
        ));
    };
    assert_ne!(second_tab, 1);
    let second_dom = read_tab_dom(&mut frontend, second_tab, 2);
    assert!(second_dom.contains("second"));
    let second_target = ScriptDocumentTarget {
        tab_id: second_tab,
        document_generation: 1,
    };
    let first_parent = match script_exchange(
        &mut script,
        &mut script_call_id,
        ScriptRequest::GetElementById {
            target: first_target,
            id: "root".to_string(),
        },
    ) {
        ScriptReply::Node { node: Some(node) } => node,
        reply => panic!("expected first page parent, got {reply:?}"),
    };
    let create_child = |script: &mut UnixStream, call_id: &mut u64, target| match script_exchange(
        script,
        call_id,
        ScriptRequest::CreateElement {
            target,
            tag_name: "aside".to_string(),
        },
    ) {
        ScriptReply::NodeCreated { node } => node,
        reply => panic!("expected detached child, got {reply:?}"),
    };
    let first_child = create_child(&mut script, &mut script_call_id, first_target);
    let second_child = create_child(&mut script, &mut script_call_id, second_target);
    assert_ne!(
        first_child, second_child,
        "two real pages need distinct handles"
    );
    assert!(matches!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            ScriptRequest::AppendChild {
                target: first_target,
                parent: first_parent,
                child: second_child,
            },
        ),
        ScriptReply::Error { .. }
    ));
    assert_eq!(read_tab_dom(&mut frontend, 1, 21), first_dom);
    assert_eq!(read_tab_dom(&mut frontend, second_tab, 22), second_dom);
    assert_eq!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            ScriptRequest::ValidateNode {
                target: first_target,
                node: first_child,
            },
        ),
        ScriptReply::Ack,
        "a rejected foreign append must not consume a local detached child"
    );
    assert!(matches!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            ScriptRequest::SetTextContent {
                target: ScriptDocumentTarget {
                    tab_id: second_tab,
                    document_generation: 0
                },
                node: first_node,
                value: "CROSS_TAB_POISON".to_string(),
            }
        ),
        ScriptReply::Error { .. }
    ));
    assert_eq!(read_tab_dom(&mut frontend, 1, 3), first_dom);
    assert_eq!(read_tab_dom(&mut frontend, second_tab, 4), second_dom);

    assert!(matches!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            ScriptRequest::SetTextContent {
                target: ScriptDocumentTarget {
                    tab_id: 1,
                    document_generation: 0
                },
                node: first_node,
                value: "STALE_GENERATION_POISON".to_string(),
            }
        ),
        ScriptReply::Error { .. }
    ));
    assert_eq!(read_tab_dom(&mut frontend, 1, 5), first_dom);
    let root = match script_exchange(
        &mut script,
        &mut script_call_id,
        ScriptRequest::GetElementById {
            target: first_target,
            id: "root".to_string(),
        },
    ) {
        ScriptReply::Node { node: Some(node) } => node,
        reply => panic!("expected the first document root element, got {reply:?}"),
    };
    let detached = match script_exchange(
        &mut script,
        &mut script_call_id,
        ScriptRequest::CreateElement {
            target: first_target,
            tag_name: "aside".to_string(),
        },
    ) {
        ScriptReply::NodeCreated { node } => node,
        reply => panic!("expected a detached node, got {reply:?}"),
    };
    for node in [root, first_node, detached] {
        assert_eq!(
            script_exchange(
                &mut script,
                &mut script_call_id,
                ScriptRequest::ValidateNode {
                    target: first_target,
                    node,
                },
            ),
            ScriptReply::Ack
        );
    }
    assert_eq!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            ScriptRequest::SetTextContent {
                target: first_target,
                node: root,
                value: "replaced subtree".to_string(),
            },
        ),
        ScriptReply::Ack
    );
    assert!(matches!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            ScriptRequest::ValidateNode {
                target: first_target,
                node: first_node,
            },
        ),
        ScriptReply::Error { .. }
    ));
    assert_eq!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            ScriptRequest::ValidateNode {
                target: first_target,
                node: detached,
            },
        ),
        ScriptReply::Ack
    );
    assert!(read_tab_dom(&mut frontend, 1, 51).contains("replaced subtree"));
    navigate_default_tab(&mut frontend, format!("http://{replacement_addr}"));
    let replacement_dom = read_tab_dom(&mut frontend, 1, 6);
    assert!(replacement_dom.contains("replacement"));
    assert!(matches!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            ScriptRequest::ValidateNode {
                target: first_target,
                node: root,
            },
        ),
        ScriptReply::Error { .. }
    ));
    assert!(matches!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            ScriptRequest::SetTextContent {
                target: first_target,
                node: first_node,
                value: "OLD_DOCUMENT_POISON".to_string(),
            }
        ),
        ScriptReply::Error { .. }
    ));
    assert!(matches!(
        script_exchange(
            &mut script,
            &mut script_call_id,
            ScriptRequest::CreateTextNode {
                target: first_target,
                data: "OLD_CREATE_POISON".to_string(),
            }
        ),
        ScriptReply::Error { .. }
    ));
    assert_eq!(read_tab_dom(&mut frontend, 1, 7), replacement_dom);
    assert_eq!(read_tab_dom(&mut frontend, second_tab, 8), second_dom);
    drop(script);
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(first_core.wait().unwrap().success());
    assert!(!script_path.exists());

    let mut successor = spawn_core(SUCCESSOR_CAPABILITY);
    assert!(wait_for(&script_path, Duration::from_secs(15)));
    let mut successor_frontend = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    successor_frontend
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    blueice_ipc::client_handshake(&mut successor_frontend).unwrap();
    navigate_default_tab(&mut successor_frontend, format!("http://{successor_addr}"));
    let successor_dom = read_tab_dom(&mut successor_frontend, 1, 9);
    assert!(successor_dom.contains("successor"));
    let successor_target = ScriptDocumentTarget {
        tab_id: 1,
        document_generation: 1,
    };
    let mut current = connect_with_retry(&script_path, Duration::from_secs(5)).unwrap();
    let mut current_call_id = 1;
    assert_eq!(
        script_exchange(
            &mut current,
            &mut current_call_id,
            ScriptRequest::Hello {
                protocol_version: SCRIPT_PROTOCOL_VERSION,
                session_token: SUCCESSOR_CAPABILITY.to_string(),
            }
        ),
        ScriptReply::HelloAck {
            protocol_version: SCRIPT_PROTOCOL_VERSION
        }
    );
    let successor_node = match script_exchange(
        &mut current,
        &mut current_call_id,
        ScriptRequest::GetElementById {
            target: successor_target,
            id: "label".to_string(),
        },
    ) {
        ScriptReply::Node { node: Some(node) } => node,
        reply => panic!("expected the successor label, got {reply:?}"),
    };
    drop(current);

    let mut predecessor = connect_with_retry(&script_path, Duration::from_secs(5)).unwrap();
    let mut queued = Vec::new();
    blueice_ipc::script::write_script_request(
        &mut queued,
        &ScriptRequest::Hello {
            protocol_version: SCRIPT_PROTOCOL_VERSION,
            session_token: FIRST_CAPABILITY.to_string(),
        },
    )
    .unwrap();
    blueice_ipc::script::write_script_request(
        &mut queued,
        &ScriptRequest::SetTextContent {
            target: successor_target,
            node: successor_node,
            value: "OLD_CORE_POISON".to_string(),
        },
    )
    .unwrap();
    predecessor.write_all(&queued).unwrap();
    assert!(matches!(
        blueice_ipc::script::read_script_reply(&mut predecessor).unwrap(),
        ScriptReply::Error { .. }
    ));
    assert!(blueice_ipc::script::read_script_reply(&mut predecessor).is_err());
    assert_eq!(read_tab_dom(&mut successor_frontend, 1, 10), successor_dom);

    let mut current = connect_with_retry(&script_path, Duration::from_secs(5)).unwrap();
    let mut current_call_id = 1;
    assert_eq!(
        script_exchange(
            &mut current,
            &mut current_call_id,
            ScriptRequest::Hello {
                protocol_version: SCRIPT_PROTOCOL_VERSION,
                session_token: SUCCESSOR_CAPABILITY.to_string(),
            }
        ),
        ScriptReply::HelloAck {
            protocol_version: SCRIPT_PROTOCOL_VERSION
        }
    );
    assert_eq!(
        script_exchange(
            &mut current,
            &mut current_call_id,
            ScriptRequest::GetTextContent {
                target: successor_target,
                node: successor_node,
            }
        ),
        ScriptReply::Text {
            value: "successor".to_string()
        }
    );
    assert_eq!(
        script_exchange(
            &mut current,
            &mut current_call_id,
            ScriptRequest::SetTextContent {
                target: successor_target,
                node: successor_node,
                value: "authorized change".to_string(),
            }
        ),
        ScriptReply::Ack
    );
    let changed = read_tab_dom(&mut successor_frontend, 1, 11);
    assert_ne!(changed, successor_dom);
    assert!(changed.contains("authorized change"));

    blueice_ipc::write_client_message(
        &mut successor_frontend,
        &blueice_ipc::ClientMessage::Shutdown,
    )
    .unwrap();
    assert!(successor.wait().unwrap().success());
    assert!(!script_path.exists());
    first_server.join().unwrap();
    second_server.join().unwrap();
    replacement_server.join().unwrap();
    successor_server.join().unwrap();
}
