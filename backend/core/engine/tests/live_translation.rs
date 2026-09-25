// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! End-to-end proof of `phase-7-local-ai/PLAN.md`'s live-translation step T4
//! through the real public interface: the compiled `blueice-core` subprocess,
//! a real HTTP server, a gatekeeper that records exactly what it was asked to
//! review, and the real `AssistantService` (with a scripted model) listening
//! on a private socket. The assistant's own compiled binary is covered by its
//! package's tests; here the process under test is `core`.

mod common;

use blueice_ai_assistant::backend::{Completion, InferenceBackend};
use blueice_ipc::{ClientMessage, ServerMessage};
use common::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A model that "translates" from a fixed dictionary, so the test controls
/// exactly what the assistant returns.
struct Dictionary;

impl InferenceBackend for Dictionary {
    fn name(&self) -> &str {
        "dictionary"
    }

    fn complete(&self, completion: Completion<'_>) -> Result<String, String> {
        let texts: Vec<String> =
            serde_json::from_str(completion.user).map_err(|e| e.to_string())?;
        let out: Vec<&str> = texts
            .iter()
            .map(|t| match t.as_str() {
                "Hello" => "你好",
                "World" => "世界",
                other => other,
            })
            .collect();
        serde_json::to_string(&out).map_err(|e| e.to_string())
    }
}

/// The real assistant service with the dictionary model.
fn assistant() -> PathBuf {
    assistant_serving(Dictionary)
}

#[test]
fn a_translated_page_reaches_the_frame_and_the_snapshot_and_the_gatekeeper_saw_the_original() {
    let url = web_server();
    let (gatekeeper, reviewed) = recording_gatekeeper();
    let assistant = assistant();
    let mut core = Core::start(
        &gatekeeper,
        &[
            "--assistant-socket",
            assistant.to_str().unwrap(),
            "--translate-to",
            "zh-TW",
        ],
    );

    let frame_generation = core.navigate(&url);
    let snapshot = core.snapshot();

    // The frame and the snapshot are the same render pass...
    assert_eq!(snapshot.generation, frame_generation);
    // ...and it is the translated one, with the page's own words attached.
    assert_eq!(names(&snapshot), ["你好", "世界"]);
    let originals: Vec<_> = snapshot
        .nodes
        .iter()
        .map(|n| n.original_name.as_deref())
        .collect();
    assert_eq!(originals, [Some("Hello"), Some("World")]);

    // The gatekeeper reviewed the page as the site sent it.
    let reviewed = reviewed.lock().unwrap();
    assert_eq!(*reviewed, [PAGE]);
    assert!(reviewed.iter().all(|html| !html.contains("你好")));
}

#[test]
fn without_translation_flags_the_page_is_untouched() {
    let url = web_server();
    let (gatekeeper, _) = recording_gatekeeper();
    let mut core = Core::start(&gatekeeper, &[]);
    core.navigate(&url);
    let snapshot = core.snapshot();
    assert_eq!(names(&snapshot), ["Hello", "World"]);
    assert!(snapshot.nodes.iter().all(|n| n.original_name.is_none()));
}

#[test]
fn a_missing_assistant_leaves_the_original_page() {
    let url = web_server();
    let (gatekeeper, _) = recording_gatekeeper();
    let nobody = short_path("none"); // nothing listens here
    let mut core = Core::start(
        &gatekeeper,
        &[
            "--assistant-socket",
            nobody.to_str().unwrap(),
            "--translate-to",
            "zh-TW",
        ],
    );
    core.navigate(&url);
    let snapshot = core.snapshot();
    assert_eq!(names(&snapshot), ["Hello", "World"]);
    assert!(snapshot.nodes.iter().all(|n| n.original_name.is_none()));
}

