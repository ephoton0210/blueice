// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parser for the explicitly supported BlueTS language subset.

use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::syntax::{
    lex, lex_with_limits, string_contents, Token, TokenKind, MAX_SOURCE_BYTES, MAX_TOKENS,
};
use std::collections::BTreeMap;

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
    /// Explicit type arguments on direct identifier calls, keyed by the
    /// callee token's source-byte offset. They are static-only and erased from
    /// JavaScript, but retained for the checker to validate a local call.
    pub(crate) generic_call_type_arguments: BTreeMap<usize, Vec<Type>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Declaration {
    Import(ImportDeclaration),
    TypeExport(TypeExportDeclaration),
    DefaultExport(DefaultExportDeclaration),
    ValueExport(ValueExportDeclaration),
    TypeAlias(TypeAliasDeclaration),
    Interface(InterfaceDeclaration),
    Variable(VariableDeclaration),
    Function(FunctionDeclaration),
    Raw(RawDeclaration),
}

impl Declaration {
    pub fn span(&self) -> &SourceSpan {
        match self {
            Self::Import(declaration) => &declaration.span,
            Self::TypeExport(declaration) => &declaration.span,
            Self::DefaultExport(declaration) => &declaration.span,
            Self::ValueExport(declaration) => &declaration.span,
            Self::TypeAlias(declaration) => &declaration.span,
            Self::Interface(declaration) => &declaration.span,
            Self::Variable(declaration) => &declaration.span,
            Self::Function(declaration) => &declaration.span,
            Self::Raw(declaration) => &declaration.span,
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

/// A value export in the narrowly supported `export default localName` form.
/// The referenced local remains the runtime declaration, while this node
/// records the public ESM binding for checking and declaration emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultExportDeclaration {
    pub name: String,
    pub span: SourceSpan,
}

/// A value export in the narrowly supported local
/// `export { localName as publicName }` form. It keeps its JavaScript syntax
/// and records the public bindings required by static checking and `.d.ts`
/// emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueExportDeclaration {
    pub bindings: Vec<ValueExportBinding>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueExportBinding {
    pub local: String,
    pub exported: String,
    pub span: SourceSpan,
}

/// A JavaScript runtime statement that the bounded TypeScript parser does not
/// otherwise classify. Its already-tokenized source is retained so an
/// authorized runtime bridge can lower it structurally without reparsing
/// BlueTSC-emitted JavaScript text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDeclaration {
    pub tokens: Vec<Token>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeAliasDeclaration {
    pub name: String,
    pub type_parameters: Vec<TypeParameter>,
    pub value: Type,
    pub exported: bool,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceDeclaration {
    pub name: String,
    pub type_parameters: Vec<TypeParameter>,
    /// Named parent interfaces inherited by this static-only declaration.
    /// Their fields participate in checker and contract expansion, while the
    /// entire interface remains erased from JavaScript.
    pub heritage: Vec<Type>,
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
    pub kind: VariableKind,
    pub annotation: Option<Type>,
    pub initializer: Vec<Token>,
    pub exported: bool,
    pub declared: bool,
    pub span: SourceSpan,
}

/// The runtime binding form retained for declaration output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableKind {
    Const,
    Let,
    Var,
}

impl VariableKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Const => "const",
            Self::Let => "let",
            Self::Var => "var",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDeclaration {
    pub name: String,
    pub type_parameters: Vec<TypeParameter>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<Type>,
    /// Ordered body items retained for direct runtime lowering. The existing
    /// `returns` and `locals` collections remain the checker-oriented views.
    pub body: Vec<FunctionBodyItem>,
    pub returns: Vec<Vec<Token>>,
    pub locals: Vec<VariableDeclaration>,
    pub exported: bool,
    /// A named `export default function` declaration. Its runtime ESM syntax
    /// stays in the emitted JavaScript, while its declaration output uses the
    /// corresponding default-export form rather than `export declare`.
    pub default_export: bool,
    pub declared: bool,
    /// A signature-only function declaration preceding an implementation.
    /// It is static-only and is erased from JavaScript, but remains a direct
    /// call candidate and public declaration signature.
    pub overload: bool,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    pub name: String,
    pub rest: bool,
    pub optional: bool,
    pub annotation: Option<Type>,
    /// Original runtime tokens for an initializer on a default parameter.
    /// They are retained so the direct BlueTS-to-BlueJS bridge can construct a
    /// structured BlueJS parameter expression without reparsing emitted text.
    pub default: Option<Vec<Token>>,
    pub span: SourceSpan,
}

/// A function-body item recognized by the bounded TypeScript parser.
///
/// Direct runtime lowering only accepts the structured variants. `Opaque`
/// records a source token that remains meaningful to the standalone parser
/// but has not been assigned direct BlueJS semantics, so a bridge cannot
/// accidentally erase and execute around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionBodyItem {
    Variable(VariableDeclaration),
    Return {
        tokens: Vec<Token>,
        span: SourceSpan,
    },
    Opaque(SourceSpan),
}

/// A generic parameter's static-only declaration. Constraints and defaults
/// participate in BlueTS type checking and declaration output, but are erased
/// from emitted JavaScript together with the parameter list itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParameter {
    pub name: String,
    pub constraint: Option<Type>,
    pub default: Option<Type>,
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
    generic_call_type_arguments: BTreeMap<usize, Vec<Type>>,
    diagnostics: Vec<Diagnostic>,
    max_type_depth: usize,
    type_depth: usize,
}

