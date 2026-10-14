// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn a_granted_capability_succeeds_and_an_ungranted_one_is_denied_over_a_real_process_boundary() {
    // The single most important test in this task: a real client,
    // across a real process boundary, sends `Hello` with the hardcoded
    // extension's id, gets `HelloAck`, gets a real `DomRead` result
    // (server-side allow), and gets `CapabilityDenied` for `DomWrite`
    // (server-side deny) -- concrete, end-to-end proof that
    // authorization is enforced by the server, not merely assumed.
    let host = ExtensionHost::spawn("authz");
    let mut stream = host.connect();

    write_extension_request(&mut stream, &hello("minimal-slice-extension")).unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        empty_hello_ack()
    );

    write_extension_request(&mut stream, &ExtensionRequest::DomRead).unwrap();
    match read_extension_reply(&mut stream).unwrap() {
        ExtensionReply::DomReadResult { value } => assert!(
            !value.is_empty(),
            "a granted DomRead must return a real, non-empty result, not an empty placeholder"
        ),
        other => panic!("expected DomReadResult for a granted dom:read, got {other:?}"),
    }

    write_extension_request(
        &mut stream,
        &ExtensionRequest::DomWrite {
            value: "attacker-controlled content".to_string(),
            target: blueice_ipc::extension::DomWriteTarget::Document,
        },
    )
    .unwrap();
    match read_extension_reply(&mut stream).unwrap() {
        ExtensionReply::CapabilityDenied { capability, reason } => {
            assert_eq!(capability, "dom:write");
            assert!(!reason.is_empty());
        }
        other => panic!("expected CapabilityDenied for an ungranted dom:write, got {other:?}"),
    }
}

#[test]
fn core_connection_mode_authenticates_then_runs_a_navigation_event_reactor_over_real_ipc() {
    let (root, manifest, _) = manifest_package("core-connect");
    std::fs::write(
        &manifest,
        r#"{"name":"Binary test","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["dom:read"],"optional":["storage"],"runtime_ephemeral":["dom:write"]}}"#,
    )
    .unwrap();
    let extension_id = load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    let socket = unique_socket_path("core-connect");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let authentication = "test-only-core-credential";
    let mut host = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .args([
            "--connect",
            socket.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        .spawn()
        .expect("failed to launch blueice-extension-host in core connection mode");

    let (mut stream, _) = listener.accept().unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::HelloAuthenticated {
            extension_id,
            capability_versions: BTreeMap::from([
                ("dom:read".to_string(), 3),
                ("dom:write".to_string(), 9),
                ("storage".to_string(), 3),
            ]),
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
        ExtensionRequest::NextRuntimeEvent
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::RuntimeEvent(
            blueice_ipc::extension::ExtensionRuntimeEvent::NavigationCommitted { tab_id: 1 },
        ),
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::DomReadTab { tab_id: 1 }
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::DomReadResult {
            value: r#"{"tab_id":1,"nodes":[]}"#.to_string(),
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
fn core_connection_mode_keeps_an_ephemeral_ticket_out_of_guest_memory() {
    let (root, manifest, _) = manifest_package("core-ephemeral");
    std::fs::write(
        &manifest,
        r#"{"name":"Binary test","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"runtime_ephemeral":["dom:read"]}}"#,
    ).unwrap();
    std::fs::write(
        root.join("extension.wasm"),
        wat::parse_str(
            r#"(module
        (import "blueice" "runtime_event_kind" (func $kind (result i32)))
        (import "blueice" "runtime_event_tab_id" (func $tab (result i64)))
        (import "blueice" "dom_read_ephemeral_utf8" (func $read (param i64 i32 i32) (result i32)))
        (memory (export "memory") 1)
        (func (export "blueice_start")
            call $kind
            i32.const 4
            i32.eq
            if
                call $tab
                i64.const 7
                i64.ne
                if unreachable end
                i64.const 7
                i32.const 0
                i32.const 65536
                call $read
                i32.const 8
                i32.ne
                if unreachable end
            else
                i64.const 7
                i32.const 0
                i32.const 65536
                call $read
                i32.const -1
                i32.ne
                if unreachable end
            end))"#,
        )
        .unwrap(),
    )
    .unwrap();
    let extension_id = load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    let socket = unique_socket_path("core-ephemeral");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let authentication = "test-only-ephemeral-credential";
    let mut host = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .args([
            "--connect",
            socket.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        .spawn()
        .expect("failed to launch the authenticated extension host");
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::HelloAuthenticated {
            extension_id,
            capability_versions: BTreeMap::from([("dom:read".to_string(), 3)]),
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
        ExtensionRequest::NextRuntimeEvent,
        "startup must not borrow the ephemeral read"
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
        ExtensionRequest::NextRuntimeEvent,
        "toolbar activation must not borrow the ephemeral read"
    );
    let ticket = "b".repeat(64);
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::RuntimeEvent(
            blueice_ipc::extension::ExtensionRuntimeEvent::TrustedEphemeralDomRead {
                tab_id: 7,
                document_epoch: 3,
                ticket: ticket.clone(),
            },
        ),
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::DomReadTabEphemeral { tab_id: 7, ticket }
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::DomReadResult {
            value: "snapshot".into(),
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
