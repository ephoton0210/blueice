// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn core_connection_mode_negotiates_network_observe_v2_and_runs_trace_import() {
    let (root, manifest, extension_id) = network_trace_manifest_package("core-connect");
    let socket = unique_socket_path("core-trace");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let authentication = "test-only-network-trace-credential";
    let mut host = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .args([
            "--connect",
            socket.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        .spawn()
        .expect("failed to launch blueice-extension-host for network trace");

    let (mut stream, _) = listener.accept().unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::HelloAuthenticated {
            extension_id,
            capability_versions: BTreeMap::from([("network:observe".to_string(), 2)]),
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
        ExtensionRequest::ReadNetworkTrace { tab_id: 1 }
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::NetworkTraceResult {
            trace: Some(blueice_ipc::extension::NetworkTraceInfo {
                request_url: "https://example.test/start".to_string(),
                redirects: vec![],
                response: blueice_ipc::extension::NetworkResponseInfo {
                    method: "GET".to_string(),
                    final_url: "https://example.test/start".to_string(),
                    status: 200,
                    content_type: Some("text/html".to_string()),
                },
            }),
        },
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
fn core_connection_mode_negotiates_storage_v3_and_keeps_v1_v2_imports_compatible() {
    let (root, manifest, extension_id) = storage_manifest_package("core-connect");
    let socket = unique_socket_path("core-storage");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let authentication = "test-only-storage-credential";
    let mut host = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .args([
            "--connect",
            socket.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        .spawn()
        .expect("failed to launch blueice-extension-host for storage");

    let (mut stream, _) = listener.accept().unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::HelloAuthenticated {
            extension_id,
            capability_versions: BTreeMap::from([("storage".to_string(), 3)]),
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
        ExtensionRequest::StorageSet {
            key: "task-state".to_string(),
            value: "complete".to_string(),
        }
    );
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::StorageSetAck)
        .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::DurableStorageSet {
            key: "task-state".to_string(),
            value: "saved".to_string(),
        }
    );
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::StorageSetAck)
        .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::DurableStorageListKeys
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::StorageKeysResult {
            keys: vec!["task-state".to_string()],
        },
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
