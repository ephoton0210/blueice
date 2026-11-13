// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explain rejected symbolic key relations from constraints, not source names.
use super::*;

fn parameter(value: &Type, aliases: &BTreeMap<String, TypeDefinition>) -> Option<String> {
    let Type::Named { name, arguments } = canonical_parameter_references(value, aliases) else {
        return None;
    };
    (arguments.is_empty()
        && aliases
            .get(&name)
            .is_some_and(|d| d.kind == TypeDefinitionKind::Parameter))
    .then_some(name)
}

fn has_keys(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
    parameters: bool,
) -> bool {
    let mut pending = vec![value.clone()];
    let mut visited = HashSet::new();
    while let Some(value) = pending.pop() {
        if !budget.consume() {
            return false;
        }
        match value {
            Type::KeyOf(object) if parameter(&object, aliases).is_some() => return true,
            Type::Union(parts) | Type::Intersection(parts) => pending.extend(parts),
            Type::Named { name, .. } if visited.insert(name.clone()) => {
                if let Some(definition) = aliases.get(&name).filter(|d| {
                    d.kind == TypeDefinitionKind::Alias
                        || parameters && d.kind == TypeDefinitionKind::Parameter
                }) {
                    pending.push(definition.value.clone());
                }
            }
            _ => {}
        }
    }
    false
}

fn domain() -> Type {
    Type::Union(vec![Type::String, Type::Number, Type::Symbol])
}

// A relation displays both sides using the same deferred-key allocation order.
// Visiting the source first retains keys allocated while projecting its object
// branches; the target can introduce further keys, but cannot reorder old ones.
fn order_key_unions(value: &mut Type, order: &mut Vec<Type>, budget: &mut TypeExpansionBudget) {
    if !budget.consume() {
        return;
    }
    let union = matches!(value, Type::Union(_));
    match value {
        Type::KeyOf(_) if !order.contains(value) => order.push(value.clone()),
        Type::Union(parts) | Type::Intersection(parts) => {
            for part in parts.iter_mut() {
                order_key_unions(part, order, budget);
            }
            if union && parts.iter().all(|part| matches!(part, Type::KeyOf(_))) {
                parts.sort_by_key(|part| order.iter().position(|known| known == part));
            }
        }
        _ => {}
    }
}

// Enumerate the disjunctive key witnesses of an intersection. This is only a
// diagnostic view: admission still uses the ordinary structural relation, and
// every cross-product branch consumes the same configured expansion budget.
fn key_witnesses(value: &Type, budget: &mut TypeExpansionBudget) -> Option<Vec<Vec<Type>>> {
    if !budget.consume() {
        return None;
    }
    match value {
        Type::KeyOf(_) => Some(vec![vec![value.clone()]]),
        Type::Union(parts) => {
            let mut branches = Vec::new();
            for part in parts {
                branches.extend(key_witnesses(part, budget)?);
            }
            Some(branches)
        }
        Type::Intersection(parts) => {
            let mut branches = vec![Vec::new()];
            for part in parts {
                let options = key_witnesses(part, budget)?;
                let mut next = Vec::new();
                for branch in branches {
                    for option in &options {
                        if !budget.consume() {
                            return None;
                        }
                        let mut product = branch.clone();
                        for key in option {
                            if !product.contains(key) {
                                product.push(key.clone());
                            }
                        }
                        next.push(product);
                    }
                }
                branches = next;
            }
            Some(branches)
        }
        _ => None,
    }
}

fn upper_bound(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
    active: &mut HashSet<String>,
) -> Type {
    if !budget.consume() {
        return value.clone();
    }
    match value {
        Type::KeyOf(object) if parameter(object, aliases).is_some() => domain(),
        Type::Named { .. } if parameter(value, aliases).is_some() => {
            let name = parameter(value, aliases).unwrap();
            if !active.insert(name.clone()) {
                return value.clone();
            }
            upper_bound(&aliases[&name].value, aliases, budget, active)
        }
        Type::Union(parts) => crate::checker::type_operators::union(
            parts
                .iter()
                .map(|part| upper_bound(part, aliases, budget, &mut active.clone())),
        ),
        Type::Intersection(parts) => {
            let values = parts
                .iter()
                .map(|part| upper_bound(part, aliases, budget, &mut active.clone()))
                .collect::<Vec<_>>();
            if values.iter().all(|part| *part == domain()) {
                domain()
            } else {
                Type::Intersection(values)
            }
        }
        _ => value.clone(),
    }
}

