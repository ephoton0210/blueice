// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};
use blueice_bluets_bluejs::{
    compile_direct_script,
    page_host_typings::{page_host_dom_tee_runtime_bindings_v9, PageHostDocumentTypingsV1},
};
#[test]
fn tee_profile_preserves_prior_bytes_and_exposes_two_typed_branches() {
    let old = PageHostDocumentTypingsV1::generate_dom_node_event();
    let profile = PageHostDocumentTypingsV1::generate_dom_tee();
    let bindings = page_host_dom_tee_runtime_bindings_v9();
    assert_eq!(bindings.len(), 39);
    assert!(!old
        .declaration_source
        .contains("interface BlueIceReadableStreamTee"));
    assert!(profile.verify_runtime_bindings(&bindings[..38]).is_err());
    let ambient = profile.verified_dom_tee_ambient_module(&bindings).unwrap();
    for(source,accepted)in[
        ("function branch(stream: BlueIceReadableStream): boolean { const branches = stream.tee(); return branches[0].locked; }",true),
        ("function branch(stream: BlueIceReadableStream): BlueIceReadableStream { const branches = stream.tee(); return branches[1]; }",true),
        ("function bad(stream: BlueIceReadableStream): void { stream.tee(7); }",false),
        ("function bad(stream: BlueIceReadableStream): number { return stream.tee(); }",false),
    ]{
        let result=compile_direct_script("memory:///tee.ts",&MapLoader::from([ModuleSource::new("memory:///tee.ts",source)]),CompilerOptions{runtime_policy:RuntimePolicy::Checked,require_declared_global_calls:true,ambient_declaration_modules:vec![ambient.clone()],..CompilerOptions::default()});
        assert_eq!(result.is_ok(),accepted,"{source}: {:?}",result.as_ref().err());
    }
}
