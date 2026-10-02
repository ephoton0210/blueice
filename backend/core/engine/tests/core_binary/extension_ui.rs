// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn core_spawned_extension_toolbar_reaches_client_and_activation_reaches_host() {
    let _guard = core_process_test_guard();
    let core_socket = unique_socket_path("uit");
    let extension_socket = unique_private_extension_socket_path("ui");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-ui-extension-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, _) = extension_manifest_package("native-ui", &["ui:inject"]);
    let gatekeeper_socket = clearing_gatekeeper("ui-gk");
    let extension_host = core_extension_host_probe_script(
        &package_root,
        "extension_host_probe_child_publishes_toolbar_and_handles_activation",
    );
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
            "--extension-host",
            extension_host.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_socket.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with its native UI extension host");
    assert!(wait_for(&core_socket, Duration::from_secs(15)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    frontend
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionToolbar {
            label: Some("Notes".to_string()),
        }
    );
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::ActivateExtensionToolbar,
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionToolbar {
            label: Some("Clicked".to_string()),
        }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionPopup {
            popup: Some(blueice_ipc::ExtensionPopup {
                id: 1,
                tab_id: 1,
                title: "Notes".to_string(),
                body: "Saved locally".to_string(),
                action_label: None,
            }),
        }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionPopup {
            popup: Some(blueice_ipc::ExtensionPopup {
                id: 2,
                tab_id: 1,
                title: "Notes".to_string(),
                body: "Ready to open".to_string(),
                action_label: Some("Open notes".to_string()),
            }),
        }
    );
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::ActivateExtensionPopupAction { popup_id: 1 },
    )
    .unwrap();
    assert!(
        matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Error { message }
            if message.contains("no matching live extension popup action"))
    );
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::ActivateExtensionPopupAction { popup_id: 2 },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionToolbar {
            label: Some("Actioned".to_string()),
        }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionToolbar { label: None }
    );
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_file(gatekeeper_socket);
    let _ = std::fs::remove_dir_all(package_root);
}
