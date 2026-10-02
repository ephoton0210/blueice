// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn missing_socket_flag_exits_with_failure_and_no_socket_is_created() {
    let output = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .output()
        .expect("failed to run blueice-extension-host");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--socket"));
}

#[test]
fn core_connection_mode_runs_ui_v3_popup_action_over_a_real_host_process() {
    let (root, manifest, extension_id) = popup_action_manifest_package("core-connect");
    let socket = unique_socket_path("core-popup-action");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let authentication = "test-only-popup-action-credential";
    let mut host = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .args([
            "--connect",
            socket.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        .spawn()
        .expect("failed to launch blueice-extension-host for popup action");
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::HelloAuthenticated {
            extension_id,
            capability_versions: BTreeMap::from([("ui:inject".to_string(), 3)]),
            authentication: authentication.to_string(),
        }
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::RuntimeReady
    );
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::RuntimeStart)
        .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::SetToolbarButton {
            label: "Notes".to_string()
        }
    );
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::UiInjectAck)
        .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::NextRuntimeEvent
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::RuntimeEvent(
            blueice_ipc::extension::ExtensionRuntimeEvent::ToolbarActivated {
                tab_id: 7,
                grant_generation: 0,
            },
        ),
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::ShowPopupAction {
            tab_id: 7,
            title: "Notes".to_string(),
            body: "Ready to open".to_string(),
            action_label: "Open".to_string(),
        }
    );
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::UiInjectAck)
        .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::NextRuntimeEvent
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::RuntimeEvent(
            blueice_ipc::extension::ExtensionRuntimeEvent::PopupActionActivated {
                tab_id: 7,
                grant_generation: 0,
            },
        ),
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::SetToolbarButton {
            label: "Done".to_string()
        }
    );
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::UiInjectAck)
        .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::NextRuntimeEvent
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::RuntimeEventStreamClosed,
    )
    .unwrap();
    drop(listener);
    assert!(host.wait().unwrap().success());
    let _ = std::fs::remove_file(socket);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn core_connection_mode_negotiates_v6_and_runs_v3_network_rule_clear_over_real_ipc() {
    let (root, manifest, extension_id) = network_rule_clear_manifest_package("core-connect");
    let socket = unique_socket_path("core-network-clear");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let authentication = "test-only-network-clear-credential";
    let mut host = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .args([
            "--connect",
            socket.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        .spawn()
        .expect("failed to launch blueice-extension-host for network rule clearing");

    let (mut stream, _) = listener.accept().unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::HelloAuthenticated {
            extension_id,
            capability_versions: BTreeMap::from([("network:intercept".to_string(), 6)]),
            authentication: authentication.to_string(),
        }
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::RuntimeReady
    );
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::RuntimeStart)
        .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::ClearNetworkBlockUrls
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::NetworkInterceptAck,
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::NextRuntimeEvent
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::RuntimeEventStreamClosed,
    )
    .unwrap();
    drop(listener);
    assert!(host.wait().unwrap().success());
    let _ = std::fs::remove_file(socket);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn core_connection_mode_runs_v5_path_and_v6_redirect_imports_over_real_ipc() {
    let (root, manifest, extension_id) =
        network_path_prefix_and_redirect_manifest_package("core-connect");
    let socket = unique_socket_path("core-path");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let authentication = "test-only-network-path-credential";
    let mut host = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .args([
            "--connect",
            socket.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        .spawn()
        .expect("failed to launch extension host for the v5 path rule");

    let (mut stream, _) = listener.accept().unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::HelloAuthenticated {
            extension_id,
            capability_versions: BTreeMap::from([("network:intercept".to_string(), 6)]),
            authentication: authentication.to_string(),
        }
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::RuntimeReady
    );
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::RuntimeStart)
        .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::RegisterNetworkBlockPathPrefix {
            host: "example.test".into(),
            path_prefix: "/private".into(),
        }
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::NetworkInterceptAck,
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::RegisterNetworkRedirectUrl {
            source_url: "https://example.test/old".into(),
            target_url: "https://example.test/new".into(),
        }
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::NetworkInterceptAck,
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::NextRuntimeEvent
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::RuntimeEventStreamClosed,
    )
    .unwrap();
    drop(listener);
    assert!(host.wait().unwrap().success());
    let _ = std::fs::remove_file(socket);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_non_hello_first_message_gets_no_reply_and_the_connection_ends() {
    let host = ExtensionHost::spawn("bad-first-msg");
    let mut stream = host.connect();

    write_extension_request(&mut stream, &ExtensionRequest::DomRead).unwrap();

    // The real subprocess must never answer a request sent before a
    // successful handshake -- reading now must fail (the connection was
    // dropped server-side), not return a stray reply or hang.
    assert!(read_extension_reply(&mut stream).is_err());
}

#[test]
fn the_real_subprocess_serves_two_independent_connections_in_sequence() {
    let host = ExtensionHost::spawn("two-conns");

    for _ in 0..2 {
        let mut stream = host.connect();
        write_extension_request(&mut stream, &hello("minimal-slice-extension")).unwrap();
        assert_eq!(
            read_extension_reply(&mut stream).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(&mut stream, &ExtensionRequest::DomRead).unwrap();
        assert!(matches!(
            read_extension_reply(&mut stream).unwrap(),
            ExtensionReply::DomReadResult { .. }
        ));
    }
}
