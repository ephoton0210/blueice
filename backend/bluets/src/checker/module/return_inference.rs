// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Return inference shares lexical scopes, expression typing and expansion limits.

use super::*;
use crate::parser::{NestedFunction, NestedFunctionBody, VariableKind};
use std::cell::RefCell;
mod completion;
pub(super) use completion::has_return;

#[derive(Default)]
pub(super) struct ReturnInference {
    pub(super) results: RefCell<BTreeMap<usize, Type>>,
    pub(super) parameters: RefCell<BTreeMap<usize, Type>>,
    active: RefCell<Vec<Frame>>,
    failures: RefCell<BTreeMap<usize, (SourceSpan, bool)>>,
}

struct Frame {
    start: usize,
    locals: BTreeMap<String, FunctionDeclaration>,
    tail_calls: BTreeSet<usize>,
    provisional: Type,
    recursive: bool,
}

#[derive(Default)]
struct BodyResults {
    returns: Vec<Type>,
    fresh: bool,
    bare: bool,
    yields: Vec<Type>,
    inputs: Vec<Type>,
}

impl ModuleChecker<'_> {
    pub(in crate::checker) fn inferred_parameter_types(&self) -> BTreeMap<usize, Type> {
        self.return_inference.parameters.borrow().clone()
    }

    pub(super) fn return_parameters(
        &self,
        parameters: &[Parameter],
        scope: &BTreeMap<String, Type>,
    ) -> Vec<Parameter> {
        parameters
            .iter()
            .map(|parameter| {
                let mut inferred = parameter.clone();
                if inferred.annotation.is_none() {
                    inferred.annotation = self
                        .return_inference
                        .parameters
                        .borrow()
                        .get(&parameter.span.start)
                        .cloned();
                }
                if inferred.annotation.is_none() {
                    if let Some(default) = &parameter.default {
                        let value = widen(self.infer_expression(default, scope));
                        self.return_inference
                            .parameters
                            .borrow_mut()
                            .insert(parameter.span.start, value.clone());
                        inferred.annotation = Some(value);
                    }
                }
                if inferred.annotation.is_none()
                    && self.module.id.ends_with(".js")
                    && self.check_javascript.is_some()
                {
                    inferred.annotation = Some(Type::Any);
                }
                inferred
            })
            .collect()
    }
    pub(in crate::checker) fn inferred_return_types(&self) -> BTreeMap<usize, Type> {
        self.return_inference.results.borrow().clone()
    }
    pub(super) fn inferred_function_result(
        &self,
        function: &FunctionDeclaration,
        scope: &BTreeMap<String, Type>,
        expression: bool,
    ) -> Type {
        if let Some(result) = &function.return_type {
            return result.clone();
        }
        self.inferred_body_result(
            &function.parameters,
            &function.body,
            scope,
            &function.span,
            function.async_function,
            function.generator,
            expression,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn inferred_body_result(
        &self,
        parameters: &[Parameter],
        body: &[FunctionBodyItem],
        outer: &BTreeMap<String, Type>,
        span: &SourceSpan,
        asynchronous: bool,
        generator: bool,
        expression: bool,
    ) -> Type {
        if let Some(result) = self.return_inference.results.borrow().get(&span.start) {
            return result.clone();
        }
        {
            let active = self.return_inference.active.borrow();
            let limited = active.len() >= self.max_type_expansions;
            if limited || active.iter().any(|frame| frame.start == span.start) {
                self.return_inference
                    .failures
                    .borrow_mut()
                    .insert(span.start, (span.clone(), limited));
                return Type::Unknown;
            }
        }
        let locals = body
            .iter()
            .filter_map(|item| match item {
                FunctionBodyItem::Function(function) => {
                    Some((function.name.clone(), (**function).clone()))
                }
                _ => None,
            })
            .collect();
        self.return_inference.active.borrow_mut().push(Frame {
            start: span.start,
            locals,
            tail_calls: tail_calls(body),
            provisional: Type::Never,
            recursive: false,
        });
        let mut scope = outer.clone();
        for item in body {
            if let FunctionBodyItem::Function(function) = item {
                scope.insert(
                    function.name.clone(),
                    Type::Function {
                        parameters: function.parameters.clone(),
                        result: Box::new(function.return_type.clone().unwrap_or(Type::Unknown)),
                    },
                );
            }
        }
        let mut fresh = BTreeSet::new();
        for parameter in self.return_parameters(parameters, outer) {
            let value = parameter.annotation.clone().or_else(|| {
                parameter
                    .default
                    .as_deref()
                    .map(|tokens| self.infer_expression(tokens, outer))
                    .map(widen)
            });
            let value = value.unwrap_or(Type::Unknown);
            if let Some(pattern) = &parameter.pattern {
                self.infer_return_pattern(pattern, &value, &mut scope);
            } else {
                scope.insert(parameter.name.clone(), value);
            }
        }
        let mut found = BodyResults {
            fresh: true,
            ..BodyResults::default()
        };
        self.infer_body_items(body, &mut scope, &mut fresh, &mut found);
        if found.returns.is_empty() {
            let provisional = if asynchronous {
                Type::Named {
                    name: "Promise".to_string(),
                    arguments: vec![Type::Void],
                }
            } else {
                Type::Void
            };
            self.return_inference
                .results
                .borrow_mut()
                .insert(span.start, provisional);
        }
        let falls_through = (found.returns.is_empty() && !expression)
            || self.inference_body_completes(body, &scope);
        let mut result = if found.returns.is_empty() {
            if expression && !found.bare && !falls_through {
                Type::Never
            } else {
                Type::Void
            }
        } else {
            if falls_through || found.bare {
                found.returns.push(Type::Undefined);
            }
            let value = joined(found.returns);
            if found.fresh && matches!(value, Type::Literal(_)) {
                widen(value)
            } else {
                value
            }
        };
        if generator {
            result = Type::Named {
                name: "Generator".to_string(),
                arguments: vec![
                    joined(found.yields.into_iter().map(widen).collect()),
                    result,
                    if found.inputs.is_empty() {
                        Type::Unknown
                    } else {
                        joined(found.inputs)
                    },
                ],
            };
        } else if asynchronous {
            result = Type::Named {
                name: "Promise".to_string(),
                arguments: vec![awaited(result, self.max_type_expansions)],
            };
        }
        let frame = self.return_inference.active.borrow_mut().pop().unwrap();
        if frame.recursive {
            result = widen(result);
        }
        if !contains_unknown(&result) {
            self.return_inference
                .results
                .borrow_mut()
                .insert(span.start, result.clone());
        } else if generator {
            // An unconstrained generator's next type is intentionally unknown.
            self.return_inference
                .results
                .borrow_mut()
                .insert(span.start, result.clone());
        }
        result
    }

    pub(super) fn inferred_call_signatures(
        &self,
        name: &str,
        call_start: usize,
        scope: &BTreeMap<String, Type>,
    ) -> Option<Vec<FunctionSignature>> {
        // A recursive variable initializer or named function expression has
        // no annotated call surface to close its inference cycle.
        let recursive_expression = self.module.declarations.iter().find_map(|declaration| {
            let Declaration::Variable(variable) = declaration else {
                return None;
            };
            if variable.name != name {
                return None;
            }
            let nested = self
                .module
                .nested_functions
                .get(&variable.initializer.first()?.start)?;
            self.return_inference
                .active
                .borrow()
                .iter()
                .any(|frame| frame.start == nested.span.start)
                .then_some(nested.span.clone())
        });
        if let Some(span) = recursive_expression {
            if let Some(result) = self.return_inference.results.borrow().get(&span.start) {
                let function = &self.module.nested_functions[&span.start];
                return Some(vec![FunctionSignature {
                    parameters: function.parameters.clone(),
                    type_parameters: function.type_parameters.clone(),
                    return_type: result.clone(),
                }]);
            }
            self.return_inference
                .failures
                .borrow_mut()
                .insert(span.start, (span, false));
            return None;
        }
        if let Some(function) = self.module.nested_functions.values().find(|function| {
            function.name.as_deref() == Some(name)
                && !function
                    .parameters
                    .iter()
                    .any(|parameter| parameter.name == name)
                && self
                    .return_inference
                    .active
                    .borrow()
                    .last()
                    .is_some_and(|frame| {
                        frame.start == function.span.start && !frame.locals.contains_key(name)
                    })
        }) {
            if let Some(result) = self
                .return_inference
                .results
                .borrow()
                .get(&function.span.start)
            {
                return Some(vec![FunctionSignature {
                    parameters: function.parameters.clone(),
                    type_parameters: function.type_parameters.clone(),
                    return_type: result.clone(),
                }]);
            }
            let result = {
                let mut active = self.return_inference.active.borrow_mut();
                let frame = active.last_mut().unwrap();
                if frame.tail_calls.contains(&call_start) {
                    frame.recursive = true;
                    frame.provisional.clone()
                } else {
                    self.return_inference
                        .failures
                        .borrow_mut()
                        .insert(function.span.start, (function.span.clone(), false));
                    Type::Unknown
                }
            };
            return Some(vec![FunctionSignature {
                parameters: self.return_parameters(&function.parameters, scope),
                type_parameters: function.type_parameters.clone(),
                return_type: result,
            }]);
        }
        let local = self
            .return_inference
            .active
            .borrow()
            .iter()
            .rev()
            .find_map(|frame| frame.locals.get(name).cloned());
        let local_function = local.is_some();
        let function = local.or_else(|| {
            (scope.get(name) == self.values.get(name)).then(|| {
                self.module
                    .declarations
                    .iter()
                    .find_map(|declaration| match declaration {
                        Declaration::Function(function)
                            if function.name == name
                                && !function.overload
                                && !function.declared =>
                        {
                            Some(function.clone())
                        }
                        _ => None,
                    })
            })?
        })?;
        if function.return_type.is_some() {
            return None;
        }
        let mut signatures = (!local_function)
            .then(|| self.functions.get(name).cloned())
            .flatten()
            .unwrap_or_else(|| {
                vec![FunctionSignature {
                    parameters: function.parameters.clone(),
                    type_parameters: function.type_parameters.clone(),
                    return_type: Type::Unknown,
                }]
            });
        // Explicit overloads remain the call surface.
        if !local_function && self.module.declarations.iter().any(|declaration| {
            matches!(declaration, Declaration::Function(other) if other.name == name && other.overload)
        }) {
            return None;
        }
        let tail_result = {
            let mut active = self.return_inference.active.borrow_mut();
            active
                .last_mut()
                .filter(|frame| {
                    frame.start == function.span.start && frame.tail_calls.contains(&call_start)
                })
                .map(|frame| {
                    frame.recursive = true;
                    frame.provisional.clone()
                })
        };
        let result =
            tail_result.unwrap_or_else(|| self.inferred_function_result(&function, scope, false));
        for signature in &mut signatures {
            signature.parameters = self.return_parameters(&function.parameters, scope);
            signature.return_type = result.clone();
        }
        Some(signatures)
    }

    pub(super) fn inferred_nested_result(
        &self,
        function: &NestedFunction,
        scope: &BTreeMap<String, Type>,
    ) -> Type {
        if let Some(result) = &function.return_type {
            return result.clone();
        }
        let body = match &function.body {
            NestedFunctionBody::Expression(tokens) => vec![FunctionBodyItem::Return {
                tokens: tokens.clone(),
                span: function.span.clone(),
            }],
            NestedFunctionBody::Block { items, .. } => items.clone(),
        };
        self.inferred_body_result(
            &function.parameters,
            &body,
            scope,
            &function.span,
            function.async_function,
            function.generator,
            true,
        )
    }

    fn infer_body_items(
        &self,
        body: &[FunctionBodyItem],
        scope: &mut BTreeMap<String, Type>,
        fresh: &mut BTreeSet<String>,
        found: &mut BodyResults,
    ) {
        for item in body {
            match item {
                FunctionBodyItem::Variable(variable) => {
                    self.infer_yields(&variable.initializer, scope, found);
                    if variable
                        .initializer
                        .first()
                        .is_some_and(|token| token.is("yield"))
                    {
                        if let Some(value) = &variable.annotation {
                            found.inputs.push(value.clone());
                        }
                    }
                    let value = variable.annotation.clone().unwrap_or_else(|| {
                        crate::parser::widen_literal_tokens(
                            &variable.initializer,
                            variable.kind == VariableKind::Const,
                        )
                        .unwrap_or_else(|| self.infer_expression(&variable.initializer, scope))
                    });
                    if variable.annotation.is_none()
                        && self.fresh_return(&variable.initializer, fresh)
                    {
                        fresh.insert(variable.name.clone());
                    } else {
                        fresh.remove(&variable.name);
                    }
                    scope.insert(
                        variable.name.clone(),
                        if variable.kind == VariableKind::Const {
                            value
                        } else {
                            widen(value)
                        },
                    );
                }
                FunctionBodyItem::Return { tokens, .. } => {
                    self.infer_yields(tokens, scope, found);
                    if tokens.is_empty() {
                        found.bare = true;
                    } else {
                        found.fresh &= self.fresh_return(tokens, fresh);
                        if let Some(frame) = self.return_inference.active.borrow_mut().last_mut() {
                            frame.provisional = widen(joined(found.returns.clone()));
                        }
                        found.returns.push(
                            crate::parser::widen_literal_tokens(tokens, true)
                                .unwrap_or_else(|| self.infer_expression(tokens, scope)),
                        );
                    }
                }
                FunctionBodyItem::If(statement) => {
                    self.infer_body_items(
                        &statement.consequent,
                        &mut scope.clone(),
                        &mut fresh.clone(),
                        found,
                    );
                    match &statement.alternate {
                        Some(FunctionElseBranch::Braced(body)) => self.infer_body_items(
                            body,
                            &mut scope.clone(),
                            &mut fresh.clone(),
                            found,
                        ),
                        Some(FunctionElseBranch::ElseIf(branch)) => self.infer_body_items(
                            &[FunctionBodyItem::If((**branch).clone())],
                            &mut scope.clone(),
                            &mut fresh.clone(),
                            found,
                        ),
                        None => {}
                    }
                }
                FunctionBodyItem::While(statement) => self.infer_body_items(
                    &statement.body,
                    &mut scope.clone(),
                    &mut fresh.clone(),
                    found,
                ),
                FunctionBodyItem::Try(statement) => {
                    self.infer_body_items(
                        &statement.block,
                        &mut scope.clone(),
                        &mut fresh.clone(),
                        found,
                    );
                    if let Some(handler) = &statement.handler {
                        let mut caught = scope.clone();
                        caught.insert(
                            handler.binding.clone(),
                            handler.annotation.clone().unwrap_or(Type::Unknown),
                        );
                        self.infer_body_items(
                            &handler.body,
                            &mut caught,
                            &mut fresh.clone(),
                            found,
                        );
                    }
                    if let Some(finalizer) = &statement.finalizer {
                        self.infer_body_items(
                            finalizer,
                            &mut scope.clone(),
                            &mut fresh.clone(),
                            found,
                        );
                    }
                }
                FunctionBodyItem::Expression { tokens, .. } => {
                    self.infer_yields(tokens, scope, found)
                }
                FunctionBodyItem::Function(_)
                | FunctionBodyItem::Throw { .. }
                | FunctionBodyItem::Opaque(_) => {}
            }
        }
    }

    fn infer_return_pattern(
        &self,
        pattern: &BindingPattern,
        value: &Type,
        scope: &mut BTreeMap<String, Type>,
    ) {
        let bindings = match pattern {
            BindingPattern::Object(bindings) => bindings
                .iter()
                .map(|binding| {
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    let value = match property_type(
                        value,
                        &binding.key,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) {
                        PropertyType::Found { value, .. } => value,
                        _ => Type::Unknown,
                    };
                    (binding.name.clone(), value, binding.default.as_ref())
                })
                .collect::<Vec<_>>(),
            BindingPattern::Array(bindings) => bindings
                .iter()
                .enumerate()
                .filter_map(|(index, binding)| {
                    let binding = binding.as_ref()?;
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    Some((
                        binding.name.clone(),
                        expressions::indexed_value_type(
                            value,
                            Some(index),
                            &self.types,
                            &mut HashSet::new(),
                            &mut budget,
                        ),
                        binding.default.as_ref(),
                    ))
                })
                .collect(),
        };
        for (name, value, default) in bindings {
            let value = if let Some(default) = default {
                match value {
                    Type::Union(parts) => joined(
                        parts
                            .into_iter()
                            .filter(|part| *part != Type::Undefined)
                            .collect(),
                    ),
                    Type::Undefined => self.infer_expression(default, scope),
                    other => other,
                }
            } else {
                value
            };
            scope.insert(name, value);
        }
    }

    fn fresh_return(&self, tokens: &[Token], names: &BTreeSet<String>) -> bool {
        let tokens = strip_outer_parentheses(tokens);
        crate::parser::widen_literal_tokens(tokens, true).is_some()
            || matches!(tokens, [name] if names.contains(&name.text)
                || self.module.declarations.iter().any(|declaration| matches!(declaration,
                    Declaration::Variable(variable) if variable.name == name.text && variable.annotation.is_none())))
    }

    fn infer_yields(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        found: &mut BodyResults,
    ) {
        for (index, token) in tokens.iter().enumerate() {
            if !token.is("yield")
                || self
                    .module
                    .nested_functions
                    .range(..=token.start)
                    .next_back()
                    .is_some_and(|(_, nested)| {
                        nested.span.start >= tokens[0].start && token.start < nested.span.end
                    })
            {
                continue;
            }
            let operand = &tokens[index + 1..];
            let delegated = operand.first().is_some_and(|token| token.is("*"));
            let value =
                self.infer_expression(if delegated { &operand[1..] } else { operand }, scope);
            found.yields.push(if delegated {
                self.iterated_type(&value).unwrap_or(Type::Unknown)
            } else {
                value
            });
        }
    }

    pub(super) fn report_return_inference_failures(&mut self) {
        let failures = std::mem::take(&mut *self.return_inference.failures.borrow_mut());
        for (_, (span, limited)) in failures {
            self.diagnostics.push(Diagnostic::error(
                if limited { DiagnosticCode::ResourceLimit } else { DiagnosticCode::TypeMismatch },
                span,
                if limited {
                    "return type inference exceeded the type expansion budget; add a return annotation"
                } else {
                    "recursive return type inference requires a return annotation"
                },
            ));
        }
    }
}

fn awaited(mut value: Type, limit: usize) -> Type {
    for _ in 0..limit {
        let Some(inner) = binding::promise_value_type(&value) else {
            break;
        };
        value = inner;
    }
    value
}

fn tail_calls(body: &[FunctionBodyItem]) -> BTreeSet<usize> {
    let mut calls = BTreeSet::new();
    for item in body {
        match item {
            FunctionBodyItem::Return { tokens, .. } => {
                if let Some(call) = direct_call_parts(strip_outer_parentheses(tokens)) {
                    if split_call_arguments(call.arguments).is_some() {
                        calls.insert(call.callee.start);
                    }
                }
            }
            FunctionBodyItem::If(statement) => {
                calls.extend(tail_calls(&statement.consequent));
                match &statement.alternate {
                    Some(FunctionElseBranch::Braced(body)) => calls.extend(tail_calls(body)),
                    Some(FunctionElseBranch::ElseIf(branch)) => {
                        calls.extend(tail_calls(&[FunctionBodyItem::If((**branch).clone())]))
                    }
                    None => {}
                }
            }
            FunctionBodyItem::While(statement) => calls.extend(tail_calls(&statement.body)),
            _ => {}
        }
    }
    calls
}

fn contains_unknown(value: &Type) -> bool {
    match value {
        Type::Unknown => true,
        Type::Union(parts) => parts.iter().any(contains_unknown),
        _ => false,
    }
}

pub(super) fn widen(value: Type) -> Type {
    match value {
        Type::Literal(ref text) if text.starts_with(['\'', '"', '`']) => Type::String,
        Type::Literal(ref text) if text.parse::<f64>().is_ok() => Type::Number,
        Type::Literal(ref text) if matches!(text.as_str(), "true" | "false") => Type::Boolean,
        other => other,
    }
}

fn joined(values: Vec<Type>) -> Type {
    let mut parts = Vec::new();
    for value in values {
        let values = match value {
            Type::Union(parts) => parts,
            other => vec![other],
        };
        for value in values {
            if value != Type::Never && !parts.contains(&value) {
                parts.push(value);
            }
        }
    }
    if parts.contains(&Type::Any) {
        return Type::Any;
    }
    if parts.contains(&Type::Unknown) {
        return Type::Unknown;
    }
    let broad = parts.clone();
    parts.retain(|part| !matches!(part, Type::Literal(_)) || !broad.contains(&widen(part.clone())));
    parts.sort_by_key(|part| match part {
        Type::Literal(text) if text.starts_with(['\'', '"', '`']) => 0,
        Type::Literal(text) if text.parse::<f64>().is_ok() => 1,
        Type::Undefined => 9,
        _ => 2,
    });
    match parts.len() {
        0 => Type::Never,
        1 => parts.pop().unwrap(),
        _ => Type::Union(parts),
    }
}
