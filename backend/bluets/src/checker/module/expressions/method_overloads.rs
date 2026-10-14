// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded disjoint event tags with callbacks and optional listener options.

use super::*;

pub(super) enum MethodOverloadError {
    Unsupported,
    Ambiguous,
    NoMatch,
}

pub(super) fn supports_callback_method_receiver(
    receiver: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    limit: usize,
) -> bool {
    if limit == 0 {
        return false;
    }
    match receiver {
        Type::Record(_) => true,
        Type::Named { .. } => {
            let mut budget = TypeExpansionBudget::new(limit);
            instantiate_named(
                receiver,
                aliases,
                &mut HashSet::new(),
                &mut budget,
                "method receiver",
            )
            .is_some_and(|expanded| {
                supports_callback_method_receiver(&expanded, aliases, limit.saturating_sub(1))
            })
        }
        Type::Intersection(parts) => {
            !parts.is_empty()
                && parts
                    .iter()
                    .all(|part| supports_callback_method_receiver(part, aliases, limit - 1))
        }
        _ => false,
    }
}

pub(super) fn has_literal_method_tags(overloads: &[Type]) -> bool {
    overloads.iter().any(|overload| {
        let Type::Function { parameters, .. } = overload else {
            return false;
        };
        parameters
            .first()
            .and_then(|parameter| parameter.annotation.as_ref())
            .and_then(literal_string_tags)
            .is_some()
    })
}

pub(super) fn select_callback_method_overload<'a>(
    overloads: &'a [Type],
    actuals: &[Type],
    aliases: &BTreeMap<String, TypeDefinition>,
    limit: usize,
) -> Result<&'a Type, MethodOverloadError> {
    if !(2..=16).contains(&overloads.len()) {
        return Err(MethodOverloadError::Unsupported);
    }
    let mut groups = Vec::with_capacity(overloads.len());
    let mut seen = HashSet::new();
    let mut fallback: Option<&Type> = None;
    for overload in overloads {
        let tags = callback_method_tags(overload, aliases, limit)
            .ok_or(MethodOverloadError::Unsupported)?;
        if let Some(tags) = &tags {
            for tag in tags {
                if !seen.insert(*tag) {
                    return Err(MethodOverloadError::Unsupported);
                }
            }
        } else if let Some(previous) = fallback {
            if !same_callback_signature(previous, overload, limit) {
                return Err(MethodOverloadError::Unsupported);
            }
        } else {
            fallback = Some(overload);
        }
        groups.push(tags);
    }
    let Some(actual_tags) = actuals.first().and_then(literal_string_tags) else {
        return fallback.ok_or(MethodOverloadError::NoMatch);
    };
    for (overload, tags) in overloads.iter().zip(groups) {
        if tags.is_some_and(|tags| actual_tags.iter().all(|tag| tags.contains(tag))) {
            return Ok(overload);
        }
    }
    if let Some(fallback) = fallback {
        return Ok(fallback);
    }
    if actual_tags.iter().all(|tag| seen.contains(tag)) {
        Err(MethodOverloadError::Ambiguous)
    } else {
        Err(MethodOverloadError::NoMatch)
    }
}

fn callback_method_tags<'a>(
    value: &'a Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    limit: usize,
) -> Option<Option<Vec<&'a str>>> {
    let Type::Function { parameters, result } = value else {
        return None;
    };
    if !(2..=3).contains(&parameters.len()) || result.as_ref() != &Type::Void {
        return None;
    }
    let (tag, callback) = (&parameters[0], &parameters[1]);
    if [tag, callback]
        .iter()
        .any(|parameter| parameter.optional || parameter.rest || parameter.default.is_some())
        || !supports_event_callback(callback.annotation.as_ref()?, aliases, limit)
    {
        return None;
    }
    if let Some(options) = parameters.get(2) {
        if !options.optional
            || options.rest
            || options.default.is_some()
            || options.annotation.is_none()
        {
            return None;
        }
    }
    let tag = tag.annotation.as_ref()?;
    if tag == &Type::String {
        Some(None)
    } else {
        literal_string_tags(tag).map(Some)
    }
}

fn same_callback_signature(left: &Type, right: &Type, limit: usize) -> bool {
    if limit == 0 {
        return false;
    }
    let (
        Type::Function {
            parameters: left,
            result: left_result,
        },
        Type::Function {
            parameters: right,
            result: right_result,
        },
    ) = (left, right)
    else {
        return false;
    };
    left_result == right_result
        && left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            let same_annotation = match (&left.annotation, &right.annotation) {
                (Some(left), Some(right)) => same_signature_type(left, right, limit - 1),
                (None, None) => true,
                _ => false,
            };
            same_annotation
                && left.optional == right.optional
                && left.rest == right.rest
                && left.default == right.default
        })
}

fn same_signature_type(left: &Type, right: &Type, limit: usize) -> bool {
    if limit == 0 {
        return false;
    }
    match (left, right) {
        (Type::Function { .. }, Type::Function { .. }) => {
            same_callback_signature(left, right, limit - 1)
        }
        (Type::Union(left), Type::Union(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| same_signature_type(left, right, limit - 1))
        }
        _ => left == right,
    }
}

fn supports_event_callback(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    limit: usize,
) -> bool {
    if limit == 0 {
        return false;
    }
    match value {
        Type::Null => true,
        Type::Union(parts) => {
            !parts.is_empty()
                && parts.len() <= 16
                && parts
                    .iter()
                    .all(|part| supports_event_callback(part, aliases, limit - 1))
        }
        Type::Named { .. } => {
            let mut budget = TypeExpansionBudget::new(limit);
            instantiate_named(
                value,
                aliases,
                &mut HashSet::new(),
                &mut budget,
                "event callback",
            )
            .is_some_and(|expanded| supports_event_callback(&expanded, aliases, limit - 1))
        }
        Type::Record(fields) => fields
            .iter()
            .find(|field| field.name == "handleEvent")
            .is_some_and(|field| {
                !field.optional
                    && matches!(field.value, Type::Function { .. })
                    && supports_event_callback(&field.value, aliases, limit - 1)
            }),
        Type::Function { parameters, result } => {
            let [parameter] = parameters.as_slice() else {
                return false;
            };
            result.as_ref() == &Type::Void && !parameter.optional && !parameter.rest
                && parameter.default.is_none()
                && parameter.annotation.as_ref().is_some_and(|annotation| {
                    matches!(annotation, Type::String | Type::Number | Type::Boolean)
                        || matches!(annotation, Type::Named { arguments, .. } if arguments.is_empty())
                })
        }
        _ => false,
    }
}

fn literal_string_tags(value: &Type) -> Option<Vec<&str>> {
    let values = match value {
        Type::Literal(raw) => return literal_string_value(raw).map(|tag| vec![tag]),
        Type::Union(parts) if !parts.is_empty() && parts.len() <= 16 => parts,
        _ => return None,
    };
    let mut tags = Vec::with_capacity(values.len());
    for value in values {
        let Type::Literal(raw) = value else {
            return None;
        };
        let tag = literal_string_value(raw)?;
        if tags.contains(&tag) {
            return None;
        }
        tags.push(tag);
    }
    Some(tags)
}

fn literal_string_value(raw: &str) -> Option<&str> {
    let value = raw
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .or_else(|| {
            raw.strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
        })?;
    (!value.contains('\\') && !value.chars().any(|ch| ch == '\n' || ch == '\r')).then_some(value)
}
