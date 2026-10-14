// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};
use blueice_bluets_bluejs::{
    compile_direct_script,
    page_host_typings::{
        page_host_dom_animation_frame_runtime_bindings_v12,
        page_host_dom_timers_runtime_bindings_v11, PageHostDocumentTypingsV1,
    },
};

#[test]
fn frame_typings_follow_the_exact_runtime_profile_and_callback_signature() {
    let profile = PageHostDocumentTypingsV1::generate_dom_animation_frame();
    let bindings = page_host_dom_animation_frame_runtime_bindings_v12();
    assert_eq!(bindings.len(), 47);
    assert_eq!(
        &bindings[..45],
        &page_host_dom_timers_runtime_bindings_v11()
    );
    assert!(profile.verify_runtime_bindings(&bindings[..46]).is_err());
    let ambient = profile
        .verified_dom_animation_frame_ambient_module(&bindings)
        .unwrap();
    for (source, accepted) in [
        ("function tick(timestamp: number): void {} const handle: number = requestAnimationFrame(tick); cancelAnimationFrame(handle);", true),
        ("function tick(): void {} requestAnimationFrame(tick);", true),
        ("function tick(timestamp: string): void {} requestAnimationFrame(tick);", false),
        ("function tick(timestamp: number, required: number): void {} requestAnimationFrame(tick);", false),
        ("requestAnimationFrame(7);", false),
        ("cancelAnimationFrame('id');", false),
        ("cancelAnimationFrame();", false),
    ] {
        let result = compile_direct_script("memory:///frame.ts", &MapLoader::from([ModuleSource::new("memory:///frame.ts", source)]), CompilerOptions {
            runtime_policy: RuntimePolicy::Checked, require_declared_global_calls: true,
            ambient_declaration_modules: vec![ambient.clone()], ..CompilerOptions::default()
        });
        assert_eq!(result.is_ok(), accepted, "{source}: {:?}", result.err());
    }
    let old = PageHostDocumentTypingsV1::generate_dom_timers();
    let ambient = old
        .verified_dom_timers_ambient_module(&page_host_dom_timers_runtime_bindings_v11())
        .unwrap();
    assert!(!old
        .declaration_source
        .contains("declare function requestAnimationFrame"));
    assert!(compile_direct_script(
        "memory:///old.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///old.ts",
            "function tick(timestamp: number): void {} requestAnimationFrame(tick);"
        )]),
        CompilerOptions {
            runtime_policy: RuntimePolicy::Checked,
            require_declared_global_calls: true,
            ambient_declaration_modules: vec![ambient],
            ..CompilerOptions::default()
        }
    )
    .is_err());
}
