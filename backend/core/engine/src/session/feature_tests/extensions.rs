// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn extension_observes_only_the_committed_http_response_for_a_live_tab() {
    let gatekeeper = clearing_gatekeeper("extension-network-observe");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/before", listener.local_addr().unwrap());
    let final_url = format!("http://{}/after", listener.local_addr().unwrap());
    let http = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut request);
        std::io::Write::write_all(
            &mut stream,
            b"HTTP/1.1 302 Found\r\nLocation: /after\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let _ = std::io::Read::read(&mut stream, &mut request);
        std::io::Write::write_all(
            &mut stream,
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nSet-Cookie: secret=never-expose\r\nContent-Length: 9\r\nConnection: close\r\n\r\n<p>ok</p>",
        )
        .unwrap();
    });
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-network-observe");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let session = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });
    handshake(&mut client);
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkResponse {
            tab_id: 1,
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        reply_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
        None
    );
    let (trace_tx, trace_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkTrace {
            tab_id: 1,
            reply: trace_tx,
        })
        .unwrap();
    assert_eq!(
        trace_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
        None
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated {
            url: final_url.clone()
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkResponse {
            tab_id: 1,
            reply: reply_tx,
        })
        .unwrap();
    let response = reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(response.method, "GET");
    assert_eq!(response.final_url, final_url);
    assert_eq!(response.status, 200);
    assert_eq!(
        response.content_type.as_deref(),
        Some("text/html; charset=utf-8")
    );
    assert!(!format!("{response:?}").contains("secret"));
    let (trace_tx, trace_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkTrace {
            tab_id: 1,
            reply: trace_tx,
        })
        .unwrap();
    let trace = trace_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(trace.request_url, url);
    assert_eq!(
        trace.redirects,
        vec![blueice_ipc::extension::NetworkRedirectInfo {
            request_url: url.clone(),
            status: 302,
            target_url: final_url.clone(),
        }]
    );
    assert_eq!(trace.response, response);
    assert!(!format!("{trace:?}").contains("secret"));

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:blank".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkResponse {
            tab_id: 1,
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        reply_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
        None
    );
    let (trace_tx, trace_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkTrace {
            tab_id: 1,
            reply: trace_tx,
        })
        .unwrap();
    assert_eq!(
        trace_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
        None
    );
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkResponse {
            tab_id: 99,
            reply: reply_tx,
        })
        .unwrap();
    assert!(reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_err());
    let (trace_tx, trace_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkTrace {
            tab_id: 99,
            reply: trace_tx,
        })
        .unwrap();
    assert!(trace_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_err());
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    session.join().unwrap().unwrap();
    http.join().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn committed_navigation_emits_a_bounded_core_defined_extension_event() {
    let (mut client, mut server) = client_pair();
    let (_extension_tx, extension_rx) = mpsc::channel();
    let (events_tx, events_rx) = mpsc::sync_channel(1);
    let dir = temp_frame_dir("extension-navigation-event");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let gatekeeper = PathBuf::from("/not-used-for-built-in-navigation");
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        run_session_with_extension_requests_and_events(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
            Some(&events_tx),
        )
    });

    blueice_ipc::client_handshake(&mut client).unwrap();
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    assert_eq!(
        events_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        ExtensionRuntimeEvent::NavigationCommitted { tab_id: 1 }
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}
