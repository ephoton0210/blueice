// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Two disjoint, bounded literal-tag groups shared by checking and inference.

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
        // Inherited interfaces and intersected receivers are excluded from
        // this first method overload form.
        _ => false,
    }
}

pub(super) fn select_callback_method_overload<'a>(
    overloads: &'a [Type],
    actuals: &[Type],
) -> Result<&'a Type, MethodOverloadError> {
    let [first, second] = overloads else {
        return Err(MethodOverloadError::Unsupported);
    };
    let Some(first_tags) = callback_method_tags(first) else {
        return Err(MethodOverloadError::Unsupported);
    };
    let Some(second_tags) = callback_method_tags(second) else {
        return Err(MethodOverloadError::Unsupported);
    };
    if first_tags.iter().any(|tag| second_tags.contains(tag)) {
        return Err(MethodOverloadError::Unsupported);
    }
    if actuals.len() != 2 {
        return Err(MethodOverloadError::NoMatch);
    }
    let Some(actual_tags) = literal_string_tags(&actuals[0]) else {
        return Err(MethodOverloadError::NoMatch);
    };
    if actual_tags.iter().all(|tag| first_tags.contains(tag)) {
        Ok(first)
    } else if actual_tags.iter().all(|tag| second_tags.contains(tag)) {
        Ok(second)
    } else if actual_tags
        .iter()
        .all(|tag| first_tags.contains(tag) || second_tags.contains(tag))
    {
        Err(MethodOverloadError::Ambiguous)
    } else {
        Err(MethodOverloadError::NoMatch)
    }
}

fn callback_method_tags(value: &Type) -> Option<Vec<&str>> {
    let Type::Function { parameters, result } = value else {
        return None;
    };
    let [tag, callback] = parameters.as_slice() else {
        return None;
    };
    if result.as_ref() != &Type::Void
        || [tag, callback]
            .iter()
            .any(|parameter| parameter.optional || parameter.rest || parameter.default.is_some())
    {
        return None;
    }
    let Type::Function {
        parameters: callback_parameters,
        result: callback_result,
    } = callback.annotation.as_ref()?
    else {
        return None;
    };
    let [callback_parameter] = callback_parameters.as_slice() else {
        return None;
    };
    if callback_result.as_ref() != &Type::Void
        || callback_parameter.optional
        || callback_parameter.rest
        || callback_parameter.default.is_some()
        || !callback_parameter.annotation.as_ref().is_some_and(|value| {
            matches!(value, Type::String | Type::Number | Type::Boolean)
                || matches!(value, Type::Named { arguments, .. } if arguments.is_empty())
        })
    {
        return None;
    }
    literal_string_tags(tag.annotation.as_ref()?)
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
