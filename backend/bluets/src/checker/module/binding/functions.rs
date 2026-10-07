// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Structured function-body checking and return-flow analysis.

use super::*;

impl<'a> ModuleChecker<'a> {
    pub(super) fn check_function(&mut self, function: &FunctionDeclaration) {
        self.check_function_in_scope(function, self.values.clone());
    }

    /// Checks a function whose body sees `scope`: the module values for a
    /// named function, or the enclosing scope for a function nested in an
    /// expression.
    pub(in crate::checker::module) fn check_function_in_scope(
        &mut self,
        function: &FunctionDeclaration,
        mut scope: BTreeMap<String, Type>,
    ) {
        let previous_async = self.async_context.replace(function.async_function);
        let generator_context = function
            .generator
            .then(|| self.generator_context_for(function));
        let previous_generator = std::mem::replace(&mut self.generator_context, generator_context);
        let previous_annotated = self.annotated_names.clone();
        let previous_parameters = self.type_parameters.clone();
        let previous_types = self.types.clone();
        let previous_bound = self.bound_parameters.clone();
        for parameter in &function.type_parameters {
            self.bound_parameters
                .insert(parameter.name.clone(), parameter.clone());
            self.types.insert(
                parameter.name.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Parameter,
                    parameters: Vec::new(),
                    value: parameter.constraint.clone().unwrap_or(Type::StrictUnknown),
                },
            );
        }
        let previous_spreads = self.allowed_tuple_spread_parameters.clone();
        self.allowed_tuple_spread_parameters = function
            .type_parameters
            .iter()
            .filter(|parameter| {
                matches!(parameter.constraint, Some(Type::Array(_) | Type::Tuple(_)))
            })
            .map(|parameter| {
                (
                    parameter.name.clone(),
                    parameter.constraint.clone().expect("filtered constraint"),
                )
            })
            .collect();
        self.check_type_parameters(&function.type_parameters);
        self.check_function_flags(function, &scope);
        for (index, parameter) in function.parameters.iter().enumerate() {
            if parameter.rest && index + 1 != function.parameters.len() {
                self.type_error(
                    &parameter.span,
                    "a rest parameter must be last".to_string(),
                    DiagnosticCode::TypeMismatch,
                );
            }
            if parameter.rest && parameter.optional {
                self.typescript_type_error(
                    &parameter.span,
                    "a rest parameter cannot be optional or have a default initializer".to_string(),
                    DiagnosticCode::TypeMismatch,
                    if parameter.default.is_some() {
                        1048
                    } else {
                        2370
                    },
                    Vec::new(),
                );
            }
            if parameter.rest
                && parameter
                    .annotation
                    .as_ref()
                    .is_some_and(|annotation| !matches!(annotation, Type::Array(_)))
            {
                self.rest_annotation_error(parameter);
            }
            if let Some(default) = &parameter.default {
                self.check_direct_runtime_expression(default, &scope, &parameter.span);
                let actual = self.infer_expression(default, &scope);
                let expected = parameter
                    .annotation
                    .as_ref()
                    .cloned()
                    .unwrap_or(Type::Unknown);
                if !self.is_assignable_bounded(&actual, &expected, &parameter.span) {
                    self.type_error(
                        &parameter.span,
                        format!(
                            "default initializer has type `{}`, which is not assignable to parameter `{}` of type `{}`",
                            type_label(&actual),
                            parameter.name,
                            type_label(&expected)
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                }
            }
            let parameter_type = match &parameter.annotation {
                Some(annotation) => {
                    self.check_type(annotation, &parameter.span);
                    if parameter.optional && parameter.default.is_none() {
                        Type::Union(vec![annotation.clone(), Type::Undefined])
                    } else {
                        annotation.clone()
                    }
                }
                None if self.explicit_checking => self
                    .return_inference
                    .parameters
                    .borrow()
                    .get(&parameter.span.start)
                    .cloned()
                    .unwrap_or_else(|| {
                        parameter
                            .default
                            .as_ref()
                            .map_or(Type::Any, |default| self.infer_expression(default, &scope))
                    }),
                None => Type::Unknown,
            };
            match &parameter.pattern {
                // A destructured parameter binds the names in its pattern.
                Some(pattern) => {
                    self.bind_pattern(pattern, &parameter_type, &parameter.span, &mut scope)
                }
                None => {
                    if parameter.annotation.is_some() && !parameter.rest {
                        self.annotated_names.insert(parameter.name.clone());
                    } else {
                        self.annotated_names.remove(&parameter.name);
                    }
                    scope.insert(parameter.name.clone(), parameter_type);
                }
            }
        }
        for local in &function.locals {
            if local.annotation.is_some() {
                self.annotated_names.insert(local.name.clone());
            } else {
                self.annotated_names.remove(&local.name);
            }
            self.check_variable_in_scope(local, &scope);
            let inferred = local
                .annotation
                .clone()
                .unwrap_or_else(|| self.infer_expression(&local.initializer, &scope));
            scope.insert(local.name.clone(), inferred);
        }
        self.check_flow_initializers(function, &scope);
        hoist_local_functions(&function.body, &mut scope);
        self.check_function_body_expressions(&function.body, &scope);
        if let Some(return_type) = &function.return_type {
            self.check_type(return_type, &function.span);
            self.check_predicate_signature(return_type, &function.parameters);
        }
        // An `async` function's body returns the type inside its `Promise<T>`.
        let body_return_type = match (&function.return_type, function.async_function) {
            (Some(return_type), true) => match promise_value_type(return_type) {
                Some(value) => Some(value),
                None => {
                    self.type_error(
                        &function.span,
                        "the return type of an async function must be the global `Promise<T>` type"
                            .to_string(),
                        DiagnosticCode::TypeMismatch,
                    );
                    None
                }
            },
            (Some(_), false) if function.generator => self
                .generator_context
                .as_ref()
                .and_then(|context| context.return_type.clone()),
            (Some(return_type), false) => Some(return_type.runtime_result()),
            (return_type, _) => return_type.clone(),
        };
        let allows_implicit_undefined = if let Some(return_type) = &body_return_type {
            self.return_type_allows_implicit_undefined(return_type, &function.span)
        } else {
            true
        };
        self.check_function_body_returns(
            &function.body,
            &scope,
            body_return_type.as_ref(),
            allows_implicit_undefined,
            &function.span,
        );
        if let Some(return_type) = &body_return_type {
            if !function.declared
                && !function.overload
                && !allows_implicit_undefined
                && !matches!(
                    Self::function_body_termination(&function.body),
                    StructuredTermination::Terminates
                )
                && self
                    .flow
                    .as_ref()
                    .and_then(|flow| flow.completes(function.span.start))
                    .unwrap_or_else(|| {
                        matches!(
                            Self::function_body_termination(&function.body),
                            StructuredTermination::FallsThrough
                        )
                    })
            {
                self.typescript_type_error(
                    &function.span,
                    format!(
                        "function with return type `{}` can complete without returning a value",
                        type_label(return_type)
                    ),
                    DiagnosticCode::ReturnTypeMismatch,
                    if return_inference::has_return(&function.body)
                        || !function.returns.is_empty()
                        || self
                            .flow
                            .as_ref()
                            .is_some_and(|flow| flow.has_return(function.span.start))
                    {
                        2366
                    } else {
                        2355
                    },
                    Vec::new(),
                );
            }
        }
        self.type_parameters = previous_parameters;
        self.types = previous_types;
        self.bound_parameters = previous_bound;
        self.allowed_tuple_spread_parameters = previous_spreads;
        self.async_context = previous_async;
        self.generator_context = previous_generator;
        self.annotated_names = previous_annotated;
    }

    /// Binds the names of a destructured parameter, typing each from the value
    /// type: an object pattern reads the property of the same key (an absent
    /// one is an error, an optional one may be `undefined` unless defaulted),
    /// and an array pattern reads the tuple element at its position.
    fn bind_pattern(
        &mut self,
        pattern: &BindingPattern,
        value: &Type,
        span: &SourceSpan,
        scope: &mut BTreeMap<String, Type>,
    ) {
        let mut resolved = value.clone();
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let mut visited = HashSet::new();
        while let Some(expanded) =
            instantiate_named(&resolved, &self.types, &mut visited, &mut budget, "pattern")
        {
            resolved = expanded;
        }
        match pattern {
            BindingPattern::Object(bindings) => {
                for binding in bindings {
                    let mut lookup_budget = TypeExpansionBudget::new(self.max_type_expansions);
                    let found = property_type(
                        &resolved,
                        &binding.key,
                        &self.types,
                        &mut HashSet::new(),
                        &mut lookup_budget,
                    );
                    let bound = match found {
                        PropertyType::Found { value, .. } => value,
                        PropertyType::Missing if matches!(resolved, Type::Record(_)) => {
                            self.type_error(
                                &binding.span,
                                format!(
                                    "property `{}` does not exist on type `{}`",
                                    binding.key,
                                    type_label(value)
                                ),
                                DiagnosticCode::TypeMismatch,
                            );
                            Type::Unknown
                        }
                        _ => Type::Unknown,
                    };
                    let bound =
                        self.defaulted_binding_type(bound, &binding.default, &binding.span, scope);
                    scope.insert(binding.name.clone(), bound);
                }
            }
            BindingPattern::Array(elements) => {
                for (index, element) in elements.iter().enumerate() {
                    let Some(element) = element else {
                        continue;
                    };
                    let bound = match &resolved {
                        Type::Tuple(items) => match items.get(index) {
                            Some(item) if item.rest => match &item.annotation {
                                Type::Array(inner) => (**inner).clone(),
                                _ => Type::Unknown,
                            },
                            Some(item) if item.optional => {
                                Type::Union(vec![item.annotation.clone(), Type::Undefined])
                            }
                            Some(item) => item.annotation.clone(),
                            None => {
                                self.type_error(
                                    &element.span,
                                    format!(
                                        "tuple type `{}` has no element at index {index}",
                                        type_label(value)
                                    ),
                                    DiagnosticCode::TypeMismatch,
                                );
                                Type::Unknown
                            }
                        },
                        Type::Array(inner) => (**inner).clone(),
                        _ => Type::Unknown,
                    };
                    let bound =
                        self.defaulted_binding_type(bound, &element.default, &element.span, scope);
                    scope.insert(element.name.clone(), bound);
                }
            }
        }
        let _ = span;
    }

    /// The type a binding has once its default is applied: the default must fit
    /// the value type, and `undefined` is no longer possible.
    fn defaulted_binding_type(
        &mut self,
        bound: Type,
        default: &Option<Vec<Token>>,
        span: &SourceSpan,
        scope: &BTreeMap<String, Type>,
    ) -> Type {
        let Some(default) = default else {
            return bound;
        };
        let without_undefined = match &bound {
            Type::Union(options) => {
                let kept: Vec<Type> = options
                    .iter()
                    .filter(|option| !matches!(option, Type::Undefined))
                    .cloned()
                    .collect();
                match kept.len() {
                    0 => bound.clone(),
                    1 => kept[0].clone(),
                    _ => Type::Union(kept),
                }
            }
            _ => bound.clone(),
        };
        let actual = self.infer_expression(default, scope);
        if !self.is_assignable_bounded(&actual, &without_undefined, span) {
            self.type_error(
                span,
                format!(
                    "default initializer has type `{}`, which is not assignable to `{}`",
                    type_label(&actual),
                    type_label(&without_undefined)
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
        without_undefined
    }

    /// Whether an explicit return annotation permits the JavaScript
    /// fall-through result. `void` is special in TypeScript return positions;
    /// the bounded assignability relation otherwise covers `undefined`,
    /// `any`, `unknown`, aliases, and unions containing one of those types.
    pub(super) fn return_type_allows_implicit_undefined(
        &mut self,
        return_type: &Type,
        span: &SourceSpan,
    ) -> bool {
        matches!(return_type, Type::Void)
            || self.is_assignable_bounded(&Type::Undefined, return_type, span)
    }

    /// Determines whether the structured function body cannot reach its end.
    ///
    /// The parser records unsupported syntax as `Opaque`, which must never be
    /// mistaken for a terminating branch. A recognized `return` (including an
    /// invalid bare return, diagnosed separately) or `throw` terminates its
    /// sequential path. An `if` does so only when both structured branches do.
    /// The unknown result preserves the standalone parser's existing opaque
    /// syntax behavior; the direct bridge rejects that syntax independently.
    /// A `try` cannot reach its end when its `finally` cannot, or when its
    /// block cannot and its `catch` clause, if any, cannot either.
    fn function_try_termination(statement: &FunctionTryStatement) -> StructuredTermination {
        let finalizer = statement
            .finalizer
            .as_deref()
            .map(Self::function_body_termination);
        if matches!(finalizer, Some(StructuredTermination::Terminates)) {
            return StructuredTermination::Terminates;
        }
        let block = Self::function_body_termination(&statement.block);
        let handler = statement
            .handler
            .as_ref()
            .map(|handler| Self::function_body_termination(&handler.body));
        if matches!(finalizer, Some(StructuredTermination::Opaque))
            || matches!(block, StructuredTermination::Opaque)
            || matches!(handler, Some(StructuredTermination::Opaque))
        {
            return StructuredTermination::Opaque;
        }
        if matches!(block, StructuredTermination::Terminates)
            && matches!(handler, None | Some(StructuredTermination::Terminates))
        {
            StructuredTermination::Terminates
        } else {
            StructuredTermination::FallsThrough
        }
    }

    pub(in crate::checker::module) fn function_body_termination(
        items: &[FunctionBodyItem],
    ) -> StructuredTermination {
        for item in items {
            match item {
                FunctionBodyItem::Return { .. } | FunctionBodyItem::Throw { .. } => {
                    return StructuredTermination::Terminates;
                }
                FunctionBodyItem::If(statement) => match Self::function_if_termination(statement) {
                    StructuredTermination::Terminates => {
                        return StructuredTermination::Terminates;
                    }
                    StructuredTermination::FallsThrough => {}
                    StructuredTermination::Opaque => return StructuredTermination::Opaque,
                },
                FunctionBodyItem::Opaque(_) => return StructuredTermination::Opaque,
                FunctionBodyItem::Try(statement) => match Self::function_try_termination(statement)
                {
                    StructuredTermination::Terminates => {
                        return StructuredTermination::Terminates;
                    }
                    StructuredTermination::FallsThrough => {}
                    StructuredTermination::Opaque => return StructuredTermination::Opaque,
                },
                FunctionBodyItem::Variable(_)
                | FunctionBodyItem::Expression { .. }
                | FunctionBodyItem::Function(_)
                | FunctionBodyItem::While(_) => {}
            }
        }
        StructuredTermination::FallsThrough
    }

    fn function_if_termination(statement: &FunctionIfStatement) -> StructuredTermination {
        let consequent = Self::function_body_termination(&statement.consequent);
        let alternate = match &statement.alternate {
            Some(FunctionElseBranch::Braced(body)) => Self::function_body_termination(body),
            Some(FunctionElseBranch::ElseIf(branch)) => Self::function_if_termination(branch),
            None => StructuredTermination::FallsThrough,
        };
        match (consequent, alternate) {
            (StructuredTermination::Opaque, _) | (_, StructuredTermination::Opaque) => {
                StructuredTermination::Opaque
            }
            (StructuredTermination::Terminates, StructuredTermination::Terminates) => {
                StructuredTermination::Terminates
            }
            (StructuredTermination::FallsThrough, _) | (_, StructuredTermination::FallsThrough) => {
                StructuredTermination::FallsThrough
            }
        }
    }

    /// Check structured returns in the lexical scope where they execute.
    /// The parser's flat `returns` list remains part of its public source
    /// representation, but cannot distinguish a catch-shadowed binding.
    fn check_function_body_returns(
        &mut self,
        items: &[FunctionBodyItem],
        scope: &BTreeMap<String, Type>,
        return_type: Option<&Type>,
        allows_implicit_undefined: bool,
        function_span: &SourceSpan,
    ) {
        let current_scope = scope.clone();
        for item in items {
            match item {
                FunctionBodyItem::Return { tokens, .. } => self.check_function_return_tokens(
                    tokens,
                    &current_scope,
                    return_type,
                    allows_implicit_undefined,
                    function_span,
                ),
                FunctionBodyItem::If(statement) => {
                    self.check_function_if_returns(
                        statement,
                        &current_scope,
                        return_type,
                        allows_implicit_undefined,
                        function_span,
                    );
                }
                FunctionBodyItem::While(statement) => self.check_function_body_returns(
                    &statement.body,
                    &current_scope,
                    return_type,
                    allows_implicit_undefined,
                    function_span,
                ),
                FunctionBodyItem::Try(statement) => {
                    self.check_function_body_returns(
                        &statement.block,
                        &current_scope,
                        return_type,
                        allows_implicit_undefined,
                        function_span,
                    );
                    if let Some(handler) = &statement.handler {
                        let catch_scope = self.catch_binding_scope(&current_scope, handler);
                        let previous_strictness = self.strict_catch_unknown;
                        self.strict_catch_unknown = true;
                        self.check_function_body_returns(
                            &handler.body,
                            &catch_scope,
                            return_type,
                            allows_implicit_undefined,
                            function_span,
                        );
                        self.strict_catch_unknown = previous_strictness;
                    }
                    if let Some(finalizer) = &statement.finalizer {
                        self.check_function_body_returns(
                            finalizer,
                            &current_scope,
                            return_type,
                            allows_implicit_undefined,
                            function_span,
                        );
                    }
                }
                FunctionBodyItem::Opaque(span) | FunctionBodyItem::Expression { span, .. } => {
                    let returned = self
                        .flow
                        .as_ref()
                        .map(|flow| flow.returned_in(function_span.start, span))
                        .unwrap_or_default();
                    for tokens in returned {
                        self.check_function_return_tokens(
                            &tokens,
                            &current_scope,
                            return_type,
                            allows_implicit_undefined,
                            function_span,
                        );
                    }
                }
                FunctionBodyItem::Variable(_) => {}
                // A nested function's returns belong to that function.
                FunctionBodyItem::Throw { .. } | FunctionBodyItem::Function(_) => {}
            }
        }
    }

    fn check_function_if_returns(
        &mut self,
        statement: &FunctionIfStatement,
        scope: &BTreeMap<String, Type>,
        return_type: Option<&Type>,
        allows_implicit_undefined: bool,
        function_span: &SourceSpan,
    ) {
        self.check_function_body_returns(
            &statement.consequent,
            scope,
            return_type,
            allows_implicit_undefined,
            function_span,
        );
        match &statement.alternate {
            Some(FunctionElseBranch::Braced(body)) => self.check_function_body_returns(
                body,
                scope,
                return_type,
                allows_implicit_undefined,
                function_span,
            ),
            Some(FunctionElseBranch::ElseIf(branch)) => self.check_function_if_returns(
                branch,
                scope,
                return_type,
                allows_implicit_undefined,
                function_span,
            ),
            None => {}
        }
    }

    pub(in crate::checker::module) fn check_function_return_tokens(
        &mut self,
        returned: &[Token],
        scope: &BTreeMap<String, Type>,
        return_type: Option<&Type>,
        allows_implicit_undefined: bool,
        function_span: &SourceSpan,
    ) {
        if returned.is_empty() {
            if let Some(return_type) = return_type.filter(|_| !allows_implicit_undefined) {
                self.type_error(
                    function_span,
                    format!(
                        "return expression has type `undefined`, which is not assignable to `{}`",
                        type_label(return_type)
                    ),
                    DiagnosticCode::ReturnTypeMismatch,
                );
            }
            return;
        }
        self.check_direct_runtime_expression(returned, scope, function_span);
        let Some(return_type) = return_type else {
            return;
        };
        if self.check_fresh_properties(returned, return_type, scope, function_span) {
            return;
        }
        let actual = self.infer_in_context(returned, scope, return_type);
        let explicit_unknown = actual == Type::Unknown
            && returned.len() == 1
            && self
                .scopes
                .as_ref()
                .is_some_and(|scopes| scopes.flow_explicit_unknown(&returned[0]));
        let previous_unknown = self.strict_catch_unknown;
        self.strict_catch_unknown |= explicit_unknown;
        let return_is_assignable = (matches!(actual, Type::Undefined) && allows_implicit_undefined)
            || self.is_assignable_bounded(&actual, return_type, function_span);
        self.strict_catch_unknown = previous_unknown;
        if !return_is_assignable {
            self.assignment_error(
                function_span,
                format!(
                    "return expression has type `{}`, which is not assignable to `{}`",
                    type_label(&actual),
                    type_label(return_type)
                ),
                DiagnosticCode::ReturnTypeMismatch,
                &actual,
                return_type,
            );
            if let Some(token) = self
                .scopes
                .as_ref()
                .and_then(|scopes| scopes.flow_return_token(returned[0].start))
            {
                self.point_last_typescript(&[token]);
            }
        }
    }

    pub(in crate::checker::module) fn catch_binding_scope(
        &self,
        scope: &BTreeMap<String, Type>,
        handler: &FunctionCatchClause,
    ) -> BTreeMap<String, Type> {
        let mut catch_scope = scope.clone();
        let binding_type = match handler.annotation {
            Some(Type::Any) => Type::Any,
            Some(Type::Unknown) => Type::Unknown,
            Some(Type::StrictUnknown) => Type::StrictUnknown,
            _ if self.checking.use_unknown_in_catch_variables => Type::Unknown,
            _ => Type::Any,
        };
        catch_scope.insert(handler.binding.clone(), binding_type);
        catch_scope
    }

    pub(super) fn check_function_body_expressions(
        &mut self,
        items: &[FunctionBodyItem],
        scope: &BTreeMap<String, Type>,
    ) {
        let current_scope = scope.clone();
        for item in items {
            let (tokens, span) = match item {
                FunctionBodyItem::Expression { tokens, span }
                | FunctionBodyItem::Throw { tokens, span } => (tokens, span),
                FunctionBodyItem::Variable(_) => continue,
                FunctionBodyItem::Function(function) => {
                    self.check_function_in_scope(function, current_scope.clone());
                    continue;
                }
                FunctionBodyItem::If(statement) => {
                    self.check_direct_function_if(statement, &current_scope);
                    continue;
                }
                FunctionBodyItem::While(statement) => {
                    self.check_direct_function_while(statement, &current_scope);
                    continue;
                }
                FunctionBodyItem::Try(statement) => {
                    self.check_function_body_expressions(&statement.block, &current_scope);
                    if let Some(handler) = &statement.handler {
                        let catch_scope = self.catch_binding_scope(&current_scope, handler);
                        let previous_strictness = self.strict_catch_unknown;
                        self.strict_catch_unknown = true;
                        self.check_function_body_expressions(&handler.body, &catch_scope);
                        self.strict_catch_unknown = previous_strictness;
                    }
                    if let Some(finalizer) = &statement.finalizer {
                        self.check_function_body_expressions(finalizer, &current_scope);
                    }
                    continue;
                }
                _ => continue,
            };
            self.check_direct_runtime_expression(tokens, &current_scope, span);
        }
    }

    pub(super) fn check_direct_function_if(
        &mut self,
        statement: &FunctionIfStatement,
        scope: &BTreeMap<String, Type>,
    ) {
        self.check_direct_runtime_expression(&statement.test, scope, &statement.span);

        self.check_function_body_expressions(&statement.consequent, scope);
        match &statement.alternate {
            Some(FunctionElseBranch::Braced(alternate)) => {
                self.check_function_body_expressions(alternate, scope);
            }
            Some(FunctionElseBranch::ElseIf(alternate)) => {
                self.check_direct_function_if(alternate, scope);
            }
            None => {}
        }
    }

    pub(super) fn check_direct_function_while(
        &mut self,
        statement: &FunctionWhileStatement,
        scope: &BTreeMap<String, Type>,
    ) {
        self.check_direct_runtime_expression(&statement.test, scope, &statement.span);
        self.check_function_body_expressions(&statement.body, scope);
    }
}

/// Binds every function declared in `items`, at any block depth, to its
/// function type in `scope`. Declarations are hoisted, so a body may call one
/// before it appears, a declaration may call itself, and siblings may call each
/// other.
pub(in crate::checker::module) fn hoist_local_functions(
    items: &[FunctionBodyItem],
    scope: &mut BTreeMap<String, Type>,
) {
    for item in items {
        match item {
            FunctionBodyItem::Function(function) => {
                scope.insert(
                    function.name.clone(),
                    super::nested_functions::declared_function_type(
                        &function.parameters,
                        super::nested_functions::async_result(
                            function.return_type.clone().unwrap_or(Type::Unknown),
                            function.async_function && function.return_type.is_none(),
                        ),
                        &function.type_parameters,
                    ),
                );
            }
            FunctionBodyItem::If(statement) => hoist_from_if(statement, scope),
            FunctionBodyItem::While(statement) => hoist_local_functions(&statement.body, scope),
            FunctionBodyItem::Try(statement) => {
                hoist_local_functions(&statement.block, scope);
                if let Some(handler) = &statement.handler {
                    hoist_local_functions(&handler.body, scope);
                }
                if let Some(finalizer) = &statement.finalizer {
                    hoist_local_functions(finalizer, scope);
                }
            }
            _ => {}
        }
    }
}

fn hoist_from_if(statement: &FunctionIfStatement, scope: &mut BTreeMap<String, Type>) {
    hoist_local_functions(&statement.consequent, scope);
    match &statement.alternate {
        Some(FunctionElseBranch::Braced(items)) => hoist_local_functions(items, scope),
        Some(FunctionElseBranch::ElseIf(next)) => hoist_from_if(next, scope),
        None => {}
    }
}

/// The `T` of `Promise<T>`, when `value` is that type.
pub(in crate::checker::module) fn promise_value_type(value: &Type) -> Option<Type> {
    match value {
        Type::Named { name, arguments } if name == "Promise" && arguments.len() == 1 => {
            Some(arguments[0].clone())
        }
        _ => None,
    }
}
