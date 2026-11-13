// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Broad primitive intersections retain narrower values and rigid parameters.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Domain {
    String,
    Number,
    Boolean,
    BigInt,
    Symbol,
}

fn broad(value: &Type) -> Option<Domain> {
    match value {
        Type::String => Some(Domain::String),
        Type::Number => Some(Domain::Number),
        Type::Boolean => Some(Domain::Boolean),
        Type::BigInt => Some(Domain::BigInt),
        Type::Symbol => Some(Domain::Symbol),
        _ => None,
    }
}

fn domain(value: &Type) -> Option<Domain> {
    broad(value).or_else(|| {
        if value.is_string_like() {
            return Some(Domain::String);
        }
        match value {
            Type::UniqueSymbol(_) => Some(Domain::Symbol),
            Type::Literal(text) if matches!(text.as_str(), "true" | "false") => {
                Some(Domain::Boolean)
            }
            Type::Literal(text) if text.ends_with('n') => Some(Domain::BigInt),
            Type::Literal(text) if text.parse::<f64>().is_ok() => Some(Domain::Number),
            _ => None,
        }
    })
}

pub(super) fn reduce(parts: &[Type], budget: &mut TypeExpansionBudget) -> Option<Type> {
    let restriction = parts.iter().find_map(broad)?;
    let mut result = Vec::new();
    let mut narrower = false;
    for part in parts {
        if !budget.consume() {
            return None;
        }
        if let Some(domain) = domain(part) {
            if domain != restriction {
                return Some(Type::Never);
            }
            if broad(part).is_some() {
                continue;
            }
            narrower = true;
        }
        if !result.contains(part) {
            result.push(part.clone());
        }
    }
    // An open parameter remains rigid: `string & K` must survive until K is
    // substituted. A concrete narrower constituent already implies the broad
    // restriction, even if other intersection factors remain symbolic.
    if !narrower {
        return None;
    }
    Some(match result.len() {
        1 => result.pop().expect("a concrete primitive constituent"),
        _ => Type::Intersection(result),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concrete_primitive_keys_reduce_but_open_parameters_remain_rigid() {
        let key = Type::Literal("\"word\"".into());
        assert_eq!(
            reduce(
                &[Type::String, key.clone()],
                &mut TypeExpansionBudget::new(20)
            ),
            Some(key)
        );
        assert_eq!(
            reduce(
                &[Type::Number, Type::Literal("3".into())],
                &mut TypeExpansionBudget::new(20)
            ),
            Some(Type::Literal("3".into()))
        );
        assert_eq!(
            reduce(
                &[Type::String, Type::Number],
                &mut TypeExpansionBudget::new(20)
            ),
            Some(Type::Never)
        );
        let parameter = Type::Named {
            name: "K".into(),
            arguments: Vec::new(),
        };
        assert!(reduce(
            &[Type::String, parameter],
            &mut TypeExpansionBudget::new(20)
        )
        .is_none());
        let mut budget = TypeExpansionBudget::new(0);
        assert!(reduce(&[Type::String, Type::Literal("\"x\"".into())], &mut budget).is_none());
        assert!(budget.exhausted);
    }
}
