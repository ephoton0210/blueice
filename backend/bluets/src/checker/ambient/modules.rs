// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Named declaration surfaces retain their owners and supply type queries only.

use super::*;
use crate::checker::module::ExportedValue;

pub(in crate::checker) fn seed_module_types(
    project: &Project,
    exports: &mut ProjectExports,
    diagnostics: &mut Vec<Diagnostic>,
    enforce_types: bool,
    max_type_expansions: usize,
) -> BTreeMap<String, BTreeMap<String, Type>> {
    let mut values = BTreeMap::new();
    let mut budget = TypeExpansionBudget::new(max_type_expansions);
    for (target, bodies) in project.ambient_modules() {
        let mut surface = AmbientDeclarations {
            enforce_types,
            ..AmbientDeclarations::default()
        };
        let mut functions: BTreeMap<String, Vec<Type>> = BTreeMap::new();
        for body in bodies {
            if !budget.consume() {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    body.span.clone(),
                    "ambient module surfaces exceed the type-expansion limit",
                ));
                return values;
            }
            insert_body(project, &body.body, &mut surface, diagnostics);
            for declaration in &body.body {
                if let Declaration::Function(function) = declaration {
                    let result = Box::new(function.return_type.clone().unwrap_or(Type::Unknown));
                    let value = if function.type_parameters.is_empty() {
                        Type::Function {
                            parameters: function.parameters.clone(),
                            result,
                        }
                    } else {
                        Type::GenericFunction {
                            type_parameters: function.type_parameters.clone(),
                            parameters: function.parameters.clone(),
                            result,
                            span: function.type_parameters[0].span.clone(),
                        }
                    };
                    functions
                        .entry(function.name.clone())
                        .or_default()
                        .push(value);
                }
            }
        }
        for (name, mut signatures) in functions {
            let value = if signatures.len() == 1 {
                signatures.pop().expect("one ambient function")
            } else {
                Type::Intersection(signatures)
            };
            surface.values.insert(name, value);
        }
        exports.exported_names.insert(
            target.clone(),
            (
                surface
                    .types
                    .keys()
                    .chain(surface.values.keys())
                    .cloned()
                    .collect(),
                false,
            ),
        );
        exports.types.insert(target.clone(), surface.types);
        values.insert(target, surface.values);
    }
    values
}

pub(in crate::checker) fn seed_module_values(
    exports: &mut ProjectExports,
    ambient: &AmbientDeclarations,
    values: BTreeMap<String, BTreeMap<String, Type>>,
) {
    for (target, entries) in values {
        let context = exports.types[&target]
            .keys()
            .map(|name| {
                (
                    name.clone(),
                    Type::Named {
                        name: contexts::private_name(&target, name),
                        arguments: Vec::new(),
                    },
                )
            })
            .collect();
        let entries = entries
            .into_iter()
            .map(|(name, value)| {
                let value = substitute_type(&value, &context);
                let metadata =
                    ExportedValue::static_declaration(&name, value, ambient.private_types.clone());
                (name, metadata)
            })
            .collect();
        exports.values.insert(target, entries);
    }
}
