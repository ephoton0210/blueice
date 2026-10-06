// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Constructor, method and static-block bodies.

use super::*;

#[derive(Clone, Copy)]
pub(super) enum ClassBodyReturnRule<'a> {
    Constructor {
        class_name: &'a str,
        instance_type: &'a Type,
        /// The base whose constructor a `super(...)` call must satisfy.
        super_base: Option<&'a str>,
    },
    Method {
        return_type: Option<&'a Type>,
        allows_implicit_undefined: bool,
    },
    /// A `static { .. }` block, which cannot `return`.
    StaticBlock,
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module::binding) fn check_class_constructor_bodies(
        &mut self,
        class: &ClassDeclaration,
    ) {
        let instance_type = self
            .types
            .get(&class.name)
            .filter(|definition| definition.kind == TypeDefinitionKind::Class)
            .map(|definition| definition.value.clone())
            .unwrap_or_else(|| class_instance_type(class));
        for constructor in class
            .members
            .iter()
            .filter_map(|member| member.constructor.as_ref())
        {
            let Some(body) = &constructor.body else {
                continue;
            };
            let mut scope = self.class_body_parameter_scope(&constructor.parameters);
            scope.insert(
                "this".to_string(),
                Type::Named {
                    name: class.name.clone(),
                    arguments: Vec::new(),
                },
            );
            if let Some(base) = self.super_scope_type(class, false) {
                scope.insert("super".to_string(), base);
            }
            hoist_local_functions(body, &mut scope);
            self.check_constructor_super_placement(class, constructor, body);
            self.constructor_readonly_fields = Some(
                class
                    .members
                    .iter()
                    .filter_map(|member| member.field.as_ref())
                    .chain(class.parameter_property_fields().iter())
                    .filter(|field| field.readonly && !field.is_static)
                    .map(|field| field.name.clone())
                    .collect(),
            );
            self.check_class_body_items(
                body,
                &scope,
                ClassBodyReturnRule::Constructor {
                    class_name: &class.name,
                    instance_type: &instance_type,
                    super_base: class.extends_name.as_deref(),
                },
            );
            self.constructor_readonly_fields = None;
        }
    }

    pub(in crate::checker::module::binding) fn check_class_method_bodies(
        &mut self,
        class: &ClassDeclaration,
    ) {
        let constructor_side = self
            .values
            .get(&class.name)
            .cloned()
            .unwrap_or_else(|| class_constructor_side_type(class));
        let accessor_methods = class.accessor_methods();
        for method in class
            .members
            .iter()
            .filter_map(|member| member.method.as_ref())
            .chain(accessor_methods.iter())
        {
            self.validate_class_parameters(&method.parameters, method.body.is_some(), "method");
            let return_type = method.return_type.as_ref().filter(|return_type| {
                let before = self.diagnostics.len();
                self.check_type(
                    return_type,
                    method.return_type_span.as_ref().unwrap_or(&method.span),
                );
                !self.diagnostics[before..]
                    .iter()
                    .any(|diagnostic| diagnostic.code == DiagnosticCode::UnknownType)
            });
            if let Some(body) = &method.body {
                let mut scope = self.class_body_parameter_scope(&method.parameters);
                if let Some(base) = self.super_scope_type(class, method.is_static) {
                    scope.insert("super".to_string(), base);
                }
                self.check_method_super_placement(class, body);
                hoist_local_functions(body, &mut scope);
                scope.insert(
                    "this".to_string(),
                    if method.is_static {
                        constructor_side.clone()
                    } else {
                        Type::Named {
                            name: class.name.clone(),
                            arguments: Vec::new(),
                        }
                    },
                );
                let allows_implicit_undefined = return_type.is_none_or(|return_type| {
                    self.return_type_allows_implicit_undefined(return_type, &method.span)
                });
                self.check_class_body_items(
                    body,
                    &scope,
                    ClassBodyReturnRule::Method {
                        return_type,
                        allows_implicit_undefined,
                    },
                );
                if let Some(return_type) = return_type {
                    if !allows_implicit_undefined
                        && matches!(
                            Self::function_body_termination(body),
                            StructuredTermination::FallsThrough
                        )
                    {
                        self.type_error(
                            &method.span,
                            format!(
                                "class method with return type `{}` can complete without returning a value",
                                type_label(return_type)
                            ),
                            DiagnosticCode::ReturnTypeMismatch,
                        );
                    }
                }
            }
        }
    }

    /// Each `static { .. }` block runs once with `this` bound to the class
    /// constructor. It is a function body that cannot `return` or `await`.
    pub(in crate::checker::module::binding) fn check_class_static_blocks(
        &mut self,
        class: &ClassDeclaration,
    ) {
        let constructor_side = self
            .values
            .get(&class.name)
            .cloned()
            .unwrap_or_else(|| class_constructor_side_type(class));
        for block in class
            .members
            .iter()
            .filter_map(|member| member.static_block.as_ref())
        {
            let mut scope = self.values.clone();
            if let Some(base) = self.super_scope_type(class, true) {
                scope.insert("super".to_string(), base);
            }
            hoist_local_functions(&block.body, &mut scope);
            scope.insert("this".to_string(), constructor_side.clone());
            let previous_async = self.async_context.replace(false);
            self.check_class_body_items(&block.body, &scope, ClassBodyReturnRule::StaticBlock);
            self.async_context = previous_async;
        }
    }

    pub(super) fn class_body_parameter_scope(
        &self,
        parameters: &[Parameter],
    ) -> BTreeMap<String, Type> {
        let mut scope = self.values.clone();
        for parameter in parameters {
            if let Some(pattern) = &parameter.pattern {
                // A destructured class parameter binds its names untyped.
                let names: Vec<&String> = match pattern {
                    BindingPattern::Object(bindings) => {
                        bindings.iter().map(|binding| &binding.name).collect()
                    }
                    BindingPattern::Array(elements) => elements
                        .iter()
                        .flatten()
                        .map(|element| &element.name)
                        .collect(),
                };
                for name in names {
                    scope.insert(name.clone(), Type::Unknown);
                }
                continue;
            }
            let value = parameter
                .annotation
                .clone()
                .or_else(|| {
                    parameter
                        .default
                        .as_ref()
                        .map(|tokens| self.infer_expression(tokens, &scope))
                })
                .unwrap_or(Type::Unknown);
            scope.insert(parameter.name.clone(), value);
        }
        scope
    }

    pub(super) fn check_class_body_items(
        &mut self,
        items: &[FunctionBodyItem],
        scope: &BTreeMap<String, Type>,
        return_rule: ClassBodyReturnRule<'_>,
    ) {
        let mut scope = scope.clone();
        for item in items {
            match item {
                FunctionBodyItem::Variable(variable) => {
                    self.check_variable_in_scope(variable, &scope);
                    let value = variable
                        .annotation
                        .clone()
                        .unwrap_or_else(|| self.infer_expression(&variable.initializer, &scope));
                    scope.insert(variable.name.clone(), value);
                }
                FunctionBodyItem::Expression { tokens, span }
                | FunctionBodyItem::Throw { tokens, span } => {
                    self.check_direct_runtime_expression(tokens, &scope, span);
                    if let (
                        FunctionBodyItem::Expression { .. },
                        ClassBodyReturnRule::Constructor {
                            super_base: Some(base),
                            ..
                        },
                    ) = (item, return_rule)
                    {
                        self.check_super_call_arguments(base, tokens, &scope, span);
                    }
                }
                FunctionBodyItem::Return { tokens, span } => {
                    if !tokens.is_empty() {
                        self.check_direct_runtime_expression(tokens, &scope, span);
                    }
                    match return_rule {
                        ClassBodyReturnRule::Constructor {
                            class_name,
                            instance_type,
                            ..
                        } if !tokens.is_empty() => {
                            let actual = self.infer_expression(tokens, &scope);
                            let primitive = Type::Union(vec![
                                Type::Null,
                                Type::Undefined,
                                Type::Boolean,
                                Type::Number,
                                Type::String,
                            ]);
                            if !self.is_assignable_bounded(&actual, &primitive, span)
                                && !self.is_assignable_bounded(&actual, instance_type, span)
                            {
                                self.type_error(
                                    span,
                                    format!(
                                        "constructor return type `{}` is not assignable to class `{}`",
                                        type_label(&actual),
                                        class_name
                                    ),
                                    DiagnosticCode::ReturnTypeMismatch,
                                );
                            }
                        }
                        ClassBodyReturnRule::Method {
                            return_type: Some(return_type),
                            allows_implicit_undefined,
                        } => {
                            let actual = if tokens.is_empty() {
                                Type::Undefined
                            } else {
                                self.infer_in_context(tokens, &scope, return_type)
                            };
                            if (!tokens.is_empty() || !allows_implicit_undefined)
                                && !self.is_assignable_bounded(&actual, return_type, span)
                            {
                                self.type_error(
                                    span,
                                    format!(
                                        "class method return type `{}` is not assignable to `{}`",
                                        type_label(&actual),
                                        type_label(return_type)
                                    ),
                                    DiagnosticCode::ReturnTypeMismatch,
                                );
                            }
                        }
                        ClassBodyReturnRule::StaticBlock => self.type_error(
                            span,
                            "a return statement cannot be used inside a class static block"
                                .to_string(),
                            DiagnosticCode::ReturnTypeMismatch,
                        ),
                        ClassBodyReturnRule::Constructor { .. }
                        | ClassBodyReturnRule::Method {
                            return_type: None, ..
                        } => {}
                    }
                }
                FunctionBodyItem::If(statement) => {
                    self.check_class_body_if(statement, &scope, return_rule)
                }
                FunctionBodyItem::While(statement) => {
                    self.check_direct_runtime_expression(&statement.test, &scope, &statement.span);
                    self.check_class_body_items(&statement.body, &scope, return_rule);
                }
                FunctionBodyItem::Try(statement) => {
                    self.check_class_body_items(&statement.block, &scope, return_rule);
                    if let Some(handler) = &statement.handler {
                        let catch_scope = self.catch_binding_scope(&scope, handler);
                        self.check_class_body_items(&handler.body, &catch_scope, return_rule);
                    }
                    if let Some(finalizer) = &statement.finalizer {
                        self.check_class_body_items(finalizer, &scope, return_rule);
                    }
                }
                FunctionBodyItem::Function(function) => {
                    self.check_function_in_scope(function, scope.clone());
                }
                FunctionBodyItem::Opaque(_) => {}
            }
        }
    }

    pub(super) fn check_class_body_if(
        &mut self,
        statement: &FunctionIfStatement,
        scope: &BTreeMap<String, Type>,
        return_rule: ClassBodyReturnRule<'_>,
    ) {
        self.check_direct_runtime_expression(&statement.test, scope, &statement.span);
        self.check_class_body_items(&statement.consequent, scope, return_rule);
        match &statement.alternate {
            Some(FunctionElseBranch::Braced(body)) => {
                self.check_class_body_items(body, scope, return_rule);
            }
            Some(FunctionElseBranch::ElseIf(branch)) => {
                self.check_class_body_if(branch, scope, return_rule);
            }
            None => {}
        }
    }
}
