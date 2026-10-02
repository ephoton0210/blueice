// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn installed_extension_v2_observes_only_a_committed_redirect_trace() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    let core_socket = unique_socket_path("ext-network-trace");
    let extension_socket = unique_private_extension_socket_path("trace");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-trace-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("network-trace", &["network:observe"]);
    let gatekeeper_socket = clearing_gatekeeper("ent");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let final_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let final_url = format!("http://{}/final", final_listener.local_addr().unwrap());
    let final_server = thread::spawn(move || {
        let (mut stream, _) = final_listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request);
        stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nSet-Cookie: private=never-expose\r\nContent-Length: 12\r\nConnection: close\r\n\r\n<p>final</p>",
        ).unwrap();
    });
    let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let request_url = format!("http://{}/start", redirect_listener.local_addr().unwrap());
    let redirect_server = thread::spawn({
        let final_url = final_url.clone();
        move || {
            let (mut stream, _) = redirect_listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            stream.write_all(format!(
                "HTTP/1.1 302 Found\r\nLocation: {final_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
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
        .expect("failed to spawn core for committed network trace test");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([("network:observe".to_string(), 2)]),
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
        &ExtensionRequest::ReadNetworkTrace { tab_id: 1 },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::NetworkTraceResult { trace: None }
    );

    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: request_url.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated {
            url: final_url.clone()
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));
    write_extension_request(
        &mut extension,
        &ExtensionRequest::ReadNetworkTrace { tab_id: 1 },
    )
    .unwrap();
    let ExtensionReply::NetworkTraceResult { trace: Some(trace) } =
        read_extension_reply(&mut extension).unwrap()
    else {
        panic!("the committed page must expose its reviewed redirect trace")
    };
    assert_eq!(trace.request_url, request_url);
    assert_eq!(
        trace.redirects,
        vec![blueice_ipc::extension::NetworkRedirectInfo {
            request_url,
            status: 302,
            target_url: final_url.clone(),
        }]
    );
    assert_eq!(trace.response.final_url, final_url);
    assert_eq!(trace.response.status, 200);
    assert_eq!(trace.response.content_type.as_deref(), Some("text/html"));
    assert!(!format!("{trace:?}").contains("private=never-expose"));
    write_extension_request(
        &mut extension,
        &ExtensionRequest::ReadNetworkResponse { tab_id: 1 },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::NetworkResponseResult {
            response: Some(trace.response),
        }
    );

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    redirect_server.join().unwrap();
    final_server.join().unwrap();
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    let _ = std::fs::remove_file(gatekeeper_socket);
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn installed_extension_v3_network_rule_clear_restores_navigation_after_a_redirect_block() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;
    use std::io::ErrorKind;

    let core_socket = unique_socket_path("ext-network-rule");
    let extension_socket = unique_private_extension_socket_path("network-rule");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-network-rule-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("exact-network-rule", &["network:intercept"]);
    let gatekeeper_socket = clearing_gatekeeper("enrg");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/private", listener.local_addr().unwrap());
    let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let redirect_url = format!("http://{}/before", redirect_listener.local_addr().unwrap());
    let redirect_server = thread::spawn({
        let url = url.clone();
        move || {
            let (mut stream, _) = redirect_listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: {url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
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
        .expect("failed to spawn core with a v3 network-rule extension");

    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([("network:intercept".to_string(), 3)]),
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
        &ExtensionRequest::RegisterNetworkBlockUrl { url: url.clone() },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );

    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate { url: redirect_url },
    )
    .unwrap();
    match blueice_ipc::read_server_message(&mut frontend).unwrap() {
        blueice_ipc::ServerMessage::Error { message } => {
            assert!(message.contains("declarative extension rule"));
            assert!(message.contains(&url));
        }
        other => panic!("matching network rule must reject the navigation, got {other:?}"),
    }
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        ErrorKind::WouldBlock,
        "the extension rule must prevent core from opening an HTTP connection"
    );
    redirect_server.join().unwrap();

    // Clearing is a version-3 operation that can only remove rules from this
    // extension connection. It needs no new gatekeeper action review because
    // it reduces, rather than adds, privileged network policy.
    write_extension_request(&mut extension, &ExtensionRequest::ClearNetworkBlockUrls).unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );
    listener.set_nonblocking(false).unwrap();
    let target_server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 19\r\nConnection: close\r\n\r\n<p>now allowed</p>\n",
            )
            .unwrap();
    });
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url }
    );
    target_server.join().unwrap();

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_dir_all(package_root);
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn installed_extension_v4_host_rule_blocks_redirect_before_target_connection() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;
    use std::io::ErrorKind;

    let core_socket = unique_socket_path("ext-host-rule");
    let extension_socket = unique_private_extension_socket_path("host-rule");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-host-rule-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("host-network-rule", &["network:intercept"]);
    let gatekeeper_socket = clearing_gatekeeper("ehrg");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    target_listener.set_nonblocking(true).unwrap();
    let blocked_url = format!(
        "http://localhost:{}/private",
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
            stream.write_all(format!(
                "HTTP/1.1 302 Found\r\nLocation: {blocked_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
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
        .expect("failed to spawn core with a v4 host-rule extension");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([("network:intercept".to_string(), 4)]),
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
        &ExtensionRequest::RegisterNetworkBlockHost {
            host: "LOCALHOST.".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );

    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate { url: redirect_url },
    )
    .unwrap();
    match blueice_ipc::read_server_message(&mut frontend).unwrap() {
        blueice_ipc::ServerMessage::Error { message } => {
            assert!(message.contains("declarative extension rule"));
            assert!(message.contains(&blocked_url));
        }
        other => panic!("the v4 host rule must reject the redirect target, got {other:?}"),
    }
    redirect_server.join().unwrap();
    assert_eq!(
        target_listener.accept().unwrap_err().kind(),
        ErrorKind::WouldBlock
    );

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_dir_all(package_root);
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn scoped_network_intercept_rule_affects_only_its_exact_origin_and_redirect_targets() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;
    use std::io::ErrorKind;

    let core_socket = unique_socket_path("eisc");
    let extension_socket = unique_private_extension_socket_path("eisc");
    let gatekeeper_socket = clearing_gatekeeper("eisg");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-intercept-scope-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, _) =
        extension_manifest_package("intercept-scope", &["network:intercept"]);
    let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    target_listener.set_nonblocking(true).unwrap();
    let target_url = format!("http://{}/private", target_listener.local_addr().unwrap());
    let target_origin = format!("http://{}", target_listener.local_addr().unwrap());
    std::fs::write(
        &manifest,
        format!(
            r#"{{"name":"Core bridge test","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{{"declared":["network:intercept"]}},"capability_origins":{{"network:intercept":["{target_origin}"]}}}}"#
        ),
    )
    .unwrap();
    let extension_id = blueice_extension_host::load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    let open_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let open_origin = format!("http://{}", open_listener.local_addr().unwrap());
    let open_server = thread::spawn({
        let target_url = target_url.clone();
        move || {
            for response in [
                "HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\n<h1>Open</h1>",
                "",
            ] {
                let (mut stream, _) = open_listener.accept().unwrap();
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                if response.is_empty() {
                    stream
                        .write_all(
                            format!(
                                "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            )
                            .as_bytes(),
                        )
                        .unwrap();
                } else {
                    stream.write_all(response.as_bytes()).unwrap();
                }
            }
        }
    });
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
        .expect("failed to spawn core with a scoped network rule");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([("network:intercept".to_string(), 4)]),
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
        &ExtensionRequest::RegisterNetworkBlockHost {
            host: "127.0.0.1".to_string(),
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
            url: format!("{open_origin}/safe"),
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

    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("{open_origin}/redirect"),
        },
    )
    .unwrap();
    match blueice_ipc::read_server_message(&mut frontend).unwrap() {
        blueice_ipc::ServerMessage::Error { message } => {
            assert!(message.contains("declarative extension rule"));
            assert!(message.contains(&target_url));
        }
        other => panic!("the in-scope redirect target must be blocked, got {other:?}"),
    }
    open_server.join().unwrap();
    assert_eq!(
        target_listener.accept().unwrap_err().kind(),
        ErrorKind::WouldBlock
    );

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    let _ = std::fs::remove_file(gatekeeper_socket);
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn installed_extension_v5_path_prefix_blocks_redirect_before_target_connection() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;
    use std::io::ErrorKind;

    let core_socket = unique_socket_path("epr");
    let extension_socket = unique_private_extension_socket_path("pr");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-path-prefix-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("path-prefix-network-rule", &["network:intercept"]);
    let gatekeeper_socket = clearing_gatekeeper("eprg");
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    target_listener.set_nonblocking(true).unwrap();
    let blocked_url = format!(
        "http://localhost:{}/private/report?download=1",
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
            stream.write_all(format!(
                "HTTP/1.1 302 Found\r\nLocation: {blocked_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
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
        .expect("failed to spawn core with a v5 path-prefix extension");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([("network:intercept".to_string(), 5)]),
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
        &ExtensionRequest::RegisterNetworkBlockPathPrefix {
            host: "LOCALHOST.".into(),
            path_prefix: "/private".into(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::NetworkInterceptAck
    );

    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate { url: redirect_url },
    )
    .unwrap();
    match blueice_ipc::read_server_message(&mut frontend).unwrap() {
        blueice_ipc::ServerMessage::Error { message } => {
            assert!(message.contains("declarative extension rule"));
            assert!(message.contains(&blocked_url));
        }
        other => panic!("the v5 path-prefix rule must reject the redirect, got {other:?}"),
    }
    redirect_server.join().unwrap();
    assert_eq!(
        target_listener.accept().unwrap_err().kind(),
        ErrorKind::WouldBlock
    );

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_dir_all(package_root);
    let _ = std::fs::remove_file(gatekeeper_socket);
}
