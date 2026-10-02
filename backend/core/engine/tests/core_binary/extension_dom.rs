// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn installed_extension_reads_a_real_core_owned_representation_over_private_sockets() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    let core_socket = unique_socket_path("extension-core");
    let extension_socket = unique_private_extension_socket_path("read");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("real-read", &["dom:read"]);
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with an installed extension");

    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([("dom:read".to_string(), 1)]),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(&mut extension, &ExtensionRequest::DomRead).unwrap();
    let snapshot = match read_extension_reply(&mut extension).unwrap() {
        ExtensionReply::DomReadResult { value } => {
            serde_json::from_str::<blueice_ipc::AiSnapshot>(&value).unwrap()
        }
        other => panic!("expected a core-backed DomReadResult, got {other:?}"),
    };
    assert_eq!(snapshot.tab_id, 1);
    assert_eq!(snapshot.url.as_deref(), Some("about:credits"));
    assert!(
        !snapshot.nodes.is_empty(),
        "the extension must receive the navigated core page, not an empty initial snapshot"
    );

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn installed_extension_origin_scopes_follow_the_live_page_across_navigation() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    let core_socket = unique_socket_path("eosc");
    let extension_socket = unique_private_extension_socket_path("eosc");
    let gatekeeper_socket = clearing_gatekeeper("eosg");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-origin-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, _) = extension_manifest_package(
        "origin-scope",
        &["dom:read", "dom:write", "network:observe"],
    );
    let allowed_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let allowed_addr = allowed_listener.local_addr().unwrap();
    let blocked_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let blocked_addr = blocked_listener.local_addr().unwrap();
    std::fs::write(
        &manifest,
        format!(
            r#"{{"name":"Core bridge test","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{{"declared":["dom:read","dom:write","network:observe"]}},"capability_origins":{{"dom:read":["http://{allowed_addr}"],"dom:write":["http://{allowed_addr}"],"network:observe":["http://{allowed_addr}"]}}}}"#
        ),
    )
    .unwrap();
    let extension_id = blueice_extension_host::load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    for (listener, body) in [
        (allowed_listener, "<h1 id=\"title\">Allowed</h1>"),
        (blocked_listener, "<h1 id=\"title\">Blocked</h1>"),
    ] {
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        });
    }
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_socket.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with an origin-scoped extension");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([
                ("dom:read".to_string(), 2),
                ("dom:write".to_string(), 9),
                ("network:observe".to_string(), 2),
            ]),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );

    let navigate_and_heading = |frontend: &mut UnixStream, url: String| {
        blueice_ipc::write_client_message(frontend, &blueice_ipc::ClientMessage::Navigate { url })
            .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(frontend).unwrap(),
            blueice_ipc::ServerMessage::Navigated { .. }
        ));
        assert!(matches!(
            blueice_ipc::read_server_message(frontend).unwrap(),
            blueice_ipc::ServerMessage::FrameReady { .. }
        ));
        blueice_ipc::write_client_message(frontend, &blueice_ipc::ClientMessage::GetRepresentation)
            .unwrap();
        let blueice_ipc::ServerMessage::Representation(snapshot) =
            blueice_ipc::read_server_message(frontend).unwrap()
        else {
            panic!("expected a page representation after navigation")
        };
        snapshot
            .nodes
            .iter()
            .find(|node| matches!(node.role, blueice_ipc::Role::Heading { .. }))
            .expect("the fetched page must expose its heading")
            .id
    };

    let allowed_heading =
        navigate_and_heading(&mut frontend, format!("http://{allowed_addr}/page"));
    write_extension_request(&mut extension, &ExtensionRequest::DomReadTab { tab_id: 1 }).unwrap();
    assert!(matches!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomReadResult { .. }
    ));
    write_extension_request(
        &mut extension,
        &ExtensionRequest::ReadNetworkResponse { tab_id: 1 },
    )
    .unwrap();
    assert!(matches!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::NetworkResponseResult { response: Some(_) }
    ));
    write_extension_request(
        &mut extension,
        &ExtensionRequest::SetVisibleLeafText {
            tab_id: 1,
            node_id: allowed_heading,
            value: "Allowed update".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::DomWriteAck
    );
    let (_, _, frame) = blueice_ipc::read_server_message_with_ids(&mut frontend).unwrap();
    assert!(matches!(
        frame,
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    let blocked_heading =
        navigate_and_heading(&mut frontend, format!("http://{blocked_addr}/page"));
    for (request, capability) in [
        (ExtensionRequest::DomReadTab { tab_id: 1 }, "dom:read"),
        (
            ExtensionRequest::ReadNetworkResponse { tab_id: 1 },
            "network:observe",
        ),
        (
            ExtensionRequest::ReadNetworkTrace { tab_id: 1 },
            "network:observe",
        ),
        (
            ExtensionRequest::SetVisibleLeafText {
                tab_id: 1,
                node_id: blocked_heading,
                value: "Must not change".to_string(),
            },
            "dom:write",
        ),
        (
            ExtensionRequest::SetVisibleTextContent {
                tab_id: 1,
                node_id: blocked_heading,
                value: "Must not change".to_string(),
            },
            "dom:write",
        ),
    ] {
        write_extension_request(&mut extension, &request).unwrap();
        assert!(matches!(
            read_extension_reply(&mut extension).unwrap(),
            ExtensionReply::OperationUnavailable { capability: denied, reason }
                if denied == capability && reason.contains("origin")
        ));
    }
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::GetRepresentation,
    )
    .unwrap();
    let blueice_ipc::ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut frontend).unwrap()
    else {
        panic!("the rejected write must not publish a frame or change the page")
    };
    assert!(snapshot.nodes.iter().any(|node| {
        matches!(node.role, blueice_ipc::Role::Heading { .. })
            && node.name.as_deref() == Some("Blocked")
    }));

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    let _ = std::fs::remove_file(gatekeeper_socket);
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn installed_extension_v6_redirects_one_same_origin_navigation_before_fetch_and_clears_it() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    let core_socket = unique_socket_path("er6");
    let extension_socket = unique_private_extension_socket_path("r6");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-redirect-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("same-origin-redirect", &["network:intercept"]);
    let gatekeeper_socket = clearing_gatekeeper("r6g");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let web_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", web_listener.local_addr().unwrap());
    let source_url = format!("{origin}/old");
    let target_url = format!("{origin}/new");
    let web_server = thread::spawn(move || {
        for (path, body) in [
            ("/new", "<h1>Rewritten</h1>"),
            ("/old", "<h1>Original</h1>"),
        ] {
            let (mut stream, _) = web_listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let len = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..len]);
            assert!(request.starts_with(&format!("GET {path} ")), "{request}");
            stream.write_all(format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len(),
            ).as_bytes()).unwrap();
        }
    });

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_socket.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with a v6 redirect extension");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([("network:intercept".to_string(), 6)]),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(
        &mut extension,
        &ExtensionRequest::RegisterNetworkRedirectUrl {
            source_url: source_url.clone(),
            target_url: target_url.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );

    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: source_url.clone(),
        },
    )
    .unwrap();
    assert!(
        matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url, .. } if url == target_url)
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    write_extension_request(&mut extension, &ExtensionRequest::ClearNetworkBlockUrls).unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: source_url.clone(),
        },
    )
    .unwrap();
    assert!(
        matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url, .. } if url == source_url)
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    web_server.join().unwrap();
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_dir_all(package_root);
    let _ = std::fs::remove_file(gatekeeper_socket);
}
