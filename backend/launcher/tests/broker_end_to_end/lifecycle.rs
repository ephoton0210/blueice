// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn two_clients_through_the_same_launcher_observe_the_same_render_pass() {
    let _guard = broker_test_guard();
    let mut launcher = Launcher::spawn();

    // Two independent connections to the *one* rendezvous socket,
    // standing in for a human's `frontend` and an AI's `mcp-server`.
    let mut human = launcher.connect();
    let mut ai = launcher.connect();

    // Navigate to a real (built-in, no network needed) page first --
    // `blueice-core` starts with a completely blank `Page`, whose
    // rendered content has no boxes at all, so its cropped-to-viewport
    // pixmap width would be clamped to a content-derived minimum rather
    // than actually reflecting the resize below.
    write_client_message(
        &mut human,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    let navigated = read_server_message(&mut human).unwrap();
    assert!(matches!(navigated, ServerMessage::Navigated { .. }));
    let initial_frame = read_server_message(&mut human).unwrap();
    assert!(matches!(initial_frame, ServerMessage::FrameReady { .. }));
    // The other connection must see this initial navigation too.
    assert!(matches!(
        read_server_message(&mut ai).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        read_server_message(&mut ai).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    // The AI's action (an ActOn-free, plain Resize here for simplicity)
    // must produce a FrameReady the *human* connection also receives,
    // even though the human connection never sent anything itself --
    // this is the actual "same render pass" property, not just "both
    // connections work independently."
    write_client_message(
        &mut ai,
        &ClientMessage::Resize {
            width: 111,
            height: 222,
        },
    )
    .unwrap();

    let ai_frame = read_server_message(&mut ai).unwrap();
    let ServerMessage::FrameReady {
        generation: ai_generation,
        width: 111,
        height: 222,
        ..
    } = ai_frame
    else {
        panic!("expected FrameReady, got {ai_frame:?}")
    };

    let human_frame = read_server_message(&mut human).unwrap();
    let ServerMessage::FrameReady {
        generation: human_generation,
        width: 111,
        height: 222,
        ..
    } = human_frame
    else {
        panic!("expected the human connection to also see the resize's FrameReady, got {human_frame:?}")
    };
    assert_eq!(
        ai_generation, human_generation,
        "both connections must see the identical generation for the same state change"
    );

    // Now the *human* connection acts (Scroll), and the AI connection
    // -- via GetRepresentation -- must report that exact same new
    // generation, proving the sharing holds in both directions.
    write_client_message(&mut human, &ClientMessage::Scroll { delta_y: 5.0 }).unwrap();
    let human_frame2 = read_server_message(&mut human).unwrap();
    let ServerMessage::FrameReady {
        generation: scroll_generation,
        ..
    } = human_frame2
    else {
        panic!("expected FrameReady, got {human_frame2:?}")
    };
    assert!(scroll_generation > human_generation);

    // Drain the same broadcast off the AI connection first (it must see
    // it too, unprompted).
    let ai_frame2 = read_server_message(&mut ai).unwrap();
    assert!(
        matches!(ai_frame2, ServerMessage::FrameReady { generation, .. } if generation == scroll_generation)
    );

    write_client_message(&mut ai, &ClientMessage::GetRepresentation).unwrap();
    let reply = read_server_message(&mut ai).unwrap();
    let ServerMessage::Representation(snapshot) = reply else {
        panic!("expected Representation, got {reply:?}")
    };
    assert_eq!(
        snapshot.generation, scroll_generation,
        "GetRepresentation must reflect the state the *other* connection's action just produced"
    );

    // This broadcasts to *human* too, same as every other reply in this
    // test -- drain it the same way a real always-reading client
    // (`frontend`'s own background reader thread) would, rather than
    // leaving it unread: `about:credits`'s `Representation` is large
    // enough that leaving it sitting in the kernel socket buffer can
    // exhaust it, and `register_client`'s bounded write timeout would
    // then have to kick in as `broadcast_core_to_clients` blocks
    // delivering it -- correct, but a needless multi-second stall this
    // test doesn't need to exercise.
    let human_reply = read_server_message(&mut human).unwrap();
    assert!(matches!(human_reply, ServerMessage::Representation(_)));

    // `Shutdown` from *either* client is forwarded into the one shared
    // `core` connection like any other message, ending `core`'s own
    // session loop; that in turn closes the launcher's internal
    // connection to it, ending the broadcaster loop and letting
    // `main()` return and exit normally -- proving the cascade, and
    // letting the process exit on its own rather than being killed.
    write_client_message(&mut human, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
}

#[test]
fn launcher_starts_a_private_gatekeeper_that_blocks_a_flagged_url_before_fetch() {
    let mut launcher = Launcher::spawn();
    let mut client = launcher.connect();

    // `malware.test` is a deterministic local rule-base entry. If the
    // launcher forgot to spawn/wire its private gatekeeper, this would
    // fail closed as an unavailable checker instead; matching the rule
    // category proves the actual supervised process answered the URL
    // review before core attempted any network fetch.
    write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "https://malware.test/launcher-proof".to_string(),
        },
    )
    .unwrap();
    match read_server_message(&mut client).unwrap() {
        ServerMessage::GatekeeperBlocked { category, .. } => {
            assert_eq!(category, "known-bad-domain");
        }
        other => {
            panic!("expected the launcher-managed gatekeeper to block before fetch, got {other:?}")
        }
    }

    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
}

#[test]
fn opt_in_launcher_supervises_the_private_bluejs_child_for_core_page_execution() {
    let _guard = broker_test_guard();
    // This is intentionally the real launcher binary, not a direct core
    // invocation or a hand-configured host socket. `--out-of-process-bluejs`
    // is the only opt-in; the launcher itself creates the child endpoint and
    // capability token, hands them directly to core, and retains supervision.
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .expect("fixture must receive HTTP request");
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request);
        let body = concat!(
            "<main>launcher-to-core-to-child</main>",
            "<script>if (typeof document !== 'undefined' || typeof fetch !== 'undefined') throw 'host binding leaked'; globalThis.answer = 42;</script>",
            "<script type=\"application/x-blueice-typescript\">const sharedAnswer: number = 42;</script>",
            "<script>if (sharedAnswer !== 42) throw 'BlueTS did not share this realm';</script>",
            "<script type=\"module\">export const moduleAnswer = 43;</script>",
            "<script src=\"untrusted.js\"></script>",
            "<script type=\"application/x-blueice-typescript-module\">export const typedModuleAnswer: number = 44;</script>",
            "<script type=\"application/x-blueice-typescript\">blueiceDocumentText(1);</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .expect("fixture must return page document");
    });

    let mut launcher = Launcher::spawn_with_supervised_bluejs(&gatekeeper_socket);
    let private_socket = launcher
        .private_bluejs_socket
        .clone()
        .expect("the opted-in launcher owns exactly one private child socket");
    assert!(
        private_socket.exists(),
        "the supervised child must be ready before launcher exposes its rendezvous socket"
    );

    let mut frontend = launcher.connect();
    write_client_message(&mut frontend, &ClientMessage::Navigate { url: url.clone() }).unwrap();
    assert_eq!(
        read_server_message(&mut frontend).unwrap(),
        ServerMessage::Navigated { url }
    );
    assert!(matches!(
        read_server_message(&mut frontend).unwrap(),
        ServerMessage::FrameReady { generation: 1, .. }
    ));
    write_client_message(&mut frontend, &ClientMessage::GetBlueJsScriptReports).unwrap();
    let reports = read_server_message(&mut frontend).unwrap();
    let public_representation = format!("{reports:?}");
    assert!(
        !public_representation.contains(&private_socket.display().to_string())
            && !public_representation.contains("out-of-process-bluejs"),
        "the frontend broker must not reflect the launch-only child endpoint or capability"
    );
    assert_eq!(
        reports,
        ServerMessage::BlueJsScriptReports(vec![
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 0,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 2,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 3,
                kind: blueice_ipc::BlueJsScriptKind::Module,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 4,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Rejected {
                    category: "external JavaScript declarations require an authorized loader"
                        .to_string(),
                },
            },
        ])
    );
    write_client_message(&mut frontend, &ClientMessage::GetBlueTsScriptReports).unwrap();
    let reply = read_server_message(&mut frontend).unwrap();
    let ServerMessage::BlueTsScriptReports(reports) = reply else {
        panic!("expected BlueTS script reports: {reply:?}");
    };
    let position = reports[2]
        .source_position
        .expect("an inline compiler rejection has a verified source position");
    assert!(position.start < position.end);
    assert!(position.end as usize <= "blueiceDocumentText(1);".len());
    let mut without_positions = reports;
    for report in &mut without_positions {
        report.source_position = None;
    }
    assert_eq!(
        without_positions,
        vec![
            blueice_ipc::BlueTsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 1,
                kind: blueice_ipc::BlueTsScriptKind::Classic,
                policy: blueice_ipc::BlueTsScriptRuntimePolicy::Checked,
                source_position: None,
                outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueTsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 5,
                kind: blueice_ipc::BlueTsScriptKind::Module,
                policy: blueice_ipc::BlueTsScriptRuntimePolicy::Checked,
                source_position: None,
                outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueTsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 6,
                kind: blueice_ipc::BlueTsScriptKind::Classic,
                policy: blueice_ipc::BlueTsScriptRuntimePolicy::Checked,
                source_position: None,
                outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Rejected {
                    category: "BlueTS compilation rejected the page script".to_string(),
                },
            },
        ]
    );

    // The only public operation here is normal frontend shutdown. It neither
    // knows the child capability token nor has a route to the child socket.
    write_client_message(&mut frontend, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
    assert!(
        !private_socket.exists(),
        "launcher teardown must reap its private child and unlink its socket"
    );
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn owner_http_manifest_admits_closed_page_graphs_and_rejects_unlisted_or_tampered_sources() {
    let _guard = broker_test_guard();
    const CLASSIC: &str = "globalThis.authorizedExternal = 42;";
    const ENTRY: &str = "import { answer } from './dep.ts'; export const result: number = answer;";
    const DEPENDENCY: &str = "export const answer: number = 42;";
    const BAD_HASH: &str = "globalThis.untrustedExternal = true;";
    const DOCUMENT: &str = concat!(
        "<main>owner-manifest-page</main>",
        "<script src=\"/allowed.js\"></script>",
        "<script type=\"application/x-blueice-typescript-module\" src=\"/entry.ts\"></script>",
        "<script src=\"/unlisted.js\"></script>",
        "<script src=\"/bad-hash.js\"></script>"
    );

    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let policy = OwnerHttpPolicyBootstrap {
        origin_rule: OwnerHttpOriginRule::SameDocumentOrigin,
        resources: vec![
            OwnerHttpResource {
                canonical_url: format!("{origin}/allowed.js"),
                integrity:
                    "sha256:5a3082683c8778aefe6229bda5a9b2acc43f717b214b67b2a37d8efcc726f70f".into(),
            },
            OwnerHttpResource {
                canonical_url: format!("{origin}/entry.ts"),
                integrity:
                    "sha256:4bd26f93084252bb985514f7b81e88c54f75969fd860bf600b2626c3c60bd55d".into(),
            },
            OwnerHttpResource {
                canonical_url: format!("{origin}/dep.ts"),
                integrity:
                    "sha256:26a8680bbf0c861168714585a9facbb8dda5378b24d70451e9cb11495e71d37a".into(),
            },
            OwnerHttpResource {
                canonical_url: format!("{origin}/bad-hash.js"),
                integrity: format!("sha256:{}", "0".repeat(64)),
            },
        ],
    };
    let policy_file = unique_path("owner-http-policy.json");
    std::fs::write(&policy_file, serde_json::to_vec(&policy).unwrap()).unwrap();
    let server = thread::spawn(move || {
        let mut requested = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(15);
        while requested.len() < 5 && Instant::now() < deadline {
            let Ok((mut stream, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(10));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0u8; 2048];
            let count = stream.read(&mut request).unwrap();
            let request = std::str::from_utf8(&request[..count]).unwrap();
            let path = request
                .split_whitespace()
                .nth(1)
                .expect("HTTP fixture request must contain a path")
                .to_string();
            let (mime, body) = match path.as_str() {
                "/" => ("text/html", DOCUMENT),
                "/allowed.js" => ("text/javascript", CLASSIC),
                "/entry.ts" => ("text/typescript", ENTRY),
                "/dep.ts" => ("text/typescript", DEPENDENCY),
                "/bad-hash.js" => ("text/javascript", BAD_HASH),
                _ => panic!("an unlisted resource was fetched: {path}"),
            };
            requested.push(path);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
        requested
    });

    let mut launcher = Launcher::spawn_with_supervised_bluejs_and_owner_http_policy(
        &gatekeeper_socket,
        &policy_file,
    );
    let private_socket = launcher.private_bluejs_socket.clone().unwrap();
    let mut frontend = launcher.connect();
    let url = format!("{origin}/");
    write_client_message(&mut frontend, &ClientMessage::Navigate { url: url.clone() }).unwrap();
    assert_eq!(
        read_server_message(&mut frontend).unwrap(),
        ServerMessage::Navigated { url }
    );
    assert!(matches!(
        read_server_message(&mut frontend).unwrap(),
        ServerMessage::FrameReady { generation: 1, .. }
    ));
    write_client_message(&mut frontend, &ClientMessage::GetBlueJsScriptReports).unwrap();
    let javascript_reports = read_server_message(&mut frontend).unwrap();
    write_client_message(&mut frontend, &ClientMessage::GetBlueTsScriptReports).unwrap();
    let bluets_reports = read_server_message(&mut frontend).unwrap();
    let mut requested = server.join().unwrap();
    requested.sort();
    assert_eq!(
        requested,
        ["/", "/allowed.js", "/bad-hash.js", "/dep.ts", "/entry.ts"]
    );
    assert_eq!(
        javascript_reports,
        ServerMessage::BlueJsScriptReports(vec![
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 0,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 2,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Rejected {
                    category: "external JavaScript source authorization rejected the page script"
                        .into(),
                },
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 3,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Rejected {
                    category: "external JavaScript source authorization rejected the page script"
                        .into(),
                },
            },
        ])
    );
    assert_eq!(
        bluets_reports,
        ServerMessage::BlueTsScriptReports(vec![blueice_ipc::BlueTsScriptExecutionReport {
            tab_id: 1,
            document_generation: 1,
            ordinal: 1,
            kind: blueice_ipc::BlueTsScriptKind::Module,
            policy: blueice_ipc::BlueTsScriptRuntimePolicy::Checked,
            source_position: None,
            outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Executed,
        }])
    );

    write_client_message(&mut frontend, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
    assert!(!private_socket.exists());
    let _ = std::fs::remove_file(gatekeeper_socket);
    let _ = std::fs::remove_file(policy_file);
}
