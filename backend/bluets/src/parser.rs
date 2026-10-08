// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parser for the explicitly supported BlueTS language subset.

use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::syntax::{
    lex, lex_with_limits, string_contents, Token, TokenKind, MAX_SOURCE_BYTES, MAX_TOKENS,
};
use std::collections::BTreeMap;

mod assertions;
mod type_forms;
pub use type_forms::{
    ConditionalType, IndexSignature, MappedModifier, MappedType, TemplateLiteralType, Variance,
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
    /// Explicit type arguments on direct identifier calls, keyed by the
    /// callee token's source-byte offset. They are static-only and erased from
    /// JavaScript, but retained for the checker to validate a local call.
    pub(crate) generic_call_type_arguments: BTreeMap<usize, Vec<Type>>,
    /// Structured arrow functions and function expressions found inside
    /// runtime expressions, keyed by the byte offset of their first token.
    /// Their annotations are erased through `edits`; the checker reads
    /// parameters, result type and body from here.
    pub(crate) nested_functions: BTreeMap<usize, NestedFunction>,
    /// Class expressions retain their lexical self name independently of their identity.
    pub(crate) class_expressions: BTreeMap<usize, ClassExpression>,
    /// Named type uses, including erased assertions, at their original tokens.
    pub(crate) type_references: Vec<TypeReference>,
    /// Annotations on declarations retained inside opaque control flow.
    pub(crate) expression_variable_types: BTreeMap<usize, Type>,
    /// Retained static `as` targets, keyed by the assertion keyword.
    pub(crate) type_assertions: BTreeMap<usize, Type>,
}

mod class_expressions;
pub(crate) use class_expressions::type_parameter_identity;
pub use class_expressions::ClassExpression;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypeReference {
    pub(crate) name: String,
    pub(crate) value_query: bool,
    pub(crate) span: SourceSpan,
}

/// An arrow function or function expression parsed inside a runtime
/// expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedFunction {
    pub kind: NestedFunctionKind,
    /// Declared `async`: its span starts at the `async` token.
    pub async_function: bool,
    /// A generator function or method.
    pub generator: bool,
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
    Enum(EnumDeclaration),
    Namespace(NamespaceDeclaration),
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
            Self::Enum(declaration) => &declaration.span,
            Self::Namespace(declaration) => &declaration.span,
            Self::Raw(declaration) => &declaration.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDeclaration {
    /// `import x = require("m")`: one binding of the module's `export =` value.
    pub equals_require: bool,
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
    /// `export = name;`: the module's whole export is that value, as the one
    /// binding named `export=`.
    pub export_assignment: bool,
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
    /// An erased `abstract` keyword; its origin remains available to checking.
    pub abstract_modifier: Option<SourceSpan>,
    /// Structural instance obligations, with each original heritage type span.
    pub implements: Vec<(Type, SourceSpan)>,
    pub type_parameters: Vec<TypeParameter>,
    /// Lexical binders retained by a class expression, without becoming its
    /// constructor's own generic parameters.
    pub captured_type_parameters: Box<[TypeParameter]>,
    /// The decorators written before the class (before or after `export`).
    pub decorators: Vec<Decorator>,
    pub name: String,
    pub name_span: SourceSpan,
    pub extends_name: Option<String>,
    pub extends_span: Option<SourceSpan>,
    /// Type arguments on the runtime base expression, erased from JavaScript.
    pub extends_arguments: Vec<Type>,
    pub extends_type_span: Option<SourceSpan>,
    pub body: Vec<Token>,
    /// Token-indexed member boundaries within `body`; opaque members remain
    /// unavailable to the checker and direct bridge.
    pub members: Vec<ClassMemberShell>,
    /// Contiguous same-name method overloads, indexed into `members`.
    /// A missing implementation remains visible for the class checker.
    pub method_groups: Vec<ClassMethodGroup>,
    pub body_span: SourceSpan,
    /// The fields of an interface of the same name that merges into this class.
    /// They are part of the instance type and nothing at run time.
    pub merged_interface_fields: Vec<TypeField>,
    pub exported: bool,
    pub span: SourceSpan,
}

/// A decorator: `@expression`, written before a class or one of its members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decorator {
    /// The expression's tokens, without the `@`: `dec`, `a.b`, `dec(args)` or
    /// `(expression)`.
    pub tokens: Vec<Token>,
    /// From the `@` to the end of the expression.
    pub span: SourceSpan,
    /// The expression's token range in the token list it was read from (the
    /// module's for a class decorator, the class body's for a member's).
    pub token_start: usize,
    pub token_end: usize,
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
    Accessor,
    StaticBlock,
    IndexSignature,
    Opaque,
}

