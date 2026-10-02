// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn extension_network_rule_blocks_a_matching_navigation_before_gatekeeper_or_fetch() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-navigation-block-rule");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    // This path has no listener. A matching navigation must still produce
    // the synchronous declarative-rule error rather than attempting the
    // ordinary gatekeeper/fetch background path.
    let gatekeeper = PathBuf::from("/not-reached-for-extension-navigation-rule.sock");
    let handle = thread::spawn(move || {
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

    blueice_ipc::client_handshake(&mut client).unwrap();
    let (rule_reply_tx, rule_reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::RegisterNetworkBlockUrl {
            connection_id: 77,
            grant_generation: 0,
            url: "https://example.test/private#fragment".to_string(),
            reply: rule_reply_tx,
        })
        .unwrap();
    rule_reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must answer the extension rule request")
        .expect("a valid HTTPS rule must be installed");

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "https://example.test/private".to_string(),
        },
    )
    .unwrap();
    match blueice_ipc::read_server_message(&mut client).unwrap() {
        ServerMessage::Error { message } => {
            assert!(message.contains("declarative extension rule"));
            assert!(message.contains("https://example.test/private"));
        }
        other => {
            panic!("matching extension rule must stop navigation before fetch, got {other:?}")
        }
    }

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_network_rule_blocks_a_redirect_target_before_its_connection() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-redirect-navigation-block-rule");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    target_listener.set_nonblocking(true).unwrap();
    let blocked_url = format!("http://{}/blocked", target_listener.local_addr().unwrap());
    let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let redirect_url = format!("http://{}/before", redirect_listener.local_addr().unwrap());
    let redirect_server = thread::spawn({
        let blocked_url = blocked_url.clone();
        move || {
            let (mut stream, _) = redirect_listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: {blocked_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
    });
    let gatekeeper = clearing_gatekeeper("extension-redirect-rule");
    let handle = thread::spawn(move || {
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

    blueice_ipc::client_handshake(&mut client).unwrap();
    let (rule_reply_tx, rule_reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::RegisterNetworkBlockUrl {
            connection_id: 78,
            grant_generation: 0,
            url: blocked_url.clone(),
            reply: rule_reply_tx,
        })
        .unwrap();
    rule_reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must accept the redirect-target rule")
        .expect("the target URL is a valid rule");

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: redirect_url })
        .unwrap();
    match blueice_ipc::read_server_message(&mut client).unwrap() {
        ServerMessage::Error { message } => {
            assert!(message.contains("declarative extension rule"));
            assert!(message.contains(&blocked_url));
        }
        other => panic!("the redirect target must be blocked, got {other:?}"),
    }
    redirect_server.join().unwrap();
    std::thread::sleep(Duration::from_millis(25));
    assert!(matches!(
        target_listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_host_rule_blocks_a_redirect_target_before_its_connection() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-host-redirect-rule");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    target_listener.set_nonblocking(true).unwrap();
    let blocked_url = format!(
        "http://localhost:{}/blocked",
        target_listener.local_addr().unwrap().port()
    );
    let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let redirect_url = format!("http://{}/before", redirect_listener.local_addr().unwrap());
    let redirect_server = thread::spawn({
        let blocked_url = blocked_url.clone();
        move || {
            let (mut stream, _) = redirect_listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            stream.write_all(
                format!("HTTP/1.1 302 Found\r\nLocation: {blocked_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes(),
            ).unwrap();
        }
    });
    let gatekeeper = clearing_gatekeeper("extension-host-redirect-rule");
    let handle = thread::spawn(move || {
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

    blueice_ipc::client_handshake(&mut client).unwrap();
    let (rule_reply_tx, rule_reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::RegisterNetworkBlockHost {
            connection_id: 78,
            grant_generation: 0,
            host: "localhost".to_string(),
            reply: rule_reply_tx,
        })
        .unwrap();
    rule_reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: redirect_url })
        .unwrap();
    match blueice_ipc::read_server_message(&mut client).unwrap() {
        ServerMessage::Error { message } => {
            assert!(message.contains("declarative extension rule"));
            assert!(message.contains(&blocked_url));
        }
        other => panic!("the redirect target must be blocked, got {other:?}"),
    }
    redirect_server.join().unwrap();
    std::thread::sleep(Duration::from_millis(25));
    assert!(matches!(
        target_listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}
