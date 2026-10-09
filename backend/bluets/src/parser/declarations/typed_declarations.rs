// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Typed declaration forms.

use super::*;

impl Parser {
    pub(in crate::parser::implementation) fn parse_type_alias(
        &mut self,
        start: usize,
        exported: bool,
    ) {
        let name = if self.peek("asserts") || self.peek("is") {
            let name = self.current().text.clone();
            self.bump();
            name
        } else {
            self.require_identifier("expected a type alias name")
        };
        let type_parameters = self.parse_type_parameters();
        self.expect("=");
        let value = self.parse_type_until(&[";"]);
        self.consume(";");
        let end = self.previous().end;
        self.edits.push(TextEdit {
            start,
            end,
            replacement: String::new(),
        });
        self.declarations
            .push(Declaration::TypeAlias(TypeAliasDeclaration {
                name,
                type_parameters,
                value,
                exported,
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    pub(in crate::parser::implementation) fn parse_interface(
        &mut self,
        start: usize,
        exported: bool,
    ) {
        let name = self.require_identifier("expected an interface name");
        let type_parameters = self.parse_type_parameters();
        let mut heritage = Vec::new();
        if self.consume("extends") {
            loop {
                let parent_start = self.current().start;
                let parent = self.parse_type_until(&[",", "{"]);
                if !matches!(&parent, Type::Named { .. }) {
                    self.unsupported(
                        SourceSpan::new(&self.id, parent_start, self.previous().end),
                        "interface heritage supports only named interface types",
                    );
                }
                heritage.push(parent);
                if !self.consume(",") {
                    break;
                }
            }
        }
        self.expect("{");
        let body = self.parse_record_type("expected an interface field name");
        let (body, indices) = match body {
            Type::IndexedRecord { object, indices } => (*object, indices),
            value => (value, Vec::new()),
        };
        let (fields, signatures) = match body {
            Type::Record(fields) => (fields, Vec::new()),
            Type::CallableRecord { fields, signatures } => (fields, signatures),
            _ => unreachable!(),
        };
        let end = self.previous().end;
        self.edits.push(TextEdit {
            start,
            end,
            replacement: String::new(),
        });
        self.declarations
            .push(Declaration::Interface(InterfaceDeclaration {
                name,
                type_parameters,
                heritage,
                fields,
                signatures,
                indices,
                exported,
                default_export: false,
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    pub(in crate::parser::implementation) fn parse_variable(
        &mut self,
        start: usize,
        exported: bool,
        declared: bool,
        kind: VariableKind,
    ) {
        let declaration = self.parse_variable_declaration(start, exported, declared, kind);
        self.declarations.push(Declaration::Variable(declaration));
    }

    pub(in crate::parser::implementation) fn parse_variable_declaration(
        &mut self,
        start: usize,
        exported: bool,
        declared: bool,
        kind: VariableKind,
    ) -> VariableDeclaration {
        let name = self.require_identifier("expected a variable name");
        if self.consume("?") {
            self.unsupported(
                self.previous().span(&self.id),
                "optional variables are not valid TypeScript declarations",
            );
        }
        let annotation_start = self.current().start;
        let annotation = if self.consume(":") {
            let value = self.parse_type_until(&["=", ";"]);
            let annotation_end = self.current().start;
            self.edits.push(TextEdit {
                start: annotation_start,
                end: annotation_end,
                replacement: String::new(),
            });
            Some(value)
        } else {
            None
        };
        let initializer = if self.consume("=") {
            let initializer_start = self.index;
            let initializer = self.collect_until_statement_end();
            self.collect_expression_type_edits(initializer_start, self.index);
            self.collect_following_variable_types(initializer_start, self.index);
            initializer
        } else {
            Vec::new()
        };
        if declared && !initializer.is_empty() {
            // An ambient `const` may only be initialized, without a type, by a
            // literal.
            let literal = matches!(
                initializer.as_slice(),
                [token] if matches!(token.kind, TokenKind::String | TokenKind::Number)
            ) || matches!(
                initializer.as_slice(),
                [sign, token] if sign.is("-") && token.kind == TokenKind::Number
            );
            if annotation.is_some() || kind != VariableKind::Const || !literal {
                self.error_at(
                    initializer[0].span(&self.id),
                    DiagnosticCode::ParseError,
                    "initializers are not allowed in ambient contexts",
                );
            }
        }
        self.consume(";");
        let end = self.previous().end;
        if declared {
            self.edits.push(TextEdit {
                start,
                end,
                replacement: String::new(),
            });
        }
        VariableDeclaration {
            name,
            kind,
            annotation,
            initializer,
            exported,
            declared,
            span: SourceSpan::new(&self.id, start, end),
        }
    }

    pub(in crate::parser::implementation) fn parse_function(
        &mut self,
        start: usize,
        exported: bool,
        default_export: bool,
        declared: bool,
        async_start: bool,
    ) {
        // `function* name`: a generator.
        let generator = self.consume("*");
        if generator && async_start {
            self.unsupported(
                self.previous().span(&self.id),
                "an async generator is not supported yet",
            );
        }
        let name = self.require_identifier("expected a function name");
        let type_parameter_start = self.current().start;
        let type_parameters = self.parse_type_parameters();
        if !type_parameters.is_empty() {
            self.edits.push(TextEdit {
                start: type_parameter_start,
                end: self.current().start,
                replacement: String::new(),
            });
        }
        let parameters = self.parse_parameters();
        let return_start = self.current().start;
        let return_type = if self.consume(":") {
            let value = self.parse_return_type_until(&["{", ";"]);
            let return_end = self.current().start;
            self.edits.push(TextEdit {
                start: return_start,
                end: return_end,
                replacement: String::new(),
            });
            Some(value)
        } else {
            None
        };

        let mut body = Vec::new();
        let mut returns = Vec::new();
        let mut locals = Vec::new();
        let body_open = self.peek("{").then(|| self.current().start);
        let overload = if self.consume("{") {
            let body_start = self.previous().start;
            self.parse_function_body(body_start, &mut body, &mut returns, &mut locals);
            false
        } else if self.consume(";") {
            !declared
        } else if !declared {
            self.error_here(DiagnosticCode::ParseError, "expected a function body");
            false
        } else {
            self.expect(";");
            false
        };
        let end = self.previous().end;
        if declared || overload {
            self.edits.push(TextEdit {
                start,
                end,
                replacement: String::new(),
            });
        }
        self.declarations
            .push(Declaration::Function(FunctionDeclaration {
                name,
                anonymous: false,
                async_function: async_start,
                generator,
                body_open,
                type_parameters,
                parameters,
                return_type,
                body,
                returns,
                locals,
                exported,
                default_export,
                declared,
                overload,
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    /// `[public|protected|private] [readonly]` before a constructor parameter,
    /// erased from the output. `None` when the parameter has neither, so it is
    /// an ordinary parameter. A modifier word counts only when a parameter
    /// follows it; `constructor(readonly)` names a parameter `readonly`.
    fn consume_parameter_property_modifiers(&mut self) -> Option<(Visibility, bool, bool, usize)> {
        let starts_parameter = |token: Option<&Token>| {
            token.is_some_and(|token| {
                matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword)
                    || token.is("...")
                    || token.is("{")
                    || token.is("[")
            })
        };
        let first = self.index;
        let mut visibility = None;
        let mut readonly = false;
        let mut override_modifier = false;
        let mut misplaced = false;
        while starts_parameter(self.tokens.get(self.index + 1)) {
            let word = self.tokens[self.index].text.as_str();
            match word {
                "public" | "protected" | "private" => {
                    misplaced |= visibility.is_some() || readonly || override_modifier;
                    visibility = Some(match word {
                        "protected" => Visibility::Protected,
                        "private" => Visibility::Private,
                        _ => Visibility::Public,
                    });
                }
                "readonly" => {
                    misplaced |= readonly;
                    readonly = true;
                }
                "override" => {
                    misplaced |= readonly || override_modifier;
                    override_modifier = true;
                }
                _ => break,
            }
            self.index += 1;
        }
        if self.index == first {
            return None;
        }
        if misplaced {
            self.error_at(
                SourceSpan::new(
                    &self.id,
                    self.tokens[first].start,
                    self.tokens[self.index - 1].end,
                ),
                DiagnosticCode::ParseError,
                "parameter modifiers must be an accessibility modifier, then `readonly`, each at most once",
            );
        }
        self.edits.push(TextEdit {
            start: self.tokens[first].start,
            end: self.current().start,
            replacement: String::new(),
        });
        Some((
            visibility.unwrap_or_default(),
            readonly,
            override_modifier,
            self.tokens[first].start,
        ))
    }

    pub(in crate::parser::implementation) fn parse_parameters(&mut self) -> Vec<Parameter> {
        self.expect("(");
        let mut parameters = Vec::new();
        while !self.at_eof() && !self.consume(")") {
            let property_modifiers = if self.parameter_property_mode
                || matches!(
                    self.current().text.as_str(),
                    "public" | "protected" | "private" | "readonly" | "override"
                ) {
                self.consume_parameter_property_modifiers()
            } else {
                None
            };
            // Legacy parameter decorators come first; the modifiers of a parameter
            // property follow them.
            let decorators = self.parse_decorators();
            let property_modifiers = if self.parameter_property_mode && property_modifiers.is_none()
            {
                self.consume_parameter_property_modifiers()
            } else {
                property_modifiers
            };
            let parameter_start = self.current().start;
            let rest = self.consume("...");
            let mut pattern = None;
            let parameter_name = if !rest && (self.peek("{") || self.peek("[")) {
                let open = self.index;
                match patterns::parse_binding_pattern(&self.tokens, open, &self.id) {
                    Some((parsed, close)) => {
                        let text = self.source[self.tokens[open].start..self.tokens[close].end]
                            .to_string();
                        self.index = close + 1;
                        pattern = Some(parsed);
                        text
                    }
                    None => {
                        self.unsupported(
                            self.current().span(&self.id),
                            "this destructuring pattern is not supported yet",
                        );
                        // Skip the pattern so parsing can continue.
                        let end = find_balanced_delimiter(
                            &self.tokens,
                            open,
                            self.tokens.len() - 1,
                            &[",", ")", ":", "=", "?"],
                        );
                        self.index = end.max(open + 1);
                        "<pattern>".to_string()
                    }
                }
            } else {
                let this_parameter = self.peek("this");
                let name = self.require_identifier("expected a parameter name");
                if this_parameter {
                    if let Some(diagnostic) = self.diagnostics.last_mut() {
                        *diagnostic = diagnostic.clone().blue_only(
                            "BlueTSC's named parameter subset does not parse TypeScript this parameters.",
                        );
                    }
                }
                name
            };
            let optional_start = self.current().start;
            let mut optional = self.consume("?");
            if optional {
                self.edits.push(TextEdit {
                    start: optional_start,
                    end: self.current().start,
                    replacement: String::new(),
                });
            }
            let annotation_start = self.current().start;
            let annotation = if self.consume(":") {
                let value = self.parse_type_until(&["=", ",", ")"]);
                let annotation_end = self.current().start;
                self.edits.push(TextEdit {
                    start: annotation_start,
                    end: annotation_end,
                    replacement: String::new(),
                });
                Some(value)
            } else {
                None
            };
            let default = if self.consume("=") {
                // A default initializer makes a parameter omittable at a call
                // site just like `?`. Retain the original runtime tokens for
                // direct BlueJS lowering while the emitter preserves source.
                optional = true;
                let end = find_balanced_delimiter(
                    &self.tokens,
                    self.index,
                    self.tokens.len() - 1,
                    &[",", ")"],
                );
                let value = self.tokens[self.index..end].to_vec();
                self.index = end;
                if value.is_empty() {
                    self.error_here(DiagnosticCode::ParseError, "expected a default initializer");
                }
                Some(value)
            } else {
                None
            };
            let parameter_end = self.previous().end;
            if let Some((visibility, readonly, override_modifier, modifiers_start)) =
                property_modifiers
            {
                if !self.parameter_property_mode {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ParseError,
                            SourceSpan::new(&self.id, parameter_start, parameter_end),
                            "a parameter property is only allowed in a constructor implementation",
                        )
                        .with_typescript(2369, Vec::new()),
                    );
                }
                if rest || pattern.is_some() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ParseError,
                            SourceSpan::new(&self.id, parameter_start, parameter_end),
                            "a parameter property cannot be a rest parameter or a binding pattern",
                        )
                        .with_typescript(if rest { 1317 } else { 1187 }, Vec::new()),
                    );
                }
                if self.parameter_property_mode {
                    self.parameter_properties.push(ParameterProperty {
                        parameter_index: parameters.len(),
                        visibility,
                        readonly,
                        override_modifier,
                        modifiers_start,
                    });
                }
            }
            parameters.push(Parameter {
                decorators,
                name: parameter_name,
                pattern,
                rest,
                optional,
                annotation,
                default,
                span: SourceSpan::new(&self.id, parameter_start, parameter_end),
            });
            if !self.consume(",") {
                self.expect(")");
                break;
            }
        }
        parameters
    }
}
