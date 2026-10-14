// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn optional_and_ephemeral_manifest_entries_cannot_be_self_granted_over_core_ipc() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{
        read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    };
    use std::collections::BTreeMap;

    let core_socket = unique_socket_path("opt-tier");
    let extension_socket = unique_private_extension_socket_path("opt");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-ungranted-frames-{}",
        std::process::id()
    ));
    let data_dir = std::env::temp_dir().join(format!(
        "blueice-core-ungranted-data-{}",
        std::process::id()
    ));
    let (package_root, manifest, _) = extension_manifest_package("ungranted", &[]);
    std::fs::write(
        &manifest,
        r#"{"name":"Ungrantable tiers","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["storage"],"runtime_ephemeral":["dom:read"]}}"#,
    )
    .unwrap();
    let extension_id = blueice_extension_host::load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
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
        .expect("failed to spawn core with an optional-only extension");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut extension = UnixStream::connect(&extension_socket).unwrap();
    write_extension_request(
        &mut extension,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([
                ("storage".to_string(), 3),
                ("dom:read".to_string(), 2),
            ]),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut extension).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::new(),
        }
    );
    for (request, expected_capability) in [
        (ExtensionRequest::DurableStorageListKeys, "storage"),
        (
            ExtensionRequest::DurableStorageSet {
                key: "key".into(),
                value: "never-persist".into(),
            },
            "storage",
        ),
        (
            ExtensionRequest::StorageSet {
                key: "key".into(),
                value: "never-store".into(),
            },
            "storage",
        ),
        (ExtensionRequest::DomReadTab { tab_id: 1 }, "dom:read"),
    ] {
        write_extension_request(&mut extension, &request).unwrap();
        match read_extension_reply(&mut extension).unwrap() {
            ExtensionReply::CapabilityDenied { capability, .. } => {
                assert_eq!(capability, expected_capability)
            }
            other => panic!("{expected_capability} must remain ungranted, got {other:?}"),
        }
    }
    assert!(
        !data_dir.exists(),
        "denied durable writes must not create a data directory"
    );
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn private_parent_pipe_grants_optional_and_consumes_ephemeral_once_in_a_real_core() {
    let _guard = core_process_test_guard();
    use blueice_ipc::permission_control::{
        read_permission_control_reply, write_permission_control_request, PermissionControlReply,
        PermissionControlRequest,
    };

    let core_socket = unique_socket_path("perm-core");
    let extension_socket = unique_private_extension_socket_path("perm");
    let probe_socket = unique_socket_path("perm-probe");
    let frame_dir =
        std::env::temp_dir().join(format!("blueice-permission-frames-{}", std::process::id()));
    let (package_root, manifest, _) = extension_manifest_package("permission-parent", &[]);
    std::fs::write(&manifest, r#"{"name":"Optional storage","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["storage"],"runtime_ephemeral":["dom:read"]}}"#).unwrap();
    let extension_id = blueice_extension_host::load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    let host = core_extension_host_probe_script(
        &package_root,
        "extension_host_probe_child_observes_optional_and_ephemeral_grants",
    );
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_file(&probe_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let probe_listener = UnixListener::bind(&probe_socket).unwrap();
    probe_listener.set_nonblocking(true).unwrap();

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--extension-host",
            host.to_str().unwrap(),
            "--permission-control-stdio",
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .env("BLUEICE_TEST_PROBE_SOCKET", &probe_socket)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn core with a private parent permission pipe");
    assert!(wait_for(&core_socket, Duration::from_secs(5)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut probe = loop {
        match probe_listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                panic!("authenticated host never reached its private probe socket: {error}")
            }
        }
    };
    probe.set_nonblocking(false).unwrap();
    probe
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut control_in = core.stdin.take().unwrap();
    let mut control_out = core.stdout.take().unwrap();
    let probe_get = |probe: &mut UnixStream, command: u8, ticket: Option<&str>| {
        probe.write_all(&[command]).unwrap();
        if matches!(command, b'r' | b's') {
            let ticket = ticket.expect("an ephemeral read probe needs a bearer token");
            assert_eq!(ticket.len(), 64);
            probe.write_all(ticket.as_bytes()).unwrap();
        }
        let mut result = [0_u8; 1];
        probe.read_exact(&mut result).unwrap();
        result[0]
    };
    assert_eq!(
        probe_get(&mut probe, b'g', None),
        b'D',
        "optional declaration must not grant itself"
    );
    let guessed_ticket = "0".repeat(64);
    assert_eq!(
        probe_get(&mut probe, b'r', Some(&guessed_ticket)),
        b'D',
        "an extension cannot guess an unarmed ephemeral token"
    );

    write_permission_control_request(&mut control_in, &PermissionControlRequest::Inspect).unwrap();
    let state = read_permission_control_reply(&mut control_out).unwrap();
    let PermissionControlReply::State {
        extension_id: observed_id,
        optional,
        ..
    } = state
    else {
        panic!("expected the installed permission state, got {state:?}")
    };
    assert_eq!(observed_id, extension_id);
    assert_eq!(
        optional.len(),
        1,
        "runtime-ephemeral declarations are not optional grants"
    );
    assert_eq!(optional[0].capability, "storage");
    assert!(!optional[0].granted);

    let inspect_document = |input: &mut std::process::ChildStdin,
                            output: &mut std::process::ChildStdout| {
        write_permission_control_request(
            input,
            &PermissionControlRequest::InspectDocument { tab_id: 1 },
        )
        .unwrap();
        read_permission_control_reply(output).unwrap()
    };
    assert_eq!(
        inspect_document(&mut control_in, &mut control_out),
        PermissionControlReply::Document {
            tab_id: 1,
            document_epoch: 0,
            url: None,
        }
    );
    for epoch in 1..=2 {
        blueice_ipc::write_client_message(
            &mut frontend,
            &blueice_ipc::ClientMessage::Navigate {
                url: "about:credits".into(),
            },
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut frontend).unwrap(),
            blueice_ipc::ServerMessage::Navigated {
                url: "about:credits".into()
            }
        );
        assert!(matches!(
            blueice_ipc::read_server_message(&mut frontend).unwrap(),
            blueice_ipc::ServerMessage::FrameReady { .. }
        ));
        assert_eq!(
            inspect_document(&mut control_in, &mut control_out),
            PermissionControlReply::Document {
                tab_id: 1,
                document_epoch: epoch,
                url: Some("about:credits".into()),
            },
            "same-URL document replacement must invalidate the previous identity"
        );
    }
    write_permission_control_request(
        &mut control_in,
        &PermissionControlRequest::InspectDocument { tab_id: u64::MAX },
    )
    .unwrap();
    assert!(matches!(
        read_permission_control_reply(&mut control_out).unwrap(),
        PermissionControlReply::Rejected { .. }
    ));
    write_permission_control_request(
        &mut control_in,
        &PermissionControlRequest::ArmEphemeral {
            capability: "dom:read".into(),
            tab_id: 1,
            document_epoch: 1,
        },
    )
    .unwrap();
    assert!(
        matches!(
            read_permission_control_reply(&mut control_out).unwrap(),
            PermissionControlReply::Rejected { .. }
        ),
        "a stale same-URL document identity cannot arm a one-shot lease"
    );
    write_permission_control_request(
        &mut control_in,
        &PermissionControlRequest::ArmEphemeral {
            capability: "dom:read".into(),
            tab_id: 1,
            document_epoch: 2,
        },
    )
    .unwrap();
    let PermissionControlReply::EphemeralArmed {
        capability,
        tab_id,
        document_epoch,
        ticket,
    } = read_permission_control_reply(&mut control_out).unwrap()
    else {
        panic!("the private pipe should arm the live document");
    };
    assert_eq!(
        (capability.as_str(), tab_id, document_epoch),
        ("dom:read", 1, 2)
    );
    assert_eq!(ticket.len(), 64);
    assert_ne!(ticket, guessed_ticket);
    probe.write_all(b"e").unwrap();
    let mut event_marker = [0_u8; 1];
    probe.read_exact(&mut event_marker).unwrap();
    assert_eq!(event_marker, *b"E");
    let mut event_ticket = [0_u8; 64];
    probe.read_exact(&mut event_ticket).unwrap();
    assert_eq!(
        event_ticket.as_slice(),
        ticket.as_bytes(),
        "the authenticated host must receive the exact core-parent-armed ticket"
    );
    assert_eq!(
        probe_get(&mut probe, b'w', None),
        b'D',
        "legacy implicit-tab read must not consume a document-bound lease"
    );
    assert_eq!(
        probe_get(&mut probe, b'v', None),
        b'D',
        "ordinary v2 explicit-tab read must not consume a document-bound lease"
    );
    assert_eq!(
        probe_get(&mut probe, b'r', Some(&guessed_ticket)),
        b'D',
        "a guessed token must not consume the valid lease"
    );
    assert_eq!(
        probe_get(&mut probe, b's', Some(&ticket)),
        b'D',
        "another tab must not consume the lease"
    );
    assert_eq!(
        probe_get(&mut probe, b'r', Some(&ticket)),
        b'A',
        "the authenticated host may read the exact document once"
    );
    assert_eq!(
        probe_get(&mut probe, b'r', Some(&ticket)),
        b'D',
        "a second read must not reuse the consumed ticket"
    );
    write_permission_control_request(
        &mut control_in,
        &PermissionControlRequest::ArmEphemeral {
            capability: "dom:read".into(),
            tab_id: 1,
            document_epoch: 2,
        },
    )
    .unwrap();
    let PermissionControlReply::EphemeralArmed {
        ticket: next_ticket,
        ..
    } = read_permission_control_reply(&mut control_out).unwrap()
    else {
        panic!("a second private arming should replace the spent lease");
    };
    assert_ne!(ticket, next_ticket);
    assert_eq!(
        probe_get(&mut probe, b'r', Some(&ticket)),
        b'D',
        "an old token cannot borrow a newer arming"
    );
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: "about:credits".into(),
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
    assert_eq!(
        probe_get(&mut probe, b'r', Some(&next_ticket)),
        b'D',
        "same-URL navigation must expire an unspent lease"
    );

    write_permission_control_request(
        &mut control_in,
        &PermissionControlRequest::Grant {
            capability: "dom:read".into(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_permission_control_reply(&mut control_out).unwrap(),
        PermissionControlReply::Rejected { .. }
    ));
    assert_eq!(probe_get(&mut probe, b'g', None), b'D');
    write_permission_control_request(
        &mut control_in,
        &PermissionControlRequest::Grant {
            capability: "storage".into(),
        },
    )
    .unwrap();
    assert_eq!(
        read_permission_control_reply(&mut control_out).unwrap(),
        PermissionControlReply::Updated {
            capability: "storage".into(),
            granted: true,
            changed: true,
        }
    );
    assert_eq!(
        probe_get(&mut probe, b'g', None),
        b'A',
        "the same authenticated host connection must gain the optional capability"
    );

    write_permission_control_request(
        &mut control_in,
        &PermissionControlRequest::Revoke {
            capability: "storage".into(),
        },
    )
    .unwrap();
    assert_eq!(
        read_permission_control_reply(&mut control_out).unwrap(),
        PermissionControlReply::Updated {
            capability: "storage".into(),
            granted: false,
            changed: true,
        }
    );
    assert_eq!(
        probe_get(&mut probe, b'g', None),
        b'D',
        "the completed revoke must deny the existing host connection"
    );
    write_permission_control_request(
        &mut control_in,
        &PermissionControlRequest::Grant {
            capability: "storage".into(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_permission_control_reply(&mut control_out).unwrap(),
        PermissionControlReply::Updated { granted: true, .. }
    ));
    assert_eq!(probe_get(&mut probe, b'g', None), b'A');
    drop(control_in);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if probe_get(&mut probe, b'g', None) == b'D' {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "parent pipe EOF must revoke its optional grants"
        );
        thread::sleep(Duration::from_millis(20));
    }

    probe.write_all(b"q").unwrap();
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    let _ = std::fs::remove_file(probe_socket);
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn private_parent_revoke_removes_published_ui_and_network_rules_before_acknowledgement() {
    let _guard = core_process_test_guard();
    use blueice_ipc::permission_control::{
        read_permission_control_reply, write_permission_control_request, PermissionControlReply,
        PermissionControlRequest,
    };

    let core_socket = unique_socket_path("perm-effects-core");
    let extension_socket = unique_private_extension_socket_path("perm-effects");
    let probe_socket = unique_socket_path("perm-effects-probe");
    let gatekeeper_socket = clearing_gatekeeper("perm-effects-gk");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-permission-effects-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, _) = extension_manifest_package("permission-effects", &[]);
    std::fs::write(&manifest, r#"{"name":"Optional effects","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["ui:inject","network:intercept"]}}"#).unwrap();
    let host = core_extension_host_probe_script(
        &package_root,
        "extension_host_probe_child_publishes_optional_effects",
    );
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_file(&probe_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let probe_listener = UnixListener::bind(&probe_socket).unwrap();
    probe_listener.set_nonblocking(true).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/optional", listener.local_addr().unwrap());

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--extension-host",
            host.to_str().unwrap(),
            "--permission-control-stdio",
            "--gatekeeper-socket",
            gatekeeper_socket.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .env("BLUEICE_TEST_PROBE_SOCKET", &probe_socket)
        .env("BLUEICE_TEST_RULE_URL", &url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn core with optional published effects");
    assert!(wait_for(&core_socket, Duration::from_secs(15)));
    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    frontend
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut probe = loop {
        match probe_listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                panic!("authenticated optional-effects host never reached its probe: {error}")
            }
        }
    };
    probe.set_nonblocking(false).unwrap();
    probe
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut ready = [0_u8; 1];
    probe.read_exact(&mut ready).unwrap();
    assert_eq!(
        ready[0], b'S',
        "the authenticated host must enter its probe loop"
    );
    let probe_actions = |probe: &mut UnixStream, command: u8| {
        probe.write_all(&[command]).unwrap();
        let mut answer = [0_u8; 1];
        probe.read_exact(&mut answer).unwrap();
        answer[0]
    };
    assert_eq!(
        probe_actions(&mut probe, b'd'),
        b'D',
        "optional declarations are not grants"
    );
    let mut control_in = core.stdin.take().unwrap();
    let mut control_out = core.stdout.take().unwrap();
    for capability in ["ui:inject", "network:intercept"] {
        write_permission_control_request(
            &mut control_in,
            &PermissionControlRequest::Grant {
                capability: capability.into(),
            },
        )
        .unwrap();
        assert_eq!(
            read_permission_control_reply(&mut control_out).unwrap(),
            PermissionControlReply::Updated {
                capability: capability.into(),
                granted: true,
                changed: true,
            }
        );
    }
    assert_eq!(probe_actions(&mut probe, b'p'), b'P');
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionToolbar {
            label: Some("Optional Notes".into())
        }
    );

    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    assert!(
        matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Error { message } if message.contains("declarative extension rule"))
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "the published rule must block before opening a connection"
    );

    for capability in ["ui:inject", "network:intercept"] {
        write_permission_control_request(
            &mut control_in,
            &PermissionControlRequest::Revoke {
                capability: capability.into(),
            },
        )
        .unwrap();
        assert_eq!(
            read_permission_control_reply(&mut control_out).unwrap(),
            PermissionControlReply::Updated {
                capability: capability.into(),
                granted: false,
                changed: true,
            }
        );
        if capability == "ui:inject" {
            assert_eq!(
                blueice_ipc::read_server_message(&mut frontend).unwrap(),
                blueice_ipc::ServerMessage::ExtensionToolbar { label: None },
                "the native toolbar must be removed before revoke completes"
            );
        }
    }
    assert_eq!(
        probe_actions(&mut probe, b'd'),
        b'D',
        "the same host cannot republish either effect"
    );

    let target_server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => {
                    panic!("navigation did not reconnect after completed revoke: {error}")
                }
            }
        };
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 16\r\nConnection: close\r\n\r\n<p>now open</p>\n").unwrap();
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

    probe.write_all(b"q").unwrap();
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    let _ = std::fs::remove_file(probe_socket);
    let _ = std::fs::remove_file(gatekeeper_socket);
    let _ = std::fs::remove_dir_all(package_root);
}

