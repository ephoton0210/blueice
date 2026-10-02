// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn show_translation_returns_the_state_and_the_representation() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert_eq!(msg, ClientMessage::ShowTranslation { shown: false });
                reply(
                    s,
                    &ServerMessage::TranslationState {
                        language: Some("zh-TW".to_string()),
                        available: true,
                        shown: false,
                    },
                );
                reply_tab(
                    s,
                    7,
                    &ServerMessage::FrameReady {
                        shm_path: "/tmp/original".to_string(),
                        width: 10,
                        height: 10,
                        generation: 5,
                    },
                );
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::Representation(sample_snapshot(5)));
            }),
        ],
    );
    let mut conn = CoreConnection::new(client);
    let outcome = conn.show_translation(false, Some(7)).unwrap();
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.language.as_deref(), Some("zh-TW"));
    assert!(outcome.available);
    assert!(!outcome.shown);
    assert_eq!(outcome.snapshot.generation, 5);
    assert_eq!(conn.last_frame(Some(7)).unwrap().generation, 5);
}

#[test]
fn a_refused_translation_language_is_an_error_with_the_current_page() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert_eq!(
                    msg,
                    ClientMessage::SetTranslationLanguage {
                        target_language: Some("zh-TW".to_string())
                    }
                );
                reply(
                    s,
                    &ServerMessage::Error {
                        message: "translation is unavailable".to_string(),
                    },
                );
            }),
            Box::new(|_, s| {
                reply(s, &ServerMessage::Representation(sample_snapshot(1)));
            }),
        ],
    );
    let mut conn = CoreConnection::new(client);
    let outcome = conn
        .set_translation_language(Some("zh-TW".to_string()), None)
        .unwrap();
    assert_eq!(outcome.error.as_deref(), Some("translation is unavailable"));
    assert_eq!(outcome.language, None);
    assert!(!outcome.available && !outcome.shown);
    assert_eq!(outcome.snapshot.generation, 1);
}

#[test]
fn a_summary_waits_for_its_own_reply_and_skips_other_traffic() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert_eq!(msg, ClientMessage::SummarizePage);
            // Another client's broadcast reply (a different request id),
            // then this request's own.
            blueice_ipc::write_server_message_with_id(
                s,
                Some(9_999),
                &ServerMessage::Error {
                    message: "someone else's".to_string(),
                },
            )
            .unwrap();
            reply(
                s,
                &ServerMessage::AssistantResult {
                    kind: blueice_ipc::AssistantTaskKind::Summary,
                    text: "short".to_string(),
                },
            );
        })],
    );
    let mut conn = CoreConnection::new(client);
    assert_eq!(
        conn.summarize_page(Some(3)).unwrap(),
        AssistantOutcome::Done {
            kind: blueice_ipc::AssistantTaskKind::Summary,
            text: "short".to_string()
        }
    );
}

#[test]
fn organize_failure_is_reported_with_the_reason() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![Box::new(|msg, s| {
            assert_eq!(
                msg,
                ClientMessage::OrganizePage {
                    instruction: "make a table".to_string()
                }
            );
            reply(
                s,
                &ServerMessage::Error {
                    message: "the assistant is not running".to_string(),
                },
            );
        })],
    );
    let mut conn = CoreConnection::new(client);
    assert_eq!(
        conn.organize_page("make a table".to_string(), None)
            .unwrap(),
        AssistantOutcome::Failed("the assistant is not running".to_string())
    );
}
