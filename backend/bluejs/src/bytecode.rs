// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compiler-owned bytecode: one byte per opcode plus a fixed u32 operand
//! where needed. The table defines decoding, width and tiering metadata
//! together; VM dispatch is an exhaustive match on its generated enum.
//! Jumps use byte offsets. No public deserializer accepts arbitrary code:
//! serialization/versioning and hostile-bytecode validation are future work.

use crate::Value;

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
    DynamicImport: 1, 0;
    ImportMeta: 1, 0;
    EnterWith: 1, 0;
    LeaveWith: 1, 0;
    WithGet: 5, 0;
    WithSet: 5, 0;
    ResolveWithReference: 5, 0;
    LoadWithReference: 1, 0;
    StoreWithReference: 1, 0;
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
    AbruptJump: 5, 0;
    DefineData: 1, 0;
    DefineAccessor: 5, 0;
    DefineMethod: 1, 0;
    DefineClassAccessor: 5, 0;
    DefineClassStaticField: 1, 0;
    DefinePrivateStaticField: 1, 0;
    DefinePrivateField: 5, 0;
    DefinePrivateMethod: 5, 0;
    DefinePrivateAccessor: 5, 0;
    CallClassStaticBlock: 1, 0;
    SetClassHome: 1, 0;
    SetClassHeritage: 1, 0;
    InitializePrivateBrand: 5, 0;
    PrivateGet: 5, MAY_USE_INLINE_CACHE;
    PrivateGetMethod: 5, MAY_USE_INLINE_CACHE;
    PrivateSet: 5, MAY_USE_INLINE_CACHE;
    PrivateIn: 5, MAY_USE_INLINE_CACHE;
    SuperGet: 1, MAY_USE_INLINE_CACHE;
    SuperGetMethod: 1, MAY_USE_INLINE_CACHE;
    SuperSet: 1, MAY_USE_INLINE_CACHE;
    SuperUpdate: 5, MAY_USE_INLINE_CACHE;
    SuperCall: 5, 0;
    SuperCallSpread: 1, 0;
    SuperCallForward: 1, 0;
    EnterClassFieldInitializer: 1, 0;
    LeaveClassFieldInitializer: 1, 0;
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
    Source,
}

#[derive(Clone)]
pub(crate) struct ModuleImport {
    pub module_request: String,
    pub import_name: ModuleImportName,
    pub local_slot: Option<u32>,
}

/// One executable [[RequestedModules]] entry, in source-text order.
/// Source-phase records resolve during linking but do not participate in
/// module evaluation, so they are omitted from this sequence.
#[derive(Clone)]
pub(crate) struct ModuleRequest {
    pub module_request: String,
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
    },
    Star {
        module_request: String,
    },
    Namespace {
        export_name: String,
        module_request: String,
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
}

impl Bytecode {
    pub(crate) fn empty() -> Self {
        Self {
            code: Vec::new(),
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
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.code
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
