// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

const WINDOW: &str = r#"
interface BlueIceWindow {
    readonly window: BlueIceWindow; self: BlueIceWindow;
    readonly top: BlueIceWindow; parent: BlueIceWindow;
}
declare const window: BlueIceWindow;
"#;

pub(super) fn bindings() -> [PageHostDocumentBindingV1; 51] {
    let old = animation_frames::bindings();
    let aliases = [
        ("window.window", "global.window", WINDOW),
        (
            "window.self",
            "global.self",
            "declare let self: BlueIceWindow;",
        ),
        (
            "window.top",
            "global.top",
            "declare const top: BlueIceWindow;",
        ),
        (
            "window.parent",
            "global.parent",
            "declare let parent: BlueIceWindow;",
        ),
    ];
    std::array::from_fn(|index| {
        if index < old.len() {
            old[index]
        } else {
            let (stable_id, runtime_binding_id, declaration) = aliases[index - old.len()];
            PageHostDocumentBindingV1 {
                stable_id,
                declaration,
                role: PageHostBindingRoleV1::Value,
                runtime_binding_id,
                capability: "page-global",
                feature_flag: "top-level-window-aliases",
                first_host_api_version: "blueice-core-script-v18",
            }
        }
    })
}

/// Project the already-declared host values into the Window interface. The
/// public BlueTS parser identifies declarations; their original type/signature
/// text remains authoritative, including overloads, optional and rest inputs.
pub(super) fn member_declaration(bindings: &[PageHostDocumentBindingV1]) -> String {
    use blueice_bluets::Declaration;
    use std::collections::BTreeSet;

    let globals = bindings
        .iter()
        .filter_map(|binding| binding.runtime_binding_id.strip_prefix("global."))
        .collect::<BTreeSet<_>>();
    let source = bindings
        .iter()
        .map(|binding| binding.declaration)
        .collect::<Vec<_>>()
        .join("\n");
    let module = blueice_bluets::parse_module("blueice:///window-members.d.ts", source)
        .expect("fixed host declarations parse before publication");
    let mut declaration = String::from("\ninterface BlueIceWindow {\n");
    let mut installed = BTreeSet::new();
    for item in &module.declarations {
        match item {
            Declaration::Variable(variable)
                if variable.declared && globals.contains(variable.name.as_str()) =>
            {
                let original = &module.source[variable.span.start..variable.span.end];
                let (_, annotation) = original
                    .split_once(':')
                    .expect("host value has an explicit type");
                declaration.push_str(&format!(
                    "    {}: {};\n",
                    variable.name,
                    annotation.trim().trim_end_matches(';')
                ));
                installed.insert(variable.name.as_str());
            }
            Declaration::Function(function)
                if function.declared && globals.contains(function.name.as_str()) =>
            {
                let original = &module.source[function.span.start..function.span.end];
                let signature = original
                    .strip_prefix("declare function ")
                    .expect("fixed host function signature");
                declaration.push_str("    ");
                declaration.push_str(signature.trim().trim_end_matches(';'));
                declaration.push_str(";\n");
                installed.insert(function.name.as_str());
            }
            _ => {}
        }
    }
    assert_eq!(
        installed, globals,
        "every installed host global has a Window member"
    );
    declaration.push_str("}\n");
    declaration
}
