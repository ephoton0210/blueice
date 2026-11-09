// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn navigate_waits_for_a_success_frame_then_returns_the_resulting_snapshot() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::Navigate { .. }));
                reply(
                    s,
                    &ServerMessage::Navigated {
                        url: "https://example.com".to_string(),
                    },
                );
                reply_tab(
                    s,
                    1,
                    &ServerMessage::FrameReady {
                        shm_path: "/tmp/x".to_string(),
                        width: 10,
                        height: 10,
                        generation: 1,
                    },
                );
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::Representation(sample_snapshot(1)));
            }),
        ],
    );

    let mut conn = CoreConnection::new(client);
    let outcome = conn.navigate("https://example.com", None).unwrap();
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.snapshot.generation, 1);
    assert_eq!(conn.last_frame(Some(1)).unwrap().generation, 1);
}

#[test]
fn navigate_failure_is_reported_but_still_returns_the_current_snapshot() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::Navigate { .. }));
                reply(
                    s,
                    &ServerMessage::Error {
                        message: "unreachable host".to_string(),
                    },
                );
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::Representation(sample_snapshot(0)));
            }),
        ],
    );

    let mut conn = CoreConnection::new(client);
    let outcome = conn.navigate("http://bad", None).unwrap();
    assert_eq!(outcome.error.as_deref(), Some("unreachable host"));
    assert_eq!(outcome.snapshot.generation, 0);
}

#[test]
fn act_on_click_with_no_navigable_effect_still_returns_cleanly() {
    // the real ambiguity this design exists to avoid: an ActOn
    // Click that doesn't land on a link produces zero replies of
    // its own (blueice_engine::session's documented behavior) --
    // proven here by never scripting a reply to the ActOn at all,
    // only to the pipelined GetRepresentation.
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::Representation(sample_snapshot(0)));
            }),
            Box::new(|msg, _s| {
                assert!(matches!(msg, ClientMessage::ActOn { .. }));
                // no reply -- matches a Click that hit nothing
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::Representation(sample_snapshot(0)));
            }),
        ],
    );

    let mut conn = CoreConnection::new(client);
    let outcome = conn.act(2, NodeAction::Click, None).unwrap();
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.snapshot.nodes[0].id, 1);
}

#[test]
fn act_on_a_link_waits_for_the_navigated_frame_before_reading_its_snapshot() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::Representation(sample_snapshot(0)));
            }),
            Box::new(|msg, s| {
                assert!(matches!(
                    msg,
                    ClientMessage::ActOn {
                        id: 1,
                        action: NodeAction::Click
                    }
                ));
                reply(
                    s,
                    &ServerMessage::Navigated {
                        url: "https://example.com/next".to_string(),
                    },
                );
                reply_tab(
                    s,
                    1,
                    &ServerMessage::FrameReady {
                        shm_path: "/tmp/next".to_string(),
                        width: 10,
                        height: 10,
                        generation: 1,
                    },
                );
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::Representation(sample_snapshot(1)));
            }),
        ],
    );

    let mut conn = CoreConnection::new(client);
    let outcome = conn.act(1, NodeAction::Click, None).unwrap();
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.snapshot.generation, 1);
    assert_eq!(conn.last_frame(Some(1)).unwrap().generation, 1);
}

#[test]
fn highlight_round_trips_like_any_other_action() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert_eq!(msg, ClientMessage::Highlight { id: Some(1) });
                reply(
                    s,
                    &ServerMessage::FrameReady {
                        shm_path: "/tmp/z".to_string(),
                        width: 5,
                        height: 5,
                        generation: 3,
                    },
                );
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::Representation(sample_snapshot(3)));
            }),
        ],
    );

    let mut conn = CoreConnection::new(client);
    let outcome = conn.highlight(Some(1), None).unwrap();
    assert_eq!(outcome.snapshot.generation, 3);
}

#[test]
fn representation_alone_sends_no_prior_action() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::GetRepresentation));
            reply(s, &ServerMessage::Representation(sample_snapshot(0)));
        })],
    );

    let mut conn = CoreConnection::new(client);
    let snap = conn.representation(None).unwrap();
    assert_eq!(snap.generation, 0);
}

#[test]
fn dom_alone_sends_no_prior_action() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::GetDom));
            reply(s, &ServerMessage::Dom("| <html>\n".to_string()));
        })],
    );

    let mut conn = CoreConnection::new(client);
    assert_eq!(conn.dom(None).unwrap(), "| <html>\n");
}

#[test]
fn dom_still_caches_a_frame_ready_seen_along_the_way() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::GetDom));
            reply_tab(
                s,
                1,
                &ServerMessage::FrameReady {
                    shm_path: "/tmp/dom".to_string(),
                    width: 1,
                    height: 1,
                    generation: 5,
                },
            );
            reply(s, &ServerMessage::Dom("| <html>\n".to_string()));
        })],
    );

    let mut conn = CoreConnection::new(client);
    conn.dom(None).unwrap();
    assert_eq!(conn.last_frame(Some(1)).unwrap().generation, 5);
}