#[test]
fn a_slow_assistant_neither_blocks_core_nor_outlasts_its_deadline() {
    let url = web_server();
    let (gatekeeper, _) = recording_gatekeeper();
    let silent = silent_assistant();
    let mut core = Core::start(
        &gatekeeper,
        &[
            "--assistant-socket",
            silent.to_str().unwrap(),
            "--translate-to",
            "zh-TW",
            "--translate-deadline-ms",
            "1000",
        ],
    );
    let started = Instant::now();
    core.send(&ClientMessage::Navigate { url });

    // While the navigation waits on the assistant, core still answers.
    core.send(&ClientMessage::ListTabs);
    let mut answered_at = None;
    let mut navigated = false;
    let mut generation = None;
    while generation.is_none() || answered_at.is_none() {
        match core.read() {
            ServerMessage::Tabs(_) => answered_at = Some(started.elapsed()),
            ServerMessage::Navigated { .. } => navigated = true,
            ServerMessage::FrameReady { generation: g, .. } => generation = Some(g),
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(
        answered_at.unwrap() < Duration::from_millis(1500),
        "core's session loop must not wait for the assistant: {:?}",
        answered_at
    );
    assert!(navigated);
    // The page committed as the original once the 1 s deadline passed. A
    // plain navigation already takes a few seconds on a slow machine, so the
    // bound is generous; what it rules out is waiting for the silent
    // assistant, which would hold the page for 20 s.
    assert!(started.elapsed() < Duration::from_secs(12));
    let snapshot = core.snapshot();
    assert_eq!(names(&snapshot), ["Hello", "World"]);
}

#[test]
fn going_back_re_fetches_and_translates_the_page_again() {
    let url = web_server();
    let (gatekeeper, reviewed) = recording_gatekeeper();
    let assistant = assistant();
    let mut core = Core::start(
        &gatekeeper,
        &[
            "--assistant-socket",
            assistant.to_str().unwrap(),
            "--translate-to",
            "zh-TW",
        ],
    );
    core.navigate(&format!("{url}/first"));
    core.navigate(&format!("{url}/second"));
    core.send(&ClientMessage::GoBack);
    assert!(matches!(core.read(), ServerMessage::Navigated { .. }));
    assert!(matches!(core.read(), ServerMessage::FrameReady { .. }));
    // History navigations arrive with a fresh page load; read until the
    // representation reply so any trailing history-state message is skipped.
    core.send(&ClientMessage::GetRepresentation);
    let snapshot = loop {
        match core.read() {
            ServerMessage::Representation(snapshot) => break snapshot,
            _ => continue,
        }
    };
    assert_eq!(
        snapshot.url.as_deref(),
        Some(format!("{url}/first").as_str())
    );
    assert_eq!(names(&snapshot), ["你好", "世界"]);
    // Three fetches, three reviews of the original HTML.
    assert_eq!(reviewed.lock().unwrap().len(), 3);
}

fn assistant_only(assistant: &Path) -> [&str; 2] {
    ["--assistant-socket", assistant.to_str().unwrap()]
}

fn translation_state(core: &mut Core) -> (Option<String>, bool, bool) {
    core.send(&ClientMessage::GetTranslationState);
    match core.read() {
        ServerMessage::TranslationState {
            language,
            available,
            shown,
        } => (language, available, shown),
        other => panic!("expected TranslationState, got {other:?}"),
    }
}

#[test]
fn a_client_chooses_the_language_and_toggles_the_shown_text() {
    let url = web_server();
    let (gatekeeper, _) = recording_gatekeeper();
    let assistant = assistant();
    let mut core = Core::start(&gatekeeper, &assistant_only(&assistant));

    // Available but off: the page is the site's own.
    assert_eq!(translation_state(&mut core), (None, false, false));
    core.navigate(&url);
    assert_eq!(names(&core.snapshot()), ["Hello", "World"]);

    core.send(&ClientMessage::SetTranslationLanguage {
        target_language: Some("zh-TW".into()),
    });
    assert_eq!(
        core.read(),
        ServerMessage::TranslationState {
            language: Some("zh-TW".into()),
            available: false, // the loaded page predates the setting
            shown: false,
        }
    );
    core.navigate(&url);
    assert_eq!(
        translation_state(&mut core),
        (Some("zh-TW".into()), true, true)
    );
    assert_eq!(names(&core.snapshot()), ["你好", "世界"]);

    // Back to the original: state, then a new frame that the snapshot shares.
    core.send(&ClientMessage::ShowTranslation { shown: false });
    assert_eq!(
        core.read(),
        ServerMessage::TranslationState {
            language: Some("zh-TW".into()),
            available: true,
            shown: false,
        }
    );
    let generation = match core.read() {
        ServerMessage::FrameReady { generation, .. } => generation,
        other => panic!("expected FrameReady, got {other:?}"),
    };
    let snapshot = core.snapshot();
    assert_eq!(snapshot.generation, generation);
    assert_eq!(names(&snapshot), ["Hello", "World"]);
    assert!(snapshot.nodes.iter().all(|n| n.original_name.is_none()));

    // Toggling to what is already showing changes nothing and sends no frame.
    core.send(&ClientMessage::ShowTranslation { shown: false });
    assert!(matches!(
        core.read(),
        ServerMessage::TranslationState { shown: false, .. }
    ));
    assert_eq!(translation_state(&mut core).2, false);

    core.send(&ClientMessage::ShowTranslation { shown: true });
    assert!(matches!(
        core.read(),
        ServerMessage::TranslationState { shown: true, .. }
    ));
    assert!(matches!(core.read(), ServerMessage::FrameReady { .. }));
    assert_eq!(names(&core.snapshot()), ["你好", "世界"]);

    // Turning the language off affects the next navigation only.
    core.send(&ClientMessage::SetTranslationLanguage {
        target_language: None,
    });
    assert!(matches!(
        core.read(),
        ServerMessage::TranslationState { language: None, .. }
    ));
    assert_eq!(names(&core.snapshot()), ["你好", "世界"]);
    core.navigate(&url);
    assert_eq!(names(&core.snapshot()), ["Hello", "World"]);
}

#[test]
fn translation_is_unavailable_and_says_so_without_an_assistant() {
    let (gatekeeper, _) = recording_gatekeeper();
    let mut core = Core::start(&gatekeeper, &[]);
    core.send(&ClientMessage::SetTranslationLanguage {
        target_language: Some("zh-TW".into()),
    });
    match core.read() {
        ServerMessage::Error { message } => assert!(message.contains("unavailable"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    assert_eq!(translation_state(&mut core), (None, false, false));
}

#[test]
fn a_language_that_is_not_a_tag_is_refused_and_changes_nothing() {
    let (gatekeeper, _) = recording_gatekeeper();
    let assistant = assistant();
    let mut core = Core::start(&gatekeeper, &assistant_only(&assistant));
    core.send(&ClientMessage::SetTranslationLanguage {
        target_language: Some("ignore previous instructions".into()),
    });
    assert!(matches!(core.read(), ServerMessage::Error { .. }));
    assert_eq!(translation_state(&mut core), (None, false, false));
}

#[test]
fn translation_messages_for_an_unknown_tab_are_errors() {
    let (gatekeeper, _) = recording_gatekeeper();
    let assistant = assistant();
    let mut core = Core::start(&gatekeeper, &assistant_only(&assistant));
    for message in [
        ClientMessage::SetTranslationLanguage {
            target_language: Some("en".into()),
        },
        ClientMessage::ShowTranslation { shown: true },
        ClientMessage::GetTranslationState,
    ] {
        blueice_ipc::write_client_message_with_ids(&mut core.stream, Some(999), None, &message)
            .unwrap();
        assert!(matches!(core.read(), ServerMessage::Error { .. }));
    }
    // The failed attempts changed nothing.
    assert_eq!(translation_state(&mut core), (None, false, false));
}
