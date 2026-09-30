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
            nested_functions: BTreeMap::new(),
            diagnostics: Vec::new(),
            max_type_depth,
            type_depth: 0,
            parameter_property_mode: false,
            parameter_properties: Vec::new(),
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
                } else if self.consume("class") {
                    if declared || async_start {
                        self.unsupported(
                            self.previous().span(&self.id),
                            "declared and async classes are not in the first class form",
                        );
                        self.skip_statement();
                    } else {
                        self.parse_class(start, exported);
                    }
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
            self.audit_erased_function_annotations();
        }
        if self.diagnostics.is_empty() {
            self.edits.sort_by_key(|edit| (edit.start, edit.end));
            Ok(Module {
                id: self.id,
                source: self.source,
                declarations: self.declarations,
                edits: self.edits,
                generic_call_type_arguments: self.generic_call_type_arguments,
                nested_functions: self.nested_functions,
            })
        } else {
            Err(self.diagnostics)
        }
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

#[path = "declarations/class.rs"]
mod class;
#[path = "declarations/erasure_audit.rs"]
mod erasure_audit;
#[path = "declarations/function_body.rs"]
mod function_body;
#[path = "declarations/imports_exports.rs"]
mod imports_exports;
#[path = "declarations/nested_functions.rs"]
mod nested_functions;
#[path = "declarations/patterns.rs"]
mod patterns;
#[path = "declarations/source_edits.rs"]
mod source_edits;
#[path = "declarations/typed_declarations.rs"]
mod typed_declarations;