impl ModuleChecker<'_> {
    pub(super) fn key_diagnostic_types(
        &self,
        actual: &Type,
        expected: &Type,
    ) -> Option<(Type, Type)> {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let display = |value: &Type, budget: &mut TypeExpansionBudget| {
            if parameter(value, &self.types).is_some() {
                value.clone()
            } else {
                crate::checker::type_operators::diagnostic_type(value, &self.types, budget)
            }
        };
        let mut actual = display(actual, &mut budget);
        let mut expected = display(expected, &mut budget);
        let target_keys = has_keys(&expected, &self.types, &mut budget, false);
        let source_keys = has_keys(&actual, &self.types, &mut budget, true);
        if !target_keys && !source_keys {
            return None;
        }
        let mut order = Vec::new();
        order_key_unions(&mut actual, &mut order, &mut budget);
        order_key_unions(&mut expected, &mut order, &mut budget);
        if !target_keys
            && !crate::checker::inference::depends_on_parameter(&expected, &self.types, &mut budget)
        {
            actual = upper_bound(&actual, &self.types, &mut budget, &mut HashSet::new());
        } else if target_keys {
            actual = match actual {
                Type::StringLiteral(_) => Type::String,
                Type::Literal(ref text) if text.starts_with(['\'', '"']) => Type::String,
                Type::Literal(ref text) if matches!(text.as_str(), "true" | "false") => {
                    Type::Boolean
                }
                Type::Literal(ref text) if text.parse::<f64>().is_ok() => Type::Number,
                value => value,
            };
        }
        self.key_reason_budget(&budget);
        (!budget.exhausted).then_some((actual, expected))
    }

    fn key_reason_budget(&self, budget: &TypeExpansionBudget) {
        if budget.exhausted {
            self.generic_inference_failure.set(Some(SourceSpan::new(
                &self.module.id,
                0,
                self.module.source.len(),
            )));
        }
    }

    fn key_failure_step(
        &self,
        actual: &Type,
        expected: &Type,
        budget: &mut TypeExpansionBudget,
        aliases_seen: &mut HashSet<String>,
    ) -> Option<Type> {
        if !budget.consume() {
            return None;
        }
        if let Some(name) = parameter(actual, &self.types) {
            let constraint = self.types[&name].value.clone();
            return (!matches!(constraint, Type::Unknown | Type::StrictUnknown))
                .then_some(constraint);
        }
        match actual {
            Type::KeyOf(object) if parameter(object, &self.types).is_some() => Some(domain()),
            Type::Union(parts) => parts
                .iter()
                .find(|part| {
                    !is_assignable(part, expected, &self.types, &mut HashSet::new(), budget)
                })
                .cloned(),
            Type::Intersection(_) => key_witnesses(actual, budget)?
                .into_iter()
                .map(|mut parts| {
                    if parts.len() == 1 {
                        parts.pop().unwrap()
                    } else {
                        Type::Intersection(parts)
                    }
                })
                .find(|part| {
                    part != actual
                        && !is_assignable(part, expected, &self.types, &mut HashSet::new(), budget)
                }),
            Type::Named { name, .. }
                if self
                    .types
                    .get(name)
                    .is_some_and(|d| d.kind == TypeDefinitionKind::Alias)
                    && aliases_seen.insert(name.clone()) =>
            {
                let expanded = instantiate_named(
                    actual,
                    &self.types,
                    &mut HashSet::new(),
                    budget,
                    "key diagnostic alias",
                )?;
                self.key_failure_step(&expanded, expected, budget, aliases_seen)
            }
            _ => None,
        }
    }

    fn key_reason_line(&self, output: &mut String, actual: &Type, expected: &Type, depth: usize) {
        let actual_text = crate::diagnostic::type_text::render_in(actual, self.project);
        let target = crate::diagnostic::type_text::render_in(expected, self.project);
        output.push_str(&format!(
            "\n{}Type '{actual_text}' is not assignable to type '{target}'.",
            "  ".repeat(depth)
        ));
        if actual_text == target && type_identity(actual) != type_identity(expected) {
            output.push_str(" Two different types with this name exist, but they are unrelated.");
        }
    }

    fn key_failure_path(
        &self,
        start: &Type,
        expected: &Type,
        depth: &mut usize,
        budget: &mut TypeExpansionBudget,
        output: &mut String,
        ancestors: &mut Vec<Type>,
    ) -> Type {
        let mut value = start.clone();
        let mut active = HashSet::new();
        while active.insert(type_identity(&value)) {
            if parameter(&value, &self.types).is_some() || matches!(value, Type::KeyOf(_)) {
                ancestors.push(value.clone());
            }
            let Some(next) = self.key_failure_step(&value, expected, budget, &mut HashSet::new())
            else {
                break;
            };
            if next == value {
                break;
            }
            *depth += 1;
            self.key_reason_line(output, &next, expected, *depth);
            value = next;
            if budget.exhausted {
                break;
            }
        }
        value
    }

    pub(super) fn symbolic_key_detail(&self, actual: &Type, expected: &Type) -> Option<String> {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        if !has_keys(expected, &self.types, &mut budget, false) {
            return None;
        }
        let mut output = String::new();
        let mut depth = 0;
        let mut ancestors = Vec::new();
        let leaf = self.key_failure_path(
            actual,
            expected,
            &mut depth,
            &mut budget,
            &mut output,
            &mut ancestors,
        );
        if let Type::KeyOf(object) = expected {
            if let Some(name) = parameter(object, &self.types) {
                let constraint = &self.types[&name].value;
                if !matches!(constraint, Type::Unknown | Type::StrictUnknown) {
                    let minimum = crate::checker::type_operators::expanded(
                        &Type::KeyOf(Box::new(constraint.clone())),
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    );
                    if minimum != Type::Never
                        && !has_keys(&minimum, &self.types, &mut budget, false)
                        && !is_assignable(
                            &leaf,
                            &minimum,
                            &self.types,
                            &mut HashSet::new(),
                            &mut budget,
                        )
                    {
                        depth += 1;
                        self.key_reason_line(&mut output, &leaf, &minimum, depth);
                        for ancestor in ancestors.into_iter().rev() {
                            depth += 1;
                            self.key_reason_line(&mut output, &ancestor, &minimum, depth);
                            self.key_failure_path(
                                &ancestor,
                                &minimum,
                                &mut depth,
                                &mut budget,
                                &mut output,
                                &mut Vec::new(),
                            );
                        }
                    }
                }
            }
        }
        self.key_reason_budget(&budget);
        (!budget.exhausted).then_some(output)
    }
}
