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
