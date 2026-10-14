// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class instances retain declaration identities across same-named method binders.

use super::*;
use crate::parser::type_parameter_identity;

pub(in crate::checker) fn class_definition_parameters(
    class: &ClassDeclaration,
) -> Vec<TypeParameter> {
    let substitutions = class_parameter_substitutions(class);
    class
        .instance_parameters()
        .into_iter()
        .map(|parameter| TypeParameter {
            name: type_parameter_identity(&parameter),
            constraint: parameter
                .constraint
                .as_ref()
                .map(|value| substitute_type(value, &substitutions)),
            default: parameter
                .default
                .as_ref()
                .map(|value| substitute_type(value, &substitutions)),
            ..parameter
        })
        .collect()
}

pub(super) fn class_parameter_substitutions(class: &ClassDeclaration) -> BTreeMap<String, Type> {
    class
        .instance_parameters()
        .iter()
        .map(|parameter| (parameter.name.clone(), parameter_reference(parameter)))
        .collect()
}

pub(super) fn captured_parameter_substitutions(class: &ClassDeclaration) -> BTreeMap<String, Type> {
    class
        .captured_type_parameters
        .iter()
        .map(|parameter| (parameter.name.clone(), parameter_reference(parameter)))
        .collect()
}

pub(super) fn parameter_reference(parameter: &TypeParameter) -> Type {
    Type::Named {
        name: type_parameter_identity(parameter),
        arguments: Vec::new(),
    }
}

pub(in crate::checker) fn formal_class_type(class: &ClassDeclaration, value: &Type) -> Type {
    substitute_type(value, &class_parameter_substitutions(class))
}

pub(in crate::checker::module::binding) fn class_body_this_type(class: &ClassDeclaration) -> Type {
    Type::Named {
        name: class.name.clone(),
        arguments: class
            .instance_parameters()
            .iter()
            .map(parameter_reference)
            .collect(),
    }
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn unrelated_parameter_origin(
        &self,
        actual: &Type,
        expected: &Type,
    ) -> Option<(String, SourceSpan)> {
        let actual = self.bound_parameter_identity(actual)?;
        let expected = self.bound_parameter_identity(expected)?;
        let spelling = crate::parser::source_type_name(&actual);
        if actual == expected || spelling != crate::parser::source_type_name(&expected) {
            return None;
        }
        let parameter = self.bound_parameters.get(&actual)?;
        let origin = SourceSpan::new(
            &parameter.span.module,
            parameter.span.start,
            parameter.span.start + spelling.len(),
        );
        Some((spelling.into(), origin))
    }

    pub(in crate::checker::module) fn parameter_constraint_origin(
        &self,
        actual: &Type,
        expected: &Type,
    ) -> Option<(String, SourceSpan)> {
        if let Some(origin) = self.unrelated_parameter_origin(actual, expected) {
            return Some(origin);
        }
        let identity = self.bound_parameter_identity(actual)?;
        let parameter = self.bound_parameters.get(&identity)?;
        if parameter.constraint.is_some()
            || self.bound_parameter_identity(expected).as_ref() == Some(&identity)
        {
            return None;
        }
        let spelling = crate::parser::source_type_name(&identity);
        Some((
            crate::diagnostic::type_text::render_in(expected, self.project),
            SourceSpan::new(
                &parameter.span.module,
                parameter.span.start,
                parameter.span.start + spelling.len(),
            ),
        ))
    }

    pub(in crate::checker::module) fn bound_parameter_identity(
        &self,
        value: &Type,
    ) -> Option<String> {
        let Type::Named { name, arguments } = value else {
            return None;
        };
        if !arguments.is_empty() {
            return None;
        }
        let definition = self.types.get(name)?;
        match definition.kind {
            TypeDefinitionKind::Parameter => Some(name.clone()),
            TypeDefinitionKind::Alias => match &definition.value {
                Type::Named { name, arguments }
                    if arguments.is_empty()
                        && self.types.get(name).is_some_and(|definition| {
                            definition.kind == TypeDefinitionKind::Parameter
                        }) =>
                {
                    Some(name.clone())
                }
                _ => None,
            },
            _ => None,
        }
    }
}
