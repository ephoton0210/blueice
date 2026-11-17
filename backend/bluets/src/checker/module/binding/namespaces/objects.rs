// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Namespace values retain callable and constructor surfaces during merging.

use super::*;
use crate::parser::{TypeSignature, Visibility};

impl ModuleChecker<'_> {
    pub(super) fn register_namespace_object(
        &mut self,
        namespace: &NamespaceDeclaration,
        path: &str,
    ) {
        let local = &namespace.name;
        let object = format!("typeof {path}");
        let own_object = self
            .types
            .get(&object)
            .is_some_and(|definition| definition.kind == TypeDefinitionKind::NamespaceObject);
        let fields = self.namespace_object_fields(namespace, path);
        if self.values.contains_key(local) && !own_object {
            if let Some(signatures) = self.functions.get(local) {
                let span = self
                    .project
                    .modules
                    .get(&namespace.span.module)
                    .and_then(|module| {
                        module
                            .declarations
                            .iter()
                            .find_map(|declaration| match declaration {
                                Declaration::Function(function) if function.name == *local => {
                                    Some(function.span.clone())
                                }
                                _ => None,
                            })
                    })
                    .unwrap_or_else(|| namespace.name_span.clone());
                let value = Type::CallableRecord {
                    fields,
                    signatures: signatures
                        .iter()
                        .map(|signature| TypeSignature {
                            construct: false,
                            abstract_constructor: false,
                            constructor_visibility: Visibility::Public,
                            constructor_arrow: false,
                            type_parameters: signature.type_parameters.clone(),
                            parameters: signature.parameters.clone(),
                            result: signature.return_type.clone(),
                            span: span.clone(),
                        })
                        .collect(),
                };
                self.values.insert(path.to_string(), value.clone());
                self.values.insert(local.clone(), value);
            } else {
                let Some(value) = self.values.get_mut(local) else {
                    return;
                };
                if let Type::Named { name, .. } = value {
                    // Enum values refer to their existing object definition.
                    if let Some(definition) = self.types.get_mut(name) {
                        merge_fields(&mut definition.value, fields);
                    }
                } else {
                    // Retain class constructor signatures, accessibility and
                    // static members while adding the namespace's data fields.
                    merge_fields(value, fields);
                }
            }
            return;
        }
        let value = Type::Named {
            name: object.clone(),
            arguments: Vec::new(),
        };
        self.types.insert(
            object,
            TypeDefinition {
                kind: TypeDefinitionKind::NamespaceObject,
                parameters: Vec::new(),
                value: Type::Record(fields),
            },
        );
        self.values.insert(path.to_string(), value.clone());
        if local != path {
            self.values.insert(local.clone(), value);
        }
    }

    fn namespace_object_fields(
        &self,
        namespace: &NamespaceDeclaration,
        path: &str,
    ) -> Vec<TypeField> {
        let members = self.namespaces.get(path).cloned().unwrap_or_default();
        let mut fields = Vec::new();
        for name in &members.values {
            let key = format!("{path}.{name}");
            let value = match (self.functions.get(&key), self.values.get(&key)) {
                (Some(signatures), _) if !signatures.is_empty() => Type::Function {
                    parameters: signatures[0].parameters.clone(),
                    result: Box::new(signatures[0].return_type.clone()),
                },
                (_, Some(value)) => value.clone(),
                _ => Type::Unknown,
            };
            fields.push(TypeField {
                accessor_write_type: None,
                method: false,
                name: name.clone(),
                readonly: false,
                optional: false,
                value,
                span: namespace.name_span.clone(),
            });
        }
        for name in &members.namespaces {
            let inner = format!("{path}.{name}");
            if self.values.contains_key(&inner) {
                fields.push(TypeField {
                    accessor_write_type: None,
                    method: false,
                    name: name.clone(),
                    readonly: true,
                    optional: false,
                    value: Type::Named {
                        name: format!("typeof {inner}"),
                        arguments: Vec::new(),
                    },
                    span: namespace.name_span.clone(),
                });
            }
        }
        fields
    }
}

fn merge_fields(value: &mut Type, incoming: Vec<TypeField>) {
    let (Type::Record(fields) | Type::CallableRecord { fields, .. }) = value.object_type_mut()
    else {
        return;
    };
    for field in incoming {
        if let Some(existing) = fields
            .iter_mut()
            .find(|existing| existing.name == field.name)
        {
            if existing.span == field.span {
                *existing = field;
            }
        } else {
            fields.push(field);
        }
    }
}
