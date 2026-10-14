// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_ipc::{AiSnapshot, ClientMessage, ServerMessage};

#[test]
fn old_snapshots_have_no_notification_stream() {
    let snapshot: AiSnapshot =
        serde_json::from_str(r#"{"generation":1,"tab_id":2,"url":null,"scroll_y":0,"nodes":[]}"#)
            .unwrap();
    assert!(snapshot.accessibility.is_none());
    assert!(serde_json::to_value(snapshot)
        .unwrap()
        .get("accessibility")
        .is_none());
}

#[test]
fn announcements_preserve_document_revision_and_unicode() {
    let snapshot: AiSnapshot = serde_json::from_str(
        r#"{"generation":8,"tab_id":2,"url":null,"scroll_y":0,"nodes":[],"accessibility":{"document_generation":3,"revision":7,"announcements":[{"sequence":7,"region_id":19,"text":"完成 😀","politeness":"Assertive"}]}}"#,
    )
    .unwrap();
    let stream = snapshot.accessibility.as_ref().unwrap();
    assert_eq!(stream.document_generation, 3);
    assert_eq!(stream.revision, 7);
    assert_eq!(stream.acknowledged_revision, 0);
    assert_eq!(
        stream.delivery_version, 0,
        "Old streams do not advertise acknowledgement support"
    );
    assert_eq!(stream.announcements[0].text, "完成 😀");
    assert_eq!(
        serde_json::from_value::<AiSnapshot>(serde_json::to_value(&snapshot).unwrap()).unwrap(),
        snapshot
    );
}

#[test]
fn acknowledgement_wire_preserves_document_prefix_without_frame_or_input_authority() {
    let json = r#"{"AccessibilityAcknowledge":{"delivery":{"version":1,"frame_source":4,"document_generation":3,"revision":7}}}"#;
    let command: ClientMessage = serde_json::from_str(json).unwrap();
    let ClientMessage::AccessibilityAcknowledge { delivery } = command else {
        panic!("delivery command")
    };
    assert_eq!(delivery.revision, 7);
    assert_eq!(
        serde_json::to_value(ClientMessage::AccessibilityAcknowledge { delivery }).unwrap(),
        serde_json::from_str::<serde_json::Value>(json).unwrap(),
    );
    let reply = ServerMessage::AccessibilityAcknowledged(delivery);
    let ServerMessage::AccessibilityAcknowledged(round_trip) =
        serde_json::from_value(serde_json::to_value(reply).unwrap()).unwrap()
    else {
        panic!("delivery reply")
    };
    assert_eq!(round_trip, delivery);
    let snapshot: AiSnapshot = serde_json::from_str(
        r#"{"generation":8,"tab_id":2,"url":null,"scroll_y":0,"nodes":[],"accessibility":{"document_generation":3,"revision":7,"acknowledged_revision":7,"delivery_version":1,"announcements":[]}}"#,
    ).unwrap();
    let stream = snapshot.accessibility.unwrap();
    assert_eq!(stream.acknowledged_revision, 7);
    assert_eq!(stream.delivery_version, 1);
}

#[test]
fn reveal_has_an_exact_context_and_no_activation_action() {
    let json = r#"{"AccessibilityReveal":{"context":{"version":1,"frame_source":4,"document_generation":3,"frame_generation":8,"node_id":19}}}"#;
    let command: ClientMessage = serde_json::from_str(json).unwrap();
    assert!(matches!(command, ClientMessage::AccessibilityReveal { .. }));
    assert_eq!(
        serde_json::to_value(command).unwrap(),
        serde_json::from_str::<serde_json::Value>(json).unwrap()
    );
    let reply: ServerMessage = serde_json::from_str(r#"{"AccessibilityRevealed":{"context":{"version":1,"frame_source":4,"document_generation":3,"frame_generation":9,"node_id":19},"bounds":{"x":2,"y":1000,"width":40,"height":30}}}"#).unwrap();
    assert!(matches!(reply, ServerMessage::AccessibilityRevealed(_)));
}
