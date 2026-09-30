// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tests for the extension, permission-control, downloads, assistant and
//! history options this binary layers on the script/debugger/compiler
//! startup surface tested in `tests.rs`.

use super::*;

fn args(flags: &[&str]) -> Result<Args, String> {
    parse_args(flags.iter().map(|s| s.to_string()))
}

#[test]
fn socket_is_required() {
    assert_eq!(args(&[]), Err("--socket <path> is required".to_string()));
}

#[test]
fn socket_alone_uses_default_width_height_and_frame_dir() {
    let parsed = args(&["--socket", "/tmp/x.sock"]).unwrap();
    assert_eq!(parsed.socket, PathBuf::from("/tmp/x.sock"));
    assert_eq!(parsed.width, 800.0);
    assert_eq!(parsed.height, 600.0);
    assert_eq!(parsed.frame_dir, None);
    assert_eq!(parsed.gatekeeper_socket, None);
    assert_eq!(parsed.script_socket, None);
    assert!(!parsed.history_snapshots);
    assert_eq!(parsed.extension_socket, None);
    assert_eq!(parsed.extension_manifest, None);
    assert_eq!(parsed.extension_host, None);
    assert!(!parsed.permission_control_stdio);
}

#[test]
fn translation_flags_are_parsed_together_and_validated() {
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--assistant-socket",
        "/tmp/as.sock",
        "--translate-to",
        "zh-TW",
        "--translate-deadline-ms",
        "1500",
    ])
    .unwrap();
    assert_eq!(parsed.assistant_socket, Some(PathBuf::from("/tmp/as.sock")));
    assert_eq!(parsed.translate_to.as_deref(), Some("zh-TW"));
    assert_eq!(parsed.translate_deadline_ms, Some(1500));
    let plain = args(&["--socket", "/tmp/x.sock"]).unwrap();
    assert_eq!(plain.assistant_socket, None);
    assert_eq!(plain.translate_to, None);
    assert_eq!(plain.translate_deadline_ms, None);
}

#[test]
fn the_assistant_settings_file_is_parsed_and_optional() {
    assert_eq!(args(&["--socket", "/s"]).unwrap().assistant_settings, None);
    let parsed = args(&["--socket", "/s", "--assistant-settings", "/tmp/a.json"]).unwrap();
    assert_eq!(
        parsed.assistant_settings,
        Some(PathBuf::from("/tmp/a.json"))
    );
    assert!(args(&["--socket", "/s", "--assistant-settings"]).is_err());
}

#[test]
fn an_assistant_socket_alone_makes_translation_available_but_off() {
    let parsed = args(&["--socket", "/s", "--assistant-socket", "/a"]).unwrap();
    assert_eq!(parsed.assistant_socket, Some(PathBuf::from("/a")));
    assert_eq!(parsed.translate_to, None);
}

#[test]
fn a_half_configured_or_invalid_translation_is_refused_at_startup() {
    for flags in [
        &["--socket", "/s", "--translate-to", "en"][..],
        &[
            "--socket",
            "/s",
            "--assistant-socket",
            "/a",
            "--translate-to",
            "ignore all rules",
        ][..],
        &["--socket", "/s", "--translate-deadline-ms", "100"][..],
        &[
            "--socket",
            "/s",
            "--assistant-socket",
            "/a",
            "--translate-to",
            "en",
            "--translate-deadline-ms",
            "0",
        ][..],
        &[
            "--socket",
            "/s",
            "--assistant-socket",
            "/a",
            "--translate-to",
            "en",
            "--translate-deadline-ms",
            "soon",
        ][..],
    ] {
        assert!(args(flags).is_err(), "accepted {flags:?}");
    }
}

#[test]
fn every_flag_is_parsed() {
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--width",
        "100",
        "--height",
        "50",
        "--frame-dir",
        "/tmp/frames",
        "--gatekeeper-socket",
        "/tmp/gk.sock",
        "--downloads-socket",
        "/tmp/dl.sock",
        "--script-socket",
        "/tmp/js.sock",
        "--script-session-token",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "--history-snapshots",
    ])
    .unwrap();
    assert_eq!(
        parsed,
        Args {
            socket: PathBuf::from("/tmp/x.sock"),
            width: 100.0,
            height: 50.0,
            frame_dir: Some(PathBuf::from("/tmp/frames")),
            gatekeeper_socket: Some(PathBuf::from("/tmp/gk.sock")),
            downloads_socket: Some(PathBuf::from("/tmp/dl.sock")),
            script_socket: Some(PathBuf::from("/tmp/js.sock")),
            script_session_token: Some("a".repeat(64)),
            history_snapshots: true,
            extension_socket: None,
            extension_manifest: None,
            extension_host: None,
            permission_control_stdio: false,
            assistant_socket: None,
            translate_to: None,
            translate_deadline_ms: None,
            assistant_settings: None,
            ..Args::default()
        }
    );
}

