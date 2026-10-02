// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_ipc as ipc;
use ipc::windows::*;
use ipc::{viewport::DisplayViewport, TabSummary};
#[test]
fn native_window_actions_and_registry_keep_typed_ids_and_nullable_urls() {
    let viewport = DisplayViewport {
        width: 300.0,
        height: 200.0,
        device_scale: 2.0,
        backing_scale: Some(2.0),
    };
    for action in [
        WindowAction::List,
        WindowAction::Command {
            window_id: 2,
            message: Box::new(ipc::ClientMessage::GetTextInputState),
        },
        WindowAction::Create { viewport },
        WindowAction::Resize {
            window_id: 2,
            viewport,
        },
        WindowAction::Close { window_id: 2 },
        WindowAction::MoveTab { window_id: 2 },
        WindowAction::OpenTab {
            window_id: 2,
            url: None,
        },
    ] {
        let command = ipc::ClientMessage::Window(action);
        let json = serde_json::to_string(&command).unwrap();
        assert_eq!(
            serde_json::from_str::<ipc::ClientMessage>(&json).unwrap(),
            command
        );
    }
    let state = ipc::ServerMessage::WindowState(WindowState {
        windows: vec![WindowSummary {
            id: 2,
            viewport,
            tabs: vec![TabSummary {
                id: 9,
                url: Some("about:credits".into()),
                group_id: Some(3),
            }],
        }],
        event: WindowEvent::TabMoved {
            tab_id: 9,
            from_window: 1,
            to_window: 2,
        },
    });
    let json = serde_json::to_string(&state).unwrap();
    assert_eq!(
        serde_json::from_str::<ipc::ServerMessage>(&json).unwrap(),
        state
    );
    assert!(serde_json::from_str::<WindowAction>(r#"{"MoveTab":{"window_id":true}}"#).is_err());
    assert!(serde_json::from_str::<WindowSummary>(r#"{"id":2,"tabs":[]}"#).is_err());
}
