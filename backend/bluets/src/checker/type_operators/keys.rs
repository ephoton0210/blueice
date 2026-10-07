// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Key sets and projections preserve numeric keys, unions and optional fields.
use super::*;

pub(super) fn literal_key(name: &str) -> Type {
    Type::Literal(if name.parse::<f64>().is_ok() {
        name.to_string()
    } else {
        format!("{name:?}")
    })
}

pub(super) fn key_name(value: &Type) -> Option<String> {
    let Type::Literal(text) = value else {
        return None;
    };
    Some(text.trim_matches(['\'', '"']).to_string())
}

pub(super) fn keyof(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    let value = expanded(value, aliases, visited, budget);
    match value {
        Type::Any => Some(Type::Union(vec![
            Type::String,
            Type::Number,
            Type::Named {
                name: "symbol".into(),
                arguments: vec![],
            },
        ])),
        Type::Unknown | Type::Never => Some(Type::Never),
        Type::Record(fields) | Type::CallableRecord { fields, .. } => {
            Some(union(fields.iter().map(|field| literal_key(&field.name))))
        }
        Type::Union(parts) => {
            let mut sets = Vec::new();
            for part in parts {
                if !budget.consume() {
                    return None;
                }
                let keys = keyof(&part, aliases, &mut visited.clone(), budget)?;
                sets.push(match keys {
                    Type::Union(keys) => keys,
                    Type::Never => vec![],
                    value => vec![value],
                });
            }
            let first = sets.first()?;
            Some(union(
                first
                    .iter()
                    .filter(|key| sets.iter().all(|set| set.contains(key)))
                    .cloned(),
            ))
        }
        Type::Intersection(parts) => {
            let mut keys = Vec::new();
            for part in parts {
                if !budget.consume() {
                    return None;
                }
                keys.push(keyof(&part, aliases, &mut visited.clone(), budget)?);
            }
            Some(union(keys))
        }
        Type::Array(_) | Type::Tuple(_) => {
            let mut keys = vec![Type::Number];
            keys.extend(
                [
                    "length",
                    "toString",
                    "toLocaleString",
                    "pop",
                    "push",
                    "concat",
                    "join",
                    "reverse",
                    "shift",
                    "slice",
                    "sort",
                    "splice",
                    "unshift",
                    "indexOf",
                    "lastIndexOf",
                    "every",
                    "some",
                    "forEach",
                    "map",
                    "filter",
                    "reduce",
                    "reduceRight",
                    "find",
                    "findIndex",
                    "fill",
                    "copyWithin",
                    "entries",
                    "keys",
                    "values",
                    "includes",
                    "flat",
                    "flatMap",
                ]
                .into_iter()
                .map(literal_key),
            );
            if let Type::Tuple(items) = &value {
                keys.extend((0..items.len()).map(|index| Type::Literal(format!("\"{index}\""))));
            }
            Some(union(keys))
        }
        _ => None,
    }
}

pub(super) fn indexed(
    object: &Type,
    index: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Option<Type> {
    let object = expanded(object, aliases, &mut visited.clone(), budget);
    let index = expanded(index, aliases, &mut visited.clone(), budget);
    if let Type::Union(indices) = &index {
        let mut values = Vec::new();
        for index in indices {
            if !budget.consume() {
                return None;
            }
            values.push(indexed(
                &object,
                index,
                aliases,
                &mut visited.clone(),
                budget,
            )?);
        }
        return Some(union(values));
    }
    if let Type::Union(objects) = &object {
        let mut values = Vec::new();
        for object in objects {
            if !budget.consume() {
                return None;
            }
            values.push(indexed(
                object,
                &index,
                aliases,
                &mut visited.clone(),
                budget,
            )?);
        }
        return Some(union(values));
    }
    match &object {
        Type::Array(item)
            if index == Type::Number
                || key_name(&index).is_some_and(|key| key.parse::<usize>().is_ok()) =>
        {
            Some((**item).clone())
        }
        Type::Tuple(items) if index == Type::Number => {
            Some(union(items.iter().map(TupleTypeElement::indexed_type)))
        }
        Type::Tuple(items) => items
            .get(key_name(&index)?.parse::<usize>().ok()?)
            .map(TupleTypeElement::indexed_type),
        Type::Record(fields) | Type::CallableRecord { fields, .. } => {
            let key = key_name(&index)?;
            let field = fields.iter().find(|field| field.name == key)?;
            Some(if field.optional {
                union([field.value.clone(), Type::Undefined])
            } else {
                field.value.clone()
            })
        }
        Type::Intersection(parts) => {
            let values = parts
                .iter()
                .filter_map(|part| indexed(part, &index, aliases, &mut visited.clone(), budget))
                .collect::<Vec<_>>();
            if values.is_empty() {
                None
            } else {
                Some(Type::Intersection(values))
            }
        }
        Type::Any => Some(Type::Any),
        _ => None,
    }
}
