// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration-only annotations preserve the source AST and runtime boundaries.

use super::*;
use crate::checker::CheckedModule;

pub(super) fn declaration_module(checked: &CheckedModule) -> Module {
    let mut module = checked.module.clone();
    apply(
        &mut module.declarations,
        &checked.inferred_returns,
        &checked.inferred_parameters,
    );
    module
}

fn apply(
    declarations: &mut [Declaration],
    inferred: &BTreeMap<usize, Type>,
    parameters: &BTreeMap<usize, Type>,
) {
    for declaration in declarations {
        match declaration {
            Declaration::Function(function) if function.return_type.is_none() => {
                function.return_type = inferred.get(&function.span.start).map(canonical);
                for parameter in &mut function.parameters {
                    if parameter.annotation.is_none() {
                        parameter.annotation = parameters.get(&parameter.span.start).cloned();
                    }
                }
            }
            Declaration::Class(class) => {
                for member in &mut class.members {
                    if let Some(method) = &mut member.method {
                        for parameter in &mut method.parameters {
                            if parameter.annotation.is_none() {
                                parameter.annotation =
                                    parameters.get(&parameter.span.start).cloned();
                            }
                        }
                        if method.return_type.is_none() {
                            method.return_type = inferred.get(&method.span.start).map(canonical);
                        }
                    }
                    if let Some(accessor) = &mut member.accessor {
                        for parameter in &mut accessor.parameters {
                            if parameter.annotation.is_none() {
                                parameter.annotation =
                                    parameters.get(&parameter.span.start).cloned();
                            }
                        }
                        if accessor.getter && accessor.return_type.is_none() {
                            accessor.return_type = inferred.get(&accessor.span.start).map(getter);
                        }
                    }
                }
            }
            Declaration::Namespace(namespace) => apply(&mut namespace.body, inferred, parameters),
            _ => {}
        }
    }
}

/// TypeScript emits numeric getter literals before string literals.
pub(super) fn getter(value: &Type) -> Type {
    let mut value = canonical(value);
    if let Type::Union(parts) = &mut value {
        parts.sort_by(|left, right| {
            let rank = |part: &Type| match part {
                Type::Literal(text) if text.parse::<f64>().is_ok() => 0,
                Type::Literal(text) if text.starts_with(['\'', '"', '`']) => 1,
                _ => 2,
            };
            rank(left)
                .cmp(&rank(right))
                .then_with(|| match (left, right) {
                    (Type::Literal(left), Type::Literal(right)) => left
                        .parse::<f64>()
                        .ok()
                        .zip(right.parse::<f64>().ok())
                        .and_then(|(left, right)| left.partial_cmp(&right))
                        .unwrap_or(std::cmp::Ordering::Equal),
                    _ => std::cmp::Ordering::Equal,
                })
        });
    }
    value
}

pub(super) fn canonical(value: &Type) -> Type {
    match value {
        Type::Literal(text) if text.starts_with('\'') && text.ends_with('\'') => {
            crate::enum_eval::decode_plain_string(text)
                .map(|value| Type::Literal(serde_json::to_string(&value).unwrap()))
                .unwrap_or_else(|| value.clone())
        }
        Type::Union(parts) => Type::Union(parts.iter().map(canonical).collect()),
        Type::Array(element) => Type::Array(Box::new(canonical(element))),
        Type::Record(fields) => Type::Record(
            fields
                .iter()
                .map(|field| {
                    let mut field = field.clone();
                    field.value = canonical(&field.value);
                    field.accessor_write_type = field
                        .accessor_write_type
                        .as_ref()
                        .map(|value| Box::new(canonical(value)));
                    field
                })
                .collect(),
        ),
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| {
                    let mut element = element.clone();
                    element.annotation = canonical(&element.annotation);
                    element
                })
                .collect(),
        ),
        Type::Named { name, arguments } => Type::Named {
            name: name.clone(),
            arguments: arguments.iter().map(canonical).collect(),
        },
        Type::Function { parameters, result } => Type::Function {
            parameters: parameters.clone(),
            result: Box::new(canonical(result)),
        },
        other => other.clone(),
    }
}
