// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Only checker-proven fresh generic literal unions widen in declarations.
use super::*;

pub(super) fn declaration_union(value: Type, fresh: bool) -> Type {
    if fresh {
        if let Type::Union(parts) = &value {
            let widened: Vec<_> = parts
                .iter()
                .map(|part| match part {
                    Type::Literal(text) if text.starts_with(['\'', '"', '`']) => Type::String,
                    Type::Literal(text) if text.parse::<f64>().is_ok() => Type::Number,
                    Type::Literal(text) if matches!(text.as_str(), "true" | "false") => {
                        Type::Boolean
                    }
                    value => value.clone(),
                })
                .collect();
            if let Some(first) = widened.first() {
                if widened.iter().all(|value| value == first) {
                    return first.clone();
                }
            }
            let mut unique = Vec::new();
            for part in widened {
                if !unique.contains(&part) {
                    unique.push(part);
                }
            }
            unique.sort_by_key(|part| match part {
                Type::String => 0,
                Type::Number => 1,
                Type::Boolean => 2,
                Type::Null => 8,
                Type::Undefined => 9,
                _ => 4,
            });
            return Type::Union(unique);
        }
    }
    value
}

/// Broad primitive element unions use the native allocation order. Explicit
/// array annotations and regular literal-member unions keep their source order.
pub(super) fn inferred_array(mut value: Type) -> Type {
    if let Type::Array(element) = &mut value {
        if let Type::Union(parts) = element.as_mut() {
            if parts.iter().all(|part| {
                matches!(
                    part,
                    Type::String | Type::Number | Type::Boolean | Type::Null | Type::Undefined
                )
            }) {
                parts.sort_by_key(|part| match part {
                    Type::String => 0,
                    Type::Number => 1,
                    Type::Boolean => 2,
                    Type::Null => 8,
                    _ => 9,
                });
            }
        }
    }
    value
}
