// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]
mod common;
use blueice_ipc::accessibility::AccessibilityContext;
use blueice_ipc::{ClientMessage, NodeAction, ServerMessage};

#[test]
fn actual_core_descendant_atomic_relevant_names_and_acknowledgement() {
    use blueice_ipc::accessibility::AccessibilityDelivery;
    let url = common::web_server_with_html("<span id='label'>Score</span><div role='log'>Outer<div aria-atomic='true' aria-labelledby='label' aria-label='Fallback'>Count<input aria-label='Atomic editor' value='1'><input type='password' value='private-descendant-secret'><span aria-hidden='true'>hidden-descendant-secret</span><span aria-live='off'>quiet descendant</span></div></div><div role='status'>Outer status<div aria-atomic='false'><input aria-label='Narrow editor' value='1'></div></div><div role='log' aria-relevant='all'><div aria-relevant='removals'><input aria-label='Suppressed editor' value='1'></div></div>");
    let (gatekeeper, reviewed) = common::recording_gatekeeper();
    let mut core = common::Core::start(&gatekeeper, &[]);
    core.navigate(&url);
    let baseline = core.snapshot();
    for (name, value) in [
        ("Atomic editor", "完成 😀"),
        ("Narrow editor", "narrow 😀"),
        ("Suppressed editor", "silent"),
    ] {
        let editor = baseline
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some(name))
            .unwrap()
            .id;
        core.send(&ClientMessage::ActOn {
            id: editor,
            action: NodeAction::SetValue(value.into()),
        });
        assert!(matches!(core.read(), ServerMessage::FrameReady { .. }));
    }
    let updated = core.snapshot();
    let stream = updated.accessibility.as_ref().unwrap();
    assert_eq!(
        stream
            .announcements
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        ["Score Count 完成 😀", "narrow 😀"]
    );
    assert_eq!(stream.revision, 2);
    assert!(!serde_json::to_string(stream).unwrap().contains("secret"));
    assert_eq!(
        stream.announcements,
        core.snapshot().accessibility.unwrap().announcements
    );
    let delivery = AccessibilityDelivery {
        version: 1,
        frame_source: updated.frame_source,
        document_generation: stream.document_generation,
        revision: stream.revision,
    };
    core.send(&common::native_command(
        ClientMessage::AccessibilityAcknowledge { delivery },
    ));
    assert!(
        matches!(core.read(), ServerMessage::AccessibilityAcknowledged(reply) if reply == delivery)
    );
    let acknowledged = core.snapshot();
    assert_eq!(acknowledged.generation, updated.generation);
    assert!(acknowledged.accessibility.unwrap().announcements.is_empty());
    core.navigate(&url);
    assert!(core
        .snapshot()
        .accessibility
        .unwrap()
        .announcements
        .is_empty());
    assert_eq!(reviewed.lock().unwrap().len(), 2);
}

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

