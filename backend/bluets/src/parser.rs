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
    /// Structured arrow functions and function expressions found inside
    /// runtime expressions, keyed by the byte offset of their first token.
    /// Their annotations are erased through `edits`; the checker reads
    /// parameters, result type and body from here.
    pub(crate) nested_functions: BTreeMap<usize, NestedFunction>,
}

/// An arrow function or function expression parsed inside a runtime
/// expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedFunction {
    pub kind: NestedFunctionKind,
    /// Declared `async`: its span starts at the `async` token.
    pub async_function: bool,
    /// The name a function expression binds inside its own body.
    pub name: Option<String>,
    /// Erased type parameters (`<T>` on an arrow or function).
    pub type_parameters: Vec<TypeParameter>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<Type>,
    pub body: NestedFunctionBody,
    /// From the first token of the parameter list to the end of the body.
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestedFunctionKind {
    Arrow,
    Function,
    /// An object-literal method, `name(parameters) { .. }`.
    Method,
    /// `get name() { .. }` in an object literal.
    Getter,
    /// `set name(value) { .. }` in an object literal.
    Setter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NestedFunctionBody {
    /// The expression tokens of a concise body.
    Expression(Vec<Token>),
    /// A braced body, in the same structured form as a named function.
    Block {
        items: Vec<FunctionBodyItem>,
        returns: Vec<Vec<Token>>,
        locals: Vec<VariableDeclaration>,
    },
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
    Class(ClassDeclaration),
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
            Self::Class(declaration) => &declaration.span,
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

/// A bounded named class shell. Members remain original tokens until the
/// class checker and direct lowering install a shared structured grammar.
/// That intermediate state is rejected by the checker before any emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassDeclaration {
    pub name: String,
    pub name_span: SourceSpan,
    pub extends_name: Option<String>,
    pub extends_span: Option<SourceSpan>,
    pub body: Vec<Token>,
    /// Token-indexed member boundaries within `body`; opaque members remain
    /// unavailable to the checker and direct bridge.
    pub members: Vec<ClassMemberShell>,
    /// Contiguous same-name method overloads, indexed into `members`.
    /// A missing implementation remains visible for the class checker.
    pub method_groups: Vec<ClassMethodGroup>,
    pub body_span: SourceSpan,
    pub exported: bool,
    pub span: SourceSpan,
}

/// A class member's TypeScript accessibility modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Visibility {
    #[default]
    Public,
    Protected,
    Private,
}

impl Visibility {
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Protected => "protected",
            Self::Private => "private",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassMemberKind {
    Constructor,
    Method,
    Field,
    Opaque,
}

/// One class member's original token and byte range. The token offsets index
/// `ClassDeclaration::body` and avoid cloning a potentially large body again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassMemberShell {
    pub kind: ClassMemberKind,
    pub name: Option<String>,
    pub token_start: usize,
    pub token_end: usize,
    pub span: SourceSpan,
    /// Present only after the bounded constructor grammar has parsed this
    /// shell.
    pub constructor: Option<ClassConstructor>,
    pub method: Option<ClassMethod>,
    pub field: Option<ClassField>,
}

/// A public instance or static property declaration:
/// `[static] [readonly] name[?|!][: T] [= initializer];`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassField {
    pub name: String,
    pub name_span: SourceSpan,
    pub visibility: Visibility,
    pub is_static: bool,
    pub readonly: bool,
    pub optional: bool,
    /// Written `name!: T`, asserting the field is assigned before use.
    pub definite: bool,
    pub annotation: Option<Type>,
    pub annotation_span: Option<SourceSpan>,
    /// Original runtime tokens of the initializer, without the `=`.
    pub initializer: Option<Vec<Token>>,
    /// A parameter property whose only type source is a default value.
    pub from_default: bool,
    pub span: SourceSpan,
}

