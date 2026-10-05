// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime token uses; property names and declaration heads are not uses.

use super::*;
use crate::parser::{NestedFunction, NestedFunctionKind};

impl ScopeModel<'_> {
    pub(super) fn expression(&mut self, tokens: &[Token], scope: ScopeId) {
        let mut index = 0;
        while index < tokens.len() {
            index = self.expression_token(tokens, index, scope);
        }
    }

    pub(super) fn expression_token(
        &mut self,
        tokens: &[Token],
        index: usize,
        scope: ScopeId,
    ) -> usize {
        let token = &tokens[index];
        if token.is("/")
            && (index == 0
                || matches!(
                    tokens[index - 1].text.as_str(),
                    "(" | "["
                        | "{"
                        | "="
                        | ":"
                        | ","
                        | "return"
                        | "throw"
                        | "=>"
                        | "!"
                        | "&&"
                        | "||"
                        | "??"
                        | "?"
                ))
        {
            if let Some(end) = regex_end(&self.module.source, token.start) {
                return tokens
                    .partition_point(|token| token.start < end)
                    .max(index + 1);
            }
        }
        if let Some(function) = self.local_functions.get(&token.start).cloned() {
            self.value(
                scope,
                &function.name,
                0,
                false,
                false,
                Type::Function {
                    parameters: function.parameters.clone(),
                    result: Box::new(function.return_type.clone().unwrap_or(Type::Unknown)),
                },
            );
            self.function(&function, scope);
            return tokens
                .partition_point(|token| token.start < function.span.end)
                .max(index + 1);
        }
        if let Some(nested) = self.module.nested_functions.get(&token.start) {
            if self.visited_functions.insert(nested.span.start) {
                self.nested_function(nested, scope);
            }
            return tokens
                .partition_point(|token| token.start < nested.span.end)
                .max(index + 1);
        }
        if token.kind == TokenKind::Template {
            self.template(token, scope);
        } else if token.kind == TokenKind::JsxElement {
            if let Ok(element) =
                crate::syntax::parse_jsx(&self.module.id, &self.module.source, token.start)
            {
                self.jsx_names(&element, scope);
            }
        } else if is_value_name(token) {
            let previous = index.checked_sub(1).and_then(|i| tokens.get(i));
            let next = tokens.get(index + 1);
            let property =
                previous.is_some_and(|token| token.is(".") || token.is("?.") || token.is("#"));
            let key = next.is_some_and(|token| token.is(":"))
                && (index == 0
                    || previous
                        .is_some_and(|token| matches!(token.text.as_str(), "{" | "," | ";" | "}")));
            let label_use = previous.is_some_and(|token| token.is("break") || token.is("continue"));
            let erased = self.erased(token);
            if !property && !key && !label_use && !erased {
                self.reference(scope, token);
            }
        }
        index + 1
    }

    pub(super) fn nested_function(&mut self, function: &NestedFunction, parent: ScopeId) {
        let scope = self.child(Some(parent), function.span.clone(), true, true);
        self.scopes[scope]
            .types
            .extend(function.type_parameters.iter().map(|p| p.name.clone()));
        if function.kind != NestedFunctionKind::Arrow {
            self.value(scope, "arguments", 0, false, false, Type::Unknown);
        }
        if let Some(name) = &function.name {
            self.value(scope, name, 0, false, false, Type::Unknown);
        }
        self.parameters(&function.parameters, scope);
        match &function.body {
            NestedFunctionBody::Expression(tokens) => self.expression(tokens, scope),
            NestedFunctionBody::Block { items, .. } => {
                self.function_body(items, scope, &function.span)
            }
        }
    }

    pub(super) fn template(&mut self, token: &Token, scope: ScopeId) {
        let bytes = token.text.as_bytes();
        let mut index = 1;
        while index + 1 < bytes.len() {
            if bytes[index] == b'\\' {
                index += 2;
                continue;
            }
            if bytes[index..].starts_with(b"${") {
                let start = token.start + index + 2;
                if let Ok((end, tokens)) =
                    crate::syntax::lex_expression(&self.module.id, &self.module.source, start)
                {
                    self.expression(&tokens, scope);
                    index = end.saturating_sub(token.start) + 1;
                    continue;
                }
            }
            index += 1;
        }
    }

    pub(super) fn jsx_names(&mut self, element: &crate::jsx::JsxElement, scope: ScopeId) {
        if let Some(name) = &element.name {
            if name.text.starts_with(|ch: char| ch.is_uppercase()) || name.text.contains('.') {
                self.references.push(Reference {
                    scope,
                    name: name.text.clone(),
                    meaning: Meaning::Value,
                    span: SourceSpan::new(&self.module.id, name.start, name.end),
                });
            }
        }
        for attribute in &element.attributes {
            if let crate::jsx::JsxAttribute::Named {
                value: Some(crate::jsx::JsxValue::Element(element)),
                ..
            } = attribute
            {
                self.jsx_names(element, scope);
            }
        }
        for child in &element.children {
            if let crate::jsx::JsxChild::Element(element) = child {
                self.jsx_names(element, scope);
            }
        }
    }

    /// Raw statements and the parser's opaque body tokens still participate in
    /// lookup. Braces introduce lexical scopes; loop heads get a scope covering
    /// the whole loop, and `var` walks through it to its declaration boundary.
    pub(super) fn statements(&mut self, tokens: &[Token], scope: ScopeId) {
        let mut current = scope;
        let mut stack = Vec::new();
        let mut statement_scopes = BTreeSet::new();
        let mut loop_operators = BTreeSet::new();
        let mut index = 0;
        let mut declaration_names = BTreeSet::new();
        while index < tokens.len() {
            if declaration_names.contains(&index) {
                index += 1;
                continue;
            }
            let token = &tokens[index];
            if token.is("for") {
                if let Some(operator) = self.for_of_operator(tokens, index) {
                    loop_operators.insert(operator);
                }
                let end = loop_end(tokens, index).map_or(token.end, |end| tokens[end].end);
                current = self.child(
                    Some(current),
                    SourceSpan::new(&self.module.id, token.start, end),
                    false,
                    false,
                );
                statement_scopes.insert(current);
            }
            if token.is("{") {
                let end = matching_end(tokens, index, "{", "}")
                    .map_or(self.scopes[current].span.end, |end| tokens[end].end);
                stack.push(current);
                current = self.child(
                    Some(current),
                    SourceSpan::new(&self.module.id, token.start, end),
                    false,
                    false,
                );
            } else if token.is("}") {
                current = stack.pop().unwrap_or(scope);
                while statement_scopes.remove(&current) {
                    current = self.scopes[current].parent.unwrap_or(scope);
                }
            } else if matches!(token.text.as_str(), "const" | "let" | "var") {
                if let Some(name) = tokens.get(index + 1).filter(|token| is_value_name(token)) {
                    let mut target = current;
                    if token.is("var") {
                        while !self.scopes[target].var_boundary {
                            target = self.scopes[target].parent.unwrap();
                        }
                    }
                    let variable = self.local_variables.get(&token.start).cloned();
                    let value = variable
                        .as_ref()
                        .and_then(|variable| {
                            variable.annotation.clone().or_else(|| {
                                crate::parser::widen_literal_tokens(
                                    &variable.initializer,
                                    variable.kind == VariableKind::Const,
                                )
                            })
                        })
                        .or_else(|| {
                            self.module
                                .expression_variable_types
                                .get(&token.start)
                                .cloned()
                        })
                        .unwrap_or(Type::Unknown);
                    if let Some(variable) = &variable {
                        if let Some(annotation) = &variable.annotation {
                            self.type_scopes(annotation, &variable.span, current);
                        }
                    }
                    let ready = declaration_ready(tokens, index + 1);
                    self.value(
                        target,
                        &name.text,
                        if token.is("var")
                            || variable.as_ref().is_some_and(|variable| variable.declared)
                        {
                            0
                        } else {
                            ready
                        },
                        false,
                        false,
                        value,
                    );
                    let mut after = tokens.partition_point(|token| token.start < ready);
                    while tokens.get(after).is_some_and(|token| token.is(","))
                        && tokens.get(after + 1).is_some_and(is_value_name)
                    {
                        let next = &tokens[after + 1];
                        declaration_names.insert(after + 1);
                        let ready = declaration_ready(tokens, after + 1);
                        let annotation = self
                            .module
                            .expression_variable_types
                            .get(&next.start)
                            .cloned()
                            .unwrap_or(Type::Unknown);
                        self.value(
                            target,
                            &next.text,
                            if token.is("var") { 0 } else { ready },
                            false,
                            false,
                            annotation,
                        );
                        after = tokens.partition_point(|token| token.start < ready);
                    }
                    index += 2;
                    continue;
                }
            } else if token.is("catch") && tokens.get(index + 1).is_some_and(|t| t.is("(")) {
                if let Some(name) = tokens.get(index + 2) {
                    let open = (index + 3..tokens.len())
                        .find(|at| tokens[*at].is("{"))
                        .unwrap_or(tokens.len());
                    let end = matching_end(tokens, open, "{", "}")
                        .map_or(self.scopes[current].span.end, |end| tokens[end].end);
                    let catch = self.child(
                        Some(current),
                        SourceSpan::new(&self.module.id, token.start, end),
                        false,
                        false,
                    );
                    self.value(catch, &name.text, 0, false, false, Type::Unknown);
                    current = catch;
                    statement_scopes.insert(catch);
                    index += 3;
                    continue;
                }
            }
            index = if loop_operators.contains(&index) {
                index + 1
            } else {
                self.expression_token(tokens, index, current)
            };
        }
    }

    /// `of` is also a valid identifier. Only the first separator at the top
    /// level of a for-of head is syntax; RHS uses still resolve as values.
    fn for_of_operator(&self, tokens: &[Token], start: usize) -> Option<usize> {
        let open = (start + 1..tokens.len()).find(|index| tokens[*index].is("("))?;
        let close = matching_end(tokens, open, "(", ")")?;
        let first = open + 1;
        let binding_name = if matches!(tokens.get(first)?.text.as_str(), "const" | "let" | "var") {
            first + 1
        } else {
            first
        };
        let mut depth = 0usize;
        let mut separator = None;
        for (index, token) in tokens.iter().enumerate().take(close).skip(first) {
            if depth == 0 {
                if token.is(";") || (separator.is_none() && token.is("in")) {
                    return None;
                }
                if index != binding_name && token.is("of") && !self.erased(token) {
                    separator.get_or_insert(index);
                }
            }
            match token.text.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        separator
    }
}

