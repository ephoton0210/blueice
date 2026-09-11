// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded operand-stack interpreter for compiler-owned bytecode.
//! Allocating instructions root all VM-held objects around heap
//! safepoints. The collector itself additionally protects store inputs.

use crate::bytecode::{Binding, ModuleExport, ModuleImportName};
use crate::heap::{
    GeneratorHandlerFrame, GeneratorHandlerState, GeneratorPendingCompletion, GeneratorState,
    PrivateElement,
};
use crate::native::{self, NativeFunction};
use crate::primitive;
use crate::{
    Bytecode, Heap, HeapConfig, HeapError, JsString, JsSymbol, ObjectId, Opcode,
    PropertyDescriptor, PropertyName, RootId, Value,
};
use num_bigint::{BigInt, Sign};
use num_traits::{One, ToPrimitive, Zero};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::rc::Rc;
mod builtins;
mod errors;
mod functions;
mod intl;
mod json;
mod regexp;
mod test262;
use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy)]
pub struct VmConfig {
    pub heap: HeapConfig,
    /// Per-execute dispatch limit, including branches/scopes and native
    /// raw/split/replace loop steps; not a wall-clock or whole-native-work limit.
    pub instruction_budget: u64,
    /// Maximum UTF-16 payload bytes in any one runtime string (not total
    /// RSS): two bytes per code unit, including lone surrogates.
    pub max_string_bytes: usize,
    /// Wall-clock limit for each isolated regex compilation or match.
    pub regex_timeout: std::time::Duration,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            heap: HeapConfig::default(),
            instruction_budget: 1_000_000,
            max_string_bytes: 1024 * 1024,
            regex_timeout: crate::regex_worker::DEFAULT_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeError {
    ReferenceError(String),
    TypeError(String),
    RangeError(String),
    SyntaxError(String),
    Thrown(Value),
    Heap(HeapError),
    InstructionLimit,
    StringLimit {
        limit: usize,
    },
    RegexTimeout,
    Test262(String),
    RegexWorker(String),
    /// Static module linking failed before any module body was evaluated.
    ModuleResolution(String),
    Unsupported(&'static str),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReferenceError(name) => write!(f, "ReferenceError: {name} is not defined"),
            Self::TypeError(message) => write!(f, "TypeError: {message}"),
            Self::RangeError(message) => write!(f, "RangeError: {message}"),
            Self::SyntaxError(message) => write!(f, "SyntaxError: {message}"),
            Self::Thrown(value) => write!(f, "uncaught JavaScript value: {value:?}"),
            Self::Heap(error) => error.fmt(f),
            Self::InstructionLimit => f.write_str("BlueJS instruction budget exhausted"),
            Self::StringLimit { limit } => write!(f, "BlueJS string exceeds {limit} bytes"),
            Self::Test262(message) => write!(f, "Test262Error: {message}"),
            Self::RegexTimeout => f.write_str("BlueJS regex deadline exceeded"),
            Self::RegexWorker(message) => write!(f, "BlueJS regex worker failed: {message}"),
            Self::ModuleResolution(message) => write!(f, "module resolution error: {message}"),
            Self::Unsupported(reason) => write!(f, "BlueJS unsupported: {reason}"),
        }
    }
}
impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Heap(error) => Some(error),
            _ => None,
        }
    }
}
impl From<HeapError> for RuntimeError {
    fn from(error: HeapError) -> Self {
        match error {
            HeapError::InvalidArrayLength => Self::RangeError("invalid array length".into()),
            HeapError::InvalidBufferRange => {
                Self::RangeError("invalid ArrayBuffer view range".into())
            }
            HeapError::DetachedArrayBuffer
            | HeapError::InvalidInternalSlot(_)
            | HeapError::RevokedProxy => Self::TypeError(error.to_string()),
            HeapError::UninitializedModuleExport => {
                Self::ReferenceError("module export is uninitialized".into())
            }
            _ => Self::Heap(error),
        }
    }
}

impl RuntimeError {
    /// Language exceptions participate in ECMAScript completion propagation.
    /// Limits, allocation failures and failed isolated host workers remain
    /// uncatchable host aborts: user code must not turn a resource boundary
    /// into an apparent JavaScript success.
    fn is_catchable(&self) -> bool {
        matches!(
            self,
            Self::ReferenceError(_)
                | Self::TypeError(_)
                | Self::RangeError(_)
                | Self::SyntaxError(_)
                | Self::Thrown(_)
                | Self::Test262(_)
        )
    }
}

#[derive(Clone)]
enum Completion {
    Throw(RuntimeError),
    Return(Value),
    TailRecur(Vec<Value>),
    Yield(Value),
    Jump { cleanup: usize, target: usize },
    Resume(usize),
    Halt(Value),
}

impl Completion {
    /// Convert a catchable completion to the heap-owned representation used
    /// by a suspended generator.  Every other RuntimeError is a host abort
    /// and is rejected by the interpreter before it reaches this boundary.
    fn into_generator_pending(self) -> Result<GeneratorPendingCompletion, RuntimeError> {
        match self {
            Self::Throw(RuntimeError::Thrown(value)) => {
                Ok(GeneratorPendingCompletion::Throw(value))
            }
            Self::Throw(RuntimeError::ReferenceError(message)) => {
                Ok(GeneratorPendingCompletion::ReferenceError(message))
            }
            Self::Throw(RuntimeError::TypeError(message)) => {
                Ok(GeneratorPendingCompletion::TypeError(message))
            }
            Self::Throw(RuntimeError::RangeError(message)) => {
                Ok(GeneratorPendingCompletion::RangeError(message))
            }
            Self::Throw(RuntimeError::SyntaxError(message)) => {
                Ok(GeneratorPendingCompletion::SyntaxError(message))
            }
            Self::Throw(RuntimeError::Test262(message)) => {
                Ok(GeneratorPendingCompletion::Test262(message))
            }
            Self::Return(value) => Ok(GeneratorPendingCompletion::Return(value)),
            Self::TailRecur(values) => Ok(GeneratorPendingCompletion::TailRecur(values)),
            Self::Jump { cleanup, target } => {
                Ok(GeneratorPendingCompletion::Jump { cleanup, target })
            }
            Self::Throw(error) => Err(error),
            Self::Yield(_) | Self::Resume(_) | Self::Halt(_) => Err(RuntimeError::Unsupported(
                "cannot suspend a generator with an internal completion",
            )),
        }
    }

    fn from_generator_pending(completion: GeneratorPendingCompletion) -> Self {
        match completion {
            GeneratorPendingCompletion::Throw(value) => Self::Throw(RuntimeError::Thrown(value)),
            GeneratorPendingCompletion::ReferenceError(message) => {
                Self::Throw(RuntimeError::ReferenceError(message))
            }
            GeneratorPendingCompletion::TypeError(message) => {
                Self::Throw(RuntimeError::TypeError(message))
            }
            GeneratorPendingCompletion::RangeError(message) => {
                Self::Throw(RuntimeError::RangeError(message))
            }
            GeneratorPendingCompletion::SyntaxError(message) => {
                Self::Throw(RuntimeError::SyntaxError(message))
            }
            GeneratorPendingCompletion::Test262(message) => {
                Self::Throw(RuntimeError::Test262(message))
            }
            GeneratorPendingCompletion::Return(value) => Self::Return(value),
            GeneratorPendingCompletion::TailRecur(values) => Self::TailRecur(values),
            GeneratorPendingCompletion::Jump { cleanup, target } => Self::Jump { cleanup, target },
        }
    }
}

type HandlerState = GeneratorHandlerState;
type HandlerFrame = GeneratorHandlerFrame;

enum CompletionAction {
    Continue,
    Jump(usize),
    Return(Value),
    TailRecur(Vec<Value>),
    Throw(RuntimeError),
}

enum InterpreterExit {
    Return(Value),
    Yield {
        value: Value,
        pc: usize,
        iterators: Vec<Value>,
        handlers: Vec<HandlerFrame>,
    },
    Suspend {
        pc: usize,
    },
    /// An async execution context reaches an Await expression. Its execution
    /// context is moved into a continuation before the next Promise job turn
    /// resumes it.
    Await {
        promise: ObjectId,
        pc: usize,
        handlers: Vec<HandlerFrame>,
    },
}

// Ordinary calls still nest the Rust interpreter. Keep this comfortably below
// the default test-thread stack so recursive JavaScript reports a catchable
// RangeError instead of aborting the embedding process.
const MAX_RECURSIVE_CALL_DEPTH: usize = 16;

/// A realm-level declarative or object-backed global binding. The cell is
/// permanently rooted for the realm lifetime so script closures and later
/// scripts observe one live binding rather than copied completion values.
struct GlobalBinding {
    cell: ObjectId,
    mutable: bool,
    strict_immutable: bool,
    property: bool,
    _root: RootId,
}

/// Declarative Environment Records distinguish an immutable binding created
/// for `const` from the non-strict immutable binding used for a named function
/// expression. The latter accepts a sloppy assignment as a no-op, but both
/// reject a strict reference.
fn binding_allows_assignment(
    binding: &Binding,
    strict_reference: bool,
) -> Result<bool, RuntimeError> {
    if binding.mutable {
        return Ok(true);
    }
    if binding.strict_immutable || strict_reference {
        return Err(RuntimeError::TypeError(format!(
            "assignment to constant {}",
            binding.name
        )));
    }
    Ok(false)
}

/// A sloppy direct eval declaration installed in an ordinary function's
/// VariableEnvironment. Unlike global bindings it is neither permanent nor
/// property-backed: it lives for the frame, may be deleted, and closures keep
/// its cell alive when they capture it.
#[derive(Clone)]
struct DynamicEvalBinding {
    cell: ObjectId,
    /// A direct eval can introduce a binding in the active function's
    /// VariableEnvironment that shadows a statically captured binding from
    /// an outer function. Track the exact captured cells, rather than only
    /// the name: an eval-local block function can use the same name while
    /// remaining an independent lexical binding.
    shadowed_cells: Vec<ObjectId>,
}

/// Runtime data that belongs to one member of a transient static module
/// graph.  The bytecode stays immutable; every top-level module slot gets a
/// rooted cell while the graph is linked and evaluated.
struct LinkedModule {
    cells: HashMap<usize, ObjectId>,
    namespace: Option<ObjectId>,
    evaluated: bool,
    evaluating: bool,
    suspended: bool,
    completion: Option<Value>,
    /// An asynchronous module keeps its abrupt completion after its frame has
    /// been released. Static parents and later dynamic imports observe this
    /// stored evaluation error instead of attempting to execute it again.
    error: Option<Value>,
}

/// The realm keeps Source Text Module Records after an entry evaluation.
/// Dynamic imports must observe the same live bindings and evaluation status
/// as static imports in an earlier graph, rather than creating a second set
/// of cells for the same canonical module name.
struct ModuleGraphState {
    linked: HashMap<String, LinkedModule>,
    roots: Vec<RootId>,
}

/// The resumable portion of a top-level module evaluation. `execution`
/// carries the live lexical cells, stack and realm state; `pc` points just
/// after the Await opcode, where its settlement value is pushed on resume.
struct ModuleContinuation {
    module: String,
    code: Bytecode,
    pc: usize,
    execution: SuspendedModuleExecution,
    iterators: Vec<Value>,
    handlers: Vec<HandlerFrame>,
}

/// The resumable portion of an ordinary async-function invocation.  This is
/// intentionally the same complete execution-state shape as a top-level
/// module continuation: later async generators and async iteration must be
/// able to reuse the same frame, rooting, and completion machinery.
struct AsyncContinuation {
    /// `Some` makes this an async-generator request frame rather than an
    /// ordinary async-function frame. The request Promise remains `target`.
    generator: Option<ObjectId>,
    target: ObjectId,
    code: Rc<Bytecode>,
    pc: usize,
    execution: SuspendedModuleExecution,
    iterators: Vec<Value>,
    handlers: Vec<HandlerFrame>,
    call_depth: usize,
}

#[derive(Clone, PartialEq, Eq)]
enum ExportResolution {
    Binding { module: String, slot: usize },
    Missing,
    Ambiguous,
    Namespace { module: String },
    Source { module: String },
}

enum PromiseStatus {
    Pending,
    Fulfilled(Value),
    Rejected(Value),
}

enum PromiseAwaitStatus {
    Pending,
    Fulfilled(Value),
    Rejected(Value),
}

impl PromiseStatus {
    fn clone_for_await(&self) -> PromiseAwaitStatus {
        match self {
            Self::Pending => PromiseAwaitStatus::Pending,
            Self::Fulfilled(value) => PromiseAwaitStatus::Fulfilled(value.clone()),
            Self::Rejected(value) => PromiseAwaitStatus::Rejected(value.clone()),
        }
    }
}

struct PromiseThenReaction {
    target: ObjectId,
    on_fulfilled: Value,
    on_rejected: Value,
}

enum PromiseReaction {
    Then(PromiseThenReaction),
    ModuleAwait {
        continuation: u64,
    },
    AsyncAwait {
        continuation: u64,
    },
    /// AsyncGeneratorYield awaits its yielded value before resolving the
    /// outstanding `.next()` capability.
    AsyncGeneratorYield {
        generator: ObjectId,
        target: ObjectId,
        result: ObjectId,
    },
    /// A return/throw request is forwarded through an active `yield*`
    /// delegate and therefore must await the delegate method's result.
    AsyncGeneratorDelegate {
        generator: ObjectId,
        target: ObjectId,
        kind: AsyncGeneratorDelegateKind,
    },
}

#[derive(Clone, Copy)]
enum AsyncGeneratorDelegateKind {
    Return,
    Throw,
}

struct PromiseRecord {
    status: PromiseStatus,
    reactions: Vec<PromiseReaction>,
}

/// Aggregation bookkeeping for `Promise.all`. Each input observes its own
/// resolution job; the target is fulfilled only after every indexed slot has
/// settled, so a pending dependency never becomes a host-level unsupported
/// condition.
struct PromiseAllState {
    values: Vec<Option<Value>>,
    remaining: usize,
}

enum PromiseJob {
    Reaction {
        target: ObjectId,
        handler: Value,
        value: Value,
        fulfilled: bool,
    },
    /// PromiseResolveThenableJob. Keeping this as a real queue entry (rather
    /// than calling `then` inline) preserves the observable microtask turn
    /// between resolving a thenable and resuming an await/reaction.
    Thenable {
        target: ObjectId,
        thenable: Value,
        then: Value,
    },
    DynamicImport {
        target: ObjectId,
        referrer: String,
        specifier: String,
    },
    ModuleAwait {
        continuation: u64,
        value: Value,
        fulfilled: bool,
    },
    AsyncAwait {
        continuation: u64,
        value: Value,
        fulfilled: bool,
    },
    AsyncGeneratorYield {
        generator: ObjectId,
        target: ObjectId,
        result: ObjectId,
        value: Value,
        fulfilled: bool,
    },
    AsyncGeneratorDelegate {
        generator: ObjectId,
        target: ObjectId,
        kind: AsyncGeneratorDelegateKind,
        value: Value,
        fulfilled: bool,
    },
}

enum DynamicImportResult {
    Fulfilled(Value),
    Waiting(String),
}

/// A Test262 realm owns a complete VM, while its public global is a facade
/// in the requesting VM. The facade keeps the host boundary explicit: values
/// that can cross heaps are copied after evaluation rather than leaking an
/// object identity from the nested heap.
struct Test262Realm {
    vm: Box<Vm>,
}

/// Runtime state displaced while a pending top-level await drives jobs. The
/// job may link and evaluate another module graph in this same realm, so the
/// outer interpreter's frame must survive both the nested graph's cleanup and
/// an intervening collection.
struct SuspendedModuleExecution {
    result_root: Option<RootId>,
    stack: Vec<Value>,
    bindings: Vec<Option<Value>>,
    binding_metadata: Vec<Binding>,
    completion: Value,
    completion_empty: bool,
    active_scopes: Vec<u32>,
    active_scope_slots: Vec<Vec<u32>>,
    with_objects: Vec<Value>,
    pending_completions: Vec<Completion>,
    completion_saves: Vec<(Value, bool)>,
    remaining_instructions: u64,
    cells: HashMap<usize, ObjectId>,
    dynamic_eval_bindings: HashMap<String, DynamicEvalBinding>,
    eval_dynamic_slots: HashMap<usize, String>,
    dynamic_eval_outer_bindings: Vec<HashMap<String, DynamicEvalBinding>>,
    this: Value,
    arguments: Vec<Value>,
    callee: Value,
    strict: bool,
    top_level_module: bool,
    script_global_slots: HashMap<usize, String>,
    variable_scope: u32,
    variable_scope_lexicals: Vec<String>,
    templates: HashMap<u64, ObjectId>,
    new_target: Value,
    new_target_allowed: bool,
    home_object: Option<ObjectId>,
    class_constructor: Option<ObjectId>,
    class_field_initializer_depth: u32,
    active_module_name: Option<String>,
}

/// An isolated execution context with one realm global environment. Ordinary
/// [`Vm::execute`] calls use fresh local bindings; classic scripts additionally
/// retain their global declarations for later [`Vm::execute_script`] calls.
pub struct Vm {
    config: VmConfig,
    heap: Heap,
    object_prototype: ObjectId,
    array_prototype: ObjectId,
    string_intrinsics: Option<(ObjectId, ObjectId)>,
    /// `%TypedArray%` and `%TypedArray%.prototype`, kept outside the global
    /// object but permanently reachable from every concrete constructor.
    typed_array_intrinsics: Option<(ObjectId, ObjectId)>,
    result_root: Option<RootId>,
    stack: Vec<Value>,
    // None is a lexical binding's uninitialized state, never JS undefined.
    bindings: Vec<Option<Value>>,
    // The matching static metadata for `bindings`; nested calls and direct
    // eval swap it together with their runtime binding vector.
    binding_metadata: Vec<Binding>,
    completion: Value,
    completion_empty: bool,
    active_scopes: Vec<u32>,
    active_scope_slots: Vec<Vec<u32>>,
    with_objects: Vec<Value>,
    // Values in suspended finally paths live here rather than in Rust-only
    // handler records, so VM safepoints root them during allocations.
    pending_completions: Vec<Completion>,
    completion_saves: Vec<(Value, bool)>,
    remaining_instructions: u64,
    cells: HashMap<usize, ObjectId>,
    /// Bytecodes supplied by the host for this realm's module loader.
    /// Dynamic imports resolve only inside this explicit registry.
    module_registry: HashMap<String, Bytecode>,
    /// Host-provided source-phase module records. Their opaque identities are
    /// intentionally separate from executable module bytecode.
    module_source_registry: HashSet<String>,
    module_source_cache: HashMap<String, ObjectId>,
    module_source_roots: HashMap<String, RootId>,
    abstract_module_source_prototype: Option<ObjectId>,
    /// Retains the entry namespace until a dynamic-import job has handed it
    /// to its promise.  The next graph evaluation replaces this cache.
    last_module_namespace: Option<ObjectId>,
    last_module_namespace_root: Option<RootId>,
    /// Per-realm Module Record namespace cache. Dynamic import is required to
    /// return this same object for repeated requests, including a namespace
    /// already made visible through a static `import * as` binding.
    module_namespace_cache: HashMap<String, ObjectId>,
    module_namespace_roots: HashMap<String, RootId>,
    module_graph: Option<ModuleGraphState>,
    module_continuations: HashMap<u64, ModuleContinuation>,
    next_module_continuation: u64,
    async_continuations: HashMap<u64, AsyncContinuation>,
    next_async_continuation: u64,
    module_pending_dependencies: HashMap<String, HashSet<String>>,
    module_async_parents: HashMap<String, Vec<String>>,
    module_import_waiters: HashMap<String, Vec<ObjectId>>,
    /// The defining source-text module for currently executing code. A
    /// closure receives the same association at creation time.
    active_module_name: Option<String>,
    module_closure_referrers: HashMap<ObjectId, String>,
    // Bindings created by sloppy direct eval in the active ordinary-function
    // VariableEnvironment. They move into a generator's suspended state when
    // it yields and are rooted at interpreter safepoints.
    dynamic_eval_bindings: HashMap<String, DynamicEvalBinding>,
    // Slot-to-name mapping for the direct-eval bytecode currently executing.
    // It lets EnterScope attach fresh `var` cells to the active frame.
    eval_dynamic_slots: HashMap<usize, String>,
    // Active callers' dynamic VariableEnvironments. A nested closure can
    // resolve an eval-created name in its still-running lexical parent, but
    // a new direct eval declaration always enters `dynamic_eval_bindings`.
    dynamic_eval_outer_bindings: Vec<HashMap<String, DynamicEvalBinding>>,
    this: Value,
    arguments: Vec<Value>,
    // The current ordinary function object is needed while materializing its
    // arguments object, notably for the sloppy `arguments.callee` data
    // property. Arrow functions never materialize a replacement binding.
    callee: Value,
    strict: bool,
    call_depth: usize,
    // Module top-level code has an undefined `this`; classic scripts lazily
    // substitute the realm global when `this` is first observed.
    top_level_module: bool,
    globals: HashMap<String, ObjectId>,
    global_bindings: HashMap<String, GlobalBinding>,
    // Slots in the currently executing classic script's outer scope. Nested
    // function/eval frames temporarily replace this map because slot indices
    // are local to their own bytecode.
    script_global_slots: HashMap<usize, String>,
    // Scope index for the active function's VariableEnvironment. Direct eval
    // uses this to distinguish intervening lexical scopes from ordinary
    // outer bindings that its `var` declarations may reuse.
    variable_scope: u32,
    // Lexical declarations in the active VariableEnvironment. During a
    // non-simple parameter initializer that scope has not been entered yet,
    // but direct eval must still reject a conflicting `var` declaration.
    variable_scope_lexicals: Vec<String>,
    iterator_prototype: Option<ObjectId>,
    regexp_iterator_prototype: Option<ObjectId>,
    templates: HashMap<u64, ObjectId>,
    new_target: Value,
    new_target_allowed: bool,
    // The `[[HomeObject]]` of the currently executing method or class
    // constructor. It is runtime frame state because `super` is lexical.
    home_object: Option<ObjectId>,
    // A class constructor's home object is its instance prototype for
    // `super.property`; `super()` separately needs the constructor closure
    // that owns the evaluated superclass metadata.
    class_constructor: Option<ObjectId>,
    // A direct eval in an instance field is outside a constructor for the
    // `super()` early-error rules even though fields are lowered into the
    // constructor bytecode.
    class_field_initializer_depth: u32,
    iterator_base: Option<ObjectId>,
    array_iterator_prototype: Option<ObjectId>,
    generator_prototype: Option<ObjectId>,
    async_iterator_base: Option<ObjectId>,
    async_generator_prototype: Option<ObjectId>,
    /// `%AsyncFunction.prototype%`, permanently rooted with the realm once
    /// the first async closure needs it. Its `constructor` property keeps
    /// `%AsyncFunction%` reachable without exposing a global binding.
    async_function_prototype: Option<ObjectId>,
    promise_prototype: Option<ObjectId>,
    map_prototype: Option<ObjectId>,
    set_prototype: Option<ObjectId>,
    promises: HashMap<ObjectId, PromiseRecord>,
    promise_all: HashMap<ObjectId, PromiseAllState>,
    promise_jobs: VecDeque<PromiseJob>,
    test262_done: Option<Result<(), Value>>,
    test262_realms: HashMap<ObjectId, Test262Realm>,
    throw_type_error: Option<ObjectId>,
    joining: Vec<ObjectId>,
}

impl Default for Vm {
    fn default() -> Self {
        Self::new(VmConfig::default()).expect("default VM configuration is valid")
    }
}

impl Vm {
    /// Creates an isolated VM and its rooted object/array prototypes.
    /// The heap budget must accommodate both prototype records.
    pub fn new(config: VmConfig) -> Result<Self, HeapError> {
        let mut heap = Heap::new(config.heap)?;
        let object_prototype = heap.alloc_object(None)?;
        // Permanent root, released with the heap. Builtin properties and
        // callable methods are installed lazily by string_intrinsics.
        heap.root(object_prototype)?;
        let array_prototype = heap.alloc_array(0, Some(object_prototype))?;
        heap.root(array_prototype)?;
        Ok(Self {
            config,
            heap,
            object_prototype,
            array_prototype,
            string_intrinsics: None,
            typed_array_intrinsics: None,
            result_root: None,
            stack: Vec::new(),
            bindings: Vec::new(),
            binding_metadata: Vec::new(),
            completion: Value::Undefined,
            completion_empty: true,
            active_scopes: Vec::new(),
            active_scope_slots: Vec::new(),
            with_objects: Vec::new(),
            pending_completions: Vec::new(),
            completion_saves: Vec::new(),
            remaining_instructions: 0,
            cells: HashMap::new(),
            module_registry: HashMap::new(),
            module_source_registry: HashSet::new(),
            module_source_cache: HashMap::new(),
            module_source_roots: HashMap::new(),
            abstract_module_source_prototype: None,
            last_module_namespace: None,
            last_module_namespace_root: None,
            module_namespace_cache: HashMap::new(),
            module_namespace_roots: HashMap::new(),
            module_graph: None,
            module_continuations: HashMap::new(),
            next_module_continuation: 0,
            async_continuations: HashMap::new(),
            next_async_continuation: 0,
            module_pending_dependencies: HashMap::new(),
            module_async_parents: HashMap::new(),
            module_import_waiters: HashMap::new(),
            active_module_name: None,
            module_closure_referrers: HashMap::new(),
            dynamic_eval_bindings: HashMap::new(),
            eval_dynamic_slots: HashMap::new(),
            dynamic_eval_outer_bindings: Vec::new(),
            this: Value::Undefined,
            arguments: Vec::new(),
            callee: Value::Undefined,
            strict: false,
            call_depth: 0,
            top_level_module: false,
            globals: HashMap::new(),
            global_bindings: HashMap::new(),
            script_global_slots: HashMap::new(),
            variable_scope: 0,
            variable_scope_lexicals: Vec::new(),
            iterator_prototype: None,
            regexp_iterator_prototype: None,
            templates: HashMap::new(),
            new_target: Value::Undefined,
            new_target_allowed: false,
            home_object: None,
            class_constructor: None,
            class_field_initializer_depth: 0,
            iterator_base: None,
            array_iterator_prototype: None,
            generator_prototype: None,
            async_iterator_base: None,
            async_generator_prototype: None,
            async_function_prototype: None,
            promise_prototype: None,
            map_prototype: None,
            set_prototype: None,
            promises: HashMap::new(),
            promise_all: HashMap::new(),
            promise_jobs: VecDeque::new(),
            test262_done: None,
            test262_realms: HashMap::new(),
            throw_type_error: None,
            joining: Vec::new(),
        })
    }

    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    /// Executes only bytecode produced by [`crate::compile`] with fresh
    /// bindings. A returned object and its reachable graph stay alive until
    /// the next execute (including a failing execute), or until this VM is
    /// dropped. Both success and error paths release temporary runtime roots.
    pub fn execute(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        self.execute_with_global_bindings(code, false, false)
    }

    /// Executes a classic script in this realm and publishes successful
    /// top-level `var` and function declarations on `globalThis` for a later
    /// classic script. Lexical bindings retain their script-local boundary.
    pub fn execute_script(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        self.execute_with_global_bindings(code, true, false)
    }

    /// Evaluates one dependency-free module. Its declarations are scoped to
    /// this evaluation and are never exposed as classic global properties.
    /// Use [`Vm::execute_module_graph`] when the code has import/export
    /// entries that need static linking.
    pub fn execute_module(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        self.execute_with_global_bindings(code, false, true)
    }

    /// Installs the bounded module-loader context used by dynamic `import()`
    /// from classic script code. The host supplies precompiled Module-goal
    /// bytecode and an opaque referrer key; BlueJS never reads module files
    /// itself.
    pub fn set_module_loader_context(
        &mut self,
        referrer: impl Into<String>,
        modules: HashMap<String, Bytecode>,
    ) {
        self.module_registry = modules;
        self.active_module_name = Some(referrer.into());
    }

    /// Installs the host's canonical names for source-phase module records.
    /// These records produce Module Source Objects and must not be supplied as
    /// executable Source Text Module bytecode.
    pub fn set_module_source_loader_context(&mut self, sources: Vec<String>) {
        self.module_source_registry = sources.into_iter().collect();
    }