/// One class member's original token and byte range. The token offsets index
/// `ClassDeclaration::body` and avoid cloning a potentially large body again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassMemberShell {
    pub abstract_modifier: Option<SourceSpan>,
    pub override_modifier: Option<SourceSpan>,
    /// The decorators written before the member. `token_start` and `span` begin
    /// after them.
    pub decorators: Vec<Decorator>,
    pub kind: ClassMemberKind,
    pub name: Option<String>,
    /// Original property-name tokens, including brackets for a computed key.
    pub key: Vec<Token>,
    pub token_start: usize,
    pub token_end: usize,
    pub span: SourceSpan,
    /// Present only after the bounded constructor grammar has parsed this
    /// shell.
    pub constructor: Option<ClassConstructor>,
    pub method: Option<ClassMethod>,
    pub field: Option<ClassField>,
    pub accessor: Option<ClassAccessor>,
    pub static_block: Option<ClassStaticBlock>,
    /// Placement and static-only contract of `[key: K]: V`.
    pub index: Option<(bool, IndexSignature)>,
}

/// `[export] [declare] namespace A.B { .. }` (or `module`). A dotted name is one
/// declaration per segment, each inner one exported from its parent and marked
/// `implicit`, since it has no header or closing brace of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceDeclaration {
    pub name: String,
    pub name_span: SourceSpan,
    pub exported: bool,
    /// `declare namespace`: an ambient declaration with no runtime form.
    pub declared: bool,
    /// The body has `export {};`, which makes only members with their own
    /// `export` exported even in an ambient namespace.
    pub explicit_exports: bool,
    /// A segment of a dotted name after the first: written only as part of its
    /// parent's header.
    pub implicit: bool,
    /// The declarations written in the body, in source order.
    pub body: Vec<Declaration>,
    /// From the `namespace` keyword (or the first `export`/`declare`) through
    /// the opening brace.
    pub header_span: SourceSpan,
    /// The closing brace.
    pub closing_span: SourceSpan,
    pub span: SourceSpan,
}

impl NamespaceDeclaration {
    /// Whether the body has any explicit `export`; an ambient body without one
    /// exports every member.
    pub fn exports_every_member(&self) -> bool {
        self.declared && !self.explicit_exports && !self.body.iter().any(declaration_is_exported)
    }
}

fn declaration_is_exported(declaration: &Declaration) -> bool {
    match declaration {
        Declaration::TypeAlias(item) => item.exported,
        Declaration::Interface(item) => item.exported,
        Declaration::Variable(item) => item.exported,
        Declaration::Function(item) => item.exported,
        Declaration::Class(item) => item.exported,
        Declaration::Enum(item) => item.exported,
        Declaration::Namespace(item) => item.exported && !item.implicit,
        _ => false,
    }
}

/// `[export] [declare] [const] enum Name { A, B = 1, "c" = "x" }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDeclaration {
    pub name: String,
    pub name_span: SourceSpan,
    pub exported: bool,
    /// `declare enum`: an ambient declaration with no runtime form.
    pub declared: bool,
    /// `const enum`: uses are replaced by the member's value.
    pub is_const: bool,
    pub members: Vec<EnumMember>,
    pub body_span: SourceSpan,
    pub span: SourceSpan,
}

/// One member of an enum, with its initializer's original tokens if written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumMember {
    /// The member's name, unquoted when written as a string.
    pub name: String,
    pub name_span: SourceSpan,
    pub initializer: Option<Vec<Token>>,
    pub span: SourceSpan,
}

/// A `static { .. }` initialization block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassStaticBlock {
    pub body: Vec<FunctionBodyItem>,
    pub span: SourceSpan,
}

/// A `get name(): T { .. }` or `set name(value: T) { .. }` member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassAccessor {
    pub name: String,
    pub name_span: SourceSpan,
    pub visibility: Visibility,
    pub is_static: bool,
    /// `true` for `get`, `false` for `set`.
    pub getter: bool,
    /// Empty for a getter; the one value parameter for a setter.
    pub parameters: Vec<Parameter>,
    /// A getter's annotated result. A setter has none.
    pub return_type: Option<Type>,
    pub return_type_span: Option<SourceSpan>,
    pub body: Vec<FunctionBodyItem>,
    /// An abstract accessor signature has no executable body.
    pub body_present: bool,
    pub span: SourceSpan,
}

