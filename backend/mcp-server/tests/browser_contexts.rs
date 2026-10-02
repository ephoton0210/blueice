// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]
use blueice_ipc::browser_contexts::{ContextAction, ContextEvent, ContextState, ContextSummary};
use blueice_ipc::{ClientMessage, ServerMessage};
use blueice_mcp_server::CoreConnection;
use std::os::unix::net::UnixStream;
use std::time::Duration;

#[test]
fn context_listing_waits_for_exact_reply_and_retains_unsolicited_tab_frames() {
    let (client, mut server) = UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let worker = std::thread::spawn(move || {
        for fail in [false, true] {
            let (tab, request, message) =
                blueice_ipc::read_client_message_with_ids(&mut server).unwrap();
            assert_eq!(tab, None);
            assert_eq!(message, ClientMessage::BrowserContext(ContextAction::List));
            let stale = ContextState {
                contexts: vec![ContextSummary {
                    id: 1,
                    name: "Default".into(),
                    windows: vec![1],
                    groups: vec![],
                }],
                event: ContextEvent::Snapshot,
            };
            blueice_ipc::write_server_message_with_id(
                &mut server,
                None,
                &ServerMessage::BrowserContextState(stale),
            )
            .unwrap();
            blueice_ipc::write_server_message_with_ids(
                &mut server,
                Some(7),
                None,
                &ServerMessage::FrameReady {
                    shm_path: "/unused/context-frame.rgba".into(),
                    width: 10,
                    height: 20,
                    generation: 3,
                },
            )
            .unwrap();
            let reply = if fail {
                ServerMessage::Error {
                    message: "Context state unavailable".into(),
                }
            } else {
                ServerMessage::BrowserContextState(ContextState {
                    contexts: vec![
                        ContextSummary {
                            id: 1,
                            name: "Default".into(),
                            windows: vec![1],
                            groups: vec![],
                        },
                        ContextSummary {
                            id: 2,
                            name: "研究".into(),
                            windows: vec![4, 6],
                            groups: vec![],
                        },
                    ],
                    event: ContextEvent::Snapshot,
                })
            };
            blueice_ipc::write_server_message_with_id(&mut server, request, &reply).unwrap();
        }
    });
    let mut connection = CoreConnection::new(client);
    let contexts = connection.list_browser_contexts().unwrap().unwrap();
    assert_eq!(contexts.len(), 2);
    assert_eq!(contexts[1].windows, [4, 6]);
    assert_eq!(contexts[1].name, "研究");
    assert_eq!(connection.last_frame(Some(7)).unwrap().generation, 3);
    assert!(
        connection.last_frame(None).is_none(),
        "Human frames must not steal MCP's unqualified screenshot target"
    );
    assert_eq!(
        connection.list_browser_contexts().unwrap().unwrap_err(),
        "Context state unavailable"
    );
    worker.join().unwrap();
}
