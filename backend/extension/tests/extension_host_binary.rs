// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Exercises the actual compiled `blueice-extension-host` binary as a
//! real subprocess -- `main`'s own argument parsing, socket binding,
//! and accept-loop wiring, none of which `src/main.rs`'s own unit tests
//! touch (those cover `parse_args` in isolation; `handle_extension_
//! connection`'s own tests in `src/lib.rs` cover the protocol logic
//! over an in-process `UnixStream` pair). This is the concrete,
//! end-to-end proof `phase-9-extension-protocol/PLAN.md`'s "Minimal
//! first slice" asks for: a real client, over a real Unix socket,
//! across a real process boundary, gets a capability it's granted and
//! is denied one it isn't -- mirroring `blueice-engine`'s own
//! `tests/core_binary.rs` real-subprocess pattern one layer over.

use blueice_extension_host::load_installed_extension;
use blueice_ipc::extension::{
    read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

fn unique_socket_path(label: &str) -> PathBuf {
    blueice_ipc::local_socket::default_socket_dir()
        .join(format!("ext-host-{label}-{}.sock", std::process::id()))
}

/// A pathname can exist just before a child process is able to accept it, so
/// readiness means a real connection succeeds rather than merely observing a
/// socket filesystem entry.
fn wait_for(path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match UnixStream::connect(path) {
            Ok(stream) => {
                drop(stream);
                return true;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) => {}
            Err(_) => return false,
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

/// A spawned real `blueice-extension-host` subprocess, killed on drop --
/// there's no `Shutdown`-style message in this protocol (unlike
/// `blueice-core`'s), so every test here ends the subprocess by simply
/// dropping this.
struct ExtensionHost {
    child: Child,
    socket: PathBuf,
}

impl ExtensionHost {
    fn spawn(label: &str) -> Self {
        Self::spawn_with_manifest(label, None)
    }

    fn spawn_with_manifest(label: &str, manifest: Option<&Path>) -> Self {
        let socket = unique_socket_path(label);
        let _ = std::fs::remove_file(&socket);

        let mut command = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"));
        command.args(["--socket", socket.to_str().unwrap()]);
        if let Some(manifest) = manifest {
            command.args(["--manifest", manifest.to_str().unwrap()]);
        }
        let child = command
            .spawn()
            .expect("failed to spawn blueice-extension-host");

        assert!(
            wait_for(&socket, Duration::from_secs(5)),
            "blueice-extension-host never created its socket"
        );
        assert_eq!(
            std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
            0o600,
            "the standalone extension listener must not be connectable by another local user"
        );
        ExtensionHost { child, socket }
    }

    fn connect(&self) -> UnixStream {
        UnixStream::connect(&self.socket).expect("failed to connect to the real subprocess")
    }
}

impl Drop for ExtensionHost {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn hello(extension_id: &str) -> ExtensionRequest {
    ExtensionRequest::Hello {
        extension_id: extension_id.to_string(),
        capability_versions: BTreeMap::from([
            ("dom:read".to_string(), 1),
            ("dom:write".to_string(), 1),
        ]),
    }
}

fn empty_hello_ack() -> ExtensionReply {
    ExtensionReply::HelloAck {
        unsupported_capabilities: BTreeMap::new(),
    }
}

fn manifest_package(label: &str) -> (PathBuf, PathBuf, String) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "blueice-extension-host-package-{label}-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(
        &manifest,
        r#"{"name":"Binary test","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["dom:read"]}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("extension.wasm"),
        wat::parse_str(
            r#"(module
                (import "blueice" "dom_read_utf8" (func $read (param i64 i32 i32) (result i32)))
                (import "blueice" "runtime_event_kind" (func $event_kind (result i32)))
                (import "blueice" "runtime_event_tab_id" (func $event_tab_id (result i64)))
                (memory (export "memory") 1)
                (func (export "blueice_start")
                    call $event_kind
                    i32.const 1
                    i32.eq
                    if
                        call $event_tab_id
                        i64.const 1
                        i64.ne
                        if unreachable end
                        i64.const 1
                        i32.const 0
                        i32.const 1024
                        call $read
                        drop
                    end))"#,
        )
        .unwrap(),
    )
    .unwrap();
    let extension_id = load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    (root, manifest, extension_id)
}

fn network_rule_clear_manifest_package(label: &str) -> (PathBuf, PathBuf, String) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "blueice-extension-host-network-rule-package-{label}-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(
        &manifest,
        r#"{"name":"Network rule clear","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["network:intercept"]}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("extension.wasm"),
        wat::parse_str(
            r#"(module
                (import "blueice" "clear_network_block_urls" (func $clear (result i32)))
                (func (export "blueice_start")
                    call $clear
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
        )
        .unwrap(),
    )
    .unwrap();
    let extension_id = load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    (root, manifest, extension_id)
}

