// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bind the checked variable pattern and its defaults after exact replay.

use super::*;
use blueice_bluets::{BindingPattern, VariableBindingPattern};

impl Evidence<'_> {
    pub(super) fn variable(
        &mut self,
        original: &VariableDeclaration,
        actual: &bluejs::VarDeclarator,
    ) {
        if self.error.is_some() {
            return;
        }
        let result = self.variable_binding(original, &actual.pattern);
        if let Err(error) = result {
            self.error = Some(error);
            return;
        }
        if let Some(value) = &actual.init {
            self.expression(value, &original.initializer);
        }
    }

    fn variable_binding(
        &mut self,
        original: &VariableDeclaration,
        actual: &bluejs::Pattern,
    ) -> Result<(), BridgeError> {
        let Some(pattern) = &original.pattern else {
            // Module default-export bindings are generated names; they have
            // no written binding token and keep an explicit gap.
            if matches!(actual, bluejs::Pattern::Identifier(name) if name == &original.name) {
                if let Some(span) = self.variable_name_span(original) {
                    self.bind(DiagnosticSourceNode::Pattern(actual), &span);
                }
            }
            return Ok(());
        };
        let replayed = crate::lowering::patterns::variable(self.module, pattern)?;
        if !same_syntax(&replayed, actual)? {
            return Err(invalid());
        }
        let span = self.variable_name_span(original).ok_or_else(invalid)?;
        self.bind(DiagnosticSourceNode::Pattern(actual), &span);
        self.pattern_children(pattern, actual)
    }

    fn pattern_children(
        &mut self,
        original: &VariableBindingPattern,
        actual: &bluejs::Pattern,
    ) -> Result<(), BridgeError> {
        match (&original.pattern, actual) {
            (BindingPattern::Object(bindings), bluejs::Pattern::Object(properties)) => {
                for (binding, property) in bindings.iter().zip(properties) {
                    let bluejs::ObjectPatternProp::KeyValue { value, default, .. } = property
                    else {
                        return Err(invalid());
                    };
                    let span = self
                        .binding_name_span(&binding.span, &binding.name, Some(&binding.key), false)
                        .ok_or_else(invalid)?;
                    self.bind(DiagnosticSourceNode::Pattern(value), &span);
                    self.pattern_default(&binding.default, default)?;
                }
                if let Some(rest) = &original.rest {
                    let Some(bluejs::ObjectPatternProp::Rest(actual)) = properties.last() else {
                        return Err(invalid());
                    };
                    let span = self
                        .binding_name_span(&rest.span, &rest.name, None, true)
                        .ok_or_else(invalid)?;
                    self.bind(DiagnosticSourceNode::Pattern(actual), &span);
                }
            }
            (BindingPattern::Array(bindings), bluejs::Pattern::Array(elements)) => {
                for (binding, element) in bindings.iter().zip(elements) {
                    match (binding, element) {
                        (None, None) => {}
                        (Some(binding), Some(element)) => {
                            let span = self
                                .binding_name_span(&binding.span, &binding.name, None, false)
                                .ok_or_else(invalid)?;
                            self.bind(DiagnosticSourceNode::Pattern(&element.pattern), &span);
                            self.pattern_default(&binding.default, &element.default)?;
                        }
                        _ => return Err(invalid()),
                    }
                }
                if let Some(rest) = &original.rest {
                    let Some(Some(actual)) = elements.last() else {
                        return Err(invalid());
                    };
                    let span = self
                        .binding_name_span(&rest.span, &rest.name, None, true)
                        .ok_or_else(invalid)?;
                    self.bind(DiagnosticSourceNode::Pattern(&actual.pattern), &span);
                }
            }
            _ => return Err(invalid()),
        }
        Ok(())
    }

    fn pattern_default(
        &mut self,
        original: &Option<Vec<Token>>,
        actual: &Option<bluejs::Expr>,
    ) -> Result<(), BridgeError> {
        match (original, actual) {
            (Some(tokens), Some(expression)) => self.expression(expression, tokens),
            (None, None) => {}
            _ => return Err(invalid()),
        }
        Ok(())
    }

    fn variable_name_span(&self, original: &VariableDeclaration) -> Option<SourceSpan> {
        let tokens = self.binding_tokens(&original.span)?;
        let keyword = tokens
            .iter()
            .position(|token| token.text == original.kind.as_str())?;
        if !tokens[..keyword]
            .iter()
            .all(|token| matches!(token.text.as_str(), "export" | "declare"))
        {
            return None;
        }
        let token = tokens.get(keyword + 1)?;
        if let Some(pattern) = &original.pattern {
            let opening = match pattern.pattern {
                BindingPattern::Object(_) => "{",
                BindingPattern::Array(_) => "[",
            };
            if token.text != opening {
                return None;
            }
            let end = token.start.checked_add(original.name.len())?;
            if end > original.span.end || self.module.source.get(token.start..end)? != original.name
            {
                return None;
            }
        } else if token.kind != TokenKind::Identifier || token.text != original.name {
            return None;
        }
        Some(token.span(&self.module.id))
    }

    fn binding_name_span(
        &self,
        span: &SourceSpan,
        name: &str,
        key: Option<&str>,
        rest: bool,
    ) -> Option<SourceSpan> {
        let tokens = self.binding_tokens(span)?;
        let first = tokens.first()?;
        let token = if rest {
            if first.text != "..." {
                return None;
            }
            tokens.get(1)?
        } else if let Some(key) = key {
            if first.text != key {
                return None;
            }
            if tokens.get(1).is_some_and(|token| token.text == ":") {
                tokens.get(2)?
            } else {
                first
            }
        } else {
            first
        };
        // The checked object-pattern grammar also accepts contextual keyword
        // shorthands such as `async`; their spelling still proves the binding.
        if !matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword) || token.text != name {
            return None;
        }
        Some(token.span(&self.module.id))
    }

    fn binding_tokens(&self, span: &SourceSpan) -> Option<&[Token]> {
        if span.module != self.module.id || span.start > span.end {
            return None;
        }
        let start = self
            .written_tokens
            .partition_point(|token| token.start < span.start);
        let end = self
            .written_tokens
            .partition_point(|token| token.end <= span.end);
        self.written_tokens.get(start..end)
    }

    pub(super) fn parameter_binding(&mut self, original: &Parameter, actual: &bluejs::Param) {
        if original.pattern.is_some()
            || original.rest != actual.rest
            || !matches!(&actual.pattern, bluejs::Pattern::Identifier(name) if name == &original.name)
        {
            return;
        }
        if let Some(span) =
            self.binding_name_span(&original.span, &original.name, None, original.rest)
        {
            self.bind(DiagnosticSourceNode::Pattern(&actual.pattern), &span);
        }
    }

    pub(super) fn catch_binding(
        &mut self,
        original: &blueice_bluets::FunctionCatchClause,
        actual: &bluejs::CatchClause,
    ) {
        let Some(pattern @ bluejs::Pattern::Identifier(name)) = &actual.param else {
            return;
        };
        if name != &original.binding {
            return;
        }
        let span = self.binding_tokens(&original.span).and_then(|tokens| {
            let [keyword, opening, name, ..] = tokens else {
                return None;
            };
            (keyword.text == "catch" && opening.text == "(" && name.text == original.binding)
                .then(|| name.span(&self.module.id))
        });
        if let Some(span) = span {
            self.bind(DiagnosticSourceNode::Pattern(pattern), &span);
        }
    }
}