    /// Links and synchronously evaluates one static module graph.
    ///
    /// Keys in `modules` are host-resolved module names. Relative requests
    /// are resolved against their referrer's slash-separated key, so callers
    /// normally use canonical paths such as `directory/entry.js`.  This is a
    /// deliberately synchronous subset: dynamic import and top-level await
    /// remain outside this API, but normal static cycles and live bindings
    /// use the same instantiate-before-evaluate shape as Source Text Module
    /// Records.
    pub fn execute_module_graph(
        &mut self,
        entry: &str,
        modules: &HashMap<String, Bytecode>,
    ) -> Result<Value, RuntimeError> {
        self.execute_module_graph_inner(entry, modules, true)
    }

    /// The Dynamic Import job owns its own observable queue turns. It must
    /// link/evaluate a module graph without recursively draining those jobs;
    /// otherwise a sibling dynamic import can start and finish before the
    /// importing job has installed its completion reaction.
    fn execute_module_graph_inner(
        &mut self,
        entry: &str,
        modules: &HashMap<String, Bytecode>,
        drain_jobs: bool,
    ) -> Result<Value, RuntimeError> {
        let (mut linked, mut roots, fresh_graph) = match self.module_graph.take() {
            Some(graph) => (graph.linked, graph.roots, false),
            None => (HashMap::new(), Vec::new(), true),
        };
        let mut result = (|| {
            self.module_registry = modules.clone();
            // Resolution errors can be converted to a dynamic-import Promise
            // rejection before graph setup reaches the usual execution reset.
            // Give that host-visible error construction a fresh budget too.
            self.remaining_instructions = self.config.instruction_budget;
            // The host supplies the closed source set for this realm. Link
            // every supplied record once so a later dynamic import shares a
            // static import's cells instead of constructing a second module
            // instance for the same canonical path.
            let mut order: Vec<_> = modules.keys().cloned().collect();
            order.sort_unstable();

            if let Some(root) = self.result_root.take() {
                self.heap.unroot(root)?;
            }
            if let Some(root) = self.last_module_namespace_root.take() {
                self.heap.unroot(root)?;
            }
            self.last_module_namespace = None;
            self.with_roots(|heap| {
                heap.collect_major();
                Ok(())
            })?;
            self.stack.clear();
            self.bindings.clear();
            self.binding_metadata.clear();
            self.cells.clear();
            self.active_scopes.clear();
            self.active_scope_slots.clear();
            self.with_objects.clear();
            self.script_global_slots.clear();
            self.dynamic_eval_bindings.clear();
            self.eval_dynamic_slots.clear();
            self.dynamic_eval_outer_bindings.clear();
            self.completion = Value::Undefined;
            self.completion_empty = true;
            self.remaining_instructions = self.config.instruction_budget;
            self.this = Value::Undefined;
            self.top_level_module = true;

            if fresh_graph {
                linked = order
                    .iter()
                    .cloned()
                    .map(|name| {
                        (
                            name,
                            LinkedModule {
                                cells: HashMap::new(),
                                namespace: None,
                                evaluated: false,
                                evaluating: false,
                                suspended: false,
                                completion: None,
                                error: None,
                            },
                        )
                    })
                    .collect();

                // ModuleDeclarationInstantiation creates all own bindings before
                // wiring imports.  A `var` binding is initialized immediately;
                // lexical bindings deliberately have no `value` property yet.
                for name in &order {
                    let code = modules
                        .get(name)
                        .expect("reachable module was checked during collection");
                    if !code.module {
                        return Err(RuntimeError::ModuleResolution(format!(
                            "{name} was not compiled using the module goal"
                        )));
                    }
                    let imported_slots: HashSet<_> = code
                        .module_imports
                        .iter()
                        .filter_map(|import| import.local_slot.map(|slot| slot as usize))
                        .collect();
                    let slots = code.scopes.first().cloned().unwrap_or_default();
                    let record = linked
                        .get_mut(name)
                        .expect("linked record was allocated for every module");
                    for slot in slots {
                        let slot = slot as usize;
                        if imported_slots.contains(&slot) {
                            continue;
                        }
                        let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                        roots.push(self.heap.root(cell)?);
                        if !code.bindings[slot].lexical {
                            self.with_roots(|heap| heap.set(cell, "value", Value::Undefined))?;
                        }
                        record.cells.insert(slot, cell);
                    }
                }

                // Validate every named indirect export before evaluating any
                // module body.  A missing or ambiguous `export { x } from ...`
                // is a ModuleDeclarationInstantiation error, including when the
                // body would otherwise call $DONOTEVALUATE().  Star exports do
                // not themselves fail here; their ambiguity matters only when a
                // particular name is resolved by an import or indirect export.
                for name in &order {
                    let code = modules
                        .get(name)
                        .expect("reachable module was checked during collection");
                    for export in &code.module_exports {
                        let ModuleExport::Indirect {
                            export_name,
                            module_request,
                            import_name,
                        } = export
                        else {
                            continue;
                        };
                        let target = Self::resolve_module_request(name, module_request)?;
                        match Self::resolve_export(modules, &target, import_name, &mut Vec::new())?
                        {
                            ExportResolution::Binding { .. }
                            | ExportResolution::Namespace { .. }
                            | ExportResolution::Source { .. } => {}
                            ExportResolution::Missing | ExportResolution::Ambiguous => {
                                return Err(RuntimeError::ModuleResolution(format!(
                                "{module_request} does not export {import_name} for {export_name}"
                            )));
                            }
                        }
                    }
                    for export in &code.module_exports {
                        let ModuleExport::Source { module_request, .. } = export else {
                            continue;
                        };
                        let target = Self::resolve_module_request(name, module_request)?;
                        self.module_source_object(&target, modules)?;
                    }
                    // Source-phase requests are resolved while loading the graph,
                    // before ordinary ModuleDeclarationInstantiation validates
                    // named imports elsewhere in that graph. This keeps a host
                    // failure to supply a source record distinct from a later
                    // SyntaxError linking failure.
                    for import in &code.module_imports {
                        if !matches!(import.import_name, ModuleImportName::Source) {
                            continue;
                        }
                        let target = Self::resolve_module_request(name, &import.module_request)?;
                        self.module_source_object(&target, modules)?;
                    }
                }

                // Import bindings are immutable aliases.  The importer stores the
                // exporter's *cell*, so later stores in the exporting module are
                // visible without any copy or notification mechanism.
                for name in &order {
                    let code = modules
                        .get(name)
                        .expect("reachable module was checked during collection");
                    let mut aliases = Vec::new();
                    for import in &code.module_imports {
                        let Some(local_slot) = import.local_slot else {
                            continue;
                        };
                        let target = Self::resolve_module_request(name, &import.module_request)?;
                        let resolution = match &import.import_name {
                            ModuleImportName::Named(import_name) => Self::resolve_export(
                                modules,
                                &target,
                                import_name,
                                &mut Vec::new(),
                            )?,
                            ModuleImportName::Namespace => ExportResolution::Namespace {
                                module: target.clone(),
                            },
                            ModuleImportName::Source => ExportResolution::Source {
                                module: target.clone(),
                            },
                        };
                        let cell = match resolution {
                            ExportResolution::Binding {
                                module: exporter,
                                slot,
                            } => linked
                                .get(&exporter)
                                .and_then(|record| record.cells.get(&slot))
                                .copied()
                                .ok_or_else(|| {
                                    RuntimeError::ModuleResolution(format!(
                                        "export binding from {exporter} has no cell"
                                    ))
                                })?,
                            ExportResolution::Namespace { module } => {
                                let namespace = self.module_namespace(
                                    &module,
                                    modules,
                                    &mut linked,
                                    &mut roots,
                                )?;
                                let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                                roots.push(self.heap.root(cell)?);
                                self.with_roots(|heap| {
                                    heap.set(cell, "value", Value::Object(namespace))
                                })?;
                                cell
                            }
                            ExportResolution::Source { module } => {
                                let source = self.module_source_object(&module, modules)?;
                                let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                                roots.push(self.heap.root(cell)?);
                                self.with_roots(|heap| {
                                    heap.set(cell, "value", Value::Object(source))
                                })?;
                                cell
                            }
                            ExportResolution::Missing | ExportResolution::Ambiguous => {
                                let import_name = match &import.import_name {
                                    ModuleImportName::Named(name) => name.as_str(),
                                    ModuleImportName::Namespace => "*",
                                    ModuleImportName::Source => "source",
                                };
                                return Err(RuntimeError::ModuleResolution(format!(
                                    "{} does not export {import_name}",
                                    import.module_request
                                )));
                            }
                        };
                        aliases.push((local_slot as usize, cell));
                    }
                    let record = linked
                        .get_mut(name)
                        .expect("linked record was allocated for every module");
                    for (slot, cell) in aliases {
                        record.cells.insert(slot, cell);
                    }
                }

                // Run declaration instantiation for every reachable module before
                // evaluating any body.  This makes function exports callable
                // across a cycle, while lexical exports remain in their TDZ.
                for name in &order {
                    let code = modules
                        .get(name)
                        .expect("reachable module was checked during collection");
                    let record = linked
                        .get_mut(name)
                        .expect("linked record was allocated for every module");
                    self.initialize_module_record(code, &mut record.cells)?;
                }
            }

            let value = self.evaluate_module_record(entry, modules, &mut linked)?;
            let namespace = self.module_namespace(entry, modules, &mut linked, &mut roots)?;
            self.last_module_namespace = Some(namespace);
            self.last_module_namespace_root = Some(self.heap.root(namespace)?);
            if let Value::Object(id) = value {
                self.result_root = Some(self.heap.root(id)?);
            }
            Ok(value)
        })();

        // Keep an abrupt object completion observable to the embedding host
        // until the next execution, just as `execute_with_global_bindings`
        // does for scripts.  In particular, Test262 needs to inspect an
        // Error's `name` after a module evaluation rejects.
        if let Err(RuntimeError::Thrown(Value::Object(id))) = &result {
            self.result_root = Some(self.heap.root(*id)?);
        }

        if fresh_graph && result.is_err() {
            for root in roots {
                self.heap.unroot(root)?;
            }
        } else {
            self.module_graph = Some(ModuleGraphState { linked, roots });
        }
        // A module can become asynchronous solely through a dependency.  The
        // host-facing evaluation path must advance that dependency's queued
        // continuation just as it does for an entry containing `await`
        // itself, otherwise an immediately rejected imported module is
        // reported as a successful evaluation.
        let entry_is_async = self
            .module_graph
            .as_ref()
            .and_then(|graph| graph.linked.get(entry))
            .is_some_and(|record| record.suspended)
            || modules.get(entry).is_some_and(|code| {
                code.instructions()
                    .any(|instruction| instruction.opcode == Opcode::Await)
            });
        if result.is_ok() && drain_jobs && entry_is_async && !self.promise_jobs.is_empty() {
            self.remaining_instructions = self.config.instruction_budget;
            if let Err(error) = self.run_promise_jobs() {
                result = Err(error);
            }
        }
        if result.is_ok() {
            if let Some(error) = self
                .module_graph
                .as_ref()
                .and_then(|graph| graph.linked.get(entry))
                .and_then(|record| record.error.clone())
            {
                result = Err(RuntimeError::Thrown(error));
            }
        }
        if result.is_ok() {
            if let Some(value) = self
                .module_graph
                .as_ref()
                .and_then(|graph| graph.linked.get(entry))
                .and_then(|record| record.completion.clone())
            {
                result = Ok(value);
            }
        }
        self.stack.clear();
        self.bindings.clear();
        self.binding_metadata.clear();
        self.cells.clear();
        self.active_scopes.clear();
        self.active_scope_slots.clear();
        self.with_objects.clear();
        self.script_global_slots.clear();
        self.dynamic_eval_bindings.clear();
        self.eval_dynamic_slots.clear();
        self.dynamic_eval_outer_bindings.clear();
        self.completion = Value::Undefined;
        self.completion_empty = true;
        self.top_level_module = false;
        self.pending_completions.clear();
        self.completion_saves.clear();
        self.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })?;
        result
    }

    fn resolve_module_request(referrer: &str, request: &str) -> Result<String, RuntimeError> {
        if request.starts_with("./") || request.starts_with("../") {
            let mut parts: Vec<&str> = referrer.split('/').collect();
            if parts.len() > 1 {
                parts.pop();
            } else {
                parts.clear();
            }
            for part in request.split('/') {
                match part {
                    "" | "." => {}
                    ".." => {
                        if parts.pop().is_none() {
                            return Err(RuntimeError::ModuleResolution(format!(
                                "relative module request {request} escapes its host root"
                            )));
                        }
                    }
                    part => parts.push(part),
                }
            }
            return Ok(parts.join("/"));
        }
        Ok(request.to_string())
    }

    /// Materializes the host identity supplied for a source-phase import.
    /// Source Text Modules deliberately do not expose such a representation:
    /// accepting bytecode here would accidentally link or evaluate a module
    /// whose import phase must remain opaque.
    fn module_source_object(
        &mut self,
        module: &str,
        modules: &HashMap<String, Bytecode>,
    ) -> Result<ObjectId, RuntimeError> {
        if let Some(source) = self.module_source_cache.get(module) {
            return Ok(*source);
        }
        if modules.contains_key(module) {
            return Err(RuntimeError::ModuleResolution(format!(
                "{module} is a Source Text Module and has no source-phase representation"
            )));
        }
        if !self.module_source_registry.contains(module) {
            return Err(RuntimeError::TypeError(format!(
                "host did not provide a source-phase representation for {module}"
            )));
        }
        let prototype = self
            .abstract_module_source_prototype
            .unwrap_or(self.object_prototype);
        let source = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        let root = self.heap.root(source)?;
        self.module_source_cache.insert(module.to_string(), source);
        self.module_source_roots.insert(module.to_string(), root);
        Ok(source)
    }

    /// Dynamic `import()` first creates a Promise capability, then defers all
    /// resolution, linking and evaluation to the realm job queue.  The host
    /// registry is deliberately the same finite registry used for static
    /// module graphs, so no JavaScript source can escape the supplied tree.
    fn dynamic_import(&mut self, specifier: Value) -> Result<Value, RuntimeError> {
        let promise = self.new_promise()?;
        let specifier = match self.coerce_string(&specifier) {
            Ok(specifier) => specifier.to_utf8().map_err(|_| {
                RuntimeError::TypeError("module specifier is not a Unicode string".into())
            }),
            Err(error) => Err(error),
        };
        match specifier {
            Ok(specifier) => {
                let referrer = self
                    .active_module_name
                    .clone()
                    .unwrap_or_else(|| "<script>".to_string());
                self.promise_jobs.push_back(PromiseJob::DynamicImport {
                    target: promise,
                    referrer,
                    specifier,
                });
            }
            Err(error) => {
                let error = self.error_value(error)?;
                self.settle_promise(promise, PromiseStatus::Rejected(error))?;
            }
        }
        Ok(Value::Object(promise))
    }

    fn dynamic_import_job(
        &mut self,
        referrer: &str,
        specifier: &str,
    ) -> Result<DynamicImportResult, RuntimeError> {
        let entry = Self::resolve_module_request(referrer, specifier)?;
        if let Some(record) = self
            .module_graph
            .as_ref()
            .and_then(|graph| graph.linked.get(&entry))
        {
            if let Some(error) = &record.error {
                return Err(RuntimeError::Thrown(error.clone()));
            }
            if record.evaluated {
                let modules = self.module_registry.clone();
                let mut graph = self
                    .module_graph
                    .take()
                    .expect("checked module graph remains installed");
                let namespace =
                    self.module_namespace(&entry, &modules, &mut graph.linked, &mut graph.roots);
                self.module_graph = Some(graph);
                return namespace
                    .map(|namespace| DynamicImportResult::Fulfilled(Value::Object(namespace)));
            }
            if record.evaluating || record.suspended {
                return Ok(DynamicImportResult::Waiting(entry));
            }
        }
        let modules = self.module_registry.clone();
        self.execute_module_graph_inner(&entry, &modules, false)?;
        if self
            .module_graph
            .as_ref()
            .and_then(|graph| graph.linked.get(&entry))
            .is_some_and(|record| record.evaluating || record.suspended)
        {
            return Ok(DynamicImportResult::Waiting(entry));
        }
        let namespace = self
            .last_module_namespace
            .ok_or(RuntimeError::ModuleResolution(format!(
                "dynamic import of {entry} did not produce a namespace"
            )))?;
        Ok(DynamicImportResult::Fulfilled(Value::Object(namespace)))
    }

    fn suspend_module_execution(&mut self) -> SuspendedModuleExecution {
        SuspendedModuleExecution {
            result_root: self.result_root.take(),
            stack: std::mem::take(&mut self.stack),
            bindings: std::mem::take(&mut self.bindings),
            binding_metadata: std::mem::take(&mut self.binding_metadata),
            completion: std::mem::replace(&mut self.completion, Value::Undefined),
            completion_empty: std::mem::replace(&mut self.completion_empty, true),
            active_scopes: std::mem::take(&mut self.active_scopes),
            active_scope_slots: std::mem::take(&mut self.active_scope_slots),
            with_objects: std::mem::take(&mut self.with_objects),
            pending_completions: std::mem::take(&mut self.pending_completions),
            completion_saves: std::mem::take(&mut self.completion_saves),
            remaining_instructions: std::mem::replace(&mut self.remaining_instructions, 0),
            cells: std::mem::take(&mut self.cells),
            dynamic_eval_bindings: std::mem::take(&mut self.dynamic_eval_bindings),
            eval_dynamic_slots: std::mem::take(&mut self.eval_dynamic_slots),
            dynamic_eval_outer_bindings: std::mem::take(&mut self.dynamic_eval_outer_bindings),
            this: std::mem::replace(&mut self.this, Value::Undefined),
            arguments: std::mem::take(&mut self.arguments),
            callee: std::mem::replace(&mut self.callee, Value::Undefined),
            strict: std::mem::replace(&mut self.strict, false),
            top_level_module: std::mem::replace(&mut self.top_level_module, false),
            script_global_slots: std::mem::take(&mut self.script_global_slots),
            variable_scope: std::mem::replace(&mut self.variable_scope, 0),
            variable_scope_lexicals: std::mem::take(&mut self.variable_scope_lexicals),
            templates: std::mem::take(&mut self.templates),
            new_target: std::mem::replace(&mut self.new_target, Value::Undefined),
            new_target_allowed: std::mem::replace(&mut self.new_target_allowed, false),
            home_object: self.home_object.take(),
            class_constructor: self.class_constructor.take(),
            class_field_initializer_depth: std::mem::replace(
                &mut self.class_field_initializer_depth,
                0,
            ),
            active_module_name: self.active_module_name.take(),
        }
    }

    fn restore_module_execution(&mut self, execution: SuspendedModuleExecution) {
        self.result_root = execution.result_root;
        self.stack = execution.stack;
        self.bindings = execution.bindings;
        self.binding_metadata = execution.binding_metadata;
        self.completion = execution.completion;
        self.completion_empty = execution.completion_empty;
        self.active_scopes = execution.active_scopes;
        self.active_scope_slots = execution.active_scope_slots;
        self.with_objects = execution.with_objects;
        self.pending_completions = execution.pending_completions;
        self.completion_saves = execution.completion_saves;
        self.remaining_instructions = execution.remaining_instructions;
        self.cells = execution.cells;
        self.dynamic_eval_bindings = execution.dynamic_eval_bindings;
        self.eval_dynamic_slots = execution.eval_dynamic_slots;
        self.dynamic_eval_outer_bindings = execution.dynamic_eval_outer_bindings;
        self.this = execution.this;
        self.arguments = execution.arguments;
        self.callee = execution.callee;
        self.strict = execution.strict;
        self.top_level_module = execution.top_level_module;
        self.script_global_slots = execution.script_global_slots;
        self.variable_scope = execution.variable_scope;
        self.variable_scope_lexicals = execution.variable_scope_lexicals;
        self.templates = execution.templates;
        self.new_target = execution.new_target;
        self.new_target_allowed = execution.new_target_allowed;
        self.home_object = execution.home_object;
        self.class_constructor = execution.class_constructor;
        self.class_field_initializer_depth = execution.class_field_initializer_depth;
        self.active_module_name = execution.active_module_name;
    }

    /// Heap edges held only by a displaced interpreter frame.  Keeping this
    /// independent from the module machinery lets ordinary async functions
    /// share the same GC contract and prevents continuation state from being
    /// accidentally treated as Rust-only data.
    fn suspended_execution_references(execution: &SuspendedModuleExecution) -> Vec<ObjectId> {
        let mut references = Vec::new();
        let mut add_value = |value: &Value| {
            if let Some(id) = value.object_id() {
                references.push(id);
            }
        };
        for value in execution
            .stack
            .iter()
            .chain(execution.bindings.iter().flatten())
            .chain(std::iter::once(&execution.completion))
            .chain(execution.with_objects.iter())
            .chain(std::iter::once(&execution.this))
            .chain(execution.arguments.iter())
            .chain(std::iter::once(&execution.callee))
            .chain(std::iter::once(&execution.new_target))
            .chain(execution.completion_saves.iter().map(|(value, _)| value))
        {
            add_value(value);
        }
        for completion in &execution.pending_completions {
            match completion {
                Completion::Return(value)
                | Completion::Yield(value)
                | Completion::Throw(RuntimeError::Thrown(value)) => add_value(value),
                Completion::TailRecur(values) => {
                    for value in values {
                        add_value(value);
                    }
                }
                Completion::Throw(_)
                | Completion::Jump { .. }
                | Completion::Resume(_)
                | Completion::Halt(_) => {}
            }
        }
        references.extend(execution.cells.values().copied());
        for binding in execution.dynamic_eval_bindings.values().chain(
            execution
                .dynamic_eval_outer_bindings
                .iter()
                .flat_map(|bindings| bindings.values()),
        ) {
            references.push(binding.cell);
            references.extend(binding.shadowed_cells.iter().copied());
        }
        references.extend(execution.templates.values().copied());
        references.extend(
            [execution.home_object, execution.class_constructor]
                .into_iter()
                .flatten(),
        );
        references
    }

    fn continuation_references(&self) -> Vec<ObjectId> {
        let mut references = Vec::new();
        for continuation in self.module_continuations.values() {
            references.extend(Self::suspended_execution_references(
                &continuation.execution,
            ));
            references.extend(continuation.iterators.iter().filter_map(Value::object_id));
        }
        for continuation in self.async_continuations.values() {
            references.extend(continuation.generator);
            references.push(continuation.target);
            references.extend(Self::suspended_execution_references(
                &continuation.execution,
            ));
            references.extend(continuation.iterators.iter().filter_map(Value::object_id));
        }
        references
    }

    fn root_suspended_module_execution(
        &mut self,
        execution: &SuspendedModuleExecution,
    ) -> Result<Vec<RootId>, RuntimeError> {
        let mut roots = Vec::new();
        let registration: Result<(), HeapError> = (|| {
            for id in Self::suspended_execution_references(execution) {
                roots.push(self.heap.root(id)?);
            }
            Ok(())
        })();
        if let Err(error) = registration {
            for root in roots {
                self.heap.unroot(root)?;
            }
            return Err(error.into());
        }
        Ok(roots)
    }

    fn run_next_job_while_module_suspended(&mut self) -> Result<bool, RuntimeError> {
        let execution = self.suspend_module_execution();
        let roots = self.root_suspended_module_execution(&execution)?;
        // Promise jobs run in a fresh ECMAScript execution context.  In
        // particular, a dynamic-import job can enter a module whose
        // top-level `await` must not inherit the suspended caller's function
        // depth; otherwise it is mistaken for an ordinary async-function
        // await and cannot install a module continuation.
        let call_depth = std::mem::replace(&mut self.call_depth, 0);
        // Jobs execute in their own execution contexts. The suspended
        // module's remaining interpreter fuel must not leave every queued
        // reaction with a zero budget after state displacement.
        self.remaining_instructions = self.config.instruction_budget;
        let result = self.run_next_promise_job();
        // The nested graph's normal completion is not the outer module's
        // completion. Its namespace has its own dedicated cache root.
        if let Some(root) = self.result_root.take() {
            self.heap.unroot(root)?;
        }
        self.restore_module_execution(execution);
        self.call_depth = call_depth;
        for root in roots {
            self.heap.unroot(root)?;
        }
        result
    }

    fn resume_module_await(
        &mut self,
        continuation: u64,
        value: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        let continuation =
            self.module_continuations
                .remove(&continuation)
                .ok_or(RuntimeError::Unsupported(
                    "unknown module await continuation",
                ))?;
        let ModuleContinuation {
            module,
            code,
            pc,
            execution,
            mut iterators,
            mut handlers,
        } = continuation;
        self.restore_module_execution(execution);
        let outcome = if fulfilled {
            self.interpret(
                &code,
                &mut iterators,
                pc,
                Some(value),
                None,
                Some((handlers, 0)),
            )
        } else {
            match self.resolve_completion(
                &code,
                &mut handlers,
                &mut iterators,
                Completion::Throw(RuntimeError::Thrown(value)),
            )? {
                CompletionAction::Continue => {
                    self.interpret(&code, &mut iterators, pc, None, None, Some((handlers, 0)))
                }
                CompletionAction::Jump(target) => self.interpret(
                    &code,
                    &mut iterators,
                    target,
                    None,
                    None,
                    Some((handlers, 0)),
                ),
                CompletionAction::Return(value) => Ok(InterpreterExit::Return(value)),
                CompletionAction::TailRecur(_) => Err(RuntimeError::TypeError(
                    "top-level await cannot recur".into(),
                )),
                CompletionAction::Throw(error) => Err(error),
            }
        };
        let mut graph = self
            .module_graph
            .take()
            .ok_or(RuntimeError::Unsupported("module graph continuation"))?;
        let result = match outcome {
            Ok(InterpreterExit::Return(value)) => {
                {
                    let record = graph
                        .linked
                        .get_mut(&module)
                        .expect("suspended module remains linked");
                    record.cells = std::mem::take(&mut self.cells);
                    record.evaluating = false;
                    record.suspended = false;
                    record.evaluated = true;
                    record.completion = Some(value.clone());
                }
                let modules = self.module_registry.clone();
                self.settle_dynamic_import_waiters(&module)?;
                self.settle_module_parents(module.clone(), &modules, &mut graph.linked)?;
                Ok(value)
            }
            Ok(InterpreterExit::Await {
                promise,
                pc,
                handlers,
            }) => {
                let execution = self.suspend_module_execution();
                let record = graph
                    .linked
                    .get_mut(&module)
                    .expect("suspended module remains linked");
                record.cells = execution.cells.clone();
                record.suspended = true;
                self.suspend_module_await(
                    ModuleContinuation {
                        module: module.clone(),
                        code,
                        pc,
                        execution,
                        iterators,
                        handlers,
                    },
                    promise,
                )?;
                Ok(Value::Undefined)
            }
            Ok(InterpreterExit::Yield { .. }) => Err(RuntimeError::TypeError(
                "yield requires a generator function".into(),
            )),
            Ok(InterpreterExit::Suspend { .. }) => {
                unreachable!("module execution has no generator suspend boundary")
            }
            Err(error) => {
                let error = self.error_value(error)?;
                let record = graph
                    .linked
                    .get_mut(&module)
                    .expect("suspended module remains linked");
                record.cells = std::mem::take(&mut self.cells);
                record.evaluating = false;
                record.suspended = false;
                record.evaluated = true;
                record.completion = None;
                record.error = Some(error.clone());
                self.reject_module_and_parents(module, error, &mut graph.linked)?;
                Ok(Value::Undefined)
            }
        };
        self.module_graph = Some(graph);
        result.map(|_| ())
    }

    /// Completing an async dependency releases the modules that were waiting
    /// on it during InnerModuleEvaluation. Process one breadth-first layer at
    /// a time: siblings retain source/DFS order, while an indirect parent is
    /// considered only after its direct siblings have had their turn.
    fn settle_module_parents(
        &mut self,
        completed: String,
        modules: &HashMap<String, Bytecode>,
        linked: &mut HashMap<String, LinkedModule>,
    ) -> Result<(), RuntimeError> {
        let mut completed = std::collections::VecDeque::from([completed]);
        while let Some(child) = completed.pop_front() {
            let parents = self.module_async_parents.remove(&child).unwrap_or_default();
            for parent in parents {
                let ready = self
                    .module_pending_dependencies
                    .get_mut(&parent)
                    .is_some_and(|dependencies| {
                        dependencies.remove(&child);
                        dependencies.is_empty()
                    });
                if !ready {
                    continue;
                }
                self.module_pending_dependencies.remove(&parent);
                let record = linked
                    .get_mut(&parent)
                    .expect("async parent remains linked");
                record.evaluating = false;
                record.suspended = false;
                self.evaluate_module_record(&parent, modules, linked)?;
                if linked.get(&parent).is_some_and(|record| record.evaluated) {
                    self.settle_dynamic_import_waiters(&parent)?;
                    completed.push_back(parent);
                }
            }
        }
        Ok(())
    }

    /// Propagate an async module evaluation error through its static parents.
    /// Dynamic imports observe rejection from each affected module, while the
    /// surrounding promise checkpoint continues processing independent jobs.
    fn reject_module_and_parents(
        &mut self,
        module: String,
        error: Value,
        linked: &mut HashMap<String, LinkedModule>,
    ) -> Result<(), RuntimeError> {
        let mut rejected = std::collections::VecDeque::from([module]);
        while let Some(module) = rejected.pop_front() {
            self.reject_dynamic_import_waiters(&module, error.clone())?;
            for parent in self
                .module_async_parents
                .remove(&module)
                .unwrap_or_default()
            {
                self.module_pending_dependencies.remove(&parent);
                let record = linked
                    .get_mut(&parent)
                    .expect("async parent remains linked");
                if record.error.is_some() {
                    continue;
                }
                record.evaluating = false;
                record.suspended = false;
                record.evaluated = true;
                record.completion = None;
                record.error = Some(error.clone());
                rejected.push_back(parent);
            }
        }
        Ok(())
    }

    fn settle_dynamic_import_waiters(&mut self, module: &str) -> Result<(), RuntimeError> {
        let Some(waiters) = self.module_import_waiters.remove(module) else {
            return Ok(());
        };
        let namespace = self.module_namespace_cache.get(module).copied().ok_or(
            RuntimeError::ModuleResolution(format!(
                "dynamic import of {module} did not produce a namespace"
            )),
        )?;
        for promise in waiters {
            self.settle_promise(promise, PromiseStatus::Fulfilled(Value::Object(namespace)))?;
        }
        Ok(())
    }

    fn reject_dynamic_import_waiters(
        &mut self,
        module: &str,
        error: Value,
    ) -> Result<(), RuntimeError> {
        let Some(waiters) = self.module_import_waiters.remove(module) else {
            return Ok(());
        };
        for promise in waiters {
            self.settle_promise(promise, PromiseStatus::Rejected(error.clone()))?;
        }
        Ok(())
    }

    fn suspend_module_await(
        &mut self,
        continuation_state: ModuleContinuation,
        promise: ObjectId,
    ) -> Result<(), RuntimeError> {
        let continuation = self.next_module_continuation;
        self.next_module_continuation = self
            .next_module_continuation
            .checked_add(1)
            .ok_or(RuntimeError::InstructionLimit)?;
        self.module_continuations
            .insert(continuation, continuation_state);
        let status = self
            .promises
            .get(&promise)
            .ok_or(RuntimeError::TypeError("invalid await Promise".into()))?
            .status
            .clone_for_await();
        match status {
            PromiseAwaitStatus::Pending => self
                .promises
                .get_mut(&promise)
                .expect("checked await Promise exists")
                .reactions
                .push(PromiseReaction::ModuleAwait { continuation }),
            PromiseAwaitStatus::Fulfilled(value) => {
                self.promise_jobs.push_back(PromiseJob::ModuleAwait {
                    continuation,
                    value,
                    fulfilled: true,
                });
            }
            PromiseAwaitStatus::Rejected(value) => {
                self.promise_jobs.push_back(PromiseJob::ModuleAwait {
                    continuation,
                    value,
                    fulfilled: false,
                });
            }
        }
        Ok(())
    }

    /// Registers an ordinary async-function frame on the Promise it awaits.
    /// A fulfilled input still goes through the job queue, preserving the
    /// required asynchronous boundary before the frame resumes.
    fn suspend_async_await(
        &mut self,
        continuation_state: AsyncContinuation,
        promise: ObjectId,
    ) -> Result<(), RuntimeError> {
        let continuation = self.next_async_continuation;
        self.next_async_continuation = self
            .next_async_continuation
            .checked_add(1)
            .ok_or(RuntimeError::InstructionLimit)?;
        self.async_continuations
            .insert(continuation, continuation_state);
        let status = self
            .promises
            .get(&promise)
            .ok_or(RuntimeError::TypeError("invalid await Promise".into()))?
            .status
            .clone_for_await();
        match status {
            PromiseAwaitStatus::Pending => self
                .promises
                .get_mut(&promise)
                .expect("checked await Promise exists")
                .reactions
                .push(PromiseReaction::AsyncAwait { continuation }),
            PromiseAwaitStatus::Fulfilled(value) => {
                self.promise_jobs.push_back(PromiseJob::AsyncAwait {
                    continuation,
                    value,
                    fulfilled: true,
                });
            }
            PromiseAwaitStatus::Rejected(value) => {
                self.promise_jobs.push_back(PromiseJob::AsyncAwait {
                    continuation,
                    value,
                    fulfilled: false,
                });
            }
        }
        Ok(())
    }

    /// Continues a suspended ordinary async function in its own Promise job
    /// execution context.  The ambient VM state may itself be a suspended
    /// module or a reaction handler, so it is displaced before the async
    /// frame is restored and put back unchanged after this turn.
    fn resume_async_await(
        &mut self,
        continuation: u64,
        value: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        let continuation =
            self.async_continuations
                .remove(&continuation)
                .ok_or(RuntimeError::Unsupported(
                    "unknown async await continuation",
                ))?;
        let AsyncContinuation {
            generator,
            target,
            code,
            pc,
            execution,
            mut iterators,
            mut handlers,
            call_depth,
        } = continuation;
        if let Some(generator) = generator {
            return self.resume_async_generator_await(
                generator, target, code, pc, execution, iterators, handlers, call_depth, value,
                fulfilled,
            );
        }
        let mut ambient = self.suspend_module_execution();
        let ambient_call_depth = std::mem::replace(&mut self.call_depth, call_depth);
        self.restore_module_execution(execution);
        let outcome = if fulfilled {
            self.interpret(
                &code,
                &mut iterators,
                pc,
                Some(value),
                None,
                Some((handlers, 0)),
            )
        } else {
            match self.resolve_completion(
                &code,
                &mut handlers,
                &mut iterators,
                Completion::Throw(RuntimeError::Thrown(value)),
            ) {
                Ok(CompletionAction::Continue) => {
                    self.interpret(&code, &mut iterators, pc, None, None, Some((handlers, 0)))
                }
                Ok(CompletionAction::Jump(target)) => self.interpret(
                    &code,
                    &mut iterators,
                    target,
                    None,
                    None,
                    Some((handlers, 0)),
                ),
                Ok(CompletionAction::Return(value)) => Ok(InterpreterExit::Return(value)),
                Ok(CompletionAction::TailRecur(_)) => Err(RuntimeError::TypeError(
                    "async function cannot tail recur across await".into(),
                )),
                Ok(CompletionAction::Throw(error)) | Err(error) => Err(error),
            }
        };

        let result = match outcome {
            Ok(InterpreterExit::Return(value)) => {
                ambient.templates.extend(self.templates.clone());
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                self.resolve_promise(target, value)
            }
            Ok(InterpreterExit::Await {
                promise,
                pc,
                handlers,
            }) => {
                let execution = self.suspend_module_execution();
                ambient.templates.extend(execution.templates.clone());
                let state = AsyncContinuation {
                    generator,
                    target,
                    code,
                    pc,
                    execution,
                    iterators,
                    handlers,
                    call_depth: self.call_depth,
                };
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                self.suspend_async_await(state, promise)
            }
            Ok(InterpreterExit::Yield { .. }) => {
                ambient.templates.extend(self.templates.clone());
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                Err(RuntimeError::TypeError(
                    "yield requires an async generator function".into(),
                ))
            }
            Ok(InterpreterExit::Suspend { .. }) => {
                unreachable!("async functions have no entry suspend")
            }
            Err(error) => {
                // Iterator records are part of the suspended frame rather
                // than ordinary heap properties. Root them while abrupt
                // cleanup invokes user-provided `return` methods.
                let base = self.stack.len();
                if let RuntimeError::Thrown(value) = &error {
                    self.stack.push(value.clone());
                }
                self.stack.extend(iterators.iter().cloned());
                for record in iterators.into_iter().rev() {
                    let _ = self.iterator_close(&record);
                }
                self.stack.truncate(base);
                ambient.templates.extend(self.templates.clone());
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                let error = self.error_value(error)?;
                self.settle_promise(target, PromiseStatus::Rejected(error))
            }
        };
        result
    }

    /// Resume one pending async-generator request. Its frame is identical to
    /// an ordinary async continuation, but a `yield` settles the request and
    /// keeps the generator resumable instead of resolving a function call.
    #[allow(clippy::too_many_arguments)]
    fn resume_async_generator_await(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        code: Rc<Bytecode>,
        pc: usize,
        execution: SuspendedModuleExecution,
        mut iterators: Vec<Value>,
        mut handlers: Vec<HandlerFrame>,
        call_depth: usize,
        value: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        let mut ambient = self.suspend_module_execution();
        let ambient_call_depth = std::mem::replace(&mut self.call_depth, call_depth);
        self.restore_module_execution(execution);
        let outcome = if fulfilled {
            self.interpret(
                &code,
                &mut iterators,
                pc,
                Some(value),
                None,
                Some((handlers, 0)),
            )
        } else {
            match self.resolve_completion(
                &code,
                &mut handlers,
                &mut iterators,
                Completion::Throw(RuntimeError::Thrown(value)),
            ) {
                Ok(CompletionAction::Continue) => {
                    self.interpret(&code, &mut iterators, pc, None, None, Some((handlers, 0)))
                }
                Ok(CompletionAction::Jump(target)) => self.interpret(
                    &code,
                    &mut iterators,
                    target,
                    None,
                    None,
                    Some((handlers, 0)),
                ),
                Ok(CompletionAction::Return(value)) => Ok(InterpreterExit::Return(value)),
                Ok(CompletionAction::TailRecur(_)) => Err(RuntimeError::TypeError(
                    "async generator cannot tail recur across await".into(),
                )),
                Ok(CompletionAction::Throw(error)) | Err(error) => Err(error),
            }
        };

        match outcome {
            Ok(InterpreterExit::Return(value)) => {
                self.heap
                    .set_generator_state(generator, GeneratorState::Done)?;
                ambient.templates.extend(self.templates.clone());
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                let result = self.iterator_result(value, true)?;
                self.await_async_generator_yield(generator, target, result)
            }
            Ok(InterpreterExit::Yield {
                value,
                pc,
                iterators,
                handlers,
            }) => {
                let stack = std::mem::take(&mut self.stack);
                let async_delegate =
                    code.async_yield_delegates
                        .iter()
                        .find(|(resume, _)| *resume as usize == pc)
                        .and_then(|(_, exit_pc)| {
                            stack.last().cloned().map(|record| {
                                crate::heap::AsyncGeneratorDelegate {
                                    record,
                                    exit_pc: *exit_pc as usize,
                                }
                            })
                        });
                let state = GeneratorState::Suspended {
                    code,
                    pc,
                    stack,
                    bindings: std::mem::take(&mut self.bindings),
                    cells: std::mem::take(&mut self.cells).into_iter().collect(),
                    this: std::mem::replace(&mut self.this, Value::Undefined),
                    args: std::mem::take(&mut self.arguments),
                    completion: std::mem::replace(&mut self.completion, Value::Undefined),
                    completion_empty: std::mem::replace(&mut self.completion_empty, true),
                    active_scopes: std::mem::take(&mut self.active_scopes),
                    iterators,
                    handlers,
                    pending_completions: std::mem::take(&mut self.pending_completions)
                        .into_iter()
                        .map(|completion| {
                            completion
                                .into_generator_pending()
                                .expect("only catchable completions can survive a generator yield")
                        })
                        .collect(),
                    completion_saves: std::mem::take(&mut self.completion_saves),
                    async_delegate,
                    dynamic_bindings: std::mem::take(&mut self.dynamic_eval_bindings)
                        .into_iter()
                        .map(|(name, binding)| (name, binding.cell, binding.shadowed_cells))
                        .collect(),
                    home: std::mem::take(&mut self.home_object),
                    callee: std::mem::replace(&mut self.callee, Value::Undefined),
                };
                self.heap.set_generator_state(generator, state)?;
                ambient.templates.extend(self.templates.clone());
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                let result = self.iterator_result(value, false)?;
                self.await_async_generator_yield(generator, target, result)
            }
            Ok(InterpreterExit::Await {
                promise,
                pc,
                handlers,
            }) => {
                let execution = self.suspend_module_execution();
                ambient.templates.extend(execution.templates.clone());
                let state = AsyncContinuation {
                    generator: Some(generator),
                    target,
                    code,
                    pc,
                    execution,
                    iterators,
                    handlers,
                    call_depth: self.call_depth,
                };
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                self.suspend_async_await(state, promise)
            }
            Ok(InterpreterExit::Suspend { .. }) => {
                unreachable!("async-generator resumption has no entry suspend")
            }
            Err(error) => {
                let base = self.stack.len();
                if let RuntimeError::Thrown(value) = &error {
                    self.stack.push(value.clone());
                }
                self.stack.extend(iterators.iter().cloned());
                for record in iterators.into_iter().rev() {
                    let _ = self.iterator_close(&record);
                }
                self.stack.truncate(base);
                self.heap
                    .set_generator_state(generator, GeneratorState::Done)?;
                ambient.templates.extend(self.templates.clone());
                self.restore_module_execution(ambient);
                self.call_depth = ambient_call_depth;
                let value = self.error_value(error)?;
                self.complete_async_generator_request(
                    generator,
                    target,
                    PromiseStatus::Rejected(value),
                )?;
                self.resume_async_generator_next(generator)
            }
        }
    }

    fn resolve_export(
        modules: &HashMap<String, Bytecode>,
        module: &str,
        export_name: &str,
        resolve_set: &mut Vec<(String, String)>,
    ) -> Result<ExportResolution, RuntimeError> {
        let pair = (module.to_string(), export_name.to_string());
        if resolve_set.contains(&pair) {
            return Ok(ExportResolution::Missing);
        }
        resolve_set.push(pair);
        let result = (|| {
            let code = modules.get(module).ok_or_else(|| {
                RuntimeError::ModuleResolution(format!(
                    "module {module} was not supplied by the host"
                ))
            })?;
            for export in &code.module_exports {
                match export {
                    ModuleExport::Local {
                        export_name: name,
                        local_slot,
                    } if name == export_name => {
                        return Ok(ExportResolution::Binding {
                            module: module.to_string(),
                            slot: *local_slot as usize,
                        });
                    }
                    ModuleExport::Indirect {
                        export_name: name,
                        module_request,
                        import_name,
                    } if name == export_name => {
                        let target = Self::resolve_module_request(module, module_request)?;
                        return Self::resolve_export(modules, &target, import_name, resolve_set);
                    }
                    ModuleExport::Namespace {
                        export_name: name,
                        module_request,
                    } if name == export_name => {
                        return Ok(ExportResolution::Namespace {
                            module: Self::resolve_module_request(module, module_request)?,
                        });
                    }
                    ModuleExport::Source {
                        export_name: name,
                        module_request,
                    } if name == export_name => {
                        return Ok(ExportResolution::Source {
                            module: Self::resolve_module_request(module, module_request)?,
                        });
                    }
                    _ => {}
                }
            }
            if export_name == "default" {
                return Ok(ExportResolution::Missing);
            }
            let mut candidate = ExportResolution::Missing;
            for export in &code.module_exports {
                let ModuleExport::Star { module_request } = export else {
                    continue;
                };
                let target = Self::resolve_module_request(module, module_request)?;
                match Self::resolve_export(modules, &target, export_name, resolve_set)? {
                    ExportResolution::Missing => {}
                    ExportResolution::Ambiguous => return Ok(ExportResolution::Ambiguous),
                    found @ (ExportResolution::Binding { .. }
                    | ExportResolution::Namespace { .. }
                    | ExportResolution::Source { .. }) => {
                        if candidate == ExportResolution::Missing {
                            candidate = found;
                        } else if candidate != found {
                            return Ok(ExportResolution::Ambiguous);
                        }
                    }
                }
            }
            Ok(candidate)
        })();
        resolve_set.pop();
        result
    }

    fn exported_names(
        modules: &HashMap<String, Bytecode>,
        module: &str,
        star_set: &mut HashSet<String>,
    ) -> Result<BTreeSet<String>, RuntimeError> {
        if !star_set.insert(module.to_string()) {
            return Ok(BTreeSet::new());
        }
        let result = (|| {
            let code = modules.get(module).ok_or_else(|| {
                RuntimeError::ModuleResolution(format!(
                    "module {module} was not supplied by the host"
                ))
            })?;
            let mut names = BTreeSet::new();
            for export in &code.module_exports {
                match export {
                    ModuleExport::Local { export_name, .. }
                    | ModuleExport::Indirect { export_name, .. }
                    | ModuleExport::Namespace { export_name, .. }
                    | ModuleExport::Source { export_name, .. } => {
                        names.insert(export_name.clone());
                    }
                    ModuleExport::Star { module_request } => {
                        let target = Self::resolve_module_request(module, module_request)?;
                        names.extend(
                            Self::exported_names(modules, &target, star_set)?
                                .into_iter()
                                .filter(|name| name != "default"),
                        );
                    }
                }
            }
            Ok(names)
        })();
        star_set.remove(module);
        result
    }

    fn module_namespace(
        &mut self,
        module: &str,
        modules: &HashMap<String, Bytecode>,
        linked: &mut HashMap<String, LinkedModule>,
        roots: &mut Vec<RootId>,
    ) -> Result<ObjectId, RuntimeError> {
        if let Some(namespace) = self.module_namespace_cache.get(module) {
            return Ok(*namespace);
        }
        if let Some(namespace) = linked
            .get(module)
            .ok_or_else(|| {
                RuntimeError::ModuleResolution(format!("module {module} was not linked"))
            })?
            .namespace
        {
            return Ok(namespace);
        }
        let names = Self::exported_names(modules, module, &mut HashSet::new())?;
        // Publish an identity-stable placeholder before resolving namespace
        // exports. A module may re-export its own namespace, directly or
        // through a cycle; waiting until after recursive resolution would
        // recurse indefinitely and overflow the host stack.
        let namespace = self.with_roots(|heap| heap.alloc_module_namespace(Vec::new()))?;
        roots.push(self.heap.root(namespace)?);
        linked
            .get_mut(module)
            .expect("linked module record exists")
            .namespace = Some(namespace);
        let cache_root = self.heap.root(namespace)?;
        self.module_namespace_cache
            .insert(module.to_string(), namespace);
        self.module_namespace_roots
            .insert(module.to_string(), cache_root);
        let mut exports = Vec::with_capacity(names.len());
        for name in names {
            let resolution = Self::resolve_export(modules, module, &name, &mut Vec::new())?;
            let cell = match resolution {
                ExportResolution::Binding {
                    module: exporter,
                    slot,
                } => {
                    let cell = linked
                        .get(&exporter)
                        .and_then(|record| record.cells.get(&slot))
                        .copied()
                        .ok_or_else(|| {
                            RuntimeError::ModuleResolution(format!(
                                "export {name} from {exporter} has no binding"
                            ))
                        })?;
                    cell
                }
                ExportResolution::Namespace { module } => {
                    // Namespace exports still need a binding cell: namespace
                    // exotic properties are live bindings uniformly, and this
                    // one is an immutable binding to the target namespace.
                    let value =
                        Value::Object(self.module_namespace(&module, modules, linked, roots)?);
                    let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                    roots.push(self.heap.root(cell)?);
                    self.with_roots(|heap| heap.set(cell, "value", value))?;
                    cell
                }
                ExportResolution::Source { module } => {
                    let value = Value::Object(self.module_source_object(&module, modules)?);
                    let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                    roots.push(self.heap.root(cell)?);
                    self.with_roots(|heap| heap.set(cell, "value", value))?;
                    cell
                }
                ExportResolution::Missing | ExportResolution::Ambiguous => continue,
            };
            exports.push((name.into(), cell));
        }
        self.with_roots(|heap| heap.initialize_module_namespace(namespace, exports))?;
        Ok(namespace)
    }

    fn enter_module_record(&mut self, code: &Bytecode, cells: HashMap<usize, ObjectId>) {
        self.stack.clear();
        self.bindings = vec![None; code.bindings.len()];
        self.binding_metadata = code.bindings.clone();
        self.cells = cells;
        self.completion = Value::Undefined;
        self.completion_empty = true;
        self.active_scopes.clear();
        self.active_scope_slots.clear();
        self.with_objects.clear();
        self.script_global_slots.clear();
        self.strict = true;
        self.this = Value::Undefined;
        self.top_level_module = true;
    }

    fn initialize_module_record(
        &mut self,
        code: &Bytecode,
        cells: &mut HashMap<usize, ObjectId>,
    ) -> Result<(), RuntimeError> {
        let entry = code.module_evaluate_entry.ok_or(RuntimeError::Unsupported(
            "module declaration instantiation",
        ))? as usize;
        self.enter_module_record(code, std::mem::take(cells));
        let mut iterators = Vec::new();
        let result = self.interpret(code, &mut iterators, 0, None, Some(entry), None);
        *cells = std::mem::take(&mut self.cells);
        self.stack.clear();
        self.active_scopes.clear();
        self.active_scope_slots.clear();
        match result? {
            InterpreterExit::Suspend { pc } if pc == entry => Ok(()),
            _ => unreachable!("module declaration prefix always suspends at its evaluation entry"),
        }
    }

    /// Returns whether a requested-module edge can reach `goal`.  This small
    /// graph walk is used only while wiring async parents, after linking has
    /// already validated every request.
    fn module_reaches(
        from: &str,
        goal: &str,
        modules: &HashMap<String, Bytecode>,
        visited: &mut HashSet<String>,
    ) -> Result<bool, RuntimeError> {
        if from == goal {
            return Ok(true);
        }
        if !visited.insert(from.to_string()) {
            return Ok(false);
        }
        let code = modules.get(from).ok_or_else(|| {
            RuntimeError::ModuleResolution(format!("module {from} was not linked"))
        })?;
        for request in &code.module_requests {
            let target = Self::resolve_module_request(from, &request.module_request)?;
            if Self::module_reaches(&target, goal, modules, visited)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// InnerModuleEvaluation attaches an importer to a suspended dependency's
    /// cycle root, not necessarily the particular module that appeared in
    /// the import declaration.  Without this distinction an outside importer
    /// can resume between a cycle leaf and its root's own top-level await.
    fn async_dependency_root(
        &self,
        dependency: &str,
        modules: &HashMap<String, Bytecode>,
        linked: &HashMap<String, LinkedModule>,
    ) -> Result<String, RuntimeError> {
        for candidate in self
            .module_async_parents
            .get(dependency)
            .into_iter()
            .flatten()
        {
            if linked
                .get(candidate)
                .is_some_and(|record| record.evaluating || record.suspended)
                && Self::module_reaches(dependency, candidate, modules, &mut HashSet::new())?
            {
                return Ok(candidate.clone());
            }
        }
        Ok(dependency.to_string())
    }

    fn evaluate_module_record(
        &mut self,
        name: &str,
        modules: &HashMap<String, Bytecode>,
        linked: &mut HashMap<String, LinkedModule>,
    ) -> Result<Value, RuntimeError> {
        let Some(record) = linked.get(name) else {
            return Err(RuntimeError::ModuleResolution(format!(
                "module {name} was not linked"
            )));
        };
        if let Some(error) = &record.error {
            return Err(RuntimeError::Thrown(error.clone()));
        }
        if record.evaluated || record.evaluating {
            return Ok(Value::Undefined);
        }
        linked
            .get_mut(name)
            .expect("checked module record exists")
            .evaluating = true;
        let result = (|| {
            let code = modules.get(name).expect("linked module has bytecode");
            let mut pending_dependencies = Vec::new();
            for request in &code.module_requests {
                let target = Self::resolve_module_request(name, &request.module_request)?;
                self.evaluate_module_record(&target, modules, linked)?;
                if linked.get(&target).is_some_and(|record| record.suspended) {
                    pending_dependencies
                        .push(self.async_dependency_root(&target, modules, linked)?);
                }
            }
            if !pending_dependencies.is_empty() {
                let dependencies = self
                    .module_pending_dependencies
                    .entry(name.to_string())
                    .or_default();
                for dependency in pending_dependencies {
                    if dependencies.insert(dependency.clone()) {
                        self.module_async_parents
                            .entry(dependency)
                            .or_default()
                            .push(name.to_string());
                    }
                }
                linked
                    .get_mut(name)
                    .expect("checked module record exists")
                    .suspended = true;
                return Ok(Value::Undefined);
            }
            let entry = code.module_evaluate_entry.ok_or(RuntimeError::Unsupported(
                "module declaration instantiation",
            ))? as usize;
            let cells = std::mem::take(
                &mut linked
                    .get_mut(name)
                    .expect("checked module record exists")
                    .cells,
            );
            self.enter_module_record(code, cells);
            // A sibling Source Text Module runs in a fresh execution context.
            // A suspended async dependency deliberately leaves its displaced
            // frame with zero ambient fuel, which must not exhaust this
            // independent module before its first instruction.
            self.remaining_instructions = self.config.instruction_budget;
            self.active_scopes.push(0);
            self.active_scope_slots
                .push(code.scopes.first().cloned().unwrap_or_default());
            let mut iterators = Vec::new();
            let previous_module = self.active_module_name.replace(name.to_string());
            let outcome = self.interpret(code, &mut iterators, entry, None, None, None);
            match outcome {
                Ok(InterpreterExit::Return(value)) => {
                    self.active_module_name = previous_module;
                    let cells = std::mem::take(&mut self.cells);
                    let record = linked.get_mut(name).expect("checked module record exists");
                    record.cells = cells;
                    record.completion = Some(value.clone());
                    Ok(value)
                }
                Ok(InterpreterExit::Await {
                    promise,
                    pc,
                    handlers,
                }) => {
                    // Move the realm frame out before another sibling module
                    // evaluates. The continuation is attached to the Await
                    // promise and resumes in its own Promise job turn.
                    let execution = self.suspend_module_execution();
                    self.active_module_name = previous_module;
                    let record = linked.get_mut(name).expect("checked module record exists");
                    record.cells = execution.cells.clone();
                    record.suspended = true;
                    self.suspend_module_await(
                        ModuleContinuation {
                            module: name.to_string(),
                            code: code.clone(),
                            pc,
                            execution,
                            iterators,
                            handlers,
                        },
                        promise,
                    )?;
                    Ok(Value::Undefined)
                }
                Ok(InterpreterExit::Yield { .. }) => Err(RuntimeError::TypeError(
                    "yield requires a generator function".into(),
                )),
                Ok(InterpreterExit::Suspend { .. }) => {
                    unreachable!("module evaluation does not suspend")
                }
                Err(error) => {
                    self.active_module_name = previous_module;
                    let cells = std::mem::take(&mut self.cells);
                    let record = linked.get_mut(name).expect("checked module record exists");
                    record.cells = cells;
                    Err(error)
                }
            }
        })();
        let record = linked.get_mut(name).expect("checked module record exists");
        if !record.suspended {
            record.evaluating = false;
        }
        if result.is_ok() && !record.suspended {
            record.evaluated = true;
        }
        result
    }

    fn execute_with_global_bindings(
        &mut self,
        code: &Bytecode,
        publish_globals: bool,
        module: bool,
    ) -> Result<Value, RuntimeError> {
        if let Some(root) = self.result_root.take() {
            self.heap.unroot(root)?;
        }
        self.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })?;
        self.bindings.resize(code.bindings.len(), None);
        self.binding_metadata = code.bindings.clone();
        self.remaining_instructions = self.config.instruction_budget;
        self.strict = code.strict;
        self.top_level_module = module;
        self.completion_empty = true;
        self.active_scopes.clear();
        self.active_scope_slots.clear();
        self.with_objects.clear();
        self.script_global_slots.clear();
        self.dynamic_eval_bindings.clear();
        self.eval_dynamic_slots.clear();
        self.dynamic_eval_outer_bindings.clear();
        if publish_globals {
            self.prepare_global_declarations(code)?;
        }
        // `this` lazily materializes the realm global only when script code
        // actually observes it. This keeps data-only executions within small
        // heap configurations while preserving script and arrow semantics.
        self.this = Value::Undefined;
        self.class_field_initializer_depth = 0;
        let result = self.run(code).and_then(|value| {
            if let Value::Object(id) = value {
                self.result_root = Some(self.heap.root(id)?);
            }
            Ok(value)
        });
        if let Err(RuntimeError::Thrown(Value::Object(id))) = &result {
            self.result_root = Some(self.heap.root(*id)?);
        }
        self.stack.clear();
        self.bindings.clear();
        self.binding_metadata.clear();
        self.cells.clear();
        self.dynamic_eval_bindings.clear();
        self.eval_dynamic_slots.clear();
        self.dynamic_eval_outer_bindings.clear();
        self.script_global_slots.clear();
        self.completion = Value::Undefined;
        self.completion_empty = true;
        self.active_scopes.clear();
        self.active_scope_slots.clear();
        self.with_objects.clear();
        self.top_level_module = false;
        self.pending_completions.clear();
        self.completion_saves.clear();
        self.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })?;
        result
    }

    /// GlobalDeclarationInstantiation for this VM's implemented classic
    /// script subset. Validation happens before execution, while bindings are
    /// created before any initializer so an abrupt script still leaves the
    /// required persistent TDZ state in its realm.
    fn prepare_global_declarations(&mut self, code: &Bytecode) -> Result<(), RuntimeError> {
        let slots = code.scopes.first().cloned().unwrap_or_default();
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("globalThis is an object");

        for &slot in &slots {
            let binding = &code.bindings[slot as usize];
            if binding.lexical {
                self.materialize_lexical_global(global, &binding.name)?;
            }
        }

        for &slot in &slots {
            let binding = &code.bindings[slot as usize];
            let existing = self.global_bindings.get(&binding.name);
            if binding.lexical {
                if existing.is_some_and(|binding| !binding.property)
                    || self
                        .heap
                        .get_own_property_descriptor(global, binding.name.as_str())?
                        .is_some_and(|descriptor| descriptor.configurable == Some(false))
                {
                    return Err(RuntimeError::SyntaxError(format!(
                        "global binding {} cannot be redeclared",
                        binding.name
                    )));
                }
            } else if existing.is_some_and(|binding| !binding.property) {
                return Err(RuntimeError::SyntaxError(format!(
                    "global lexical binding {} conflicts with var declaration",
                    binding.name
                )));
            } else if code.global_function_names.contains(&binding.name) {
                if !self.can_declare_global_function(global, &binding.name)? {
                    return Err(RuntimeError::TypeError(format!(
                        "cannot declare global function {}",
                        binding.name
                    )));
                }
            } else if !self.can_declare_global_var(global, &binding.name)? {
                return Err(RuntimeError::TypeError(format!(
                    "cannot declare global var {}",
                    binding.name
                )));
            }
        }

        for &slot in &slots {
            let binding = &code.bindings[slot as usize];
            if !self.global_bindings.contains_key(&binding.name) {
                self.create_global_binding(
                    global,
                    binding,
                    code.global_function_names.contains(&binding.name),
                    false,
                )?;
            }
            self.script_global_slots
                .insert(slot as usize, binding.name.clone());
        }
        Ok(())
    }

    /// Standard global properties exist independently of a script lexical
    /// declaration that shadows them. Most intrinsics are otherwise lazy, so
    /// materialize only the property that a global lexical declaration needs
    /// to inspect or shadow.
    fn materialize_lexical_global(
        &mut self,
        global: ObjectId,
        name: &str,
    ) -> Result<(), RuntimeError> {
        let constant = match name {
            "undefined" => Some(Value::Undefined),
            "NaN" => Some(Value::Number(f64::NAN)),
            "Infinity" => Some(Value::Number(f64::INFINITY)),
            _ => None,
        };
        if let Some(value) = constant {
            if self
                .heap
                .get_own_property_descriptor(global, name)?
                .is_none()
            {
                self.define_data(global, name, value, false, false, false)?;
            }
            return Ok(());
        }
        if matches!(
            name,
            "String"
                | "Symbol"
                | "RegExp"
                | "Object"
                | "Reflect"
                | "Math"
                | "Number"
                | "Boolean"
                | "BigInt"
                | "Array"
                | "ArrayBuffer"
                | "DataView"
                | "Int8Array"
                | "Uint8Array"
                | "Uint8ClampedArray"
                | "Int16Array"
                | "Uint16Array"
                | "Int32Array"
                | "Uint32Array"
                | "Float32Array"
                | "Float64Array"
                | "Map"
                | "Set"
                | "Function"
                | "Intl"
                | "Error"
                | "TypeError"
                | "RangeError"
                | "SyntaxError"
                | "ReferenceError"
                | "EvalError"
                | "URIError"
                | "eval"
                | "isNaN"
                | "isFinite"
                | "parseInt"
                | "parseFloat"
                | "JSON"
        ) {
            self.global(name)?;
        }
        Ok(())
    }

    /// The realm global's intrinsic properties are created lazily. Property
    /// access must still observe their specified descriptors, even when the
    /// name did not first occur as an unqualified identifier.
    fn materialize_global_object_property(
        &mut self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Result<(), RuntimeError> {
        if self.globals.get("globalThis") != Some(&object) {
            return Ok(());
        }
        let PropertyName::String(name) = key else {
            return Ok(());
        };
        let Ok(name) = name.to_utf8() else {
            return Ok(());
        };
        self.materialize_lexical_global(object, &name)
    }

    fn can_declare_global_var(
        &mut self,
        global: ObjectId,
        name: &str,
    ) -> Result<bool, RuntimeError> {
        self.materialize_lexical_global(global, name)?;
        Ok(self
            .heap
            .get_own_property_descriptor(global, name)?
            .is_some()
            || self.heap.is_extensible(global)?)
    }

    fn can_declare_global_function(
        &mut self,
        global: ObjectId,
        name: &str,
    ) -> Result<bool, RuntimeError> {
        self.materialize_lexical_global(global, name)?;
        let Some(descriptor) = self.heap.get_own_property_descriptor(global, name)? else {
            return Ok(self.heap.is_extensible(global)?);
        };
        Ok(descriptor.configurable == Some(true)
            || (descriptor.value.is_some()
                && descriptor.writable == Some(true)
                && descriptor.enumerable == Some(true)))
    }

    fn create_global_binding(
        &mut self,
        global: ObjectId,
        binding: &Binding,
        function: bool,
        configurable: bool,
    ) -> Result<(), RuntimeError> {
        let property = !binding.lexical;
        let descriptor = self
            .heap
            .get_own_property_descriptor(global, binding.name.as_str())?;
        let initial = if property && !function {
            descriptor
                .as_ref()
                .and_then(|descriptor| descriptor.value.clone())
                .unwrap_or(Value::Undefined)
        } else {
            Value::Undefined
        };
        let cell = self.with_roots(|heap| heap.alloc_object(None))?;
        let root = self.heap.root(cell)?;
        let result = (|| {
            if property {
                if function
                    && descriptor
                        .as_ref()
                        .is_some_and(|descriptor| descriptor.configurable == Some(true))
                    || descriptor.is_none()
                {
                    let defined = self.with_roots(|heap| {
                        heap.define_own_property(
                            global,
                            binding.name.as_str(),
                            PropertyDescriptor::data(Value::Undefined, true, true, configurable),
                        )
                    })?;
                    if !defined {
                        return Err(RuntimeError::TypeError(
                            "cannot create global binding".into(),
                        ));
                    }
                } else if function {
                    self.with_roots(|heap| {
                        heap.set(global, binding.name.as_str(), Value::Undefined)
                    })?;
                }
            }
            if property {
                self.with_roots(|heap| heap.set(cell, "value", initial.clone()))?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.heap.unroot(root)?;
            return Err(error);
        }
        self.global_bindings.insert(
            binding.name.clone(),
            GlobalBinding {
                cell,
                mutable: binding.mutable,
                strict_immutable: binding.strict_immutable,
                property,
                _root: root,
            },
        );
        Ok(())
    }

    fn global_binding_value(&self, name: &str) -> Result<Option<Value>, RuntimeError> {
        let Some(binding) = self.global_bindings.get(name) else {
            return Ok(None);
        };
        self.heap.get_own(binding.cell, "value").map_err(Into::into)
    }

    fn set_global_binding(&mut self, name: &str, value: Value) -> Result<bool, RuntimeError> {
        let Some(binding) = self.global_bindings.get(name) else {
            return Ok(false);
        };
        let cell = binding.cell;
        let mutable = binding.mutable;
        let strict_immutable = binding.strict_immutable;
        if self.heap.get_own(cell, "value")?.is_none() {
            return Err(RuntimeError::ReferenceError(name.into()));
        }
        if !mutable && strict_immutable {
            return Err(RuntimeError::TypeError(format!(
                "assignment to constant {name}"
            )));
        }
        if !mutable {
            return Ok(true);
        }
        self.store_global_cell(cell, value)?;
        Ok(true)
    }

    fn dynamic_eval_binding_value(&self, name: &str) -> Result<Option<Value>, RuntimeError> {
        let binding = self.dynamic_eval_bindings.get(name).or_else(|| {
            self.dynamic_eval_outer_bindings
                .iter()
                .rev()
                .find_map(|bindings| bindings.get(name))
        });
        match binding {
            Some(binding) => self.heap.get_own(binding.cell, "value").map_err(Into::into),
            None => Ok(None),
        }
    }

    /// Returns the dynamic eval cell that shadows this exact statically
    /// resolved cell. A name match by itself is insufficient: Annex B block
    /// functions in the eval can share the name of the eval's var binding
    /// without being that binding.
    fn dynamic_eval_shadowing_cell(&self, name: &str, cell: ObjectId) -> Option<ObjectId> {
        self.dynamic_eval_bindings
            .get(name)
            .into_iter()
            .chain(
                self.dynamic_eval_outer_bindings
                    .iter()
                    .rev()
                    .filter_map(|bindings| bindings.get(name)),
            )
            .find(|binding| binding.shadowed_cells.contains(&cell))
            .map(|binding| binding.cell)
    }

    fn store_dynamic_eval_shadowing_binding(
        &mut self,
        slot: usize,
        name: &str,
        value: Value,
    ) -> Result<bool, RuntimeError> {
        let Some(&cell) = self.cells.get(&slot) else {
            return Ok(false);
        };
        let Some(shadowing) = self.dynamic_eval_shadowing_cell(name, cell) else {
            return Ok(false);
        };
        self.store_global_cell(shadowing, value)?;
        Ok(true)
    }

    fn set_dynamic_eval_binding(&mut self, name: &str, value: Value) -> Result<bool, RuntimeError> {
        let binding = self.dynamic_eval_bindings.get(name).or_else(|| {
            self.dynamic_eval_outer_bindings
                .iter()
                .rev()
                .find_map(|bindings| bindings.get(name))
        });
        let Some(binding) = binding else {
            return Ok(false);
        };
        self.store_global_cell(binding.cell, value)?;
        Ok(true)
    }

    fn delete_dynamic_eval_binding(&mut self, name: &str) -> Result<bool, RuntimeError> {
        let binding = self.dynamic_eval_bindings.remove(name).or_else(|| {
            self.dynamic_eval_outer_bindings
                .iter_mut()
                .rev()
                .find_map(|bindings| bindings.remove(name))
        });
        let Some(binding) = binding else {
            return Ok(true);
        };
        self.heap.delete(binding.cell, "value").map_err(Into::into)
    }

    fn store_global_cell(&mut self, cell: ObjectId, value: Value) -> Result<(), RuntimeError> {
        self.with_roots(|heap| heap.set(cell, "value", value.clone()))?;
        let property = self.global_bindings.iter().find_map(|(name, binding)| {
            (binding.cell == cell && binding.property).then(|| name.clone())
        });
        if let Some(name) = property {
            let global = self
                .global("globalThis")?
                .object_id()
                .expect("globalThis is an object");
            self.with_roots(|heap| heap.set(global, name, value))?;
        }
        Ok(())
    }

    fn global_property_cell(&self, object: ObjectId, key: &PropertyName) -> Option<ObjectId> {
        if self.globals.get("globalThis") != Some(&object) {
            return None;
        }
        let PropertyName::String(name) = key else {
            return None;
        };
        let name = name.to_utf8().ok()?;
        self.global_bindings
            .get(&name)
            .filter(|binding| binding.property)
            .map(|binding| binding.cell)
    }

    fn pop(&mut self) -> Value {
        self.stack
            .pop()
            .expect("compiler balances the operand stack")
    }

    fn reset_scope(&mut self, code: &Bytecode, scope: u32) {
        for slot in &code.scopes[scope as usize] {
            // The graph linker owns module outer-scope cells until every
            // dependent body has finished.  Do not discard them merely
            // because a suspended/resumed module leaves its lexical scope.
            if code.module && scope == 0 {
                self.bindings[*slot as usize] = None;
                continue;
            }
            self.cells.remove(&(*slot as usize));
            self.bindings[*slot as usize] = None;
        }
    }

    /// Creates the next per-iteration environment for a lexical `for`
    /// declaration.  The previous cells deliberately stay alive through any
    /// closures that captured them; this frame starts using fresh cells with
    /// the values left by the completed loop body, ready for its update
    /// expression.
    fn clone_scope(&mut self, code: &Bytecode, scope: u32) -> Result<(), RuntimeError> {
        let slots = code.scopes[scope as usize].clone();
        for slot in slots {
            let slot = slot as usize;
            let value = self.binding_value(slot)?;
            let cell = self.with_roots(|heap| heap.alloc_object(None))?;
            // Insert before the allocation-backed store so the fresh cell is
            // an interpreter root if the store needs to collect.
            self.cells.insert(slot, cell);
            self.bindings[slot] = None;
            if let Some(value) = value {
                self.with_roots(|heap| heap.set(cell, "value", value))?;
            }
        }
        Ok(())
    }

    fn leave_scope(&mut self, code: &Bytecode, scope: u32) {
        if self.active_scopes.last() == Some(&scope) {
            self.active_scopes.pop();
            self.active_scope_slots.pop();
            self.reset_scope(code, scope);
        } else {
            // A control-transfer gateway can be resumed after a handler has
            // already unwound an inner scope before running `finally`.
            // Gateways still list that lexical scope; it is a no-op now.
            debug_assert!(
                !self.active_scopes.contains(&scope),
                "scope {scope} is below an active inner scope: {:?}",
                self.active_scopes
            );
        }
    }

    fn unwind_scopes(&mut self, code: &Bytecode, depth: usize) {
        while self.active_scopes.len() > depth {
            let scope = self.active_scopes.pop().expect("scope length was checked");
            self.active_scope_slots.pop();
            self.reset_scope(code, scope);
        }
    }

    fn unwind_with(&mut self, depth: usize) {
        self.with_objects.truncate(depth);
    }

    fn close_iterators_to(&mut self, iterators: &mut Vec<Value>, depth: usize) {
        // A compiler-emitted `break` can close its target for-of iterator
        // before a surrounding handler starts finalizer cleanup.
        let active = iterators.split_off(depth.min(iterators.len()));
        for record in active.into_iter().rev() {
            // This is cleanup for an already-selected abrupt completion. The
            // original completion wins over a return() failure.
            let _ = self.iterator_close(&record);
        }
    }

    fn error_value(&mut self, error: RuntimeError) -> Result<Value, RuntimeError> {
        match error {
            RuntimeError::Thrown(value) => Ok(value),
            RuntimeError::ReferenceError(message) => self.error_object("ReferenceError", message),
            RuntimeError::TypeError(message) => self.error_object("TypeError", message),
            RuntimeError::RangeError(message) => self.error_object("RangeError", message),
            RuntimeError::SyntaxError(message) => self.error_object("SyntaxError", message),
            // Dynamic import reports a linking/host-resolution failure by
            // rejecting its Promise with a SyntaxError; it must not escape
            // the job queue as an implementation error.
            RuntimeError::ModuleResolution(message) => self.error_object("SyntaxError", message),
            RuntimeError::Test262(message) => self.error_object("Test262Error", message),
            error => Err(error),
        }
    }

    fn error_object(&mut self, name: &str, message: String) -> Result<Value, RuntimeError> {
        let constructor = self.error_global(name)?;
        self.call_native(
            constructor,
            Value::Undefined,
            vec![Value::String(message.into())],
            false,
        )
    }

    fn restore_completion(&mut self) {
        let (value, empty) = self
            .completion_saves
            .pop()
            .expect("normal finally entry saves its preceding completion");
        self.completion = value;
        self.completion_empty = empty;
    }

    fn resolve_completion(
        &mut self,
        code: &Bytecode,
        handlers: &mut Vec<HandlerFrame>,
        iterators: &mut Vec<Value>,
        completion: Completion,
    ) -> Result<CompletionAction, RuntimeError> {
        if let Completion::Halt(value) = completion {
            return Ok(CompletionAction::Return(value));
        }
        if let Completion::Resume(metadata) = completion {
            if let Some(frame) = handlers.pop_if(|frame| frame.metadata == metadata) {
                let pending = frame
                    .pending
                    .expect("only an abrupt finally resumes a handler");
                return self.resolve_completion(
                    code,
                    handlers,
                    iterators,
                    self.pending_completions[pending].clone(),
                );
            }
            self.restore_completion();
            return Ok(CompletionAction::Continue);
        }

        // Keep a potential thrown/returned object reachable while scope and
        // iterator cleanup can call user code and trigger collection.
        self.pending_completions.push(completion.clone());
        let mut completion = completion;
        loop {
            let Some(frame) = handlers.last() else {
                return Ok(match completion {
                    Completion::Throw(error) => CompletionAction::Throw(error),
                    Completion::Return(value) => CompletionAction::Return(value),
                    Completion::TailRecur(args) => CompletionAction::TailRecur(args),
                    Completion::Jump { cleanup, .. } => CompletionAction::Jump(cleanup),
                    Completion::Resume(_) | Completion::Halt(_) | Completion::Yield(_) => {
                        unreachable!("handled above")
                    }
                });
            };
            let metadata = frame.metadata;
            let stack_depth = frame.stack_depth;
            let scope_depth = frame.scope_depth;
            let iterator_depth = frame.iterator_depth;
            let with_depth = frame.with_depth;
            let state = frame.state;
            let handler = &code.handlers[metadata];
            let catch = handler.catch;
            let finally = handler.finally;

            // A break/continue may target a loop that is contained in this
            // try or catch block. It has not left this handler, so it must not
            // consume the frame or spuriously run an outer finalizer.
            if let Completion::Jump { cleanup, target } = completion {
                let region = match state {
                    HandlerState::Try => {
                        Some((handler.try_start as usize, handler.try_end as usize))
                    }
                    HandlerState::Catch => handler.catch.map(|start| {
                        (
                            start as usize,
                            handler.catch_end.expect("catch end is compiled") as usize,
                        )
                    }),
                    HandlerState::Finally => None,
                };
                if region.is_some_and(|(start, end)| (start..end).contains(&target)) {
                    return Ok(CompletionAction::Jump(cleanup));
                }
            }

            // Break/continue resume at compiler-emitted cleanup gateways. If
            // one crosses this handler to reach a finalizer, unwind its try or
            // catch scope before the finalizer runs; its later gateway skips
            // that already-cleared scope. Throws and returns have no bytecode
            // continuation, so they always unwind immediately.
            let unwind = !matches!(completion, Completion::Jump { .. })
                || (state != HandlerState::Finally && finally.is_some());
            if unwind {
                self.stack.truncate(stack_depth);
                self.unwind_scopes(code, scope_depth);
                self.close_iterators_to(iterators, iterator_depth);
                self.unwind_with(with_depth);
            }

            if state == HandlerState::Try {
                if let (Some(target), Completion::Throw(error)) = (catch, &completion) {
                    let value = self.error_value(error.clone())?;
                    handlers
                        .last_mut()
                        .expect("handler was inspected above")
                        .state = HandlerState::Catch;
                    self.stack.push(value);
                    return Ok(CompletionAction::Jump(target as usize));
                }
            }
            if state != HandlerState::Finally {
                if let Some(target) = finally {
                    let pending = self.pending_completions.len();
                    self.pending_completions.push(completion);
                    let frame = handlers.last_mut().expect("handler was inspected above");
                    frame.state = HandlerState::Finally;
                    frame.pending = Some(pending);
                    return Ok(CompletionAction::Jump(target as usize));
                }
            }
            handlers.pop();
            completion = self
                .pending_completions
                .last()
                .expect("completion remains rooted")
                .clone();
        }
    }

    fn check_string(&self, value: &Value) -> Result<(), RuntimeError> {
        if matches!(value, Value::String(s) if s.byte_len() > self.config.max_string_bytes) {
            Err(RuntimeError::StringLimit {
                limit: self.config.max_string_bytes,
            })
        } else {
            Ok(())
        }
    }

    fn with_roots<T>(
        &mut self,
        operation: impl FnOnce(&mut Heap) -> Result<T, HeapError>,
    ) -> Result<T, RuntimeError> {
        let mut roots = Vec::new();
        let registration = (|| {
            for value in self
                .stack
                .iter()
                .chain(self.bindings.iter().flatten())
                .chain(std::iter::once(&self.completion))
                .chain(std::iter::once(&self.callee))
                .chain(self.pending_completions.iter().flat_map(|completion| {
                    let values: &[Value] = match completion {
                        Completion::Return(value)
                        | Completion::Yield(value)
                        | Completion::Throw(RuntimeError::Thrown(value)) => {
                            std::slice::from_ref(value)
                        }
                        Completion::TailRecur(args) => args,
                        Completion::Throw(_)
                        | Completion::Jump { .. }
                        | Completion::Resume(_)
                        | Completion::Halt(_) => &[],
                    };
                    values.iter()
                }))
                .chain(self.completion_saves.iter().map(|(value, _)| value))
                .chain(self.with_objects.iter())
            {
                if let Value::Object(id) = value {
                    // Rooting cannot GC, but can exhaust the root-ID
                    // counter. Partial registrations must be released too.
                    roots.push(self.heap.root(*id)?);
                }
            }
            for id in self.cells.values() {
                roots.push(self.heap.root(*id)?);
            }
            for binding in self.dynamic_eval_bindings.values() {
                roots.push(self.heap.root(binding.cell)?);
            }
            for bindings in &self.dynamic_eval_outer_bindings {
                for binding in bindings.values() {
                    roots.push(self.heap.root(binding.cell)?);
                }
            }
            // A continuation lives in a Rust map while it waits for a Promise
            // job.  Its frame has no ordinary heap owner, so root every edge
            // before any allocation is allowed to trigger collection.
            let continuation_references = self.continuation_references();
            for id in continuation_references {
                roots.push(self.heap.root(id)?);
            }
            for (&promise, record) in &self.promises {
                roots.push(self.heap.root(promise)?);
                if matches!(record.status, PromiseStatus::Pending) {
                    for reaction in &record.reactions {
                        if let PromiseReaction::AsyncGeneratorYield {
                            generator,
                            target,
                            result,
                        } = reaction
                        {
                            roots.push(self.heap.root(*generator)?);
                            roots.push(self.heap.root(*target)?);
                            roots.push(self.heap.root(*result)?);
                        }
                        if let PromiseReaction::AsyncGeneratorDelegate {
                            generator, target, ..
                        } = reaction
                        {
                            roots.push(self.heap.root(*generator)?);
                            roots.push(self.heap.root(*target)?);
                        }
                    }
                }
                let values: Vec<&Value> = match &record.status {
                    PromiseStatus::Pending => record
                        .reactions
                        .iter()
                        .filter_map(|reaction| match reaction {
                            PromiseReaction::Then(reaction) => {
                                Some([&reaction.on_fulfilled, &reaction.on_rejected])
                            }
                            PromiseReaction::ModuleAwait { .. }
                            | PromiseReaction::AsyncAwait { .. }
                            | PromiseReaction::AsyncGeneratorYield { .. }
                            | PromiseReaction::AsyncGeneratorDelegate { .. } => None,
                        })
                        .flatten()
                        .collect(),
                    PromiseStatus::Fulfilled(value) | PromiseStatus::Rejected(value) => {
                        vec![value]
                    }
                };
                for value in values {
                    if let Value::Object(id) = value {
                        roots.push(self.heap.root(*id)?);
                    }
                }
            }
            for state in self.promise_all.values() {
                for value in state.values.iter().flatten() {
                    if let Value::Object(id) = value {
                        roots.push(self.heap.root(*id)?);
                    }
                }
            }
            for job in &self.promise_jobs {
                match job {
                    PromiseJob::Reaction {
                        target,
                        handler,
                        value,
                        ..
                    } => {
                        roots.push(self.heap.root(*target)?);
                        for value in [handler, value] {
                            if let Value::Object(id) = value {
                                roots.push(self.heap.root(*id)?);
                            }
                        }
                    }
                    PromiseJob::Thenable {
                        target,
                        thenable,
                        then,
                    } => {
                        roots.push(self.heap.root(*target)?);
                        for value in [thenable, then] {
                            if let Value::Object(id) = value {
                                roots.push(self.heap.root(*id)?);
                            }
                        }
                    }
                    PromiseJob::DynamicImport { target, .. } => {
                        roots.push(self.heap.root(*target)?)
                    }
                    PromiseJob::ModuleAwait { value, .. }
                    | PromiseJob::AsyncAwait { value, .. } => {
                        if let Value::Object(id) = value {
                            roots.push(self.heap.root(*id)?);
                        }
                    }
                    PromiseJob::AsyncGeneratorYield {
                        generator,
                        target,
                        result,
                        value,
                        ..
                    } => {
                        roots.push(self.heap.root(*generator)?);
                        roots.push(self.heap.root(*target)?);
                        roots.push(self.heap.root(*result)?);
                        if let Value::Object(id) = value {
                            roots.push(self.heap.root(*id)?);
                        }
                    }
                    PromiseJob::AsyncGeneratorDelegate {
                        generator,
                        target,
                        value,
                        ..
                    } => {
                        roots.push(self.heap.root(*generator)?);
                        roots.push(self.heap.root(*target)?);
                        if let Value::Object(id) = value {
                            roots.push(self.heap.root(*id)?);
                        }
                    }
                }
            }
            if let Some(Err(Value::Object(id))) = &self.test262_done {
                roots.push(self.heap.root(*id)?);
            }
            Ok(())
        })();
        let result = registration.and_then(|()| operation(&mut self.heap));
        for root in roots {
            self.heap
                .unroot(root)
                .expect("temporary root belongs to this safepoint");
        }
        result.map_err(RuntimeError::from)
    }

    fn run(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        let pending_base = self.pending_completions.len();
        let save_base = self.completion_saves.len();
        let mut iterators = Vec::new();
        let result = self
            .interpret(code, &mut iterators, 0, None, None, None)
            .and_then(|exit| match exit {
                InterpreterExit::Return(value) => Ok(value),
                InterpreterExit::Yield { .. } => Err(RuntimeError::TypeError(
                    "yield requires a generator function".into(),
                )),
                InterpreterExit::Suspend { .. } => unreachable!("only generator entry suspends"),
                InterpreterExit::Await { .. } => {
                    unreachable!("only module evaluation can suspend at await")
                }
            });
        if result.is_err() {
            if let Err(RuntimeError::Thrown(value)) = &result {
                self.stack.push(value.clone());
            }
            self.stack.extend(iterators.iter().cloned());
            for record in iterators.into_iter().rev() {
                // IteratorClose preserves an existing throw even when return
                // throws too. Resource exhaustion retains the original limit.
                let _ = self.iterator_close(&record);
            }
        }
        self.pending_completions.truncate(pending_base);
        self.completion_saves.truncate(save_base);
        result
    }

    fn execute_eval(
        &mut self,
        code: &Bytecode,
        captures: Vec<ObjectId>,
        global_var_environment: bool,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.extend(self.bindings.iter().flatten().cloned());
        self.stack
            .extend(self.cells.values().copied().map(Value::Object));
        self.stack.push(self.completion.clone());
        self.stack.push(self.this.clone());
        self.stack.extend(self.arguments.iter().cloned());
        let bindings = std::mem::replace(&mut self.bindings, vec![None; code.bindings.len()]);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, code.bindings.clone());
        let cells = std::mem::replace(&mut self.cells, captures.into_iter().enumerate().collect());
        let script_global_slots = std::mem::take(&mut self.script_global_slots);
        let eval_dynamic_slots = std::mem::take(&mut self.eval_dynamic_slots);
        let completion = std::mem::replace(&mut self.completion, Value::Undefined);
        let completion_empty = std::mem::replace(&mut self.completion_empty, true);
        let active_scopes = std::mem::take(&mut self.active_scopes);
        let active_scope_slots = std::mem::take(&mut self.active_scope_slots);
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let variable_scope = code
            .strict
            .then(|| std::mem::replace(&mut self.variable_scope, code.variable_scope));
        let result = if global_var_environment {
            self.prepare_eval_global_var_declarations(code)
                .and_then(|()| self.run(code))
        } else if !code.strict {
            self.prepare_eval_dynamic_var_declarations(code)
                .and_then(|()| self.run(code))
        } else {
            self.run(code)
        };
        self.bindings = bindings;
        self.binding_metadata = binding_metadata;
        self.cells = cells;
        self.script_global_slots = script_global_slots;
        self.eval_dynamic_slots = eval_dynamic_slots;
        self.completion = completion;
        self.completion_empty = completion_empty;
        self.active_scopes = active_scopes;
        self.active_scope_slots = active_scope_slots;
        self.strict = strict;
        if let Some(variable_scope) = variable_scope {
            self.variable_scope = variable_scope;
        }
        self.stack.truncate(base);
        result
    }

    /// EvalDeclarationInstantiation's ordinary-function branch. Sloppy
    /// direct eval extends the caller's VariableEnvironment, so fresh `var`
    /// and function cells survive eval and can be captured by closures.
    fn prepare_eval_dynamic_var_declarations(
        &mut self,
        code: &Bytecode,
    ) -> Result<(), RuntimeError> {
        for &slot in &code.dynamic_eval_slots {
            let binding = &code.bindings[slot as usize];
            let shadowed_cells: Vec<_> = code
                .captures
                .iter()
                .enumerate()
                .filter_map(|(captured_slot, _)| {
                    (code.bindings[captured_slot].name == binding.name)
                        .then(|| self.cells.get(&captured_slot).copied())
                        .flatten()
                })
                .collect();
            if !self.dynamic_eval_bindings.contains_key(&binding.name) {
                let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                self.dynamic_eval_bindings.insert(
                    binding.name.clone(),
                    DynamicEvalBinding {
                        cell,
                        shadowed_cells: shadowed_cells.clone(),
                    },
                );
                if let Err(error) =
                    self.with_roots(|heap| heap.set(cell, "value", Value::Undefined))
                {
                    self.dynamic_eval_bindings.remove(&binding.name);
                    return Err(error);
                }
            } else if let Some(dynamic) = self.dynamic_eval_bindings.get_mut(&binding.name) {
                for cell in shadowed_cells {
                    if !dynamic.shadowed_cells.contains(&cell) {
                        dynamic.shadowed_cells.push(cell);
                    }
                }
            }
            self.eval_dynamic_slots
                .insert(slot as usize, binding.name.clone());
        }
        Ok(())
    }

    /// EvalDeclarationInstantiation's global-variable branch. The eval
    /// lexical environment remains transient, so only `var` and top-level
    /// function bindings are published into the realm's global environment.
    fn prepare_eval_global_var_declarations(
        &mut self,
        code: &Bytecode,
    ) -> Result<(), RuntimeError> {
        let slots = code.scopes.first().cloned().unwrap_or_default();
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("globalThis is an object");

        for &slot in &slots {
            let binding = &code.bindings[slot as usize];
            if binding.lexical {
                continue;
            }
            let existing = self.global_bindings.get(&binding.name);
            if existing.is_some_and(|binding| !binding.property) {
                return Err(RuntimeError::SyntaxError(format!(
                    "global lexical binding {} conflicts with eval declaration",
                    binding.name
                )));
            }
            if code.global_function_names.contains(&binding.name) {
                if !self.can_declare_global_function(global, &binding.name)? {
                    return Err(RuntimeError::TypeError(format!(
                        "cannot declare global function {}",
                        binding.name
                    )));
                }
            } else if !self.can_declare_global_var(global, &binding.name)? {
                return Err(RuntimeError::TypeError(format!(
                    "cannot declare global var {}",
                    binding.name
                )));
            }
        }

        for &slot in &slots {
            let binding = &code.bindings[slot as usize];
            if binding.lexical {
                continue;
            }
            if !self.global_bindings.contains_key(&binding.name) {
                self.create_global_binding(
                    global,
                    binding,
                    code.global_function_names.contains(&binding.name),
                    true,
                )?;
            }
            self.script_global_slots
                .insert(slot as usize, binding.name.clone());
        }
        Ok(())
    }

    /// Evaluates a new classic script in the current realm while another
    /// script/function frame is active (the Test262 `$262.evalScript` host
    /// path). It deliberately gets fresh script bindings and global `this`,
    /// but preserves the caller frame and the remaining resource budget.
    fn execute_nested_script(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.extend(self.bindings.iter().flatten().cloned());
        self.stack
            .extend(self.cells.values().copied().map(Value::Object));
        self.stack.push(self.completion.clone());
        self.stack.push(self.this.clone());
        self.stack.extend(self.arguments.iter().cloned());
        self.stack.extend(self.with_objects.iter().cloned());
        let bindings = std::mem::replace(&mut self.bindings, vec![None; code.bindings.len()]);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, code.bindings.clone());
        let cells = std::mem::take(&mut self.cells);
        let completion = std::mem::replace(&mut self.completion, Value::Undefined);
        let completion_empty = std::mem::replace(&mut self.completion_empty, true);
        let active_scopes = std::mem::take(&mut self.active_scopes);
        let active_scope_slots = std::mem::take(&mut self.active_scope_slots);
        let with_objects = std::mem::take(&mut self.with_objects);
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let top_level_module = std::mem::replace(&mut self.top_level_module, false);
        let global_this = self.global("globalThis")?;
        let this = std::mem::replace(&mut self.this, global_this);
        let arguments = std::mem::take(&mut self.arguments);
        let new_target = std::mem::replace(&mut self.new_target, Value::Undefined);
        let new_target_allowed = std::mem::replace(&mut self.new_target_allowed, false);
        let home_object = std::mem::take(&mut self.home_object);
        let class_constructor = std::mem::take(&mut self.class_constructor);
        let class_field_initializer_depth =
            std::mem::replace(&mut self.class_field_initializer_depth, 0);
        let script_global_slots = std::mem::take(&mut self.script_global_slots);
        let result = self
            .prepare_global_declarations(code)
            .and_then(|()| self.run(code));
        self.bindings = bindings;
        self.binding_metadata = binding_metadata;
        self.cells = cells;
        self.completion = completion;
        self.completion_empty = completion_empty;
        self.active_scopes = active_scopes;
        self.active_scope_slots = active_scope_slots;
        self.with_objects = with_objects;
        self.strict = strict;
        self.top_level_module = top_level_module;
        self.this = this;
        self.arguments = arguments;
        self.new_target = new_target;
        self.new_target_allowed = new_target_allowed;
        self.home_object = home_object;
        self.class_constructor = class_constructor;
        self.class_field_initializer_depth = class_field_initializer_depth;
        self.script_global_slots = script_global_slots;
        self.stack.truncate(base);
        result
    }

    fn eval_visible_bindings(&self) -> Vec<(String, Binding, u32)> {
        let mut visible = std::collections::BTreeMap::new();
        for scope in &self.active_scope_slots {
            for &slot in scope {
                let slot = slot as usize;
                visible.insert(
                    self.binding_metadata[slot].name.clone(),
                    (self.binding_metadata[slot].clone(), slot as u32),
                );
            }
        }
        for &slot in self.cells.keys() {
            visible
                .entry(self.binding_metadata[slot].name.clone())
                .or_insert_with(|| (self.binding_metadata[slot].clone(), slot as u32));
        }
        // A named function expression's immutable name environment is not a
        // block scope, so it is neither in `active_scope_slots` nor captured
        // until a nested closure needs it. Direct eval nevertheless sees that
        // live binding and must retain its strict/sloppy write behavior.
        for (slot, value) in self.bindings.iter().enumerate() {
            if value.is_some() {
                visible
                    .entry(self.binding_metadata[slot].name.clone())
                    .or_insert_with(|| (self.binding_metadata[slot].clone(), slot as u32));
            }
        }
        visible
            .into_iter()
            .map(|(name, (binding, slot))| (name, binding, slot))
            .collect()
    }

    /// The variable environment is narrower than the set of lexically
    /// visible cells. In particular, an inner function may capture `x` from
    /// an outer function while a sloppy direct eval still has to create its
    /// own `var x` in the inner VariableEnvironment.
    fn eval_variable_environment_names(&self) -> Vec<String> {
        let mut names = std::collections::BTreeSet::new();
        if let Some(position) = self
            .active_scopes
            .iter()
            .position(|scope| *scope == self.variable_scope)
        {
            names.extend(self.active_scope_slots[position].iter().filter_map(|slot| {
                let binding = &self.binding_metadata[*slot as usize];
                (!binding.lexical).then(|| binding.name.clone())
            }));
        }
        names.extend(self.dynamic_eval_bindings.keys().cloned());
        names.into_iter().collect()
    }

    /// A sloppy direct eval declaration can shadow a static captured binding
    /// from an outer function. The dynamic binding records the exact cell it
    /// masks, so an eval-local block binding with the same name stays visible.
    fn eval_aware_binding_value(
        &mut self,
        slot: usize,
        name: &str,
    ) -> Result<Option<Value>, RuntimeError> {
        if let Some(&cell) = self.cells.get(&slot) {
            if let Some(shadowing) = self.dynamic_eval_shadowing_cell(name, cell) {
                return self.heap.get_own(shadowing, "value").map_err(Into::into);
            }
        }
        self.binding_value(slot)
    }

    /// Lexical names between a direct eval site and the active function's
    /// VariableEnvironment. Unlike captured outer bindings, these prevent a
    /// sloppy eval `var` declaration from being instantiated.
    fn eval_lexical_conflicts(&self) -> Vec<String> {
        let variable_scope_position = self
            .active_scopes
            .iter()
            .position(|scope| *scope == self.variable_scope);
        let start = variable_scope_position.map_or(0, |index| index + 1);
        let mut conflicts = self.active_scope_slots[start..]
            .iter()
            .flat_map(|slots| slots.iter().copied())
            .filter_map(|slot| {
                let binding = &self.binding_metadata[slot as usize];
                (binding.lexical && !binding.catch_parameter).then(|| binding.name.clone())
            })
            .collect::<std::collections::BTreeSet<_>>();
        // A non-arrow function's parameter expressions retain the separate
        // body VariableEnvironment boundary. Its body lexical declarations
        // therefore block a sloppy direct-eval var declaration even before
        // the body scope is entered. Arrow parameters inherit their outer
        // VariableEnvironment instead, so their not-yet-entered body lexical
        // declarations must not be treated as a conflict.
        let ordinary_function = self
            .callee
            .object_id()
            .and_then(|callee| self.heap.closure(callee).ok().flatten())
            .is_some_and(|(code, _, _, _, _)| !code.arrow);
        if variable_scope_position.is_none() && ordinary_function {
            conflicts.extend(self.variable_scope_lexicals.iter().cloned());
        }
        conflicts.into_iter().collect()
    }

    fn interpret(
        &mut self,
        code: &Bytecode,
        iterators: &mut Vec<Value>,
        start_pc: usize,
        resume_value: Option<Value>,
        suspend_at: Option<usize>,
        restored_handlers: Option<(Vec<HandlerFrame>, usize)>,
    ) -> Result<InterpreterExit, RuntimeError> {
        let stack_base = self.stack.len();
        let pending_base = self.pending_completions.len();
        let save_base = self.completion_saves.len();
        if let Some(value) = resume_value {
            self.stack.push(value);
        }
        let mut pc = start_pc;
        let (mut handlers, handler_stack_base) =
            restored_handlers.unwrap_or_else(|| (Vec::new(), stack_base));
        // Suspended frames store stack offsets relative to their saved stack,
        // while an active interpreter may have an ambient caller frame below
        // it.  Convert at the boundary so catch/finally cleanup always uses
        // the current absolute operand-stack offsets.
        for handler in &mut handlers {
            handler.stack_depth += handler_stack_base;
        }
        let mut suspended_await = None;
        loop {
            if suspend_at == Some(pc) {
                return Ok(InterpreterExit::Suspend { pc });
            }
            self.charge_step()?;
            let instruction = code
                .instruction(pc)
                .expect("compiler emits valid instruction boundaries");
            let operand = instruction.operand.unwrap_or(0) as usize;
            pc += instruction.opcode.width();
            let outcome: Result<Option<Completion>, RuntimeError> = (|| {
                match instruction.opcode {
                    Opcode::DefineData
                    | Opcode::DefineAccessor
                    | Opcode::DefineMethod
                    | Opcode::DefineClassAccessor => {
                        let value = self.pop();
                        let (receiver, key) = self.property_reference()?;
                        let object = receiver.object_id().unwrap();
                        if matches!(
                            instruction.opcode,
                            Opcode::DefineMethod
                                | Opcode::DefineAccessor
                                | Opcode::DefineClassAccessor
                        ) {
                            if let Value::Object(function) = value {
                                self.with_roots(|heap| heap.set_closure_home(function, object))?;
                            }
                        }
                        if instruction.opcode == Opcode::DefineData {
                            self.define_data(object, key, value.clone(), true, true, true)?;
                        } else {
                            let descriptor = if instruction.opcode == Opcode::DefineMethod {
                                PropertyDescriptor::data(value.clone(), true, operand != 0, true)
                            } else {
                                PropertyDescriptor {
                                    get: (operand == 0).then(|| value.clone()),
                                    set: (operand != 0).then(|| value.clone()),
                                    enumerable: Some(instruction.opcode == Opcode::DefineAccessor),
                                    configurable: Some(true),
                                    ..Default::default()
                                }
                            };
                            if !self.with_roots(|heap| {
                                heap.define_own_property(object, key, descriptor)
                            })? {
                                return Err(RuntimeError::TypeError(
                                    "cannot define class property".into(),
                                ));
                            }
                        }
                        self.stack.push(value);
                    }
                    Opcode::DefinePrivateField => {
                        let (receiver, name) = self.property_reference()?;
                        let Value::Object(owner) = receiver else {
                            unreachable!("private class owner is an object")
                        };
                        let PropertyName::String(name) = name else {
                            unreachable!("compiler emits string private names")
                        };
                        self.with_roots(|heap| {
                            if operand != 0 {
                                heap.add_private_brand(owner, owner)?;
                            }
                            heap.define_private_field(owner, name)
                        })?;
                    }
                    Opcode::DefinePrivateMethod | Opcode::DefinePrivateAccessor => {
                        let value = self.pop();
                        let (receiver, name) = self.property_reference()?;
                        let Value::Object(owner) = receiver else {
                            unreachable!("private class owner is an object")
                        };
                        let PropertyName::String(name) = name else {
                            unreachable!("compiler emits string private names")
                        };
                        if let Value::Object(function) = value {
                            self.with_roots(|heap| heap.set_closure_home(function, owner))?;
                        }
                        if instruction.opcode == Opcode::DefinePrivateMethod {
                            self.with_roots(|heap| {
                                if operand != 0 {
                                    heap.add_private_brand(owner, owner)?;
                                }
                                heap.define_private_method(owner, name, value)
                            })?;
                        } else {
                            self.with_roots(|heap| {
                                if operand & 2 != 0 {
                                    heap.add_private_brand(owner, owner)?;
                                }
                                heap.define_private_accessor(owner, name, value, operand & 1 != 0)
                            })?;
                        }
                    }
                    Opcode::DeleteProperty => {
                        let (receiver, key) = self.property_reference()?;
                        let deleted = match receiver {
                            Value::Object(id) => self.object_delete(id, &key)?,
                            Value::String(s) => {
                                !matches!(&key, PropertyName::String(key) if s.own_property(key).is_some())
                            }
                            _ => true,
                        };
                        if !deleted && self.strict {
                            return Err(RuntimeError::TypeError(
                                "cannot delete non-configurable property".into(),
                            ));
                        }
                        self.stack.push(Value::Bool(deleted));
                    }
                    Opcode::Throw => {
                        return Ok(Some(Completion::Throw(RuntimeError::Thrown(self.pop()))))
                    }
                    Opcode::InvalidAssignmentTarget => {
                        // Annex B CallExpression targets evaluate the call,
                        // then throw before an assignment RHS or update
                        // coercion can be observed.
                        self.pop();
                        return Err(RuntimeError::ReferenceError(
                            "invalid assignment target".into(),
                        ));
                    }
                    Opcode::ArrayPush => {
                        let base = self.stack.len() - 2;
                        self.array_push(
                            &self.stack[base].clone(),
                            &self.stack[base + 1].clone(),
                            operand,
                        )?;
                        self.stack.truncate(base + 1);
                    }
                    Opcode::CallSpread | Opcode::DirectEvalSpread => {
                        let base = self.stack.len() - 3;
                        let args = self.array_like_values(&self.stack[base + 2].clone())?;
                        let callee = self.stack[base].clone();
                        let receiver = self.stack[base + 1].clone();
                        let result = if instruction.opcode == Opcode::DirectEvalSpread
                            && self.is_intrinsic_eval(&callee)?
                        {
                            self.direct_eval(native::argument(&args, 0))?
                        } else {
                            self.call_native(callee, receiver, args, operand != 0)?
                        };
                        self.stack.truncate(base);
                        self.stack.push(result);
                    }
                    Opcode::CallClassStaticBlock => {
                        let base = self.stack.len() - 2;
                        let Value::Object(target) = self.stack[base].clone() else {
                            unreachable!("class constructors are objects")
                        };
                        if let Value::Object(function) = self.stack[base + 1] {
                            self.with_roots(|heap| heap.set_closure_home(function, target))?;
                        }
                        self.call_native(
                            self.stack[base + 1].clone(),
                            self.stack[base].clone(),
                            Vec::new(),
                            false,
                        )?;
                        self.stack.truncate(base + 1);
                    }
                    Opcode::DefineClassStaticField => {
                        let base = self.stack.len() - 4;
                        let target = self.stack[base + 1].clone();
                        let key = self.coerce_property_key(&self.stack[base + 2].clone())?;
                        let initializer = self.stack[base + 3].clone();
                        if let (Value::Object(target), Value::Object(function)) =
                            (&target, &initializer)
                        {
                            self.with_roots(|heap| heap.set_closure_home(*function, *target))?;
                        }
                        let value =
                            self.call_native(initializer, target.clone(), Vec::new(), false)?;
                        let Value::Object(target) = target else {
                            unreachable!("class fields target the constructor")
                        };
                        if !self.with_roots(|heap| {
                            heap.define_own_property(
                                target,
                                key,
                                PropertyDescriptor::data(value, true, true, true),
                            )
                        })? {
                            return Err(RuntimeError::TypeError(
                                "cannot define class field".into(),
                            ));
                        }
                        self.stack.truncate(base + 1);
                    }
                    Opcode::DefinePrivateStaticField => {
                        let base = self.stack.len() - 4;
                        let target = self.stack[base + 1].clone();
                        let name = match self.stack[base + 2].clone() {
                            Value::String(name) => name,
                            _ => unreachable!("compiler emits a private-name string"),
                        };
                        let initializer = self.stack[base + 3].clone();
                        if let (Value::Object(target), Value::Object(function)) =
                            (&target, &initializer)
                        {
                            self.with_roots(|heap| heap.set_closure_home(*function, *target))?;
                        }
                        let value =
                            self.call_native(initializer, target.clone(), Vec::new(), false)?;
                        let Value::Object(target) = target else {
                            unreachable!("class fields target the constructor")
                        };
                        self.with_roots(|heap| heap.set_private_slot(target, target, name, value))?;
                        self.stack.truncate(base + 1);
                    }
                    Opcode::SetClassHome => self.set_class_home()?,
                    Opcode::SetClassHeritage => self.set_class_heritage()?,
                    Opcode::InitializePrivateBrand => {
                        let owner = self
                            .binding_value(operand)?
                            .and_then(|value| value.object_id())
                            .ok_or_else(|| {
                                RuntimeError::TypeError(
                                    "private elements are not available in this function".into(),
                                )
                            })?;
                        let receiver = self.this.object_id().ok_or_else(|| {
                            RuntimeError::TypeError(
                                "private fields require an object receiver".into(),
                            )
                        })?;
                        self.with_roots(|heap| heap.add_private_brand(receiver, owner))?;
                    }
                    Opcode::PrivateGet | Opcode::PrivateGetMethod => {
                        let (receiver, owner, name) = self.private_reference(operand)?;
                        let value = self.private_get(&receiver, owner, &name)?;
                        self.stack.push(value);
                        if instruction.opcode == Opcode::PrivateGetMethod {
                            self.stack.push(receiver);
                        }
                    }
                    Opcode::PrivateSet => {
                        let value = self.pop();
                        let (receiver, owner, name) = self.private_reference(operand)?;
                        self.private_set(&receiver, owner, name, value.clone())?;
                        self.stack.push(value);
                    }
                    Opcode::PrivateIn => {
                        let (receiver, owner, _name) = self.private_reference(operand)?;
                        let object = receiver.object_id().ok_or_else(|| {
                            RuntimeError::TypeError("private brand checks require an object".into())
                        })?;
                        self.stack
                            .push(Value::Bool(self.heap.has_private_brand(object, owner)?));
                    }
                    Opcode::SuperGet | Opcode::SuperGetMethod => {
                        let key_value = self.pop();
                        let key = self.coerce_property_key(&key_value)?;
                        let value = self.super_get(&key)?;
                        self.check_string(&value)?;
                        self.stack.push(value);
                        if instruction.opcode == Opcode::SuperGetMethod {
                            self.stack.push(self.this.clone());
                        }
                    }
                    Opcode::SuperSet => {
                        let value = self.pop();
                        let key_value = self.pop();
                        let key = self.coerce_property_key(&key_value)?;
                        self.super_set(&key, &value)?;
                        self.stack.push(value);
                    }
                    Opcode::SuperUpdate => {
                        let key_value = self.pop();
                        let key = self.coerce_property_key(&key_value)?;
                        let old_value = self.super_get(&key)?;
                        let old = self.coerce_number(&old_value)?;
                        let new = if operand & 1 == 0 {
                            old + 1.0
                        } else {
                            old - 1.0
                        };
                        self.super_set(&key, &Value::Number(new))?;
                        self.stack
                            .push(Value::Number(if operand & 2 == 0 { old } else { new }));
                    }
                    Opcode::SuperCall | Opcode::SuperCallSpread | Opcode::SuperCallForward => {
                        let args = if instruction.opcode == Opcode::SuperCall {
                            let base = self.stack.len() - operand;
                            let args = self.stack[base..].to_vec();
                            self.stack.truncate(base);
                            args
                        } else if instruction.opcode == Opcode::SuperCallSpread {
                            let arguments = self.pop();
                            self.array_like_values(&arguments)?
                        } else {
                            self.arguments.clone()
                        };
                        let value = self.super_call(args)?;
                        self.stack.push(value);
                    }
                    Opcode::EnterClassFieldInitializer => self.class_field_initializer_depth += 1,
                    Opcode::LeaveClassFieldInitializer => {
                        self.class_field_initializer_depth = self
                            .class_field_initializer_depth
                            .checked_sub(1)
                            .expect("compiler balances class field initializers");
                    }
                    Opcode::RegExpLiteral => {
                        let base = self.stack.len() - 2;
                        let regexp = self.regexp_create(
                            &self.stack[base].clone(),
                            &self.stack[base + 1].clone(),
                        )?;
                        self.stack.truncate(base);
                        self.stack.push(regexp);
                    }
                    Opcode::TemplateObject => {
                        let object = self.template_object(&code.templates[operand])?;
                        self.stack.push(object);
                    }
                    Opcode::GetIterator => {
                        let value = self.stack.last().unwrap().clone();
                        let iterator = self.get_iterator(&value)?;
                        self.pop();
                        self.stack.push(iterator.clone());
                        iterators.push(iterator);
                    }
                    Opcode::GetAsyncIterator => {
                        let value = self.stack.last().unwrap().clone();
                        let iterator = self.get_async_iterator(&value)?;
                        self.pop();
                        self.stack.push(iterator.clone());
                        iterators.push(iterator);
                    }
                    Opcode::ForInKeys => {
                        let source = self.stack.last().expect("for-in has a source").clone();
                        let keys = self.for_in_keys(&source)?;
                        self.pop();
                        self.stack.push(keys);
                    }
                    Opcode::IteratorStep => {
                        let record = self.stack.last().unwrap().clone();
                        iterators.retain(|active| active != &record);
                        let result = self.iterator_step(&record, true)?;
                        self.pop();
                        if let Some(value) = result {
                            iterators.push(record);
                            self.stack.push(value);
                        } else {
                            pc = operand;
                        }
                    }
                    Opcode::AsyncIteratorNext => {
                        let (record, argument) = if operand == 0 {
                            (self.stack.last().unwrap().clone(), None)
                        } else {
                            let base = self.stack.len() - 2;
                            let record = self.stack[base].clone();
                            let argument = self.stack[base + 1].clone();
                            self.stack.truncate(base + 1);
                            (record, Some(argument))
                        };
                        let promise = self.async_iterator_next(&record, argument)?;
                        self.stack.push(promise);
                    }
                    Opcode::AsyncIteratorStep => {
                        let base = self.stack.len() - 2;
                        let record = self.stack[base].clone();
                        let result = self.stack[base + 1].clone();
                        iterators.retain(|active| active != &record);
                        let value = self.async_iterator_step(&record, &result)?;
                        self.stack.truncate(base);
                        if let Some(value) = value {
                            iterators.push(record);
                            self.stack.push(value);
                        } else {
                            pc = operand;
                        }
                    }
                    Opcode::AsyncIteratorStepValue => {
                        // yield* needs the completed iterator result's value
                        // as its own expression result, unlike for-await.
                        let base = self.stack.len() - 2;
                        let record = self.stack[base].clone();
                        let result = self.stack[base + 1].clone();
                        iterators.retain(|active| active != &record);
                        let Value::Object(record_id) = record else {
                            unreachable!("compiler only emits iterator records")
                        };
                        if !matches!(result, Value::Object(_)) {
                            return Err(RuntimeError::TypeError(
                                "async iterator result must be an object".into(),
                            ));
                        }
                        let done = self.get_property(&result, &"done".into())?;
                        let value = self.get_property(&result, &"value".into())?;
                        self.stack.truncate(base);
                        if self.to_boolean(&done)? {
                            self.with_roots(|heap| heap.set(record_id, "done", Value::Bool(true)))?;
                            self.stack.push(value);
                            pc = operand;
                        } else {
                            iterators.push(Value::Object(record_id));
                            self.stack.push(Value::Object(record_id));
                            self.stack.push(value);
                        }
                    }
                    Opcode::IteratorStepReference => {
                        let base = self.stack.len() - 3;
                        let record = self.stack[base].clone();
                        iterators.retain(|active| active != &record);
                        let result = self.iterator_step(&record, true)?;
                        self.stack.remove(base);
                        if let Some(value) = result {
                            iterators.push(record);
                            self.stack.push(value);
                        } else {
                            pc = operand;
                        }
                    }
                    Opcode::IteratorElision => {
                        let record = self.stack.last().unwrap().clone();
                        iterators.retain(|active| active != &record);
                        if self.iterator_step(&record, false)?.is_some() {
                            iterators.push(record);
                        }
                    }
                    Opcode::IteratorClose => {
                        let record = self.stack.last().unwrap().clone();
                        iterators.retain(|active| active != &record);
                        self.iterator_close(&record)?;
                        self.pop();
                    }
                    Opcode::IteratorFinish => {
                        let record = self.stack.last().unwrap().clone();
                        let Value::Object(id) = record else {
                            unreachable!("compiler only emits iterator records")
                        };
                        let done =
                            matches!(self.heap.get_own(id, "done")?, Some(Value::Bool(true)));
                        iterators.retain(|candidate| candidate != &record);
                        if !done {
                            self.iterator_close(&record)?;
                        }
                        self.pop();
                    }
                    Opcode::IteratorRest => {
                        let record = self.stack.last().unwrap().clone();
                        let base = self.stack.len() - 1;
                        iterators.retain(|candidate| candidate != &record);
                        iterators.push(record.clone());
                        // Keep accumulated values in a rooted managed array while
                        // subsequent next/done/value callbacks can trigger GC.
                        let array = self.array_from(Vec::new())?;
                        self.stack.push(array.clone());
                        while let Some(value) = self.iterator_step(&record, true)? {
                            self.charge_step()?;
                            self.array_push(&array, &value, 0)?;
                        }
                        iterators.retain(|candidate| candidate != &record);
                        self.stack.truncate(base);
                        self.stack.push(array);
                    }
                    Opcode::IteratorRestReference => {
                        let base = self.stack.len() - 3;
                        let record = self.stack[base].clone();
                        iterators.retain(|active| active != &record);
                        iterators.push(record.clone());
                        let array = self.array_from(Vec::new())?;
                        self.stack.push(array.clone());
                        while let Some(value) = self.iterator_step(&record, true)? {
                            self.charge_step()?;
                            self.array_push(&array, &value, 0)?;
                        }
                        iterators.retain(|active| active != &record);
                        self.stack.remove(base);
                    }
                    Opcode::RequireObject => {
                        let value = self.stack.last().unwrap().clone();
                        self.coerce_object(&value)?;
                    }
                    Opcode::DestructureProperty => {
                        let base = self.stack.len() - 3;
                        let source = self.stack[base].clone();
                        let excluded = self.stack[base + 1].clone();
                        let key = self.coerce_property_key(&self.stack[base + 2].clone())?;
                        let value = self.get_property(&source, &key)?;
                        self.array_push(&excluded, &key.value(), 0)?;
                        self.stack.truncate(base);
                        self.stack.extend([source, excluded, value]);
                    }
                    Opcode::DestructurePropertyReference => {
                        // [..., source, excluded, source-key, source-key,
                        // target-object, raw-target-key] becomes [...,
                        // source, excluded, target-object, raw-target-key,
                        // value].
                        // The duplicate source key keeps target evaluation
                        // ahead of GetV without changing excluded-key order.
                        let base = self.stack.len() - 6;
                        let source = self.stack[base].clone();
                        let excluded = self.stack[base + 1].clone();
                        let source_key = self.coerce_property_key(&self.stack[base + 2].clone())?;
                        let object = self.stack[base + 4].clone();
                        let raw_target_key = self.stack[base + 5].clone();
                        let value = self.get_property(&source, &source_key)?;
                        self.array_push(&excluded, &source_key.value(), 0)?;
                        self.stack.truncate(base);
                        self.stack
                            .extend([source, excluded, object, raw_target_key, value]);
                    }
                    Opcode::ObjectRest => {
                        let base = self.stack.len() - 2;
                        let source = self.stack[base].clone();
                        let excluded = self.stack[base + 1].clone();
                        let rest = self.destructure_object_rest(&source, &excluded)?;
                        self.stack.truncate(base);
                        self.stack.push(rest);
                    }
                    Opcode::CopyDataProperties => {
                        let base = self.stack.len() - 2;
                        let Value::Object(target) = self.stack[base].clone() else {
                            unreachable!("compiler creates an object literal target")
                        };
                        let source = self.stack[base + 1].clone();
                        self.copy_data_properties(target, &source, &[])?;
                        self.pop();
                    }
                    Opcode::Closure => {
                        let child = code.functions[operand].clone();
                        let function_prototype = if child.async_function {
                            self.async_function_prototype()?
                        } else {
                            self.function_prototype()?
                        };
                        let mut captures = Vec::new();
                        for &slot in &child.captures {
                            captures.push(self.capture(slot as usize)?);
                        }
                        let this = if child.arrow {
                            if self.this == Value::Undefined
                                && self.call_depth == 0
                                && !self.top_level_module
                            {
                                self.global("globalThis")?
                            } else {
                                self.this.clone()
                            }
                        } else {
                            Value::Undefined
                        };
                        let id = self.with_roots(|heap| {
                            heap.alloc_closure(child.clone(), captures, this, function_prototype)
                        })?;
                        if let Some(module) = &self.active_module_name {
                            self.module_closure_referrers.insert(id, module.clone());
                        }
                        self.stack.push(Value::Object(id));
                        // Arrow functions inherit their containing function's
                        // [[HomeObject]] together with lexical `this`.  Keeping
                        // the new closure on the operand stack first makes it a
                        // GC root while installing metadata may allocate.
                        if child.arrow {
                            if let Some(home) = self.home_object {
                                self.with_roots(|heap| heap.set_closure_home(id, home))?;
                            }
                            // A derived constructor's arrow may invoke `super()`.
                            // Store its resolved superclass on the arrow closure;
                            // the call frame then treats that closure as the
                            // lexical derived-constructor context.
                            if let Some(constructor) = self.class_constructor {
                                if let Some(base) = self.heap.class_base(constructor)? {
                                    self.with_roots(|heap| heap.set_class_base(id, base))?;
                                }
                            }
                        }
                        self.define_data(
                            id,
                            "name",
                            Value::String(child.function_name.clone().into()),
                            false,
                            false,
                            true,
                        )?;
                        self.define_data(
                            id,
                            "length",
                            Value::Number(child.function_length as f64),
                            false,
                            false,
                            true,
                        )?;
                        if child.generator {
                            // Generator function objects are not constructors,
                            // but each owns the prototype used for iterators it
                            // creates. The own object inherits the shared
                            // generator/async-generator prototype and remains
                            // replaceable by user code.
                            let base_prototype = if child.async_function {
                                self.async_generator_prototype()?
                            } else {
                                self.generator_prototype()?
                            };
                            let prototype =
                                self.with_roots(|heap| heap.alloc_object(Some(base_prototype)))?;
                            self.define_data(
                                id,
                                "prototype",
                                Value::Object(prototype),
                                true,
                                false,
                                false,
                            )?;
                        } else if child.constructible {
                            let object_prototype = self.object_prototype;
                            let prototype =
                                self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                            self.define_data(
                                id,
                                "prototype",
                                Value::Object(prototype),
                                !child.class_constructor,
                                false,
                                false,
                            )?;
                            self.define_data(
                                prototype,
                                "constructor",
                                Value::Object(id),
                                true,
                                false,
                                true,
                            )?;
                        }
                    }
                    Opcode::This => {
                        if self.this == Value::Undefined
                            && self.call_depth == 0
                            && !self.top_level_module
                        {
                            self.this = self.global("globalThis")?;
                        }
                        self.stack.push(self.this.clone());
                    }
                    Opcode::NewTarget => self.stack.push(self.new_target.clone()),
                    Opcode::Argument => self
                        .stack
                        .push(native::argument(&self.arguments, operand).clone()),
                    Opcode::RestArguments => {
                        let array = self
                            .array_from(self.arguments.iter().skip(operand).cloned().collect())?;
                        self.stack.push(array);
                    }
                    Opcode::ArgumentsObject => self.create_arguments_object(code)?,
                    Opcode::Return => return Ok(Some(Completion::Return(self.pop()))),
                    Opcode::TailRecur => {
                        let base = self.stack.len() - operand;
                        let args = self.stack[base..].to_vec();
                        self.stack.truncate(base);
                        return Ok(Some(Completion::TailRecur(args)));
                    }
                    Opcode::Yield => {
                        if !code.generator {
                            return Err(RuntimeError::TypeError(
                                "yield is not supported in this execution context".into(),
                            ));
                        }
                        return Ok(Some(Completion::Yield(self.pop())));
                    }
                    Opcode::Await => {
                        // Await always applies PromiseResolve before it
                        // observes settlement, so plain thenables use the
                        // same queued resolution path as Promise values.
                        let awaited_value = self.pop();
                        let awaited = self.promise_resolve(awaited_value)?;
                        if code.async_function || (self.top_level_module && self.call_depth == 0) {
                            suspended_await = Some(
                                awaited
                                    .object_id()
                                    .expect("PromiseResolve returns a Promise object"),
                            );
                            return Ok(None);
                        }
                        // Await resumes only after the next queued turn. A
                        // fulfilled Promise still crosses that boundary; a
                        // pending one may need several pre-existing jobs to
                        // settle, but we never collapse later turns into the
                        // current continuation.
                        loop {
                            let pending = awaited
                                .object_id()
                                .and_then(|promise| self.promises.get(&promise))
                                .is_some_and(|record| {
                                    matches!(record.status, PromiseStatus::Pending)
                                });
                            let advanced = self.run_next_job_while_module_suspended()?;
                            if !pending || !advanced {
                                break;
                            }
                        }
                        let value = self.await_value(awaited)?;
                        self.stack.push(value);
                    }
                    Opcode::DynamicImport => {
                        let specifier = self.pop();
                        let promise = self.dynamic_import(specifier)?;
                        self.stack.push(promise);
                    }
                    Opcode::EnterWith => {
                        let object = self.pop();
                        let object = self.coerce_object(&object)?;
                        self.with_objects.push(Value::Object(object));
                    }
                    Opcode::LeaveWith => {
                        self.with_objects
                            .pop()
                            .expect("compiler balances with scopes");
                    }
                    Opcode::WithGet => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        let name = name.to_utf8().expect("compiler emits a UTF-8 identifier");
                        let fallback = code
                            .bindings
                            .iter()
                            // Captures precede function-local bindings in
                            // bytecode. An object-environment miss therefore
                            // resolves the innermost matching slot.
                            .rposition(|binding| binding.name == name)
                            .map(|slot| self.eval_aware_binding_value(slot, &name))
                            .transpose()?;
                        let value = self.with_get(&name, fallback)?;
                        self.stack.push(value);
                    }
                    Opcode::WithSet => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        self.with_set(
                            &name.to_utf8().expect("compiler emits a UTF-8 identifier"),
                            self.stack.last().expect("assignment has a value").clone(),
                        )?;
                    }
                    Opcode::ResolveWithReference => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        let name = name.to_utf8().expect("compiler emits a UTF-8 identifier");
                        let mut object_reference = None;
                        for object in self.with_objects.clone().into_iter().rev() {
                            if self.with_has_binding(&object, &name)? {
                                object_reference = Some(object);
                                break;
                            }
                        }
                        if let Some(object) = object_reference {
                            // The pair is a property reference. Keeping it on
                            // the operand stack makes it survive calls, handler
                            // unwinding and generator suspension just like an
                            // ordinary member-reference pair.
                            self.stack.push(object);
                            self.stack.push(Value::String(name.into()));
                        } else if let Some(slot) = code
                            .bindings
                            .iter()
                            .rposition(|binding| binding.name == name)
                        {
                            // `Null` tags an internal binding reference; the
                            // slot is safe because it is compiler-owned and is
                            // consumed only by StoreWithReference.
                            self.stack.push(Value::Number(slot as f64));
                            self.stack.push(Value::Null);
                        } else {
                            // `Undefined` tags an unresolvable reference and
                            // preserves its source name for sloppy PutValue.
                            self.stack.push(Value::Undefined);
                            self.stack.push(Value::String(name.into()));
                        }
                    }
                    Opcode::LoadWithReference => {
                        let marker = self.pop();
                        let target = self.pop();
                        let value = match (&target, &marker) {
                            (Value::Object(object), Value::String(name)) => {
                                self.get_property(&Value::Object(*object), &name.clone().into())?
                            }
                            (Value::Number(slot), Value::Null)
                                if slot.is_finite()
                                    && *slot >= 0.0
                                    && slot.fract() == 0.0
                                    && (*slot as usize) < code.bindings.len() =>
                            {
                                let slot = *slot as usize;
                                self.eval_aware_binding_value(slot, &code.bindings[slot].name)?
                                    .ok_or_else(|| {
                                        RuntimeError::ReferenceError(
                                            code.bindings[slot].name.clone(),
                                        )
                                    })?
                            }
                            (Value::Undefined, Value::String(name)) => {
                                return Err(RuntimeError::ReferenceError(
                                    name.to_utf8().expect("compiler emits a UTF-8 identifier"),
                                ));
                            }
                            _ => unreachable!("compiler emits a valid with reference"),
                        };
                        // Preserve the original Reference for PutValue after
                        // the RHS has run. `get_property` may invoke a getter,
                        // so stack-resident values are the GC roots here.
                        self.stack.push(target);
                        self.stack.push(marker);
                        self.stack.push(value);
                    }
                    Opcode::StoreWithReference => {
                        let value = self.pop();
                        let marker = self.pop();
                        let target = self.pop();
                        match (target, marker) {
                            (Value::Object(object), Value::String(name)) => {
                                self.set_property(&Value::Object(object), &name.into(), &value)?;
                            }
                            (Value::Number(slot), Value::Null)
                                if slot.is_finite()
                                    && slot >= 0.0
                                    && slot.fract() == 0.0
                                    && (slot as usize) < code.bindings.len() =>
                            {
                                let slot = slot as usize;
                                let name = &code.bindings[slot].name;
                                if !self.store_dynamic_eval_shadowing_binding(
                                    slot,
                                    name,
                                    value.clone(),
                                )? {
                                    if self.binding_value(slot)?.is_none() {
                                        return Err(RuntimeError::ReferenceError(name.clone()));
                                    }
                                    if binding_allows_assignment(&code.bindings[slot], code.strict)?
                                    {
                                        self.store_binding(slot, value.clone())?;
                                    }
                                }
                            }
                            (Value::Undefined, Value::String(name)) => {
                                let name =
                                    name.to_utf8().expect("compiler emits a UTF-8 identifier");
                                if !self.set_dynamic_eval_binding(&name, value.clone())?
                                    && !self.set_global_binding(&name, value.clone())?
                                {
                                    let global = self.global("globalThis")?;
                                    self.set_property(&global, &name.into(), &value)?;
                                }
                            }
                            _ => unreachable!("compiler emits a valid with reference"),
                        }
                        self.stack.push(value);
                    }
                    Opcode::Global => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!()
                        };
                        let value = self.global(&name.to_utf8().unwrap())?;
                        self.stack.push(value);
                    }
                    Opcode::ToPropertyKey => {
                        let value = self.stack.last().unwrap().clone();
                        let key = self.coerce_property_key(&value)?;
                        self.pop();
                        self.stack.push(key.value());
                    }
                    Opcode::PreparePropertyReference => {
                        let base = self.stack.len() - 2;
                        if matches!(self.stack[base], Value::Null | Value::Undefined) {
                            return Err(RuntimeError::TypeError(
                                "cannot access a property of null or undefined".into(),
                            ));
                        }
                        let key = self.coerce_property_key(&self.stack[base + 1].clone())?;
                        self.stack[base + 1] = key.value();
                    }
                    Opcode::Constant => {
                        self.check_string(&code.constants[operand])?;
                        self.stack.push(code.constants[operand].clone());
                    }
                    Opcode::GlobalString => {
                        let (constructor, _) = self.string_intrinsics()?;
                        self.stack.push(Value::Object(constructor));
                    }
                    Opcode::GetBinding => {
                        let value = self
                            .eval_aware_binding_value(operand, &code.bindings[operand].name)?
                            .ok_or_else(|| {
                                RuntimeError::ReferenceError(code.bindings[operand].name.clone())
                            })?;
                        self.stack.push(value);
                    }
                    Opcode::ResolveBindingReference => {
                        let slot = operand;
                        let dynamic = self.cells.get(&slot).and_then(|cell| {
                            self.dynamic_eval_shadowing_cell(&code.bindings[slot].name, *cell)
                        });
                        // `Number(slot), Null` is a static binding reference;
                        // replacing Null with a cell records a dynamic eval
                        // binding that was already visible at resolution.
                        self.stack.push(Value::Number(slot as f64));
                        self.stack
                            .push(dynamic.map(Value::Object).unwrap_or(Value::Null));
                    }
                    Opcode::LoadBindingReference => {
                        let marker = self.pop();
                        let target = self.pop();
                        let Value::Number(slot) = target else {
                            panic!("compiler emits a binding reference: target={target:?}, marker={marker:?}")
                        };
                        let slot = slot as usize;
                        let value = match marker {
                            Value::Null => self.binding_value(slot)?,
                            Value::Object(cell) => self.heap.get_own(cell, "value")?,
                            _ => unreachable!("compiler emits a binding reference marker"),
                        }
                        .ok_or_else(|| {
                            RuntimeError::ReferenceError(code.bindings[slot].name.clone())
                        })?;
                        self.stack.push(Value::Number(slot as f64));
                        self.stack.push(marker);
                        self.stack.push(value);
                    }
                    Opcode::InitializeBinding => {
                        let value = self.pop();
                        self.store_binding(operand, value)?;
                    }
                    Opcode::StoreBinding => {
                        // ECMA-262 §9.1.1.1.5: TDZ takes precedence over the
                        // immutable-binding assignment error, including const.
                        let name = &code.bindings[operand].name;
                        let value = self.stack.last().expect("store has a value").clone();
                        if !self.store_dynamic_eval_shadowing_binding(operand, name, value)? {
                            if self.binding_value(operand)?.is_none() {
                                return Err(RuntimeError::ReferenceError(name.clone()));
                            }
                            if binding_allows_assignment(&code.bindings[operand], code.strict)? {
                                self.store_binding(
                                    operand,
                                    self.stack.last().expect("store has a value").clone(),
                                )?;
                            }
                        }
                    }
                    Opcode::StoreBindingReference => {
                        let (target, marker, value, result) = if operand == 0 {
                            let value = self.pop();
                            let marker = self.pop();
                            let target = self.pop();
                            (target, marker, value.clone(), value)
                        } else {
                            // A postfix update leaves its original value
                            // below the reference's new value.
                            let base = self.stack.len() - 4;
                            let target = self.stack[base].clone();
                            let marker = self.stack[base + 1].clone();
                            let result = self.stack[base + 2].clone();
                            let value = self.stack[base + 3].clone();
                            self.stack.truncate(base);
                            (target, marker, value, result)
                        };
                        let Value::Number(slot) = target else {
                            unreachable!("compiler emits a binding reference")
                        };
                        let slot = slot as usize;
                        match marker {
                            Value::Null => {
                                if self.binding_value(slot)?.is_none() {
                                    return Err(RuntimeError::ReferenceError(
                                        code.bindings[slot].name.clone(),
                                    ));
                                }
                                if binding_allows_assignment(&code.bindings[slot], code.strict)? {
                                    self.store_binding(slot, value.clone())?;
                                }
                            }
                            Value::Object(cell) => self.store_global_cell(cell, value.clone())?,
                            _ => unreachable!("compiler emits a binding reference marker"),
                        }
                        if operand != 0 {
                            // The compiler removes the new value first and
                            // leaves the old value as the postfix result.
                            self.stack.push(result);
                            self.stack.push(value);
                        } else {
                            self.stack.push(result);
                        }
                    }
                    Opcode::UnboundName | Opcode::TypeofName => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        let name = name.to_utf8().expect("compiler emits a UTF-8 identifier");
                        let value = self.lookup_global_name(&name)?;
                        if instruction.opcode == Opcode::TypeofName {
                            let value = value.unwrap_or(Value::Undefined);
                            self.stack
                                .push(Value::String(self.typeof_value(&value)?.into()));
                        } else {
                            self.stack
                                .push(value.ok_or(RuntimeError::ReferenceError(name))?);
                        }
                    }
                    Opcode::SetUnboundName => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        let name = name.to_utf8().expect("compiler emits a UTF-8 identifier");
                        let value = self.stack.last().expect("assignment has a value").clone();
                        if !self.set_dynamic_eval_binding(&name, value.clone())?
                            && !self.set_global_binding(&name, value.clone())?
                        {
                            let global = self.global("globalThis")?;
                            let global_id = global.object_id().expect("globalThis is an object");
                            let key: PropertyName = name.as_str().into();
                            if code.strict && !self.has_property(global_id, &key)? {
                                return Err(RuntimeError::ReferenceError(name));
                            }
                            self.set_property(&global, &key, &value)?;
                        }
                    }
                    Opcode::DeleteUnboundName => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        let name = name.to_utf8().expect("compiler emits a UTF-8 identifier");
                        let deleted = self.delete_dynamic_eval_binding(&name)?;
                        self.stack.push(Value::Bool(deleted));
                    }
                    Opcode::DeleteDynamicBinding => {
                        let name = &code.bindings[operand].name;
                        let deleted = self.delete_dynamic_eval_binding(name)?;
                        self.stack.push(Value::Bool(deleted));
                    }
                    Opcode::EnterScope => {
                        for slot in &code.scopes[operand] {
                            if let Some(name) = self.eval_dynamic_slots.get(&(*slot as usize)) {
                                let cell = self
                                    .dynamic_eval_bindings
                                    .get(name)
                                    .expect("prepared dynamic eval binding survives execution")
                                    .cell;
                                self.cells.insert(*slot as usize, cell);
                                continue;
                            }
                            if let Some(name) = self.script_global_slots.get(&(*slot as usize)) {
                                let cell = self
                                    .global_bindings
                                    .get(name)
                                    .expect("prepared global binding survives script execution")
                                    .cell;
                                self.cells.insert(*slot as usize, cell);
                                continue;
                            }
                            if code.module
                                && operand == 0
                                && self.cells.contains_key(&(*slot as usize))
                            {
                                // ModuleDeclarationInstantiation seeded this
                                // slot (or linked it to an exporter cell).
                                continue;
                            }
                            self.cells.remove(&(*slot as usize));
                            self.bindings[*slot as usize] =
                                if !code.bindings[*slot as usize].lexical {
                                    Some(Value::Undefined)
                                } else {
                                    None
                                };
                        }
                        self.active_scopes.push(operand as u32);
                        self.active_scope_slots.push(code.scopes[operand].clone());
                    }
                    Opcode::CloneScope => self.clone_scope(code, operand as u32)?,
                    Opcode::LeaveScope => self.leave_scope(code, operand as u32),
                    Opcode::Pop => {
                        self.pop();
                    }
                    Opcode::Dup => self
                        .stack
                        .push(self.stack.last().expect("dup has a value").clone()),
                    Opcode::Dup2 => {
                        let index = self.stack.len() - 2;
                        self.stack.push(self.stack[index].clone());
                        self.stack.push(self.stack[index + 1].clone());
                    }
                    Opcode::Add => self.binary(Self::add)?,
                    Opcode::Subtract => self.numeric(|a, b| a - b)?,
                    Opcode::Multiply => self.numeric(|a, b| a * b)?,
                    Opcode::Exponentiate => self.exponentiate()?,
                    Opcode::Divide => self.numeric(|a, b| a / b)?,
                    Opcode::Remainder => self.numeric(|a, b| a % b)?,
                    Opcode::ShiftLeft | Opcode::ShiftRight | Opcode::UnsignedShiftRight => {
                        self.shift(instruction.opcode)?
                    }
                    Opcode::BitAnd | Opcode::BitXor | Opcode::BitOr => {
                        self.bitwise(instruction.opcode)?
                    }
                    Opcode::StrictEqual => self.binary(|_, a, b| Ok(Value::Bool(a == b)))?,
                    Opcode::StrictNotEqual => self.binary(|_, a, b| Ok(Value::Bool(a != b)))?,
                    Opcode::Equal => {
                        self.binary(|vm, a, b| vm.loose_equal(a, b).map(Value::Bool))?
                    }
                    Opcode::NotEqual => self
                        .binary(|vm, a, b| vm.loose_equal(a, b).map(|equal| Value::Bool(!equal)))?,
                    Opcode::Instanceof => self.binary(|vm, value, target| {
                        vm.has_instance(value, target, false).map(Value::Bool)
                    })?,
                    Opcode::In => self
                        .binary(|vm, key, object| vm.property_in(&key, &object).map(Value::Bool))?,
                    Opcode::Less => self.relational(|order| order == Ordering::Less)?,
                    Opcode::Greater => self.relational(|order| order == Ordering::Greater)?,
                    Opcode::LessEqual => self.relational(|order| order != Ordering::Greater)?,
                    Opcode::GreaterEqual => self.relational(|order| order != Ordering::Less)?,
                    Opcode::Negate
                    | Opcode::BitNot
                    | Opcode::ToNumber
                    | Opcode::ToString
                    | Opcode::Not
                    | Opcode::Typeof => {
                        let arg = self.stack.last().unwrap().clone();
                        let value = match instruction.opcode {
                            Opcode::Negate => self.negate(&arg)?,
                            Opcode::BitNot => self.bit_not(&arg)?,
                            Opcode::ToNumber => Value::Number(self.coerce_number(&arg)?),
                            Opcode::ToString => Value::String(self.coerce_string(&arg)?),
                            Opcode::Not => Value::Bool(!self.to_boolean(&arg)?),
                            _ => Value::String(self.typeof_value(&arg)?.into()),
                        };
                        self.check_string(&value)?;
                        self.pop();
                        self.stack.push(value);
                    }
                    Opcode::Jump => pc = operand,
                    Opcode::JumpIfFalse | Opcode::JumpIfTrue | Opcode::JumpIfNotNullish => {
                        let arg = self.pop();
                        let take = match instruction.opcode {
                            Opcode::JumpIfFalse => !self.to_boolean(&arg)?,
                            Opcode::JumpIfTrue => self.to_boolean(&arg)?,
                            _ => !matches!(arg, Value::Null | Value::Undefined),
                        };
                        if take {
                            pc = operand;
                        }
                    }
                    Opcode::NewObject => {
                        let prototype = self.object_prototype;
                        let id = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                        self.stack.push(Value::Object(id));
                    }
                    Opcode::NewArray => {
                        let prototype = self.array_prototype;
                        let id = self
                            .with_roots(|heap| heap.alloc_array(operand as u32, Some(prototype)))?;
                        self.stack.push(Value::Object(id));
                    }
                    Opcode::GetProperty | Opcode::GetMethod => {
                        let (receiver, key) = self.property_reference()?;
                        let value = self.get_property(&receiver, &key)?;
                        self.check_string(&value)?;
                        self.stack.push(value);
                        if instruction.opcode == Opcode::GetMethod {
                            self.stack.push(receiver);
                        }
                    }
                    Opcode::Call | Opcode::DirectEval | Opcode::Construct => {
                        // Leave every call input on the stack until dispatch
                        // completes, so native allocations see all GC roots.
                        let base = self.stack.len() - operand - 2;
                        let callee = self.stack[base].clone();
                        let receiver = self.stack[base + 1].clone();
                        let args = self.stack[base + 2..].to_vec();
                        let result = if instruction.opcode == Opcode::DirectEval
                            && self.is_intrinsic_eval(&callee)?
                        {
                            self.direct_eval(native::argument(&args, 0))?
                        } else {
                            self.call_native(
                                callee,
                                receiver,
                                args,
                                instruction.opcode == Opcode::Construct,
                            )?
                        };
                        self.check_string(&result)?;
                        self.stack.truncate(base);
                        self.stack.push(result);
                    }
                    Opcode::DiscardReference => {
                        let value = self.pop();
                        let reference_values = operand;
                        let reference_start = self
                            .stack
                            .len()
                            .checked_sub(reference_values)
                            .expect("compiler retains a complete reference");
                        self.stack.truncate(reference_start);
                        self.stack.push(value);
                    }
                    Opcode::SetProperty => {
                        let value = self.pop();
                        let (object, key) = self.property_reference()?;
                        self.set_property(&object, &key, &value)?;
                        self.stack.push(value);
                    }
                    Opcode::SetDestructureProperty => {
                        // A destructuring leaf has already produced its value;
                        // evaluating a member target appends its object/key after
                        // that value. Preserve the value for the caller to pop.
                        let base = self.stack.len() - 3;
                        let value = self.stack[base].clone();
                        let object = self.stack[base + 1].clone();
                        let key = self.coerce_property_key(&self.stack[base + 2].clone())?;
                        self.set_property(&object, &key, &value)?;
                        self.stack.truncate(base);
                        self.stack.push(value);
                    }
                    Opcode::SetDestructurePropertyReference => {
                        // IteratorStepReference removed the iterator record,
                        // leaving object, raw key and element value in order.
                        let base = self.stack.len() - 3;
                        let object = self.stack[base].clone();
                        let key = self.coerce_property_key(&self.stack[base + 1].clone())?;
                        let value = self.stack[base + 2].clone();
                        self.set_property(&object, &key, &value)?;
                        self.stack.truncate(base);
                        self.stack.push(value);
                    }
                    Opcode::UpdateProperty => {
                        let (object, key) = self.property_reference()?;
                        self.stack.push(object.clone());
                        let old = self.get_property(&object, &key)?;
                        let old = self.coerce_number(&old)?;
                        let new = if operand & 1 == 0 {
                            old + 1.0
                        } else {
                            old - 1.0
                        };
                        self.set_property(&object, &key, &Value::Number(new))?;
                        self.stack.pop();
                        self.stack
                            .push(Value::Number(if operand & 2 == 0 { old } else { new }));
                    }
                    Opcode::SetLiteralPrototype => {
                        let value = self.pop();
                        let Value::Object(object) = self.pop() else {
                            unreachable!("literal receiver is an object")
                        };
                        match value {
                            Value::Object(prototype) => {
                                self.heap.set_prototype(object, Some(prototype))?
                            }
                            Value::Null => self.heap.set_prototype(object, None)?,
                            _ => {} // Literal __proto__ with a primitive value has no effect.
                        }
                    }
                    Opcode::SetCompletion => {
                        self.completion = self.pop();
                        self.completion_empty = false;
                    }
                    Opcode::ClearCompletion => {
                        self.completion = Value::Undefined;
                        self.completion_empty = true;
                    }
                    Opcode::PushHandler => {
                        debug_assert!(operand < code.handlers.len());
                        handlers.push(HandlerFrame {
                            metadata: operand,
                            stack_depth: self.stack.len(),
                            scope_depth: self.active_scopes.len(),
                            iterator_depth: iterators.len(),
                            with_depth: self.with_objects.len(),
                            state: HandlerState::Try,
                            pending: None,
                        });
                    }
                    Opcode::PopHandler => {
                        handlers
                            .pop()
                            .expect("compiler pops its active try handler");
                    }
                    Opcode::SaveCompletion => self
                        .completion_saves
                        .push((self.completion.clone(), self.completion_empty)),
                    Opcode::ResumeCompletion => return Ok(Some(Completion::Resume(operand))),
                    Opcode::AbruptJump => {
                        let jump = &code.abrupt_jumps[operand];
                        return Ok(Some(Completion::Jump {
                            cleanup: jump.cleanup as usize,
                            target: jump.target as usize,
                        }));
                    }
                    Opcode::Halt => return Ok(Some(Completion::Halt(self.completion.clone()))),
                }
                Ok(None)
            })();
            let completion = match outcome {
                Ok(completion) => completion,
                Err(error) if error.is_catchable() => Some(Completion::Throw(error)),
                Err(error) => return Err(error),
            };
            if let Some(promise) = suspended_await.take() {
                for handler in &mut handlers {
                    handler.stack_depth -= handler_stack_base;
                }
                return Ok(InterpreterExit::Await {
                    promise,
                    pc,
                    handlers: std::mem::take(&mut handlers),
                });
            }
            if let Some(completion) = completion {
                if let Completion::Yield(value) = completion {
                    for handler in &mut handlers {
                        handler.stack_depth -= handler_stack_base;
                    }
                    return Ok(InterpreterExit::Yield {
                        value,
                        pc,
                        iterators: std::mem::take(iterators),
                        handlers: std::mem::take(&mut handlers),
                    });
                }
                match self.resolve_completion(code, &mut handlers, iterators, completion)? {
                    CompletionAction::Continue => {}
                    CompletionAction::Jump(target) => pc = target,
                    CompletionAction::Return(value) => return Ok(InterpreterExit::Return(value)),
                    CompletionAction::TailRecur(args) => {
                        self.stack.truncate(stack_base);
                        self.unwind_scopes(code, 0);
                        self.unwind_with(0);
                        self.close_iterators_to(iterators, 0);
                        handlers.clear();
                        self.pending_completions.truncate(pending_base);
                        self.completion_saves.truncate(save_base);
                        self.completion = Value::Undefined;
                        self.completion_empty = true;
                        self.this = Value::Undefined;
                        self.arguments = args;
                        pc = 0;
                    }
                    CompletionAction::Throw(error) => return Err(error),
                }
            }
        }
    }

    fn charge_step(&mut self) -> Result<(), RuntimeError> {
        if self.remaining_instructions == 0 {
            return Err(RuntimeError::InstructionLimit);
        }
        self.remaining_instructions -= 1;
        Ok(())
    }

    fn get_property(
        &mut self,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        let result = self.get_property_value(receiver, key);
        self.stack.truncate(base);
        result
    }

    fn get_property_value(
        &mut self,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        match receiver {
            Value::Object(id) => self.get_object_property(*id, receiver, key),
            Value::String(string) => {
                if let PropertyName::String(key) = key {
                    if let Some(value) = string.own_property(key) {
                        return Ok(value);
                    }
                }
                let (_, prototype) = self.string_intrinsics()?;
                self.get_from_prototype(prototype, receiver, key)
            }
            Value::Null | Value::Undefined => Err(RuntimeError::TypeError(
                "cannot access a property of null or undefined".into(),
            )),
            _ => {
                let constructor = self.global(match receiver {
                    Value::Symbol(_) => "Symbol",
                    Value::Bool(_) => "Boolean",
                    _ => "Number",
                })?;
                let prototype = self
                    .get_property(&constructor, &"prototype".into())?
                    .object_id()
                    .unwrap();
                self.get_from_prototype(prototype, receiver, key)
            }
        }
    }

    /// [[Get]] with the lookup target separated from the receiver supplied to
    /// accessors and Proxy traps.  Ordinary property syntax supplies the same
    /// object for both arguments; Reflect.get and inherited Proxy operations
    /// intentionally do not.
    pub(super) fn get_object_property(
        &mut self,
        target: ObjectId,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        self.materialize_global_object_property(target, key)?;
        if let Some(cell) = self.global_property_cell(target, key) {
            return self
                .heap
                .get_own(cell, "value")?
                .ok_or_else(|| RuntimeError::ReferenceError("global binding".into()));
        }
        if self.string_intrinsics.is_none()
            && (key == "toString"
                || key == "valueOf"
                || key == "join"
                || key == "forEach"
                || key == "includes"
                || *key == PropertyName::from(JsSymbol::well_known("iterator")))
        {
            self.string_intrinsics()?;
        }
        if key == "propertyIsEnumerable" {
            self.property_is_enumerable_intrinsic()?;
        }
        if key == "hasOwnProperty" {
            self.has_own_property_intrinsic()?;
        }
        // `%Object.prototype%` has an initial own constructor property.
        // Intrinsics otherwise bootstrap lazily, so make it observable before
        // an ordinary object performs an inherited lookup.
        if key == "constructor"
            && self
                .heap
                .get_own_property_descriptor(self.object_prototype, "constructor")?
                .is_none()
        {
            self.global("Object")?;
        }
        if self.heap.proxy(target)?.is_some() {
            return self.proxy_get(target, receiver, key);
        }
        self.get_from_prototype(target, receiver, key)
    }

    fn set_property(
        &mut self,
        receiver: &Value,
        key: &PropertyName,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        self.stack.extend([receiver.clone(), value.clone()]);
        let result = self.set_property_value(receiver, key, value);
        self.stack.truncate(base);
        result
    }

    fn set_property_value(
        &mut self,
        receiver: &Value,
        key: &PropertyName,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        if let Value::Object(object) = receiver {
            self.materialize_global_object_property(*object, key)?;
            if let Some(cell) = self.global_property_cell(*object, key) {
                let result = self.with_roots(|heap| heap.set(*object, key.clone(), value.clone()));
                return match result {
                    Err(RuntimeError::Heap(HeapError::ReadOnlyProperty)) if self.strict => Err(
                        RuntimeError::TypeError("property cannot be assigned".into()),
                    ),
                    Err(RuntimeError::Heap(HeapError::ReadOnlyProperty)) => Ok(()),
                    Ok(()) => self.with_roots(|heap| heap.set(cell, "value", value.clone())),
                    Err(error) => Err(error),
                };
            }
        }
        // ToObject provides the lookup target; accessors retain the original
        // receiver. `ordinary_set_with_receiver` also routes a Proxy found
        // anywhere in the prototype chain through its [[Set]] trap.
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let succeeded = self.ordinary_set_with_receiver(object, receiver, key, value)?;
        if succeeded {
            Ok(())
        } else if self.strict {
            Err(RuntimeError::TypeError(
                "property cannot be assigned".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn property_reference(&mut self) -> Result<(Value, PropertyName), RuntimeError> {
        let key = self.pop();
        let key = self.coerce_property_key(&key)?;
        match self.pop() {
            Value::Null | Value::Undefined => Err(RuntimeError::TypeError(
                "cannot access a property of null or undefined".into(),
            )),
            receiver => Ok((receiver, key)),
        }
    }

    /// Extracts the two stack values used to retain a private Reference and
    /// resolves its owner through the compiler-generated lexical private-name
    /// binding.  Unlike an ordinary property reference, its name is not a
    /// PropertyKey and its receiver is never boxed.
    fn private_reference(
        &mut self,
        owner_slot: usize,
    ) -> Result<(Value, ObjectId, JsString), RuntimeError> {
        let name = match self.pop() {
            Value::String(name) => name,
            _ => unreachable!("compiler emits a string private name"),
        };
        let receiver = self.pop();
        let owner = self
            .binding_value(owner_slot)?
            .and_then(|value| value.object_id())
            .ok_or_else(|| {
                RuntimeError::TypeError(
                    "private elements are not available in this function".into(),
                )
            })?;
        Ok((receiver, owner, name))
    }

    fn private_receiver(
        &self,
        receiver: &Value,
        owner: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("private fields require an object receiver".into())
        })?;
        if !self.heap.has_private_brand(object, owner)? {
            return Err(RuntimeError::TypeError(
                "receiver does not have the requested private element".into(),
            ));
        }
        Ok(object)
    }

    fn private_get(
        &mut self,
        receiver: &Value,
        owner: ObjectId,
        name: &JsString,
    ) -> Result<Value, RuntimeError> {
        let object = self.private_receiver(receiver, owner)?;
        let element = self.heap.private_element(owner, name)?.ok_or_else(|| {
            RuntimeError::TypeError("private element is not declared by this class".into())
        })?;
        match element {
            PrivateElement::Field => {
                self.heap.private_slot(object, owner, name)?.ok_or_else(|| {
                    RuntimeError::TypeError("private field has not been initialized".into())
                })
            }
            PrivateElement::Method(function) => Ok(function),
            PrivateElement::Accessor { get: None, .. } => Ok(Value::Undefined),
            PrivateElement::Accessor {
                get: Some(getter), ..
            } => self.call_native(getter, receiver.clone(), Vec::new(), false),
        }
    }

    fn private_set(
        &mut self,
        receiver: &Value,
        owner: ObjectId,
        name: JsString,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let object = self.private_receiver(receiver, owner)?;
        let element = self.heap.private_element(owner, &name)?.ok_or_else(|| {
            RuntimeError::TypeError("private element is not declared by this class".into())
        })?;
        match element {
            PrivateElement::Field => {
                self.with_roots(|heap| heap.set_private_slot(object, owner, name, value))
            }
            PrivateElement::Method(_) => Err(RuntimeError::TypeError(
                "cannot assign to a private method".into(),
            )),
            PrivateElement::Accessor { set: None, .. } => Err(RuntimeError::TypeError(
                "private accessor has no setter".into(),
            )),
            PrivateElement::Accessor {
                set: Some(setter), ..
            } => {
                self.call_native(setter, receiver.clone(), vec![value], false)?;
                Ok(())
            }
        }
    }

    fn set_class_heritage(&mut self) -> Result<(), RuntimeError> {
        let base = self.pop();
        let class = self
            .stack
            .last()
            .expect("class closure remains on the stack")
            .object_id()
            .expect("compiler emits a class closure before heritage");
        let prototype = self
            .heap
            .get(class, "prototype")?
            .object_id()
            .expect("class constructors have a prototype object");
        let (constructor_parent, instance_parent) = match &base {
            Value::Null => (None, None),
            Value::Object(base) if self.is_constructor(&Value::Object(*base))? => {
                let instance_parent =
                    match self.get_property(&Value::Object(*base), &"prototype".into())? {
                        Value::Object(prototype) => Some(prototype),
                        Value::Null => None,
                        _ => {
                            return Err(RuntimeError::TypeError(
                                "superclass prototype must be an object or null".into(),
                            ))
                        }
                    };
                (Some(*base), instance_parent)
            }
            _ => {
                return Err(RuntimeError::TypeError(
                    "class extends value is not a constructor or null".into(),
                ))
            }
        };
        self.heap.set_prototype(class, constructor_parent)?;
        self.heap.set_prototype(prototype, instance_parent)?;
        self.with_roots(|heap| heap.set_class_base(class, base))?;
        self.with_roots(|heap| heap.set_closure_home(class, prototype))?;
        Ok(())
    }

    fn set_class_home(&mut self) -> Result<(), RuntimeError> {
        let class = self
            .stack
            .last()
            .expect("class closure remains on the stack")
            .object_id()
            .expect("compiler emits a class closure before setting its home object");
        let prototype = self
            .heap
            .get(class, "prototype")?
            .object_id()
            .expect("class constructors have a prototype object");
        self.with_roots(|heap| heap.set_closure_home(class, prototype))?;
        Ok(())
    }

    fn super_base(&mut self) -> Result<ObjectId, RuntimeError> {
        let home = self.home_object.ok_or_else(|| {
            RuntimeError::TypeError("super is not available in this function".into())
        })?;
        self.object_get_prototype(home)?
            .ok_or_else(|| RuntimeError::TypeError("superclass is null".into()))
    }

    fn super_get(&mut self, key: &PropertyName) -> Result<Value, RuntimeError> {
        let base = self.super_base()?;
        self.get_from_prototype(base, &self.this.clone(), key)
    }

    fn super_set(&mut self, key: &PropertyName, value: &Value) -> Result<(), RuntimeError> {
        let base = self.super_base()?;
        let this = self.this.clone();
        if !matches!(this, Value::Object(_)) {
            return Err(RuntimeError::ReferenceError(
                "this is uninitialized before super()".into(),
            ));
        }
        if self.ordinary_set_with_receiver(base, &this, key, value)? {
            Ok(())
        } else {
            self.super_assignment_failed("super property cannot be assigned")
        }
    }

    fn super_assignment_failed(&self, message: &str) -> Result<(), RuntimeError> {
        if self.strict {
            Err(RuntimeError::TypeError(message.into()))
        } else {
            Ok(())
        }
    }

    fn super_call(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        let constructor = self.class_constructor.ok_or_else(|| {
            RuntimeError::TypeError("super() is not available in this function".into())
        })?;
        let base = self.heap.class_base(constructor)?.ok_or_else(|| {
            RuntimeError::TypeError("super() requires a derived constructor".into())
        })?;
        if matches!(base, Value::Null) {
            return Err(RuntimeError::TypeError("super constructor is null".into()));
        }
        let value =
            self.call_with_target(base, Value::Undefined, args, true, self.new_target.clone())?;
        self.this = value.clone();
        Ok(value)
    }
    /// Implements CopyDataProperties for an object-rest binding.  The
    /// compiler supplies an internal array of already-coerced excluded keys;
    /// getters are read from the original source object and copied as normal
    /// enumerable data properties onto a fresh ordinary object.
    fn destructure_object_rest(
        &mut self,
        source: &Value,
        excluded: &Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            let length_value = self.get_property(excluded, &"length".into())?;
            let length = self.coerce_length(&length_value)? as u64;
            let mut excluded_keys = Vec::new();
            for index in 0..length {
                self.charge_step()?;
                let key = self.get_property(excluded, &index.to_string().into())?;
                excluded_keys.push(self.coerce_property_key(&key)?);
            }
            let prototype = self.object_prototype;
            let target = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(target));
            self.copy_data_properties(target, source, &excluded_keys)?;
            self.stack.pop();
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }

    /// The enumerable-property portion of CopyDataProperties.  Object spread
    /// skips nullish sources, while object-rest has already performed
    /// RequireObjectCoercible before arriving here.
    fn copy_data_properties(
        &mut self,
        target: ObjectId,
        source: &Value,
        excluded: &[PropertyName],
    ) -> Result<(), RuntimeError> {
        if matches!(source, Value::Null | Value::Undefined) {
            return Ok(());
        }
        let source_object = self.coerce_object(source)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(source_object));
        let result = (|| {
            for key in self.object_own_property_keys(source_object)? {
                self.charge_step()?;
                if excluded.iter().any(|excluded| excluded == &key) {
                    continue;
                }
                // A Proxy's ownKeys trap is allowed to report a key for which
                // its getOwnPropertyDescriptor trap returns undefined.  This
                // is therefore a conditional copy, rather than an assertion
                // that every reported key still has a descriptor.
                let Some(descriptor) = self.object_get_own_property(source_object, &key)? else {
                    continue;
                };
                if descriptor.enumerable != Some(true) {
                    continue;
                }
                let value = self.get_property(&Value::Object(source_object), &key)?;
                if !self.object_define_own_property(
                    target,
                    key,
                    PropertyDescriptor::data(value, true, true, true),
                )? {
                    return Err(RuntimeError::TypeError(
                        "cannot define copied property".into(),
                    ));
                }
            }
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    /// Snapshots the enumerable string keys visible through an object's
    /// prototype chain. Non-enumerable own keys still suppress an inherited
    /// key with the same name; symbols never participate in `for-in`.
    fn for_in_keys(&mut self, source: &Value) -> Result<Value, RuntimeError> {
        let mut current = Some(self.coerce_object(source)?);
        let base = self.stack.len();
        let mut seen = HashSet::new();
        let mut visited_objects = HashSet::new();
        let mut keys = Vec::new();
        let result = (|| {
            while let Some(object) = current {
                // Keep each traversed object live while Proxy traps execute.
                // A proxy is allowed to return a prototype that is not
                // otherwise reachable from its target or handler.
                self.stack.push(Value::Object(object));
                if !visited_objects.insert(object) {
                    break;
                }
                for key in self.object_own_property_keys(object)? {
                    if !seen.insert(key.clone()) {
                        continue;
                    }
                    if let PropertyName::String(key) = key {
                        if self
                            .object_get_own_property(object, &PropertyName::String(key.clone()))?
                            .is_some_and(|descriptor| descriptor.enumerable == Some(true))
                        {
                            keys.push(Value::String(key));
                        }
                    }
                }
                current = self.object_get_prototype(object)?;
            }
            self.array_from(keys)
        })();
        self.stack.truncate(base);
        result
    }

    fn string_intrinsics(&mut self) -> Result<(ObjectId, ObjectId), RuntimeError> {
        if let Some(intrinsics) = self.string_intrinsics {
            return Ok(intrinsics);
        }
        let object_prototype = self.object_prototype;
        let function_prototype = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::Empty, "", object_prototype)
        })?;
        let constructor = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::String, "String", function_prototype)
        })?;
        let root = self.heap.root(constructor)?;
        let result = (|| {
            let prototype = self.with_roots(|heap| {
                heap.alloc_string(JsString::default(), Some(object_prototype))
            })?;
            self.define_data(
                constructor,
                "prototype",
                Value::Object(prototype),
                false,
                false,
                false,
            )?;
            self.define_data(
                prototype,
                "constructor",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "name",
                Value::String("String".into()),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "length",
                Value::Number(1.0),
                false,
                false,
                true,
            )?;
            self.define_data(
                function_prototype,
                "length",
                Value::Number(0.0),
                false,
                false,
                true,
            )?;
            self.define_data(
                function_prototype,
                "name",
                Value::String(JsString::default()),
                false,
                false,
                true,
            )?;
            self.install_native(
                function_prototype,
                function_prototype,
                "call",
                1,
                NativeFunction::Call,
            )?;
            self.install_native(
                function_prototype,
                function_prototype,
                "apply",
                2,
                NativeFunction::Apply,
            )?;
            self.install_native(
                function_prototype,
                function_prototype,
                "bind",
                1,
                NativeFunction::Bind,
            )?;
            self.install_symbol_native(
                function_prototype,
                function_prototype,
                "hasInstance",
                1,
                NativeFunction::HasInstance,
            )?;
            self.install_native(
                function_prototype,
                function_prototype,
                "toString",
                0,
                NativeFunction::FunctionToString,
            )?;
            self.install_native(
                constructor,
                function_prototype,
                "fromCharCode",
                1,
                NativeFunction::FromCharCode,
            )?;
            self.install_native(
                constructor,
                function_prototype,
                "fromCodePoint",
                1,
                NativeFunction::FromCodePoint,
            )?;
            self.install_native(
                constructor,
                function_prototype,
                "raw",
                1,
                NativeFunction::Raw,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "split",
                2,
                NativeFunction::Split,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "replace",
                2,
                NativeFunction::Replace,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "replaceAll",
                2,
                NativeFunction::ReplaceAll,
            )?;
            for &(name, length, method) in native::STRING_METHODS {
                self.install_native(
                    prototype,
                    function_prototype,
                    name,
                    length,
                    NativeFunction::StringMethod(method),
                )?;
            }
            for (name, length, method) in [
                ("toLocaleLowerCase", 0, NativeFunction::ToLocaleLowerCase),
                ("toLocaleUpperCase", 0, NativeFunction::ToLocaleUpperCase),
                ("localeCompare", 1, NativeFunction::LocaleCompare),
            ] {
                self.install_native(prototype, function_prototype, name, length, method)?;
            }
            for (alias, original) in [("trimLeft", "trimStart"), ("trimRight", "trimEnd")] {
                let function = self.heap.get(prototype, original)?;
                self.define_data(prototype, alias, function, true, false, true)?;
            }
            self.install_symbol_native(
                prototype,
                function_prototype,
                "iterator",
                0,
                NativeFunction::StringIterator,
            )?;
            for (name, method) in [
                ("match", native::PatternMethod::Match),
                ("matchAll", native::PatternMethod::MatchAll),
                ("search", native::PatternMethod::Search),
            ] {
                self.install_native(
                    prototype,
                    function_prototype,
                    name,
                    1,
                    NativeFunction::Pattern(method),
                )?;
            }
            self.install_native(
                object_prototype,
                function_prototype,
                "toString",
                0,
                NativeFunction::ObjectToString,
            )?;
            self.install_native(
                object_prototype,
                function_prototype,
                "valueOf",
                0,
                NativeFunction::ObjectValueOf,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "toString",
                0,
                NativeFunction::ArrayToString,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "concat",
                1,
                NativeFunction::ArrayConcat,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "join",
                1,
                NativeFunction::ArrayJoin,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "forEach",
                1,
                NativeFunction::ArrayForEach,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "includes",
                1,
                NativeFunction::ArrayIncludes,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "reduce",
                1,
                NativeFunction::ArrayReduce,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "push",
                1,
                NativeFunction::ArrayPush,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "indexOf",
                1,
                NativeFunction::ArrayIndexOf,
            )?;
            self.install_symbol_native(
                self.array_prototype,
                function_prototype,
                "iterator",
                0,
                NativeFunction::ArrayIterator,
            )?;
            Ok((constructor, prototype))
        })();
        match result {
            Ok(intrinsics) => {
                self.string_intrinsics = Some(intrinsics);
                Ok(intrinsics)
            }
            Err(error) => {
                // The only pre-existing owners modified by bootstrap are
                // these two empty prototypes. Roll back published edges too.
                for (owner, key) in [
                    (object_prototype, PropertyName::from("toString")),
                    (object_prototype, "valueOf".into()),
                    (object_prototype, "hasOwnProperty".into()),
                    (self.array_prototype, "toString".into()),
                    (self.array_prototype, "concat".into()),
                    (self.array_prototype, "join".into()),
                    (self.array_prototype, "forEach".into()),
                    (self.array_prototype, "includes".into()),
                    (self.array_prototype, "reduce".into()),
                    (
                        self.array_prototype,
                        JsSymbol::well_known("iterator").into(),
                    ),
                ] {
                    self.heap.delete(owner, key)?;
                }
                self.heap.unroot(root)?;
                Err(error)
            }
        }
    }

    /// Native functions inherit from %Function.prototype%, not from
    /// %String.prototype%. The String intrinsic cache deliberately stores
    /// the latter as its second member, so callers that install a callable
    /// must use this helper rather than indexing that cache directly.
    fn function_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        let (string_constructor, _) = self.string_intrinsics()?;
        self.heap
            .prototype(string_constructor)?
            .ok_or_else(|| RuntimeError::TypeError("Function prototype is unavailable".into()))
    }

    fn install_native(
        &mut self,
        owner: ObjectId,
        prototype: ObjectId,
        name: &str,
        length: u32,
        function: NativeFunction,
    ) -> Result<(), RuntimeError> {
        let id = self.with_roots(|heap| heap.alloc_native_function(function, name, prototype))?;
        self.stack.push(Value::Object(id));
        let result = (|| {
            self.define_data(id, "name", Value::String(name.into()), false, false, true)?;
            self.define_data(
                id,
                "length",
                Value::Number(f64::from(length)),
                false,
                false,
                true,
            )?;
            self.define_data(owner, name, Value::Object(id), true, false, true)
        })();
        self.stack.pop();
        result?;
        Ok(())
    }

    fn install_native_getter(
        &mut self,
        owner: ObjectId,
        prototype: ObjectId,
        name: &str,
        function: NativeFunction,
    ) -> Result<(), RuntimeError> {
        let getter =
            self.with_roots(|heap| heap.alloc_native_function(function, name, prototype))?;
        self.stack.push(Value::Object(getter));
        let result = (|| {
            self.define_data(
                getter,
                "name",
                Value::String(format!("get {name}").into()),
                false,
                false,
                true,
            )?;
            self.define_data(getter, "length", Value::Number(0.0), false, false, true)?;
            self.with_roots(|heap| {
                heap.define_own_property(
                    owner,
                    name,
                    PropertyDescriptor {
                        get: Some(Value::Object(getter)),
                        set: Some(Value::Undefined),
                        enumerable: Some(false),
                        configurable: Some(true),
                        ..Default::default()
                    },
                )
            })?;
            Ok(())
        })();
        self.stack.pop();
        result
    }

    fn property_is_enumerable_intrinsic(&mut self) -> Result<(), RuntimeError> {
        if self
            .heap
            .get_own_property_descriptor(self.object_prototype, "propertyIsEnumerable")?
            .is_some()
        {
            return Ok(());
        }
        let function_prototype = self.function_prototype()?;
        self.install_native(
            self.object_prototype,
            function_prototype,
            "propertyIsEnumerable",
            1,
            NativeFunction::ObjectMethod(native::ObjectMethod::PropertyIsEnumerable),
        )
    }

    fn has_own_property_intrinsic(&mut self) -> Result<(), RuntimeError> {
        if self
            .heap
            .get_own_property_descriptor(self.object_prototype, "hasOwnProperty")?
            .is_some()
        {
            return Ok(());
        }
        let function_prototype = self.function_prototype()?;
        self.install_native(
            self.object_prototype,
            function_prototype,
            "hasOwnProperty",
            1,
            NativeFunction::ObjectMethod(native::ObjectMethod::HasOwnProperty),
        )
    }

    fn call_native(
        &mut self,
        callee: Value,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let target = if construct {
            callee.clone()
        } else {
            Value::Undefined
        };
        self.call_with_target(callee, receiver, args, construct, target)
    }

    fn call_with_target(
        &mut self,
        callee: Value,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
        target: Value,
    ) -> Result<Value, RuntimeError> {
        if self.call_depth >= MAX_RECURSIVE_CALL_DEPTH {
            return Err(RuntimeError::RangeError(
                "maximum call depth exceeded".into(),
            ));
        }
        // Arrow functions inherit their enclosing `new.target`.  This is
        // observable when a derived-constructor arrow invokes `super()`:
        // the superclass must allocate with the original derived class.
        let arrow = if !construct {
            match callee.object_id() {
                Some(id) => self
                    .heap
                    .closure(id)?
                    .is_some_and(|(code, _, _, _, _)| code.arrow),
                None => false,
            }
        } else {
            false
        };
        let target = if arrow {
            self.new_target.clone()
        } else {
            target
        };
        self.charge_step()?;
        let base = self.stack.len();
        self.stack.extend([callee.clone(), receiver.clone()]);
        self.stack.extend(args.iter().cloned());
        self.stack.push(target.clone());
        let previous_target = std::mem::replace(&mut self.new_target, target);
        let regular_function = matches!(
            callee.object_id(),
            Some(id) if self
                .heap
                .closure(id)?
                .is_some_and(|(code, _, _, _, _)| !code.arrow)
        );
        let next_new_target_allowed = (arrow && self.new_target_allowed) || regular_function;
        let previous_new_target_allowed =
            std::mem::replace(&mut self.new_target_allowed, next_new_target_allowed);
        let previous_module = callee.object_id().and_then(|id| {
            self.module_closure_referrers
                .get(&id)
                .cloned()
                .map(|module| self.active_module_name.replace(module))
        });
        self.call_depth += 1;
        let result = self
            .dispatch_call(callee, receiver, args, construct)
            .and_then(|value| {
                self.check_string(&value)?;
                Ok(value)
            });
        self.new_target = previous_target;
        self.new_target_allowed = previous_new_target_allowed;
        if let Some(module) = previous_module {
            self.active_module_name = module;
        }
        self.call_depth -= 1;
        self.stack.truncate(base);
        result
    }

    fn dispatch_call(
        &mut self,
        mut callee: Value,
        mut receiver: Value,
        mut args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        // Bound wrappers have no execution contexts of their own. Walk them
        // with fuel rather than consuming Rust stack or the JS frame limit.
        let mut prefixes = Vec::new();
        while let Value::Object(id) = callee {
            let Some(bound) = self.heap.bound_function(id)?.cloned() else {
                break;
            };
            self.charge_step()?;
            if construct && !bound.constructible {
                return Err(RuntimeError::TypeError(
                    "bound target is not a constructor".into(),
                ));
            }
            if construct && self.new_target == callee {
                self.new_target = Value::Object(bound.target);
            }
            callee = Value::Object(bound.target);
            receiver = bound.this;
            prefixes.push(bound.args);
        }
        if !prefixes.is_empty() {
            // Concatenate once, starting at the innermost wrapper, to avoid
            // repeatedly copying the accumulated arguments of long chains.
            args = prefixes.into_iter().rev().flatten().chain(args).collect();
        }
        if let Value::Object(id) = callee {
            if self.heap.proxy(id)?.is_some() {
                return self.proxy_call(id, receiver, args, construct);
            }
        }
        if let Value::Object(id) = callee {
            if let Some((code, captures, lexical_this, home, class_base)) = self.heap.closure(id)? {
                let receiver = if code.arrow { lexical_this } else { receiver };
                return self.call_closure(builtins::ClosureCall {
                    code,
                    captures,
                    callee,
                    receiver,
                    args,
                    construct,
                    home,
                    class_base,
                });
            }
        }
        let function = if let Value::Object(id) = callee {
            self.heap.native_function(id)?
        } else {
            None
        };
        let Some(function) = function else {
            return Err(RuntimeError::TypeError("value is not callable".into()));
        };
        if construct
            && !matches!(
                function,
                NativeFunction::String
                    | NativeFunction::Array
                    | NativeFunction::ArrayBuffer
                    | NativeFunction::DataView
                    | NativeFunction::TypedArray(_)
                    | NativeFunction::Proxy
                    | NativeFunction::Map
                    | NativeFunction::Set
                    | NativeFunction::Object
                    | NativeFunction::RegExp
                    | NativeFunction::Collator
                    | NativeFunction::Locale
                    | NativeFunction::Error(_)
                    | NativeFunction::Promise
                    | NativeFunction::Function
                    | NativeFunction::PrimitiveConstructor(_)
            )
        {
            return Err(RuntimeError::TypeError("value is not a constructor".into()));
        }
        self.native_call(function, receiver, args, construct)
    }

    fn unbox_string(&self, value: &Value) -> Result<Value, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(string) = self.heap.boxed_string(*id)? {
                return Ok(Value::String(string.clone()));
            }
        }
        Ok(value.clone())
    }

    fn string_receiver(&mut self, value: &Value) -> Result<JsString, RuntimeError> {
        if matches!(value, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "String method requires a non-null receiver".into(),
            ));
        }
        self.coerce_string(value)
    }

    fn string_raw(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let template = Value::Object(self.coerce_object(native::argument(args, 0))?);
        self.stack.push(template.clone());
        let raw = self.get_property(&template, &"raw".into())?;
        self.stack.push(raw.clone());
        let raw = Value::Object(self.coerce_object(&raw)?);
        self.stack.push(raw.clone());
        let length = self.get_property(&raw, &"length".into())?;
        let count = self.coerce_length(&length)? as u64;
        let mut result = JsString::default();
        for index in 0..count {
            self.charge_step()?; // A huge array-like length with empty entries must still terminate.
            let literal = self.get_property(&raw, &index.to_string().into())?;
            native::append(
                &mut result,
                &self.coerce_string(&literal)?,
                self.config.max_string_bytes,
            )?;
            if index + 1 < count {
                if let Some(substitution) = args.get(index as usize + 1) {
                    native::append(
                        &mut result,
                        &self.coerce_string(substitution)?,
                        self.config.max_string_bytes,
                    )?;
                }
            }
        }
        Ok(Value::String(result))
    }

    fn string_split(&mut self, receiver: &Value, args: &[Value]) -> Result<Value, RuntimeError> {
        if matches!(receiver, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "String method requires a non-null receiver".into(),
            ));
        }
        let separator = native::argument(args, 0);
        if !matches!(separator, Value::Null | Value::Undefined) {
            let method = self.get_method(separator, &JsSymbol::well_known("split").into())?;
            if method != Value::Undefined {
                return self.call_native(
                    method,
                    separator.clone(),
                    vec![receiver.clone(), native::argument(args, 1).clone()],
                    false,
                );
            }
        }
        let string = self.string_receiver(receiver)?;
        let separator = native::argument(args, 0);
        let limit = native::argument(args, 1);
        let limit = if matches!(limit, Value::Undefined) {
            u32::MAX
        } else {
            native::uint32(&Value::Number(self.coerce_number(limit)?))?
        };
        // Even a zero limit converts the separator first (§22.1.3.23).
        let search = self.coerce_string(separator)?;
        let prototype = self.array_prototype;
        let array = self.with_roots(|heap| heap.alloc_array(0, Some(prototype)))?;
        self.stack.push(Value::Object(array)); // Root the growing result through every store.
        let mut count = 0u32;
        let units = string.as_code_units();
        let mut start = 0;
        while count < limit {
            self.charge_step()?;
            let end = if matches!(separator, Value::Undefined) {
                units.len()
            } else if search.is_empty() {
                if start == units.len() {
                    break;
                }
                start + 1
            } else if search.len() <= units.len() {
                (start..=units.len() - search.len())
                    .find(|&index| units[index..].starts_with(search.as_code_units()))
                    .unwrap_or(units.len())
            } else {
                units.len()
            };
            let value = Value::String(JsString::from_code_units(units[start..end].to_vec()));
            self.with_roots(|heap| heap.set(array, count.to_string(), value))?;
            count += 1;
            if end == units.len() {
                break;
            }
            start = end + search.len();
        }
        Ok(Value::Object(array))
    }

    fn string_replace(
        &mut self,
        receiver: &Value,
        args: &[Value],
        all: bool,
    ) -> Result<Value, RuntimeError> {
        if matches!(receiver, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "String method requires a non-null receiver".into(),
            ));
        }
        let search = native::argument(args, 0);
        if !matches!(search, Value::Null | Value::Undefined) {
            if all {
                self.require_global_pattern(search)?;
            }
            let method = self.get_method(search, &JsSymbol::well_known("replace").into())?;
            if method != Value::Undefined {
                return self.call_native(
                    method,
                    search.clone(),
                    vec![receiver.clone(), native::argument(args, 1).clone()],
                    false,
                );
            }
        }
        let string = self.string_receiver(receiver)?;
        let search = self.coerce_string(native::argument(args, 0))?;
        let replace = native::argument(args, 1);
        let callable = self.is_callable(replace)?;
        let template = if callable {
            JsString::default()
        } else {
            self.coerce_string(replace)?
        };
        let mut result = JsString::default();
        let mut end = 0;
        let mut next = 0;
        if search.len() <= string.len() {
            while let Some(position) = (next..=string.len() - search.len())
                .find(|&index| string.as_code_units()[index..].starts_with(search.as_code_units()))
            {
                self.charge_step()?;
                let replacement = if callable {
                    let value = self.call_native(
                        replace.clone(),
                        Value::Undefined,
                        vec![
                            Value::String(search.clone()),
                            Value::Number(position as f64),
                            Value::String(string.clone()),
                        ],
                        false,
                    )?;
                    self.coerce_string(&value)?
                } else {
                    native::substitution(
                        &string,
                        &search,
                        position,
                        &template,
                        self.config.max_string_bytes,
                    )?
                };
                native::append(
                    &mut result,
                    &JsString::from_code_units(string.as_code_units()[end..position].to_vec()),
                    self.config.max_string_bytes,
                )?;
                native::append(&mut result, &replacement, self.config.max_string_bytes)?;
                end = position + search.len();
                if !all {
                    break;
                }
                next = position + search.len().max(1);
            }
        }
        native::append(
            &mut result,
            &JsString::from_code_units(string.as_code_units()[end..].to_vec()),
            self.config.max_string_bytes,
        )?;
        Ok(Value::String(result))
    }

    fn binary(
        &mut self,
        operation: impl FnOnce(&mut Self, Value, Value) -> Result<Value, RuntimeError>,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len() - 2;
        let value = operation(self, self.stack[base].clone(), self.stack[base + 1].clone())?;
        self.stack.truncate(base);
        self.stack.push(value);
        Ok(())
    }

    fn numeric(&mut self, operation: fn(f64, f64) -> f64) -> Result<(), RuntimeError> {
        self.binary(|vm, a, b| {
            Ok(Value::Number(operation(
                vm.coerce_number(&a)?,
                vm.coerce_number(&b)?,
            )))
        })
    }

    fn exponentiate(&mut self) -> Result<(), RuntimeError> {
        self.binary(|vm, left, right| {
            let left = vm.coerce_numeric(&left)?;
            let right = vm.coerce_numeric(&right)?;
            match (left, right) {
                (primitive::Numeric::Number(left), primitive::Numeric::Number(right)) => {
                    // libm's powf returns 1 for ±1 raised to ±∞, whereas
                    // Number::exponentiate explicitly specifies NaN for
                    // that pair.
                    let value = if right.is_infinite() && left.abs() == 1.0 {
                        f64::NAN
                    } else {
                        left.powf(right)
                    };
                    Ok(Value::Number(value))
                }
                (primitive::Numeric::BigInt(left), primitive::Numeric::BigInt(right)) => {
                    Ok(Value::BigInt(bigint_exponentiate(left, right)?))
                }
                _ => Err(RuntimeError::TypeError(
                    "cannot mix BigInt and other types in an exponentiation operation".into(),
                )),
            }
        })
    }

    fn negate(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        Ok(match self.coerce_numeric(value)? {
            primitive::Numeric::Number(value) => Value::Number(-value),
            primitive::Numeric::BigInt(value) => Value::BigInt(-value),
        })
    }

    fn bit_not(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        Ok(match self.coerce_numeric(value)? {
            primitive::Numeric::Number(value) => {
                Value::Number((!primitive::to_int32(value)) as f64)
            }
            primitive::Numeric::BigInt(value) => Value::BigInt(!value),
        })
    }

    fn bitwise(&mut self, operation: Opcode) -> Result<(), RuntimeError> {
        self.binary(|vm, left, right| {
            let left = vm.coerce_numeric(&left)?;
            let right = vm.coerce_numeric(&right)?;
            match (left, right) {
                (primitive::Numeric::Number(left), primitive::Numeric::Number(right)) => {
                    let left = primitive::to_int32(left);
                    let right = primitive::to_int32(right);
                    let value = match operation {
                        Opcode::BitAnd => left & right,
                        Opcode::BitXor => left ^ right,
                        Opcode::BitOr => left | right,
                        _ => unreachable!("bitwise caller selects a bitwise opcode"),
                    };
                    Ok(Value::Number(value as f64))
                }
                (primitive::Numeric::BigInt(left), primitive::Numeric::BigInt(right)) => {
                    let value = match operation {
                        Opcode::BitAnd => left & right,
                        Opcode::BitXor => left ^ right,
                        Opcode::BitOr => left | right,
                        _ => unreachable!("bitwise caller selects a bitwise opcode"),
                    };
                    Ok(Value::BigInt(value))
                }
                _ => Err(RuntimeError::TypeError(
                    "cannot mix BigInt and other types in a bitwise operation".into(),
                )),
            }
        })
    }

    fn shift(&mut self, operation: Opcode) -> Result<(), RuntimeError> {
        self.binary(|vm, left, right| {
            let left = vm.coerce_numeric(&left)?;
            let right = vm.coerce_numeric(&right)?;
            match (left, right) {
                (primitive::Numeric::Number(left), primitive::Numeric::Number(right)) => {
                    let left = primitive::to_int32(left);
                    let right = primitive::to_uint32(right) & 0x1f;
                    let value = match operation {
                        Opcode::ShiftLeft => left.wrapping_shl(right) as f64,
                        Opcode::ShiftRight => (left >> right) as f64,
                        Opcode::UnsignedShiftRight => ((left as u32) >> right) as f64,
                        _ => unreachable!("shift caller selects a shift opcode"),
                    };
                    Ok(Value::Number(value))
                }
                (primitive::Numeric::BigInt(left), primitive::Numeric::BigInt(right)) => {
                    if operation == Opcode::UnsignedShiftRight {
                        return Err(RuntimeError::TypeError(
                            "BigInt does not support unsigned right shift".into(),
                        ));
                    }
                    Ok(Value::BigInt(bigint_shift(
                        left,
                        right,
                        operation == Opcode::ShiftLeft,
                    )?))
                }
                _ => Err(RuntimeError::TypeError(
                    "cannot mix BigInt and other types in a shift operation".into(),
                )),
            }
        })
    }

    fn relational(&mut self, accept: fn(Ordering) -> bool) -> Result<(), RuntimeError> {
        self.binary(|vm, a, b| {
            let a = vm.coerce_primitive(&a, "number")?;
            let b = vm.coerce_primitive(&b, "number")?;
            Ok(Value::Bool(primitive::compare(&a, &b)?.is_some_and(accept)))
        })
    }

    fn loose_equal(&mut self, left: Value, right: Value) -> Result<bool, RuntimeError> {
        if std::mem::discriminant(&left) == std::mem::discriminant(&right) {
            return Ok(left == right);
        }
        if matches!(right, Value::Null | Value::Undefined)
            && matches!(&left, Value::Object(object) if self.heap.is_html_dda(*object)?)
        {
            return Ok(true);
        }
        if matches!(left, Value::Null | Value::Undefined)
            && matches!(&right, Value::Object(object) if self.heap.is_html_dda(*object)?)
        {
            return Ok(true);
        }
        if matches!(
            (&left, &right),
            (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null)
        ) {
            return Ok(true);
        }
        match (left, right) {
            (Value::Number(left), Value::String(right)) => {
                Ok(left == primitive::number(&Value::String(right))?)
            }
            (Value::String(left), Value::Number(right)) => {
                Ok(primitive::number(&Value::String(left))? == right)
            }
            (Value::Bool(left), right) => {
                self.loose_equal(Value::Number(if left { 1.0 } else { 0.0 }), right)
            }
            (left, Value::Bool(right)) => {
                self.loose_equal(left, Value::Number(if right { 1.0 } else { 0.0 }))
            }
            (
                Value::Object(left),
                right @ (Value::Number(_) | Value::String(_) | Value::Symbol(_)),
            ) => {
                let left = self.coerce_primitive(&Value::Object(left), "default")?;
                self.loose_equal(left, right)
            }
            (
                left @ (Value::Number(_) | Value::String(_) | Value::Symbol(_)),
                Value::Object(right),
            ) => {
                let right = self.coerce_primitive(&Value::Object(right), "default")?;
                self.loose_equal(left, right)
            }
            _ => Ok(false),
        }
    }

    fn property_in(&mut self, key: &Value, object: &Value) -> Result<bool, RuntimeError> {
        let Value::Object(object) = object else {
            return Err(RuntimeError::TypeError(
                "right operand of in must be an object".into(),
            ));
        };
        let key = self.coerce_property_key(key)?;
        if self.heap.proxy(*object)?.is_some() {
            return self.proxy_has(*object, &key);
        }
        self.has_property(*object, &key)
    }

    /// Object Environment Record HasBinding. `with` lookup first observes the
    /// target object's property chain, then gives an object-valued
    /// `Symbol.unscopables` a chance to hide that name from lexical lookup.
    fn with_has_binding(&mut self, object: &Value, name: &str) -> Result<bool, RuntimeError> {
        let key = Value::String(name.into());
        if !self.property_in(&key, object)? {
            return Ok(false);
        }
        let unscopables = self.get_property(object, &JsSymbol::well_known("unscopables").into())?;
        if !matches!(unscopables, Value::Object(_)) {
            return Ok(true);
        }
        // The getter for an unscopables entry can allocate. Keep its receiver
        // live on the VM stack rather than relying on an unrooted Rust Value.
        self.stack.push(unscopables);
        let result = (|| {
            let unscopables = self.stack.last().expect("unscopables is rooted").clone();
            let blocked = self.get_property(&unscopables, &name.into())?;
            Ok(!self.to_boolean(&blocked)?)
        })();
        self.stack.pop();
        result
    }

    fn with_get(
        &mut self,
        name: &str,
        fallback: Option<Option<Value>>,
    ) -> Result<Value, RuntimeError> {
        for object in self.with_objects.clone().into_iter().rev() {
            if self.with_has_binding(&object, name)? {
                return self.get_property(&object, &name.into());
            }
        }
        match fallback {
            Some(Some(value)) => Ok(value),
            Some(None) => Err(RuntimeError::ReferenceError(name.into())),
            None => self
                .lookup_global_name(name)?
                .ok_or_else(|| RuntimeError::ReferenceError(name.into())),
        }
    }

    fn with_set(&mut self, name: &str, value: Value) -> Result<(), RuntimeError> {
        for object in self.with_objects.clone().into_iter().rev() {
            if self.with_has_binding(&object, name)? {
                return self.set_property(&object, &name.into(), &value);
            }
        }
        Err(RuntimeError::ReferenceError(name.into()))
    }

    fn add(&mut self, left: Value, right: Value) -> Result<Value, RuntimeError> {
        let left = self.coerce_primitive(&left, "default")?;
        let right = self.coerce_primitive(&right, "default")?;
        if matches!(left, Value::String(_)) || matches!(right, Value::String(_)) {
            let mut a = primitive::string(&left)?;
            let b = primitive::string(&right)?;
            if a.byte_len()
                .checked_add(b.byte_len())
                .is_none_or(|len| len > self.config.max_string_bytes)
            {
                return Err(RuntimeError::StringLimit {
                    limit: self.config.max_string_bytes,
                });
            }
            a.push_str(&b);
            Ok(Value::String(a))
        } else {
            Ok(Value::Number(
                primitive::number(&left)? + primitive::number(&right)?,
            ))
        }
    }
}