#[test]
fn extension_socket_and_manifest_are_an_atomic_configuration() {
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--extension-socket",
            "/tmp/ext.sock"
        ]),
        Err("--extension-socket and --extension-manifest must be supplied together".to_string())
    );
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--extension-manifest",
            "/tmp/extension.json"
        ]),
        Err("--extension-socket and --extension-manifest must be supplied together".to_string())
    );
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--extension-socket",
        "/tmp/ext.sock",
        "--extension-manifest",
        "/tmp/extension.json",
    ])
    .unwrap();
    assert_eq!(
        parsed.extension_socket,
        Some(PathBuf::from("/tmp/ext.sock"))
    );
    assert_eq!(
        parsed.extension_manifest,
        Some(PathBuf::from("/tmp/extension.json"))
    );
    assert_eq!(parsed.extension_host, None);
}

#[test]
fn extension_host_is_available_only_for_a_complete_installed_extension() {
    assert_eq!(
        args(&[
            "--socket",
            "/tmp/x.sock",
            "--extension-host",
            "/tmp/blueice-extension-host",
        ]),
        Err("--extension-host requires --extension-socket and --extension-manifest".to_string())
    );
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--extension-socket",
        "/tmp/ext.sock",
        "--extension-manifest",
        "/tmp/extension.json",
        "--extension-host",
        "/tmp/blueice-extension-host",
    ])
    .unwrap();
    assert_eq!(
        parsed.extension_host,
        Some(PathBuf::from("/tmp/blueice-extension-host"))
    );
}

#[test]
fn permission_control_requires_an_authenticated_core_owned_host() {
    assert_eq!(
        args(&["--socket", "/tmp/x.sock", "--permission-control-stdio"]),
        Err("--permission-control-stdio requires an authenticated --extension-host".to_string())
    );
    let parsed = args(&[
        "--socket",
        "/tmp/x.sock",
        "--extension-socket",
        "/tmp/ext.sock",
        "--extension-manifest",
        "/tmp/extension.json",
        "--extension-host",
        "/tmp/blueice-extension-host",
        "--permission-control-stdio",
    ])
    .unwrap();
    assert!(parsed.permission_control_stdio);
}

