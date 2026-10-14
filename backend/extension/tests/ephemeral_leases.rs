// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_extension_host::{
    load_installed_extension, registry_for_installed_extension, CAPABILITY_DOM_READ,
};

#[test]
fn ephemeral_tickets_are_fresh_and_rearming_invalidates_the_previous_bearer() {
    let root = std::env::temp_dir().join(format!("bi-ephemeral-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("manifest.json");
    std::fs::write(
        &manifest,
        r#"{"name":"Lease regression","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"runtime_ephemeral":["dom:read"]},"capability_origins":{"dom:read":["https://example.test"]}}"#,
    )
    .unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = load_installed_extension(&manifest).unwrap();
    let registry = registry_for_installed_extension(&installed);
    let id = installed.extension_id();
    let previous = registry
        .arm_runtime_ephemeral(id, CAPABILITY_DOM_READ, 1, 2)
        .unwrap();
    let current = registry
        .arm_runtime_ephemeral(id, CAPABILITY_DOM_READ, 1, 2)
        .unwrap();
    assert_ne!(previous, current);
    assert_eq!(current.len(), 64);
    assert!(current
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    assert!(!registry.has_capability(id, CAPABILITY_DOM_READ));
    assert!(!registry.consume_runtime_ephemeral(id, CAPABILITY_DOM_READ, &previous, 1, 2));
    assert!(!registry.consume_runtime_ephemeral(id, CAPABILITY_DOM_READ, &current, 2, 2));
    assert!(registry.consume_runtime_ephemeral(id, CAPABILITY_DOM_READ, &current, 1, 2));
    assert!(!registry.consume_runtime_ephemeral(id, CAPABILITY_DOM_READ, &current, 1, 2));
    assert!(!registry.has_capability(id, CAPABILITY_DOM_READ));
    std::fs::remove_dir_all(root).unwrap();
}
