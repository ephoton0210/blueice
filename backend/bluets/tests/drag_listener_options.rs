// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource};

const DECLARATIONS: &str = "interface Click { readonly type: 'click'; } interface Selection { readonly type: 'input' | 'change'; } interface Drag { readonly type: 'drop' | 'dragover'; } type DragCallback = (event: Drag) => void; interface Listener { handleEvent(event: Drag): void; } interface Options { capture?: boolean; once?: boolean; passive?: boolean; } interface Target { listen(kind: 'click', callback: (event: Click) => void): void; listen(kind: 'input' | 'change', callback: (event: Selection) => void): void; listen(kind: 'drop' | 'dragover', callback: DragCallback | Listener | null, options?: boolean | Options): void; } declare const target: Target;";

fn check(source: &str, declarations: &str) -> bool {
    !compile(
        "memory:///drag.ts",
        &MapLoader::from([ModuleSource::new("memory:///drag.ts", source)]),
        CompilerOptions {
            require_declared_global_calls: true,
            ambient_declaration_modules: vec![ModuleSource::new(
                "memory:///drag.d.ts",
                declarations,
            )],
            ..CompilerOptions::default()
        },
    )
    .has_errors()
}

#[test]
fn three_literal_event_groups_select_callbacks_with_optional_listener_options() {
    for source in [
        "function drag(event: Drag): void {} target.listen('drop', drag);",
        "function drag(event: Drag): void {} target.listen('dragover', drag, {capture: true, once: true});",
        "function drag(event: Drag): void {} target.listen('drop', drag, true);",
        "function drag(event: Drag): void {} const listener: Listener = {handleEvent: drag}; target.listen('drop', listener);",
        "target.listen('drop', null, {passive: false});",
        "function click(event: Click): void {} target.listen('click', click);",
        "function selected(event: Selection): void {} target.listen('change', selected);",
    ] {
        assert!(check(source, DECLARATIONS), "{source}");
    }
}

#[test]
fn additional_event_groups_do_not_bypass_callback_option_or_tag_validation() {
    for source in [
        "function drag(event: Drag): void {} target.listen('click', drag);",
        "function click(event: Click): void {} target.listen('drop', click);",
        "function drag(event: Drag): void {} target.listen('unknown', drag);",
        "function drag(event: Drag): void {} target.listen('drop', drag, {capture: 'yes'});",
        "function drag(event: Drag): void {} target.listen('drop', drag, 1);",
        "function drag(event: Drag): void {} target.listen('click', drag, true);",
        "function drag(event: Drag): void {} function mixed(kind: 'drop' | 'click'): void { target.listen(kind, drag); }",
    ] {
        assert!(!check(source, DECLARATIONS), "{source}");
    }
    let overlapping = DECLARATIONS.replace("kind: 'click'", "kind: 'click' | 'drop'");
    // TypeScript 5.9.3 accepts the call through the compatible Drag overload;
    // overlapping event tags do not make an incompatible callback applicable.
    assert!(check(
        "function drag(event: Drag): void {} target.listen('drop', drag);",
        &overlapping,
    ));
}