#[test]
fn representation_alone_still_caches_a_frame_ready_seen_along_the_way() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert!(matches!(msg, ClientMessage::GetRepresentation));
            reply_tab(
                s,
                1,
                &ServerMessage::FrameReady {
                    shm_path: "/tmp/w".to_string(),
                    width: 1,
                    height: 1,
                    generation: 9,
                },
            );
            reply(s, &ServerMessage::Representation(sample_snapshot(9)));
        })],
    );

    let mut conn = CoreConnection::new(client);
    conn.representation(None).unwrap();
    assert_eq!(conn.last_frame(Some(1)).unwrap().generation, 9);
}

#[test]
fn set_visible_sends_a_chrome_command() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, _s| {
            assert_eq!(msg, ClientMessage::Chrome(ChromeCommand::SetVisible(true)));
        })],
    );

    let mut conn = CoreConnection::new(client);
    conn.set_visible(true).unwrap();
}

#[test]
fn last_frame_is_none_before_any_action_produces_one() {
    let (client, _server) = UnixStream::pair().unwrap();
    let conn = CoreConnection::new(client);
    assert!(conn.last_frame(None).is_none());
}

#[test]
fn frame_to_png_bytes_produces_a_real_decodable_png() {
    let pixels = vec![
        255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
    ];
    let png = frame_to_png_bytes(&pixels, 2, 2).unwrap();
    assert_eq!(
        &png[0..8],
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
    );
}

#[test]
fn a_wheel_scroll_records_the_page_and_node_trajectory_notch_by_notch() {
    let (client, server) = UnixStream::pair().unwrap();
    // Node 1 sits at document y 100; the page is scrolled to `scroll_y`.
    let representation = |scroll_y: f64, generation: u64| -> FakeCoreStep {
        Box::new(move |msg, s| {
            assert!(matches!(msg, ClientMessage::GetRepresentation));
            let mut snapshot = sample_snapshot(generation);
            snapshot.scroll_y = scroll_y;
            snapshot.nodes[0].bounds.y = 100.0;
            reply(s, &ServerMessage::Representation(snapshot));
        })
    };
    let notch = |expected: f64, generation: u64| -> FakeCoreStep {
        Box::new(move |msg, s| {
            assert_eq!(msg, ClientMessage::Scroll { delta_y: expected });
            reply_tab(
                s,
                1,
                &ServerMessage::FrameReady {
                    shm_path: "/tmp/x".to_string(),
                    width: 10,
                    height: 40,
                    generation,
                },
            );
        })
    };
    fake_core(
        server,
        vec![
            representation(0.0, 4),
            notch(20.0, 5),
            representation(20.0, 5),
            notch(20.0, 6),
            representation(40.0, 6),
            notch(10.0, 7),
            representation(50.0, 7),
        ],
    );

    let mut conn = CoreConnection::new(client);
    let record = conn
        .scroll(50.0, 20.0, Duration::ZERO, Some(vec![1, 99]), None)
        .unwrap();
    assert_eq!(record.error, None);
    assert_eq!(record.tracked, [1, 99]);
    assert_eq!(record.start.scroll_y, 0.0);
    assert_eq!(record.start.nodes[0].viewport_y, 100.0);
    assert_eq!(record.start.missing, [99]);
    assert_eq!(
        record.steps.iter().map(|s| s.delta_y).collect::<Vec<_>>(),
        [20.0, 20.0, 10.0]
    );
    assert_eq!(
        record
            .steps
            .iter()
            .map(|s| s.generation)
            .collect::<Vec<_>>(),
        [Some(5), Some(6), Some(7)]
    );
    assert!(record.steps.iter().all(|s| s.frame_ms.is_some()));
    // The node's document position never changes; its place in the window
    // moves up by exactly what was scrolled, and it leaves the 40px view.
    let after: Vec<(f64, f64, Option<bool>)> = record
        .steps
        .iter()
        .map(|s| {
            let p = s.position.as_ref().unwrap();
            (p.scroll_y, p.nodes[0].viewport_y, p.nodes[0].in_view)
        })
        .collect();
    assert_eq!(
        after,
        [
            (20.0, 80.0, Some(false)),
            (40.0, 60.0, Some(false)),
            (50.0, 50.0, Some(false))
        ]
    );
    assert!(record
        .steps
        .iter()
        .all(|s| s.position.as_ref().unwrap().nodes[0].doc_y == 100.0));
    assert_eq!(conn.last_frame(Some(1)).unwrap().generation, 7);
}

