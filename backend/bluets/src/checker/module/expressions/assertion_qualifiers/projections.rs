// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A parameter projection retains its origin when asserted back to that binder.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Plain,
    Related,
    Unproven,
    Protected,
}

impl Origin {
    fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Protected, _) | (_, Self::Protected) => Self::Protected,
            (Self::Unproven, _) | (_, Self::Unproven) => Self::Unproven,
            (Self::Related, _) | (_, Self::Related) => Self::Related,
            _ => Self::Plain,
        }
    }
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module::expressions) fn assertion_retains_parameter_projection(
        &self,
        tokens: &[Token],
        actual: &Type,
        target: &Type,
        scope: &BTreeMap<String, Type>,
    ) -> bool {
        let target = canonical_parameter_references(target, &self.types);
        let Type::Named { name, arguments } = &target else {
            return false;
        };
        if !arguments.is_empty()
            || !self
                .types
                .get(name)
                .is_some_and(|definition| definition.kind == TypeDefinitionKind::Parameter)
        {
            return false;
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let origin = self.assertion_projection_origin(tokens, actual, scope, name, &mut budget, 0);
        if budget.exhausted {
            if let (Some(first), Some(last)) = (tokens.first(), tokens.last()) {
                self.generic_inference_failure.set(Some(SourceSpan::new(
                    &self.module.id,
                    first.start,
                    last.end,
                )));
            }
        }
        !budget.exhausted && origin == Origin::Related
    }

    fn assertion_projection_origin(
        &self,
        tokens: &[Token],
        actual: &Type,
        scope: &BTreeMap<String, Type>,
        root: &str,
        budget: &mut TypeExpansionBudget,
        depth: usize,
    ) -> Origin {
        if depth >= 128 || !budget.consume() {
            budget.exhausted = true;
            return Origin::Unproven;
        }
        let typed = type_origin(actual, root, &self.types, budget, &mut HashSet::new(), 0);
        if typed != Origin::Unproven || budget.exhausted {
            return typed;
        }
        let mut tokens = strip_outer_parentheses(tokens);
        while let Some(operand) = erased_assertion_operand(tokens) {
            tokens = strip_outer_parentheses(operand);
        }
        if let Some(parts) = Self::literal_elements(tokens) {
            let mut origin = Origin::Plain;
            for part in parts {
                let part = if part.first().is_some_and(|token| token.is("...")) {
                    &part[1..]
                } else {
                    part
                };
                let value = self.infer_expression(part, scope);
                origin = origin.join(self.assertion_projection_origin(
                    part,
                    &value,
                    scope,
                    root,
                    budget,
                    depth + 1,
                ));
                if matches!(origin, Origin::Protected | Origin::Unproven) {
                    return origin;
                }
            }
            return origin;
        }
        // An indexed parameter can be presented as its constraint's unknown
        // element. Inspect its typed receiver before that information was lost.
        // No spelling comparison with a value name supplies this evidence.
        if let Some((receiver, Some(index))) = member_access_target(tokens) {
            if index.parse::<usize>().is_ok() {
                let receiver = self.infer_expression(receiver, scope);
                if same_parameter(&receiver, root, &self.types) {
                    return Origin::Related;
                }
            }
        }
        if let [token] = tokens {
            if let Some(scopes) = self.scopes.as_ref() {
                if let Some(initializer) = scopes.unobserved_const_initializer(token, budget) {
                    if initializer.first().is_some_and(|first| {
                        scopes.flow_execution(first.start) == scopes.flow_execution(token.start)
                    }) {
                        // The binding lookup supplies the initializer of this
                        // exact const, including lexical shadows. An annotated
                        // binding, an observer/escape or a different execution
                        // frame supplies no transparent provenance here.
                        let value = self.infer_expression(&initializer, scope);
                        return self.assertion_projection_origin(
                            &initializer,
                            &value,
                            scope,
                            root,
                            budget,
                            depth + 1,
                        );
                    }
                }
            }
        }
        Origin::Unproven
    }
}

fn same_parameter(value: &Type, root: &str, aliases: &BTreeMap<String, TypeDefinition>) -> bool {
    matches!(canonical_parameter_references(value, aliases), Type::Named {name, arguments}
        if name == root && arguments.is_empty())
}

fn type_origin(
    value: &Type,
    root: &str,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
    visited: &mut HashSet<String>,
    depth: usize,
) -> Origin {
    if depth >= 128 || !budget.consume() {
        budget.exhausted = true;
        return Origin::Unproven;
    }
    if same_parameter(value, root, aliases) {
        return Origin::Related;
    }
    match value {
        Type::Unknown | Type::StrictUnknown => Origin::Unproven,
        Type::Readonly(_) => Origin::Protected,
        Type::IndexedAccess { object, .. } if same_parameter(object, root, aliases) => {
            Origin::Related
        }
        Type::Array(element) => type_origin(element, root, aliases, budget, visited, depth + 1),
        Type::Tuple(elements) => elements.iter().fold(Origin::Plain, |origin, element| {
            origin.join(type_origin(
                &element.annotation,
                root,
                aliases,
                budget,
                &mut visited.clone(),
                depth + 1,
            ))
        }),
        Type::Record(fields) | Type::CallableRecord { fields, .. } => {
            fields.iter().fold(Origin::Plain, |origin, field| {
                if field.readonly {
                    Origin::Protected
                } else {
                    origin.join(type_origin(
                        &field.value,
                        root,
                        aliases,
                        budget,
                        &mut visited.clone(),
                        depth + 1,
                    ))
                }
            })
        }
        Type::Union(parts) | Type::Intersection(parts) => {
            parts.iter().fold(Origin::Plain, |origin, value| {
                origin.join(type_origin(
                    value,
                    root,
                    aliases,
                    budget,
                    &mut visited.clone(),
                    depth + 1,
                ))
            })
        }
        Type::Named { .. }
        | Type::IndexedAccess { .. }
        | Type::Conditional(_)
        | Type::Mapped(_)
        | Type::KeyOf(_)
        | Type::TemplateLiteral(_)
        | Type::InferenceBarrier(_) => {
            match instantiate_named(
                value,
                aliases,
                visited,
                budget,
                "assertion projection origin",
            ) {
                Some(value) => type_origin(&value, root, aliases, budget, visited, depth + 1),
                None => {
                    if qualified(value, aliases, &mut visited.clone(), budget, 0) {
                        Origin::Unproven
                    } else {
                        Origin::Plain
                    }
                }
            }
        }
        _ => Origin::Plain,
    }
}
