// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Conditional candidates retain parameter direction until pattern validation.

use super::*;

#[derive(Clone, Default)]
struct Candidates {
    outputs: Vec<Type>,
    inputs: Vec<Type>,
}

#[derive(Clone, Default)]
pub(in crate::checker::type_operators) struct MatchBindings {
    pub(super) values: BTreeMap<String, Type>,
    candidates: BTreeMap<String, Candidates>,
    defaults: BTreeMap<String, Type>,
    pub(super) contravariant: bool,
    bivariant: bool,
}

impl std::ops::Deref for MatchBindings {
    type Target = BTreeMap<String, Type>;
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl MatchBindings {
    pub(in crate::checker::type_operators) fn record(
        &mut self,
        name: &str,
        actual: &Type,
        aliases: &BTreeMap<String, TypeDefinition>,
        budget: &mut TypeExpansionBudget,
    ) -> bool {
        if !budget.consume() {
            return false;
        }
        let candidates = self.candidates.entry(name.to_string()).or_default();
        let values = if self.contravariant && !self.bivariant {
            &mut candidates.inputs
        } else {
            &mut candidates.outputs
        };
        for value in values.iter() {
            if !budget.consume() {
                return false;
            }
            if value == actual {
                return true;
            }
        }
        values.push(actual.clone());
        let value = select(candidates, aliases, budget);
        self.values.insert(name.to_string(), value);
        !budget.exhausted
    }

    pub(super) fn parameter(
        &mut self,
        actual: &Type,
        pattern: &Type,
        aliases: &BTreeMap<String, TypeDefinition>,
        budget: &mut TypeExpansionBudget,
    ) -> bool {
        self.contravariant = !self.contravariant;
        let result = super::matches(actual, pattern, self, aliases, budget);
        self.contravariant = !self.contravariant;
        result
    }

    pub(super) fn field(
        &mut self,
        actual: &Type,
        pattern: &Type,
        method: bool,
        aliases: &BTreeMap<String, TypeDefinition>,
        budget: &mut TypeExpansionBudget,
    ) -> bool {
        let saved = self.bivariant;
        self.bivariant |= method;
        let result = super::matches(actual, pattern, self, aliases, budget);
        self.bivariant = saved;
        result
    }

    pub(super) fn missing_rest(&mut self, name: &str) {
        self.defaults
            .insert(name.to_string(), Type::Array(Box::new(Type::StrictUnknown)));
    }

    pub(super) fn finish(
        &mut self,
        parameters: &[TypeParameter],
        aliases: &BTreeMap<String, TypeDefinition>,
        budget: &mut TypeExpansionBudget,
    ) {
        for parameter in parameters {
            if !budget.consume() {
                return;
            }
            let constraint = parameter
                .constraint
                .as_ref()
                .map(|value| substitute_type(value, &self.values));
            let mut value = self
                .candidates
                .get(&parameter.name)
                .map(|candidates| select(candidates, aliases, budget))
                .or_else(|| constraint.clone())
                .or_else(|| self.defaults.get(&parameter.name).cloned())
                .unwrap_or(Type::StrictUnknown);
            if let Some(constraint) = constraint {
                if !is_assignable(&value, &constraint, aliases, &mut HashSet::new(), budget) {
                    value = constraint;
                }
            }
            self.values.insert(parameter.name.clone(), value);
        }
    }
}

fn select(
    candidates: &Candidates,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    if !candidates.outputs.is_empty() {
        return union(candidates.outputs.iter().cloned());
    }
    let mut values = candidates.inputs.iter();
    let Some(first) = values.next() else {
        return Type::StrictUnknown;
    };
    values.fold(first.clone(), |result, value| {
        intersect(&result, value, aliases, budget)
    })
}

fn scalar(value: &Type) -> bool {
    matches!(
        value,
        Type::String
            | Type::Number
            | Type::Boolean
            | Type::BigInt
            | Type::Symbol
            | Type::Literal(_)
            | Type::StringLiteral(_)
            | Type::UniqueSymbol(_)
            | Type::Null
            | Type::Undefined
    )
}

fn intersect(
    left: &Type,
    right: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    if !budget.consume() || left == right {
        return left.clone();
    }
    if *left == Type::Any || *right == Type::Any {
        return Type::Any;
    }
    if *left == Type::Never || *right == Type::Never {
        return Type::Never;
    }
    if *left == Type::StrictUnknown {
        return right.clone();
    }
    if *right == Type::StrictUnknown {
        return left.clone();
    }
    if let Type::Union(parts) = left {
        return union(
            parts
                .iter()
                .map(|part| intersect(part, right, aliases, budget)),
        );
    }
    if let Type::Union(parts) = right {
        return union(
            parts
                .iter()
                .map(|part| intersect(left, part, aliases, budget)),
        );
    }
    if scalar(left) && scalar(right) {
        if is_assignable(left, right, aliases, &mut HashSet::new(), budget) {
            return left.clone();
        }
        if is_assignable(right, left, aliases, &mut HashSet::new(), budget) {
            return right.clone();
        }
        return Type::Never;
    }
    let mut parts = Vec::new();
    for value in [left, right] {
        let items = match value {
            Type::Intersection(parts) => parts.as_slice(),
            value => std::slice::from_ref(value),
        };
        for item in items {
            if !budget.consume() {
                return Type::Intersection(parts);
            }
            if !parts.contains(item) {
                parts.push(item.clone());
            }
        }
    }
    Type::Intersection(parts)
}
