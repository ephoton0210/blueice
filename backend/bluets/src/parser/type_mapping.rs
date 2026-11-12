// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Map immediate type children with their lexical parameter bindings intact.

use super::*;

impl Type {
    pub(crate) fn map_children(&self, mut map: impl FnMut(&Type, &[String]) -> Type) -> Type {
        if let Some(value) = self.map_operator(&mut map) {
            return value;
        }
        match self {
            Self::KeyOf(value) => Self::KeyOf(Box::new(map(value, &[]))),
            Self::IndexedAccess {
                object,
                index,
                index_span,
            } => Self::IndexedAccess {
                object: Box::new(map(object, &[])),
                index: Box::new(map(index, &[])),
                index_span: index_span.clone(),
            },
            Self::Named { name, arguments } => Self::Named {
                name: name.clone(),
                arguments: arguments.iter().map(|value| map(value, &[])).collect(),
            },
            Self::Array(value) => Self::Array(Box::new(map(value, &[]))),
            Self::Tuple(values) => Self::Tuple(
                values
                    .iter()
                    .map(|value| TupleTypeElement {
                        annotation: map(&value.annotation, &[]),
                        ..value.clone()
                    })
                    .collect(),
            ),
            Self::Record(fields) => Self::Record(
                fields
                    .iter()
                    .map(|field| TypeField {
                        value: map(&field.value, &[]),
                        accessor_write_type: field
                            .accessor_write_type
                            .as_ref()
                            .map(|value| Box::new(map(value, &[]))),
                        ..field.clone()
                    })
                    .collect(),
            ),
            Self::CallableRecord { fields, signatures } => {
                let fields = fields
                    .iter()
                    .map(|field| TypeField {
                        value: map(&field.value, &[]),
                        accessor_write_type: field
                            .accessor_write_type
                            .as_ref()
                            .map(|value| Box::new(map(value, &[]))),
                        ..field.clone()
                    })
                    .collect();
                let signatures = signatures
                    .iter()
                    .map(|signature| signature.map_types(|value| map(value, &[])))
                    .collect();
                Self::CallableRecord { fields, signatures }
            }
            Self::Function { parameters, result } => Self::Function {
                parameters: parameters
                    .iter()
                    .map(|parameter| Parameter {
                        annotation: parameter.annotation.as_ref().map(|value| map(value, &[])),
                        ..parameter.clone()
                    })
                    .collect(),
                result: Box::new(map(result, &[])),
            },
            Self::GenericFunction {
                type_parameters,
                parameters,
                result,
                span,
            } => {
                let bound = type_parameters
                    .iter()
                    .flat_map(|parameter| {
                        [parameter.name.clone(), type_parameter_identity(parameter)]
                    })
                    .collect::<Vec<_>>();
                Self::GenericFunction {
                    type_parameters: type_parameters
                        .iter()
                        .map(|parameter| TypeParameter {
                            constraint: parameter
                                .constraint
                                .as_ref()
                                .map(|value| map(value, &bound)),
                            default: parameter.default.as_ref().map(|value| map(value, &bound)),
                            ..parameter.clone()
                        })
                        .collect(),
                    parameters: parameters
                        .iter()
                        .map(|parameter| Parameter {
                            annotation: parameter
                                .annotation
                                .as_ref()
                                .map(|value| map(value, &bound)),
                            ..parameter.clone()
                        })
                        .collect(),
                    result: Box::new(map(result, &bound)),
                    span: span.clone(),
                }
            }
            Self::Predicate(predicate) => {
                let mut predicate = predicate.clone();
                predicate.target = predicate
                    .target
                    .as_ref()
                    .map(|value| Box::new(map(value, &[])));
                Self::Predicate(predicate)
            }
            Self::Union(values) => {
                Self::Union(values.iter().map(|value| map(value, &[])).collect())
            }
            Self::Intersection(values) => {
                Self::Intersection(values.iter().map(|value| map(value, &[])).collect())
            }
            value => value.clone(),
        }
    }
}
