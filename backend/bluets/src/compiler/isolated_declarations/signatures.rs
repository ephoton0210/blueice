// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Instantiated record members cannot reuse a private signature's type binder.
use super::*;
use crate::checker::CheckedModule;
use crate::parser::Type;
use std::collections::BTreeMap;

fn key(span: &SourceSpan) -> (&str, usize, usize) {
    (&span.module, span.start, span.end)
}

pub(super) fn check(checked: &CheckedModule, out: &mut Vec<Diagnostic>) {
    let module = &checked.module;
    let mut candidates = BTreeMap::new();
    for declaration in &module.declarations {
        let Declaration::Function(function) = declaration else {
            continue;
        };
        if let Some(Type::Record(fields)) = &function.return_type {
            for field in fields {
                if matches!(&field.value, Type::Named { name, arguments }
                    if arguments.is_empty() && function.type_parameters.iter().any(|p| p.name == *name))
                {
                    candidates.insert(key(&field.span), field);
                }
            }
        }
    }
    if candidates.is_empty() {
        return;
    }
    let values: BTreeMap<_, _> = checked
        .symbols
        .iter()
        .filter_map(|symbol| {
            symbol
                .value_type
                .as_ref()
                .map(|value| (key(&symbol.span), value))
        })
        .collect();
    let mut reported = BTreeSet::new();
    for declaration in &module.declarations {
        let Declaration::Variable(variable) = declaration else {
            continue;
        };
        if variable.annotation.is_some() || !variable.exported {
            continue;
        }
        let Some(value) = values.get(&key(&variable.span)) else {
            continue;
        };
        let Type::Record(fields) = value else {
            continue;
        };
        for field in fields {
            let Some(original) = candidates.get(&key(&field.span)) else {
                continue;
            };
            if reported.insert(original.span.start) {
                let location = name_span(module, &original.span, &original.name);
                out.push(
                    Diagnostic::error(
                        DiagnosticCode::TypeMismatch,
                        location,
                        "an instantiated declaration member requires syntactic type inference",
                    )
                    .with_typescript(9013, Vec::new()),
                );
            }
        }
    }
}
