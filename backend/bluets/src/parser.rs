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
    pub readonly: bool,
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
    /// A semicolon-terminated runtime expression statement. Its original
    /// tokens preserve BlueTSC emission while allowing the BlueTS-to-BlueJS
    /// bridge to lower the expression without parsing emitted JavaScript.
    Expression {
        tokens: Vec<Token>,
        span: SourceSpan,
    },
    /// A runtime throw statement whose value remains structured for direct
    /// BlueJS lowering. The standalone emitter preserves the same source
    /// tokens after TypeScript-only edits are erased.
    Throw {
        tokens: Vec<Token>,
        span: SourceSpan,
    },
    /// A braced `if` statement. Its typed representation distinguishes an
    /// `else if` from a braced `else` block so direct lowering preserves the
    /// BlueJS AST shape without adding an artificial block scope.
    If(FunctionIfStatement),
    Return {
        tokens: Vec<Token>,
        span: SourceSpan,
    },
    Opaque(SourceSpan),
}

/// The bounded function-body representation for one `if` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionIfStatement {
    pub test: Vec<Token>,
    pub consequent: Vec<FunctionBodyItem>,
    pub alternate: Option<FunctionElseBranch>,
    pub span: SourceSpan,
}

/// The direct bridge distinguishes a braced `else` block from `else if`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionElseBranch {
    Braced(Vec<FunctionBodyItem>),
    ElseIf(Box<FunctionIfStatement>),
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
    Named {
        name: String,
        arguments: Vec<Type>,
    },
    Array(Box<Type>),
    Tuple(Vec<Type>),
    Record(Vec<TypeField>),
    /// A bounded, non-generic method signature in an interface or record.
    /// It is erased from runtime code but retains exact parameter and result
    /// types for member-call checking and declaration emission.
    Function {
        parameters: Vec<Parameter>,
        result: Box<Type>,
    },
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

#[path = "parser/implementation.rs"]
mod implementation;
use implementation::Parser;

#[cfg(test)]
mod tests;
