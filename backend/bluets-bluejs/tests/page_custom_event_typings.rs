// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};
use blueice_bluets_bluejs::{
    compile_direct_script,
    page_host_typings::{
        page_host_dom_custom_event_runtime_bindings_v7, PageHostDocumentTypingsV1,
    },
};

#[test]
fn custom_event_profile_is_additive_and_keeps_payload_and_dispatch_types() {
    let old = PageHostDocumentTypingsV1::generate_dom_abort();
    let profile = PageHostDocumentTypingsV1::generate_dom_custom_event();
    let bindings = page_host_dom_custom_event_runtime_bindings_v7();
    assert_eq!(bindings.len(), 37);
    assert!(!old
        .declaration_source
        .contains("declare const CustomEvent:"));
    assert!(profile.verify_runtime_bindings(&bindings[..36]).is_err());
    let ambient = profile
        .verified_dom_custom_event_ambient_module(&bindings)
        .unwrap();
    for (source, accepted) in [
        ("function send(target: BlueIceEventTarget): boolean { const event = new CustomEvent('data', { detail: {value: 7}, cancelable: true }); return target.dispatchEvent(event); }", true),
        ("function detail(event: BlueIceCustomEvent): unknown { event.initCustomEvent('data', false, false, 7); return event.detail; }", true),
        ("function bad(event: BlueIceCustomEvent): void { event.detail = 7; }", false),
        ("function bad(): BlueIceCustomEvent { return new CustomEvent(7); }", false),
    ] {
        let result = compile_direct_script("memory:///custom-event.ts", &MapLoader::from([ModuleSource::new("memory:///custom-event.ts", source)]), CompilerOptions {
            runtime_policy: RuntimePolicy::Checked,
            require_declared_global_calls: true,
            ambient_declaration_modules: vec![ambient.clone()],
            ..CompilerOptions::default()
        });
        assert_eq!(result.is_ok(), accepted, "{source}: {:?}", result.as_ref().err());
    }
}