/// A public instance or static property declaration:
/// `[static] [readonly] name[?|!][: T] [= initializer];`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassField {
    /// An ambient field contributes a type without defining a runtime property.
    pub declared: bool,
    /// Written `accessor name`: an auto-accessor, a field with a private backing
    /// store and a generated getter and setter.
    pub accessor: bool,
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
    /// Each accessor as the method it behaves like when its body is checked: a
    /// getter returns its annotated type, a setter returns nothing.
    pub fn accessor_methods(&self) -> Vec<ClassMethod> {
        self.members
            .iter()
            .filter_map(|member| member.accessor.as_ref())
            .map(|accessor| ClassMethod {
                name: accessor.name.clone(),
                name_span: accessor.name_span.clone(),
                visibility: accessor.visibility,
                is_static: accessor.is_static,
                type_parameters: Vec::new(),
                parameters: accessor.parameters.clone(),
                return_type: if accessor.getter {
                    accessor.return_type.clone()
                } else {
                    Some(Type::Void)
                },
                return_type_span: accessor.return_type_span.clone(),
                body: accessor.body_present.then(|| accessor.body.clone()),
                optional: false,
                span: accessor.span.clone(),
            })
            .collect()
    }

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
                    declared: false,
                    accessor: false,
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
    /// Where statements a constructor must begin with (parameter-property and
    /// lowered field assignments) go, when the place can be found (see
    /// `ParameterPropertyInsertion`).
    pub prologue_insertion: Option<ParameterPropertyInsertion>,
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
    pub override_modifier: bool,
    pub modifiers_start: usize,
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
    pub name_span: SourceSpan,
    pub visibility: Visibility,
    pub is_static: bool,
    pub optional: bool,
    pub type_parameters: Vec<TypeParameter>,
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
    pub signatures: Vec<TypeSignature>,
    pub indices: Vec<IndexSignature>,
    pub exported: bool,
    pub span: SourceSpan,
}

impl InterfaceDeclaration {
    pub(crate) fn body_type(&self) -> Type {
        let object = if self.signatures.is_empty() {
            Type::Record(self.fields.clone())
        } else {
            Type::CallableRecord {
                fields: self.fields.clone(),
                signatures: self.signatures.clone(),
            }
        };
        if self.indices.is_empty() {
            object
        } else {
            Type::IndexedRecord {
                object: Box::new(object),
                indices: self.indices.clone(),
            }
        }
    }
}

/// A call or construct signature retains its binders and declaration origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeSignature {
    pub construct: bool,
    pub abstract_constructor: bool,
    /// Class constructor accessibility retained when its value is aliased.
    pub constructor_visibility: Visibility,
    /// Written as `new (...) => T` rather than an object signature.
    pub constructor_arrow: bool,
    pub type_parameters: Vec<TypeParameter>,
    pub parameters: Vec<Parameter>,
    pub result: Type,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeField {
    pub method: bool,
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
    /// `function*`: the body may `yield`, and the result is a generator.
    pub generator: bool,
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
    /// Legacy parameter decorators (`m(@d x: number)`), in source order. Each
    /// decorator's token range indexes the module's token list.
    pub decorators: Vec<Decorator>,
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
    pub variance: Option<Variance>,
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

/// A value query's internal identity includes its source position, because
/// two queries with the same spelling can resolve different shadowed values.
/// Diagnostics and declaration output retain the source spelling.
pub(crate) fn source_type_name(name: &str) -> &str {
    if let Some(source) = class_expressions::parameter_source_name(name) {
        return source;
    }
    if name.starts_with("typeof ") {
        if let Some((source, position)) = name.rsplit_once('@') {
            if position.parse::<usize>().is_ok() {
                return source;
            }
        }
    }
    name
}

/// The supported, reifiable portion of the TypeScript type grammar.
impl Type {
    /// Renders an array element type, parenthesized when its own syntax binds
    /// looser than the postfix `[]` (a union, intersection or function type).
    pub(crate) fn array_element_text(&self, render: impl Fn(&Type) -> String) -> String {
        let text = render(self);
        if matches!(
            self,
            Type::Union(_)
                | Type::Intersection(_)
                | Type::Function { .. }
                | Type::GenericFunction { .. }
                | Type::KeyOf(_)
                | Type::Conditional(_)
        ) {
            format!("({text})[]")
        } else {
            format!("{text}[]")
        }
    }
}

/// An explicit guard/assertion result, retained separately from its runtime
/// boolean/void result so calls can refine the corresponding lexical value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypePredicate {
    pub parameter: String,
    pub asserts: bool,
    pub target: Option<Box<Type>>,
    pub span: SourceSpan,
    pub parameter_span: SourceSpan,
    pub is_span: Option<SourceSpan>,
    pub target_span: Option<SourceSpan>,
    pub return_position: bool,
}