fn network_host_rule_manifest_package(label: &str) -> (PathBuf, PathBuf, String) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "blueice-extension-host-network-host-package-{label}-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(
        &manifest,
        r#"{"name":"Network host rule","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["network:intercept"]}}"#,
    ).unwrap();
    std::fs::write(
        root.join("extension.wasm"),
        wat::parse_str(
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
        ).unwrap(),
    ).unwrap();
    let extension_id = load_installed_extension(&manifest).unwrap().extension_id().to_string();
    (root, manifest, extension_id)
}

fn network_path_prefix_and_redirect_manifest_package(label: &str) -> (PathBuf, PathBuf, String) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "blueice-extension-host-path-package-{label}-{}-{id}", std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Network path rule","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["network:intercept"]}}"#,
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), wat::parse_str(
        r#"(module
            (import "blueice" "register_network_block_path_prefix" (func $block (param i32 i32 i32 i32) (result i32)))
            (import "blueice" "register_network_redirect_url" (func $redirect (param i32 i32 i32 i32) (result i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "example.test")
            (data (i32.const 16) "/private")
            (data (i32.const 32) "https://example.test/old")
            (data (i32.const 64) "https://example.test/new")
            (func (export "blueice_start")
                i32.const 0
                i32.const 12
                i32.const 16
                i32.const 8
                call $block
                i32.const 0
                i32.ne
                if unreachable end
                i32.const 32
                i32.const 24
                i32.const 64
                i32.const 24
                call $redirect
                i32.const 0
                i32.ne
                if unreachable end))"#,
    ).unwrap()).unwrap();
    let extension_id = load_installed_extension(&manifest).unwrap().extension_id().to_string();
    (root, manifest, extension_id)
}

fn storage_manifest_package(label: &str) -> (PathBuf, PathBuf, String) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "blueice-extension-host-storage-package-{label}-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(
        &manifest,
        r#"{"name":"Storage","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["storage"]}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("extension.wasm"),
        wat::parse_str(
            r#"(module
                (import "blueice" "storage_set_utf8" (func $set (param i32 i32 i32 i32) (result i32)))
                (import "blueice" "durable_storage_set_utf8" (func $durable_set (param i32 i32 i32 i32) (result i32)))
                (import "blueice" "durable_storage_keys_utf8" (func $durable_keys (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "task-state")
                (data (i32.const 16) "complete")
                (data (i32.const 32) "saved")
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
                    i32.const 32
                    i32.const 5
                    call $durable_set
                    i32.const 0
                    i32.ne
                    if unreachable end
                    i32.const 64
                    i32.const 64
                    call $durable_keys
                    i32.const 14
                    i32.ne
                    if unreachable end
                    i32.const 64
                    i32.load8_u
                    i32.const 91
                    i32.ne
                    if unreachable end))"#,
        )
        .unwrap(),
    )
    .unwrap();
    let extension_id = load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    (root, manifest, extension_id)
}

fn network_trace_manifest_package(label: &str) -> (PathBuf, PathBuf, String) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "blueice-extension-host-trace-package-{label}-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(
        &manifest,
        r#"{"name":"Trace","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["network:observe"]}}"#,
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), wat::parse_str(
        r#"(module
            (import "blueice" "network_trace_utf8" (func $trace (param i64 i32 i32) (result i32)))
            (memory (export "memory") 1)
            (func (export "blueice_start")
                i64.const 1
                i32.const 0
                i32.const 1024
                call $trace
                i32.const 0
                i32.le_s
                if unreachable end))"#,
    ).unwrap()).unwrap();
    let extension_id = load_installed_extension(&manifest).unwrap().extension_id().to_string();
    (root, manifest, extension_id)
}

