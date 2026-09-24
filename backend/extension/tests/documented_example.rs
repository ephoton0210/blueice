// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Keeps the published Phase 9 sample package executable against the current
//! manifest loader and no-WASI ABI, rather than letting documentation drift.

use blueice_extension_host::{
    execute_installed_extension_for_invocation, load_installed_extension,
    registry_for_installed_extension, RuntimeInvocation, CAPABILITY_UI_INJECT,
};
use blueice_ipc::extension::{
    read_extension_request, write_extension_reply, ExtensionReply, ExtensionRequest,
};
use std::fs;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

static NEXT_PACKAGE: AtomicU64 = AtomicU64::new(1);

#[test]
fn documented_toolbar_package_loads_and_uses_only_its_declared_native_ui_abi() {
    let directory = std::env::temp_dir().join(format!(
        "blueice-extension-example-{}-{}",
        std::process::id(),
        NEXT_PACKAGE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&directory).unwrap();
    let manifest = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../development/browser_core/phase-9-extension-protocol/examples/toolbar/extension.json"
    ));
    let module = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../development/browser_core/phase-9-extension-protocol/examples/toolbar/extension.wat"
    ));
    fs::write(directory.join("extension.json"), manifest).unwrap();
    fs::write(
        directory.join("extension.wasm"),
        wat::parse_str(module).unwrap(),
    )
    .unwrap();
    let extension = load_installed_extension(directory.join("extension.json")).unwrap();
    let registry = registry_for_installed_extension(&extension);
    assert!(registry.has_capability(extension.extension_id(), CAPABILITY_UI_INJECT));

    let (guest, mut core) = UnixStream::pair().unwrap();
    let core_worker = thread::spawn(move || {
        assert_eq!(
            read_extension_request(&mut core).unwrap(),
            ExtensionRequest::SetToolbarButton {
                label: "Example".to_string(),
            }
        );
        write_extension_reply(&mut core, &ExtensionReply::UiInjectAck).unwrap();
        assert_eq!(
            read_extension_request(&mut core).unwrap(),
            ExtensionRequest::ShowPopup {
                tab_id: 7,
                title: "Hello".to_string(),
                body: "Ready".to_string(),
            }
        );
        write_extension_reply(&mut core, &ExtensionReply::UiInjectAck).unwrap();
    });
    execute_installed_extension_for_invocation(
        &extension,
        guest.try_clone().unwrap(),
        RuntimeInvocation::Startup,
    )
    .unwrap();
    execute_installed_extension_for_invocation(
        &extension,
        guest,
        RuntimeInvocation::ToolbarActivated { tab_id: 7 },
    )
    .unwrap();
    core_worker.join().unwrap();
    fs::remove_dir_all(directory).unwrap();
}
