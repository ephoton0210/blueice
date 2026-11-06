// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit primitive-array allocations share the source trace with literals.
use super::*;

pub(super) fn key(value: &Type) -> Option<Key> {
    match value {
        Type::Literal(text) => literal(text),
        Type::Array(element)
            if matches!(
                element.as_ref(),
                Type::String | Type::Number | Type::Boolean | Type::BigInt | Type::Symbol
            ) =>
        {
            Some(Key::Array(type_to_ts(element)))
        }
        _ => None,
    }
}

pub(super) fn variables(module: &Module) -> Vec<(usize, Key)> {
    fn variable(variable: &VariableDeclaration, result: &mut Vec<(usize, Key)>) {
        if let Some(key) = variable.annotation.as_ref().and_then(key) {
            result.push((variable.span.start, key));
        }
    }
    fn declarations(items: &[Declaration], result: &mut Vec<(usize, Key)>) {
        for item in items {
            match item {
                Declaration::Variable(item) => variable(item, result),
                Declaration::Function(function) => {
                    for local in &function.locals {
                        variable(local, result);
                    }
                }
                Declaration::Namespace(namespace) => declarations(&namespace.body, result),
                _ => {}
            }
        }
    }
    let mut result = Vec::new();
    declarations(&module.declarations, &mut result);
    for function in module.nested_functions.values() {
        if let NestedFunctionBody::Block { locals, .. } = &function.body {
            for local in locals {
                variable(local, &mut result);
            }
        }
    }
    result
}

pub(super) fn inferred_array(value: &mut Type, ranks: &Ranks) {
    let Type::Array(element) = value else { return };
    let Type::Union(parts) = element.as_mut() else {
        return;
    };
    let Some(keys) = parts.iter().map(key).collect::<Option<Vec<_>>>() else {
        return;
    };
    if keys.iter().all(|key| ranks.contains_key(key))
        && keys.iter().any(|key| matches!(key, Key::Array(_)))
    {
        parts.sort_by_key(|value| ranks[&key(value).expect("proven allocation key")]);
    }
}
