// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};
use blueice_bluets_bluejs::{
    compile_direct_script,
    page_host_typings::{
        page_host_dom_animation_frame_runtime_bindings_v12,
        page_host_dom_window_runtime_bindings_v13, PageHostDocumentTypingsV1,
        PAGE_HOST_DOM_WINDOW_PROFILE_V13,
    },
};

fn accepts(ambient: ModuleSource, source: &str) -> bool {
    let result = compile_direct_script(
        "memory:///window.ts",
        &MapLoader::from([ModuleSource::new("memory:///window.ts", source)]),
        CompilerOptions {
            runtime_policy: RuntimePolicy::Checked,
            require_declared_global_calls: true,
            ambient_declaration_modules: vec![ambient],
            ..CompilerOptions::default()
        },
    );
    if let Err(error) = &result {
        eprintln!("{source}: {error:?}");
    }
    result.is_ok()
}

#[test]
fn window_profile_extends_the_immutable_frame_profile_and_checks_exact_installation() {
    let profile = PageHostDocumentTypingsV1::generate_dom_window();
    let bindings = page_host_dom_window_runtime_bindings_v13();
    assert_eq!(profile.profile, PAGE_HOST_DOM_WINDOW_PROFILE_V13);
    assert_eq!(bindings.len(), 51);
    assert_eq!(
        &bindings[..47],
        &page_host_dom_animation_frame_runtime_bindings_v12()
    );
    assert!(profile
        .verified_dom_window_ambient_module(&bindings[..50])
        .is_err());
    let mut wrong = bindings;
    wrong[50] = wrong[49];
    assert!(profile.verified_dom_window_ambient_module(&wrong).is_err());
    assert!(PageHostDocumentTypingsV1::generate_dom_animation_frame()
        .verified_dom_window_ambient_module(&bindings)
        .is_err());
}

#[test]
fn aliases_keep_the_installed_host_member_signatures() {
    let profile = PageHostDocumentTypingsV1::generate_dom_window();
    let ambient = profile
        .verified_dom_window_ambient_module(&page_host_dom_window_runtime_bindings_v13())
        .unwrap();
    let mut failures = Vec::new();
    for (source, accepted) in [
        ("const node = self.document.createElement('p'); node.textContent = 'next'; window.document.getElementById('value');", true),
        ("function tick(): void {} const id: number = window.setInterval(tick, 1); top.clearInterval(id); parent.queueMicrotask(tick);", true),
        ("function tick(time: number): void {} const id: number = self.requestAnimationFrame(tick); window.cancelAnimationFrame(id);", true),
        ("const controller = new window.AbortController(); parent.AbortSignal.any([controller.signal]);", true),
        ("const blob = new window.Blob(['text']); const reader = new self.FileReader(); reader.readAsText(blob);", true),
        ("self = window; parent = top; window.self = top; window.parent = self;", true),
        ("window.requestAnimationFrame(7);", false),
        ("function tick(time: string): void {} self.requestAnimationFrame(tick);", false),
        ("window.setTimeout(7);", false),
        ("parent.document.createElement(7);", false),
        ("window.fetch('https://example.test');", false),
        ("const unavailable = window.console;", false),
        ("window = self;", false),
        ("top = parent;", false),
        ("window.top = self;", false),
        ("self.window = parent;", false),
        ("window.AbortSignal.timeout('soon');", false),
    ] {
        if accepts(ambient.clone(), source) != accepted {
            failures.push((source, accepted));
        }
    }
    assert!(
        failures.is_empty(),
        "unexpected typing verdicts: {failures:?}"
    );
}

#[test]
fn older_profile_keeps_alias_names_undeclared() {
    let old = PageHostDocumentTypingsV1::generate_dom_animation_frame();
    assert!(!old.declaration_source.contains("BlueIceWindow"));
    let ambient = old
        .verified_dom_animation_frame_ambient_module(
            &page_host_dom_animation_frame_runtime_bindings_v12(),
        )
        .unwrap();
    for name in ["window", "self", "top", "parent"] {
        assert!(
            !accepts(
                ambient.clone(),
                &format!("{name}.setTimeout(function () {{}}, 1);")
            ),
            "{name}"
        );
    }
}

#[test]
fn window_members_cover_every_installed_host_global() {
    let profile = PageHostDocumentTypingsV1::generate_dom_window();
    for binding in page_host_dom_animation_frame_runtime_bindings_v12() {
        if let Some(name) = binding.runtime_binding_id.strip_prefix("global.") {
            assert!(
                profile.declaration_source.contains(&format!("    {name}:"))
                    || profile.declaration_source.contains(&format!("    {name}(")),
                "{name}"
            );
        }
    }
}

#[test]
fn absent_intermediate_host_members_reject_every_following_chain() {
    let ambient = PageHostDocumentTypingsV1::generate_dom_window()
        .verified_dom_window_ambient_module(&page_host_dom_window_runtime_bindings_v13())
        .unwrap();
    let mut accepted = Vec::new();
    for source in [
        "window.console.log('missing');",
        "window['console'].log('missing');",
        "self.console?.log('missing');",
        "window.document.missing.deep();",
        "const value = window.console.deep.value;",
        "function accept(value: unknown): void {} accept(window.console.deep);",
        "window['fetch']('https://example.test');",
        "const unavailable = window.document || window.console.deep.value;",
    ] {
        if accepts(ambient.clone(), source) {
            accepted.push(source);
        }
    }
    assert!(
        accepted.is_empty(),
        "absent host members accepted: {accepted:?}"
    );
}
