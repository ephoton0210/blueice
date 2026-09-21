// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compiler-owned bytecode: one byte per opcode plus a fixed u32 operand
//! where needed. The table defines decoding, width and tiering metadata
//! together; VM dispatch is an exhaustive match on its generated enum.
//! Jumps use byte offsets. No public deserializer accepts arbitrary code:
//! serialization/versioning and hostile-bytecode validation are future work.

use crate::{ModuleType, Value};

/// Reserved opcode metadata for later inline-cache tiers. No cache is
/// allocated or consulted by the first interpreter.
pub const MAY_USE_INLINE_CACHE: u8 = 1;

macro_rules! opcodes {
    ($($name:ident: $width:literal, $flags:expr;)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        pub enum Opcode { $($name,)* }

        impl Opcode {
            pub fn width(self) -> usize { match self { $(Self::$name => $width,)* } }
            pub fn flags(self) -> u8 { match self { $(Self::$name => $flags,)* } }
            fn decode(byte: u8) -> Option<Self> {
                match byte { $(n if n == Self::$name as u8 => Some(Self::$name),)* _ => None }
            }
        }
    };
}

opcodes! {
    Constant: 5, 0;
    GetBinding: 5, 0;
    InitializeBinding: 5, 0;
    StoreBinding: 5, 0;
    UnboundName: 5, 0;
    SetUnboundName: 5, 0;
    DeleteUnboundName: 5, 0;
    // Strict `name = value` for a name that no binding resolves: resolves the
    // reference before the right-hand side runs (pushes whether it resolved),
    // and SetResolvedUnboundName stores through it (stack: flag, value).
    ResolveUnboundName: 5, 0;
    SetResolvedUnboundName: 5, 0;
    // `delete name` inside `with`: deletes the property of the innermost with
    // object that has the binding and pushes the result, or pushes `undefined`
    // when no with object has it (the caller then falls back to the binding).
    DeleteWithBinding: 5, 0;
    DeleteDynamicBinding: 5, 0;
    EnterScope: 5, 0;
    CloneScope: 5, 0;
    LeaveScope: 5, 0;
    Pop: 1, 0;
    Dup: 1, 0;
    Dup2: 1, 0;
    Swap: 1, 0;
    Add: 1, 0;
    Subtract: 1, 0;
    Multiply: 1, 0;
    Exponentiate: 1, 0;
    Divide: 1, 0;
    Remainder: 1, 0;
    ShiftLeft: 1, 0;
    ShiftRight: 1, 0;
    UnsignedShiftRight: 1, 0;
    BitAnd: 1, 0;
    BitXor: 1, 0;
    BitOr: 1, 0;
    StrictEqual: 1, 0;
    StrictNotEqual: 1, 0;
    Equal: 1, 0;
    NotEqual: 1, 0;
    Less: 1, 0;
    Greater: 1, 0;
    LessEqual: 1, 0;
    GreaterEqual: 1, 0;
    Negate: 1, 0;
    BitNot: 1, 0;
    ToNumber: 1, 0;
    // ToNumeric: like ToNumber, but a BigInt operand passes through
    // unchanged instead of throwing. Used by `++`/`--` on a plain
    // identifier, whose result must stay a BigInt when its operand is one.
    ToNumeric: 1, 0;
    // Pushes `1` matching the numeric type already on top of the stack
    // (Number `1.0` or BigInt `1n`), so the following Add/Subtract never
    // mixes BigInt with Number. Always immediately preceded by ToNumeric
    // (possibly through a Dup), so the peeked type is always Number or
    // BigInt.
    PushOne: 1, 0;
    ToString: 1, 0;
    Not: 1, 0;
    Typeof: 1, 0;
    Jump: 5, 0;
    JumpIfFalse: 5, 0;
    JumpIfTrue: 5, 0;
    JumpIfNotNullish: 5, 0;
    NewObject: 1, 0;
    GetProperty: 1, MAY_USE_INLINE_CACHE;
    SetProperty: 1, MAY_USE_INLINE_CACHE;
    SetDestructureProperty: 1, MAY_USE_INLINE_CACHE;
    SetDestructurePropertyReference: 1, MAY_USE_INLINE_CACHE;
    UpdateProperty: 5, MAY_USE_INLINE_CACHE;
    SetLiteralPrototype: 1, 0;
    SetCompletion: 1, 0;
    ClearCompletion: 1, 0;
    Halt: 1, 0;
    NewArray: 5, 0;
    GlobalString: 1, 0;
    GetMethod: 1, MAY_USE_INLINE_CACHE;
    Call: 5, 0;
    // `return callee(args)` in tail position (§15.10.2). Same stack layout as
    // Call. Operand: argument count << 1, plus 1 when the callee is spelled
    // `eval` (a direct eval candidate). The frame is replaced by the callee's
    // when the current call can be replaced; otherwise it is an ordinary call.
    TailCall: 5, 0;
    DirectEval: 5, 0;
    Construct: 5, 0;
    Closure: 5, 0;
    This: 1, 0;
    NewTarget: 1, 0;
    Argument: 5, 0;
    RestArguments: 5, 0;
    ArgumentsObject: 1, 0;
    Return: 1, 0;
    TailRecur: 5, 0;
    Yield: 1, 0;
    Await: 1, 0;
    DynamicImport: 5, 0;
    ImportMeta: 1, 0;
    EnterWith: 1, 0;
    LeaveWith: 1, 0;
    WithGet: 5, 0;
    WithGetOrUndefined: 5, 0;
    // Callee of `name(...)` inside `with`: pushes the function and its `this`
    // (the with object the name was found on, else undefined).
    WithGetMethod: 5, 0;
    WithSet: 5, 0;
    ResolveWithReference: 5, 0;
    LoadWithReference: 1, 0;
    StoreWithReference: 1, 0;
    // Like StoreWithReference, for a reference resolved *after* the value:
    // stack `value, target, marker`. Leaves the value.
    StoreResolvedWithReference: 1, 0;
    // `name++` etc. on a `ResolveWithReference` pair. Operand bit 0:
    // decrement; bit 1: prefix.
    UpdateWithReference: 5, 0;
    // The parameter list has been evaluated: later direct evals declare their
    // `var`s in the function body's own environment again.
    EndParameterEvalScope: 1, 0;
    Global: 5, 0;
    ToPropertyKey: 1, 0;
    PreparePropertyReference: 1, MAY_USE_INLINE_CACHE;
    GetIterator: 1, 0;
    IteratorNext: 5, 0;
    IteratorStepValue: 5, 0;
    GetAsyncIterator: 1, 0;
    ForInKeys: 1, 0;
    IteratorStep: 5, 0;
    AsyncIteratorNext: 5, 0;
    AsyncIteratorStep: 5, 0;
    AsyncIteratorStepValue: 5, 0;
    IteratorStepReference: 5, 0;
    IteratorElision: 1, 0;
    IteratorClose: 1, 0;
    // Control-transfer cleanup may resume after a handler already closed the
    // iterator and unwound the scope that held this compiler-private slot.
    CloseIteratorBinding: 5, 0;
    IteratorFinish: 1, 0;
    IteratorRest: 1, 0;
    IteratorRestReference: 1, 0;
    RequireObject: 1, 0;
    DestructureProperty: 1, 0;
    DestructurePropertyReference: 1, 0;
    ObjectRest: 1, 0;
    CopyDataProperties: 1, 0;
    RegExpLiteral: 1, 0;
    TemplateObject: 5, 0;
    ArrayPush: 5, 0;
    CallSpread: 5, 0;
    DirectEvalSpread: 1, 0;
    Throw: 1, 0;
    InvalidAssignmentTarget: 1, 0;
    PushHandler: 5, 0;
    PopHandler: 1, 0;
    ResumeCompletion: 5, 0;
    SaveCompletion: 1, 0;
    // Explicit Resource Management: `MarkDisposables` records the current
    // depth of the VM's disposable-resource stack when a `using`-declaring
    // block/function body is entered; `AddDisposableResource` (operand 0 =
    // sync-dispose, 1 = async-dispose) pops an initialized `using` binding's
    // value and appends its disposal record; `DisposeResources` drains back
    // down to the last mark, disposing in reverse order and merging a
    // disposal error with any already-pending completion as a
    // `SuppressedError`. Always compiled as a matched Mark/Dispose pair
    // around a synthetic try/finally (see `statements_with_disposal`), so
    // depths never need cross-checking at runtime.
    MarkDisposables: 1, 0;
    AddDisposableResource: 5, 0;
    // `await using`-capable disposal: drains this block's disposable-resource
    // stack (the same runtime state `DisposeResources` drains) into a plain
    // JS value `[hasError, pendingError, entries]` where `entries` is a real
    // Array of `[receiver, method, hasArgument, argument, isAsync,
    // syncFallback]` records, one per resource in declaration order. The
    // compiler then compiles an
    // ordinary (synthesized) `while`/`try`/`catch` loop over that value using
    // its normal statement/expression compiling -- `Await` included -- so a
    // dispose call that needs awaiting uses the same suspend/resume path as
    // any other `await`. See `Compiler::compile_async_dispose_finally`.
    // Operand: same "am I an abrupt or normal entry" handler index as
    // `DisposeResources`.
    DrainAsyncDisposables: 5, 0;
    // Operand: the static index (into `Bytecode::handlers`) of the
    // synthetic try/finally this disposal is the finally clause of. Lets
    // the interpreter tell an abrupt entry (the handler frame is still on
    // the runtime handler stack, in `Finally` state, with a pending
    // completion to merge a disposal error into as a `SuppressedError`)
    // apart from a normal-completion entry (the frame was already popped
    // by `PopHandler`, so no prior error can exist to merge with).
    DisposeResources: 5, 0;
    AbruptJump: 5, 0;
    // SetFunctionName from a property key: stack `key, function`, both left
    // in place. Operand: 0 plain, 1 `get ` prefix, 2 `set ` prefix.
    SetFunctionName: 5, 0;
    DefineData: 1, 0;
    DefineAccessor: 5, 0;
    // Operand: non-zero for an object-literal method (enumerable), zero for a
    // class method.
    DefineMethod: 5, 0;
    DefineClassAccessor: 5, 0;
    DefineInstanceField: 1, 0;
    // `receiver, name, value` -> nothing: PrivateFieldAdd for the private
    // name whose owner is in the operand's slot.
    PrivateFieldAdd: 5, 0;
    DefinePrivateField: 5, 0;
    DefinePrivateMethod: 5, 0;
    DefinePrivateAccessor: 5, 0;
    CallClassStaticBlock: 1, 0;
    SetClassHome: 1, 0;
    SetClassHeritage: 1, 0;
    // `F, initializer` -> `F`: install the class's [[Fields]] (a method-like
    // function run with the new instance as `this`) and give it the class
    // prototype as its home object.
    SetClassFields: 1, 0;
    // Pops the receiver to brand with the owner in the operand's slot.
    InitializePrivateBrand: 5, 0;
    PrivateGet: 5, MAY_USE_INLINE_CACHE;
    PrivateGetMethod: 5, MAY_USE_INLINE_CACHE;
    PrivateSet: 5, MAY_USE_INLINE_CACHE;
    // A destructuring leaf's PrivateSet: `value, receiver, name` -> `value`.
    PrivateSetLeaf: 5, 0;
    PrivateIn: 5, MAY_USE_INLINE_CACHE;
    // A super property Reference is the operand pair `base, key` (like any
    // other property Reference) with the `this` value pushed on top just
    // before its consuming opcode: `SuperBase` (below) captures the base once
    // the key expression has been evaluated, and `this` is re-read (it can
    // only ever go from uninitialized to a fixed value) rather than stored.
    // `SuperGet`: `base, key, this` -> value. `SuperGetMethod`: `base, key,
    // this` -> function, this. `SuperSet`: operand 0 takes `base, key, value,
    // this`, operand 1 takes the destructuring-leaf order `value, base, key,
    // this`; both leave the value. `SuperUpdate`: `base, key, this` -> the
    // old or new number (operand bit 0 decrement, bit 1 prefix).
    SuperGet: 1, MAY_USE_INLINE_CACHE;
    SuperGetMethod: 1, MAY_USE_INLINE_CACHE;
    SuperSet: 5, MAY_USE_INLINE_CACHE;
    SuperUpdate: 5, MAY_USE_INLINE_CACHE;
    // Pushes GetSuperBase, the home object's [[Prototype]] (an object or
    // null), above the already-evaluated key.
    SuperBase: 1, 0;
    // Pops any evaluated key and throws the ReferenceError of `delete
    // super.x`.
    DeleteSuperProperty: 1, 0;
    // Pushes a derived constructor's (or one of its arrows') `this` binding
    // held in the operand's hidden slot; a ReferenceError until `super()`
    // has bound it.
    ThisBinding: 5, 0;
    // `F` -> `F.[[GetPrototypeOf]]()`, GetSuperConstructor for the active
    // function `F`; evaluated before the `super()` arguments.
    SuperConstructor: 1, 0;
    // `F, superCtor, arg...` -> `F, result`: the IsConstructor check that follows
    // argument evaluation, then Construct(superCtor, args, new.target). The
    // operand is the argument count (`SuperCallSpread` takes one argument
    // array, `SuperCallForward` the frame's own arguments).
    SuperCall: 5, 0;
    SuperCallSpread: 1, 0;
    SuperCallForward: 1, 0;
    // Peeks the constructed value and binds it as the operand slot's `this`;
    // a second bind is a ReferenceError.
    BindThisValue: 5, 0;
    // `F, result` -> `result` after InitializeInstanceElements(result, F).
    InitializeInstanceElements: 1, 0;
    // Decorators. `F` -> `F, metadata`: a fresh metadata object whose
    // prototype is the superclass's `Symbol.metadata` (or null).
    CreateMetadata: 1, 0;
    // `class, metadata` -> nothing: defines `class[Symbol.metadata]`.
    DefineMetadata: 1, 0;
    // `list, decorator, receiver` -> `list`: appends the value of one decorator
    // expression and the `this` value it is called with to a decorator list.
    PushDecorator: 1, 0;
    // `F, receiver, function` -> `F`: runs a static field's or static block's
    // function with `this` = receiver (the decorated class) while its home
    // object stays F.
    CallDecoratedStaticElement: 1, 0;
    // `decorators, name, owner, privateName, value, value2, metadata` ->
    // `record`: applies one class element's decorators, last to first, and
    // returns `[extraInitializers, ...]` (the operand encodes the element
    // kind, `static` and `private`; see `vm/builtins/decorators.rs`).
    DecorateElement: 5, 0;
    // `F, decorators, name, metadata` -> `[extraInitializers, F']`.
    DecorateClass: 1, 0;
    // `target, key, original, replacement` -> nothing: puts a decorated
    // method or accessor half back on its class (or private owner) unless a
    // later element already replaced it. The operand encodes the kind and
    // whether the name is private.
    ReplaceClassElement: 5, 0;
    // `receiver, initializers` -> nothing: calls each with `this` = receiver.
    RunInitializers: 1, 0;
    // `value, receiver, initializers` -> `value'`: threads a field's initial
    // value through each initializer with `this` = receiver.
    ApplyInitializers: 1, 0;
    DeleteProperty: 1, 0;
    Instanceof: 1, 0;
    In: 1, 0;
    TypeofName: 5, 0;
    // A lexical binding reference is resolved before an assignment's RHS.
    // Direct eval can add a same-named var binding during that RHS, so the
    // reference needs to retain its original target until PutValue.
    ResolveBindingReference: 5, 0;
    LoadBindingReference: 1, 0;
    StoreBindingReference: 5, 0;
    // Drop a retained Reference beneath the assignment's expression value.
    // The operand gives the number of stack values encoding that Reference.
    DiscardReference: 5, 0;
}

