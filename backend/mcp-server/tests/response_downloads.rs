// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

use blueice_ipc::{AiSnapshot, ClientMessage, ServerMessage};
use blueice_mcp_server::CoreConnection;
use std::os::unix::net::UnixStream;
use std::time::Duration;

#[test]
fn representation_ignores_native_activation_hints_and_returns_only_its_snapshot() {
    let (client, mut server) = UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let snapshot = AiSnapshot {
        accessibility: None,
        frame_source: 7,
        generation: 8,
        tab_id: 1,
        url: Some("https://example.com/retained".into()),
        scroll_y: 0.0,
        nodes: vec![],
    };
    let retained = snapshot.clone();
    let worker = std::thread::spawn(move || {
        let (tab, request, message) = blueice_ipc::read_client_message_with_ids(&mut server)?;
        assert_eq!(message, ClientMessage::GetRepresentation);
        for file_input in [
            None,
            Some(blueice_ipc::file_input::FileInputState {
                context: blueice_ipc::file_input::FileInputContext {
                    tab_id: 1,
                    frame_source: 7,
                    document_generation: 8,
                    node_id: 11,
                    revision: 2,
                },
                multiple: false,
                accept: ".txt".into(),
                names: vec!["selected.txt".into()],
            }),
        ] {
            blueice_ipc::write_server_message_with_ids(
                &mut server,
                tab,
                request,
                &ServerMessage::NativeActivationCompleted {
                    gesture: 42,
                    file_input,
                },
            )?;
        }
        blueice_ipc::write_server_message_with_ids(
            &mut server,
            tab,
            request,
            &ServerMessage::Representation(snapshot),
        )
    });
    let result = CoreConnection::new(client).representation(Some(1));
    worker.join().unwrap().unwrap();
    assert_eq!(result.unwrap(), retained);
}

#[test]
fn download_navigation_returns_current_snapshot_without_authorizing_body_handoff() {
    let (client, mut server) = UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let snapshot = AiSnapshot {
        accessibility: None,
        frame_source: 0,
        generation: 7,
        tab_id: 1,
        url: Some("https://example.com/retained".into()),
        scroll_y: 0.0,
        nodes: vec![],
    };
    let retained = snapshot.clone();
    let worker = std::thread::spawn(move || {
        let (tab, request, message) = blueice_ipc::read_client_message_with_ids(&mut server)?;
        assert_eq!(
            message,
            ClientMessage::Navigate {
                url: "https://example.com/file".into()
            }
        );
        let offered = ServerMessage::NavigationDownloadOffered {
            navigation_id: 8,
            current_url: snapshot.url.clone(),
        };
        blueice_ipc::write_server_message_with_ids(&mut server, tab, Some(999), &offered)?;
        blueice_ipc::write_server_message_with_ids(&mut server, tab, request, &offered)?;
        let (tab, request, message) = blueice_ipc::read_client_message_with_ids(&mut server)?;
        assert_eq!(
            message,
            ClientMessage::GetRepresentation,
            "MCP must not accept or replay the download body"
        );
        blueice_ipc::write_server_message_with_ids(
            &mut server,
            tab,
            request,
            &ServerMessage::Representation(snapshot),
        )
    });
    let result = CoreConnection::new(client).navigate("https://example.com/file", Some(1));
    let served = worker.join().unwrap();
    served.unwrap();
    let outcome = result.unwrap();
    assert!(outcome.error.unwrap().contains("download response"));
    assert_eq!(outcome.snapshot, retained);
}