impl TypePredicate {
    pub(crate) fn runtime_type(&self) -> Type {
        if self.asserts {
            Type::Void
        } else {
            Type::Boolean
        }
    }

    pub(crate) fn text(&self, render: impl FnOnce(&Type) -> String) -> String {
        format!(
            "{}{}{}",
            if self.asserts { "asserts " } else { "" },
            self.parameter,
            self.target
                .as_deref()
                .map(|target| format!(" is {}", render(target)))
                .unwrap_or_default()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Any,
    Unknown,
    /// Explicit source `unknown`; `Unknown` also represents opaque inference.
    StrictUnknown,
    Never,
    Void,
    Null,
    Undefined,
    Boolean,
    Number,
    BigInt,
    Symbol,
    UniqueSymbol(SourceSpan),
    /// Static assertion marker, never a value or a runtime contract.
    ConstAssertion,
    String,
    Literal(String),
    Named {
        name: String,
        arguments: Vec<Type>,
    },
    Array(Box<Type>),
    Readonly(Box<Type>),
    Tuple(Vec<TupleTypeElement>),
    Record(Vec<TypeField>),
    IndexedRecord {
        object: Box<Type>,
        indices: Vec<IndexSignature>,
    },
    CallableRecord {
        fields: Vec<TypeField>,
        signatures: Vec<TypeSignature>,
    },
    /// A bounded, non-generic method signature in an interface or record.
    /// It is erased from runtime code but retains exact parameter and result
    /// types for member-call checking and declaration emission.
    Function {
        parameters: Vec<Parameter>,
        result: Box<Type>,
    },
    KeyOf(Box<Type>),
    IndexedAccess {
        object: Box<Type>,
        index: Box<Type>,
        index_span: SourceSpan,
    },
    Conditional(Box<ConditionalType>),
    Infer(Box<TypeParameter>),
    Mapped(Box<MappedType>),
    TemplateLiteral(TemplateLiteralType),
    GenericFunction {
        type_parameters: Vec<TypeParameter>,
        parameters: Vec<Parameter>,
        result: Box<Type>,
        span: SourceSpan,
    },
    Predicate(Box<TypePredicate>),
    Union(Vec<Type>),
    Intersection(Vec<Type>),
}

impl Type {
    pub(crate) fn runtime_result(&self) -> Type {
        match self {
            Type::Predicate(predicate) => predicate.runtime_type(),
            value => value.clone(),
        }
    }
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
    parse_module_with_namespaces(id, source, limits, &[])
}

/// Parses a module that imports namespaces, given each by its local name and
/// what it exports, so a reference to one of their members is one token.
pub(crate) fn parse_module_with_namespaces(
    id: impl Into<String>,
    source: impl Into<String>,
    limits: ParserLimits,
    imported_namespaces: &[(String, NamespaceTree)],
) -> Result<Module, Vec<Diagnostic>> {
    let id = id.into();
    let source = source.into();
    let tokens = if limits == ParserLimits::default() {
        lex(&id, &source)?
    } else {
        lex_with_limits(&id, &source, limits.max_source_bytes, limits.max_tokens)?
    };
    // A module with namespaces is parsed twice: the first pass learns which
    // names are namespaces and what they export, and the second merges each
    // qualified reference (`N.x`) into one token before parsing again.
    let first = Parser::new(
        id.clone(),
        source.clone(),
        tokens.clone(),
        limits.max_type_depth,
    )
    .parse_module()?;
    let Some(names) =
        namespace_names::NamespaceNames::collect(&first.declarations, imported_namespaces)
    else {
        return Ok(first);
    };
    Parser::new(id, source, tokens, limits.max_type_depth)
        .with_namespace_names(&names)
        .parse_module()
}

#[path = "parser/implementation.rs"]
mod implementation;
mod namespace_names;
use implementation::Parser;
pub use namespace_names::{exported_namespace_trees, NamespaceTree};

#[cfg(test)]
mod tests;
