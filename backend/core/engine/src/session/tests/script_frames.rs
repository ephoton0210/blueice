// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A page-script DOM change made between client requests (a timer, later a
//! network completion) must reach the client as a new frame on the same
//! session tick, not wait for an unrelated request to carry it.

use super::*;
use blueice_ipc::script::{ScriptDocumentTarget, ScriptReply, ScriptRequest};
use std::time::Duration;

#[test]
fn script_text_change_between_requests_publishes_a_new_frame() {
    let dir = temp_frame_dir("script-frame-push");
    let gatekeeper = clearing_gatekeeper("script-frame-push");
    let (mut client, mut server) = client_pair();
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();

    let mut tabs = TabManager::new(320.0, 200.0);
    let tab = tabs.default_tab();
    default_page(&mut tabs).load_html_str("<p id='t'>one</p>", None);
    let target = ScriptDocumentTarget {
        tab_id: tab.as_u64(),
        document_generation: tabs.get(tab).unwrap().document_generation(),
    };

    let (sender, receiver) = crate::script::script_request_channel();
    let handle = thread::spawn(move || {
        let mut generation = 0u64;
        run_session_with_script_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            Some(&receiver),
        )
        .unwrap();
        dir
    });
    handshake(&mut client);

    let script = thread::spawn(move || {
        let ScriptReply::Node { node: Some(node) } = sender
            .request(ScriptRequest::GetElementById {
                target,
                id: "t".to_string(),
            })
            .unwrap()
        else {
            panic!("the paragraph resolves");
        };
        assert_eq!(
            sender
                .request(ScriptRequest::SetTextContent {
                    target,
                    node,
                    value: "two".to_string(),
                })
                .unwrap(),
            ScriptReply::Ack
        );
    });

    // No request was sent after the handshake, yet a frame must arrive.
    let first = blueice_ipc::read_server_message(&mut client).unwrap();
    let ServerMessage::FrameReady { generation, .. } = first else {
        panic!("expected an unprompted FrameReady, got {first:?}");
    };
    assert!(generation >= 1);
    script.join().unwrap();

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_quiet_session_pushes_no_frames_on_its_own() {
    let dir = temp_frame_dir("script-frame-quiet");
    let gatekeeper = clearing_gatekeeper("script-frame-quiet");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session_with_script_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            None,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);
    client
        .set_read_timeout(Some(Duration::from_millis(400)))
        .unwrap();
    assert!(
        blueice_ipc::read_server_message(&mut client).is_err(),
        "an idle session must not invent frames"
    );
    client.set_read_timeout(None).unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
