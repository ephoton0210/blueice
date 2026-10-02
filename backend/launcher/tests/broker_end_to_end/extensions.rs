// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn an_invalidated_extension_package_aborts_cutover_without_losing_v1() {
    let (package_root, manifest) = installed_extension_package();
    let mut launcher = Launcher::spawn_with_manifest(Some(&manifest));
    let v1_core_socket = launcher.v1_internal_socket_path();
    let v1_extension_socket = launcher.extension_socket_path(0);
    let v2_extension_socket = launcher.extension_socket_path(1);
    assert!(v1_core_socket.exists() && v1_extension_socket.exists());
    let original_permissions = launcher.inspect_extension_permissions();
    assert_eq!(original_permissions.0, 0);

    let mut client = launcher.connect();
    write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    // v1 has already validated and loaded this package. A later malformed
    // on-disk manifest must prevent v2's startup, not take v1 down with it.
    std::fs::write(&manifest, b"{ invalid manifest").unwrap();
    let mut control = launcher.connect_control();
    write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
    let reply = read_control_reply(&mut control).unwrap();
    assert!(
        matches!(reply, ControlReply::CutoverFailed { .. }),
        "expected v2 package validation to fail, got {reply:?}"
    );
    assert!(v1_core_socket.exists() && v1_extension_socket.exists());
    assert!(!v2_extension_socket.exists());
    assert_eq!(
        launcher.inspect_extension_permissions(),
        original_permissions,
        "a failed cutover must keep v1's identity, grants, and generation"
    );

    // The already-connected client must still reach v1 after this failure.
    write_client_message_with_id(&mut client, Some(901), &ClientMessage::ListTabs).unwrap();
    let tabs = loop {
        let (reply_id, message) = read_server_message_with_id(&mut client).unwrap();
        if matches!(reply_id, Some(id) if id != 901) {
            continue;
        }
        match message {
            ServerMessage::Tabs(tabs) => break tabs,
            other => panic!("expected v1 Tabs after failed cutover, got {other:?}"),
        }
    };
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].url.as_deref(), Some("about:credits"));

    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
    std::fs::remove_dir_all(&package_root).unwrap();
}
