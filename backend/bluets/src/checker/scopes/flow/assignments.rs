// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compound writes reset the flow value while keeping its declared boundary.

use super::*;

/// The checker validates every write against its annotation. When that union
/// has one named non-nullish constituent, flow retains that declared API and
/// records whether a nullish input survives. Literal/structural candidates
/// keep their more precise representation; distinct named alternatives stay
/// separate.
pub(super) fn named_union_assignment(
    scopes: &ScopeModel<'_>,
    actual: Type,
    declared: &Type,
    tokens: &[Token],
) -> Type {
    let Type::Union(candidates) = declared else {
        return actual;
    };
    let named = candidates
        .iter()
        .filter(|value| !matches!(value, Type::Null | Type::Undefined))
        .collect::<Vec<_>>();
    let [candidate @ Type::Named { .. }] = named.as_slice() else {
        return actual;
    };
    let parts = match actual {
        Type::Union(parts) => parts,
        value => vec![value],
    };
    let mut budget = TypeExpansionBudget::new(scopes.max_type_expansions);
    let result = union(parts.into_iter().map(|part| {
        if matches!(part, Type::Named { .. })
            && !crate::checker::module::projection_qualifiers(
                &part,
                candidate,
                &scopes.type_definitions,
                &mut budget,
            )
        {
            (*candidate).clone()
        } else {
            part
        }
    }));
    if budget.exhausted {
        scopes.flow_limit(
            tokens,
            "assignment qualifiers exceed their generic-expansion limit",
        );
    }
    result
}

pub(super) fn compound(
    scopes: &ScopeModel<'_>,
    tokens: &[Token],
    state: &State,
    infer: &impl Fn(&[Token], &BTreeMap<String, Type>, &Type) -> Type,
) -> Option<State> {
    for operator in super::super::targets::ASSIGNMENTS
        .iter()
        .filter(|operator| **operator != "=")
    {
        let Some(index) = super::super::targets::top_level(tokens, operator) else {
            continue;
        };
        let [target] = &tokens[..index] else {
            return Some(state.clone());
        };
        let id = scopes.flow_binding(&target.text, target.start)?;
        let before = current(scopes, state, &id);
        let declared = scopes.flow_declared(&id);
        let after = infer(
            &tokens[index + 1..],
            &values(scopes, state, target.start),
            &declared,
        );
        let result = match *operator {
            "??=" => {
                let parts = match before {
                    Type::Union(parts) => parts,
                    value => vec![value],
                };
                let nullish = parts
                    .iter()
                    .any(|part| matches!(part, Type::Null | Type::Undefined));
                union(
                    parts
                        .into_iter()
                        .filter(|part| !matches!(part, Type::Null | Type::Undefined))
                        .chain(nullish.then_some(after)),
                )
            }
            "&&=" | "||=" => {
                let positive = *operator == "||=";
                let retained = predicates::truthy_type(&before, positive);
                let assigned = predicates::truthy_type(&before, !positive);
                union(retained.into_iter().chain(assigned.map(|_| after)))
            }
            "+=" if string_type(&before) || string_type(&after) => Type::String,
            _ => Type::Number,
        };
        let mut next = state.clone();
        next.invalidate(&id);
        if result == declared {
            next.remove(&id);
        } else {
            next.insert(id, result);
        }
        return Some(next);
    }
    None
}

fn string_type(value: &Type) -> bool {
    *value == Type::String
        || matches!(value,Type::Literal(text) if literal_primitive(text) == Type::String)
}