/// The operand encoding shared by `DecorateElement` and `ReplaceClassElement`:
/// bits 0-2 are the element kind, bit 3 is `static` and bit 4 is `private`.
pub(crate) mod decoration {
    pub const METHOD: u32 = 0;
    pub const GETTER: u32 = 1;
    pub const SETTER: u32 = 2;
    pub const FIELD: u32 = 3;
    pub const ACCESSOR: u32 = 4;
    pub const KIND_MASK: u32 = 7;
    pub const STATIC: u32 = 8;
    pub const PRIVATE: u32 = 16;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    pub offset: usize,
    pub opcode: Opcode,
    pub operand: Option<u32>,
}

#[derive(Clone)]
pub(crate) struct Binding {
    pub name: String,
    pub mutable: bool,
    /// Immutable lexical bindings created by `const` reject every write.
    /// A named function expression instead has an immutable but non-strict
    /// binding: sloppy references ignore its writes while strict references
    /// throw.
    pub strict_immutable: bool,
    pub lexical: bool,
    /// Annex B lets a direct eval in the immediately containing catch block
    /// redeclare a simple catch parameter with `var` or a function.
    pub catch_parameter: bool,
}

/// A linked import's local slot.  The VM replaces that slot's cell with the
/// resolved exporter cell before module initialization, preserving the live
/// binding rather than copying a value.
#[derive(Clone)]
pub(crate) enum ModuleImportName {
    Named(String),
    Namespace,
    DeferredNamespace,
    Source,
}