fn visible_text_manifest_package(label: &str) -> (PathBuf, PathBuf, String) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "blueice-extension-host-range-package-{label}-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(
        &manifest,
        r#"{"name":"Range","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["dom:write"]}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("extension.wasm"),
        wat::parse_str(
            r#"(module
                (import "blueice" "set_range_input_value" (func $set (param i64 i64 i64) (result i32)))
                (import "blueice" "set_visible_leaf_text" (func $leaf (param i64 i64 i32 i32) (result i32)))
                (import "blueice" "set_visible_text_content" (func $content (param i64 i64 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "Updated heading")
                (data (i32.const 32) "Updated formatted text")
                (func (export "blueice_start")
                    i64.const 7
                    i64.const 17
                    i64.const -3
                    call $set
                    i32.const 0
                    i32.ne
                    if unreachable end
                    i64.const 7
                    i64.const 19
                    i32.const 0
                    i32.const 15
                    call $leaf
                    i32.const 0
                    i32.ne
                    if unreachable end
                    i64.const 7
                    i64.const 21
                    i32.const 32
                    i32.const 22
                    call $content
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
        )
        .unwrap(),
    )
    .unwrap();
    let extension_id = load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    (root, manifest, extension_id)
}

fn popup_action_manifest_package(label: &str) -> (PathBuf, PathBuf, String) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "blueice-extension-host-popup-action-{label}-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(
        &manifest,
        r#"{"name":"Popup action","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["ui:inject"]}}"#,
    ).unwrap();
    std::fs::write(
        root.join("extension.wasm"),
        wat::parse_str(r#"(module
            (import "blueice" "runtime_event_kind" (func $kind (result i32)))
            (import "blueice" "runtime_event_tab_id" (func $tab (result i64)))
            (import "blueice" "set_toolbar_button_utf8" (func $toolbar (param i32 i32) (result i32)))
            (import "blueice" "show_popup_action_utf8" (func $popup (param i64 i32 i32 i32 i32 i32 i32) (result i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "Notes")
            (data (i32.const 16) "Ready to open")
            (data (i32.const 48) "Open")
            (data (i32.const 64) "Done")
            (func (export "blueice_start")
                call $kind
                i32.const 0
                i32.eq
                if
                    i32.const 0
                    i32.const 5
                    call $toolbar
                    i32.const 0
                    i32.ne
                    if unreachable end
                end
                call $kind
                i32.const 2
                i32.eq
                if
                    call $tab
                    i32.const 0
                    i32.const 5
                    i32.const 16
                    i32.const 13
                    i32.const 48
                    i32.const 4
                    call $popup
                    i32.const 0
                    i32.ne
                    if unreachable end
                end
                call $kind
                i32.const 3
                i32.eq
                if
                    i32.const 64
                    i32.const 4
                    call $toolbar
                    i32.const 0
                    i32.ne
                    if unreachable end
                end))"#).unwrap(),
    ).unwrap();
    let extension_id = load_installed_extension(&manifest).unwrap().extension_id().to_string();
    (root, manifest, extension_id)
}

#[test]
fn missing_socket_flag_exits_with_failure_and_no_socket_is_created() {
    let output = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .output()
        .expect("failed to run blueice-extension-host");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--socket"));
}

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
fn a_manifest_derived_identity_is_required_over_a_real_process_boundary() {
    let (root, manifest, extension_id) = manifest_package("derived-id");
    {
        let host = ExtensionHost::spawn_with_manifest("derived-id", Some(&manifest));

        let mut installed_extension = host.connect();
        write_extension_request(&mut installed_extension, &hello(&extension_id)).unwrap();
        assert_eq!(
            read_extension_reply(&mut installed_extension).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(&mut installed_extension, &ExtensionRequest::DomRead).unwrap();
        assert!(matches!(
            read_extension_reply(&mut installed_extension).unwrap(),
            ExtensionReply::DomReadResult { .. }
        ));
        drop(installed_extension);

        let mut friendly_name_impersonator = host.connect();
        write_extension_request(&mut friendly_name_impersonator, &hello("Binary test")).unwrap();
        assert_eq!(
            read_extension_reply(&mut friendly_name_impersonator).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(&mut friendly_name_impersonator, &ExtensionRequest::DomRead)
            .unwrap();
        assert!(matches!(
            read_extension_reply(&mut friendly_name_impersonator).unwrap(),
            ExtensionReply::CapabilityDenied { capability, .. } if capability == "dom:read"
        ));
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn core_connection_mode_authenticates_then_runs_a_navigation_event_reactor_over_real_ipc() {
    let (root, manifest, _) = manifest_package("core-connect");
    std::fs::write(
        &manifest,
        r#"{"name":"Binary test","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["dom:read"],"optional":["storage"],"runtime_ephemeral":["dom:write"]}}"#,
    )
    .unwrap();
    let extension_id = load_installed_extension(&manifest).unwrap().extension_id().to_string();
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
                ("dom:read".to_string(), 2),
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
fn core_connection_mode_runs_ui_v3_popup_action_over_a_real_host_process() {
    let (root, manifest, extension_id) = popup_action_manifest_package("core-connect");
    let socket = unique_socket_path("core-popup-action");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let authentication = "test-only-popup-action-credential";
    let mut host = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .args([
            "--connect", socket.to_str().unwrap(),
            "--manifest", manifest.to_str().unwrap(),
        ])
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        .spawn()
        .expect("failed to launch blueice-extension-host for popup action");
    let (mut stream, _) = listener.accept().unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::HelloAuthenticated {
            extension_id,
            capability_versions: BTreeMap::from([("ui:inject".to_string(), 3)]),
            authentication: authentication.to_string(),
        }
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::HelloAck { unsupported_capabilities: BTreeMap::new() },
    ).unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::RuntimeReady);
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::RuntimeStart)
        .unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::SetToolbarButton { label: "Notes".to_string() });
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::UiInjectAck)
        .unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::NextRuntimeEvent);
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::RuntimeEvent(
            blueice_ipc::extension::ExtensionRuntimeEvent::ToolbarActivated { tab_id: 7 },
        ),
    ).unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::ShowPopupAction {
            tab_id: 7,
            title: "Notes".to_string(),
            body: "Ready to open".to_string(),
            action_label: "Open".to_string(),
        });
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::UiInjectAck)
        .unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::NextRuntimeEvent);
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::RuntimeEvent(
            blueice_ipc::extension::ExtensionRuntimeEvent::PopupActionActivated { tab_id: 7 },
        ),
    ).unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::SetToolbarButton { label: "Done".to_string() });
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::UiInjectAck)
        .unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::NextRuntimeEvent);
    blueice_ipc::extension::write_extension_reply(
        &mut stream,
        &ExtensionReply::RuntimeEventStreamClosed,
    ).unwrap();
    drop(listener);
    assert!(host.wait().unwrap().success());
    let _ = std::fs::remove_file(socket);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn core_connection_mode_negotiates_v6_and_runs_v3_network_rule_clear_over_real_ipc() {
    let (root, manifest, extension_id) = network_rule_clear_manifest_package("core-connect");
    let socket = unique_socket_path("core-network-clear");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let authentication = "test-only-network-clear-credential";
    let mut host = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .args([
            "--connect",
            socket.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        .spawn()
        .expect("failed to launch blueice-extension-host for network rule clearing");

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
        ExtensionRequest::ClearNetworkBlockUrls
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

#[test]
fn core_connection_mode_negotiates_v6_and_runs_host_rule_over_real_ipc() {
    let (root, manifest, extension_id) = network_host_rule_manifest_package("core-connect");
    let socket = unique_socket_path("core-network-host");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let authentication = "test-only-network-host-credential";
    let mut host = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .args([
            "--connect", socket.to_str().unwrap(),
            "--manifest", manifest.to_str().unwrap(),
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
        &ExtensionReply::HelloAck { unsupported_capabilities: BTreeMap::new() },
    ).unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::RuntimeReady
    );
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::RuntimeStart)
        .unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::RegisterNetworkBlockHost { host: "example.test".to_string() }
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream, &ExtensionReply::NetworkInterceptAck,
    ).unwrap();
    assert_eq!(
        blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::NextRuntimeEvent
    );
    blueice_ipc::extension::write_extension_reply(
        &mut stream, &ExtensionReply::RuntimeEventStreamClosed,
    ).unwrap();
    drop(listener);
    assert!(host.wait().unwrap().success());
    let _ = std::fs::remove_file(socket);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn core_connection_mode_runs_v5_path_and_v6_redirect_imports_over_real_ipc() {
    let (root, manifest, extension_id) = network_path_prefix_and_redirect_manifest_package("core-connect");
    let socket = unique_socket_path("core-path");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let authentication = "test-only-network-path-credential";
    let mut host = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .args(["--connect", socket.to_str().unwrap(), "--manifest", manifest.to_str().unwrap()])
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        .spawn()
        .expect("failed to launch extension host for the v5 path rule");

    let (mut stream, _) = listener.accept().unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::HelloAuthenticated {
            extension_id,
            capability_versions: BTreeMap::from([("network:intercept".to_string(), 6)]),
            authentication: authentication.to_string(),
        });
    blueice_ipc::extension::write_extension_reply(&mut stream,
        &ExtensionReply::HelloAck { unsupported_capabilities: BTreeMap::new() },
    ).unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::RuntimeReady);
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::RuntimeStart).unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::RegisterNetworkBlockPathPrefix {
            host: "example.test".into(), path_prefix: "/private".into(),
        });
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::NetworkInterceptAck).unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::RegisterNetworkRedirectUrl {
            source_url: "https://example.test/old".into(),
            target_url: "https://example.test/new".into(),
        });
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::NetworkInterceptAck).unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::NextRuntimeEvent);
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::RuntimeEventStreamClosed).unwrap();
    drop(listener);
    assert!(host.wait().unwrap().success());
    let _ = std::fs::remove_file(socket);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn core_connection_mode_negotiates_network_observe_v2_and_runs_trace_import() {
    let (root, manifest, extension_id) = network_trace_manifest_package("core-connect");
    let socket = unique_socket_path("core-trace");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let authentication = "test-only-network-trace-credential";
    let mut host = Command::new(env!("CARGO_BIN_EXE_blueice-extension-host"))
        .args(["--connect", socket.to_str().unwrap(), "--manifest", manifest.to_str().unwrap()])
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        .spawn()
        .expect("failed to launch blueice-extension-host for network trace");

    let (mut stream, _) = listener.accept().unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::HelloAuthenticated {
            extension_id,
            capability_versions: BTreeMap::from([("network:observe".to_string(), 2)]),
            authentication: authentication.to_string(),
        });
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::HelloAck {
        unsupported_capabilities: BTreeMap::new(),
    }).unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::RuntimeReady);
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::RuntimeStart).unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::ReadNetworkTrace { tab_id: 1 });
    blueice_ipc::extension::write_extension_reply(&mut stream, &ExtensionReply::NetworkTraceResult {
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
    }).unwrap();
    assert_eq!(blueice_ipc::extension::read_extension_request(&mut stream).unwrap(),
        ExtensionRequest::NextRuntimeEvent);
    blueice_ipc::extension::write_extension_reply(
        &mut stream, &ExtensionReply::RuntimeEventStreamClosed,
    ).unwrap();
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
        &ExtensionReply::StorageKeysResult { keys: vec!["task-state".to_string()] },
    ).unwrap();
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

