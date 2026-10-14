// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn an_invalid_installed_extension_never_publishes_core_or_extension_sockets() {
    let _guard = core_process_test_guard();
    let core_socket = unique_socket_path("invalid-extension-core");
    let extension_socket = unique_socket_path("invalid-extension-protocol");
    let root = std::env::temp_dir().join(format!(
        "blueice-core-invalid-extension-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest, "{not valid JSON").unwrap();
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);

    let output = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run core with a bad installed extension");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("could not install extension"));
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn installed_extension_storage_v1_v2_v3_keep_their_separate_lifetimes() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    let core_socket = unique_socket_path("ext-storage");
    let extension_socket = unique_private_extension_socket_path("storage");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-storage-frames-{}",
        std::process::id()
    ));
    let data_dir = std::env::temp_dir().join(format!(
        "blueice-core-extension-storage-data-{}",
        std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("storage", &["storage"]);
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let _ = std::fs::remove_dir_all(&data_dir);

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
        .env("XDG_DATA_HOME", &data_dir)
        .spawn()
        .expect("failed to spawn core with a storage extension");

    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let hello = || ExtensionRequest::Hello {
        extension_id: extension_id.clone(),
        capability_versions: BTreeMap::from([("storage".to_string(), 1)]),
    };
    let hello_v2 = || ExtensionRequest::Hello {
        extension_id: extension_id.clone(),
        capability_versions: BTreeMap::from([("storage".to_string(), 2)]),
    };
    let hello_v3 = || ExtensionRequest::Hello {
        extension_id: extension_id.clone(),
        capability_versions: BTreeMap::from([("storage".to_string(), 3)]),
    };

    let mut first_connection = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(&mut first_connection, &hello()).unwrap();
    assert_eq!(
        read_extension_reply(&mut first_connection).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(
        &mut first_connection,
        &ExtensionRequest::StorageSet {
            key: "task-state".to_string(),
            value: "complete".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut first_connection).unwrap(),
        ExtensionReply::StorageSetAck
    );
    drop(first_connection);

    let mut reconnected = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(&mut reconnected, &hello()).unwrap();
    assert_eq!(
        read_extension_reply(&mut reconnected).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(
        &mut reconnected,
        &ExtensionRequest::StorageGet {
            key: "task-state".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut reconnected).unwrap(),
        ExtensionReply::StorageGetResult {
            value: Some("complete".to_string()),
        }
    );
    write_extension_request(
        &mut reconnected,
        &ExtensionRequest::StorageRemove {
            key: "task-state".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut reconnected).unwrap(),
        ExtensionReply::StorageRemoveAck { removed: true }
    );
    drop(reconnected);

    let mut durable = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(&mut durable, &hello_v2()).unwrap();
    assert_eq!(
        read_extension_reply(&mut durable).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(
        &mut durable,
        &ExtensionRequest::DurableStorageSet {
            key: "task-state".to_string(),
            value: "persisted".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut durable).unwrap(),
        ExtensionReply::StorageSetAck
    );
    write_extension_request(
        &mut durable,
        &ExtensionRequest::StorageSet {
            key: "task-state".to_string(),
            value: "temporary".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut durable).unwrap(),
        ExtensionReply::StorageSetAck
    );
    write_extension_request(
        &mut durable,
        &ExtensionRequest::StorageSet {
            key: "ephemeral-only".to_string(),
            value: "not-durable".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut durable).unwrap(),
        ExtensionReply::StorageSetAck
    );
    write_extension_request(&mut durable, &ExtensionRequest::DurableStorageListKeys).unwrap();
    assert!(matches!(read_extension_reply(&mut durable).unwrap(),
        ExtensionReply::CapabilityDenied { capability, .. } if capability == "storage"));
    write_extension_request(&mut durable, &hello_v3()).unwrap();
    assert_eq!(
        read_extension_reply(&mut durable).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new()
        }
    );
    write_extension_request(&mut durable, &ExtensionRequest::DurableStorageListKeys).unwrap();
    assert_eq!(
        read_extension_reply(&mut durable).unwrap(),
        ExtensionReply::StorageKeysResult {
            keys: vec!["task-state".to_string()]
        }
    );
    drop(durable);

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());

    let mut restarted_core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
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
        .env("XDG_DATA_HOME", &data_dir)
        .spawn()
        .expect("failed to restart core with the same storage extension");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut restarted_frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut restarted_frontend).unwrap();
    let mut restarted_extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(&mut restarted_extension, &hello_v3()).unwrap();
    assert_eq!(
        read_extension_reply(&mut restarted_extension).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    write_extension_request(
        &mut restarted_extension,
        &ExtensionRequest::DurableStorageGet {
            key: "task-state".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut restarted_extension).unwrap(),
        ExtensionReply::StorageGetResult {
            value: Some("persisted".to_string())
        }
    );
    write_extension_request(
        &mut restarted_extension,
        &ExtensionRequest::DurableStorageListKeys,
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut restarted_extension).unwrap(),
        ExtensionReply::StorageKeysResult {
            keys: vec!["task-state".to_string()]
        }
    );
    write_extension_request(
        &mut restarted_extension,
        &ExtensionRequest::StorageGet {
            key: "task-state".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut restarted_extension).unwrap(),
        ExtensionReply::StorageGetResult { value: None }
    );
    blueice_ipc::write_client_message(
        &mut restarted_frontend,
        &blueice_ipc::ClientMessage::Shutdown,
    )
    .unwrap();
    assert!(restarted_core.wait().unwrap().success());
    let _ = std::fs::remove_dir_all(package_root);
    let _ = std::fs::remove_dir_all(data_dir);
}