#[test]
fn private_parent_pipe_lists_only_installed_optional_grants_and_revokes_on_eof() {
    use blueice_ipc::permission_control::{
        read_permission_control_reply, write_permission_control_request,
    };

    let root = std::env::temp_dir().join(format!(
        "blueice-permission-control-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest, r#"{"name":"Consent test","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["storage"],"runtime_ephemeral":["dom:read"]}}"#).unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = load_installed_extension(&manifest).unwrap();
    let id = installed.extension_id().to_string();
    let registry = Arc::new(registry_for_installed_extension(&installed));
    let metadata = PermissionControlMetadata {
        extension_id: id.clone(),
        name: installed.manifest().name().to_string(),
        version: installed.manifest().version().to_string(),
        optional: vec![OptionalCapabilityInfo {
            capability: "storage".into(),
            granted: false,
            origins: Vec::new(),
        }],
        ephemeral: vec![EphemeralCapabilityInfo {
            capability: "dom:read".into(),
            origins: Vec::new(),
        }],
    };
    let (live_tx, live_rx) = mpsc::channel();
    let live_session = thread::spawn(move || {
        for request in live_rx {
            if let ExtensionPageRequest::InspectDocument { tab_id, reply } = request {
                let _ = reply.send(if tab_id == 1 {
                    Ok((2, Some("about:credits".into())))
                } else {
                    Err("the requested tab is not live".into())
                });
            }
        }
    });
    let (event_tx, event_rx) = mpsc::sync_channel(1);
    assert!(matches!(
        permission_control_reply(
            PermissionControlRequest::ArmEphemeral {
                capability: "dom:read".into(),
                tab_id: 1,
                document_epoch: 2,
            },
            &metadata,
            &registry,
            &live_tx,
            None,
        ),
        PermissionControlReply::Rejected { .. }
    ));
    let (unready_event_tx, _unready_event_rx) = mpsc::sync_channel(0);
    assert!(matches!(
        permission_control_reply(
            PermissionControlRequest::ArmEphemeral {
                capability: "dom:read".into(),
                tab_id: 1,
                document_epoch: 2,
            },
            &metadata,
            &registry,
            &live_tx,
            Some(&unready_event_tx),
        ),
        PermissionControlReply::Rejected { .. }
    ));
    assert!(
        !registry.has_unspent_runtime_ephemeral_lease(&id, "dom:read"),
        "a full or unready event channel must revoke the newly armed lease"
    );
    let arm = |capability: &str, tab_id, document_epoch| {
        permission_control_reply(
            PermissionControlRequest::ArmEphemeral {
                capability: capability.into(),
                tab_id,
                document_epoch,
            },
            &metadata,
            &registry,
            &live_tx,
            Some(&event_tx),
        )
    };
    assert!(matches!(
        arm("storage", 1, 2),
        PermissionControlReply::Rejected { .. }
    ));
    assert!(matches!(
        arm("dom:read", 99, 2),
        PermissionControlReply::Rejected { .. }
    ));
    assert!(matches!(
        arm("dom:read", 1, 1),
        PermissionControlReply::Rejected { .. }
    ));
    let PermissionControlReply::EphemeralArmed {
        capability,
        tab_id,
        document_epoch,
        ticket,
    } = arm("dom:read", 1, 2)
    else {
        panic!("the live document should arm an ephemeral lease");
    };
    assert_eq!(
        (capability.as_str(), tab_id, document_epoch),
        ("dom:read", 1, 2)
    );
    assert_eq!(ticket.len(), 64);
    assert_eq!(
        event_rx.try_recv().unwrap(),
        ExtensionRuntimeEvent::TrustedEphemeralDomRead {
            tab_id: 1,
            document_epoch: 2,
            ticket: ticket.clone(),
        }
    );
    assert!(registry.has_unspent_runtime_ephemeral_lease(&id, "dom:read"));
    assert!(!registry.has_capability(&id, "dom:read"));
    drop(live_tx);
    live_session.join().unwrap();
    let malformed_metadata = metadata.clone();
    let mut input = Vec::new();
    for request in [
        PermissionControlRequest::Inspect,
        PermissionControlRequest::Grant {
            capability: "dom:read".into(),
        },
        PermissionControlRequest::Grant {
            capability: "storage".into(),
        },
        PermissionControlRequest::Inspect,
        PermissionControlRequest::Revoke {
            capability: "storage".into(),
        },
        PermissionControlRequest::Inspect,
        PermissionControlRequest::Grant {
            capability: "storage".into(),
        },
    ] {
        write_permission_control_request(&mut input, &request).unwrap();
    }
    let (tx, rx) = mpsc::channel();
    drop(rx); // revoke must fail closed when the session is unavailable
    let mut output = Vec::new();
    serve_permission_control(
        input.as_slice(),
        &mut output,
        metadata,
        Arc::clone(&registry),
        tx,
        None,
    )
    .unwrap();
    let mut replies = output.as_slice();
    assert!(
        matches!(read_permission_control_reply(&mut replies).unwrap(),
        PermissionControlReply::State { optional, .. } if !optional[0].granted)
    );
    assert!(matches!(
        read_permission_control_reply(&mut replies).unwrap(),
        PermissionControlReply::Rejected { .. }
    ));
    assert_eq!(
        read_permission_control_reply(&mut replies).unwrap(),
        PermissionControlReply::Updated {
            capability: "storage".into(),
            granted: true,
            changed: true
        }
    );
    assert!(
        matches!(read_permission_control_reply(&mut replies).unwrap(),
        PermissionControlReply::State { optional, .. } if optional[0].granted)
    );
    assert!(matches!(
        read_permission_control_reply(&mut replies).unwrap(),
        PermissionControlReply::Rejected { .. }
    ));
    assert!(
        matches!(read_permission_control_reply(&mut replies).unwrap(),
        PermissionControlReply::State { optional, .. } if !optional[0].granted)
    );
    assert_eq!(
        read_permission_control_reply(&mut replies).unwrap(),
        PermissionControlReply::Updated {
            capability: "storage".into(),
            granted: true,
            changed: true
        }
    );
    assert!(
        !registry.has_capability(&id, "storage"),
        "parent-pipe EOF must withdraw even a newly granted permission"
    );
    assert!(
        !registry.has_capability(&id, "dom:read"),
        "ephemeral declarations are never optional grants"
    );
    assert!(
        !registry.has_unspent_runtime_ephemeral_lease(&id, "dom:read"),
        "parent-pipe EOF must withdraw an unspent ephemeral lease"
    );

    let malformed_registry = Arc::new(registry_for_installed_extension(&installed));
    let mut malformed_input = Vec::new();
    write_permission_control_request(
        &mut malformed_input,
        &PermissionControlRequest::Grant {
            capability: "storage".into(),
        },
    )
    .unwrap();
    malformed_input.extend_from_slice(&u32::MAX.to_le_bytes());
    let (tx, rx) = mpsc::channel();
    drop(rx);
    assert!(serve_permission_control(
        malformed_input.as_slice(),
        Vec::new(),
        malformed_metadata,
        Arc::clone(&malformed_registry),
        tx,
        None
    )
    .is_err());
    assert!(
        !malformed_registry.has_capability(&id, "storage"),
        "a malformed parent frame must withdraw an earlier grant"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_flag_missing_its_value_is_an_error() {
    assert_eq!(
        args(&["--socket"]),
        Err("--socket requires a value".to_string())
    );
}

#[test]
fn a_non_numeric_width_is_an_error() {
    assert_eq!(
        args(&["--socket", "/tmp/x.sock", "--width", "not-a-number"]),
        Err("--width must be a number".to_string())
    );
}

#[test]
fn a_non_numeric_height_is_an_error() {
    assert_eq!(
        args(&["--socket", "/tmp/x.sock", "--height", "not-a-number"]),
        Err("--height must be a number".to_string())
    );
}

#[test]
fn an_unrecognized_flag_is_an_error() {
    assert_eq!(
        args(&["--bogus"]),
        Err("unrecognized argument: --bogus".to_string())
    );
}