#[derive(Clone)]
pub(crate) struct ModuleImport {
    pub module_request: String,
    pub import_name: ModuleImportName,
    pub local_slot: Option<u32>,
    /// This request's `with { type }` attribute. Drives the synthetic
    /// module (JSON, text, bytes) routing instead of ordinary Source Text
    /// Module linking; other attribute keys/values are accepted but not
    /// otherwise acted on (see `parser/module_items.rs`).
    pub module_type: ModuleType,
}

/// One executable [[RequestedModules]] entry, in source-text order.
/// Source-phase records resolve during linking but do not participate in
/// module evaluation, so they are omitted from this sequence. A deferred
/// request (`import defer * as ns from`) contributes only its asynchronous
/// transitive dependencies to evaluation, never the module itself.
#[derive(Clone)]
pub(crate) struct ModuleRequest {
    pub module_request: String,
    pub module_type: ModuleType,
    pub deferred: bool,
}

#[derive(Clone)]
pub(crate) enum ModuleExport {
    Local {
        export_name: String,
        local_slot: u32,
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
    /// A re-export of a deferred namespace import (`import defer * as ns`
    /// then `export { ns }`): the export resolves to the target's deferred
    /// namespace object rather than a lexical cell.
    DeferredNamespace {
        export_name: String,
        module_request: String,
        module_type: ModuleType,
    },
    /// A local re-export of a source-phase import. It resolves to the
    /// source record's Module Source Object rather than a lexical cell.
    Source {
        export_name: String,
        module_request: String,
    },
}

#[derive(Clone)]
pub(crate) struct TemplateSite {
    pub id: u64,
    pub raw: Vec<crate::JsString>,
    pub cooked: Vec<Option<crate::JsString>>,
}

/// Static destinations for one `try` statement. Runtime handler frames add
/// the operand-stack, lexical-scope and iterator depths needed to restore the
/// execution state on an abrupt completion.
#[derive(Clone)]
pub(crate) struct Handler {
    pub try_start: u32,
    pub try_end: u32,
    pub catch: Option<u32>,
    pub catch_end: Option<u32>,
    pub finally: Option<u32>,
    /// One past the finalizer's closing `ResumeCompletion`.
    pub finally_end: Option<u32>,
}

/// One control-transfer continuation. `cleanup` runs after every enclosing
/// finalizer; `target` is the eventual destination used to decide which
/// enclosing handlers the transfer actually leaves.
#[derive(Clone)]
pub(crate) struct AbruptJump {
    pub cleanup: u32,
    pub target: u32,
}

/// Read-only compiled code plus its constant pool and binding/scope
/// metadata. Can execute repeatedly in the same or independent VMs;
/// runtime object handles are never stored in its constant pool.
#[derive(Clone)]
pub struct Bytecode {
    pub(crate) code: Vec<u8>,
    /// Compiler-recorded instruction starts for the root program's source
    /// order statements. `None` means the statement emits no root-code-unit
    /// instruction and therefore has no executable safe point.
    pub(crate) root_statement_offsets: Vec<Option<u32>>,
    pub(crate) constants: Vec<Value>,
    pub(crate) bindings: Vec<Binding>,
    pub(crate) scopes: Vec<Vec<u32>>,
    /// Names declared by top-level function declarations. Global declaration
    /// instantiation treats these differently from `var` declarations.
    pub(crate) global_function_names: Vec<String>,
    /// Slots introduced by a sloppy direct eval's VariableEnvironment. They
    /// are backed by the caller frame's dynamic binding table rather than by
    /// ordinary lexical slots, and remain deletable after eval returns.
    pub(crate) dynamic_eval_slots: Vec<u32>,
    /// Scope holding a function's VariableEnvironment. Non-simple parameter
    /// lists use a preceding parameter scope, which direct eval must inspect
    /// for lexical conflicts before reaching this scope.
    pub(crate) variable_scope: u32,
    /// Generator functions execute their parameter/instantiation prefix when
    /// called, then suspend at this bytecode offset until `.next()` begins
    /// evaluating the body.
    pub(crate) generator_entry: u32,
    pub(crate) generator_initializes_parameters: bool,
    /// Whether direct eval may read an enclosing function's `new.target`.
    pub(crate) new_target_allowed: bool,
    /// Whether this code was parsed under the Module goal, including a nested
    /// function whose own bytecode is not a module record.
    pub(crate) import_meta_allowed: bool,
    /// How many enclosing `with` statements this function was created inside.
    /// A closure over such a function captures the with objects that are
    /// active when it is created.
    pub(crate) with_depth: u32,
    /// A sloppy function whose parameter list contains a direct eval: its
    /// calls get an environment of their own, outside the parameters, for the
    /// `var`s such an eval declares (see `Vm::call_closure`).
    pub(crate) parameter_eval_scope: bool,
    pub(crate) functions: Vec<std::rc::Rc<Bytecode>>,
    pub(crate) captures: Vec<u32>,
    /// The immutable name environment binding of a named function expression.
    /// It is initialized to the closure object when that closure is called.
    pub(crate) self_slot: Option<u32>,
    pub(crate) function_name: String,
    pub(crate) function_length: u32,
    /// The per-invocation `arguments` binding of a non-arrow function.  The
    /// interpreter creates it after its function environment has entered.
    pub(crate) arguments_slot: Option<u32>,
    /// For a non-strict simple parameter list, the formal binding cell mapped
    /// by each arguments index. `None` means either an unmapped arguments
    /// object or a duplicate formal shadowed by a later occurrence.
    pub(crate) arguments_mapped_slots: Vec<Option<u32>>,
    pub(crate) arguments_mapped: bool,
    pub(crate) arrow: bool,
    pub(crate) generator: bool,
    /// Async functions require Promise capabilities and job-queue integration
    /// at call time. Keeping the declaration bit in bytecode lets lexical
    /// instantiation remain correct before that execution support exists.
    pub(crate) async_function: bool,
    pub(crate) constructible: bool,
    /// Class constructors require `new`, unlike ordinary constructible
    /// closures. Class methods are non-constructible closures.
    pub(crate) class_constructor: bool,
    /// Derived class constructors receive their `this` binding from a
    /// superclass construction rather than from their own call entry.
    pub(crate) derived_constructor: bool,
    /// The hidden lexical slot holding a derived constructor's `this` binding
    /// (`None` for every other function).
    pub(crate) derived_this_slot: Option<u32>,
    /// This function is a class field initializer: it may not observe an
    /// `arguments` binding, which a direct eval inside it (or inside an arrow
    /// function it creates) must respect.
    pub(crate) class_field_initializer: bool,
    pub(crate) strict: bool,
    pub(crate) templates: Vec<TemplateSite>,
    pub(crate) handlers: Vec<Handler>,
    pub(crate) abrupt_jumps: Vec<AbruptJump>,
    /// Resume and exit offsets for compiler-emitted async `yield*` loops.
    /// The VM copies the matching entry into the suspended generator frame,
    /// so later `.return()` and `.throw()` do not depend on recognizing a
    /// bytecode instruction pattern.
    pub(crate) async_yield_delegates: Vec<(u32, u32)>,
    /// Resume and exit offsets for compiler-emitted synchronous `yield*`
    /// loops. The suspended frame carries the matching record so public
    /// `throw()` and `return()` can forward into the delegate.
    pub(crate) yield_delegates: Vec<(u32, u32)>,
    /// This code was compiled with the Module goal.  Its outer scope may be
    /// suspended after declaration instantiation and resumed for evaluation.
    pub(crate) module: bool,
    /// Byte offset immediately after top-level function instantiation.
    /// `None` for scripts and nested function bytecode.
    pub(crate) module_evaluate_entry: Option<u32>,
    pub(crate) module_imports: Vec<ModuleImport>,
    pub(crate) module_exports: Vec<ModuleExport>,
    pub(crate) module_requests: Vec<ModuleRequest>,
    /// Set only for a host-synthesized module (`ParseJSONModule`,
    /// `CreateTextModule`, `CreateBytesModule`, each ending in
    /// `CreateDefaultExportSyntheticModule`): the value of its sole
    /// `default` export. This is a deliberate, narrow exception to "runtime
    /// object handles are never stored in its constant pool" above -- a
    /// synthetic module's Bytecode is synthesized fresh per-`Vm` by
    /// `vm/modules.rs::ensure_synthetic_module` from host-supplied resource
    /// data, never shared across independent VMs, so a live heap value tied
    /// to this realm is safe to carry here (never in `constants`).
    pub(crate) synthetic_default_export: Option<Value>,
}

impl Bytecode {
    pub(crate) fn empty() -> Self {
        Self {
            code: Vec::new(),
            root_statement_offsets: Vec::new(),
            constants: Vec::new(),
            bindings: Vec::new(),
            scopes: Vec::new(),
            global_function_names: Vec::new(),
            dynamic_eval_slots: Vec::new(),
            variable_scope: 0,
            generator_entry: 0,
            generator_initializes_parameters: false,
            new_target_allowed: false,
            import_meta_allowed: false,
            with_depth: 0,
            parameter_eval_scope: false,
            functions: Vec::new(),
            captures: Vec::new(),
            self_slot: None,
            function_name: String::new(),
            function_length: 0,
            arguments_slot: None,
            arguments_mapped_slots: Vec::new(),
            arguments_mapped: false,
            arrow: false,
            generator: false,
            async_function: false,
            constructible: false,
            class_constructor: false,
            derived_constructor: false,
            derived_this_slot: None,
            class_field_initializer: false,
            strict: false,
            templates: Vec::new(),
            handlers: Vec::new(),
            abrupt_jumps: Vec::new(),
            async_yield_delegates: Vec::new(),
            yield_delegates: Vec::new(),
            module: false,
            module_evaluate_entry: None,
            module_imports: Vec::new(),
            module_exports: Vec::new(),
            module_requests: Vec::new(),
            synthetic_default_export: None,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.code
    }

    /// Root-program statement instruction starts in source order.
    ///
    /// This is compiler-produced provenance metadata, not a heuristic based
    /// on source text or bytecode scanning. A `None` entry is an explicit
    /// unbound result for a statement that contributes no root instruction.
    pub fn root_statement_offsets(&self) -> &[Option<u32>] {
        &self.root_statement_offsets
    }

    pub fn constants(&self) -> &[Value] {
        &self.constants
    }

    pub fn instructions(&self) -> impl Iterator<Item = Instruction> + '_ {
        let mut offset = 0;
        std::iter::from_fn(move || {
            let instruction = self.instruction(offset)?;
            offset += instruction.opcode.width();
            Some(instruction)
        })
    }

    /// Nested executable code units created for local function closures.
    ///
    /// The returned order is the compiler's stable closure-table order. A
    /// host that needs generation-bound code-unit identifiers must traverse
    /// this tree deterministically rather than deriving an identity from a
    /// byte offset or a heap object address.
    pub fn child_code_units(&self) -> impl Iterator<Item = &Bytecode> {
        self.functions.iter().map(std::rc::Rc::as_ref)
    }

    pub(crate) fn instruction(&self, offset: usize) -> Option<Instruction> {
        let opcode = Opcode::decode(*self.code.get(offset)?)?;
        let operand = if opcode.width() == 5 {
            Some(u32::from_le_bytes(
                self.code.get(offset + 1..offset + 5)?.try_into().ok()?,
            ))
        } else {
            None
        };
        Some(Instruction {
            offset,
            opcode,
            operand,
        })
    }
}
