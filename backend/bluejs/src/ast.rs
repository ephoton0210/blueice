// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The AST `parser.rs` builds -- scoped to exactly
//! `phase-2-mvp-scope/PLAN.md`'s "MVP JS scope (decided)" section. Two
//! omissions worth calling out because they're easy to expect and
//! aren't oversights: there is no `ClassDecl`/`ClassExpr` (that section
//! defers `class` entirely). Assignment patterns are deliberately a
//! separate AST from binding patterns because their leaves may be
//! existing member references as well as identifiers.

use crate::JsString;
use num_bigint::BigInt;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub body: Vec<Stmt>,
}

/// The syntactic information a Source Text Module Record retains after its
/// executable statements have been parsed.  Import and export declarations
/// are deliberately not ordinary statements: they participate in linking
/// before any statement in the graph is evaluated.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub body: Vec<Stmt>,
    pub imports: Vec<ImportEntry>,
    pub exports: Vec<ExportEntry>,
    /// [[RequestedModules]] in source-text order. Import/export entries are
    /// otherwise stored separately for resolution, which must not change the
    /// dependency evaluation order.
    pub requests: Vec<RequestedModule>,
}

/// One ModuleRequest of a Source Text Module Record: the specifier and the
/// phase it is imported at. A module requested both eagerly and deferred
/// (`import defer * as ns from "m"` next to `import "m"`) has one entry per
/// phase, because the phases contribute to evaluation differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestedModule {
    pub specifier: String,
    /// The `with { type }` attribute the request carries; a request is
    /// identified by its specifier *and* its module type.
    pub module_type: ModuleType,
    /// `Evaluation` or `Defer`; source-phase imports never evaluate, so they
    /// are not requested modules at all.
    pub phase: ImportPhase,
}

/// The `type` import attribute of a ModuleRequest, which selects how the
/// host turns the resolved resource into a module record: as a Source Text
/// Module (no attribute), or as a synthetic module whose only export is its
/// `default` (JSON modules, and the import-text and import-bytes proposals).
/// The attribute is part of a request's identity: `import "./m" with { type:
/// "text" }` and `import "./m"` name two distinct module records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ModuleType {
    /// No (recognized) `type` attribute: a Source Text Module.
    #[default]
    JavaScript,
    /// `type: "json"`: ParseJSONModule.
    Json,
    /// `type: "text"`: the resource decoded as UTF-8 into a String.
    Text,
    /// `type: "bytes"`: the resource as a `Uint8Array` over an immutable
    /// `ArrayBuffer`.
    Bytes,
}

impl ModuleType {
    /// The module type a `type` attribute value selects. Other attribute
    /// values are accepted and ignored, as every other attribute key is.
    pub fn from_attribute_value(value: &str) -> ModuleType {
        match value {
            "json" => ModuleType::Json,
            "text" => ModuleType::Text,
            "bytes" => ModuleType::Bytes,
            _ => ModuleType::JavaScript,
        }
    }

    fn key_suffix(self) -> Option<&'static str> {
        match self {
            ModuleType::JavaScript => None,
            ModuleType::Json => Some("json"),
            ModuleType::Text => Some("text"),
            ModuleType::Bytes => Some("bytes"),
        }
    }

    /// The module-registry key of `resolved_path` loaded as this type. A
    /// Source Text Module keeps its plain resolved path; every synthetic
    /// module gets a distinct key, so one resource imported under two types
    /// (or a module importing itself as text) yields two module records.
    pub(crate) fn module_key(self, resolved_path: &str) -> String {
        match self.key_suffix() {
            None => resolved_path.to_string(),
            Some(suffix) => format!("{resolved_path}\0{suffix}"),
        }
    }

    /// Splits a registry key made by [`Self::module_key`] back into the
    /// resource path the host knows it by and its module type.
    pub(crate) fn split_module_key(key: &str) -> (&str, ModuleType) {
        for module_type in [ModuleType::Json, ModuleType::Text, ModuleType::Bytes] {
            let suffix = module_type
                .key_suffix()
                .expect("synthetic types have a suffix");
            if let Some(path) = key
                .strip_suffix(suffix)
                .and_then(|prefix| prefix.strip_suffix('\0'))
            {
                return (path, module_type);
            }
        }
        (key, ModuleType::JavaScript)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportName {
    Named(String),
    Namespace,
    /// `import defer * as local from "specifier"`: a *deferred* namespace
    /// object that evaluates its module on first observation.
    DeferredNamespace,
    /// `import source local from "specifier"`: a host-provided Module Source
    /// Object, never linked or evaluated as an ordinary module.
    Source,
}

/// The phase an ImportCall requests of its module: `import()` evaluates it,
/// `import.source()` asks the host for its source object (source-phase
/// imports proposal), and `import.defer()` links it but postpones evaluation
/// until its namespace is first observed (import-defer proposal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportPhase {
    Evaluation,
    Source,
    Defer,
}

impl ImportPhase {
    /// The `DynamicImport` instruction's operand encoding of this phase.
    pub(crate) fn operand(self) -> u32 {
        match self {
            ImportPhase::Evaluation => 0,
            ImportPhase::Source => 1,
            ImportPhase::Defer => 2,
        }
    }

