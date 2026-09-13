// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded operand-stack interpreter for compiler-owned bytecode.
//! Allocating instructions root all VM-held objects around heap
//! safepoints. The collector itself additionally protects store inputs.

use crate::bytecode::{Binding, ModuleExport, ModuleImportName};
use crate::heap::{
    ArrayIteratorKind, GeneratorHandlerFrame, GeneratorHandlerState, GeneratorPendingCompletion,
    GeneratorState, PrivateElement,
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
mod execution;
mod functions;
mod interpreter;
mod intl;
mod json;
mod modules;
mod operations;
mod properties;
mod regexp;
mod test262;
mod test262_agents;
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

/// A Test262 realm owns a complete VM. Foreign objects are represented by a
/// rooted membrane object in the requesting VM, preserving their identity and
/// internal slots without ever leaking a child-heap `ObjectId` into a parent
/// value.
struct Test262Realm {
    vm: Box<Vm>,
    wrappers: HashMap<ObjectId, ObjectId>,
    /// Parent-heap values temporarily represented by an ordinary object in
    /// this child heap. The reverse map preserves identity when child code
    /// returns or retains an argument supplied by the parent.
    imported_sources: HashMap<ObjectId, ObjectId>,
    imported_values: HashMap<ObjectId, Test262ImportedValue>,
}

/// The two roots keep an opaque membrane transport value alive in each heap.
/// Such a value preserves identity across a call boundary. Property forwarding
/// remains a separate membrane operation; no parent-heap handle is exposed to
/// child heap storage.
struct Test262ImportedValue {
    value: Value,
    _source_root: RootId,
    _target_root: RootId,
}

/// A parent-heap object that stands for an object retained in a Test262 child
/// realm. The two roots keep both endpoints alive while the membrane identity
/// is observable; the child object is never stored in the parent heap.
struct Test262ForeignValue {
    realm: ObjectId,
    target: ObjectId,
    callable: bool,
    constructible: bool,
    // A result created in one Test262 realm can use a constructor from a
    // second realm as its `newTarget`.  The child heap cannot retain that
    // second heap's ObjectId, so retain the observable [[Prototype]] at the
    // membrane boundary instead.
    prototype_override: Option<ObjectId>,
    _wrapper_root: RootId,
    _target_root: RootId,
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
    /// The host-created [[ImportMeta]] value for each source-text module.
    /// The root gives module records stable identity across nested closure
    /// calls and later graph evaluations.
    module_import_meta: HashMap<String, ObjectId>,
    module_import_meta_roots: HashMap<String, RootId>,
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
    /// Test262-only host scheduler state. Ordinary realms never install or
    /// expose it; agent VMs receive the same Arc while retaining their own
    /// heap and realm records.
    test262_agent_host: Option<std::sync::Arc<test262_agents::Test262AgentHost>>,
    test262_agent_control: Option<std::sync::Arc<test262_agents::Test262AgentControl>>,
    test262_async_waits: std::sync::Arc<test262_agents::Test262AsyncWaits>,
    test262_realms: HashMap<ObjectId, Test262Realm>,
    test262_foreign_values: HashMap<ObjectId, Test262ForeignValue>,
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
            module_import_meta: HashMap::new(),
            module_import_meta_roots: HashMap::new(),
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
            test262_agent_host: None,
            test262_agent_control: None,
            test262_async_waits: std::sync::Arc::new(test262_agents::Test262AsyncWaits::new()),
            test262_realms: HashMap::new(),
            test262_foreign_values: HashMap::new(),
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
}

impl Vm {
    fn charge_step(&mut self) -> Result<(), RuntimeError> {
        if self.remaining_instructions == 0 {
            return Err(RuntimeError::InstructionLimit);
        }
        self.remaining_instructions -= 1;
        Ok(())
    }
}

impl Vm {
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
                object_prototype,
                function_prototype,
                "isPrototypeOf",
                1,
                NativeFunction::ObjectIsPrototypeOf,
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
                "filter",
                1,
                NativeFunction::ArrayFilter,
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
            self.install_native(
                self.array_prototype,
                function_prototype,
                "slice",
                2,
                NativeFunction::ArraySlice,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "splice",
                2,
                NativeFunction::ArraySplice,
            )?;
            self.install_native(
                self.array_prototype,
                function_prototype,
                "sort",
                1,
                NativeFunction::ArraySort,
            )?;
            self.install_symbol_native(
                self.array_prototype,
                function_prototype,
                "iterator",
                0,
                NativeFunction::ArrayIterator(ArrayIteratorKind::Values),
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
                    (object_prototype, "isPrototypeOf".into()),
                    (object_prototype, "hasOwnProperty".into()),
                    (self.array_prototype, "toString".into()),
                    (self.array_prototype, "concat".into()),
                    (self.array_prototype, "join".into()),
                    (self.array_prototype, "forEach".into()),
                    (self.array_prototype, "filter".into()),
                    (self.array_prototype, "includes".into()),
                    (self.array_prototype, "reduce".into()),
                    (self.array_prototype, "sort".into()),
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
            self.define_data(
                id,
                "length",
                Value::Number(f64::from(length)),
                false,
                false,
                true,
            )?;
            self.define_data(id, "name", Value::String(name.into()), false, false, true)?;
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
            self.define_data(getter, "length", Value::Number(0.0), false, false, true)?;
            self.define_data(
                getter,
                "name",
                Value::String(format!("get {name}").into()),
                false,
                false,
                true,
            )?;
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
            if self.test262_foreign_reference(id).is_some() {
                return self.test262_foreign_call(id, receiver, args, construct);
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
                    | NativeFunction::SharedArrayBuffer
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
