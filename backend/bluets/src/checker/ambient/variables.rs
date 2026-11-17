// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared variable declarations preserve the first program declaration's type.

use super::*;

pub(super) fn insert_variable(
    project: &Project,
    ambient: &mut AmbientDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
    variable: &crate::VariableDeclaration,
    value: Type,
) {
    let name = &variable.name;
    if ambient.conflicting_names.contains(name) && ambient.values.contains_key(name) {
        return;
    }
    if let Some((kind, span)) = ambient.variable_declarations.get(name) {
        if *kind == crate::VariableKind::Var && variable.kind == crate::VariableKind::Var {
            let previous = &ambient.values[name];
            if *previous != value {
                let position = global_declarations::name_span(project, &variable.span, name);
                let mut diagnostic = Diagnostic::error(
                    DiagnosticCode::TypeMismatch,
                    position,
                    format!("subsequent global variable `{name}` has a different type"),
                )
                .with_typescript(
                    2403,
                    vec![
                        name.clone(),
                        crate::diagnostic::type_text::render_in(previous, project),
                        crate::diagnostic::type_text::render_in(&value, project),
                    ],
                );
                diagnostic
                    .typescript
                    .as_mut()
                    .expect("Native variable merge template")
                    .related_information
                    .push(crate::TypeScriptRelatedInformation {
                        code: 6203,
                        message: format!("'{name}' was also declared here."),
                        span: global_declarations::name_span(project, span, name),
                        position: None,
                    });
                diagnostics.push(diagnostic);
            }
            return;
        }
    }
    insert_ambient_value(ambient, diagnostics, name, value, &variable.span);
    ambient
        .variable_declarations
        .entry(name.clone())
        .or_insert((variable.kind, variable.span.clone()));
}
