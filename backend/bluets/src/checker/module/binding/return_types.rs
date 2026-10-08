// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Publication of inferred function and class return signatures.

use super::*;
use crate::parser::ClassDeclaration;

impl ModuleChecker<'_> {
    pub(super) fn infer_module_return_signatures(&mut self) {
        // Initializers contribute closed-over value types without replacing an
        // explicit annotation. Checking those initializers remains a separate step.
        let mut scope = self.values.clone();
        for declaration in &self.module.declarations {
            if let Declaration::Variable(variable) = declaration {
                let value = variable
                    .annotation
                    .clone()
                    .unwrap_or_else(|| self.infer_variable_type(variable, &scope));
                scope.insert(variable.name.clone(), value);
            }
        }
        self.values = scope.clone();
        for declaration in &self.module.declarations {
            if let Declaration::Function(function) = declaration {
                if function.return_type.is_some() || function.overload || function.declared {
                    continue;
                }
                let result = self.inferred_function_result(function, &scope, false);
                let parameters = self.return_parameters(&function.parameters, &scope);
                if !self.module.declarations.iter().any(|declaration| {
                    matches!(declaration, Declaration::Function(other) if other.name == function.name && other.overload)
                }) {
                    if let Some(signatures) = self.functions.get_mut(&function.name) {
                        for signature in signatures {
                            signature.parameters = parameters.clone();
                            signature.return_type = result.clone();
                        }
                    }
                }
                self.values.insert(function.name.clone(), result.clone());
                scope.insert(function.name.clone(), result.clone());
                for symbol in &mut self.symbols {
                    if symbol.kind == SymbolKind::Function && symbol.span == function.span {
                        symbol.value_type = Some(result.clone());
                    }
                }
            }
        }
        for declaration in &self.module.declarations {
            if let Declaration::Class(class) = declaration {
                let inferred = self.class_with_inferred_returns(class);
                let instance = class_instance_type(&inferred);
                if let Some(definition) = self.types.get_mut(&class.name) {
                    definition.value = instance;
                }
                self.values.insert(
                    class.name.clone(),
                    classes::class_constructor_side_type(&inferred),
                );
            }
        }
    }

    pub(super) fn class_with_inferred_returns(
        &mut self,
        class: &ClassDeclaration,
    ) -> ClassDeclaration {
        let resolved = self.class_with_resolved_heritage(class);
        let class = &resolved;
        self.with_class_access(class, |checker| checker.infer_class_return_types(class))
    }

    fn infer_class_return_types(&self, class: &ClassDeclaration) -> ClassDeclaration {
        let mut inferred = class.clone();
        for member in &mut inferred.members {
            let (parameters, body, result, span, is_static, type_parameters) =
                if let Some(method) = &mut member.method {
                    let Some(body) = &method.body else { continue };
                    (
                        &mut method.parameters,
                        body,
                        &mut method.return_type,
                        &method.span,
                        method.is_static,
                        method.type_parameters.clone(),
                    )
                } else if let Some(accessor) = &mut member.accessor {
                    if !accessor.getter {
                        continue;
                    }
                    (
                        &mut accessor.parameters,
                        &accessor.body,
                        &mut accessor.return_type,
                        &accessor.span,
                        accessor.is_static,
                        Vec::new(),
                    )
                } else {
                    continue;
                };
            let mut scope = self.values.clone();
            scope.insert(
                "this".to_string(),
                if is_static {
                    self.values
                        .get(&class.name)
                        .cloned()
                        .unwrap_or(Type::Unknown)
                } else {
                    super::classes::class_body_this_type(class)
                },
            );
            if let Some(base) = &class.extends_name {
                scope.insert(
                    "super".to_string(),
                    Type::Named {
                        name: if is_static {
                            format!("typeof {base}")
                        } else {
                            base.clone()
                        },
                        arguments: if is_static {
                            Vec::new()
                        } else {
                            class.extends_arguments.clone()
                        },
                    },
                );
            }
            *parameters = self.return_parameters(parameters, &scope);
            if result.is_some() {
                continue;
            }
            let function = FunctionDeclaration {
                name: "<method>".to_string(),
                async_function: false,
                generator: false,
                body_open: None,
                type_parameters,
                parameters: parameters.clone(),
                return_type: None,
                body: body.clone(),
                returns: Vec::new(),
                locals: Vec::new(),
                exported: false,
                default_export: false,
                declared: false,
                overload: false,
                span: span.clone(),
            };
            let value = self.inferred_function_result(&function, &scope, false);
            let explicit_unknown = value == Type::Unknown && body.iter().any(|item| {
                let FunctionBodyItem::Return { tokens, .. } = item else { return false; };
                match tokens.as_slice() {
                    [receiver, dot, property] if receiver.is("this") && dot.is(".") => class.members.iter()
                        .filter_map(|member| member.field.as_ref())
                        .any(|field| field.name == property.text && field.annotation == Some(Type::Unknown)),
                    [name] => parameters.iter().any(|parameter| parameter.name == name.text && parameter.annotation == Some(Type::Unknown))
                        || body.iter().any(|item| matches!(item, FunctionBodyItem::Variable(variable)
                            if variable.name == name.text && variable.annotation == Some(Type::Unknown))),
                    _ => false,
                }
            });
            if value != Type::Unknown || explicit_unknown {
                if explicit_unknown {
                    self.return_inference
                        .results
                        .borrow_mut()
                        .insert(span.start, value.clone());
                }
                *result = Some(value);
            }
        }
        let getters = inferred
            .members
            .iter()
            .filter_map(|member| member.accessor.as_ref())
            .filter(|accessor| accessor.getter)
            .map(|accessor| {
                (
                    (accessor.is_static, accessor.name.clone()),
                    accessor.return_type.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        for setter in inferred
            .members
            .iter_mut()
            .filter_map(|member| member.accessor.as_mut())
            .filter(|accessor| !accessor.getter)
        {
            if let Some(parameter) = setter
                .parameters
                .first_mut()
                .filter(|parameter| parameter.annotation.is_none())
            {
                if let Some(Some(value)) = getters.get(&(setter.is_static, setter.name.clone())) {
                    parameter.annotation = Some(value.clone());
                    self.return_inference
                        .parameters
                        .borrow_mut()
                        .insert(parameter.span.start, value.clone());
                }
            }
        }
        inferred
    }
}