#[test]
fn a_scroll_record_says_how_far_the_page_really_moved_so_scripts_and_page_ends_show() {
    let (client, server) = UnixStream::pair().unwrap();
    let representation = |scroll_y: f64, generation: u64| -> FakeCoreStep {
        Box::new(move |msg, s| {
            assert!(matches!(msg, ClientMessage::GetRepresentation));
            let mut snapshot = sample_snapshot(generation);
            snapshot.scroll_y = scroll_y;
            reply(s, &ServerMessage::Representation(snapshot));
        })
    };
    let notch = |generation: u64| -> FakeCoreStep {
        Box::new(move |msg, s| {
            assert!(matches!(msg, ClientMessage::Scroll { .. }));
            reply_tab(
                s,
                1,
                &ServerMessage::FrameReady {
                    shm_path: "/tmp/x".to_string(),
                    width: 10,
                    height: 40,
                    generation,
                },
            );
        })
    };
    // Three 20 px notches. The first moves the page 20; before the second a page script
    // scrolled it on its own (to 200); the third finds the end of the page and moves nothing.
    fake_core(
        server,
        vec![
            representation(0.0, 1),
            notch(2),
            representation(20.0, 2),
            notch(3),
            representation(200.0, 3),
            notch(4),
            representation(200.0, 4),
        ],
    );
    let mut conn = CoreConnection::new(client);
    let record = conn
        .scroll(60.0, 20.0, Duration::ZERO, Some(vec![]), None)
        .unwrap();
    assert_eq!(
        record.steps.iter().map(|s| s.moved_px).collect::<Vec<_>>(),
        [Some(20.0), Some(180.0), Some(0.0)]
    );
    assert_eq!(record.requested_px, 60.0);
    assert_eq!(record.moved_px, 200.0, "the page's own scroll counts: it is where the page ended up");
    assert_eq!(record.final_scroll_y, 200.0);
}

#[test]
fn without_a_track_list_the_first_visible_named_nodes_are_followed() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|_, s| {
            reply(s, &ServerMessage::Representation(sample_snapshot(1)));
        })],
    );
    let mut conn = CoreConnection::new(client);
    let record = conn.scroll(0.0, 20.0, Duration::ZERO, None, None).unwrap();
    assert_eq!(record.tracked, [1]);
    assert!(record.steps.is_empty());
    assert_eq!(record.start.nodes.len(), 1);
}

#[test]
fn an_upward_scroll_sends_negative_notches() {
    assert_eq!(scroll_notches(-45.0, 20.0), [-20.0, -20.0, -5.0]);
    assert_eq!(scroll_notches(40.0, 20.0), [20.0, 20.0]);
    assert!(scroll_notches(0.0, 20.0).is_empty());
}

#[test]
fn a_wheel_scroll_the_core_refuses_stops_and_reports_why() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|_, s| {
                reply(s, &ServerMessage::Representation(sample_snapshot(1)));
            }),
            Box::new(|_, s| {
                reply(
                    s,
                    &ServerMessage::Error {
                        message: "unknown tab 9".to_string(),
                    },
                );
            }),
        ],
    );
    let mut conn = CoreConnection::new(client);
    let record = conn
        .scroll(100.0, 20.0, Duration::ZERO, None, Some(9))
        .unwrap();
    assert_eq!(record.error.as_deref(), Some("unknown tab 9"));
    assert_eq!(record.steps.len(), 1);
    assert_eq!(record.steps[0].generation, None);
    assert_eq!(record.steps[0].position, None);
}

#[test]
fn a_screenshot_whose_frame_file_has_been_replaced_asks_for_a_fresh_frame() {
    // A page that keeps repainting rotates its frame files; the one this connection
    // last heard of may be gone by the time a screenshot reads it.
    let dir = std::env::temp_dir().join(format!("bi-mcp-shot-{}", std::process::id()));
    let fresh = blueice_ipc::shm::write_frame(&dir, 1, 8, &[200u8; 4 * 4 * 4]).unwrap();
    let (client, server) = UnixStream::pair().unwrap();
    let fresh_path = fresh.to_string_lossy().into_owned();
    fake_core(
        server,
        vec![Box::new(move |msg, s| {
            assert_eq!(msg, ClientMessage::Scroll { delta_y: 0.0 });
            reply_tab(
                s,
                1,
                &ServerMessage::FrameReady {
                    shm_path: fresh_path.clone(),
                    width: 4,
                    height: 4,
                    generation: 8,
                },
            );
        })],
    );
    let mut conn = CoreConnection::new(client);
    conn.record_frame(
        Some(1),
        FrameInfo {
            shm_path: dir.join("gone.frame").to_string_lossy().into_owned(),
            width: 4,
            height: 4,
            generation: 7,
        },
        true,
    );
    let (png, tab, generation, _) = conn.screenshot_png(None).unwrap().expect("a screenshot");
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    assert_eq!((tab, generation), (1, 8));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_screenshot_with_no_frame_at_all_is_none_not_an_error() {
    let (client, _server) = UnixStream::pair().unwrap();
    let mut conn = CoreConnection::new(client);
    assert!(conn.screenshot_png(None).unwrap().is_none());
}