impl ClassDeclaration {
    /// The properties the constructor's parameter properties declare, as
    /// fields, in parameter order. A default initializer with no annotation
    /// gives the widened type of a literal default; readonly does not keep the
    /// literal, unlike a `readonly` field declaration.
    pub fn parameter_property_fields(&self) -> Vec<ClassField> {
        let Some(constructor) = self
            .members
            .iter()
            .filter_map(|member| member.constructor.as_ref())
            .find(|constructor| constructor.body.is_some())
        else {
            return Vec::new();
        };
        constructor
            .parameter_properties
            .iter()
            .map(|property| {
                let parameter = &constructor.parameters[property.parameter_index];
                let annotation = parameter.annotation.clone().or_else(|| {
                    parameter
                        .default
                        .as_deref()
                        .and_then(|tokens| widen_literal_tokens(tokens, false))
                });
                ClassField {
                    name: parameter.name.clone(),
                    name_span: parameter.span.clone(),
                    visibility: property.visibility,
                    is_static: false,
                    readonly: property.readonly,
                    optional: parameter.optional && parameter.default.is_none(),
                    // Assigned by the synthesized constructor statement.
                    definite: true,
                    annotation,
                    annotation_span: None,
                    initializer: None,
                    from_default: parameter.default.is_some(),
                    span: parameter.span.clone(),
                }
            })
            .collect()
    }
}

/// The type of a lone number, string or boolean literal (signed for a number),
/// widened unless `keep_literal`.
pub fn widen_literal_tokens(tokens: &[Token], keep_literal: bool) -> Option<Type> {
    let (negative, literal) = match tokens {
        [sign, literal] if sign.is("-") => (true, literal),
        [literal] => (false, literal),
        _ => return None,
    };
    match literal.kind {
        TokenKind::Number => Some(if keep_literal {
            Type::Literal(format!(
                "{}{}",
                if negative { "-" } else { "" },
                literal.text
            ))
        } else {
            Type::Number
        }),
        TokenKind::String if !negative => Some(if keep_literal {
            Type::Literal(literal.text.clone())
        } else {
            Type::String
        }),
        _ if !negative && (literal.is("true") || literal.is("false")) => Some(if keep_literal {
            Type::Literal(literal.text.clone())
        } else {
            Type::Boolean
        }),
        _ => None,
    }
}

impl ClassField {
    /// The type a field carries: its annotation, or, without one, the widened
    /// type of a literal initializer (kept literal for a `readonly` field, as
    /// TypeScript does). `None` for an unannotated field whose type would need
    /// general expression inference at bind time.
    pub fn declared_type(&self) -> Option<Type> {
        if let Some(annotation) = &self.annotation {
            return Some(annotation.clone());
        }
        widen_literal_tokens(self.initializer.as_deref()?, self.readonly)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassConstructor {
    pub visibility: Visibility,
    pub parameters: Vec<Parameter>,
    /// Parameters written with an accessibility modifier or `readonly`, each
    /// of which also declares a property assigned from its argument.
    pub parameter_properties: Vec<ParameterProperty>,
    /// Where those assignments go, when the constructor has properties and
    /// the place can be found (see `ParameterPropertyInsertion`).
    pub parameter_property_insertion: Option<ParameterPropertyInsertion>,
    /// `None` denotes a signature declaration; `Some` retains body items,
    /// including an empty implementation body.
    pub body: Option<Vec<FunctionBodyItem>>,
    pub span: SourceSpan,
}

/// A constructor parameter that also declares a property:
/// `constructor(private x: number)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterProperty {
    /// Index into the constructor's `parameters`.
    pub parameter_index: usize,
    pub visibility: Visibility,
    pub readonly: bool,
}

/// The point in a constructor body after which `this.p = p;` is emitted for
/// each parameter property: the start of the body in a base class, just after
/// the `super(...)` statement in a derived one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterPropertyInsertion {
    /// How many top-level body items come before the assignments.
    pub item_index: usize,
    /// The source offset the assignment text is inserted at.
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassMethod {
    pub name: String,
    pub visibility: Visibility,
    pub is_static: bool,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<Type>,
    /// Original annotation tokens, excluding the colon and trailing gap.
    pub return_type_span: Option<SourceSpan>,
    pub body: Option<Vec<FunctionBodyItem>>,
    pub span: SourceSpan,
}

/// One source-ordered method declaration or overload set. Member indices
/// refer to `ClassDeclaration::members` and preserve its original spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassMethodGroup {
    pub name: String,
    pub is_static: bool,
    pub signature_member_indices: Vec<usize>,
    pub implementation_member_index: Option<usize>,
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

/// A destructuring pattern in the supported subset: shorthand, renamed and
/// defaulted object properties, and named, defaulted or skipped array
/// elements. Nested patterns, rest elements and computed keys are outside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingPattern {
    Object(Vec<ObjectBinding>),
    Array(Vec<Option<ElementBinding>>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectBinding {
    /// The property read from the value.
    pub key: String,
    /// The local name it is bound to.
    pub name: String,
    pub default: Option<Vec<Token>>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementBinding {
    pub name: String,
    pub default: Option<Vec<Token>>,
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
    /// `async` changes the runtime result to a Promise and cannot satisfy a
    /// primitive-string emitted boundary after type erasure.
    pub async_function: bool,
    /// Original `{` byte position for emitted boundary insertion. Signature
    /// declarations have no body opening brace.
    pub body_open: Option<usize>,
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
    /// The parameter's name, or for a destructured parameter the source text
    /// of its pattern (which is also how a declaration prints it).
    pub name: String,
    /// The destructuring pattern, for a parameter written `{ a, b }` or `[a, b]`.
    pub pattern: Option<BindingPattern>,
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
    /// A braced `while` in a named function body. Runtime lowering remains
    /// separately gated by the direct bridge.
    While(FunctionWhileStatement),
    /// A braced `try` with a single identifier catch, a finalizer, or both.
    /// The direct bridge separately gates checker scope and runtime lowering.
    Try(FunctionTryStatement),
    Return {
        tokens: Vec<Token>,
        span: SourceSpan,
    },
    /// A function declared inside a body. It is hoisted to the top of that
    /// body for the checker and keeps its own structured body.
    Function(Box<FunctionDeclaration>),
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

/// The bounded function-body representation for one braced `while` loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionWhileStatement {
    pub test: Vec<Token>,
    pub body: Vec<FunctionBodyItem>,
    pub span: SourceSpan,
}

/// The bounded function-body representation for one braced `try` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionTryStatement {
    pub block: Vec<FunctionBodyItem>,
    pub handler: Option<FunctionCatchClause>,
    pub finalizer: Option<Vec<FunctionBodyItem>>,
    pub span: SourceSpan,
}

/// One unannotated identifier binding, visible only in the catch body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionCatchClause {
    pub binding: String,
    /// The erased `: any` or `: unknown` on the binding, when written.
    pub annotation: Option<Type>,
    pub body: Vec<FunctionBodyItem>,
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

/// One element of a parsed tuple type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TupleTypeElement {
    pub(crate) annotation: Type,
    pub(crate) optional: bool,
    pub(crate) label: Option<String>,
    pub(crate) rest: bool,
}

impl TupleTypeElement {
    /// Construct the currently supported required, unlabeled tuple element.
    pub fn required(annotation: Type) -> Self {
        Self {
            annotation,
            optional: false,
            label: None,
            rest: false,
        }
    }

