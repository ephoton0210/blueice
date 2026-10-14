// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Constructor assignment reasons use the checker's bounded instantiation.

use super::*;
use crate::checker::type_relations::{constructor_relation_type, record_fields_assignable};
use crate::diagnostic::type_text;
use crate::parser::TypeSignature;

fn function(signature: &TypeSignature) -> Type {
    if signature.type_parameters.is_empty() {
        Type::Function {
            parameters: signature.parameters.clone(),
            result: Box::new(signature.result.clone()),
        }
    } else {
        signature.function_type()
    }
}

fn spelling(signature: &TypeSignature, project: &Project) -> String {
    format!(
        "{}new {}",
        if signature.abstract_constructor {
            "abstract "
        } else {
            ""
        },
        type_text::render_in(&function(signature), project)
    )
}

impl ModuleChecker<'_> {
    pub(super) fn constructor_assignment_error(
        &mut self,
        span: &SourceSpan,
        message: &str,
        bts_code: DiagnosticCode,
        actual: &Type,
        expected: &Type,
    ) -> bool {
        let (
            Type::CallableRecord {
                fields: af,
                signatures: ac,
            },
            Type::CallableRecord {
                fields: ef,
                signatures: ec,
            },
        ) = (
            self.compatibility_shape(actual),
            self.compatibility_shape(expected),
        )
        else {
            return false;
        };
        let ([source], [target]) = (ac.as_slice(), ec.as_slice()) else {
            return false;
        };
        if !source.construct
            || !target.construct
            || source.abstract_constructor && !target.abstract_constructor
            || source.constructor_visibility != target.constructor_visibility
        {
            return false;
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        if !record_fields_assignable(
            &af,
            &ef,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
            false,
        ) {
            return false;
        }
        let (instantiated, valid) =
            constructor_relation_type(source, target, &self.types, &mut budget);
        let target_function = function(target);
        if valid
            && is_assignable(
                &instantiated,
                &target_function,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            )
        {
            return false;
        }
        if budget.exhausted {
            return false;
        }
        let (source_parameters, source_result) = match &instantiated {
            Type::Function { parameters, result }
            | Type::GenericFunction {
                parameters, result, ..
            } => (parameters, result),
            _ => return false,
        };
        let parameters = |parameters: &[Parameter]| Type::Function {
            parameters: parameters.to_vec(),
            result: Box::new(Type::Void),
        };
        let parameters_match = valid
            && is_assignable(
                &parameters(source_parameters),
                &parameters(&target.parameters),
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            );
        if budget.exhausted {
            return false;
        }
        // Concrete object construct signatures use the ordinary parameter
        // cause; generic source instantiation retains the construct signature
        // comparison above that cause.
        if !parameters_match && source.type_parameters.is_empty() {
            return false;
        }
        let pair = format!(
            "Type '{}' is not assignable to type '{}'.",
            spelling(source, self.project),
            spelling(target, self.project)
        );
        let (code, args, detail) = if parameters_match {
            let mut detail = format!(
                "\n  {pair}\n    Construct signature return types '{}' and '{}' are incompatible.",
                type_text::render_in(source_result, self.project),
                type_text::render_in(&target.result, self.project)
            );
            let (Some(source_fields), false) =
                self.expanded_record_fields((**source_result).clone())
            else {
                return false;
            };
            let (Some(target_fields), false) = self.expanded_record_fields(target.result.clone())
            else {
                return false;
            };
            for field in &target_fields {
                if let Some(actual_field) = source_fields
                    .iter()
                    .find(|actual| actual.name == field.name)
                {
                    if !is_assignable(
                        &actual_field.value,
                        &field.value,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) {
                        detail.push_str(&format!("\n      The types of '{}' are incompatible between these types.\n        Type '{}' is not assignable to type '{}'.", field.name, type_text::render_in(&actual_field.value, self.project), type_text::render_in(&field.value, self.project)));
                        detail.push_str(
                            &type_text::detail(&actual_field.value, &field.value, self.project)
                                .replace('\n', "\n        "),
                        );
                        break;
                    }
                }
            }
            if budget.exhausted {
                return false;
            }
            (2419, Vec::new(), detail)
        } else {
            let cause = type_text::detail(
                &parameters(source_parameters),
                &parameters(&target.parameters),
                self.project,
            )
            .replace('\n', "\n    ");
            (
                2322,
                vec![
                    type_text::render_in(actual, self.project),
                    type_text::render_in(expected, self.project),
                ],
                format!("\n  Types of construct signatures are incompatible.\n    {pair}{cause}"),
            )
        };
        self.typescript_type_error(span, message.into(), bts_code, code, args);
        if let Some(counterpart) = self
            .diagnostics
            .last_mut()
            .and_then(|diagnostic| diagnostic.typescript.as_mut())
        {
            counterpart.message.push_str(&detail);
        }
        true
    }
}
