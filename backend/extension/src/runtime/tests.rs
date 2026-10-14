// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::load_installed_extension;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

fn installed_extension(label: &str, wasm: &str) -> (std::path::PathBuf, InstalledExtension) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "blueice-extension-runtime-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("extension.json"),
        r#"{"name":"Runtime test","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["dom:read","dom:write","storage"]}}"#,
    )
    .unwrap();
    fs::write(root.join("extension.wasm"), wat::parse_str(wasm).unwrap()).unwrap();
    let installed = load_installed_extension(root.join("extension.json")).unwrap();
    (root, installed)
}

#[test]
fn reactor_reads_bounded_network_response_metadata_without_ambient_network_access() {
    let (root, extension) = installed_extension(
        "network-observe",
        r#"(module
                (import "blueice" "network_response_utf8" (func $observe (param i64 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "blueice_start")
                    i64.const 7
                    i32.const 0
                    i32.const 512
                    call $observe
                    i32.const 0
                    i32.le_s
                    if unreachable end))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_thread = thread::spawn(move || {
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::ReadNetworkResponse { tab_id: 7 }
        );
        blueice_ipc::extension::write_extension_reply(
            &mut core,
            &ExtensionReply::NetworkResponseResult {
                response: Some(blueice_ipc::extension::NetworkResponseInfo {
                    method: "GET".to_string(),
                    final_url: "https://example.test/final".to_string(),
                    status: 200,
                    content_type: Some("text/html".to_string()),
                }),
            },
        )
        .unwrap();
    });
    execute_installed_extension(&extension, guest).unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reactor_reads_bounded_committed_network_trace_without_ambient_network_access() {
    let (root, extension) = installed_extension(
        "network-trace",
        r#"(module
                (import "blueice" "network_trace_utf8" (func $observe (param i64 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "blueice_start")
                    i64.const 7
                    i32.const 0
                    i32.const 1024
                    call $observe
                    i32.const 0
                    i32.le_s
                    if unreachable end))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_thread = thread::spawn(move || {
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::ReadNetworkTrace { tab_id: 7 }
        );
        blueice_ipc::extension::write_extension_reply(
            &mut core,
            &ExtensionReply::NetworkTraceResult {
                trace: Some(blueice_ipc::extension::NetworkTraceInfo {
                    request_url: "https://example.test/start".to_string(),
                    redirects: vec![blueice_ipc::extension::NetworkRedirectInfo {
                        request_url: "https://example.test/start".to_string(),
                        status: 302,
                        target_url: "https://example.test/final".to_string(),
                    }],
                    response: blueice_ipc::extension::NetworkResponseInfo {
                        method: "GET".to_string(),
                        final_url: "https://example.test/final".to_string(),
                        status: 200,
                        content_type: Some("text/html".to_string()),
                    },
                }),
            },
        )
        .unwrap();
    });
    execute_installed_extension(&extension, guest).unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn toolbar_activation_runs_a_fresh_guest_with_an_explicit_tab_and_bounded_ui_import() {
    let (root, extension) = installed_extension(
        "toolbar-activation",
        r#"(module
                (import "blueice" "runtime_event_kind" (func $kind (result i32)))
                (import "blueice" "runtime_event_tab_id" (func $tab (result i64)))
                (import "blueice" "set_toolbar_button_utf8" (func $toolbar (param i32 i32) (result i32)))
                (import "blueice" "clear_toolbar_button" (func $clear (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "Notes")
                (func (export "blueice_start")
                    call $kind
                    i32.const 2
                    i32.ne
                    if unreachable end
                    call $tab
                    i64.const 9
                    i64.ne
                    if unreachable end
                    i32.const 0
                    i32.const 5
                    call $toolbar
                    i32.const 0
                    i32.ne
                    if unreachable end
                    call $clear
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_thread = thread::spawn(move || {
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::SetToolbarButton {
                label: "Notes".to_string(),
            }
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::UiInjectAck)
            .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::ClearToolbarButton
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::UiInjectAck)
            .unwrap();
    });
    execute_installed_extension_for_invocation(
        &extension,
        guest,
        RuntimeInvocation::ToolbarActivated { tab_id: 9 },
    )
    .unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn popup_import_forwards_bounded_text_and_clear_to_core() {
    let (root, extension) = installed_extension(
        "popup-activation",
        r#"(module
                (import "blueice" "show_popup_utf8" (func $show (param i64 i32 i32 i32 i32) (result i32)))
                (import "blueice" "clear_popup" (func $clear (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "Notes")
                (data (i32.const 16) "Saved locally")
                (func (export "blueice_start")
                    i64.const 9
                    i32.const 0
                    i32.const 5
                    i32.const 16
                    i32.const 13
                    call $show
                    i32.const 0
                    i32.ne
                    if unreachable end
                    call $clear
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_thread = thread::spawn(move || {
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::ShowPopup {
                tab_id: 9,
                title: "Notes".to_string(),
                body: "Saved locally".to_string(),
            }
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::UiInjectAck)
            .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::ClearPopup
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::UiInjectAck)
            .unwrap();
    });
    execute_installed_extension_for_invocation(
        &extension,
        guest,
        RuntimeInvocation::ToolbarActivated { tab_id: 9 },
    )
    .unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn popup_action_import_forwards_label_and_receives_core_defined_activation() {
    let (root, extension) = installed_extension(
        "popup-action-activation",
        r#"(module
                (import "blueice" "runtime_event_kind" (func $kind (result i32)))
                (import "blueice" "runtime_event_tab_id" (func $tab (result i64)))
                (import "blueice" "show_popup_action_utf8" (func $show (param i64 i32 i32 i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "Notes")
                (data (i32.const 16) "Saved locally")
                (data (i32.const 48) "Open notes")
                (func (export "blueice_start")
                    call $kind
                    i32.const 3
                    i32.ne
                    if unreachable end
                    call $tab
                    i64.const 9
                    i64.ne
                    if unreachable end
                    i64.const 9
                    i32.const 0
                    i32.const 5
                    i32.const 16
                    i32.const 13
                    i32.const 48
                    i32.const 10
                    call $show
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_thread = thread::spawn(move || {
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::ShowPopupAction {
                tab_id: 9,
                title: "Notes".to_string(),
                body: "Saved locally".to_string(),
                action_label: "Open notes".to_string(),
            }
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::UiInjectAck)
            .unwrap();
    });
    execute_installed_extension_for_invocation(
        &extension,
        guest,
        RuntimeInvocation::PopupActionActivated { tab_id: 9 },
    )
    .unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn ordinary_runtime_events_cannot_borrow_an_ephemeral_dom_ticket() {
    let (root, extension) = installed_extension(
        "ephemeral-denied",
        r#"(module
                (import "blueice" "dom_read_ephemeral_utf8" (func $read (param i64 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "blueice_start")
                    i64.const 9
                    i32.const 0
                    i32.const 65536
                    call $read
                    i32.const -1
                    i32.ne
                    if unreachable end))"#,
    );
    for invocation in [
        RuntimeInvocation::Startup,
        RuntimeInvocation::NavigationCommitted { tab_id: 9 },
        RuntimeInvocation::ToolbarActivated { tab_id: 9 },
        RuntimeInvocation::PopupActionActivated { tab_id: 9 },
    ] {
        let (guest, mut core) = UnixStream::pair().unwrap();
        execute_installed_extension_for_invocation(&extension, guest, invocation).unwrap();
        assert!(
            blueice_ipc::extension::read_extension_request(&mut core).is_err(),
            "a non-trusted runtime event must not send a core read request"
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn trusted_ephemeral_dom_event_keeps_its_ticket_in_host_state() {
    let (root, extension) = installed_extension(
        "ephemeral-read",
        r#"(module
                (import "blueice" "runtime_event_kind" (func $kind (result i32)))
                (import "blueice" "runtime_event_tab_id" (func $tab (result i64)))
                (import "blueice" "dom_read_ephemeral_utf8" (func $read (param i64 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "blueice_start")
                    call $kind
                    i32.const 4
                    i32.ne
                    if unreachable end
                    call $tab
                    i64.const 9
                    i64.ne
                    if unreachable end
                    i64.const 10
                    i32.const 0
                    i32.const 65536
                    call $read
                    i32.const -1
                    i32.ne
                    if unreachable end
                    i64.const 9
                    i32.const 0
                    i32.const 65536
                    call $read
                    i32.const 8
                    i32.ne
                    if unreachable end))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let ticket = "a".repeat(64);
    let expected_ticket = ticket.clone();
    let core_thread = thread::spawn(move || {
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::DomReadTabEphemeral {
                tab_id: 9,
                ticket: expected_ticket
            },
        );
        blueice_ipc::extension::write_extension_reply(
            &mut core,
            &ExtensionReply::DomReadResult {
                value: "snapshot".into(),
            },
        )
        .unwrap();
        assert!(
            blueice_ipc::extension::read_extension_request(&mut core).is_err(),
            "the invocation must not send an extra request"
        );
    });
    execute_installed_extension_for_invocation(
        &extension,
        guest,
        RuntimeInvocation::TrustedEphemeralDomRead {
            tab_id: 9,
            document_epoch: 12,
            ticket,
        },
    )
    .unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reactor_forwards_bounded_reads_form_writes_and_visible_text_to_core() {
    let (root, extension) = installed_extension(
        "requests",
        r#"(module
                (import "blueice" "dom_read_utf8" (func $read (param i64 i32 i32) (result i32)))
                (import "blueice" "set_text_input_value" (func $text (param i64 i64 i32 i32) (result i32)))
                (import "blueice" "set_checkbox_checked" (func $checkbox (param i64 i64 i32) (result i32)))
                (import "blueice" "set_radio_checked" (func $radio (param i64 i64) (result i32)))
                (import "blueice" "select_option" (func $select (param i64 i64) (result i32)))
                (import "blueice" "set_textarea_value" (func $textarea (param i64 i64 i32 i32) (result i32)))
                (import "blueice" "set_range_input_value" (func $range (param i64 i64 i64) (result i32)))
                (import "blueice" "set_visible_leaf_text" (func $leaf (param i64 i64 i32 i32) (result i32)))
                (import "blueice" "set_visible_text_content" (func $content (param i64 i64 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "BlueIce")
                (func (export "blueice_start")
                    i64.const 7
                    i32.const 64
                    i32.const 128
                    call $read
                    drop
                    i64.const 7
                    i64.const 12
                    i32.const 0
                    i32.const 7
                    call $text
                    drop
                    i64.const 7
                    i64.const 13
                    i32.const 1
                    call $checkbox
                    drop
                    i64.const 7
                    i64.const 15
                    call $radio
                    drop
                    i64.const 7
                    i64.const 16
                    call $select
                    drop
                    i64.const 7
                    i64.const 14
                    i32.const 0
                    i32.const 7
                    call $textarea
                    drop
                    i64.const 7
                    i64.const 17
                    i64.const -3
                    call $range
                    drop
                    i64.const 7
                    i64.const 18
                    i32.const 0
                    i32.const 7
                    call $leaf
                    drop
                    i64.const 7
                    i64.const 19
                    i32.const 0
                    i32.const 7
                    call $content
                    drop))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_thread = thread::spawn(move || {
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::DomReadTab { tab_id: 7 }
        );
        blueice_ipc::extension::write_extension_reply(
            &mut core,
            &ExtensionReply::DomReadResult {
                value: r#"{"tab_id":7}"#.to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::SetTextInputValue {
                tab_id: 7,
                node_id: 12,
                value: "BlueIce".to_string(),
            }
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::DomWriteAck)
            .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::SetCheckboxChecked {
                tab_id: 7,
                node_id: 13,
                checked: true,
            }
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::DomWriteAck)
            .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::SetRadioChecked {
                tab_id: 7,
                node_id: 15,
            }
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::DomWriteAck)
            .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::SelectOption {
                tab_id: 7,
                node_id: 16,
            }
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::DomWriteAck)
            .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::SetTextareaValue {
                tab_id: 7,
                node_id: 14,
                value: "BlueIce".to_string(),
            }
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::DomWriteAck)
            .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::SetRangeInputValue {
                tab_id: 7,
                node_id: 17,
                value: -3,
            }
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::DomWriteAck)
            .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::SetVisibleLeafText {
                tab_id: 7,
                node_id: 18,
                value: "BlueIce".to_string(),
            }
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::DomWriteAck)
            .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::SetVisibleTextContent {
                tab_id: 7,
                node_id: 19,
                value: "BlueIce".to_string(),
            }
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::DomWriteAck)
            .unwrap();
    });

    execute_installed_extension(&extension, guest).unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reactor_forwards_a_bounded_declarative_navigation_block_url_to_core() {
    let url = "https://example.test/private";
    let (root, extension) = installed_extension(
        "network-block-rule",
        r#"(module
                (import "blueice" "register_network_block_url" (func $block (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "https://example.test/private")
                (func (export "blueice_start")
                    i32.const 0
                    i32.const 28
                    call $block
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_thread = thread::spawn(move || {
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::RegisterNetworkBlockUrl {
                url: url.to_string(),
            }
        );
        blueice_ipc::extension::write_extension_reply(
            &mut core,
            &ExtensionReply::NetworkInterceptAck,
        )
        .unwrap();
    });

    execute_installed_extension(&extension, guest).unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reactor_forwards_a_bounded_host_block_rule_to_core() {
    let (root, extension) = installed_extension(
        "network-block-host",
        r#"(module
                (import "blueice" "register_network_block_host" (func $block (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "example.test")
                (func (export "blueice_start")
                    i32.const 0
                    i32.const 12
                    call $block
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_thread = thread::spawn(move || {
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::RegisterNetworkBlockHost {
                host: "example.test".to_string(),
            }
        );
        blueice_ipc::extension::write_extension_reply(
            &mut core,
            &ExtensionReply::NetworkInterceptAck,
        )
        .unwrap();
    });

    execute_installed_extension(&extension, guest).unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reactor_forwards_a_bounded_path_prefix_rule_to_core() {
    let (root, extension) = installed_extension(
        "network-block-path-prefix",
        r#"(module
                (import "blueice" "register_network_block_path_prefix" (func $block (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "example.test")
                (data (i32.const 16) "/private")
                (func (export "blueice_start")
                    i32.const 0
                    i32.const 12
                    i32.const 16
                    i32.const 8
                    call $block
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_thread = thread::spawn(move || {
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::RegisterNetworkBlockPathPrefix {
                host: "example.test".into(),
                path_prefix: "/private".into(),
            }
        );
        blueice_ipc::extension::write_extension_reply(
            &mut core,
            &ExtensionReply::NetworkInterceptAck,
        )
        .unwrap();
    });
    execute_installed_extension(&extension, guest).unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reactor_forwards_a_bounded_same_origin_redirect_to_core() {
    let (root, extension) = installed_extension(
        "network-redirect-url",
        r#"(module
                (import "blueice" "register_network_redirect_url" (func $redirect (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "https://example.test/old")
                (data (i32.const 64) "https://example.test/new")
                (func (export "blueice_start")
                    i32.const 0
                    i32.const 24
                    i32.const 64
                    i32.const 24
                    call $redirect
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_thread = thread::spawn(move || {
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::RegisterNetworkRedirectUrl {
                source_url: "https://example.test/old".into(),
                target_url: "https://example.test/new".into(),
            }
        );
        blueice_ipc::extension::write_extension_reply(
            &mut core,
            &ExtensionReply::NetworkInterceptAck,
        )
        .unwrap();
    });
    execute_installed_extension(&extension, guest).unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reactor_forwards_a_network_rule_clear_to_core() {
    let (root, extension) = installed_extension(
        "network-rule-clear",
        r#"(module
                (import "blueice" "clear_network_block_urls" (func $clear (result i32)))
                (func (export "blueice_start")
                    call $clear
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_thread = thread::spawn(move || {
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::ClearNetworkBlockUrls
        );
        blueice_ipc::extension::write_extension_reply(
            &mut core,
            &ExtensionReply::NetworkInterceptAck,
        )
        .unwrap();
    });

    execute_installed_extension(&extension, guest).unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reactor_forwards_bounded_storage_operations_and_copies_a_found_value() {
    let (root, extension) = installed_extension(
        "storage",
        r#"(module
                (import "blueice" "storage_get_utf8" (func $get (param i32 i32 i32 i32) (result i32)))
                (import "blueice" "storage_set_utf8" (func $set (param i32 i32 i32 i32) (result i32)))
                (import "blueice" "storage_remove_utf8" (func $remove (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "task-state")
                (data (i32.const 16) "complete")
                (func (export "blueice_start")
                    i32.const 0
                    i32.const 10
                    i32.const 16
                    i32.const 8
                    call $set
                    i32.const 0
                    i32.ne
                    if unreachable end
                    i32.const 0
                    i32.const 10
                    i32.const 64
                    i32.const 16
                    call $get
                    i32.const 8
                    i32.ne
                    if unreachable end
                    i32.const 64
                    i32.load8_u
                    i32.const 99
                    i32.ne
                    if unreachable end
                    i32.const 0
                    i32.const 10
                    call $remove
                    i32.const 1
                    i32.ne
                    if unreachable end
                    i32.const 0
                    i32.const 10
                    i32.const 64
                    i32.const 16
                    call $get
                    i32.const -4
                    i32.ne
                    if unreachable end))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_thread = thread::spawn(move || {
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::StorageSet {
                key: "task-state".to_string(),
                value: "complete".to_string(),
            }
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::StorageSetAck)
            .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::StorageGet {
                key: "task-state".to_string(),
            }
        );
        blueice_ipc::extension::write_extension_reply(
            &mut core,
            &ExtensionReply::StorageGetResult {
                value: Some("complete".to_string()),
            },
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::StorageRemove {
                key: "task-state".to_string(),
            }
        );
        blueice_ipc::extension::write_extension_reply(
            &mut core,
            &ExtensionReply::StorageRemoveAck { removed: true },
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::StorageGet {
                key: "task-state".to_string(),
            }
        );
        blueice_ipc::extension::write_extension_reply(
            &mut core,
            &ExtensionReply::StorageGetResult { value: None },
        )
        .unwrap();
    });

    execute_installed_extension(&extension, guest).unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reactor_forwards_separate_durable_storage_v2_imports() {
    let (root, extension) = installed_extension(
        "durable-storage",
        r#"(module
                (import "blueice" "durable_storage_set_utf8" (func $set (param i32 i32 i32 i32) (result i32)))
                (import "blueice" "durable_storage_get_utf8" (func $get (param i32 i32 i32 i32) (result i32)))
                (import "blueice" "durable_storage_remove_utf8" (func $remove (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "key")
                (data (i32.const 16) "value")
                (func (export "blueice_start")
                    i32.const 0 i32.const 3 i32.const 16 i32.const 5 call $set
                    i32.const 0 i32.ne if unreachable end
                    i32.const 0 i32.const 3 i32.const 64 i32.const 16 call $get
                    i32.const 5 i32.ne if unreachable end
                    i32.const 64 i32.load8_u i32.const 118 i32.ne if unreachable end
                    i32.const 0 i32.const 3 call $remove
                    i32.const 1 i32.ne if unreachable end))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_thread = thread::spawn(move || {
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::DurableStorageSet {
                key: "key".to_string(),
                value: "value".to_string(),
            }
        );
        blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::StorageSetAck)
            .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::DurableStorageGet {
                key: "key".to_string()
            }
        );
        blueice_ipc::extension::write_extension_reply(
            &mut core,
            &ExtensionReply::StorageGetResult {
                value: Some("value".to_string()),
            },
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
            ExtensionRequest::DurableStorageRemove {
                key: "key".to_string()
            }
        );
        blueice_ipc::extension::write_extension_reply(
            &mut core,
            &ExtensionReply::StorageRemoveAck { removed: true },
        )
        .unwrap();
    });

    execute_installed_extension(&extension, guest).unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reactor_lists_bounded_durable_keys_without_exposing_values() {
    let (root, extension) = installed_extension(
        "durable-storage-keys",
        r#"(module
                (import "blueice" "durable_storage_keys_utf8" (func $keys (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "blueice_start")
                    i32.const 0 i32.const 2 call $keys
                    i32.const -2 i32.ne if unreachable end
                    i32.const 0 i32.const 64 call $keys
                    i32.const 16 i32.ne if unreachable end
                    i32.const 0 i32.load8_u i32.const 91 i32.ne if unreachable end
                    i32.const 15 i32.load8_u i32.const 93 i32.ne if unreachable end
                    i32.const 0 i32.const 64 call $keys
                    i32.const -1 i32.ne if unreachable end))"#,
    );
    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_thread = thread::spawn(move || {
        for keys in [
            vec!["alpha".to_string(), "task".to_string()],
            vec!["alpha".to_string(), "task".to_string()],
            vec!["task".to_string(), "alpha".to_string()],
        ] {
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::DurableStorageListKeys
            );
            blueice_ipc::extension::write_extension_reply(
                &mut core,
                &ExtensionReply::StorageKeysResult { keys },
            )
            .unwrap();
        }
    });
    execute_installed_extension(&extension, guest).unwrap();
    core_thread.join().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reactor_requires_the_fixed_entrypoint_and_terminates_a_fuel_exhausting_module() {
    let (missing_root, missing) = installed_extension("missing-entry", "(module)");
    let (guest, _) = UnixStream::pair().unwrap();
    assert!(execute_installed_extension(&missing, guest).is_err());
    let _ = fs::remove_dir_all(missing_root);

    let (loop_root, looping) = installed_extension(
        "fuel",
        r#"(module (func (export "blueice_start") (loop br 0)))"#,
    );
    let (guest, _) = UnixStream::pair().unwrap();
    assert!(execute_installed_extension(&looping, guest).is_err());
    let _ = fs::remove_dir_all(loop_root);
}

#[test]
fn reactor_exposes_only_the_core_defined_navigation_event_context() {
    let (root, extension) = installed_extension(
        "event-context",
        r#"(module
                (import "blueice" "runtime_event_kind" (func $kind (result i32)))
                (import "blueice" "runtime_event_tab_id" (func $tab (result i64)))
                (func (export "blueice_start")
                    call $kind
                    i32.const 1
                    i32.ne
                    if unreachable end
                    call $tab
                    i64.const 77
                    i64.ne
                    if unreachable end))"#,
    );
    let (guest, _) = UnixStream::pair().unwrap();
    execute_installed_extension_for_invocation(
        &extension,
        guest,
        RuntimeInvocation::NavigationCommitted { tab_id: 77 },
    )
    .unwrap();
    let _ = fs::remove_dir_all(root);
}
