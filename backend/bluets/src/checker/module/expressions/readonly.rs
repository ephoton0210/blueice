// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded possible-receiver expansion for readonly mutation checks only.
//! Ordinary expression inference must not pretend a heterogeneous indexed
//! value has one definite type; this path asks whether *any* reachable value
//! has a readonly property that a write could touch.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn possible_readonly_computed_receiver(
        &self,
        receiver: &[Token],
        property: Option<&str>,
        scope: &BTreeMap<String, Type>,
    ) -> Result<bool, ()> {
        if receiver.iter().any(|token| token.is("[")) {
            may_mutate_readonly(self, receiver, property, scope)
        } else {
            Ok(false)
        }
    }

    /// Fail-closed policy for receivers inference cannot model. When the
    /// receiver's type is lost (`Unknown`) while a readonly-bearing binding is
    /// in scope, the write may reach that value through an unmodeled form, so
    /// it cannot be proven safe. Declared `any` stays an explicit opt-out.
    pub(super) fn opaque_receiver_may_reach_readonly(
        &self,
        receiver: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Result<bool, ()> {
        if self.infer_expression(receiver, scope) != Type::Unknown {
            return Ok(false);
        }
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        for value in scope.values() {
            if deep_contains_readonly(value, &self.types, &mut budget, 0)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn reject_opaque_readonly_receiver(
        &mut self,
        receiver: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) -> bool {
        match self.opaque_receiver_may_reach_readonly(receiver, scope) {
            Ok(false) => return false,
            Ok(true) => self.type_error(
                span,
                "cannot prove a write through an unmodeled receiver avoids readonly members".into(),
                DiagnosticCode::TypeMismatch,
            ),
            Err(()) => self.type_error(
                span,
                "opaque readonly receiver exceeds its type-expansion limit".into(),
                DiagnosticCode::ResourceLimit,
            ),
        }
        true
    }

    pub(super) fn reject_computed_readonly_mutation(
        &mut self,
        receiver: &[Token],
        property: Option<&str>,
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) -> bool {
        match self.possible_readonly_computed_receiver(receiver, property, scope) {
            Ok(true) => self.type_error(
                span,
                format!(
                    "cannot mutate readonly property{} through a computed receiver",
                    property.map_or(String::new(), |name| format!(" `{name}`"))
                ),
                DiagnosticCode::TypeMismatch,
            ),
            Err(()) => self.type_error(
                span,
                "computed readonly receiver exceeds its type-expansion limit".into(),
                DiagnosticCode::ResourceLimit,
            ),
            Ok(false) => return false,
        }
        true
    }
}

fn may_mutate_readonly(
    checker: &ModuleChecker<'_>,
    receiver: &[Token],
    property: Option<&str>,
    scope: &BTreeMap<String, Type>,
) -> Result<bool, ()> {
    let mut budget = TypeExpansionBudget::new(checker.max_type_expansions);
    let owners = possible_types(checker, receiver, scope, &mut budget)?;
    let mut concrete = Vec::new();
    for owner in owners {
        expand_final_owner(
            checker,
            owner,
            &mut concrete,
            &mut budget,
            &mut HashSet::new(),
        )?;
    }
    for owner in concrete {
        let mut visited = HashSet::new();
        if let Some(property) = property {
            match property_type(&owner, property, &checker.types, &mut visited, &mut budget) {
                PropertyType::Found { readonly: true, .. } => return Ok(true),
                PropertyType::Exhausted => return Err(()),
                PropertyType::Found { .. }
                | PropertyType::Missing
                | PropertyType::Indeterminate => {}
            }
        } else if contains_readonly_member(&owner, &checker.types, &mut visited, &mut budget)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn expand_final_owner(
    checker: &ModuleChecker<'_>,
    owner: Type,
    result: &mut Vec<Type>,
    budget: &mut TypeExpansionBudget,
    visited: &mut HashSet<String>,
) -> Result<(), ()> {
    match owner {
        Type::Union(parts) => {
            for part in parts {
                expand_final_owner(checker, part, result, budget, &mut visited.clone())?;
            }
        }
        Type::Named { .. } => {
            match instantiate_named(
                &owner,
                &checker.types,
                visited,
                budget,
                "readonly final owner",
            ) {
                Some(value) => expand_final_owner(checker, value, result, budget, visited)?,
                None if budget.exhausted => return Err(()),
                None => push_type(Type::Unknown, result, budget)?,
            }
        }
        owner => push_type(owner, result, budget)?,
    }
    Ok(())
}

fn possible_types(
    checker: &ModuleChecker<'_>,
    tokens: &[Token],
    scope: &BTreeMap<String, Type>,
    budget: &mut TypeExpansionBudget,
) -> Result<Vec<Type>, ()> {
    if !budget.consume() {
        return Err(());
    }
    let tokens = strip_outer_parentheses(tokens);
    if tokens.len() > 1 && tokens.last().is_some_and(|token| token.is("!")) {
        return possible_types(checker, &tokens[..tokens.len() - 1], scope, budget);
    }
    let Some((receiver, property)) = member_access_target(tokens) else {
        let mut result = Vec::new();
        push_type(checker.infer_expression(tokens, scope), &mut result, budget)?;
        return Ok(result);
    };
    let index = tokens
        .last()
        .is_some_and(|token| token.is("]"))
        .then(|| canonical_index_key(&tokens[receiver.len() + 1..tokens.len() - 1]))
        .flatten();
    let mut result = Vec::new();
    for owner in possible_types(checker, receiver, scope, budget)? {
        access_candidates(checker, owner, property, index, &mut result, budget)?;
    }
    Ok(result)
}

fn access_candidates(
    checker: &ModuleChecker<'_>,
    owner: Type,
    property: Option<&str>,
    index: Option<usize>,
    result: &mut Vec<Type>,
    budget: &mut TypeExpansionBudget,
) -> Result<(), ()> {
    match owner {
        Type::Union(parts) => {
            for part in parts {
                access_candidates(checker, part, property, index, result, budget)?;
            }
        }
        Type::Named { .. } => {
            let mut visited = HashSet::new();
            match instantiate_named(
                &owner,
                &checker.types,
                &mut visited,
                budget,
                "readonly receiver",
            ) {
                Some(value) => {
                    access_candidates(checker, value, property, index, result, budget)?;
                }
                None if budget.exhausted => return Err(()),
                None => push_type(Type::Unknown, result, budget)?,
            }
        }
        Type::Array(element) if property.is_none() || index.is_some() => {
            push_type(*element, result, budget)?;
        }
        Type::Tuple(values) if property.is_none() || index.is_some() => {
            if let Some(index) = index {
                if let Some(value) = values.get(index) {
                    push_type(value.clone(), result, budget)?;
                }
            } else {
                for value in values {
                    push_type(value, result, budget)?;
                }
            }
        }
        Type::Record(fields) if property.is_none() => {
            for field in fields {
                push_type(field.value, result, budget)?;
            }
        }
        owner => {
            if let Some(property) = property {
                let mut visited = HashSet::new();
                match property_type(&owner, property, &checker.types, &mut visited, budget) {
                    PropertyType::Found { value, .. } => push_type(value, result, budget)?,
                    PropertyType::Exhausted => return Err(()),
                    PropertyType::Indeterminate => push_type(Type::Unknown, result, budget)?,
                    PropertyType::Missing => {}
                }
            } else {
                push_type(Type::Unknown, result, budget)?;
            }
        }
    }
    Ok(())
}

fn push_type(
    value: Type,
    result: &mut Vec<Type>,
    budget: &mut TypeExpansionBudget,
) -> Result<(), ()> {
    if !budget.consume() {
        return Err(());
    }
    if let Type::Union(parts) = value {
        for part in parts {
            push_type(part, result, budget)?;
        }
    } else {
        result.push(value);
    }
    Ok(())
}

const MAX_READONLY_TAINT_DEPTH: usize = 16;

/// Deep, bounded search for a readonly member anywhere reachable through
/// arrays, tuples, records, unions and named types.
fn deep_contains_readonly(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
    depth: usize,
) -> Result<bool, ()> {
    if depth > MAX_READONLY_TAINT_DEPTH || !budget.consume() {
        return Err(());
    }
    match value {
        Type::Record(fields) => {
            for field in fields {
                if field.readonly
                    || deep_contains_readonly(&field.value, aliases, budget, depth + 1)?
                {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Type::Array(element) => deep_contains_readonly(element, aliases, budget, depth + 1),
        Type::Tuple(parts) | Type::Union(parts) | Type::Intersection(parts) => {
            for part in parts {
                if deep_contains_readonly(part, aliases, budget, depth + 1)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Type::Named { .. } => {
            match instantiate_named(
                value,
                aliases,
                &mut HashSet::new(),
                budget,
                "readonly taint",
            ) {
                Some(value) => deep_contains_readonly(&value, aliases, budget, depth + 1),
                None if budget.exhausted => Err(()),
                None => Ok(false),
            }
        }
        _ => Ok(false),
    }
}