#[test]
fn an_unsupported_capability_version_is_reported_without_breaking_a_compatible_capability() {
    let host = ExtensionHost::spawn("partial-version");
    let mut stream = host.connect();

    write_extension_request(
        &mut stream,
        &ExtensionRequest::Hello {
            extension_id: "minimal-slice-extension".to_string(),
            capability_versions: BTreeMap::from([
                ("dom:read".to_string(), 1),
                ("future:capability".to_string(), 99),
            ]),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::from([(
                "future:capability".to_string(),
                blueice_ipc::extension::UnsupportedCapabilityVersion::UnknownCapability,
            )]),
        }
    );

    // The host must keep the connection alive and honor the separately
    // compatible, granted capability.
    write_extension_request(&mut stream, &ExtensionRequest::DomRead).unwrap();
    assert!(matches!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::DomReadResult { .. }
    ));
}

#[test]
fn a_non_hello_first_message_gets_no_reply_and_the_connection_ends() {
    let host = ExtensionHost::spawn("bad-first-msg");
    let mut stream = host.connect();

    write_extension_request(&mut stream, &ExtensionRequest::DomRead).unwrap();

    // The real subprocess must never answer a request sent before a
    // successful handshake -- reading now must fail (the connection was
    // dropped server-side), not return a stray reply or hang.
    assert!(read_extension_reply(&mut stream).is_err());
}

#[test]
fn the_real_subprocess_serves_two_independent_connections_in_sequence() {
    let host = ExtensionHost::spawn("two-conns");

    for _ in 0..2 {
        let mut stream = host.connect();
        write_extension_request(&mut stream, &hello("minimal-slice-extension")).unwrap();
        assert_eq!(
            read_extension_reply(&mut stream).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(&mut stream, &ExtensionRequest::DomRead).unwrap();
        assert!(matches!(
            read_extension_reply(&mut stream).unwrap(),
            ExtensionReply::DomReadResult { .. }
        ));
    }
}
