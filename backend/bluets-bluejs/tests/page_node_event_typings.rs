// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};
use blueice_bluets_bluejs::{
    compile_direct_script,
    page_host_typings::{page_host_dom_node_event_runtime_bindings_v8, PageHostDocumentTypingsV1},
};

#[test]
fn node_event_profile_preserves_prior_profiles_and_types_general_dispatch() {
    let old = PageHostDocumentTypingsV1::generate_dom_custom_event();
    let profile = PageHostDocumentTypingsV1::generate_dom_node_event();
    let bindings = page_host_dom_node_event_runtime_bindings_v8();
    assert_eq!(bindings.len(), 38);
    assert!(!old
        .declaration_source
        .contains("interface BlueIceNodeDispatch"));
    assert!(profile.verify_runtime_bindings(&bindings[..37]).is_err());
    let ambient = profile
        .verified_dom_node_event_ambient_module(&bindings)
        .unwrap();
    for(source,accepted)in[
        ("function handler(event: BlueIceEvent): void {} function dispatch(node: BlueIceNode): boolean { node.addEventListener('custom', handler, {capture: true, signal: new AbortController().signal}); return node.dispatchEvent(new CustomEvent('custom', {detail: 7, bubbles: true})); }",true),
        ("function dispatch(): boolean { return document.dispatchEvent(new Event('custom')); }",true),
        ("function clicks(node: BlueIceNode): void { function handler(event: BlueIceClickEvent): void {} node.addEventListener('click', handler); }",true),
        ("function bad(node: BlueIceNode): boolean { return node.dispatchEvent(7); }",false),
        ("function bad(node: BlueIceNode): void { node.addEventListener('custom', 7); }",false),
    ]{
        let result=compile_direct_script("memory:///node-events.ts",&MapLoader::from([ModuleSource::new("memory:///node-events.ts",source)]),CompilerOptions{runtime_policy:RuntimePolicy::Checked,require_declared_global_calls:true,ambient_declaration_modules:vec![ambient.clone()],..CompilerOptions::default()});
        assert_eq!(result.is_ok(),accepted,"{source}: {:?}",result.as_ref().err());
    }
}
