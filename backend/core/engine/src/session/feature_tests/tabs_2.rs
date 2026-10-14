// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn a_message_addressed_to_an_unknown_tab_replies_error_not_a_silent_no_op() {
    let dir = temp_frame_dir("unknown-tab-error");
    let gatekeeper = clearing_gatekeeper("unknown-tab-error");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(999_999),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(
        reply_tab,
        Some(999_999),
        "the reply should still echo back which (nonexistent) tab was addressed"
    );
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected Error, got {reply:?}"
    );

    // The session must survive an unknown-tab error, same as every
    // other error case in this file.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Tabs(_)
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn close_tab_removes_it_and_a_later_message_to_it_becomes_an_error() {
    let dir = temp_frame_dir("close-tab");
    let gatekeeper = clearing_gatekeeper("close-tab");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened { tab_id: new_id, .. } =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::CloseTab,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(new_id));
    assert_eq!(reply, ServerMessage::TabClosed { tab_id: new_id });

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "a closed tab's id must no longer resolve, expected Error, got {reply:?}"
    );

    // Closing again is a harmless-but-reported "unknown tab" error,
    // not a panic or a second TabClosed.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::CloseTab,
    )
    .unwrap();
    let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::Error { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_reply_to_an_untagged_request_still_echoes_the_resolved_default_tab_id() {
    // The load-bearing property that makes broadcast-shared,
    // multi-tab connections work at all: a request that left
    // `tab_id` implicit still gets a reply that self-discloses the
    // *concrete* tab it resolved to, not `None` -- otherwise a
    // second client sharing the connection via `blueice-launcher`'s
    // broker could never tell which tab an untagged client's
    // broadcasted reply was actually about.
    let dir = temp_frame_dir("echo-resolved-default-tab");
    let gatekeeper = clearing_gatekeeper("echo-resolved-default-tab");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(tabs) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected Tabs")
    };
    let default_tab_id = tabs[0].id;

    // Sent with no tab_id at all -- the envelope-level default.
    blueice_ipc::write_client_message_with_id(&mut client, None, &ClientMessage::GetRepresentation)
        .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(
        reply_tab,
        Some(default_tab_id),
        "the reply must echo the resolved tab, not None"
    );
    assert!(matches!(reply, ServerMessage::Representation(_)));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_stalled_gatekeeper_check_for_one_tab_does_not_block_a_reply_to_another_tab() {
    // The single most important proof of the property this whole
    // mechanism exists for: a slow/stuck gatekeeper review for one
    // tab must never stall the one shared connection other tabs
    // (or clients sharing it via `blueice-launcher`'s broker) are
    // also using.
    let dir = temp_frame_dir("non-blocking-concurrency");
    let gatekeeper_path = unique_gatekeeper_socket_path("non-blocking-concurrency");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            thread::spawn(move || {
                if let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) {
                    if matches!(&req, blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { url } if url.contains("slow-tab"))
                    {
                        thread::sleep(Duration::from_millis(300));
                    }
                    let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(
                        &mut stream,
                        &blueice_ipc::gatekeeper::GatekeeperReply::Cleared,
                    );
                }
            });
        }
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper_path,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened { tab_id: tab_b, .. } =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };

    // Kick off the default tab's navigation, whose gatekeeper check
    // stalls for 300ms -- fire-and-forget, its own reply isn't
    // waited on here.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://127.0.0.1:1/slow-tab".to_string(),
        },
    )
    .unwrap();

    // Immediately address tab_b with an unrelated message.
    let start = Instant::now();
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_b),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_b));
    assert!(matches!(reply, ServerMessage::Representation(_)));
    assert!(
        start.elapsed() < Duration::from_millis(150),
        "tab_b's reply must arrive well before tab_a's stalled gatekeeper check resolves, took {:?}",
        start.elapsed()
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn open_tab_with_a_url_the_gatekeeper_blocks_replies_gatekeeper_blocked_not_tab_opened() {
    // `OpenTab{url: Some(_)}` goes through the same gated path
    // `Navigate` does (`PendingKind::OpenTab`) -- this is the
    // `OpenTab`-specific proof that a blocked outcome there reports
    // `GatekeeperBlocked`, not a bare `TabOpened`/`Error`, and that
    // no orphaned-but-blank tab id is leaked into a reply shape a
    // caller wouldn't expect.
    let dir = temp_frame_dir("open-tab-gatekeeper-blocked");
    let gatekeeper_path = unique_gatekeeper_socket_path("open-tab-gatekeeper-blocked");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let Ok(_req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) else {
                continue;
            };
            let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(
                &mut stream,
                &blueice_ipc::gatekeeper::GatekeeperReply::Rejected {
                    reason: "known-bad domain".to_string(),
                    category: "blocklist".to_string(),
                },
            );
        }
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper_path,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);

    let url = "http://example.invalid/".to_string();
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::OpenTab {
            url: Some(url.clone()),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(
        reply,
        ServerMessage::GatekeeperBlocked {
            reason: "known-bad domain".to_string(),
            category: "blocklist".to_string(),
            url
        }
    );

    // The session must still be alive afterward, same as every
    // other error/blocked case in this file -- and `ListTabs` must
    // still show the new (blank) tab `OpenTab` always creates,
    // per `ServerMessage::TabOpened`'s own documented limitation.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(tabs) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected Tabs")
    };
    assert_eq!(
        tabs.len(),
        2,
        "OpenTab always creates the tab, even though its requested navigation was blocked"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_non_navigating_message_to_a_tab_with_a_pending_navigation_applies_immediately() {
    // `phase-7-local-ai/PLAN.md`'s "Wiring design" is explicit that
    // this must work the way a real browser reflows/scrolls a
    // still-displayed old page while a new one loads: `Resize`
    // addressed to a tab whose gated navigation hasn't resolved yet
    // must apply immediately against that tab's *current*
    // (pre-navigation) `Page` state, not queue up behind it.
    let dir = temp_frame_dir("resize-during-pending-nav");
    let gatekeeper_path = unique_gatekeeper_socket_path("resize-during-pending-nav");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            thread::spawn(move || {
                if let Ok(_req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) {
                    // Stalls every stage, so the navigation this
                    // test kicks off never resolves within the
                    // test's own lifetime -- the point is proving
                    // `Resize` doesn't wait on it at all.
                    thread::sleep(Duration::from_secs(5));
                    let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(
                        &mut stream,
                        &blueice_ipc::gatekeeper::GatekeeperReply::Cleared,
                    );
                }
            });
        }
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>still the old page</p>", None);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper_path,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://127.0.0.1:1/never-resolves".to_string(),
        },
    )
    .unwrap();

    let start = Instant::now();
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 111,
            height: 222,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(
            reply,
            ServerMessage::FrameReady {
                width: 111,
                height: 222,
                ..
            }
        ),
        "expected an immediate FrameReady for the resize, got {reply:?}"
    );
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "Resize must apply immediately, not wait behind the pending navigation, took {:?}",
        start.elapsed()
    );

    // The old page's content is still what's shown -- the pending
    // navigation never actually applied.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(snap
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("still the old page")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
