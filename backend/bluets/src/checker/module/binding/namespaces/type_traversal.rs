// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Qualified type traversal preserves callable binders and original spans.

use super::*;

/// Every named type a type mentions.
pub(super) fn named_types(value: &Type, into: &mut BTreeSet<String>) {
    if let Some(children) = value.operator_children() {
        let mut names = BTreeSet::new();
        for child in children {
            named_types(child, &mut names);
        }
        if let Type::Mapped(value) = value {
            names.remove(&value.parameter.name);
        }
        if let Type::Conditional(value) = value {
            for parameter in value.extends.infer_parameters() {
                names.remove(&parameter.name);
            }
        }
        into.extend(names);
        return;
    }
    match value {
        Type::CallableRecord { fields, signatures } => {
            named_types(&Type::Record(fields.clone()), into);
            for signature in signatures {
                named_types(&signature.function_type(), into);
            }
        }
        Type::Named { name, arguments } => {
            into.insert(name.clone());
            for argument in arguments {
                named_types(argument, into);
            }
        }
        Type::Literal(text) => {
            into.insert(text.clone());
        }
        Type::Array(element) => named_types(element, into),
        Type::Tuple(elements) => {
            for element in elements {
                named_types(&element.annotation, into);
            }
        }
        Type::Record(fields) => {
            for field in fields {
                named_types(&field.value, into);
            }
        }
        Type::Predicate(predicate) => {
            if let Some(target) = &predicate.target {
                named_types(target, into);
            }
        }
        Type::KeyOf(value) => named_types(value, into),
        Type::IndexedAccess { object, index, .. } => {
            named_types(object, into);
            named_types(index, into);
        }
        Type::GenericFunction {
            type_parameters,
            parameters,
            result,
            ..
        } => {
            let mut local = BTreeSet::new();
            named_types(
                &Type::Function {
                    parameters: parameters.clone(),
                    result: result.clone(),
                },
                &mut local,
            );
            for parameter in type_parameters {
                if let Some(constraint) = &parameter.constraint {
                    named_types(constraint, &mut local);
                }
                if let Some(default) = &parameter.default {
                    named_types(default, &mut local);
                }
            }
            for parameter in type_parameters {
                local.remove(&parameter.name);
            }
            into.extend(local);
        }
        Type::Function { parameters, result } => {
            for parameter in parameters {
                if let Some(annotation) = &parameter.annotation {
                    named_types(annotation, into);
                }
            }
            named_types(result, into);
        }
        Type::Union(options) | Type::Intersection(options) => {
            for option in options {
                named_types(option, into);
            }
        }
        _ => {}
    }
}

/// Renames type keys to their qualified form inside a type.
pub(super) struct Qualifier<'r> {
    pub(super) rename: &'r BTreeMap<String, String>,
}

impl Qualifier<'_> {
    fn name(&self, name: &str) -> String {
        self.rename
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    pub(super) fn ty(&self, value: &Type) -> Type {
        if let Some(value) = value.map_operator(|value, bound| {
            let rename = self
                .rename
                .iter()
                .filter(|(name, _)| !bound.contains(name))
                .map(|(name, target)| (name.clone(), target.clone()))
                .collect();
            Qualifier { rename: &rename }.ty(value)
        }) {
            return value;
        }
        match value {
            Type::CallableRecord { fields, signatures } => {
                let Type::Record(fields) = self.ty(&Type::Record(fields.clone())) else {
                    unreachable!()
                };
                Type::CallableRecord {
                    fields,
                    signatures: signatures
                        .iter()
                        .map(|signature| signature.map_types(|value| self.ty(value)))
                        .collect(),
                }
            }
            Type::Predicate(predicate) => {
                let mut predicate = predicate.clone();
                predicate.target = predicate
                    .target
                    .as_ref()
                    .map(|target| Box::new(self.ty(target)));
                Type::Predicate(predicate)
            }
            Type::KeyOf(value) => Type::KeyOf(Box::new(self.ty(value))),
            Type::IndexedAccess {
                object,
                index,
                index_span,
            } => Type::IndexedAccess {
                object: Box::new(self.ty(object)),
                index: Box::new(self.ty(index)),
                index_span: index_span.clone(),
            },
            Type::GenericFunction {
                type_parameters,
                parameters,
                result,
                span,
            } => {
                let rename = self
                    .rename
                    .iter()
                    .filter(|(name, _)| !type_parameters.iter().any(|p| &p.name == *name))
                    .map(|(name, target)| (name.clone(), target.clone()))
                    .collect();
                let scoped = Qualifier { rename: &rename };
                Type::GenericFunction {
                    type_parameters: scoped.type_parameters(type_parameters),
                    parameters: scoped.parameters(parameters),
                    result: Box::new(scoped.ty(result)),
                    span: span.clone(),
                }
            }
            Type::Named { name, arguments } => Type::Named {
                name: self.name(name),
                arguments: arguments.iter().map(|argument| self.ty(argument)).collect(),
            },
            Type::Literal(text) => Type::Literal(self.name(text)),
            Type::Array(element) => Type::Array(Box::new(self.ty(element))),
            Type::Tuple(elements) => Type::Tuple(
                elements
                    .iter()
                    .map(|element| TupleTypeElement {
                        annotation: self.ty(&element.annotation),
                        ..element.clone()
                    })
                    .collect(),
            ),
            Type::Record(fields) => Type::Record(
                fields
                    .iter()
                    .map(|field| TypeField {
                        value: self.ty(&field.value),
                        ..field.clone()
                    })
                    .collect(),
            ),
            Type::Function { parameters, result } => Type::Function {
                parameters: self.parameters(parameters),
                result: Box::new(self.ty(result)),
            },
            Type::Union(options) => {
                Type::Union(options.iter().map(|option| self.ty(option)).collect())
            }
            Type::Intersection(options) => {
                Type::Intersection(options.iter().map(|option| self.ty(option)).collect())
            }
            other => other.clone(),
        }
    }

    pub(super) fn parameters(&self, parameters: &[Parameter]) -> Vec<Parameter> {
        parameters
            .iter()
            .map(|parameter| Parameter {
                annotation: parameter.annotation.as_ref().map(|value| self.ty(value)),
                ..parameter.clone()
            })
            .collect()
    }

    pub(super) fn type_parameters(&self, parameters: &[TypeParameter]) -> Vec<TypeParameter> {
        parameters
            .iter()
            .map(|parameter| TypeParameter {
                constraint: parameter.constraint.as_ref().map(|value| self.ty(value)),
                default: parameter.default.as_ref().map(|value| self.ty(value)),
                ..parameter.clone()
            })
            .collect()
    }

    pub(super) fn signatures(&self, signatures: &[FunctionSignature]) -> Vec<FunctionSignature> {
        signatures
            .iter()
            .map(|signature| FunctionSignature {
                parameters: self.parameters(&signature.parameters),
                type_parameters: self.type_parameters(&signature.type_parameters),
                return_type: self.ty(&signature.return_type),
            })
            .collect()
    }

    pub(super) fn definition(&self, definition: &TypeDefinition) -> TypeDefinition {
        TypeDefinition {
            kind: definition.kind,
            parameters: self.type_parameters(&definition.parameters),
            value: self.ty(&definition.value),
        }
    }
}
