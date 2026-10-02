// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn an_extension_id_that_was_never_registered_gets_capability_denied_even_for_dom_read() {
    let host = ExtensionHost::spawn("unknown-id");
    let mut stream = host.connect();

    write_extension_request(&mut stream, &hello("an-extension-nobody-installed")).unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        empty_hello_ack()
    );

    write_extension_request(&mut stream, &ExtensionRequest::DomRead).unwrap();
    match read_extension_reply(&mut stream).unwrap() {
        ExtensionReply::CapabilityDenied { capability, .. } => assert_eq!(capability, "dom:read"),
        other => {
            panic!("expected CapabilityDenied for an unregistered extension_id, got {other:?}")
        }
    }
}

#[test]
fn core_connection_mode_negotiates_dom_write_v9_and_runs_visible_text_writes() {
    let (root, manifest, extension_id) = visible_text_manifest_package("core-connect");
    let socket = unique_socket_path("core-range");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let authentication = "test-only-range-credential";
    let mut host = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .args([
            "--connect",
            socket.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        .spawn()
        .expect("failed to launch blueice-extension-host for a range write");

    let (mut stream, _) = listener.accept().unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::HelloAuthenticated {
            extension_id,
            capability_versions: BTreeMap::from([("dom:write".to_string(), 9)]),
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
        ExtensionRequest::SetRangeInputValue {
            tab_id: 7,
            node_id: 17,
            value: -3,
        }
    );
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::DomWriteAck)
        .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::SetVisibleLeafText {
            tab_id: 7,
            node_id: 19,
            value: "Updated heading".to_string(),
        }
    );
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::DomWriteAck)
        .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::SetVisibleTextContent {
            tab_id: 7,
            node_id: 21,
            value: "Updated formatted text".to_string(),
        }
    );
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::DomWriteAck)
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
