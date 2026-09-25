// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module declarations, function bodies, and erasure edits.

use super::runtime_syntax::*;
use super::*;

impl Parser {
    pub(crate) fn new(
        id: String,
        source: String,
        tokens: Vec<Token>,
        max_type_depth: usize,
    ) -> Self {
        Self {
            id,
            source,
            // TypeScript permits adjacent generic closers without whitespace,
            // even though the JavaScript lexer initially recognizes `>>` and
            // `>>>` as shift operators.  The BlueTS parser does not parse
            // runtime expressions from this token stream (the emitter keeps
            // their original source), so present each closer independently to
            // the type grammar while retaining its exact source span.
            tokens: split_generic_closers(tokens),
            index: 0,
            declarations: Vec::new(),
            edits: Vec::new(),
            generic_call_type_arguments: BTreeMap::new(),
            diagnostics: Vec::new(),
            max_type_depth,
            type_depth: 0,
        }
    }

    pub(crate) fn parse_module(mut self) -> Result<Module, Vec<Diagnostic>> {
        if self.id.ends_with(".tsx") {
            self.unsupported(
                SourceSpan::new(&self.id, 0, self.source.len()),
                "TSX/JSX is not in the initial BlueTS matrix",
            );
            return Err(self.diagnostics);
        }
        self.diagnose_unparenthesized_nullish_logical_mixing();
        self.diagnose_unparenthesized_unary_exponentiation();
        while !self.at_eof() {
            if self.consume(";") {
                continue;
            }
            let start = self.current().start;
            let exported = self.consume("export");
            if exported && self.consume("default") {
                let default_span = self.previous().span(&self.id);
                let async_start = self.consume("async");
                if self.consume("function") {
                    if self.current().kind == TokenKind::Identifier {
                        self.parse_function(start, true, true, false, async_start);
                    } else {
                        self.unsupported(
                            default_span,
                            "anonymous default function exports are not in the initial BlueTS matrix",
                        );
                        self.skip_statement();
                    }
                } else if !async_start
                    && self.current().kind == TokenKind::Identifier
                    && (self
                        .tokens
                        .get(self.index + 1)
                        .is_some_and(|token| token.is(";"))
                        || self
                            .tokens
                            .get(self.index + 1)
                            .is_some_and(|token| token.kind == TokenKind::Eof))
                {
                    self.parse_default_export(start);
                } else {
                    self.unsupported(
                        default_span,
                        "default export expressions are not in the initial BlueTS matrix",
                    );
                    self.skip_statement();
                }
                continue;
            }
            if exported && self.peek("=") {
                self.unsupported(
                    self.current().span(&self.id),
                    "`export =` is not in the initial BlueTS matrix",
                );
                self.skip_statement();
                continue;
            }
            if exported && self.peek("*") {
                self.unsupported(
                    self.current().span(&self.id),
                    "value re-exports from another module are not in the initial BlueTS matrix",
                );
                self.skip_statement();
                continue;
            }
            if exported && self.peek("{") {
                self.parse_value_export(start);
                continue;
            }

            if self.peek("import") {
                if exported {
                    self.error_here(DiagnosticCode::ParseError, "an import cannot be exported");
                }
                self.parse_import(start);
            } else if self.consume("type") {
                if exported && (self.peek("{") || self.peek("*")) {
                    self.parse_type_export(start);
                } else {
                    self.parse_type_alias(start, exported);
                }
            } else if self.consume("interface") {
                self.parse_interface(start, exported);
            } else if self.peek("abstract") {
                self.unsupported(
                    self.current().span(&self.id),
                    "`abstract` declarations are not in the initial BlueTS matrix",
                );
                self.skip_statement();
            } else {
                let declared = self.consume("declare");
                let async_start = self.consume("async");
                if self.consume("function") {
                    self.parse_function(start, exported, false, declared, async_start);
                } else if self.peek("const")
                    && self
                        .tokens
                        .get(self.index + 1)
                        .is_some_and(|token| token.is("enum"))
                {
                    self.unsupported(
                        self.tokens[self.index + 1].span(&self.id),
                        "`enum` is not in the initial BlueTS matrix",
                    );
                    self.skip_statement();
                } else if self.peek("const") || self.peek("let") || self.peek("var") {
                    let kind = match self.current().text.as_str() {
                        "const" => VariableKind::Const,
                        "let" => VariableKind::Let,
                        "var" => VariableKind::Var,
                        _ => unreachable!("variable declaration was guarded by its keyword"),
                    };
                    self.bump();
                    self.parse_variable(start, exported, declared, kind);
                } else if self.peek_any(&[
                    "enum",
                    "namespace",
                    "module",
                    "class",
                    "abstract",
                    "implements",
                    "decorator",
                ]) {
                    self.unsupported(
                        self.current().span(&self.id),
                        format!(
                            "`{}` is not in the initial BlueTS matrix",
                            self.current().text
                        ),
                    );
                    self.skip_statement();
                } else if self.peek("@")
                    || (self.peek("<")
                        && self
                            .tokens
                            .get(self.index + 1)
                            .is_some_and(|token| token.kind == TokenKind::Identifier))
                {
                    self.unsupported(
                        self.current().span(&self.id),
                        "decorators and TSX/JSX are not in the initial BlueTS matrix",
                    );
                    self.skip_statement();
                } else {
                    if declared {
                        self.error_here(
                            DiagnosticCode::ParseError,
                            "`declare` must introduce a supported declaration",
                        );
                    }
                    if async_start {
                        self.error_here(
                            DiagnosticCode::ParseError,
                            "`async` must precede a function declaration in the initial matrix",
                        );
                    }
                    self.parse_raw(start);
                }
            }
        }

        if self.diagnostics.is_empty() {
            self.edits.sort_by_key(|edit| (edit.start, edit.end));
            Ok(Module {
                id: self.id,
                source: self.source,
                declarations: self.declarations,
                edits: self.edits,
                generic_call_type_arguments: self.generic_call_type_arguments,
            })
        } else {
            Err(self.diagnostics)
        }
    }

