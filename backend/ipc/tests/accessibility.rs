// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_ipc::accessibility::{AccessibilityTextAction, AccessibilityTextContext};
use blueice_ipc::input::TextRange;
use blueice_ipc::{ClientMessage, ServerMessage};

#[test]
fn text_commands_preserve_utf16_ranges_and_document_identity_on_the_wire() {
    let context = AccessibilityTextContext {
        version: 1,
        frame_source: 9,
        document_generation: 3,
        frame_generation: 8,
        node_id: 7,
    };
    for action in [
        AccessibilityTextAction::Inspect,
        AccessibilityTextAction::Select {
            range: TextRange {
                location: 1,
                length: 2,
            },
        },
        AccessibilityTextAction::ReplaceSelection {
            text: "中文😀".into(),
        },
        AccessibilityTextAction::RangeForPosition { x: 1.5, y: 2.5 },
    ] {
        let message = ClientMessage::AccessibilityText {
            context,
            action: action.clone(),
        };
        let json = serde_json::to_vec(&message).unwrap();
        let ClientMessage::AccessibilityText {
            context: decoded_context,
            action: decoded_action,
        } = serde_json::from_slice(&json).unwrap()
        else {
            panic!("text command expected")
        };
        assert_eq!(decoded_context, context);
        assert_eq!(decoded_action, action);
    }
    let reply: ServerMessage = serde_json::from_str(r#"{"AccessibilityTextState":{"context":{"version":1,"frame_source":9,"document_generation":3,"frame_generation":8,"node_id":7},"result":{"Range":{"location":1,"length":2}}}}"#).unwrap();
    assert!(matches!(reply, ServerMessage::AccessibilityTextState(_)));
}