    pub fn annotation(&self) -> &Type {
        &self.annotation
    }

    pub fn is_optional(&self) -> bool {
        self.optional
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub fn is_rest(&self) -> bool {
        self.rest
    }

    pub(crate) fn value_type(&self) -> Type {
        if self.optional {
            Type::Union(vec![self.annotation.clone(), Type::Undefined])
        } else {
            self.annotation.clone()
        }
    }

    pub(crate) fn indexed_type(&self) -> Type {
        if self.rest {
            match &self.annotation {
                Type::Array(element) => (**element).clone(),
                _ => Type::Unknown,
            }
        } else {
            self.value_type()
        }
    }
}

/// A concrete tuple spread can place an optional source position before a
/// required suffix. The position then remains present and accepts undefined.
pub(crate) fn require_tuple_positions_before_suffix(elements: &mut [TupleTypeElement]) {
    let mut required_suffix = false;
    for element in elements.iter_mut().rev() {
        if element.optional && required_suffix {
            element.annotation = Type::Union(vec![element.annotation.clone(), Type::Undefined]);
            element.optional = false;
        }
        required_suffix |= !element.optional && !element.rest;
    }
}

/// The supported, reifiable portion of the TypeScript type grammar.
impl Type {
    /// Renders an array element type, parenthesized when its own syntax binds
    /// looser than the postfix `[]` (a union, intersection or function type).
    pub(crate) fn array_element_text(&self, render: impl Fn(&Type) -> String) -> String {
        let text = render(self);
        if matches!(
            self,
            Type::Union(_) | Type::Intersection(_) | Type::Function { .. }
        ) {
            format!("({text})[]")
        } else {
            format!("{text}[]")
        }
    }
}

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
    Tuple(Vec<TupleTypeElement>),
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