/// BigInt shifts use the full signed right operand, unlike Number shifts
/// whose count is reduced modulo 32. A negative count reverses direction.
fn bigint_shift(value: BigInt, count: BigInt, left: bool) -> Result<BigInt, RuntimeError> {
    let reverse = count.sign() == Sign::Minus;
    let shift_left = left != reverse;
    let magnitude = count.magnitude().to_usize();
    let Some(magnitude) = magnitude else {
        if !shift_left {
            return Ok(if value.sign() == Sign::Minus {
                BigInt::from(-1)
            } else {
                BigInt::from(0)
            });
        }
        return Err(RuntimeError::RangeError(
            "BigInt shift count exceeds implementation capacity".into(),
        ));
    };
    Ok(if shift_left {
        value << magnitude
    } else {
        value >> magnitude
    })
}

/// BigInt::exponentiate permits only non-negative BigInt exponents.  The
/// standard result is exact; this interpreter additionally bounds the host
/// exponent representation before allocating the result.
fn bigint_exponentiate(base: BigInt, exponent: BigInt) -> Result<BigInt, RuntimeError> {
    if exponent.sign() == Sign::Minus {
        return Err(RuntimeError::RangeError(
            "BigInt exponent must be non-negative".into(),
        ));
    }
    if base.is_zero() {
        return Ok(if exponent.is_zero() {
            BigInt::one()
        } else {
            BigInt::zero()
        });
    }
    if base == BigInt::one() {
        return Ok(base);
    }
    let Some(exponent) = exponent.to_u32() else {
        return Err(RuntimeError::RangeError(
            "BigInt exponent exceeds implementation capacity".into(),
        ));
    };
    Ok(base.pow(exponent))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpreter_converts_catchable_errors_and_rejects_a_top_level_yield() {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        vm.remaining_instructions = vm.config.instruction_budget;
        let test262_error = vm.error_value(RuntimeError::Test262("failure".into()));
        assert!(
            matches!(test262_error, Ok(Value::Object(_))),
            "{test262_error:?}"
        );
        assert_eq!(
            vm.error_value(RuntimeError::InstructionLimit),
            Err(RuntimeError::InstructionLimit)
        );
        let mut code = Bytecode::empty();
        code.constants.push(Value::Undefined);
        code.code
            .extend([Opcode::Constant as u8, 0, 0, 0, 0, Opcode::Yield as u8]);
        vm.remaining_instructions = vm.config.instruction_budget;
        let yield_error = vm.execute(&code);
        assert!(
            matches!(yield_error, Err(RuntimeError::TypeError(ref message)) if message == "yield is not supported in this execution context"),
            "{yield_error:?}"
        );
        code.generator = true;
        vm.remaining_instructions = vm.config.instruction_budget;
        assert!(
            matches!(vm.execute(&code), Err(RuntimeError::TypeError(message)) if message == "yield requires a generator function")
        );
    }

    #[test]
    fn with_lookup_uses_the_object_then_reports_an_unbound_name() {
        let mut vm = Vm::default();
        let object = vm.heap.alloc_object(None).unwrap();
        vm.heap.set(object, "value", Value::Number(7.0)).unwrap();
        vm.with_objects.push(Value::Object(object));
        assert_eq!(vm.with_get("value", None), Ok(Value::Number(7.0)));
        assert_eq!(
            vm.with_get("missing", None),
            Err(RuntimeError::ReferenceError("missing".into()))
        );
    }

    #[test]
    fn class_definition_opcodes_assign_home_objects_to_closures() {
        let mut vm = Vm::default();
        let target = vm.heap.alloc_object(None).unwrap();
        let mut function_code = Bytecode::empty();
        function_code.code.push(Opcode::Halt as u8);
        let function = vm
            .heap
            .alloc_closure(
                std::rc::Rc::new(function_code),
                Vec::new(),
                Value::Undefined,
                vm.object_prototype,
            )
            .unwrap();
        let mut code = Bytecode::empty();
        code.code
            .extend([Opcode::DefineMethod as u8, Opcode::Halt as u8]);

        vm.stack = vec![
            Value::Object(target),
            Value::String("method".into()),
            Value::Object(function),
        ];
        vm.remaining_instructions = vm.config.instruction_budget;
        assert!(matches!(
            vm.interpret(&code, &mut Vec::new(), 0, None, None, None),
            Ok(InterpreterExit::Return(Value::Undefined))
        ));
        vm.stack = vec![
            Value::Object(target),
            Value::String("empty".into()),
            Value::Undefined,
        ];
        vm.remaining_instructions = vm.config.instruction_budget;
        assert!(matches!(
            vm.interpret(&code, &mut Vec::new(), 0, None, None, None),
            Ok(InterpreterExit::Return(Value::Undefined))
        ));

        code.code[0] = Opcode::CallClassStaticBlock as u8;
        vm.stack = vec![Value::Object(target), Value::Object(function)];
        vm.remaining_instructions = vm.config.instruction_budget;
        assert!(matches!(
            vm.interpret(&code, &mut Vec::new(), 0, None, None, None),
            Ok(InterpreterExit::Return(Value::Undefined))
        ));
        vm.stack = vec![Value::Object(target), Value::Undefined];
        vm.remaining_instructions = vm.config.instruction_budget;
        assert!(matches!(
            vm.interpret(&code, &mut Vec::new(), 0, None, None, None),
            Err(RuntimeError::TypeError(_))
        ));

        code.code[0] = Opcode::DefineClassStaticField as u8;
        vm.stack = vec![
            Value::Undefined,
            Value::Object(target),
            Value::String("field".into()),
            Value::Object(function),
        ];
        vm.remaining_instructions = vm.config.instruction_budget;
        assert!(matches!(
            vm.interpret(&code, &mut Vec::new(), 0, None, None, None),
            Ok(InterpreterExit::Return(Value::Undefined))
        ));
        vm.stack = vec![
            Value::Undefined,
            Value::Object(target),
            Value::String("emptyField".into()),
            Value::Undefined,
        ];
        vm.remaining_instructions = vm.config.instruction_budget;
        assert!(matches!(
            vm.interpret(&code, &mut Vec::new(), 0, None, None, None),
            Err(RuntimeError::TypeError(_))
        ));
    }

    #[test]
    fn super_assignment_reports_a_non_extensible_receiver() {
        let mut vm = Vm::default();
        let base = vm.heap.alloc_object(None).unwrap();
        let home = vm.heap.alloc_object(Some(base)).unwrap();
        let receiver = vm.heap.alloc_object(None).unwrap();
        vm.heap.prevent_extensions(receiver).unwrap();
        vm.home_object = Some(home);
        vm.this = Value::Object(receiver);
        vm.strict = true;
        assert_eq!(
            vm.super_set(&"value".into(), &Value::Number(1.0)),
            Err(RuntimeError::TypeError(
                "super property cannot be assigned".into()
            ))
        );

        let base = vm.heap.alloc_object(None).unwrap();
        let home = vm.heap.alloc_object(Some(base)).unwrap();
        vm.heap.root(home).unwrap();
        let stale_receiver = vm.heap.alloc_object(None).unwrap();
        vm.heap.collect_major();
        vm.home_object = Some(home);
        vm.this = Value::Object(stale_receiver);
        assert_eq!(
            vm.super_set(&"value".into(), &Value::Number(1.0)),
            Err(RuntimeError::Heap(HeapError::InvalidObject(stale_receiver)))
        );
    }

    #[test]
    fn super_and_eval_context_errors_describe_missing_internal_context() {
        let mut vm = Vm::default();
        assert_eq!(
            vm.super_base(),
            Err(RuntimeError::TypeError(
                "super is not available in this function".into()
            ))
        );
        let home = vm.heap.alloc_object(None).unwrap();
        vm.home_object = Some(home);
        assert_eq!(
            vm.super_base(),
            Err(RuntimeError::TypeError("superclass is null".into()))
        );
        assert_eq!(
            vm.super_call(Vec::new()),
            Err(RuntimeError::TypeError(
                "super() is not available in this function".into()
            ))
        );
        let closure = vm
            .heap
            .alloc_closure(
                std::rc::Rc::new(Bytecode::empty()),
                Vec::new(),
                Value::Undefined,
                vm.object_prototype,
            )
            .unwrap();
        vm.class_constructor = Some(closure);
        assert_eq!(
            vm.super_call(Vec::new()),
            Err(RuntimeError::TypeError(
                "super() requires a derived constructor".into()
            ))
        );
        vm.heap.set_class_base(closure, Value::Null).unwrap();
        assert_eq!(
            vm.super_call(Vec::new()),
            Err(RuntimeError::TypeError("super constructor is null".into()))
        );
        vm.binding_metadata.push(Binding {
            name: "captured".into(),
            mutable: true,
            strict_immutable: false,
            lexical: true,
            catch_parameter: false,
        });
        vm.cells.insert(0, home);
        assert_eq!(
            vm.eval_visible_bindings()
                .into_iter()
                .map(|(name, _, slot)| (name, slot))
                .collect::<Vec<_>>(),
            vec![("captured".into(), 0)]
        );
    }

    #[test]
    fn compiler_owned_bytecode_invariants_fail_loudly() {
        let no_handler = Bytecode::empty();
        let mut vm = Vm::default();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.resolve_completion(
                &no_handler,
                &mut Vec::new(),
                &mut Vec::new(),
                Completion::Yield(Value::Undefined)
            )))
            .is_err()
        );
        let mut vm = Vm::default();
        vm.stack.push(Value::Number(0.0));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.set_class_heritage()))
                .is_err()
        );
        let mut vm = Vm::default();
        vm.stack.push(Value::Number(0.0));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.set_class_home())).is_err()
        );
    }
}
