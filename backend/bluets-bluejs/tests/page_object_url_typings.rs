// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};
use blueice_bluets_bluejs::{
    compile_direct_script,
    page_host_typings::{page_host_dom_object_url_runtime_bindings_v5, PageHostDocumentTypingsV1},
};

#[test]
fn object_url_profile_requires_the_exact_inventory_and_preserves_stream_profile() {
    let old = PageHostDocumentTypingsV1::generate_dom_stream();
    let profile = PageHostDocumentTypingsV1::generate_dom_object_url();
    let bindings = page_host_dom_object_url_runtime_bindings_v5();
    assert_eq!(bindings.len(), 34);
    assert!(!old.declaration_source.contains("declare const URL:"));
    assert!(profile.verify_runtime_bindings(&bindings[..33]).is_err());
    let ambient = profile
        .verified_dom_object_url_ambient_module(&bindings)
        .unwrap();
    for (source, accepted) in [
        ("function publish(blob: BlueIceBlob): string { return URL.createObjectURL(blob); } function revoke(url: string): void { URL.revokeObjectURL(url); }", true),
        ("function make(): string { const url = new URL('/path', 'https://example.test/'); url.hash = '#section'; return url.href; }", true),
        ("function bad(): string { return URL.createObjectURL('bytes'); }", false),
        ("function bad(url: BlueIceURL): void { url.origin = 'https://forged.test'; }", false),
    ] {
        let result = compile_direct_script("memory:///object-urls.ts", &MapLoader::from([ModuleSource::new("memory:///object-urls.ts", source)]), CompilerOptions {
            runtime_policy: RuntimePolicy::Checked, require_declared_global_calls: true,
            ambient_declaration_modules: vec![ambient.clone()], ..CompilerOptions::default()
        });
        assert_eq!(result.is_ok(), accepted, "{source}: {:?}", result.as_ref().err());
    }
}
