// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! End-to-end proof of `phase-7-local-ai/PLAN.md`'s panel steps S4 and S7:
//! the compiled `blueice-core` with the real `AssistantService` (and a
//! scripted model). A summary reaches the requester as a request-correlated
//! reply *and* the `about:assistant` page, which an AI reads through the same
//! snapshot as any page; failures say why; and a slow assistant never blocks
//! `core`.

mod common;

use blueice_ai_assistant::backend::{Completion, InferenceBackend};
use blueice_ipc::{AiSnapshot, AssistantTaskKind, ClientMessage, ServerMessage};
use common::*;
use std::path::Path;
use std::time::{Duration, Instant};

/// A model with a fixed voice per task, so results are recognizable.
struct Voice;

impl InferenceBackend for Voice {
    fn name(&self) -> &str {
        "voice"
    }

    fn complete(&self, completion: Completion<'_>) -> Result<String, String> {
        if completion.system.contains("summarizer") {
            Ok(format!(
                "SUMMARY OF: {}",
                completion.user.replace('\n', " / ")
            ))
        } else if completion.system.contains("organizer") {
            Ok(format!(
                "ORGANIZED: {}",
                completion.user.replace('\n', " / ")
            ))
        } else {
            Err("unexpected task".into())
        }
    }
}

/// A model that always fails, to prove the reason reaches the person.
struct Down;

impl InferenceBackend for Down {
    fn name(&self) -> &str {
        "down"
    }

    fn complete(&self, _completion: Completion<'_>) -> Result<String, String> {
        Err("model down".into())
    }
}

fn with_assistant(assistant: &Path) -> [&str; 2] {
    ["--assistant-socket", assistant.to_str().unwrap()]
}

