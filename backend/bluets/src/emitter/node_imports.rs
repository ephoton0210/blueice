// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Checked Node ESM import assignments retain their require-mode dependency.
use super::{javascript_specifier, Declaration, Module, TextEdit};
use crate::{CompilerOptions, JsxMode, ModuleKind};
pub(crate) const SOURCE: &str = include_str!("node_imports.v1.js");

pub(super) fn lower(module: &Module, options: &CompilerOptions, edits: &mut Vec<TextEdit>) {
    if options.module_kind != ModuleKind::Esm {
        return;
    }
    let imports = module
        .declarations
        .iter()
        .filter_map(|declaration| {
            let Declaration::Import(import) = declaration else {
                return None;
            };
            (import.equals_require && !import.is_type_only()).then_some(import)
        })
        .collect::<Vec<_>>();
    if imports.is_empty() {
        return;
    }
    let unused = |base: &str| {
        (0usize..)
            .map(|index| format!("{base}_{index}"))
            .find(|name| !module.source.contains(name))
            .expect("finite source admits a fresh binding")
    };
    let factory = unused("__bluetsNodeCreateRequire");
    let require = unused("__bluetsNodeRequire");
    edits.push(TextEdit {
        start: 0,
        end: 0,
        replacement: SOURCE
            .replace("__bluetsNodeCreateRequire", &factory)
            .replace("__bluetsNodeRequire", &require),
    });
    for import in imports {
        let local = &import.bindings[0].local;
        let specifier =
            javascript_specifier(&import.specifier, options.jsx == Some(JsxMode::Preserve));
        edits.push(TextEdit {
            start: import.span.start,
            end: import.span.end,
            replacement: format!(
                "{}const {local} = {require}({});{}",
                if import.exported { "export " } else { "" },
                serde_json::to_string(&specifier).expect("string specifier"),
                "\n".repeat(
                    module.source[import.span.start..import.span.end]
                        .matches('\n')
                        .count()
                )
            ),
        });
    }
}
