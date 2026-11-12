// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Post-inference checking consumes type operations rather than alias names.

use super::*;
mod presence;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    Checking,
    Diagnostic,
    Operand,
}

pub(super) fn alias_has_barrier(name: &str, aliases: &BTreeMap<String, TypeDefinition>) -> bool {
    let mut name = name;
    let mut visited = HashSet::new();
    while visited.insert(name) {
        let Some(definition) = aliases
            .get(name)
            .filter(|definition| definition.kind == TypeDefinitionKind::Alias)
        else {
            return false;
        };
        match &definition.value {
            Type::InferenceBarrier(_) => return true,
            Type::Named { name: next, .. } => name = next,
            _ => return false,
        }
    }
    false
}

pub(in crate::checker) fn checking_type(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    normalize(value, aliases, budget, Form::Checking)
}

/// Reduce inference barriers while preserving ordinary diagnostic identities.
pub(in crate::checker) fn diagnostic_type(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    normalize(value, aliases, budget, Form::Diagnostic)
}

fn operator(value: &Type) -> bool {
    matches!(
        value,
        Type::KeyOf(_)
            | Type::IndexedAccess { .. }
            | Type::Conditional(_)
            | Type::Mapped(_)
            | Type::TemplateLiteral(_)
    )
}

fn normalize(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
    form: Form,
) -> Type {
    if budget.exhausted {
        return value.clone();
    }
    if let Type::InferenceBarrier(base) = value {
        if budget.consume() {
            let base = normalize(base, aliases, budget, form);
            if form != Form::Diagnostic {
                return base;
            }
            let checked = normalize(&base, aliases, budget, Form::Operand);
            let displayed =
                super::super::type_operators::diagnostic_type(&checked, aliases, budget);
            // Native retains NoInfer around object types, but scalar operator
            // results shed the wrapper while keeping names such as keyof Events.
            let semantic = if operator(&displayed) {
                super::super::type_operators::expanded(
                    &displayed,
                    aliases,
                    &mut HashSet::new(),
                    budget,
                )
            } else {
                displayed.clone()
            };
            return if matches!(Type::inference_barrier(semantic), Type::InferenceBarrier(_)) {
                Type::inference_barrier(base)
            } else {
                displayed
            };
        }
        return value.clone();
    }
    if matches!(value, Type::Named {name, ..} if alias_has_barrier(name, aliases)) {
        if let Some(expanded) = instantiate_named(
            value,
            aliases,
            &mut HashSet::new(),
            budget,
            "checked inference barrier",
        ) {
            if expanded != *value {
                return normalize(&expanded, aliases, budget, form);
            }
        }
    }
    if form == Form::Diagnostic
        && matches!(value, Type::Named {name, ..}
        if aliases.get(name).is_some_and(|definition| definition.kind == TypeDefinitionKind::Alias && operator(&definition.value)))
    {
        if let Some(expanded) = instantiate_named(
            value,
            aliases,
            &mut HashSet::new(),
            budget,
            "diagnostic operator alias",
        ) {
            if expanded != *value && presence::contains(&expanded, aliases, budget) {
                return normalize(&expanded, aliases, budget, form);
            }
        }
    }
    // Conditional patterns use barriers while matching infer positions. An
    // operand view must keep that operation intact for the conditional resolver.
    if form != Form::Diagnostic && matches!(value, Type::Conditional(_)) {
        return value.clone();
    }
    let mapped = value.map_children(|child, _| normalize(child, aliases, budget, form));
    if mapped != *value && operator(&mapped) {
        if form == Form::Diagnostic {
            let checked = normalize(&mapped, aliases, budget, Form::Operand);
            super::super::type_operators::diagnostic_type(&checked, aliases, budget)
        } else if form == Form::Checking {
            super::results::finish(mapped, aliases, budget)
        } else {
            mapped
        }
    } else {
        mapped
    }
}
