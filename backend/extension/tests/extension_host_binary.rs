// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

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
    let extension_id = load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    (root, manifest, extension_id)
}

fn network_path_prefix_and_redirect_manifest_package(label: &str) -> (PathBuf, PathBuf, String) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "blueice-extension-host-path-package-{label}-{}-{id}",
        std::process::id()
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
    let extension_id = load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
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
    std::fs::write(
        root.join("extension.wasm"),
        wat::parse_str(
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
    let extension_id = load_installed_extension(&manifest)
        .unwrap()
        .extension_id()
        .to_string();
    (root, manifest, extension_id)
}

#[path = "extension_host_binary/transport.rs"]
mod transport;

#[path = "extension_host_binary/permissions.rs"]
mod permissions;

#[path = "extension_host_binary/extension_dom.rs"]
mod extension_dom;

#[path = "extension_host_binary/lifecycle.rs"]
mod lifecycle;

#[path = "extension_host_binary/extension_network.rs"]
mod extension_network;

#[path = "extension_host_binary/cutover.rs"]
mod cutover;
