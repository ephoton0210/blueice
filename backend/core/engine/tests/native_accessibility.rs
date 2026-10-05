// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]
mod common;
use blueice_ipc::accessibility::AccessibilityContext;
use blueice_ipc::{ClientMessage, NodeAction, ServerMessage};

#[test]
fn actual_core_announcements_and_scoped_reveal_share_the_live_frame() {
    let url = common::web_server_with_html("<div role='status'>Progress <input aria-label='Progress editor' value='1'></div><input type='password' aria-label='Secret' value='private-live-secret'><div style='height:1200px'></div><h2>Lower heading</h2><a href='/danger' style='display:block'>Do not follow</a>");
    let (gatekeeper, reviewed) = common::recording_gatekeeper();
    let mut core = common::Core::start(&gatekeeper, &[]);
    core.navigate(&url);
    let before = core.snapshot();
    assert!(before
        .accessibility
        .as_ref()
        .unwrap()
        .announcements
        .is_empty());
    let input = before
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("Progress editor"))
        .unwrap();
    core.send(&ClientMessage::ActOn {
        id: input.id,
        action: NodeAction::SetValue("2😀".into()),
    });
    assert!(matches!(core.read(), ServerMessage::FrameReady { .. }));
    let snapshot = core.snapshot();
    let stream = snapshot.accessibility.as_ref().unwrap();
    assert_eq!(stream.announcements[0].text, "Progress 2😀");
    assert!(!serde_json::to_string(stream)
        .unwrap()
        .contains("private-live-secret"));
    let heading = snapshot
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("Lower heading"))
        .unwrap();
    let context = AccessibilityContext {
        version: 1,
        frame_source: snapshot.frame_source,
        document_generation: stream.document_generation,
        frame_generation: snapshot.generation,
        node_id: heading.id,
    };
    core.send(&common::native_command(
        ClientMessage::AccessibilityReveal { context },
    ));
    let reply = loop {
        match core.read() {
            ServerMessage::AccessibilityRevealed(reply) => break reply,
            ServerMessage::FrameReady { .. }
            | ServerMessage::ViewportState(_)
            | ServerMessage::DisplayPreferencesState(_) => {}
            other => panic!("unexpected reveal reply {other:?}"),
        }
    };
    assert!(reply.context.frame_generation > snapshot.generation);
    assert_eq!(reply.context.node_id, heading.id);
    let revealed = core.snapshot();
    assert!(revealed.scroll_y > 900.0);
    assert_eq!(revealed.url.as_deref(), Some(url.as_str()));
    core.send(&common::native_command(
        ClientMessage::AccessibilityReveal { context },
    ));
    assert!(matches!(core.read(), ServerMessage::Error { .. }));
    assert_eq!(
        reviewed.lock().unwrap().len(),
        1,
        "AT navigation must not fetch or activate a link"
    );
}
