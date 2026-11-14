// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Template products consume shared work; open substitutions remain patterns.
use super::*;
use crate::utf16::{StringLiteral, Utf16String};

#[path = "templates/numeric.rs"]
mod numeric;
#[cfg(test)]
#[path = "templates/tests.rs"]
mod tests;

fn literal_text(value: &Type) -> Option<Utf16String> {
    if let Some(value) = value.string_value() {
        return Some(value);
    }
    match value {
        Type::Literal(value) if !value.starts_with(['\'', '"']) => {
            Some(Utf16String::from(value.as_str()))
        }
        Type::Null => Some(Utf16String::from("null")),
        Type::Undefined => Some(Utf16String::from("undefined")),
        _ => None,
    }
}

fn normalize(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
    preserve_barriers: bool,
) -> Type {
    let value = expand(
        value,
        aliases,
        &mut HashSet::new(),
        budget,
        preserve_barriers,
    );
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
    let mut products = vec![cooked(&value.head)?];
    for (part, tail) in &value.spans {
        let tail = cooked(tail)?;
        let part = normalize(part, aliases, budget, false);
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
                next.push(Utf16String::from_units(
                    prefix
                        .units()
                        .iter()
                        .chain(part.units())
                        .chain(tail.units())
                        .copied()
                        .collect(),
                ));
            }
        }
        products = next;
    }
    Some(union(products.into_iter().map(|value| {
        Type::StringLiteral(StringLiteral::generated(value))
    })))
}

pub(super) fn matches(
    actual: &Type,
    value: &TemplateLiteralType,
    bindings: &mut conditional::MatchBindings,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    let Some(actual) = literal_text(actual) else {
        return false;
    };
    let Some(head) = cooked(&value.head) else {
        return false;
    };
    let Some(rest) = actual.units().strip_prefix(head.units()) else {
        return false;
    };
    let Some(spans): Option<Vec<_>> = value
        .spans
        .iter()
        .map(|(value, tail)| Some((value.clone(), cooked(tail)?)))
        .collect()
    else {
        return false;
    };
    match_spans(rest, &spans, bindings, aliases, budget)
}

fn cooked(raw: &str) -> Option<Utf16String> {
    Utf16String::parse_template_chunk(raw)
}

fn match_spans(
    rest: &[u16],
    spans: &[(Type, Utf16String)],
    bindings: &mut conditional::MatchBindings,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    if !budget.consume() {
        return false;
    }
    let Some((pattern, tail)) = spans.first() else {
        return rest.is_empty();
    };
    let (part, remaining) = if spans.len() == 1 {
        let Some(part) = rest.strip_suffix(tail.units()) else {
            return false;
        };
        (part, &[][..])
    } else if tail.units().is_empty() {
        if rest.is_empty() {
            return false;
        }
        (&rest[..1], &rest[1..])
    } else {
        let Some(index) = rest
            .windows(tail.units().len())
            .position(|part| part == tail.units())
        else {
            return false;
        };
        (&rest[..index], &rest[index + tail.units().len()..])
    };
    let mut candidate = bindings.clone();
    let normalized = normalize(pattern, aliases, budget, true);
    let fits = match &normalized {
        Type::String | Type::Any => true,
        Type::Number => numeric::number(part).is_some(),
        Type::BigInt => numeric::bigint(part),
        Type::Infer(parameter) => {
            let literal = match parameter.constraint.as_ref() {
                Some(Type::Number) => {
                    let Some(number) = numeric::number(part) else {
                        return false;
                    };
                    let canonical = crate::enum_eval::js_number_text(number);
                    if canonical.encode_utf16().eq(part.iter().copied()) {
                        Type::Literal(canonical)
                    } else {
                        Type::Number
                    }
                }
                _ => Type::StringLiteral(StringLiteral::generated(Utf16String::from_units(
                    part.to_vec(),
                ))),
            };
            if parameter.constraint.as_ref().is_none_or(|constraint| {
                is_assignable(&literal, constraint, aliases, &mut HashSet::new(), budget)
            }) {
                candidate.record(&parameter.name, &literal, aliases, budget)
            } else {
                false
            }
        }
        value => conditional::matches(
            &Type::StringLiteral(StringLiteral::generated(Utf16String::from_units(
                part.to_vec(),
            ))),
            value,
            &mut candidate,
            aliases,
            budget,
        ),
    };
    if fits && match_spans(remaining, &spans[1..], &mut candidate, aliases, budget) {
        *bindings = candidate;
        return true;
    }
    false
}
