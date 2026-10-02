// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_ipc::browser_contexts::{ContextAction, ContextEvent, ContextState, ContextSummary};
use blueice_ipc::viewport::DisplayViewport;
use blueice_ipc::windows::WindowAction;
use blueice_ipc::{ClientMessage, ServerMessage, TabGroupSummary};

#[test]
fn contexts_round_trip_without_changing_existing_tab_and_window_payloads() {
    let actions = [
        ContextAction::List,
        ContextAction::Create {
            name: "研究".into(),
        },
        ContextAction::Rename {
            context_id: 2,
            name: "Work".into(),
        },
        ContextAction::Close { context_id: 2 },
        ContextAction::Command {
            context_id: 2,
            message: Box::new(ClientMessage::Window(WindowAction::CreateInContext {
                context_id: 2,
                viewport: DisplayViewport {
                    width: 300.0,
                    height: 200.0,
                    device_scale: 1.0,
                    backing_scale: None,
                },
            })),
        },
    ];
    for action in actions {
        let message = ClientMessage::BrowserContext(action);
        let mut bytes = Vec::new();
        blueice_ipc::write_client_message_with_ids(&mut bytes, Some(9), Some(7), &message).unwrap();
        assert_eq!(
            blueice_ipc::read_client_message_with_ids(&mut bytes.as_slice()).unwrap(),
            (Some(9), Some(7), message)
        );
    }
    for event in [
        ContextEvent::Snapshot,
        ContextEvent::Created { context_id: 2 },
        ContextEvent::Renamed { context_id: 2 },
        ContextEvent::Closed { context_id: 3 },
    ] {
        let message = ServerMessage::BrowserContextState(ContextState {
            contexts: vec![ContextSummary {
                id: 2,
                name: "研究".into(),
                windows: vec![4, 7],
                groups: vec![TabGroupSummary {
                    id: 5,
                    name: "Sources".into(),
                    color: "#4477cc".into(),
                    collapsed: false,
                }],
            }],
            event,
        });
        let mut bytes = Vec::new();
        blueice_ipc::write_server_message_with_id(&mut bytes, Some(7), &message).unwrap();
        assert_eq!(
            blueice_ipc::read_server_message_with_ids(&mut bytes.as_slice()).unwrap(),
            (None, Some(7), message)
        );
    }
}
