// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original AMD/UMD wrappers around the shared live-binding module lowering.

use super::{javascript_specifier, Declaration, EmittedJavaScript, Module};
use crate::{CompilerOptions, JsxMode, ModuleKind};

pub(crate) const VERSION: &str = "bluets-module-wrappers/1";

pub(super) fn wrap(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
    kind: ModuleKind,
) -> EmittedJavaScript {
    if !matches!(kind, ModuleKind::Amd | ModuleKind::Umd) || !module.is_external_module() {
        return emitted;
    }
    let mut dependencies = vec!["require".to_string(), "exports".to_string()];
    if options.import_helpers
        && options.target == crate::EcmaTarget::Es5
        && module.declarations.iter().any(|declaration| {
            matches!(declaration,
            Declaration::Class(class) if class.extends_name.is_some())
        })
    {
        dependencies.push("tslib".to_string());
    }
    for declaration in &module.declarations {
        let specifier = match declaration {
            Declaration::Import(import) if !import.is_type_only() => &import.specifier,
            Declaration::ValueExport(export)
                if !export.is_type_only() && export.specifier.is_some() =>
            {
                export.specifier.as_ref().unwrap()
            }
            _ => continue,
        };
        let specifier = javascript_specifier(specifier, options.jsx == Some(JsxMode::Preserve));
        if !dependencies.contains(&specifier) {
            dependencies.push(specifier);
        }
    }
    let dependencies = serde_json::to_string(&dependencies).expect("module dependency strings");
    let prefix = match kind {
        ModuleKind::Amd => format!(
            "define({dependencies}, function(require, exports) {{ 'use strict'; var module = {{exports: exports}};\n"
        ),
        ModuleKind::Umd => format!(
            "(function(factory) {{ if (typeof module === 'object' && typeof module.exports === 'object') {{ var value = factory(require, exports); if (value !== undefined) module.exports = value; }} else if (typeof define === 'function' && define.amd) {{ define({dependencies}, factory); }} }})(function(require, exports) {{ 'use strict'; var module = {{exports: exports}};\n"
        ),
        _ => unreachable!(),
    };
    // The wrapper occupies its own line; original source coordinates survive
    // the same shared source-map path as the enclosed target lowering.
    for segment in &mut emitted.provenance {
        segment.generated_line += 1;
    }
    emitted.javascript = format!(
        "{prefix}{}\nreturn module.exports;\n}});",
        emitted.javascript
    );
    emitted
}
