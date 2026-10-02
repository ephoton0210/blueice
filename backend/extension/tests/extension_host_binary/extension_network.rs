// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn core_connection_mode_negotiates_v6_and_runs_host_rule_over_real_ipc() {
    let (root, manifest, extension_id) = network_host_rule_manifest_package("core-connect");
    let socket = unique_socket_path("core-network-host");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let authentication = "test-only-network-host-credential";
    let mut host = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .args([
            "--connect",
            socket.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        .spawn()
        .expect("failed to launch blueice-extension-host for network host rule");

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
        ExtensionRequest::RegisterNetworkBlockHost {
            host: "example.test".to_string()
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
