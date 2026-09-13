// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parser for the explicitly supported BlueTS language subset.

use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::syntax::{
    lex, lex_with_limits, string_contents, Token, TokenKind, MAX_SOURCE_BYTES, MAX_TOKENS,
};

/// Parser work bounds. Hosts may lower these for a constrained compile slot;
/// the values are included in [`crate::CompilerLimits`] fingerprints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParserLimits {
    pub max_source_bytes: usize,
    pub max_tokens: usize,
    pub max_type_depth: usize,
}

impl Default for ParserLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: MAX_SOURCE_BYTES,
            max_tokens: MAX_TOKENS,
            max_type_depth: 128,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub id: String,
    pub source: String,
    pub declarations: Vec<Declaration>,
    pub(crate) edits: Vec<TextEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Declaration {
    Import(ImportDeclaration),
    TypeExport(TypeExportDeclaration),
    TypeAlias(TypeAliasDeclaration),
    Interface(InterfaceDeclaration),
    Variable(VariableDeclaration),
    Function(FunctionDeclaration),
    Raw(SourceSpan),
}

impl Declaration {
    pub fn span(&self) -> &SourceSpan {
        match self {
            Self::Import(declaration) => &declaration.span,
            Self::TypeExport(declaration) => &declaration.span,
            Self::TypeAlias(declaration) => &declaration.span,
            Self::Interface(declaration) => &declaration.span,
            Self::Variable(declaration) => &declaration.span,
            Self::Function(declaration) => &declaration.span,
            Self::Raw(span) => span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDeclaration {
    pub type_only: bool,
    pub specifier: String,
    pub specifier_span: SourceSpan,
    pub bindings: Vec<ImportBinding>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBinding {
    pub imported: String,
    pub local: String,
    pub type_only: bool,
}

/// A static-only `export type` declaration.  It has no JavaScript runtime
/// representation but can still extend the closed type-module graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeExportDeclaration {
    pub bindings: Vec<String>,
    pub specifier: Option<String>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeAliasDeclaration {
    pub name: String,
    pub type_parameters: Vec<String>,
    pub value: Type,
    pub exported: bool,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceDeclaration {
    pub name: String,
    pub type_parameters: Vec<String>,
    pub fields: Vec<TypeField>,
    pub exported: bool,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeField {
    pub name: String,
    pub optional: bool,
    pub value: Type,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableDeclaration {
    pub name: String,
    pub annotation: Option<Type>,
    pub initializer: Vec<Token>,
    pub exported: bool,
    pub declared: bool,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDeclaration {
    pub name: String,
    pub type_parameters: Vec<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<Type>,
    pub returns: Vec<Vec<Token>>,
    pub locals: Vec<VariableDeclaration>,
    pub exported: bool,
    pub declared: bool,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    pub name: String,
    pub optional: bool,
    pub annotation: Option<Type>,
    pub span: SourceSpan,
}

/// The supported, reifiable portion of the TypeScript type grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Any,
    Unknown,
    Never,
    Void,
    Null,
    Undefined,
    Boolean,
    Number,
    String,
    Literal(String),
    Named { name: String, arguments: Vec<Type> },
    Array(Box<Type>),
    Tuple(Vec<Type>),
    Record(Vec<TypeField>),
    Union(Vec<Type>),
    Intersection(Vec<Type>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextEdit {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

/// Parses one source record.  The caller owns resolution and can decide which
/// source records are authorized to participate in the project.
pub fn parse_module(
    id: impl Into<String>,
    source: impl Into<String>,
) -> Result<Module, Vec<Diagnostic>> {
    parse_module_with_limits(id, source, ParserLimits::default())
}

pub(crate) fn parse_module_with_limits(
    id: impl Into<String>,
    source: impl Into<String>,
    limits: ParserLimits,
) -> Result<Module, Vec<Diagnostic>> {
    let id = id.into();
    let source = source.into();
    let tokens = if limits == ParserLimits::default() {
        lex(&id, &source)?
    } else {
        lex_with_limits(&id, &source, limits.max_source_bytes, limits.max_tokens)?
    };
    Parser::new(id, source, tokens, limits.max_type_depth).parse_module()
}

struct Parser {
    id: String,
    source: String,
    tokens: Vec<Token>,
    index: usize,
    declarations: Vec<Declaration>,
    edits: Vec<TextEdit>,
    diagnostics: Vec<Diagnostic>,
    max_type_depth: usize,
    type_depth: usize,
}

impl Parser {
    fn new(id: String, source: String, tokens: Vec<Token>, max_type_depth: usize) -> Self {
        Self {
            id,
            source,
            tokens,
            index: 0,
            declarations: Vec::new(),
            edits: Vec::new(),
            diagnostics: Vec::new(),
            max_type_depth,
            type_depth: 0,
        }
    }

    fn parse_module(mut self) -> Result<Module, Vec<Diagnostic>> {
        if self.id.ends_with(".tsx") {
            self.unsupported(
                SourceSpan::new(&self.id, 0, self.source.len()),
                "TSX/JSX is not in the initial BlueTS matrix",
            );
            return Err(self.diagnostics);
        }
        while !self.at_eof() {
            if self.consume(";") {
                continue;
            }
            let start = self.current().start;
            let exported = self.consume("export");
            if exported && self.consume("default") {
                self.unsupported(
                    self.previous().span(&self.id),
                    "default exports are not in the initial BlueTS matrix",
                );
                self.skip_statement();
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
            } else {
                let declared = self.consume("declare");
                let async_start = self.consume("async");
                if self.consume("function") {
                    self.parse_function(start, exported, declared, async_start);
                } else if self.peek("const") || self.peek("let") || self.peek("var") {
                    self.bump();
                    self.parse_variable(start, exported, declared);
                } else if self.peek_any(&[
                    "enum",
                    "namespace",
                    "module",
                    "class",
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
            })
        } else {
            Err(self.diagnostics)
        }
    }

    fn parse_import(&mut self, start: usize) {
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

    fn parse_type_export(&mut self, start: usize) {
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

    fn parse_type_alias(&mut self, start: usize, exported: bool) {
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

    fn parse_interface(&mut self, start: usize, exported: bool) {
        let name = self.require_identifier("expected an interface name");
        let type_parameters = self.parse_type_parameters();
        if self.consume("extends") {
            self.unsupported(
                self.previous().span(&self.id),
                "interface extends is not in the initial BlueTS matrix",
            );
            self.skip_until(&["{"]);
        }
        self.expect("{");
        let mut fields = Vec::new();
        while !self.at_eof() && !self.consume("}") {
            if self.consume("readonly") {
                // `readonly` is static-only and represented by the field
                // itself in this first checker.
            }
            let field_start = self.current().start;
            let name = self.require_identifier("expected an interface field name");
            let optional = self.consume("?");
            self.expect(":");
            let value = self.parse_type_until(&[";", ",", "}"]);
            let end = self.previous().end;
            fields.push(TypeField {
                name,
                optional,
                value,
                span: SourceSpan::new(&self.id, field_start, end),
            });
            self.consume(";");
            self.consume(",");
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
                fields,
                exported,
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    fn parse_variable(&mut self, start: usize, exported: bool, declared: bool) {
        let declaration = self.parse_variable_declaration(start, exported, declared);
        self.declarations.push(Declaration::Variable(declaration));
    }

    fn parse_variable_declaration(
        &mut self,
        start: usize,
        exported: bool,
        declared: bool,
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
            annotation,
            initializer,
            exported,
            declared,
            span: SourceSpan::new(&self.id, start, end),
        }
    }

    fn parse_function(&mut self, start: usize, exported: bool, declared: bool, _async_start: bool) {
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
            self.consume("...");
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
            if self.consume("=") {
                // A default initializer makes a parameter omittable at a call
                // site just like `?`; the emitter preserves the initializer.
                optional = true;
                self.skip_until(&[",", ")"]);
            }
            let parameter_end = self.previous().end;
            parameters.push(Parameter {
                name: parameter_name,
                optional,
                annotation,
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

        let mut returns = Vec::new();
        let mut locals = Vec::new();
        if self.consume("{") {
            let body_start = self.previous().start;
            self.parse_function_body(body_start, &mut returns, &mut locals);
        } else if !declared {
            self.error_here(DiagnosticCode::ParseError, "expected a function body");
        } else {
            self.consume(";");
        }
        let end = self.previous().end;
        if declared {
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
                returns,
                locals,
                exported,
                declared,
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    fn parse_function_body(
        &mut self,
        body_start: usize,
        returns: &mut Vec<Vec<Token>>,
        locals: &mut Vec<VariableDeclaration>,
    ) {
        let mut depth = 1usize;
        while !self.at_eof() && depth > 0 {
            if self.consume("{") {
                depth += 1;
                continue;
            }
            if self.consume("}") {
                depth -= 1;
                continue;
            }
            if self.consume("return") {
                let expression_start = self.index;
                let expression = self.collect_until_statement_end();
                self.collect_expression_type_edits(expression_start, self.index);
                returns.push(expression);
                self.consume(";");
                continue;
            }
            if self.peek("const") || self.peek("let") || self.peek("var") {
                let start = self.current().start;
                self.bump();
                locals.push(self.parse_variable_declaration(start, false, false));
                continue;
            }
            if self.peek("as") || self.peek("satisfies") {
                let end = find_balanced_delimiter(
                    &self.tokens,
                    self.index + 1,
                    self.tokens.len() - 1,
                    &[";", ",", ")", "]", "}", "&&", "||"],
                );
                self.index = self.erase_assertion(self.index, end);
                continue;
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

    fn collect_expression_type_edits(&mut self, start: usize, end: usize) {
        let mut index = start;
        while index < end {
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

    fn erase_assertion(&mut self, index: usize, end: usize) -> usize {
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

    fn parse_raw(&mut self, start: usize) {
        let raw_start = self.index;
        let end_index =
            find_balanced_delimiter(&self.tokens, self.index, self.tokens.len() - 1, &[";"]);
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
        self.declarations
            .push(Declaration::Raw(SourceSpan::new(&self.id, start, end)));
    }

    fn parse_type_parameters(&mut self) -> Vec<String> {
        if !self.consume("<") {
            return Vec::new();
        }
        let mut parameters = Vec::new();
        while !self.at_eof() && !self.consume(">") {
            let name = self.require_identifier("expected a type parameter name");
            parameters.push(name);
            if self.consume("extends") {
                self.parse_type_until(&["=", ",", ">"]);
            }
            if self.consume("=") {
                self.parse_type_until(&[",", ">"]);
            }
            if !self.consume(",") {
                self.expect(">");
                break;
            }
        }
        parameters
    }

    fn parse_type_until(&mut self, stop: &[&str]) -> Type {
        if self.type_depth >= self.max_type_depth {
            self.error_here(
                DiagnosticCode::ResourceLimit,
                format!(
                    "type expression exceeds the {} nesting limit",
                    self.max_type_depth
                ),
            );
            self.skip_type_until(stop);
            return Type::Unknown;
        }
        self.type_depth += 1;
        let type_start = self.index;
        let value = self.parse_union(stop);
        if self.index == type_start && !self.at_eof() && !stop.iter().any(|stop| self.peek(stop)) {
            self.error_here(DiagnosticCode::ParseError, "expected a type");
            self.bump();
        }
        self.type_depth -= 1;
        value
    }

    fn skip_type_until(&mut self, stop: &[&str]) {
        let mut nesting = 0usize;
        while !self.at_eof() {
            match self.current().text.as_str() {
                "{" | "[" | "<" => nesting += 1,
                "}" | "]" | ">" if nesting > 0 => nesting -= 1,
                _ if nesting == 0 && stop.iter().any(|stop| self.peek(stop)) => break,
                _ => {}
            }
            self.bump();
        }
    }

    fn parse_union(&mut self, stop: &[&str]) -> Type {
        let mut values = vec![self.parse_intersection(stop)];
        while self.consume("|") {
            values.push(self.parse_intersection(stop));
        }
        if values.len() == 1 {
            values.pop().expect("one parsed type")
        } else {
            Type::Union(values)
        }
    }

    fn parse_intersection(&mut self, stop: &[&str]) -> Type {
        let mut values = vec![self.parse_type_primary(stop)];
        while self.consume("&") {
            values.push(self.parse_type_primary(stop));
        }
        if values.len() == 1 {
            values.pop().expect("one parsed type")
        } else {
            Type::Intersection(values)
        }
    }

    fn parse_type_primary(&mut self, _stop: &[&str]) -> Type {
        let mut value = if self.consume("readonly") {
            self.parse_type_primary(&[])
        } else if self.consume("[") {
            let mut values = Vec::new();
            while !self.at_eof() && !self.consume("]") {
                values.push(self.parse_type_until(&[",", "]"]));
                if !self.consume(",") {
                    self.expect("]");
                    break;
                }
            }
            Type::Tuple(values)
        } else if self.consume("{") {
            let mut fields = Vec::new();
            while !self.at_eof() && !self.consume("}") {
                let start = self.current().start;
                self.consume("readonly");
                let name = self.require_identifier("expected a record field name");
                let optional = self.consume("?");
                self.expect(":");
                let value = self.parse_type_until(&[";", ",", "}"]);
                let end = self.previous().end;
                fields.push(TypeField {
                    name,
                    optional,
                    value,
                    span: SourceSpan::new(&self.id, start, end),
                });
                self.consume(";");
                self.consume(",");
            }
            Type::Record(fields)
        } else if self.current().kind == TokenKind::String
            || self.current().kind == TokenKind::Number
            || self.peek("true")
            || self.peek("false")
        {
            let value = Type::Literal(self.current().text.clone());
            self.bump();
            value
        } else if self.consume("null") {
            Type::Null
        } else if self.consume("undefined") {
            Type::Undefined
        } else if let Some(name) = self.consume_identifier_or_keyword() {
            let mut arguments = Vec::new();
            if self.consume("<") {
                while !self.at_eof() && !self.consume(">") {
                    arguments.push(self.parse_type_until(&[",", ">"]));
                    if !self.consume(",") {
                        self.expect(">");
                        break;
                    }
                }
            }
            match name.as_str() {
                "any" => Type::Any,
                "unknown" => Type::Unknown,
                "never" => Type::Never,
                "void" => Type::Void,
                "boolean" => Type::Boolean,
                "number" => Type::Number,
                "string" => Type::String,
                _ => Type::Named { name, arguments },
            }
        } else {
            self.error_here(DiagnosticCode::ParseError, "expected a type");
            Type::Unknown
        };

        while self.consume("[") {
            self.expect("]");
            value = Type::Array(Box::new(value));
        }
        value
    }

    fn collect_until_statement_end(&mut self) -> Vec<Token> {
        let end = find_balanced_delimiter(&self.tokens, self.index, self.tokens.len() - 1, &[";"]);
        let values = self.tokens[self.index..end].to_vec();
        self.index = end;
        values
    }

    fn skip_statement(&mut self) {
        let end = find_balanced_delimiter(&self.tokens, self.index, self.tokens.len() - 1, &[";"]);
        self.index = if self.tokens[end].is(";") {
            end + 1
        } else {
            end
        };
    }

    fn skip_until(&mut self, stops: &[&str]) {
        while !self.at_eof() && !stops.iter().any(|stop| self.peek(stop)) {
            self.bump();
        }
    }

    fn expect(&mut self, text: &str) {
        if !self.consume(text) {
            self.error_here(DiagnosticCode::ParseError, format!("expected `{text}`"));
        }
    }

    fn require_identifier(&mut self, message: impl Into<String>) -> String {
        self.consume_identifier().unwrap_or_else(|| {
            self.error_here(DiagnosticCode::ParseError, message);
            "<error>".to_string()
        })
    }

    fn consume_identifier(&mut self) -> Option<String> {
        if self.current().kind == TokenKind::Identifier {
            let value = self.current().text.clone();
            self.bump();
            Some(value)
        } else {
            None
        }
    }

    fn consume_identifier_or_keyword(&mut self) -> Option<String> {
        if matches!(
            self.current().kind,
            TokenKind::Identifier | TokenKind::Keyword
        ) {
            let value = self.current().text.clone();
            self.bump();
            Some(value)
        } else {
            None
        }
    }

    fn consume(&mut self, text: &str) -> bool {
        if self.peek(text) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn peek(&self, text: &str) -> bool {
        self.current().is(text)
    }

    fn peek_any(&self, texts: &[&str]) -> bool {
        texts.iter().any(|text| self.peek(text))
    }

    fn current(&self) -> &Token {
        &self.tokens[self.index]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.index.saturating_sub(1)]
    }

    fn bump(&mut self) {
        if !self.at_eof() {
            self.index += 1;
        }
    }

    fn at_eof(&self) -> bool {
        self.current().kind == TokenKind::Eof
    }

    fn error_here(&mut self, code: DiagnosticCode, message: impl Into<String>) {
        let span = self.current().span(&self.id);
        self.error_at(span, code, message);
    }

    fn error_at(&mut self, span: SourceSpan, code: DiagnosticCode, message: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::error(code, span, message));
    }

    fn unsupported(&mut self, span: SourceSpan, message: impl Into<String>) {
        self.error_at(span, DiagnosticCode::UnsupportedSyntax, message);
    }
}

fn is_typed_arrow_parameter(tokens: &[Token], colon: usize, end: usize) -> bool {
    let mut parentheses = 0usize;
    let mut index = colon + 1;
    while index < end {
        match tokens[index].text.as_str() {
            "(" | "[" | "{" => parentheses += 1,
            ")" | "]" | "}" if parentheses == 0 => {
                return tokens.get(index + 1).is_some_and(|token| token.is("=>"));
            }
            ")" | "]" | "}" => parentheses -= 1,
            ";" => return false,
            _ => {}
        }
        index += 1;
    }
    false
}

fn find_balanced_delimiter(
    tokens: &[Token],
    start: usize,
    limit: usize,
    delimiters: &[&str],
) -> usize {
    let mut index = start;
    let mut parentheses = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    while index < limit {
        let token = &tokens[index];
        match token.text.as_str() {
            "(" => parentheses += 1,
            ")" if parentheses > 0 => parentheses -= 1,
            "[" => brackets += 1,
            "]" if brackets > 0 => brackets -= 1,
            "{" => braces += 1,
            "}" if braces > 0 => braces -= 1,
            _ if parentheses == 0
                && brackets == 0
                && braces == 0
                && delimiters.iter().any(|delimiter| token.is(delimiter)) =>
            {
                return index
            }
            _ => {}
        }
        index += 1;
    }
    limit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typed_exports_and_marks_only_type_syntax_for_erasure() {
        let module = parse_module(
            "memory:///app.ts",
            "export interface User { name: string; age?: number }\nexport const user: User = { name: 'Ada' };",
        )
        .unwrap();
        assert!(matches!(module.declarations[0], Declaration::Interface(_)));
        assert!(matches!(module.declarations[1], Declaration::Variable(_)));
        assert_eq!(module.edits.len(), 2);
    }

    #[test]
    fn rejects_runtime_enums_explicitly() {
        let diagnostics = parse_module("memory:///app.ts", "enum Colour { Red }").unwrap_err();
        assert_eq!(diagnostics[0].code, DiagnosticCode::UnsupportedSyntax);
    }

    #[test]
    fn rejects_tsx_modules_even_when_they_contain_no_tag_tokens() {
        let diagnostics =
            parse_module("memory:///view.tsx", "const label: string = 'BlueIce';").unwrap_err();
        assert_eq!(diagnostics[0].code, DiagnosticCode::UnsupportedSyntax);
        assert!(diagnostics[0].message.contains("TSX/JSX"));
    }

    #[test]
    fn rejects_legacy_commonjs_module_assignment_forms_explicitly() {
        for source in [
            "import Legacy = require('./legacy.ts');",
            "export = Legacy;",
        ] {
            let diagnostics = parse_module("memory:///app.ts", source).unwrap_err();
            assert_eq!(diagnostics[0].code, DiagnosticCode::UnsupportedSyntax);
        }
    }

    #[test]
    fn bounds_deeply_nested_type_expressions() {
        let diagnostics = parse_module_with_limits(
            "memory:///deep.ts",
            "type Deep = { value: { value: { value: string } } };",
            ParserLimits {
                max_type_depth: 2,
                ..ParserLimits::default()
            },
        )
        .unwrap_err();
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::ResourceLimit));
    }
}
