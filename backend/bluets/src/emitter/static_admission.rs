// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pure subset admission: no lowering, executable artifact or host access.

use super::*;
use crate::ModuleKind;

pub(crate) fn semantics(project: &Project, options: &CompilerOptions) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (id, module) in &project.modules {
        if is_declaration_module(id) || crate::is_external_library_module(id) {
            continue;
        }
        let mut file_options = options.clone();
        file_options.module_kind = project.module_kind(id, options.module_kind);
        diagnostics.extend(helper_selection::validate_static(
            module,
            project,
            &file_options,
        ));
        if file_options.module_kind == ModuleKind::System {
            if let Err(diagnostic) = system::validate_semantics(module) {
                diagnostics.push(diagnostic);
            }
        }
    }
    diagnostics
}

pub(crate) fn validate(project: &Project) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (id, module) in &project.modules {
        if is_declaration_module(id) || crate::is_external_library_module(id) {
            continue;
        }
        let classes = runtime_declarations(&module.declarations)
            .into_iter()
            .filter_map(|(declaration, _)| match declaration {
                Declaration::Class(class) => Some(class),
                _ => None,
            })
            .chain(
                module
                    .class_expressions
                    .values()
                    .map(|expression| &expression.class),
            );
        for class in classes {
            for constructor in class
                .members
                .iter()
                .filter_map(|member| member.constructor.as_ref())
            {
                if constructor.body.is_some()
                    && !constructor.parameter_properties.is_empty()
                    && constructor.prologue_insertion.is_none()
                {
                    diagnostics.push(class_lowering::missing_super(constructor.span.clone()));
                }
            }
        }
    }
    diagnostics
}