fn representation_of(core: &mut Core, tab: u64) -> AiSnapshot {
    blueice_ipc::write_client_message_with_ids(
        &mut core.stream,
        Some(tab),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    match core.read() {
        ServerMessage::Representation(snapshot) => snapshot,
        other => panic!("expected Representation, got {other:?}"),
    }
}

#[test]
fn a_summary_reaches_the_requester_and_the_about_assistant_snapshot() {
    let url = web_server();
    let (gatekeeper, _) = recording_gatekeeper();
    let assistant = assistant_serving(Voice);
    let mut core = Core::start(&gatekeeper, &with_assistant(&assistant));
    core.navigate(&url);

    core.send(&ClientMessage::SummarizePage);
    assert_eq!(
        core.read(),
        ServerMessage::AssistantResult {
            kind: AssistantTaskKind::Summary,
            text: "SUMMARY OF: Hello / World".into(),
        }
    );

    // The same result is on the panel a human or an AI opens.
    core.navigate("about:assistant");
    let snapshot = core.snapshot();
    let text = names(&snapshot);
    assert!(
        text.iter().any(|n| n == "SUMMARY OF: Hello / World"),
        "{text:?}"
    );
    assert!(text.iter().any(|n| n.contains("Written by a local model")));
}

#[test]
fn organized_data_carries_the_instruction_to_the_model_and_onto_the_panel() {
    let url = web_server();
    let (gatekeeper, _) = recording_gatekeeper();
    let assistant = assistant_serving(Voice);
    let mut core = Core::start(&gatekeeper, &with_assistant(&assistant));
    core.navigate(&url);

    core.send(&ClientMessage::OrganizePage {
        instruction: "make a table".into(),
    });
    assert_eq!(
        core.read(),
        ServerMessage::AssistantResult {
            kind: AssistantTaskKind::Organized,
            text: "ORGANIZED: Hello / World".into(),
        }
    );
    core.navigate("about:assistant");
    let text = names(&core.snapshot());
    assert!(text.iter().any(|n| n == "ORGANIZED: Hello / World"));
    assert!(text.iter().any(|n| n.contains("make a table")), "{text:?}");
}

#[test]
fn a_tab_already_showing_the_panel_is_refreshed_with_a_frame() {
    let url = web_server();
    let (gatekeeper, _) = recording_gatekeeper();
    let assistant = assistant_serving(Voice);
    let mut core = Core::start(&gatekeeper, &with_assistant(&assistant));
    core.navigate(&url); // tab 1

    core.send(&ClientMessage::OpenTab {
        url: Some("about:assistant".into()),
    });
    let ServerMessage::TabOpened {
        tab_id: panel_tab, ..
    } = core.read()
    else {
        panic!("expected TabOpened");
    };
    assert!(matches!(core.read(), ServerMessage::FrameReady { .. }));
    assert!(names(&representation_of(&mut core, panel_tab))
        .iter()
        .any(|n| n.contains("Nothing here yet")));

    blueice_ipc::write_client_message_with_ids(
        &mut core.stream,
        Some(1),
        Some(77),
        &ClientMessage::SummarizePage,
    )
    .unwrap();
    let (tab, request, message) =
        blueice_ipc::read_server_message_with_ids(&mut core.stream).unwrap();
    assert_eq!((tab, request), (Some(1), Some(77)));
    assert!(matches!(message, ServerMessage::AssistantResult { .. }));
    // Unsolicited: the panel tab gets a fresh frame with no request id.
    let (tab, request, message) =
        blueice_ipc::read_server_message_with_ids(&mut core.stream).unwrap();
    assert_eq!((tab, request), (Some(panel_tab), None));
    let ServerMessage::FrameReady { generation, .. } = message else {
        panic!("expected FrameReady, got {message:?}");
    };
    let snapshot = representation_of(&mut core, panel_tab);
    assert_eq!(
        snapshot.generation, generation,
        "frame and snapshot share a render pass"
    );
    assert!(names(&snapshot)
        .iter()
        .any(|n| n == "SUMMARY OF: Hello / World"));
}

#[test]
fn a_failing_assistant_reports_why_to_the_requester_and_on_the_panel() {
    let url = web_server();
    let (gatekeeper, _) = recording_gatekeeper();
    let assistant = assistant_serving(Down);
    let mut core = Core::start(&gatekeeper, &with_assistant(&assistant));
    core.navigate(&url);

    core.send(&ClientMessage::SummarizePage);
    assert_eq!(
        core.read(),
        ServerMessage::Error {
            message: "model down".into()
        }
    );
    core.navigate("about:assistant");
    let text = names(&core.snapshot());
    assert!(
        text.iter()
            .any(|n| n.contains("Could not complete this request: model down")),
        "{text:?}"
    );
}

#[test]
fn refusals_that_need_no_assistant_are_immediate_errors() {
    let url = web_server();
    let (gatekeeper, _) = recording_gatekeeper();

    // No assistant configured.
    let mut core = Core::start(&gatekeeper, &[]);
    core.navigate(&url);
    core.send(&ClientMessage::SummarizePage);
    match core.read() {
        ServerMessage::Error { message } => assert!(message.contains("unavailable"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    drop(core);

    let assistant = assistant_serving(Voice);
    let mut core = Core::start(&gatekeeper, &with_assistant(&assistant));

    // A blank page has no text.
    core.send(&ClientMessage::SummarizePage);
    match core.read() {
        ServerMessage::Error { message } => assert!(message.contains("no text"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }

    // An empty or oversized instruction is refused before any task starts.
    core.navigate(&url);
    for instruction in [String::new(), "i".repeat(600)] {
        core.send(&ClientMessage::OrganizePage { instruction });
        assert!(matches!(core.read(), ServerMessage::Error { .. }));
    }

    // An unknown tab.
    blueice_ipc::write_client_message_with_ids(
        &mut core.stream,
        Some(999),
        None,
        &ClientMessage::SummarizePage,
    )
    .unwrap();
    assert!(matches!(core.read(), ServerMessage::Error { .. }));
}

#[test]
fn a_pending_task_never_blocks_core() {
    let url = web_server();
    let (gatekeeper, _) = recording_gatekeeper();
    let silent = silent_assistant();
    let mut core = Core::start(&gatekeeper, &with_assistant(&silent));
    core.navigate(&url);

    core.send(&ClientMessage::SummarizePage);
    let asked = Instant::now();
    core.send(&ClientMessage::ListTabs);
    assert!(matches!(core.read(), ServerMessage::Tabs(_)));
    assert!(
        asked.elapsed() < Duration::from_millis(1500),
        "core must answer other requests while the assistant is silent"
    );
    core.send(&ClientMessage::GetTranslationState);
    assert!(matches!(
        core.read(),
        ServerMessage::TranslationState { .. }
    ));
}