fn is_value_name(token: &Token) -> bool {
    !token.text.starts_with('#')
        && (token.kind == TokenKind::Identifier
            || (token.kind == TokenKind::Keyword
                && matches!(
                    token.text.as_str(),
                    "any"
                        | "asserts"
                        | "constructor"
                        | "from"
                        | "is"
                        | "of"
                        | "get"
                        | "set"
                        | "type"
                        | "namespace"
                        | "module"
                        | "number"
                        | "string"
                        | "boolean"
                        | "object"
                        | "symbol"
                        | "async"
                        | "readonly"
                        | "abstract"
                        | "override"
                        | "declare"
                        | "infer"
                        | "keyof"
                        | "unique"
                        | "unknown"
                        | "never"
                )))
}

fn matching_end(tokens: &[Token], start: usize, open: &str, close: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        if token.is(open) {
            depth += 1;
        }
        if token.is(close) {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn declaration_ready(tokens: &[Token], name: usize) -> usize {
    let mut depth = 0usize;
    for token in &tokens[name + 1..] {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" if depth > 0 => depth -= 1,
            ";" | "," | ")" | "}" if depth == 0 => return token.start,
            _ => {}
        }
    }
    tokens.last().map_or(tokens[name].end, |token| token.end)
}

fn loop_end(tokens: &[Token], start: usize) -> Option<usize> {
    let close = matching_end(tokens, start + 1, "(", ")")?;
    if tokens.get(close + 1)?.is("{") {
        matching_end(tokens, close + 1, "{", "}")
    } else {
        (close + 1..tokens.len()).find(|index| tokens[*index].is(";"))
    }
}

fn regex_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = start + 1;
    let mut class = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => {
                index += 2;
                continue;
            }
            b'[' => class = true,
            b']' => class = false,
            b'/' if !class => {
                index += 1;
                while bytes.get(index).is_some_and(u8::is_ascii_alphabetic) {
                    index += 1;
                }
                return Some(index);
            }
            b'\n' | b'\r' => return None,
            _ => {}
        }
        index += 1;
    }
    None
}
