// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};
use blueice_bluets_bluejs::{
    compile_direct_script,
    page_host_typings::{
        page_host_dom_console_runtime_bindings_v14, page_host_dom_errors_runtime_bindings_v15,
        PageHostDocumentTypingsV1, PAGE_HOST_DOM_ERRORS_PROFILE_V15,
    },
};

fn accepts(ambient: ModuleSource, source: &str) -> bool {
    let result = compile_direct_script(
        "memory:///errors.ts",
        &MapLoader::from([ModuleSource::new("memory:///errors.ts", source)]),
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
fn errors_profile_requires_exact_bindings_and_extends_the_immutable_console_profile() {
    let profile = PageHostDocumentTypingsV1::generate_dom_errors();
    let bindings = page_host_dom_errors_runtime_bindings_v15();
    assert_eq!(profile.profile, PAGE_HOST_DOM_ERRORS_PROFILE_V15);
    assert_eq!(bindings[..52], page_host_dom_console_runtime_bindings_v14());
    assert!(profile
        .verified_dom_errors_ambient_module(&bindings)
        .is_ok());
    assert!(profile
        .verified_dom_errors_ambient_module(&bindings[..57])
        .is_err());
    let mut wrong = bindings;
    wrong[57] = wrong[56];
    assert!(profile.verified_dom_errors_ambient_module(&wrong).is_err());
    assert!(PageHostDocumentTypingsV1::generate_dom_console()
        .verified_dom_errors_ambient_module(&bindings)
        .is_err());
}

#[test]
fn installed_error_members_accept_typed_use_and_reject_incorrect_calls() {
    let ambient = PageHostDocumentTypingsV1::generate_dom_errors()
        .verified_dom_errors_ambient_module(&page_host_dom_errors_runtime_bindings_v15())
        .unwrap();
    let mut failures = Vec::new();
    for (source, expected) in [
        ("reportError('problem'); window.reportError({code: 7}); self.onerror = null;", true),
        ("const event = new window.ErrorEvent('error', {message:'problem',filename:'page.js',lineno:1,colno:2,error:7,cancelable:true}); const message:string = event.message; window.dispatchEvent(event);", true),
        ("function listener(event: BlueIceErrorEvent): void { const message:string = event.message; event.preventDefault(); } addEventListener('error', listener, {once:true}); window.removeEventListener('error', listener);", true),
        ("function listener(message: string | BlueIceEvent, filename?: string, lineno?: number, colno?: number, error?: unknown): boolean {return true;} window.onerror = listener;", true),
        ("reportError();", false),
        ("new ErrorEvent('error', {lineno:'wrong'});", false),
        ("const event = new ErrorEvent('error'); event.message = 'changed';", false),
        ("window.dispatchEvent(7);", false),
        ("window.onerror = 7;", false),
        ("window.fetch('https://example.test');", false),
    ] {
        if accepts(ambient.clone(), source) != expected { failures.push((source, expected)); }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn older_console_profile_keeps_the_error_reporting_surface_absent() {
    let ambient = PageHostDocumentTypingsV1::generate_dom_console()
        .verified_dom_console_ambient_module(&page_host_dom_console_runtime_bindings_v14())
        .unwrap();
    for source in [
        "reportError(7);",
        "window.reportError(7);",
        "new ErrorEvent('error');",
        "window.onerror=null;",
        "window.addEventListener('error', function(){});",
    ] {
        assert!(!accepts(ambient.clone(), source), "{source}");
    }
}
