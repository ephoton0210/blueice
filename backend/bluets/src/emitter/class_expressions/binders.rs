// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fresh declaration names preserve captured parameters under method binders.

use super::*;

pub(in crate::emitter) fn rename_shadowed_binders(value: &Type, outer: &[TypeParameter]) -> Type {
    let used = outer
        .iter()
        .map(|parameter| parameter.name.clone())
        .collect();
    rename(value, &used)
}

fn rename(value: &Type, used: &BTreeSet<String>) -> Type {
    if let Type::GenericFunction {
        type_parameters,
        parameters,
        result,
        span,
    } = value
    {
        let mut used = used.clone();
        let mut substitutions = BTreeMap::new();
        let mut binders = type_parameters.clone();
        for parameter in &mut binders {
            let source = crate::parser::source_type_name(&parameter.name).to_string();
            let mut name = source.clone();
            let mut index = 1;
            while used.contains(&name) {
                name = format!("{source}_{index}");
                index += 1;
            }
            used.insert(name.clone());
            let reference = Type::Named {
                name: name.clone(),
                arguments: Vec::new(),
            };
            substitutions.insert(parameter.name.clone(), reference.clone());
            substitutions.insert(crate::parser::type_parameter_identity(parameter), reference);
            parameter.name = name;
        }
        let child = |value: &Type| rename(&substitute_type(value, &substitutions), &used);
        for parameter in &mut binders {
            parameter.constraint = parameter.constraint.as_ref().map(child);
            parameter.default = parameter.default.as_ref().map(child);
        }
        return Type::GenericFunction {
            type_parameters: binders,
            parameters: parameters
                .iter()
                .map(|parameter| Parameter {
                    annotation: parameter.annotation.as_ref().map(child),
                    ..parameter.clone()
                })
                .collect(),
            result: Box::new(child(result)),
            span: span.clone(),
        };
    }
    let child = |value: &Type| rename(value, used);
    if let Some(mapped) = value.map_operator(|value, _| child(value)) {
        return mapped;
    }
    match value {
        Type::CallableRecord { fields, signatures } => Type::CallableRecord {
            fields: fields
                .iter()
                .map(|field| TypeField {
                    accessor_write_type: field
                        .accessor_write_type
                        .as_ref()
                        .map(|value| Box::new(child(value))),
                    value: child(&field.value),
                    ..field.clone()
                })
                .collect(),
            signatures: signatures
                .iter()
                .map(|signature| signature.map_types(child))
                .collect(),
        },
        Type::Record(fields) => Type::Record(
            fields
                .iter()
                .map(|field| TypeField {
                    accessor_write_type: field
                        .accessor_write_type
                        .as_ref()
                        .map(|value| Box::new(child(value))),
                    value: child(&field.value),
                    ..field.clone()
                })
                .collect(),
        ),
        Type::Function { parameters, result } => Type::Function {
            parameters: parameters
                .iter()
                .map(|parameter| Parameter {
                    annotation: parameter.annotation.as_ref().map(child),
                    ..parameter.clone()
                })
                .collect(),
            result: Box::new(child(result)),
        },
        Type::Named { name, arguments } => Type::Named {
            name: name.clone(),
            arguments: arguments.iter().map(child).collect(),
        },
        Type::Array(value) => Type::Array(Box::new(child(value))),
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| crate::parser::TupleTypeElement {
                    annotation: child(&element.annotation),
                    ..element.clone()
                })
                .collect(),
        ),
        Type::Union(options) => Type::Union(options.iter().map(child).collect()),
        Type::Intersection(options) => Type::Intersection(options.iter().map(child).collect()),
        other => other.clone(),
    }
}
