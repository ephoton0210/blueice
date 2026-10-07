// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Template products consume shared work; open substitutions remain patterns.
use super::*;

fn literal_text(value: &Type) -> Option<String> {
    match value {
        Type::Literal(value) => Some(value.trim_matches(['\'', '"']).to_string()),
        Type::Null => Some("null".into()),
        Type::Undefined => Some("undefined".into()),
        _ => None,
    }
}

fn normalize(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    let value = expanded(value, aliases, &mut HashSet::new(), budget);
    if let Type::Intersection(parts) = &value {
        for part in parts {
            let part = expanded(part, aliases, &mut HashSet::new(), budget);
            if literal_text(&part).is_some()
                && parts.iter().all(|expected| {
                    is_assignable(&part, expected, aliases, &mut HashSet::new(), budget)
                })
            {
                return part;
            }
        }
    }
    value
}

pub(super) fn resolve(
    value: &TemplateLiteralType,
    aliases: &BTreeMap<String, TypeDefinition>,
    _visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    let mut products = vec![value.head.clone()];
    for (part, tail) in &value.spans {
        let part = normalize(part, aliases, budget);
        let mut alternatives = match part {
            Type::Union(parts) => parts,
            Type::Never => return Some(Type::Never),
            Type::Boolean => vec![Type::Literal("false".into()), Type::Literal("true".into())],
            value => vec![value],
        };
        // Literal type IDs follow pinned default-library initialization;
        // uncached literals retain their first source occurrence.
        alternatives.sort_by_key(|part| match part {
            Type::Literal(text) => literal_order::numeric_rank(text),
            _ => usize::MAX,
        });
        let mut next = Vec::new();
        for prefix in &products {
            for part in &alternatives {
                if !budget.consume() {
                    return None;
                }
                let part = literal_text(part)?;
                next.push(format!("{prefix}{part}{tail}"));
            }
        }
        products = next;
    }
    Some(union(
        products
            .into_iter()
            .map(|value| Type::Literal(format!("{value:?}"))),
    ))
}

pub(super) fn matches(
    actual: &Type,
    value: &TemplateLiteralType,
    bindings: &mut BTreeMap<String, Type>,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    let Some(actual) = literal_text(actual) else {
        return false;
    };
    let Some(rest) = actual.strip_prefix(&value.head) else {
        return false;
    };
    match_spans(rest, &value.spans, bindings, aliases, budget)
}

fn match_spans(
    rest: &str,
    spans: &[(Type, String)],
    bindings: &mut BTreeMap<String, Type>,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    if !budget.consume() {
        return false;
    }
    let Some((pattern, tail)) = spans.first() else {
        return rest.is_empty();
    };
    let splits = if spans.len() == 1 {
        rest.strip_suffix(tail)
            .map(|part| vec![(part, "")])
            .unwrap_or_default()
    } else if tail.is_empty() {
        rest.char_indices()
            .skip(1)
            .map(|(index, _)| (&rest[..index], &rest[index..]))
            .collect()
    } else {
        rest.match_indices(tail)
            .map(|(index, _)| (&rest[..index], &rest[index + tail.len()..]))
            .collect()
    };
    for (part, rest) in splits {
        let mut candidate = bindings.clone();
        let normalized = normalize(pattern, aliases, budget);
        let fits = match &normalized {
            Type::String | Type::Any => true,
            Type::Number => !part.trim().is_empty() && part.parse::<f64>().is_ok(),
            Type::Infer(parameter) => {
                let literal = Type::Literal(format!("{part:?}"));
                if parameter.constraint.as_ref().is_none_or(|constraint| {
                    is_assignable(&literal, constraint, aliases, &mut HashSet::new(), budget)
                }) {
                    candidate.insert(parameter.name.clone(), literal);
                    true
                } else {
                    false
                }
            }
            value => conditional::matches(
                &Type::Literal(format!("{part:?}")),
                value,
                &mut candidate,
                aliases,
                budget,
            ),
        };
        if fits && match_spans(rest, &spans[1..], &mut candidate, aliases, budget) {
            *bindings = candidate;
            return true;
        }
    }
    false
}