#[test]
fn core_waits_for_its_spawned_extension_host_and_rejects_a_bearer_claim_peer() {
    let _guard = core_process_test_guard();
    use blueice_ipc::extension::{read_extension_reply, write_extension_request, ExtensionRequest};
    use std::collections::BTreeMap;

    // Darwin's Unix-domain socket path budget is small beneath its long
    // per-user temporary root; the PID in `unique_socket_path` keeps these
    // concise labels independent.
    let core_socket = unique_socket_path("aec");
    let extension_socket = unique_private_extension_socket_path("auth");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-authenticated-extension-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, extension_id) =
        extension_manifest_package("authenticated-host", &["dom:read"]);
    let extension_host = core_extension_host_probe_script(
        &package_root,
        "extension_host_probe_child_authenticates_to_core",
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
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn core with its authenticated extension host");

    // The core socket is its public readiness signal. Its existence proves
    // that the child host completed the token handshake first; merely binding
    // the extension listener is insufficient in this mode.
    if !wait_for(&core_socket, Duration::from_secs(15)) {
        let output = core
            .wait_with_output()
            .expect("failed to collect core startup diagnostics");
        panic!(
            "core never published its frontend socket: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(wait_for(&extension_socket, Duration::from_secs(5)));
    assert_eq!(
        std::fs::metadata(&extension_socket)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "the core-owned extension listener must be private before any peer connects"
    );

    let mut bearer_claim_peer = UnixStream::connect(&extension_socket).unwrap();
    bearer_claim_peer
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_extension_request(
        &mut bearer_claim_peer,
        &ExtensionRequest::Hello {
            extension_id,
            capability_versions: BTreeMap::from([("dom:read".to_string(), 1)]),
        },
    )
    .unwrap();
    assert!(
        read_extension_reply(&mut bearer_claim_peer).is_err(),
        "the hash-derived manifest identity alone must not receive HelloAck in core-spawned mode"
    );

    let mut frontend = UnixStream::connect(&core_socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.wait().unwrap().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
    let _ = std::fs::remove_dir_all(package_root);
}