    pub(super) fn parse_import(&mut self, start: usize) {
        self.expect("import");
        let type_only = self.consume("type");
        if !type_only
            && self.current().kind == TokenKind::Identifier
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| token.is("="))
        {
            self.unsupported(
                self.current().span(&self.id),
                "`import =` is not in the initial BlueTS matrix",
            );
            self.skip_statement();
            return;
        }
        let mut bindings = Vec::new();
        let mut specifier = None;
        let mut specifier_span = None;
        if self.current().kind == TokenKind::String {
            specifier = string_contents(self.current());
            specifier_span = Some(self.current().span(&self.id));
            self.bump();
        } else {
            if self.consume("{") {
                while !self.at_eof() && !self.consume("}") {
                    let binding_type_only = self.consume("type");
                    let Some(imported) = self.consume_identifier() else {
                        self.error_here(DiagnosticCode::ParseError, "expected an imported binding");
                        self.skip_until(&["}", ";"]);
                        self.consume("}");
                        break;
                    };
                    let local = if self.consume("as") {
                        self.require_identifier("expected a local import name")
                    } else {
                        imported.clone()
                    };
                    bindings.push(ImportBinding {
                        imported,
                        local,
                        type_only: type_only || binding_type_only,
                    });
                    if !self.consume(",") {
                        self.expect("}");
                        break;
                    }
                }
            } else if let Some(local) = self.consume_identifier() {
                bindings.push(ImportBinding {
                    imported: "default".to_string(),
                    local,
                    type_only,
                });
                self.consume(",");
                if self.consume("*") {
                    self.expect("as");
                    let local = self.require_identifier("expected namespace import name");
                    bindings.push(ImportBinding {
                        imported: "*".to_string(),
                        local,
                        type_only,
                    });
                }
            } else if self.consume("*") {
                self.expect("as");
                let local = self.require_identifier("expected namespace import name");
                bindings.push(ImportBinding {
                    imported: "*".to_string(),
                    local,
                    type_only,
                });
            } else {
                self.error_here(
                    DiagnosticCode::ParseError,
                    "expected an import clause or module specifier",
                );
            }
            self.expect("from");
            if self.current().kind == TokenKind::String {
                specifier = string_contents(self.current());
                specifier_span = Some(self.current().span(&self.id));
                self.bump();
            } else {
                self.error_here(
                    DiagnosticCode::ParseError,
                    "expected a string module specifier",
                );
            }
        }
        self.consume(";");
        let end = self.previous().end;
        let span = SourceSpan::new(&self.id, start, end);
        let (Some(specifier), Some(specifier_span)) = (specifier, specifier_span) else {
            return;
        };
        if !type_only && bindings.iter().any(|binding| binding.type_only) {
            self.unsupported(
                span.clone(),
                "mixed value/type imports are not in the initial BlueTS matrix; use a separate `import type` declaration",
            );
        }
        if type_only || bindings.iter().all(|binding| binding.type_only) && !bindings.is_empty() {
            self.edits.push(TextEdit {
                start,
                end,
                replacement: String::new(),
            });
        }
        self.declarations
            .push(Declaration::Import(ImportDeclaration {
                type_only,
                specifier,
                specifier_span,
                bindings,
                span,
            }));
    }

    pub(super) fn parse_type_export(&mut self, start: usize) {
        let mut bindings = Vec::new();
        if self.consume("{") {
            while !self.at_eof() && !self.consume("}") {
                let local = self.require_identifier("expected a type export name");
                let exported = if self.consume("as") {
                    self.require_identifier("expected an exported type name")
                } else {
                    local.clone()
                };
                bindings.push(if local == exported {
                    local
                } else {
                    format!("{local} as {exported}")
                });
                if !self.consume(",") {
                    self.expect("}");
                    break;
                }
            }
        } else {
            self.expect("*");
            bindings.push("*".to_string());
        }
        let specifier = if self.consume("from") {
            if self.current().kind == TokenKind::String {
                let value = string_contents(self.current());
                self.bump();
                value
            } else {
                self.error_here(
                    DiagnosticCode::ParseError,
                    "expected a string module specifier",
                );
                None
            }
        } else {
            None
        };
        self.consume(";");
        let end = self.previous().end;
        self.edits.push(TextEdit {
            start,
            end,
            replacement: String::new(),
        });
        self.declarations
            .push(Declaration::TypeExport(TypeExportDeclaration {
                bindings,
                specifier,
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    pub(super) fn parse_default_export(&mut self, start: usize) {
        let name = self.require_identifier("expected a default export name");
        self.consume(";");
        let end = self.previous().end;
        self.declarations
            .push(Declaration::DefaultExport(DefaultExportDeclaration {
                name,
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    pub(super) fn parse_value_export(&mut self, start: usize) {
        self.expect("{");
        let mut bindings = Vec::new();
        while !self.at_eof() && !self.consume("}") {
            if self.peek("type") {
                self.unsupported(
                    self.current().span(&self.id),
                    "type-only bindings in a value export are not in the initial BlueTS matrix; use `export type`",
                );
                self.skip_statement();
                return;
            }
            let binding_start = self.current().start;
            let local = self.require_identifier("expected a value export name");
            let exported = if self.consume("as") {
                self.require_identifier("expected an exported value name")
            } else {
                local.clone()
            };
            if exported == "default" {
                self.unsupported(
                    SourceSpan::new(&self.id, binding_start, self.previous().end),
                    "default aliases in named value exports are not in the initial BlueTS matrix",
                );
            }
            bindings.push(ValueExportBinding {
                local,
                exported,
                span: SourceSpan::new(&self.id, binding_start, self.previous().end),
            });
            if !self.consume(",") {
                self.expect("}");
                break;
            }
        }
        if self.consume("from") {
            self.unsupported(
                self.previous().span(&self.id),
                "value re-exports from another module are not in the initial BlueTS matrix",
            );
            self.skip_statement();
            return;
        }
        self.consume(";");
        let end = self.previous().end;
        self.declarations
            .push(Declaration::ValueExport(ValueExportDeclaration {
                bindings,
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    pub(super) fn parse_type_alias(&mut self, start: usize, exported: bool) {
        let name = self.require_identifier("expected a type alias name");
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

    pub(super) fn parse_interface(&mut self, start: usize, exported: bool) {
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
        let mut fields = Vec::new();
        let mut closed = false;
        while !self.at_eof() {
            if self.consume("}") {
                closed = true;
                break;
            }
            let readonly = self.consume("readonly");
            let field_start = self.current().start;
            let name = self.require_property_name("expected an interface field name");
            let optional = self.consume("?");
            let value = if self.peek("(") {
                self.parse_method_signature(&[";", ",", "}"])
            } else {
                self.expect(":");
                self.parse_type_until(&[";", ",", "}"])
            };
            let end = self.previous().end;
            fields.push(TypeField {
                name,
                readonly,
                optional,
                value,
                span: SourceSpan::new(&self.id, field_start, end),
            });
            self.consume(";");
            self.consume(",");
        }
        if !closed {
            self.expect("}");
        }
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
                exported,
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    pub(super) fn parse_variable(
        &mut self,
        start: usize,
        exported: bool,
        declared: bool,
        kind: VariableKind,
    ) {
        let declaration = self.parse_variable_declaration(start, exported, declared, kind);
        self.declarations.push(Declaration::Variable(declaration));
    }

    pub(super) fn parse_variable_declaration(
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
            initializer
        } else {
            Vec::new()
        };
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

    pub(super) fn parse_function(
        &mut self,
        start: usize,
        exported: bool,
        default_export: bool,
        declared: bool,
        _async_start: bool,
    ) {
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
        self.expect("(");
        let mut parameters = Vec::new();
        while !self.at_eof() && !self.consume(")") {
            let parameter_start = self.current().start;
            let rest = self.consume("...");
            let parameter_name = self.require_identifier("expected a parameter name");
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
            parameters.push(Parameter {
                name: parameter_name,
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
        let return_start = self.current().start;
        let return_type = if self.consume(":") {
            let value = self.parse_type_until(&["{", ";"]);
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

    pub(super) fn parse_function_body(
        &mut self,
        body_start: usize,
        body: &mut Vec<FunctionBodyItem>,
        returns: &mut Vec<Vec<Token>>,
        locals: &mut Vec<VariableDeclaration>,
    ) {
        let mut depth = 1usize;
        let mut parentheses = 0usize;
        let mut brackets = 0usize;
        while !self.at_eof() && depth > 0 {
            self.diagnose_unsupported_opaque_syntax(
                self.index,
                self.index.saturating_add(2).min(self.tokens.len()),
            );
            if is_generic_arrow_function(&self.tokens, self.index, self.tokens.len() - 1) {
                self.unsupported(
                    self.tokens[self.index].span(&self.id),
                    "generic arrow functions are not in the initial BlueTS matrix",
                );
            }
            if parentheses == 0
                && brackets == 0
                && is_direct_braced_if_statement(&self.tokens, self.index)
            {
                body.push(FunctionBodyItem::If(
                    self.parse_direct_braced_if_statement(returns, locals),
                ));
                continue;
            }
            if self.consume("{") {
                depth += 1;
                body.push(FunctionBodyItem::Opaque(self.previous().span(&self.id)));
                continue;
            }
            if self.consume("}") {
                depth -= 1;
                if depth > 0 {
                    body.push(FunctionBodyItem::Opaque(self.previous().span(&self.id)));
                }
                continue;
            }
            if self.consume("return") {
                let return_start = self.previous().start;
                let expression_start = self.index;
                let expression = self.collect_until_statement_end();
                self.collect_expression_type_edits(expression_start, self.index);
                returns.push(expression.clone());
                self.consume(";");
                body.push(FunctionBodyItem::Return {
                    tokens: expression,
                    span: SourceSpan::new(&self.id, return_start, self.previous().end),
                });
                continue;
            }
            if self.consume("throw") {
                let throw_token = self.previous().clone();
                let line_terminator_after_throw = self
                    .source
                    .get(throw_token.end..self.current().start)
                    .is_some_and(|gap| gap.contains('\n') || gap.contains('\r'));
                let expression_start = self.index;
                let tokens = self.collect_until_function_statement_end();
                self.collect_expression_type_edits(expression_start, self.index);
                if line_terminator_after_throw {
                    self.error_at(
                        throw_token.span(&self.id),
                        DiagnosticCode::ParseError,
                        "a line terminator is not permitted after `throw`",
                    );
                }
                if tokens.is_empty() {
                    self.error_at(
                        throw_token.span(&self.id),
                        DiagnosticCode::ParseError,
                        "expected an expression after `throw`",
                    );
                }
                self.consume(";");
                body.push(FunctionBodyItem::Throw {
                    tokens,
                    span: SourceSpan::new(&self.id, throw_token.start, self.previous().end),
                });
                continue;
            }
            if self.peek("const") || self.peek("let") || self.peek("var") {
                let start = self.current().start;
                let kind = match self.current().text.as_str() {
                    "const" => VariableKind::Const,
                    "let" => VariableKind::Let,
                    "var" => VariableKind::Var,
                    _ => unreachable!("variable declaration was guarded by its keyword"),
                };
                self.bump();
                let variable = self.parse_variable_declaration(start, false, false, kind);
                locals.push(variable.clone());
                body.push(FunctionBodyItem::Variable(variable));
                continue;
            }
            if parentheses == 0
                && brackets == 0
                && starts_runtime_expression_statement(self.current())
            {
                let start = self.current().start;
                let expression_start = self.index;
                let tokens = self.collect_until_function_statement_end();
                self.collect_expression_type_edits(expression_start, self.index);
                self.consume(";");
                body.push(FunctionBodyItem::Expression {
                    tokens,
                    span: SourceSpan::new(&self.id, start, self.previous().end),
                });
                continue;
            }
            if self.peek("as") || self.peek("satisfies") {
                body.push(FunctionBodyItem::Opaque(self.current().span(&self.id)));
                let end = find_balanced_delimiter(
                    &self.tokens,
                    self.index + 1,
                    self.tokens.len() - 1,
                    &[";", ",", ")", "]", "}", "&&", "||"],
                );
                self.index = self.erase_assertion(self.index, end);
                continue;
            }
            if self.consume(";") {
                continue;
            }
            let token = self.current().clone();
            body.push(FunctionBodyItem::Opaque(token.span(&self.id)));
            match token.text.as_str() {
                "(" => parentheses += 1,
                ")" if parentheses > 0 => parentheses -= 1,
                "[" => brackets += 1,
                "]" if brackets > 0 => brackets -= 1,
                _ => {}
            }
            self.bump();
        }
        if depth != 0 {
            self.error_at(
                SourceSpan::new(&self.id, body_start, self.source.len()),
                DiagnosticCode::ParseError,
                "unterminated function body",
            );
        }
    }

    /// Parses a preflighted direct `if` statement. The preflight guarantees
    /// explicit branch braces and a complete alternate before this method
    /// consumes input, preserving opaque fallback behavior for every other
    /// control-flow shape.
    pub(super) fn parse_direct_braced_if_statement(
        &mut self,
        returns: &mut Vec<Vec<Token>>,
        locals: &mut Vec<VariableDeclaration>,
    ) -> FunctionIfStatement {
        debug_assert!(is_direct_braced_if_statement(&self.tokens, self.index));
        let if_start = self.current().start;
        self.bump();
        let test_start = self.index + 1;
        let test_end =
            matching_closing_delimiter(&self.tokens, self.index, self.tokens.len() - 1, "(", ")")
                .expect("the direct braced-if preflight found a closing parenthesis");
        let test = self.tokens[test_start..test_end].to_vec();
        self.collect_expression_type_edits(test_start, test_end);
        self.index = test_end + 1;
        let consequent = self.parse_direct_function_block(returns, locals);
        let alternate = if self.consume("else") {
            if self.peek("{") {
                Some(FunctionElseBranch::Braced(
                    self.parse_direct_function_block(returns, locals),
                ))
            } else {
                Some(FunctionElseBranch::ElseIf(Box::new(
                    self.parse_direct_braced_if_statement(returns, locals),
                )))
            }
        } else {
            None
        };
        FunctionIfStatement {
            test,
            consequent,
            alternate,
            span: SourceSpan::new(&self.id, if_start, self.previous().end),
        }
    }

    pub(super) fn parse_direct_function_block(
        &mut self,
        returns: &mut Vec<Vec<Token>>,
        locals: &mut Vec<VariableDeclaration>,
    ) -> Vec<FunctionBodyItem> {
        self.expect("{");
        let block_start = self.previous().start;
        let mut body = Vec::new();
        self.parse_function_body(block_start, &mut body, returns, locals);
        body
    }

    pub(super) fn collect_expression_type_edits(&mut self, start: usize, end: usize) {
        self.diagnose_unsupported_opaque_syntax(start, end);
        let mut index = start;
        while index < end {
            if self.tokens[index].kind == TokenKind::Identifier
                && self
                    .tokens
                    .get(index + 1)
                    .is_some_and(|token| token.is("<"))
            {
                if let Some(close) = matching_angle_bracket(&self.tokens, index + 1, end) {
                    if self
                        .tokens
                        .get(close + 1)
                        .is_some_and(|token| token.is("("))
                    {
                        let type_arguments = self.parse_call_type_arguments(index + 2, close);
                        self.edits.push(TextEdit {
                            start: self.tokens[index + 1].start,
                            end: self.tokens[close].end,
                            replacement: String::new(),
                        });
                        self.generic_call_type_arguments
                            .insert(self.tokens[index].start, type_arguments);
                        index = close + 1;
                        continue;
                    }
                }
            }
            if self.tokens[index].is("as") || self.tokens[index].is("satisfies") {
                index = self.erase_assertion(index, end);
                continue;
            }
            if self.tokens[index].is(":") && is_typed_arrow_parameter(&self.tokens, index, end) {
                self.unsupported(
                    self.tokens[index].span(&self.id),
                    "typed arrow parameters are not in the initial BlueTS matrix",
                );
            }
            if self.tokens[index].is("!")
                && index > start
                && self
                    .tokens
                    .get(index + 1)
                    .is_some_and(|token| matches!(token.text.as_str(), ";" | "," | ")" | "]" | "."))
            {
                self.edits.push(TextEdit {
                    start: self.tokens[index].start,
                    end: self.tokens[index].end,
                    replacement: String::new(),
                });
            }
            index += 1;
        }
    }

    /// Opaque expression spans are otherwise preserved for JavaScript
    /// emission. Known TypeScript-only declarations must still be rejected
    /// there, rather than being emitted as invalid JavaScript merely because
    /// they were nested in an arrow initializer, return expression, or raw
    /// statement.
    pub(super) fn diagnose_unsupported_opaque_syntax(&mut self, start: usize, end: usize) {
        for index in start..end.saturating_sub(1) {
            let previous_is_abstract = self
                .tokens
                .get(index.saturating_sub(1))
                .is_some_and(|token| token.is("abstract"));
            let decorated_class = self
                .tokens
                .get(index.saturating_sub(2))
                .is_some_and(|token| token.is("@"));
            let next = self.tokens.get(index + 1);
            let message = if self.tokens[index].is("@") {
                Some("decorators and TSX/JSX are not in the initial BlueTS matrix")
            } else if self.tokens[index].is("abstract")
                && next.is_some_and(|token| token.is("class"))
            {
                Some("`abstract` declarations are not in the initial BlueTS matrix")
            } else if self.tokens[index].is("class")
                && !previous_is_abstract
                && !decorated_class
                && next.is_some_and(|token| {
                    token.kind == TokenKind::Identifier || token.is("extends") || token.is("{")
                })
            {
                Some("`class` is not in the initial BlueTS matrix")
            } else if self.tokens[index].is("enum")
                && next.is_some_and(|token| token.kind == TokenKind::Identifier)
            {
                Some("`enum` is not in the initial BlueTS matrix")
            } else if self.tokens[index].is("namespace")
                && next.is_some_and(|token| token.kind == TokenKind::Identifier)
            {
                Some("`namespace` is not in the initial BlueTS matrix")
            } else if self.tokens[index].is("module")
                && next.is_some_and(|token| {
                    token.kind == TokenKind::Identifier || token.kind == TokenKind::String
                })
            {
                Some("`module` is not in the initial BlueTS matrix")
            } else if is_generic_arrow_function(&self.tokens, index, end) {
                Some("generic arrow functions are not in the initial BlueTS matrix")
            } else {
                None
            };
            if let Some(message) = message {
                self.unsupported(self.tokens[index].span(&self.id), message);
            }
        }
    }

    /// ECMAScript requires parentheses when `??` appears with `&&` or `||`
    /// in the same logical expression. The bounded parser otherwise retains
    /// runtime expressions as token spans, so enforce this early error before
    /// unsupported raw statements can be copied into emitted JavaScript.
    pub(super) fn diagnose_unparenthesized_nullish_logical_mixing(&mut self) {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum LogicalFamily {
            Nullish,
            AndOr,
        }

        let mut scopes = vec![None::<LogicalFamily>];
        for index in 0..self.tokens.len() {
            let token = self.tokens[index].clone();
            match token.text.as_str() {
                "(" | "[" | "{" => scopes.push(None),
                ")" | "]" | "}" if scopes.len() > 1 => {
                    scopes.pop();
                }
                "," | ";" | "?" | ":" => {
                    *scopes
                        .last_mut()
                        .expect("the global expression scope remains") = None;
                }
                "??" => {
                    let scope = scopes
                        .last_mut()
                        .expect("the global expression scope remains");
                    if *scope == Some(LogicalFamily::AndOr) {
                        self.error_at(
                            token.span(&self.id),
                            DiagnosticCode::ParseError,
                            "parentheses are required when mixing `??` with `&&` or `||`",
                        );
                        *scope = None;
                    } else {
                        *scope = Some(LogicalFamily::Nullish);
                    }
                }
                "&&" | "||" => {
                    let scope = scopes
                        .last_mut()
                        .expect("the global expression scope remains");
                    if *scope == Some(LogicalFamily::Nullish) {
                        self.error_at(
                            token.span(&self.id),
                            DiagnosticCode::ParseError,
                            "parentheses are required when mixing `??` with `&&` or `||`",
                        );
                        *scope = None;
                    } else {
                        *scope = Some(LogicalFamily::AndOr);
                    }
                }
                _ => {}
            }
        }
    }

    /// `ExponentiationExpression` accepts an update expression as its left
    /// operand, not an unparenthesized unary expression. The bounded parser
    /// otherwise retains runtime token spans, so keep this ECMAScript early
    /// error from reaching emitted JavaScript unchanged.
    pub(super) fn diagnose_unparenthesized_unary_exponentiation(&mut self) {
        for exponent in 0..self.tokens.len() {
            if !self.tokens[exponent].is("**") {
                continue;
            }
            let Some(base_start) = exponentiation_base_start(&self.tokens, exponent) else {
                continue;
            };
            let Some(operator) = base_start.checked_sub(1) else {
                continue;
            };
            if is_unparenthesized_unary_exponent_base(&self.tokens, operator) {
                self.error_at(
                    self.tokens[exponent].span(&self.id),
                    DiagnosticCode::ParseError,
                    "a unary expression cannot be the unparenthesized base of exponentiation",
                );
            }
        }
    }

    pub(super) fn parse_call_type_arguments(&mut self, start: usize, end: usize) -> Vec<Type> {
        if start == end {
            return Vec::new();
        }
        let eof = self.tokens[end - 1].end;
        let mut tokens = self.tokens[start..end].to_vec();
        tokens.push(Token {
            kind: TokenKind::Eof,
            text: String::new(),
            start: eof,
            end: eof,
        });
        let mut parser = Parser::new(self.id.clone(), String::new(), tokens, self.max_type_depth);
        let mut values = Vec::new();
        while !parser.at_eof() {
            let before = parser.index;
            values.push(parser.parse_type_until(&[","]));
            if parser.index == before {
                parser.error_here(DiagnosticCode::ParseError, "expected a type argument");
                break;
            }
            if !parser.consume(",") {
                break;
            }
        }
        if !parser.at_eof() {
            parser.error_here(
                DiagnosticCode::ParseError,
                "expected a comma between type arguments",
            );
        }
        self.diagnostics.append(&mut parser.diagnostics);
        values
    }

    pub(super) fn erase_assertion(&mut self, index: usize, end: usize) -> usize {
        let edit_start = self.tokens[index].start;
        let type_start = index + 1;
        let end_index = find_balanced_delimiter(
            &self.tokens,
            type_start,
            end,
            &[";", ",", ")", "]", "}", "&&", "||"],
        );
        self.edits.push(TextEdit {
            start: edit_start,
            end: self.tokens[end_index].start,
            replacement: String::new(),
        });
        end_index
    }

    pub(super) fn parse_raw(&mut self, start: usize) {
        let raw_start = self.index;
        let end_index =
            find_balanced_delimiter(&self.tokens, self.index, self.tokens.len() - 1, &[";"]);
        self.collect_expression_type_edits(raw_start, end_index);
        let assertions = self.tokens[raw_start..end_index]
            .iter()
            .filter(|token| token.is("as") || token.is("satisfies"))
            .map(|token| token.span(&self.id))
            .collect::<Vec<_>>();
        for span in assertions {
            self.unsupported(
                span,
                "TypeScript assertions outside a supported declaration are not in the initial BlueTS matrix",
            );
        }
        let end = self.tokens[end_index].end;
        self.index = if self.tokens[end_index].is(";") {
            end_index + 1
        } else {
            end_index
        };
        self.declarations.push(Declaration::Raw(RawDeclaration {
            tokens: self.tokens[raw_start..end_index].to_vec(),
            span: SourceSpan::new(&self.id, start, end),
        }));
    }
}
