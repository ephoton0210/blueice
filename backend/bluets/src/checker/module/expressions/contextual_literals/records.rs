// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A primitive-constrained binder preserves fresh nested property candidates.

use super::*;

fn primitive_constraint(value: &Type) -> bool {
    match value {
        Type::String
        | Type::Number
        | Type::Boolean
        | Type::BigInt
        | Type::Symbol
        | Type::Literal(_) => true,
        Type::Union(parts) => parts.iter().any(primitive_constraint),
        _ => false,
    }
}

fn preserve(
    template: &Type,
    actual: &Type,
    fresh: &Type,
    signature: &FunctionSignature,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
    visited: &mut HashSet<String>,
) -> Option<Type> {
    if !budget.consume() || matches!(template, Type::InferenceBarrier(_)) {
        return None;
    }
    if let Type::Named { name, arguments } = template {
        if arguments.is_empty() {
            if let Some(constraint) = signature
                .type_parameters
                .iter()
                .find(|parameter| parameter.name == *name)
                .and_then(|parameter| parameter.constraint.as_ref())
                .filter(|constraint| primitive_constraint(constraint))
            {
                // The const view supplies candidate values only. It must not
                // turn an ordinary writable object into a readonly object.
                let value = if matches!(fresh, Type::Literal(_))
                    && is_assignable(fresh, constraint, aliases, &mut HashSet::new(), budget)
                {
                    fresh.clone()
                } else {
                    actual.clone()
                };
                return Some(value);
            }
        }
        let expanded = instantiate_named(
            template,
            aliases,
            visited,
            budget,
            "nested property candidate",
        )?;
        return preserve(
            &expanded, actual, fresh, signature, aliases, budget, visited,
        );
    }
    let Type::Record(template) = template else {
        return None;
    };
    let (Type::Record(actual), Type::Record(fresh)) = (actual, fresh) else {
        return None;
    };
    let mut fields = actual.clone();
    let mut contextual = false;
    for field in &mut fields {
        let Some(expected) = template.iter().find(|value| value.name == field.name) else {
            continue;
        };
        let Some(fresh) = fresh.iter().find(|value| value.name == field.name) else {
            continue;
        };
        if let Some(value) = preserve(
            &expected.value,
            &field.value,
            &fresh.value,
            signature,
            aliases,
            budget,
            &mut visited.clone(),
        ) {
            contextual = true;
            field.value = value;
        }
    }
    contextual.then_some(Type::Record(fields))
}

impl ModuleChecker<'_> {
    pub(super) fn constrained_property_literals(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        parameter: &Parameter,
        signature: &FunctionSignature,
    ) -> Option<Type> {
        if !signature.type_parameters.iter().any(|parameter| {
            parameter
                .constraint
                .as_ref()
                .is_some_and(primitive_constraint)
        }) {
            return None;
        }
        let template = parameter.annotation.as_ref()?;
        let actual = self.infer_expression(tokens, scope);
        let fresh = self.infer_const_assertion(tokens, scope);
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let result = preserve(
            template,
            &actual,
            &fresh,
            signature,
            &self.types,
            &mut budget,
            &mut HashSet::new(),
        );
        if budget.exhausted {
            if self.enforce_types {
                if let (Some(first), Some(last)) = (tokens.first(), tokens.last()) {
                    self.generic_inference_failure.set(Some(SourceSpan::new(
                        &self.module.id,
                        first.start,
                        last.end,
                    )));
                }
            }
            None
        } else {
            result
        }
    }
}
