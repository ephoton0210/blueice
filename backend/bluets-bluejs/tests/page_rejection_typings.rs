// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};
use blueice_bluets_bluejs::{
    compile_direct_script,
    page_host_typings::{
        page_host_dom_errors_runtime_bindings_v15, page_host_dom_rejections_runtime_bindings_v16,
        PageHostDocumentTypingsV1, PAGE_HOST_DOM_REJECTIONS_PROFILE_V16,
    },
};

fn accepts(ambient: ModuleSource, source: &str) -> bool {
    let result = compile_direct_script(
        "memory:///rejections.ts",
        &MapLoader::from([ModuleSource::new("memory:///rejections.ts", source)]),
        CompilerOptions {
            runtime_policy: RuntimePolicy::Checked,
            require_declared_global_calls: true,
            ambient_declaration_modules: vec![ambient],
            ..Default::default()
        },
    );
    if let Err(error) = &result {
        eprintln!("{source}: {error:?}");
    }
    result.is_ok()
}

#[test]
fn rejection_profile_extends_runtime_identity_and_rejects_incomplete_installation() {
    let profile = PageHostDocumentTypingsV1::generate_dom_rejections();
    let bindings = page_host_dom_rejections_runtime_bindings_v16();
    assert_eq!(profile.profile, PAGE_HOST_DOM_REJECTIONS_PROFILE_V16);
    assert_eq!(bindings[..58], page_host_dom_errors_runtime_bindings_v15());
    assert!(profile
        .verified_dom_rejections_ambient_module(&bindings)
        .is_ok());
    assert!(profile
        .verified_dom_rejections_ambient_module(&bindings[..60])
        .is_err());
    let mut wrong = bindings;
    wrong[60] = wrong[59];
    assert!(profile
        .verified_dom_rejections_ambient_module(&wrong)
        .is_err());
    assert!(PageHostDocumentTypingsV1::generate_dom_errors()
        .verified_dom_rejections_ambient_module(&bindings)
        .is_err());
}

#[test]
fn rejection_members_check_constructor_callbacks_and_readonly_fields() {
    let ambient = PageHostDocumentTypingsV1::generate_dom_rejections()
        .verified_dom_rejections_ambient_module(&page_host_dom_rejections_runtime_bindings_v16())
        .unwrap();
    let mut failures = Vec::new();
    for(source,expected)in[
        ("function rejectTyped(value: string): void {throw value;} new Blob(['typed-rejection']).text().then(rejectTyped);",true),
        ("Promise.reject('typed-rejection');",false),
        ("const event = new window.PromiseRejectionEvent('unhandledrejection',{promise:{},reason:7,cancelable:true}); const promise:object=event.promise; const reason:unknown=event.reason;",true),
        ("function listener(event: BlueIcePromiseRejectionEvent): void {const reason:unknown=event.reason;event.preventDefault();} window.addEventListener('unhandledrejection',listener,{once:true}); removeEventListener('rejectionhandled',listener); window.onunhandledrejection=listener; onrejectionhandled=null;",true),
        ("function listener(event: BlueIceErrorEvent): void {event.preventDefault();} addEventListener('error',listener); reportError('still supported');",true),
        ("new PromiseRejectionEvent('error');",false),
        ("new PromiseRejectionEvent('error',{});",false),
        ("new PromiseRejectionEvent('error',{promise:7});",false),
        ("const event=new PromiseRejectionEvent('x',{promise:{}}); event.reason=7;",false),
        ("window.onunhandledrejection=7;",false),
        ("window.onrejectionhandled='wrong';",false),
        ("window.fetch('https://example.test');",false),
    ]{
        if accepts(ambient.clone(),source)!=expected {failures.push((source,expected));}
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn immutable_error_profile_does_not_gain_rejection_bindings() {
    let ambient = PageHostDocumentTypingsV1::generate_dom_errors()
        .verified_dom_errors_ambient_module(&page_host_dom_errors_runtime_bindings_v15())
        .unwrap();
    for source in [
        "new PromiseRejectionEvent('x',{promise:{}});",
        "window.onunhandledrejection=null;",
        "window.onrejectionhandled=null;",
    ] {
        assert!(!accepts(ambient.clone(), source), "{source}");
    }
}