impl Parser {
    fn new(id: String, source: String, tokens: Vec<Token>, max_type_depth: usize) -> Self {
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

    fn parse_module(mut self) -> Result<Module, Vec<Diagnostic>> {
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

    fn parse_default_export(&mut self, start: usize) {
        let name = self.require_identifier("expected a default export name");
        self.consume(";");
        let end = self.previous().end;
        self.declarations
            .push(Declaration::DefaultExport(DefaultExportDeclaration {
                name,
                span: SourceSpan::new(&self.id, start, end),
            }));
    }

    fn parse_value_export(&mut self, start: usize) {
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

    fn parse_variable(&mut self, start: usize, exported: bool, declared: bool, kind: VariableKind) {
        let declaration = self.parse_variable_declaration(start, exported, declared, kind);
        self.declarations.push(Declaration::Variable(declaration));
    }

    fn parse_variable_declaration(
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

    fn parse_function(
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

    fn parse_function_body(
        &mut self,
        body_start: usize,
        body: &mut Vec<FunctionBodyItem>,
        returns: &mut Vec<Vec<Token>>,
        locals: &mut Vec<VariableDeclaration>,
    ) {
        let mut depth = 1usize;
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
            body.push(FunctionBodyItem::Opaque(self.current().span(&self.id)));
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
    fn diagnose_unsupported_opaque_syntax(&mut self, start: usize, end: usize) {
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
    fn diagnose_unparenthesized_nullish_logical_mixing(&mut self) {
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
    fn diagnose_unparenthesized_unary_exponentiation(&mut self) {
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

    fn parse_call_type_arguments(&mut self, start: usize, end: usize) -> Vec<Type> {
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

    fn parse_type_parameters(&mut self) -> Vec<TypeParameter> {
        if !self.consume("<") {
            return Vec::new();
        }
        let mut parameters = Vec::new();
        while !self.at_eof() && !self.consume(">") {
            let start = self.current().start;
            let name = self.require_identifier("expected a type parameter name");
            let constraint = self
                .consume("extends")
                .then(|| self.parse_type_until(&["=", ",", ">"]));
            let default = self
                .consume("=")
                .then(|| self.parse_type_until(&[",", ">"]));
            let end = self.previous().end;
            parameters.push(TypeParameter {
                name,
                constraint,
                default,
                span: SourceSpan::new(&self.id, start, end),
            });
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
        } else if let Some(mut name) = self.consume_identifier_or_keyword() {
            while self.consume(".") {
                let member = self.require_identifier("expected a qualified type name");
                name.push('.');
                name.push_str(&member);
            }
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

/// Splits a lexically valid JavaScript shift-token run into individual generic
/// closers for the TypeScript grammar. Every replacement token keeps its
/// original source byte, so diagnostics and erasure edits remain source-based.
fn split_generic_closers(tokens: Vec<Token>) -> Vec<Token> {
    let mut split = Vec::with_capacity(tokens.len());
    for token in tokens {
        if token.kind == TokenKind::Punct
            && token.text.len() > 1
            && token.text.bytes().all(|byte| byte == b'>')
        {
            for start in token.start..token.end {
                split.push(Token {
                    kind: TokenKind::Punct,
                    text: ">".to_string(),
                    start,
                    end: start + 1,
                });
            }
        } else {
            split.push(token);
        }
    }
    split
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

fn exponentiation_base_start(tokens: &[Token], exponent: usize) -> Option<usize> {
    let mut start = exponent.checked_sub(1)?;
    loop {
        match tokens.get(start)?.text.as_str() {
            ")" => {
                let open = matching_opening_delimiter(tokens, start, "(", ")")?;
                if open > 0 && token_ends_runtime_primary(&tokens[open - 1]) {
                    start = open - 1;
                    continue;
                }
                return Some(open);
            }
            "]" => {
                let open = matching_opening_delimiter(tokens, start, "[", "]")?;
                if open > 0 && token_ends_runtime_primary(&tokens[open - 1]) {
                    start = open - 1;
                    continue;
                }
                return Some(open);
            }
            _ if start >= 2
                && tokens[start - 1].is(".")
                && token_ends_runtime_primary(&tokens[start - 2]) =>
            {
                start -= 2;
            }
            _ => return Some(start),
        }
    }
}

fn matching_opening_delimiter(
    tokens: &[Token],
    close: usize,
    opening: &str,
    closing: &str,
) -> Option<usize> {
    debug_assert!(tokens.get(close).is_some_and(|token| token.is(closing)));
    let mut depth = 0usize;
    for index in (0..=close).rev() {
        let token = &tokens[index];
        if token.is(closing) {
            depth += 1;
        } else if token.is(opening) {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn token_ends_runtime_primary(token: &Token) -> bool {
    matches!(
        token.kind,
        TokenKind::Identifier | TokenKind::Number | TokenKind::String | TokenKind::Template
    ) || matches!(
        token.text.as_str(),
        "true" | "false" | "null" | "undefined" | ")" | "]"
    )
}

fn is_unparenthesized_unary_exponent_base(tokens: &[Token], operator: usize) -> bool {
    let token = &tokens[operator];
    match token.text.as_str() {
        "!" | "~" | "typeof" | "void" | "delete" => true,
        "+" | "-" => {
            operator == 0
                || tokens.get(operator - 1).is_some_and(|previous| {
                    matches!(
                        previous.text.as_str(),
                        "(" | "["
                            | "{"
                            | "?"
                            | ":"
                            | ","
                            | ";"
                            | "="
                            | "+"
                            | "-"
                            | "*"
                            | "/"
                            | "%"
                            | "**"
                            | "<<"
                            | ">>"
                            | ">>>"
                            | "&"
                            | "^"
                            | "|"
                            | "&&"
                            | "||"
                            | "??"
                            | "return"
                            | "throw"
                            | "case"
                            | "=>"
                    )
                })
        }
        _ => false,
    }
}

/// Generic arrow functions need type-parameter erasure, but the initial
/// matrix only supports generic declarations and direct calls. Recognize the
/// complete `<...>(...) =>` shape so it cannot be preserved as invalid
/// JavaScript by an otherwise opaque expression span.
fn is_generic_arrow_function(tokens: &[Token], start: usize, end: usize) -> bool {
    if !tokens.get(start).is_some_and(|token| token.is("<")) {
        return false;
    }
    let Some(type_parameters_end) = matching_angle_bracket(tokens, start, end) else {
        return false;
    };
    let parameters_start = type_parameters_end + 1;
    if !tokens
        .get(parameters_start)
        .is_some_and(|token| token.is("("))
    {
        return false;
    }
    let Some(parameters_end) = matching_parenthesis(tokens, parameters_start, end) else {
        return false;
    };
    tokens
        .get(parameters_end + 1)
        .is_some_and(|token| token.is("=>"))
}

fn matching_parenthesis(tokens: &[Token], start: usize, limit: usize) -> Option<usize> {
    debug_assert!(tokens.get(start).is_some_and(|token| token.is("(")));
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(limit).skip(start) {
        match token.text.as_str() {
            "(" => depth += 1,
            ")" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
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

fn matching_angle_bracket(tokens: &[Token], start: usize, limit: usize) -> Option<usize> {
    debug_assert!(tokens.get(start).is_some_and(|token| token.is("<")));
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(limit).skip(start) {
        match token.text.as_str() {
            "<" => depth += 1,
            ">" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
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
    fn rejects_unparenthesized_nullish_and_logical_mixing() {
        for source in [
            "const value = false || null ?? 42;",
            "const value = null ?? false || true;",
            "function choose() { return null ?? false || true; }",
            "null ?? false || true;",
        ] {
            let diagnostics = parse_module("memory:///app.ts", source).unwrap_err();
            assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
            assert_eq!(diagnostics[0].code, DiagnosticCode::ParseError, "{source}");
            assert!(diagnostics[0].message.contains("parentheses are required"));
        }

        parse_module(
            "memory:///app.ts",
            "const left = (false || null) ?? 42; const right = null ?? (false || true);",
        )
        .unwrap();
    }

    #[test]
    fn rejects_unparenthesized_unary_exponentiation_bases() {
        for source in [
            "const invalid: number = -2 ** 2;",
            "const invalid: number = ~(2) ** 2;",
            "function value() { return -value() ** 2; }",
        ] {
            let diagnostics = parse_module("memory:///app.ts", source).unwrap_err();
            assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
            assert_eq!(diagnostics[0].code, DiagnosticCode::ParseError, "{source}");
            assert!(diagnostics[0].message.contains("unparenthesized base"));
        }

        parse_module(
            "memory:///app.ts",
            "const reciprocal: number = 2 ** -3; const squared: number = (-2) ** 2;",
        )
        .unwrap();
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

    #[test]
    fn retains_generic_constraints_and_defaults_for_checker_and_declarations() {
        let module = parse_module(
            "memory:///generic.ts",
            "export interface Box<T extends string = string> { value: T }",
        )
        .unwrap();
        let Declaration::Interface(interface) = &module.declarations[0] else {
            panic!("expected interface declaration");
        };
        assert_eq!(interface.type_parameters.len(), 1);
        assert_eq!(interface.type_parameters[0].name, "T");
        assert_eq!(interface.type_parameters[0].constraint, Some(Type::String));
        assert_eq!(interface.type_parameters[0].default, Some(Type::String));
    }

    #[test]
    fn retains_named_generic_interface_heritage() {
        let module = parse_module(
            "memory:///inheritance.ts",
            "interface Envelope<T> { payload: T }\n\
             interface Tagged { tag: string }\n\
             interface Labeled<T extends string = string> extends Envelope<T>, Tagged { label: T }",
        )
        .unwrap();
        let Declaration::Interface(interface) = &module.declarations[2] else {
            panic!("expected inherited interface declaration");
        };
        assert_eq!(
            interface.heritage,
            vec![
                Type::Named {
                    name: "Envelope".to_string(),
                    arguments: vec![Type::Named {
                        name: "T".to_string(),
                        arguments: Vec::new(),
                    }],
                },
                Type::Named {
                    name: "Tagged".to_string(),
                    arguments: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn rejects_non_named_interface_heritage() {
        let diagnostics = parse_module(
            "memory:///invalid.ts",
            "interface Invalid extends string {}",
        )
        .unwrap_err();
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedSyntax));
    }

    #[test]
    fn parses_and_erases_signature_only_function_overloads() {
        let source = "function describe(value: string): string;\n\
                      function describe(value: number): number;\n\
                      function describe(value: string | number): string | number { return value; }";
        let module = parse_module("memory:///overload.ts", source).unwrap();
        let Declaration::Function(first) = &module.declarations[0] else {
            panic!("expected first overload declaration");
        };
        let Declaration::Function(implementation) = &module.declarations[2] else {
            panic!("expected implementation declaration");
        };
        assert!(first.overload);
        assert!(!implementation.overload);
        assert!(module.edits.iter().any(|edit| {
            &source[edit.start..edit.end] == "function describe(value: string): string;"
        }));
    }

    #[test]
    fn records_and_erases_explicit_direct_call_type_arguments() {
        let source = "function identity<T>(value: T): T { return value; }\n\
                      const result: string = identity<string>('Ada');";
        let module = parse_module("memory:///generic-call.ts", source).unwrap();
        let call_start = source.rfind("identity<string>").unwrap();
        assert_eq!(
            module.generic_call_type_arguments.get(&call_start),
            Some(&vec![Type::String])
        );
        assert!(module.edits.iter().any(|edit| {
            &source[edit.start..edit.end] == "<string>" && edit.replacement.is_empty()
        }));
    }
}
