// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compound writes reset the flow value while keeping its declared boundary.

use super::*;

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
