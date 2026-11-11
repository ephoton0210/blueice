// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explain retained generic signatures and rejected callable union members.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn callable_type_detail(&self, actual: &Type, expected: &Type) -> Option<String> {
        let Type::Function {
            parameters: source,
            result: source_result,
        } = actual
        else {
            return None;
        };
        if let Type::GenericFunction {
            parameters, result, ..
        } = expected
        {
            let positional = |parameters: &[Parameter]| {
                parameters
                    .iter()
                    .filter(|p| p.name != "this")
                    .cloned()
                    .collect()
            };
            return Some(crate::diagnostic::type_text::detail(
                &Type::Function {
                    parameters: positional(source),
                    result: source_result.clone(),
                },
                &Type::Function {
                    parameters: positional(parameters),
                    result: result.clone(),
                },
                self.project,
            ));
        }
        if let Type::Union(parts) = expected {
            if !parts.is_empty()
                && parts
                    .iter()
                    .all(|part| matches!(part, Type::Function { .. }))
            {
                let target = &parts[0];
                let detail = crate::diagnostic::type_text::detail(actual, target, self.project);
                return Some(format!(
                    "\n  Type '{}' is not assignable to type '{}'.{}",
                    crate::diagnostic::type_text::render_in(actual, self.project),
                    crate::diagnostic::type_text::render_in(target, self.project),
                    detail.replace('\n', "\n  ")
                ));
            }
        }
        None
    }

    pub(super) fn callable_parameter_origin(
        &self,
        actual: &Type,
        expected: &Type,
    ) -> Option<(String, SourceSpan)> {
        let actual = self.compatibility_shape(actual);
        let expected = self.compatibility_shape(expected);
        let (
            Type::Function {
                parameters: source, ..
            },
            Type::GenericFunction {
                type_parameters,
                parameters: target,
                ..
            },
        ) = (&actual, &expected)
        else {
            return None;
        };
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        for (source, target) in source
            .iter()
            .filter(|p| p.name != "this")
            .zip(target.iter().filter(|p| p.name != "this"))
        {
            let source = source.annotation.as_ref()?;
            let target = target.annotation.as_ref()?;
            let Type::Named { name, arguments } = target else {
                continue;
            };
            let parameter = type_parameters.iter().find(|p| {
                arguments.is_empty()
                    && p.constraint.is_none()
                    && (*name == p.name || *name == crate::parser::type_parameter_identity(p))
            });
            if let Some(parameter) = parameter {
                if !is_assignable(
                    target,
                    source,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                ) && !budget.exhausted
                {
                    return Some((
                        crate::diagnostic::type_text::render_in(source, self.project),
                        SourceSpan::new(
                            &parameter.span.module,
                            parameter.span.start,
                            parameter.span.start + parameter.name.len(),
                        ),
                    ));
                }
            }
        }
        None
    }
}
