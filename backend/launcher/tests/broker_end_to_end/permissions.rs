// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn permission_inspection_reports_no_package_without_creating_grant_authority() {
    let mut launcher = Launcher::spawn();
    assert_eq!(launcher.inspect_extension_permissions(), (0, None));
    let mut client = launcher.connect();
    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
}

#[test]
fn an_installed_extension_survives_cutover_with_a_fresh_authenticated_host() {
    let (package_root, manifest) = installed_extension_package();

    let mut launcher = Launcher::spawn_with_manifest(Some(&manifest));
    let v1_extension_socket = launcher.extension_socket_path(0);
    let v2_extension_socket = launcher.extension_socket_path(1);
    assert!(
        v1_extension_socket.exists(),
        "launcher readiness must include a live, authenticated v1 extension host"
    );
    assert!(
        !v2_extension_socket.exists(),
        "v2's private extension socket must not predate the cutover"
    );
    let (before_generation, before) = launcher.inspect_extension_permissions();
    assert_eq!(before_generation, 0);
    let before = before.expect("v1 must answer through its private parent pipe");
    assert_eq!(before.name, "Launcher cutover test");
    assert_eq!(before.version, "1.0.0");
    assert!(!before.extension_id.is_empty());
    assert_eq!(before.optional.len(), 1);
    assert_eq!(before.optional[0].capability, "storage");
    assert!(
        !before.optional[0].granted,
        "inspection must not grant optional storage"
    );
    assert!(before.optional[0].origins.is_empty());

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

    let mut control = launcher.connect_control();
    write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
    let reply = read_control_reply(&mut control).unwrap();
    assert!(
        matches!(reply, ControlReply::CutoverDone { tabs_migrated: 1 }),
        "{reply:?}"
    );
    assert!(
        !v1_extension_socket.exists(),
        "v1's extension listener must be removed during the cutover"
    );
    assert!(
        v2_extension_socket.exists(),
        "v2 must revalidate the same package and start a fresh authenticated host"
    );
    let (after_generation, after) = launcher.inspect_extension_permissions();
    assert_eq!(
        after_generation, 1,
        "the inspected state must belong to cutover v2"
    );
    let after = after.expect("v2 must own a new responsive private parent pipe");
    assert_eq!(
        after, before,
        "cutover must keep package identity and declared-only grants"
    );

    write_client_message_with_id(&mut client, Some(900), &ClientMessage::ListTabs).unwrap();
    let tabs = loop {
        let (reply_id, message) = read_server_message_with_id(&mut client).unwrap();
        if reply_id != Some(900) {
            continue;
        }
        match message {
            ServerMessage::Tabs(tabs) => break tabs,
            other => panic!("expected Tabs after extension cutover, got {other:?}"),
        }
    };
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].url.as_deref(), Some("about:credits"));

    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
    assert!(!v2_extension_socket.exists());
    std::fs::remove_dir_all(&package_root).unwrap();
}
