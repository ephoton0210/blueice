// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bind trusted declarations after source and owner bindings, without symbols.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn bind_standard_library(&mut self) {
        let protected_types: BTreeSet<_> = self.types.keys().cloned().collect();
        let protected_values: BTreeSet<_> = self.values.keys().cloned().collect();
        for module in crate::standard_library::modules(self.target) {
            for declaration in &module.declarations {
                match declaration {
                    Declaration::Interface(interface)
                        if !protected_types.contains(&interface.name) =>
                    {
                        let mut value = interface_value(interface);
                        if self.explicit_checking
                            && interface.name == "Array"
                            && !self.types.contains_key("Array")
                        {
                            if let Type::Record(fields) = &mut value {
                                fields.push(TypeField {
                                    accessor_write_type: None,
                                    method: false,
                                    name: "values".to_string(),
                                    readonly: false,
                                    optional: false,
                                    span: interface.span.clone(),
                                    value: Type::Function {
                                        parameters: Vec::new(),
                                        result: Box::new(Type::Named {
                                            name: "IterableIterator".to_string(),
                                            arguments: vec![
                                                Type::Named {
                                                    name: "T".to_string(),
                                                    arguments: Vec::new(),
                                                },
                                                if self.checking.strict_builtin_iterator_return {
                                                    Type::Undefined
                                                } else {
                                                    Type::Any
                                                },
                                            ],
                                        }),
                                    },
                                });
                            }
                        }
                        if let Some(existing) = self.types.get_mut(&interface.name) {
                            merge_records(&mut existing.value, value);
                        } else {
                            self.types.insert(
                                interface.name.clone(),
                                TypeDefinition {
                                    kind: TypeDefinitionKind::LibraryInterface,
                                    parameters: interface.type_parameters.clone(),
                                    value,
                                },
                            );
                        }
                    }
                    Declaration::TypeAlias(alias) if !protected_types.contains(&alias.name) => {
                        self.types.insert(
                            alias.name.clone(),
                            TypeDefinition {
                                kind: TypeDefinitionKind::Alias,
                                parameters: alias.type_parameters.clone(),
                                value: alias.value.clone(),
                            },
                        );
                    }
                    Declaration::Variable(variable)
                        if !self.require_declared_global_calls
                            && !protected_values.contains(&variable.name) =>
                    {
                        let value = variable.annotation.clone().unwrap_or(Type::Unknown);
                        if let Some(existing) = self.values.get_mut(&variable.name) {
                            merge_records(existing, value);
                        } else {
                            self.values.insert(variable.name.clone(), value);
                        }
                    }
                    Declaration::Function(function)
                        if !self.require_declared_global_calls
                            && !protected_values.contains(&function.name) =>
                    {
                        if let Some(value @ Type::Record(_)) = self.values.get_mut(&function.name) {
                            *value = Type::Intersection(vec![
                                value.clone(),
                                Type::Function {
                                    parameters: function.parameters.clone(),
                                    result: Box::new(
                                        function.return_type.clone().unwrap_or(Type::Unknown),
                                    ),
                                },
                            ]);
                        }
                        self.values.entry(function.name.clone()).or_insert_with(|| {
                            function.return_type.clone().unwrap_or(Type::Unknown)
                        });
                        self.functions
                            .entry(function.name.clone())
                            .or_default()
                            .push(signature(function));
                    }
                    Declaration::Namespace(namespace)
                        if !self.require_declared_global_calls
                            && !protected_values.contains(&namespace.name) =>
                    {
                        let mut fields = Vec::new();
                        for declaration in &namespace.body {
                            if let Declaration::Variable(variable) = declaration {
                                fields.push(TypeField {
                                    accessor_write_type: None,
                                    method: false,
                                    name: variable.name.clone(),
                                    readonly: variable.kind == crate::parser::VariableKind::Const,
                                    optional: false,
                                    span: variable.span.clone(),
                                    value: variable.annotation.clone().unwrap_or(Type::Unknown),
                                });
                                continue;
                            }
                            let Declaration::Function(function) = declaration else {
                                continue;
                            };
                            let signature = signature(function);
                            fields.push(TypeField {
                                accessor_write_type: None,
                                method: false,
                                name: function.name.clone(),
                                readonly: false,
                                optional: false,
                                span: function.span.clone(),
                                value: Type::Function {
                                    parameters: signature.parameters.clone(),
                                    result: Box::new(signature.return_type.clone()),
                                },
                            });
                            self.functions
                                .entry(format!("{}.{}", namespace.name, function.name))
                                .or_default()
                                .push(signature);
                        }
                        if let Some(existing) = self.values.get_mut(&namespace.name) {
                            merge_records(existing, Type::Record(fields));
                        } else {
                            self.values
                                .insert(namespace.name.clone(), Type::Record(fields));
                        }
                        self.module_namespace_imports.insert(namespace.name.clone());
                    }
                    _ => {}
                }
            }
        }
        // BlueTS currently represents these primitive keywords as named types.
        // Their adapters consume the original declarations above, without API names.
        self.types
            .entry("symbol".to_string())
            .or_insert(TypeDefinition {
                kind: TypeDefinitionKind::Alias,
                parameters: Vec::new(),
                value: Type::Named {
                    name: "Symbol".to_string(),
                    arguments: Vec::new(),
                },
            });
        self.types
            .entry("object".to_string())
            .or_insert(TypeDefinition {
                kind: TypeDefinitionKind::Alias,
                parameters: Vec::new(),
                value: Type::Record(Vec::new()),
            });
        self.values
            .entry("undefined".to_string())
            .or_insert(Type::Undefined);
        self.library_values.extend(
            self.values
                .keys()
                .filter(|name| !protected_values.contains(*name))
                .cloned(),
        );
        // Constructor parameter lists are parsed function-type descriptors:
        // BlueTS does not admit `declare class` or a construct signature yet.
        if !self.require_declared_global_calls {
            for name in self.values.keys().cloned().collect::<Vec<_>>() {
                if protected_values.contains(&name) || protected_types.contains(&name) {
                    continue;
                }
                let Some(TypeDefinition {
                    value: Type::Function { parameters, result },
                    ..
                }) = self.types.get(&format!("{name}Constructor"))
                else {
                    continue;
                };
                let binding = ClassConstructorBinding {
                    modifiers: ClassModifierSurface::default(),
                    signatures: vec![FunctionSignature {
                        parameters: parameters.clone(),
                        type_parameters: Vec::new(),
                        return_type: (**result).clone(),
                    }],
                    inherited: false,
                    visibility: crate::parser::Visibility::Public,
                };
                self.class_constructors.insert(name.clone(), binding);
                if let Some(definition) = self.types.get_mut(&name) {
                    definition.kind = TypeDefinitionKind::Class;
                }
            }
        }
    }
}

fn signature(function: &FunctionDeclaration) -> FunctionSignature {
    FunctionSignature {
        parameters: function.parameters.clone(),
        type_parameters: function.type_parameters.clone(),
        return_type: function.return_type.clone().unwrap_or(Type::Unknown),
    }
}

fn merge_records(existing: &mut Type, value: Type) {
    if let (Type::Record(existing), Type::Record(fields)) = (existing, value) {
        existing.extend(fields);
    }
}
