// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};
use blueice_bluets_bluejs::{
    compile_direct_script,
    page_host_typings::{page_host_dom_timers_runtime_bindings_v11, PageHostDocumentTypingsV1},
};

#[test]
fn timers_are_typed_only_in_the_exact_installed_profile() {
    let profile = PageHostDocumentTypingsV1::generate_dom_timers();
    let bindings = page_host_dom_timers_runtime_bindings_v11();
    assert_eq!(bindings.len(), 45);
    assert!(profile.verify_runtime_bindings(&bindings[..44]).is_err());
    let ambient = profile
        .verified_dom_timers_ambient_module(&bindings)
        .unwrap();
    for (source, accepted) in [
        ("function tick(value: number): void {} function job(): void {} const timer: number = setInterval(tick, 5, 7); clearTimeout(timer); clearInterval(); queueMicrotask(job);", true),
        ("function tick(): void {} const id: number = setTimeout(tick); clearInterval(id);", true),
        ("setTimeout('string code', 0);", false),
        ("function tick(): void {} setInterval(tick, '5');", false),
        ("clearTimeout('id');", false),
        ("queueMicrotask(7);", false),
        ("function job(required: number): void {} queueMicrotask(job);", false),
        ("function frame(): void {} requestAnimationFrame(frame);", false),
    ] {
        let result = compile_direct_script("memory:///timer.ts", &MapLoader::from([ModuleSource::new("memory:///timer.ts", source)]), CompilerOptions {
            runtime_policy: RuntimePolicy::Checked,
            require_declared_global_calls: true,
            ambient_declaration_modules: vec![ambient.clone()],
            ..CompilerOptions::default()
        });
        assert_eq!(result.is_ok(), accepted, "{source}: {:?}", result.err());
    }
    let old = PageHostDocumentTypingsV1::generate_dom_iteration();
    assert!(!old
        .declaration_source
        .contains("declare function setTimeout"));
    let old_ambient = old
        .verified_dom_iteration_ambient_module(
            &blueice_bluets_bluejs::page_host_typings::page_host_dom_iteration_runtime_bindings_v10(
            ),
        )
        .unwrap();
    assert!(compile_direct_script(
        "memory:///old.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///old.ts",
            "function tick(): void {} setTimeout(tick, 0);"
        )]),
        CompilerOptions {
            runtime_policy: RuntimePolicy::Checked,
            require_declared_global_calls: true,
            ambient_declaration_modules: vec![old_ambient],
            ..CompilerOptions::default()
        }
    )
    .is_err());
}
