// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Global class surfaces retain their declaration's generic identity.

use super::*;

pub(super) fn insert_class(
    project: &Project,
    ambient: &mut AmbientDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
    class: &crate::parser::ClassDeclaration,
) {
    let mut surface = module::class_export(class);
    let substitutions = ambient
        .contexts
        .get(&class.span.module)
        .into_iter()
        .flat_map(|context| context.iter())
        .filter(|(name, _)| {
            !surface
                .type_parameters
                .iter()
                .any(|parameter| &parameter.name == *name)
        })
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    surface.instance_type = substitute_type(&surface.instance_type, &substitutions);
    surface.constructor_type = substitute_type(&surface.constructor_type, &substitutions);
    for parameter in &mut surface.type_parameters {
        parameter.constraint = parameter
            .constraint
            .as_ref()
            .map(|value| substitute_type(value, &substitutions));
        parameter.default = parameter
            .default
            .as_ref()
            .map(|value| substitute_type(value, &substitutions));
    }
    for signature in &mut surface.constructor_binding.signatures {
        for parameter in &mut signature.parameters {
            parameter.annotation = parameter
                .annotation
                .as_ref()
                .map(|value| substitute_type(value, &substitutions));
        }
        signature.return_type = substitute_type(&signature.return_type, &substitutions);
    }
    insert_ambient_type(
        project,
        ambient,
        diagnostics,
        &class.name,
        TypeDefinition {
            kind: TypeDefinitionKind::Class,
            parameters: surface.type_parameters.clone(),
            value: surface.instance_type.clone(),
        },
        &class.span,
    );
    insert_ambient_value(
        ambient,
        diagnostics,
        &class.name,
        surface.constructor_type.clone(),
        &class.span,
    );
    ambient.classes.entry(class.name.clone()).or_insert(surface);
}
