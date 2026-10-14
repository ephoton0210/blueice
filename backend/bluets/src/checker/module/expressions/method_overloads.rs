// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Exact two-tag callback method selection shared by checking and inference.

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
    let Some(first_tag) = callback_method_tag(first) else {
        return Err(MethodOverloadError::Unsupported);
    };
    let Some(second_tag) = callback_method_tag(second) else {
        return Err(MethodOverloadError::Unsupported);
    };
    if first_tag == second_tag {
        return Err(MethodOverloadError::Unsupported);
    }
    if actuals.len() != 2 {
        return Err(MethodOverloadError::NoMatch);
    }
    match &actuals[0] {
        Type::Literal(raw) => match literal_string_value(raw) {
            Some(tag) if tag == first_tag => Ok(first),
            Some(tag) if tag == second_tag => Ok(second),
            _ => Err(MethodOverloadError::NoMatch),
        },
        Type::Union(parts) if parts.len() == 2 => {
            let tags = parts
                .iter()
                .map(|part| match part {
                    Type::Literal(raw) => literal_string_value(raw),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>();
            match tags.as_deref() {
                Some([left, right])
                    if (left == &first_tag && right == &second_tag)
                        || (left == &second_tag && right == &first_tag) =>
                {
                    Err(MethodOverloadError::Ambiguous)
                }
                _ => Err(MethodOverloadError::NoMatch),
            }
        }
        _ => Err(MethodOverloadError::NoMatch),
    }
}

fn callback_method_tag(value: &Type) -> Option<&str> {
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
    let Type::Literal(raw_tag) = tag.annotation.as_ref()? else {
        return None;
    };
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
        || !matches!(
            callback_parameter.annotation,
            Some(Type::String | Type::Number | Type::Boolean)
        )
    {
        return None;
    }
    literal_string_value(raw_tag)
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