#[test]
fn actual_core_retains_and_acknowledges_only_the_owned_observed_prefix() {
    use blueice_ipc::accessibility::AccessibilityDelivery;
    use blueice_ipc::browser_contexts::{ContextAction, ContextEvent};
    use blueice_ipc::windows::WindowAction;
    let url = common::web_server_with_html("<div role='log'><input aria-label='Editor' value='ready'></div><input type='password' value='private-secret'>");
    let (gatekeeper, reviewed) = common::recording_gatekeeper();
    let mut core = common::Core::start(&gatekeeper, &[]);
    core.navigate(&url);
    let editor = core
        .snapshot()
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("Editor"))
        .unwrap()
        .id;
    core.send(&ClientMessage::ActOn {
        id: editor,
        action: NodeAction::Focus,
    });
    assert!(matches!(core.read(), ServerMessage::FrameReady { .. }));
    for value in ["first", "second"] {
        core.send(&ClientMessage::ActOn {
            id: editor,
            action: NodeAction::SetValue(value.into()),
        });
        assert!(matches!(core.read(), ServerMessage::FrameReady { .. }));
    }
    let observed = core.snapshot();
    core.send(&ClientMessage::Resize {
        width: 500,
        height: 240,
    });
    assert!(matches!(core.read(), ServerMessage::FrameReady { .. }));
    let before = core.snapshot();
    assert!(before.generation > observed.generation);
    let stream = before.accessibility.as_ref().unwrap();
    assert_eq!(stream.delivery_version, 1);
    assert_eq!(stream.revision, 2);
    assert_eq!(
        stream
            .announcements
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    core.send(&ClientMessage::GetTextInputState);
    let ServerMessage::TextInputState(input_before) = core.read() else {
        panic!("input state")
    };
    let delivery = AccessibilityDelivery {
        version: 1,
        frame_source: before.frame_source,
        document_generation: stream.document_generation,
        revision: 1,
    };
    for bad in [
        AccessibilityDelivery {
            version: 2,
            ..delivery
        },
        AccessibilityDelivery {
            frame_source: delivery.frame_source.wrapping_add(1),
            ..delivery
        },
        AccessibilityDelivery {
            document_generation: delivery.document_generation + 1,
            ..delivery
        },
        AccessibilityDelivery {
            revision: 3,
            ..delivery
        },
    ] {
        core.send(&common::native_command(
            ClientMessage::AccessibilityAcknowledge { delivery: bad },
        ));
        assert!(matches!(core.read(), ServerMessage::Error { .. }));
        assert_eq!(core.snapshot(), before);
    }
    core.send(&ClientMessage::BrowserContext(ContextAction::Create {
        name: "Foreign".into(),
    }));
    let ServerMessage::BrowserContextState(created) = core.read() else {
        panic!("context")
    };
    let ContextEvent::Created { context_id } = created.event else {
        panic!("created")
    };
    for (context, window) in [(context_id, 1), (1, 999)] {
        let message = ClientMessage::BrowserContext(ContextAction::Command {
            context_id: context,
            message: Box::new(ClientMessage::Window(WindowAction::Command {
                window_id: window,
                message: Box::new(ClientMessage::AccessibilityAcknowledge { delivery }),
            })),
        });
        core.send(&message);
        assert!(matches!(core.read(), ServerMessage::Error { .. }));
        assert_eq!(core.snapshot(), before);
    }
    for revision in [1, 1, 0, 2] {
        let request = AccessibilityDelivery {
            revision,
            ..delivery
        };
        core.send(&common::native_command(
            ClientMessage::AccessibilityAcknowledge { delivery: request },
        ));
        let ServerMessage::AccessibilityAcknowledged(reply) = core.read() else {
            panic!("ACK must have no preceding frame")
        };
        assert_eq!(reply.revision, revision.max(1));
        let after = core.snapshot();
        assert_eq!(after.generation, before.generation);
        assert_eq!(after.scroll_y, before.scroll_y);
        assert_eq!(after.url, before.url);
        assert_eq!(after.nodes, before.nodes);
        let pending = after.accessibility.as_ref().unwrap();
        assert_eq!(pending.acknowledged_revision, revision.max(1));
        assert_eq!(pending.announcements.len(), usize::from(revision < 2));
        if revision < 2 {
            assert_eq!(pending.announcements[0].text, "second");
        }
        core.send(&ClientMessage::GetTextInputState);
        let ServerMessage::TextInputState(input_after) = core.read() else {
            panic!("input after")
        };
        assert_eq!(input_after, input_before);
    }
    assert_eq!(
        reviewed.lock().unwrap().len(),
        1,
        "Acknowledgement must not review or fetch"
    );
    core.navigate(&url);
    let replacement = core.snapshot();
    core.send(&common::native_command(
        ClientMessage::AccessibilityAcknowledge { delivery },
    ));
    assert!(matches!(core.read(), ServerMessage::Error { .. }));
    assert_eq!(core.snapshot(), replacement);
}
