// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};
use blueice_bluets_bluejs::{
    compile_direct_script,
    page_host_typings::{page_host_dom_abort_runtime_bindings_v6, PageHostDocumentTypingsV1},
};
#[test]
fn abort_profile_is_additive_and_types_signal_based_listener_removal() {
    let old = PageHostDocumentTypingsV1::generate_dom_object_url();
    let profile = PageHostDocumentTypingsV1::generate_dom_abort();
    let bindings = page_host_dom_abort_runtime_bindings_v6();
    assert_eq!(bindings.len(), 36);
    assert!(!old
        .declaration_source
        .contains("declare const AbortController:"));
    assert!(profile.verify_runtime_bindings(&bindings[..35]).is_err());
    let ambient = profile
        .verified_dom_abort_ambient_module(&bindings)
        .unwrap();
    for(source,accepted)in[
        ("function handler(event: BlueIceEvent): void {} function stop(target: BlueIceEventTarget): void { const controller = new AbortController(); target.addEventListener('x', handler, { signal: controller.signal }); controller.abort('stop'); }",true),
        ("function reason(signal: BlueIceAbortSignal): unknown { signal.throwIfAborted(); return signal.reason; }",true),
        ("function bad(signal: BlueIceAbortSignal): void { signal.aborted = true; }",false),
        ("function bad(): BlueIceAbortSignal { return AbortSignal.any(['signal']); }",false),
    ]{
        let result=compile_direct_script("memory:///abort.ts",&MapLoader::from([ModuleSource::new("memory:///abort.ts",source)]),CompilerOptions{runtime_policy:RuntimePolicy::Checked,require_declared_global_calls:true,ambient_declaration_modules:vec![ambient.clone()],..CompilerOptions::default()});
        assert_eq!(result.is_ok(),accepted,"{source}: {:?}",result.as_ref().err());
    }
}