    pub(crate) fn from_operand(operand: usize) -> ImportPhase {
        match operand {
            1 => ImportPhase::Source,
            2 => ImportPhase::Defer,
            _ => ImportPhase::Evaluation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEntry {
    pub module_request: String,
    pub import_name: ImportName,
    /// `None` represents `import "specifier";`, which participates in
    /// dependency evaluation but creates no local binding.
    pub local_name: Option<String>,
    /// This request's `with { type }` attribute, routing it to a synthetic
    /// module (JSON, text, bytes) instead of ordinary module linking.
    pub module_type: ModuleType,
}

/// One declarative export.  Local entries point at a binding in this module;
/// indirect and star entries are followed during graph resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportEntry {
    Local {
        export_name: String,
        local_name: String,
    },
    Indirect {
        export_name: String,
        module_request: String,
        import_name: String,
        module_type: ModuleType,
    },
    Star {
        module_request: String,
        module_type: ModuleType,
    },
    Namespace {
        export_name: String,
        module_request: String,
        module_type: ModuleType,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    Var,
    Let,
    Const,
    /// `using x = value;` (Explicit Resource Management, synchronous
    /// disposal). Scoping/TDZ/immutability behave like `const`; the
    /// distinguishing behavior is that the compiler arranges for the bound
    /// value's `[Symbol.dispose]` to run when the enclosing block exits.
    Using,
    /// `await using x = value;` -- same, but disposal happens through
    /// `[Symbol.asyncDispose]` and is awaited. Parsing/compiling for this
    /// form is not yet implemented; the variant exists so `DeclKind`
    /// already distinguishes the two hints where later code needs to.
    AwaitUsing,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Function {
    pub name: Option<String>,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub generator: bool,
    /// Contextual `async` on a method. Async execution itself is a later
    /// suspension slice, but retaining the grammar prevents valid programs
    /// from being misreported as malformed source.
    pub is_async: bool,
    /// The source text this function was parsed from (its `[[SourceText]]`).
    pub source_text: SourceText,
}

/// The exact source text of a function or class: what
/// `Function.prototype.toString` returns for it. It is a range of the whole
/// text that was parsed, which every function and class of one parse shares
/// (a function nested in another is a sub-range of its parent's range, so no
/// text is ever copied per function).
///
/// The default value has no text: it is what a function the compiler
/// synthesizes (an implicit initializer, an auto-accessor's getter) carries,
/// and `Function.prototype.toString` then reports a NativeFunction.
///
/// A source range is metadata, not structure, so it never takes part in
/// equality: two syntax trees that differ only in where their functions were
/// written are equal.
#[derive(Clone, Default)]
pub struct SourceText {
    text: Option<Arc<str>>,
    /// Byte offsets into `text`, on character boundaries.
    start: u32,
    end: u32,
}

impl SourceText {
    /// The range `start..end` (byte offsets on character boundaries) of
    /// `text`. Text longer than `u32::MAX` bytes has no representable range:
    /// it yields the default, textless value rather than a wrong range.
    pub(crate) fn range(text: &Arc<str>, start: usize, end: usize) -> Self {
        debug_assert!(start <= end && text.is_char_boundary(start) && text.is_char_boundary(end));
        match (u32::try_from(start), u32::try_from(end)) {
            (Ok(start), Ok(end)) => Self {
                text: Some(Arc::clone(text)),
                start,
                end,
            },
            _ => Self::default(),
        }
    }

    /// All of `text`, for a function whose source is synthesized by the
    /// specification (CreateDynamicFunction) rather than sliced from a
    /// program.
    pub(crate) fn whole(text: impl Into<Arc<str>>) -> Self {
        let text = text.into();
        let end = text.len();
        Self::range(&text, 0, end)
    }

    /// The source text, or `None` for a function that has none.
    pub fn as_str(&self) -> Option<&str> {
        let text = self.text.as_deref()?;
        text.get(self.start as usize..self.end as usize)
    }
}

/// Prints the range, not the (whole program's) text it is a range of, so a
/// syntax tree stays readable in assertion failures and debug output.
impl std::fmt::Debug for SourceText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.text {
            Some(_) => write!(formatter, "SourceText({}..{})", self.start, self.end),
            None => formatter.write_str("SourceText(none)"),
        }
    }
}

impl PartialEq for SourceText {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// A class definition with the executable elements currently supported by the
/// compiler. Private keys share the ordinary key representation with a
/// `#` prefix.
///
/// Decorators (the Stage 3 decorators proposal) are kept as the expressions
/// written after each `@`, in source order: a `DecoratorMemberExpression`, a
/// `DecoratorCallExpression` or the inside of a `DecoratorParenthesizedExpression`
/// (the value the decorator expression produces is what is later called).
#[derive(Debug, Clone, PartialEq)]
pub struct Class {
    pub name: Option<String>,
    pub extends: Option<Box<Expr>>,
    pub elements: Vec<ClassElement>,
    /// Decorators before `class`, in source order.
    pub decorators: Vec<Expr>,
    /// The source text of the whole class, its decorators included: what the
    /// class constructor's `Function.prototype.toString` returns.
    pub source_text: SourceText,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClassElement {
    Method {
        key: PropertyKey,
        function: Function,
        is_static: bool,
        /// Decorators before the element, in source order.
        decorators: Vec<Expr>,
    },
    Accessor {
        key: PropertyKey,
        function: Function,
        getter: bool,
        is_static: bool,
        decorators: Vec<Expr>,
    },
    Field {
        key: PropertyKey,
        initializer: Option<Expr>,
        is_static: bool,
        /// An auto-accessor (`accessor x = 1`): a getter/setter pair backed
        /// by a hidden private field that holds the initializer's value.
        accessor: bool,
        decorators: Vec<Expr>,
    },
    /// A static block cannot be decorated.
    StaticBlock(Vec<Stmt>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub pattern: Pattern,
    pub default: Option<Expr>,
    pub rest: bool,
}

/// A binding target -- for `var`/`let`/`const` declarators, function
/// parameters, and `for`/`for-in`/`for-of` loop heads. Not used for
/// plain assignment-expression targets; see this module's doc comment.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Identifier(String),
    Array(Vec<Option<ArrayPatternElement>>),
    Object(Vec<ObjectPatternProp>),
}

/// A target used by a plain destructuring assignment such as
/// `([name, target.value] = source)`. Unlike [`Pattern`], it never
/// declares names and may write an existing member reference.
#[derive(Debug, Clone, PartialEq)]
pub enum AssignmentPattern {
    Target(Box<Expr>),
    Array(Vec<Option<AssignmentPatternElement>>),
    Object(Vec<AssignmentPatternProp>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssignmentPatternElement {
    pub pattern: AssignmentPattern,
    pub default: Option<Expr>,
    pub rest: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignmentPatternProp {
    KeyValue {
        key: PropertyKey,
        value: AssignmentPattern,
        default: Option<Expr>,
    },
    Rest(AssignmentPattern),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayPatternElement {
    pub pattern: Pattern,
    pub default: Option<Expr>,
    pub rest: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObjectPatternProp {
    KeyValue {
        key: PropertyKey,
        value: Pattern,
        default: Option<Expr>,
    },
    Rest(Pattern),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyKey {
    Identifier(String),
    String(JsString),
    Number(f64),
    Computed(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct VarDeclarator {
    pub pattern: Pattern,
    pub init: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    /// `None` for a `default:` case.
    pub test: Option<Expr>,
    pub consequent: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause {
    pub param: Option<Pattern>,
    pub body: Vec<Stmt>,
}

/// A `for`-loop head. Non-declaration heads use an [`AssignmentPattern`],
/// such as `for (x of values)` or `for ({x: target.value} of values)`. The
/// `Expr` form retains the Annex B web-compat
/// CallExpression target so execution can evaluate its call and then report
/// the required runtime ReferenceError, rather than rejecting the source
/// before the observable call takes place.
#[derive(Debug, Clone, PartialEq)]
pub enum ForHead {
    Decl(DeclKind, Pattern),
    /// Annex B permits a `var` initializer in a sloppy for-in head. The
    /// initializer is evaluated once before the RHS expression.
    AnnexBVarInit(Pattern, Expr),
    Assignment(AssignmentPattern),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ForInit {
    VarDecl(DeclKind, Vec<VarDeclarator>),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Empty,
    Expr(Expr),
    Block(Vec<Stmt>),
    VarDecl(DeclKind, Vec<VarDeclarator>),
    If {
        test: Expr,
        consequent: Box<Stmt>,
        alternate: Option<Box<Stmt>>,
    },
    For {
        init: Option<ForInit>,
        test: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
    },
    ForIn {
        left: ForHead,
        right: Expr,
        body: Box<Stmt>,
    },
    ForOf {
        left: ForHead,
        right: Expr,
        body: Box<Stmt>,
        is_await: bool,
    },
    While {
        test: Expr,
        body: Box<Stmt>,
    },
    DoWhile {
        body: Box<Stmt>,
        test: Expr,
    },
    Switch {
        discriminant: Expr,
        cases: Vec<SwitchCase>,
    },
    /// A labelled statement. Labels target the entire wrapped statement;
    /// consecutive labels are collapsed by the compiler onto an iteration
    /// statement when appropriate.
    Labelled {
        label: String,
        item: Box<Stmt>,
    },
    Break(Option<String>),
    Continue(Option<String>),
    Return(Option<Expr>),
    Throw(Expr),
    Try {
        block: Vec<Stmt>,
        handler: Option<CatchClause>,
        finalizer: Option<Vec<Stmt>>,
    },
    With {
        object: Expr,
        body: Box<Stmt>,
    },
    FunctionDecl(Function),
    /// `export default function …` has no ordinary module-local binding for
    /// its source-level name, but its default export is initialized during
    /// ModuleDeclarationInstantiation just like a function declaration.
    ModuleDefaultFunction {
        function: Function,
        binding: String,
    },
    ClassDecl(Class),
    /// Compiler-internal wrapper for a class field lowered to `this[key] =
    /// initializer` inside the function that defines it (the class's
    /// instance-field initializer, or a static field's own function). The
    /// compiler turns it into DefineField or PrivateFieldAdd.
    ClassField(Box<Stmt>),
    /// Compiler-internal marker inserted before the instance-element
    /// initializers of a class that declares private elements.  The string
    /// names the hidden lexical binding that holds the declaring class's
    /// private-brand owner.
    ClassPrivateBrand(String),
    /// Compiler-internal wrapper for a decorated class field (or an
    /// auto-accessor's hidden storage field). The inner `ClassField` defines
    /// the field; its initial value first passes through the decorators'
    /// initializer functions, and the extra initializers the decorators added
    /// run right after the definition. `record` names the hidden lexical
    /// binding holding the `[extraInitializers, initializers, ...]` record
    /// the decorators produced.
    ClassDecoratedField {
        field: Box<Stmt>,
        record: String,
    },
    /// Compiler-internal: calls the extra initializers (`record[0]`) the
    /// decorators of a method or accessor added, with `this` the instance
    /// being initialized. The string names the hidden record binding.
    ClassExtraInitializers(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Plus,
    Not,
    BitNot,
    Void,
    Typeof,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOp {
    Inc,
    Dec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Exponent,
    Div,
    Mod,
    ShiftLeft,
    ShiftRight,
    UnsignedShiftRight,
    BitAnd,
    BitXor,
    BitOr,
    Eq,
    NotEq,
    StrictEq,
    StrictNotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Instanceof,
    In,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    And,
    Or,
    Nullish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    ExponentAssign,
    DivAssign,
    ModAssign,
    ShiftLeftAssign,
    ShiftRightAssign,
    UnsignedShiftRightAssign,
    BitAndAssign,
    BitXorAssign,
    BitOrAssign,
    LogicalAndAssign,
    LogicalOrAssign,
    NullishAssign,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArrayElement {
    Normal(Expr),
    Spread(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObjectProp {
    KeyValue {
        key: PropertyKey,
        value: Expr,
        shorthand: bool,
    },
    Spread(Expr),
    Method {
        key: PropertyKey,
        function: Function,
    },
    Accessor {
        key: PropertyKey,
        function: Function,
        getter: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Argument {
    Normal(Expr),
    Spread(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArrowBody {
    Expr(Box<Expr>),
    Block(Vec<Stmt>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    BigInt(BigInt),
    String(JsString),
    Bool(bool),
    Null,
    This,
    Identifier(String),
    /// Parentheses preserve AssignmentTargetType metadata: `(name)` remains
    /// a valid assignment target, but it is not an IdentifierReference for
    /// anonymous function name inference.
    Parenthesized(Box<Expr>),
    Template {
        quasis: Vec<JsString>,
        expressions: Vec<Expr>,
    },
    TaggedTemplate {
        tag: Box<Expr>,
        raw: Vec<JsString>,
        cooked: Vec<Option<JsString>>,
        expressions: Vec<Expr>,
    },
    RegExp {
        pattern: JsString,
        flags: JsString,
    },
    Array(Vec<Option<ArrayElement>>),
    Object(Vec<ObjectProp>),
    Function(Function),
    Class(Class),
    Super,
    NewTarget,
    Yield {
        value: Option<Box<Expr>>,
        delegate: bool,
    },
    /// Contextual `await`, valid in async functions and at module top level.
    Await(Box<Expr>),
    /// The `import()` expression is distinct from the static module-item
    /// grammar and always evaluates to a Promise. The optional second
    /// argument carries import attributes (`import(specifier, { with: {...} })`).
    /// `phase` distinguishes `import()` from `import.source()` and
    /// `import.defer()`.
    DynamicImport {
        specifier: Box<Expr>,
        options: Option<Box<Expr>>,
        phase: ImportPhase,
    },
    /// Module-only meta property. Retained independently of host metadata
    /// support so its invalid assignment-target shape is rejected at parse
    /// time.
    ImportMeta,
    Arrow {
        params: Vec<Param>,
        body: ArrowBody,
        is_async: bool,
        source_text: SourceText,
    },
    Unary {
        op: UnaryOp,
        arg: Box<Expr>,
    },
    Update {
        op: UpdateOp,
        arg: Box<Expr>,
        prefix: bool,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Logical {
        op: LogicalOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// A left-to-right `Expression` sequence separated by commas. The value
    /// of the sequence is its final assignment expression.
    Sequence(Vec<Expr>),
    Assign {
        op: AssignOp,
        target: Box<Expr>,
        value: Box<Expr>,
    },
    DestructureAssign {
        pattern: AssignmentPattern,
        value: Box<Expr>,
    },
    Conditional {
        test: Box<Expr>,
        consequent: Box<Expr>,
        alternate: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Argument>,
    },
    /// A call whose callee is tested for nullishness before its arguments are
    /// evaluated.  This remains distinct from an ordinary Call built on an
    /// OptionalMember: `fn?.()` and `fn?.method()` have different guards.
    OptionalCall {
        callee: Box<Expr>,
        args: Vec<Argument>,
    },
    New {
        callee: Box<Expr>,
        args: Vec<Argument>,
    },
    Member {
        object: Box<Expr>,
        property: Box<Expr>,
        computed: bool,
    },
    /// A private-brand check.  The name is source-level (without its `#`);
    /// compilation resolves it through the enclosing class private-name
    /// environment just like a private member reference.
    PrivateIn {
        name: String,
        object: Box<Expr>,
    },
    /// Retained so assignment-target early errors can be established even
    /// before optional-chain execution is implemented.
    OptionalMember {
        object: Box<Expr>,
        property: Box<Expr>,
        computed: bool,
    },
}

/// Whether eval source contains a `super()` that belongs to the surrounding
/// eval context. A nested class establishes its own constructor context, so
/// its elements do not contribute to this check.
pub(crate) fn contains_super_call_outside_class(program: &Program) -> bool {
    program
        .body
        .iter()
        .any(|statement| stmt_contains_super(statement, SuperSearch::Call))
}

/// Whether script source contains a `super` property reference that is not
/// owned by a nested class or object method.
pub(crate) fn contains_super_property_outside_class(program: &Program) -> bool {
    program
        .body
        .iter()
        .any(|statement| stmt_contains_super(statement, SuperSearch::Property))
}

/// Whether an expression contains a `super()` belonging to its surrounding
/// class context. Nested classes establish their own context.
pub(crate) fn expr_contains_super_call_outside_class(expr: &Expr) -> bool {
    expr_contains_super(expr, SuperSearch::Call)
}

/// Whether statements contain a `super()` belonging to their surrounding
/// class context. Nested classes establish their own context.
pub(crate) fn statements_contain_super_call_outside_class(statements: &[Stmt]) -> bool {
    stmts_contain_super_call(statements)
}

/// Whether a function's parameters or body contain a `super()` belonging to
/// its surrounding class context. Nested classes establish their own context.
pub(crate) fn function_contains_super_call_outside_class(function: &Function) -> bool {
    function_contains_super(function, SuperSearch::Call)
}

/// Whether a normal function contains a `super` property reference which is
/// not owned by a nested class or object method.
pub(crate) fn function_contains_super_property_outside_class(function: &Function) -> bool {
    function_contains_super(function, SuperSearch::Property)
}

/// Whether an expression contains the lexical `arguments` reference forbidden
/// by class field initializers. Ordinary functions establish their own
/// `arguments` binding; arrows deliberately do not.
pub(crate) fn expr_contains_arguments(expr: &Expr) -> bool {
    expr_contains_super(expr, SuperSearch::Arguments)
}

/// Whether statements contain the lexical `arguments` reference forbidden by
/// a class static block. Ordinary functions establish their own `arguments`
/// binding, so their bodies are not traversed.
pub(crate) fn statements_contain_arguments(statements: &[Stmt]) -> bool {
    stmts_contain_super(statements, SuperSearch::Arguments)
}

/// Whether a direct `eval(...)` call belongs to these parameters' own
/// evaluation: in a default or a computed key, but not inside a nested
/// function or arrow, which have environments of their own.
pub(crate) fn params_contain_direct_eval(params: &[Param]) -> bool {
    params.iter().any(|param| {
        pattern_contains_super(&param.pattern, SuperSearch::DirectEval)
            || param
                .default
                .as_ref()
                .is_some_and(|expr| expr_contains_super(expr, SuperSearch::DirectEval))
    })
}

/// Whether a direct `eval(...)` call belongs to a function body's own
/// evaluation (not to a nested function or arrow).
pub(crate) fn body_contains_direct_eval(body: &[Stmt]) -> bool {
    stmts_contain_super(body, SuperSearch::DirectEval)
}

/// Whether an ordinary function's parameters or body can observe the
/// function's own `arguments` object: a lexical reference to the name (arrow
/// functions inside it share the object; nested ordinary functions have their
/// own and are not entered) or a direct `eval`, which can read it by name from
/// source that only exists at run time. When neither is present the object is
/// unobservable, so the compiler need not build it on every call.
pub(crate) fn function_may_observe_arguments(function: &Function) -> bool {
    let search = SuperSearch::ArgumentsOrEval;
    function.params.iter().any(|param| {
        pattern_contains_super(&param.pattern, search)
            || param
                .default
                .as_ref()
                .is_some_and(|expr| expr_contains_super(expr, search))
    }) || stmts_contain_super(&function.body, search)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SuperSearch {
    Call,
    Property,
    /// A lexical `arguments` reference, for the class-field and static-block
    /// early errors.
    Arguments,
    /// A direct `eval` call in this function's own parameter list (not in a
    /// nested function or arrow).
    DirectEval,
    /// A lexical `arguments` reference or a direct `eval` call: everything
    /// that can reach the enclosing function's `arguments` object.
    ArgumentsOrEval,
}

impl SuperSearch {
    /// The searches that look for `arguments` and therefore stop at an
    /// ordinary function boundary, which has its own binding.
    fn looks_for_arguments(self) -> bool {
        matches!(self, Self::Arguments | Self::ArgumentsOrEval)
    }
}

/// `eval` written as the callee of a call, possibly parenthesized: the forms
/// that are direct evals when they name the intrinsic.
fn is_eval_reference(expr: &Expr) -> bool {
    match expr {
        Expr::Identifier(name) => name == "eval",
        Expr::Parenthesized(expr) => is_eval_reference(expr),
        _ => false,
    }
}

fn stmts_contain_super_call(statements: &[Stmt]) -> bool {
    stmts_contain_super(statements, SuperSearch::Call)
}

fn stmts_contain_super(statements: &[Stmt], search: SuperSearch) -> bool {
    statements
        .iter()
        .any(|statement| stmt_contains_super(statement, search))
}

fn stmt_contains_super(statement: &Stmt, search: SuperSearch) -> bool {
    match statement {
        Stmt::Empty | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::Expr(expr) | Stmt::Throw(expr) => expr_contains_super(expr, search),
        Stmt::Block(statements) => stmts_contain_super(statements, search),
        Stmt::VarDecl(_, declarations) => declarations.iter().any(|declaration| {
            pattern_contains_super(&declaration.pattern, search)
                || declaration
                    .init
                    .as_ref()
                    .is_some_and(|expr| expr_contains_super(expr, search))
        }),
        Stmt::If {
            test,
            consequent,
            alternate,
        } => {
            expr_contains_super(test, search)
                || stmt_contains_super(consequent, search)
                || alternate
                    .as_deref()
                    .is_some_and(|statement| stmt_contains_super(statement, search))
        }
        Stmt::For {
            init,
            test,
            update,
            body,
        } => {
            init.as_ref()
                .is_some_and(|init| for_init_contains_super(init, search))
                || test
                    .as_ref()
                    .is_some_and(|expr| expr_contains_super(expr, search))
                || update
                    .as_ref()
                    .is_some_and(|expr| expr_contains_super(expr, search))
                || stmt_contains_super(body, search)
        }
        Stmt::ForIn { left, right, body }
        | Stmt::ForOf {
            left, right, body, ..
        } => {
            for_head_contains_super(left, search)
                || expr_contains_super(right, search)
                || stmt_contains_super(body, search)
        }
        Stmt::While { test, body } | Stmt::DoWhile { body, test } => {
            expr_contains_super(test, search) || stmt_contains_super(body, search)
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_contains_super(discriminant, search)
                || cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .is_some_and(|expr| expr_contains_super(expr, search))
                        || stmts_contain_super(&case.consequent, search)
                })
        }
        Stmt::Return(value) => value
            .as_ref()
            .is_some_and(|expr| expr_contains_super(expr, search)),
        Stmt::Try {
            block,
            handler,
            finalizer,
        } => {
            stmts_contain_super(block, search)
                || handler.as_ref().is_some_and(|handler| {
                    pattern_option_contains_super(handler.param.as_ref(), search)
                        || stmts_contain_super(&handler.body, search)
                })
                || finalizer
                    .as_deref()
                    .is_some_and(|statements| stmts_contain_super(statements, search))
        }
        Stmt::With { object, body } => {
            expr_contains_super(object, search) || stmt_contains_super(body, search)
        }
        Stmt::Labelled { item, .. } => stmt_contains_super(item, search),
        Stmt::FunctionDecl(function) | Stmt::ModuleDefaultFunction { function, .. } => {
            !search.looks_for_arguments() && function_contains_super(function, search)
        }
        Stmt::ClassDecl(class) => {
            search.looks_for_arguments() && class_contains_arguments(class, search)
        }
        Stmt::ClassField(statement) => stmt_contains_super(statement, search),
        Stmt::ClassDecoratedField { field, .. } => stmt_contains_super(field, search),
        Stmt::ClassPrivateBrand(_) | Stmt::ClassExtraInitializers(_) => false,
    }
}

fn for_init_contains_super(init: &ForInit, search: SuperSearch) -> bool {
    match init {
        ForInit::Expr(expr) => expr_contains_super(expr, search),
        ForInit::VarDecl(_, declarations) => declarations.iter().any(|declaration| {
            pattern_contains_super(&declaration.pattern, search)
                || declaration
                    .init
                    .as_ref()
                    .is_some_and(|expr| expr_contains_super(expr, search))
        }),
    }
}

fn for_head_contains_super(head: &ForHead, search: SuperSearch) -> bool {
    match head {
        ForHead::Decl(_, pattern) => pattern_contains_super(pattern, search),
        ForHead::AnnexBVarInit(pattern, initializer) => {
            pattern_contains_super(pattern, search) || expr_contains_super(initializer, search)
        }
        ForHead::Assignment(pattern) => assignment_pattern_contains_super(pattern, search),
        ForHead::Expr(expr) => expr_contains_super(expr, search),
    }
}

fn function_contains_super(function: &Function, search: SuperSearch) -> bool {
    if search.looks_for_arguments() || search == SuperSearch::DirectEval {
        return false;
    }
    function.params.iter().any(|param| {
        pattern_contains_super(&param.pattern, search)
            || param
                .default
                .as_ref()
                .is_some_and(|expr| expr_contains_super(expr, search))
    }) || stmts_contain_super(&function.body, search)
}

fn pattern_option_contains_super(pattern: Option<&Pattern>, search: SuperSearch) -> bool {
    pattern.is_some_and(|pattern| pattern_contains_super(pattern, search))
}

fn pattern_contains_super(pattern: &Pattern, search: SuperSearch) -> bool {
    match pattern {
        Pattern::Identifier(_) => false,
        Pattern::Array(elements) => elements.iter().flatten().any(|element| {
            pattern_contains_super(&element.pattern, search)
                || element
                    .default
                    .as_ref()
                    .is_some_and(|expr| expr_contains_super(expr, search))
        }),
        Pattern::Object(properties) => {
            for property in properties {
                let contains_super = match property {
                    ObjectPatternProp::KeyValue {
                        key,
                        value,
                        default,
                    } => {
                        property_key_contains_super(key, search)
                            || pattern_contains_super(value, search)
                            || default
                                .as_ref()
                                .is_some_and(|expr| expr_contains_super(expr, search))
                    }
                    ObjectPatternProp::Rest(pattern) => pattern_contains_super(pattern, search),
                };
                if contains_super {
                    return true;
                }
            }
            false
        }
    }
}

fn assignment_pattern_contains_super(pattern: &AssignmentPattern, search: SuperSearch) -> bool {
    match pattern {
        AssignmentPattern::Target(expr) => expr_contains_super(expr, search),
        AssignmentPattern::Array(elements) => elements.iter().flatten().any(|element| {
            assignment_pattern_contains_super(&element.pattern, search)
                || element
                    .default
                    .as_ref()
                    .is_some_and(|expr| expr_contains_super(expr, search))
        }),
        AssignmentPattern::Object(properties) => properties.iter().any(|property| match property {
            AssignmentPatternProp::KeyValue {
                key,
                value,
                default,
            } => {
                property_key_contains_super(key, search)
                    || assignment_pattern_contains_super(value, search)
                    || default
                        .as_ref()
                        .is_some_and(|expr| expr_contains_super(expr, search))
            }
            AssignmentPatternProp::Rest(pattern) => {
                assignment_pattern_contains_super(pattern, search)
            }
        }),
    }
}

fn property_key_contains_super(key: &PropertyKey, search: SuperSearch) -> bool {
    matches!(key, PropertyKey::Computed(expr) if expr_contains_super(expr, search))
}

fn expr_contains_super(expr: &Expr, search: SuperSearch) -> bool {
    match expr {
        Expr::Number(_)
        | Expr::BigInt(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::This
        | Expr::RegExp { .. }
        | Expr::Super
        | Expr::NewTarget
        | Expr::ImportMeta => false,
        Expr::Identifier(name) => search.looks_for_arguments() && name == "arguments",
        Expr::Parenthesized(expr) => expr_contains_super(expr, search),
        Expr::Template { expressions, .. } => expressions
            .iter()
            .any(|expr| expr_contains_super(expr, search)),
        Expr::TaggedTemplate {
            tag, expressions, ..
        } => {
            expr_contains_super(tag, search)
                || expressions
                    .iter()
                    .any(|expr| expr_contains_super(expr, search))
        }
        Expr::Array(elements) => elements.iter().flatten().any(|element| match element {
            ArrayElement::Normal(expr) | ArrayElement::Spread(expr) => {
                expr_contains_super(expr, search)
            }
        }),
        Expr::Object(properties) => properties.iter().any(|property| match property {
            ObjectProp::KeyValue { key, value, .. } => {
                property_key_contains_super(key, search) || expr_contains_super(value, search)
            }
            ObjectProp::Spread(expr) => expr_contains_super(expr, search),
            ObjectProp::Method { key, function } | ObjectProp::Accessor { key, function, .. } => {
                property_key_contains_super(key, search)
                    || (search == SuperSearch::Call && function_contains_super(function, search))
            }
        }),
        Expr::Function(function) => function_contains_super(function, search),
        Expr::Class(class) => {
            search.looks_for_arguments() && class_contains_arguments(class, search)
        }
        Expr::Yield { value, .. } => value
            .as_deref()
            .is_some_and(|expr| expr_contains_super(expr, search)),
        Expr::Await(expr) | Expr::Unary { arg: expr, .. } | Expr::Update { arg: expr, .. } => {
            expr_contains_super(expr, search)
        }
        Expr::DynamicImport {
            specifier, options, ..
        } => {
            expr_contains_super(specifier, search)
                || options
                    .as_deref()
                    .is_some_and(|expr| expr_contains_super(expr, search))
        }
        // An arrow function's parameters and body are evaluated in its own
        // call, so a direct eval there is not this function's.
        Expr::Arrow { .. } if search == SuperSearch::DirectEval => false,
        Expr::Arrow { params, body, .. } => {
            params.iter().any(|param| {
                pattern_contains_super(&param.pattern, search)
                    || param
                        .default
                        .as_ref()
                        .is_some_and(|expr| expr_contains_super(expr, search))
            }) || match body {
                ArrowBody::Expr(expr) => expr_contains_super(expr, search),
                ArrowBody::Block(statements) => stmts_contain_super(statements, search),
            }
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            expr_contains_super(left, search) || expr_contains_super(right, search)
        }
        Expr::Sequence(expressions) => expressions
            .iter()
            .any(|expr| expr_contains_super(expr, search)),
        Expr::Assign { target, value, .. } => {
            expr_contains_super(target, search) || expr_contains_super(value, search)
        }
        Expr::DestructureAssign { pattern, value } => {
            assignment_pattern_contains_super(pattern, search) || expr_contains_super(value, search)
        }
        Expr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            expr_contains_super(test, search)
                || expr_contains_super(consequent, search)
                || expr_contains_super(alternate, search)
        }
        Expr::Call { callee, args } | Expr::OptionalCall { callee, args } => {
            (search == SuperSearch::Call && matches!(callee.as_ref(), Expr::Super))
                || (matches!(
                    search,
                    SuperSearch::DirectEval | SuperSearch::ArgumentsOrEval
                ) && is_eval_reference(callee))
                || expr_contains_super(callee, search)
                || args.iter().any(|argument| match argument {
                    Argument::Normal(expr) | Argument::Spread(expr) => {
                        expr_contains_super(expr, search)
                    }
                })
        }
        Expr::New { callee, args } => {
            expr_contains_super(callee, search)
                || args.iter().any(|argument| match argument {
                    Argument::Normal(expr) | Argument::Spread(expr) => {
                        expr_contains_super(expr, search)
                    }
                })
        }
        Expr::Member {
            object, property, ..
        } => {
            (search == SuperSearch::Property && matches!(object.as_ref(), Expr::Super))
                || expr_contains_super(object, search)
                || expr_contains_super(property, search)
        }
        Expr::PrivateIn { object, .. } => expr_contains_super(object, search),
        Expr::OptionalMember {
            object, property, ..
        } => expr_contains_super(object, search) || expr_contains_super(property, search),
    }
}

fn class_contains_arguments(class: &Class, search: SuperSearch) -> bool {
    class
        .extends
        .as_deref()
        .is_some_and(|expr| expr_contains_super(expr, search))
        || class
            .decorators
            .iter()
            .any(|expr| expr_contains_super(expr, search))
        || class.elements.iter().any(|element| match element {
            ClassElement::Method {
                key, decorators, ..
            }
            | ClassElement::Accessor {
                key, decorators, ..
            }
            | ClassElement::Field {
                key, decorators, ..
            } => {
                decorators
                    .iter()
                    .any(|expr| expr_contains_super(expr, search))
                    || matches!(key, PropertyKey::Computed(expr) if expr_contains_super(expr, search))
            }
            ClassElement::StaticBlock(_) => false,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_text_is_a_shared_range_that_never_affects_equality() {
        let text: Arc<str> = Arc::from("a \u{3042} function () {} z");
        let range = SourceText::range(&text, 6, 20);
        assert_eq!(range.as_str(), Some("function () {}"));
        assert_eq!(format!("{range:?}"), "SourceText(6..20)");
        // A clone is another handle on the same text, not a copy of it.
        assert_eq!(Arc::strong_count(&text), 2);
        let copy = range.clone();
        assert_eq!(copy.as_str(), Some("function () {}"));
        assert_eq!(Arc::strong_count(&text), 3);
        // No text: the default of every synthesized function.
        assert_eq!(SourceText::default().as_str(), None);
        assert_eq!(format!("{:?}", SourceText::default()), "SourceText(none)");
        // Where a function was written is metadata, not structure.
        assert_eq!(range, SourceText::default());
        assert_eq!(
            Function {
                source_text: range,
                ..Function::default()
            },
            Function::default()
        );
    }

    fn super_member() -> Expr {
        Expr::Member {
            object: Box::new(Expr::Super),
            property: Box::new(Expr::Identifier("value".into())),
            computed: false,
        }
    }

    fn super_call() -> Expr {
        Expr::Call {
            callee: Box::new(Expr::Super),
            args: Vec::new(),
        }
    }

    fn function(body: Vec<Stmt>) -> Function {
        Function {
            name: None,
            params: Vec::new(),
            body,
            generator: false,
            is_async: false,
            source_text: Default::default(),
        }
    }

    #[test]
    fn super_early_error_scanners_visit_all_executable_container_shapes() {
        for body in [
            vec![Stmt::Switch {
                discriminant: Expr::Number(0.0),
                cases: vec![SwitchCase {
                    test: Some(super_member()),
                    consequent: Vec::new(),
                }],
            }],
            vec![Stmt::Switch {
                discriminant: Expr::Number(0.0),
                cases: vec![SwitchCase {
                    test: None,
                    consequent: vec![Stmt::Expr(super_member())],
                }],
            }],
            vec![Stmt::For {
                init: Some(ForInit::Expr(super_member())),
                test: None,
                update: None,
                body: Box::new(Stmt::Empty),
            }],
            vec![Stmt::ClassField(Box::new(Stmt::Expr(super_member())))],
        ] {
            assert!(function_contains_super_property_outside_class(&function(
                body
            )));
        }
        for expression in [
            Expr::Template {
                quasis: vec!["".into()],
                expressions: vec![super_member()],
            },
            Expr::Array(vec![Some(ArrayElement::Normal(super_member()))]),
            Expr::Array(vec![Some(ArrayElement::Spread(super_member()))]),
            Expr::Object(vec![ObjectProp::Spread(super_member())]),
            Expr::DestructureAssign {
                pattern: AssignmentPattern::Target(Box::new(super_member())),
                value: Box::new(Expr::Identifier("source".into())),
            },
            Expr::DestructureAssign {
                pattern: AssignmentPattern::Array(vec![Some(AssignmentPatternElement {
                    pattern: AssignmentPattern::Target(Box::new(Expr::Identifier("target".into()))),
                    default: Some(super_member()),
                    rest: false,
                })]),
                value: Box::new(Expr::Identifier("source".into())),
            },
            Expr::DestructureAssign {
                pattern: AssignmentPattern::Object(vec![AssignmentPatternProp::KeyValue {
                    key: PropertyKey::Computed(Box::new(super_member())),
                    value: AssignmentPattern::Target(Box::new(Expr::Identifier("target".into()))),
                    default: None,
                }]),
                value: Box::new(Expr::Identifier("source".into())),
            },
            Expr::DestructureAssign {
                pattern: AssignmentPattern::Object(vec![AssignmentPatternProp::KeyValue {
                    key: PropertyKey::Identifier("key".into()),
                    value: AssignmentPattern::Target(Box::new(Expr::Identifier("target".into()))),
                    default: Some(super_member()),
                }]),
                value: Box::new(Expr::Identifier("source".into())),
            },
            Expr::DestructureAssign {
                pattern: AssignmentPattern::Object(vec![AssignmentPatternProp::Rest(
                    AssignmentPattern::Target(Box::new(super_member())),
                )]),
                value: Box::new(Expr::Identifier("source".into())),
            },
        ] {
            assert!(expr_contains_super(&expression, SuperSearch::Property));
        }
        assert!(!expr_contains_super(
            &Expr::Class(Class {
                name: None,
                extends: None,
                elements: Vec::new(),
                decorators: Vec::new(),
                source_text: Default::default(),
            }),
            SuperSearch::Property
        ));
        assert!(function_contains_super_property_outside_class(&Function {
            name: None,
            params: vec![Param {
                pattern: Pattern::Identifier("parameter".into()),
                default: Some(super_member()),
                rest: false
            }],
            body: Vec::new(),
            generator: false,
            is_async: false,
            source_text: Default::default(),
        }));
        assert!(contains_super_call_outside_class(&Program {
            body: vec![Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::Super),
                args: Vec::new()
            })]
        }));
        assert!(expr_contains_super_call_outside_class(&Expr::New {
            callee: Box::new(Expr::Identifier("C".into())),
            args: vec![Argument::Normal(Expr::Call {
                callee: Box::new(Expr::Super),
                args: Vec::new()
            })]
        }));
    }

    #[test]
    fn super_call_scanner_visits_nested_control_and_pattern_shapes() {
        assert!(statements_contain_super_call_outside_class(&[Stmt::Expr(
            super_call()
        )]));
        let binding = Pattern::Array(vec![Some(ArrayPatternElement {
            pattern: Pattern::Identifier("value".into()),
            default: Some(super_call()),
            rest: false,
        })]);
        assert!(for_head_contains_super(
            &ForHead::Decl(DeclKind::Let, binding.clone()),
            SuperSearch::Call
        ));
        assert!(pattern_option_contains_super(
            Some(&binding),
            SuperSearch::Call
        ));
        let object_binding = Pattern::Object(vec![ObjectPatternProp::KeyValue {
            key: PropertyKey::Identifier("value".into()),
            value: Pattern::Identifier("target".into()),
            default: Some(super_call()),
        }]);
        assert!(pattern_contains_super(&object_binding, SuperSearch::Call));
        let computed_object_binding = Pattern::Object(vec![ObjectPatternProp::KeyValue {
            key: PropertyKey::Computed(Box::new(super_member())),
            value: Pattern::Identifier("target".into()),
            default: None,
        }]);
        assert!(pattern_contains_super(
            &computed_object_binding,
            SuperSearch::Property
        ));
        assert!(pattern_contains_super(
            &Pattern::Object(vec![ObjectPatternProp::Rest(Pattern::Array(vec![Some(
                ArrayPatternElement {
                    pattern: Pattern::Identifier("target".into()),
                    default: Some(super_call()),
                    rest: false,
                }
            )]))]),
            SuperSearch::Call,
        ));
        assert!(!pattern_contains_super(
            &Pattern::Object(vec![ObjectPatternProp::KeyValue {
                key: PropertyKey::Identifier("value".into()),
                value: Pattern::Identifier("target".into()),
                default: None,
            }]),
            SuperSearch::Call,
        ));
        assert!(assignment_pattern_contains_super(
            &AssignmentPattern::Array(vec![Some(AssignmentPatternElement {
                pattern: AssignmentPattern::Target(Box::new(Expr::Identifier("value".into()))),
                default: Some(super_call()),
                rest: false,
            })]),
            SuperSearch::Call,
        ));
        assert!(assignment_pattern_contains_super(
            &AssignmentPattern::Object(vec![AssignmentPatternProp::KeyValue {
                key: PropertyKey::Identifier("value".into()),
                value: AssignmentPattern::Target(Box::new(Expr::Identifier("target".into()))),
                default: Some(super_call()),
            }]),
            SuperSearch::Call,
        ));
        assert!(assignment_pattern_contains_super(
            &AssignmentPattern::Object(vec![AssignmentPatternProp::Rest(
                AssignmentPattern::Target(Box::new(super_call()))
            )]),
            SuperSearch::Call,
        ));

        for statement in [
            Stmt::VarDecl(
                DeclKind::Let,
                vec![VarDeclarator {
                    pattern: Pattern::Identifier("value".into()),
                    init: Some(super_call()),
                }],
            ),
            Stmt::If {
                test: Expr::Bool(false),
                consequent: Box::new(Stmt::Empty),
                alternate: Some(Box::new(Stmt::Expr(super_call()))),
            },
            Stmt::For {
                init: Some(ForInit::VarDecl(
                    DeclKind::Let,
                    vec![VarDeclarator {
                        pattern: Pattern::Identifier("value".into()),
                        init: Some(super_call()),
                    }],
                )),
                test: None,
                update: None,
                body: Box::new(Stmt::Empty),
            },
            Stmt::For {
                init: None,
                test: Some(super_call()),
                update: None,
                body: Box::new(Stmt::Empty),
            },
            Stmt::For {
                init: None,
                test: Some(Expr::Bool(false)),
                update: Some(super_call()),
                body: Box::new(Stmt::Empty),
            },
            Stmt::ForIn {
                left: ForHead::Assignment(AssignmentPattern::Target(Box::new(Expr::Identifier(
                    "value".into(),
                )))),
                right: super_call(),
                body: Box::new(Stmt::Empty),
            },
            Stmt::ForOf {
                left: ForHead::Assignment(AssignmentPattern::Target(Box::new(Expr::Identifier(
                    "value".into(),
                )))),
                right: Expr::Array(Vec::new()),
                body: Box::new(Stmt::Expr(super_call())),
                is_await: false,
            },
            Stmt::Switch {
                discriminant: Expr::Number(0.0),
                cases: vec![SwitchCase {
                    test: Some(super_call()),
                    consequent: Vec::new(),
                }],
            },
            Stmt::Switch {
                discriminant: Expr::Number(0.0),
                cases: vec![SwitchCase {
                    test: Some(Expr::Number(1.0)),
                    consequent: vec![Stmt::Expr(super_call())],
                }],
            },
            Stmt::Try {
                block: Vec::new(),
                handler: Some(CatchClause {
                    param: Some(binding),
                    body: Vec::new(),
                }),
                finalizer: None,
            },
            Stmt::Try {
                block: Vec::new(),
                handler: Some(CatchClause {
                    param: None,
                    body: vec![Stmt::Expr(super_call())],
                }),
                finalizer: None,
            },
            Stmt::Try {
                block: Vec::new(),
                handler: None,
                finalizer: Some(vec![Stmt::Expr(super_call())]),
            },
            Stmt::Block(vec![Stmt::Expr(super_call())]),
            Stmt::ClassField(Box::new(Stmt::Expr(super_call()))),
            Stmt::For {
                init: Some(ForInit::Expr(super_call())),
                test: None,
                update: None,
                body: Box::new(Stmt::Empty),
            },
            Stmt::For {
                init: None,
                test: None,
                update: None,
                body: Box::new(Stmt::Expr(super_call())),
            },
            Stmt::While {
                test: Expr::Bool(false),
                body: Box::new(Stmt::Expr(super_call())),
            },
            Stmt::DoWhile {
                body: Box::new(Stmt::Expr(super_call())),
                test: Expr::Bool(false),
            },
            Stmt::With {
                object: Expr::Bool(true),
                body: Box::new(Stmt::Expr(super_call())),
            },
            Stmt::With {
                object: super_call(),
                body: Box::new(Stmt::Empty),
            },
            Stmt::FunctionDecl(function(vec![Stmt::Expr(super_call())])),
        ] {
            assert!(
                stmt_contains_super(&statement, SuperSearch::Call),
                "{statement:?}"
            );
        }

        for expression in [
            Expr::Template {
                quasis: vec!["".into()],
                expressions: vec![super_call()],
            },
            Expr::TaggedTemplate {
                tag: Box::new(Expr::Identifier("tag".into())),
                raw: vec!["".into()],
                cooked: vec![Some("".into())],
                expressions: vec![super_call()],
            },
            Expr::Yield {
                value: Some(Box::new(super_call())),
                delegate: false,
            },
            Expr::Arrow {
                params: vec![Param {
                    pattern: Pattern::Identifier("value".into()),
                    default: Some(super_call()),
                    rest: false,
                }],
                body: ArrowBody::Expr(Box::new(Expr::Number(0.0))),
                is_async: false,
                source_text: Default::default(),
            },
            Expr::Sequence(vec![super_call()]),
            Expr::Call {
                callee: Box::new(Expr::Identifier("call".into())),
                args: vec![
                    Argument::Normal(super_call()),
                    Argument::Spread(super_call()),
                ],
            },
            Expr::New {
                callee: Box::new(Expr::Identifier("Constructor".into())),
                args: vec![
                    Argument::Normal(super_call()),
                    Argument::Spread(super_call()),
                ],
            },
            Expr::Object(vec![ObjectProp::KeyValue {
                key: PropertyKey::Identifier("value".into()),
                value: super_call(),
                shorthand: false,
            }]),
            Expr::Object(vec![ObjectProp::Spread(super_call())]),
            Expr::Object(vec![ObjectProp::Method {
                key: PropertyKey::Identifier("method".into()),
                function: function(vec![Stmt::Expr(super_call())]),
            }]),
            Expr::Object(vec![ObjectProp::Accessor {
                key: PropertyKey::Identifier("value".into()),
                function: function(vec![Stmt::Expr(super_call())]),
                getter: true,
            }]),
            Expr::Function(function(vec![Stmt::Expr(super_call())])),
            Expr::Await(Box::new(super_call())),
            Expr::Unary {
                op: UnaryOp::Void,
                arg: Box::new(super_call()),
            },
            Expr::Update {
                op: UpdateOp::Inc,
                arg: Box::new(super_call()),
                prefix: true,
            },
            Expr::Arrow {
                params: Vec::new(),
                body: ArrowBody::Expr(Box::new(super_call())),
                is_async: false,
                source_text: Default::default(),
            },
            Expr::Arrow {
                params: Vec::new(),
                body: ArrowBody::Block(vec![Stmt::Expr(super_call())]),
                is_async: false,
                source_text: Default::default(),
            },
        ] {
            assert!(
                expr_contains_super_call_outside_class(&expression),
                "{expression:?}"
            );
        }
        assert!(!expr_contains_super_call_outside_class(&Expr::Class(
            Class {
                name: None,
                extends: None,
                elements: Vec::new(),
                decorators: Vec::new(),
                source_text: Default::default(),
            }
        )));
        let property = Expr::Member {
            object: Box::new(Expr::Identifier("object".into())),
            property: Box::new(super_member()),
            computed: true,
        };
        assert!(expr_contains_super(&property, SuperSearch::Property));
    }
}
