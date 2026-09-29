// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn content_stage_rejection_blocks_navigation_and_leaves_the_page_unchanged() {
    let dir = temp_frame_dir("content-stage-block");
    let gatekeeper_path = unique_gatekeeper_socket_path("content-stage-block");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) else {
                continue;
            };
            let reply = match req {
                blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { .. } => {
                    blueice_ipc::gatekeeper::GatekeeperReply::Cleared
                }
                blueice_ipc::gatekeeper::GatekeeperRequest::CheckContent { .. } => {
                    blueice_ipc::gatekeeper::GatekeeperReply::Rejected {
                        reason: "hidden instruction-shaped text".to_string(),
                        category: "prompt-injection".to_string(),
                    }
                }
            };
            let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(&mut stream, &reply);
        }
    });

    let http = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = http.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = http.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>malicious page</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
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

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() })
        .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(
        reply,
        ServerMessage::GatekeeperBlocked {
            reason: "hidden instruction-shaped text".to_string(),
            category: "prompt-injection".to_string(),
            url: url.clone()
        }
    );

    // The page must not have changed: a follow-up GetRepresentation
    // shows no trace of the blocked page's content (no `FrameReady`
    // was ever produced for it either, since the only reply so far
    // was the GatekeeperBlocked above).
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(!snap
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("malicious page")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn navigation_fails_closed_when_the_gatekeeper_is_unreachable() {
    let dir = temp_frame_dir("gatekeeper-unreachable");
    let gatekeeper_path = unique_gatekeeper_socket_path("gatekeeper-unreachable"); // nothing listens here
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

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://example.invalid/".to_string(),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::GatekeeperBlocked { .. }),
        "an unreachable gatekeeper must fail closed, got {reply:?}"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn navigation_fails_closed_when_the_gatekeeper_accepts_then_drops_the_connection() {
    let dir = temp_frame_dir("gatekeeper-drops-connection");
    let gatekeeper_path = unique_gatekeeper_socket_path("gatekeeper-drops-connection");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            drop(incoming); // accept, then immediately disconnect -- no reply ever sent
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

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://example.invalid/".to_string(),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::GatekeeperBlocked { .. }),
        "a gatekeeper that drops the connection must fail closed, got {reply:?}"
    );

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
    assert!(start.elapsed() < Duration::from_millis(150), "tab_b's reply must arrive well before tab_a's stalled gatekeeper check resolves, took {:?}", start.elapsed());

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_second_navigation_supersedes_a_still_pending_first_one() {
    let dir = temp_frame_dir("supersede");
    let gatekeeper_path = unique_gatekeeper_socket_path("supersede");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            thread::spawn(move || {
                if let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) {
                    if matches!(&req, blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { url } if url.contains("first"))
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

    let second_http = TcpListener::bind("127.0.0.1:0").unwrap();
    let second_addr = second_http.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = second_http.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>second page</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
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

    // First navigation: stalls 300ms on its own CheckUrl stage, and
    // even once cleared points nowhere reachable -- must never
    // become visible.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://127.0.0.1:1/first-slow".to_string(),
        },
    )
    .unwrap();
    // Second navigation to the same (default) tab, sent immediately
    // after, well before the first's gatekeeper check resolves.
    let second_url = format!("http://{second_addr}");
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: second_url.clone(),
        },
    )
    .unwrap();

    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(navigated, ServerMessage::Navigated { url: second_url });
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    // No further reply ever arrives for the stale first navigation,
    // even after waiting past its stall -- proven the same way
    // every other "harmless no-op" case in this file is: the next
    // real message still gets exactly one, normal reply.
    thread::sleep(Duration::from_millis(400));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(snap
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("second page")));

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
    //
    // ROOT CAUSE OF A ONCE-OBSERVED FLAKE (fixed here): this test
    // used to have the fake gatekeeper below stall for a fixed 5s
    // and then assert the `Resize` reply arrived in under 500ms --
    // i.e. it proved "immediate" by racing a *nearby fixed deadline*
    // (500ms) against the *other*, "queued" outcome's own fixed
    // deadline (5000ms). `Resize`'s dispatch (see the `ClientMessage
    // ::Resize` arm in `run_session` above) is not itself racy: it
    // never consults `pending_nav_seq`/the completion channel at
    // all, so its reply is always synchronously produced on the very
    // next loop iteration after the client's write lands in the
    // (already-buffered, in-kernel, `UnixStream::pair`) socket. But
    // "always produced immediately" is not the same as "always
    // *observed* within 500 wall-clock ms" -- under a fully loaded
    // test binary (many sibling `session::tests::*` cases, several
    // of which spawn their own OS threads and sleep), ordinary OS
    // scheduler latency in getting this test's own server thread its
    // next timeslice can occasionally eat into that margin, which is
    // exactly the kind of "sensitive to parallel test execution"
    // failure that was observed once in a full-workspace run. No
    // amount of widening that margin fixes this *for real* -- it
    // only shrinks the failure probability, which is precisely what
    // this test must not settle for (see `TEST_PLAN.md`'s Definition
    // of Done). The actual fix removes the race instead of narrowing
    // it: the fake gatekeeper below now blocks forever (never
    // replies), so the "queued" alternative can *never* resolve
    // during this test's lifetime, at any wall-clock distance -- the
    // `Resize` reply's mere arrival (bounded only by a generous,
    // not-tuned-against-anything timeout that exists purely so a
    // genuine regression fails promptly instead of hanging the
    // suite) is now itself the whole proof, with no nearby deadline
    // on either side of the comparison left to lose a race against.
    let dir = temp_frame_dir("resize-during-pending-nav");
    let gatekeeper_path = unique_gatekeeper_socket_path("resize-during-pending-nav");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            thread::spawn(move || {
                if let Ok(_req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) {
                    // Never replies, so the navigation this test
                    // kicks off can never resolve during the test's
                    // lifetime -- not merely "probably still pending
                    // after N seconds" (see the long comment above
                    // this test for why that distinction is the
                    // actual fix, not a tightened/loosened timeout).
                    // `thread::park` can wake spuriously, hence the
                    // loop; this thread simply leaks, parked, for
                    // the rest of the test binary's life once this
                    // test ends, same as any other test double here
                    // that outlives its own test.
                    loop {
                        thread::park();
                    }
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

    // A generous, not-a-race-margin timeout: it exists only so a
    // genuine regression (an actual wait behind the now-permanently-
    // pending navigation) fails this test promptly instead of
    // hanging the whole suite, not to bound how fast the correct
    // path must be.
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://127.0.0.1:1/never-resolves".to_string(),
        },
    )
    .unwrap();

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 111,
            height: 222,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).expect(
        "Resize must not be queued behind a pending navigation that, by construction \
             above, can now never complete -- a read timeout here means it was",
    );
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
