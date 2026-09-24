// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded operand-stack interpreter for compiler-owned bytecode.
//! Allocating instructions root all VM-held objects around heap
//! safepoints. The collector itself additionally protects store inputs.

use crate::bytecode::{Binding, ModuleExport, ModuleImportName};
use crate::heap::{GeneratorState, PrivateElement};
use crate::native::{self, NativeFunction};
use crate::primitive;
use crate::{
    Bytecode, Heap, HeapConfig, HeapError, ImportPhase, JsString, JsSymbol, ModuleType, ObjectId,
    Opcode, PropertyDescriptor, PropertyName, RootId, Value,
};
use num_bigint::{BigInt, Sign};
use num_traits::{One, ToPrimitive, Zero};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::rc::Rc;
mod builtins;
mod completion;
mod debugger;
mod errors;
mod execution;
mod functions;
mod host_objects;
mod interpreter;
mod intl;
mod intrinsics;
mod json;
mod lifecycle;
mod modules;
mod operations;
mod properties;
mod regexp;
mod shadow_realm;
mod temporal;
mod test262;
mod test262_agents;
use completion::{
    Completion, CompletionAction, HandlerFrame, HandlerState, InterpreterExit,
    MAX_RECURSIVE_CALL_DEPTH,
};
use debugger::DebuggerContinuation;
pub use debugger::VmDebuggerExecutionState;
pub use host_objects::{
    HostObjectFactory, HostObjectFamily, HostObjectKey, HostObjectMethod, HostObjectPairMethod,
};
use host_objects::{
    HostObjectFactoryRegistration, HostObjectFamilyState, HostObjectMethodRegistration,
    HostObjectPairMethodRegistration,
};
use std::fmt;

/// A private interpreter suspension boundary. `Offset` is also used by
/// generator/module setup; only the debugger requests one executed root
/// instruction before the next suspension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterpreterSuspensionPoint {
    Offset(usize),
    AfterRootInstruction,
}

/// An opaque object created by [`Vm::install_host_object`]. It can only be
/// populated through the VM that created it, preventing an embedder from
/// accidentally attaching a host method to an object from another realm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostObject(ObjectId);

/// A primitive value permitted to cross the synchronous host callback
/// boundary. JavaScript object identities intentionally cannot cross this
/// boundary: retaining one in Rust would evade the VM collector.
#[derive(Debug, Clone, PartialEq)]
pub enum HostValue {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(JsString),
}

impl TryFrom<&Value> for HostValue {
    type Error = HostFunctionError;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        match value {
            Value::Undefined => Ok(Self::Undefined),
            Value::Null => Ok(Self::Null),
            Value::Bool(value) => Ok(Self::Bool(*value)),
            Value::Number(value) => Ok(Self::Number(*value)),
            Value::String(value) => Ok(Self::String(value.clone())),
            Value::BigInt(_) | Value::Symbol(_) | Value::Object(_) => Err(HostFunctionError::new(
                "host functions accept primitive values only",
            )),
        }
    }
}

impl From<HostValue> for Value {
    fn from(value: HostValue) -> Self {
        match value {
            HostValue::Undefined => Self::Undefined,
            HostValue::Null => Self::Null,
            HostValue::Bool(value) => Self::Bool(value),
            HostValue::Number(value) => Self::Number(value),
            HostValue::String(value) => Self::String(value),
        }
    }
}

/// A synchronous capability supplied by the embedding host.
///
/// The callback receives primitive [`HostValue`] arguments but no heap, VM, or
/// JavaScript object identity. Constructors are never dispatched to host
/// functions.
pub trait HostFunction: 'static {
    fn call(&mut self, args: &[HostValue]) -> Result<HostValue, HostFunctionError>;
}

impl<F> HostFunction for F
where
    F: for<'args> FnMut(&'args [HostValue]) -> Result<HostValue, HostFunctionError> + 'static,
{
    fn call(&mut self, args: &[HostValue]) -> Result<HostValue, HostFunctionError> {
        self(args)
    }
}

/// A host callback failure that becomes a JavaScript `TypeError` at the call
/// boundary. The message is host-controlled; page data must not be reflected
/// into it without the embedding host's own policy check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFunctionError(String);

impl HostFunctionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for HostFunctionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for HostFunctionError {}

fn host_property_name_is_valid(name: &str) -> bool {
    let mut chars = name.bytes();
    matches!(chars.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$'))
        && chars.all(|byte| matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'$'))
}

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
            | HeapError::ImmutableArrayBuffer
            | HeapError::InvalidWeakTarget
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
    /// The module's *deferred* namespace object (import-defer proposal), kept
    /// apart from `namespace`: the two are distinct objects.
    deferred_namespace: Option<ObjectId>,
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
    DeferredNamespace { module: String },
    Source { module: String },
}

/// A pending `import.defer()` promise. It resolves with the deferred
/// namespace once every asynchronous dependency the import had to evaluate
/// (its `pending` set) has finished, and rejects if any of them fails.
struct DeferredImportWaiter {
    promise: ObjectId,
    namespace: ObjectId,
    pending: HashSet<String>,
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

/// A PromiseCapability Record: a promise together with the functions that
/// resolve and reject it. For a promise made by a user constructor these are
/// whatever that constructor handed its executor.
#[derive(Clone)]
struct PromiseCapability {
    promise: Value,
    resolve: Value,
    reject: Value,
}

/// Where a reaction job delivers its result.
#[derive(Clone)]
enum ReactionTarget {
    /// A promise the VM created itself for `%Promise%`-constructed results
    /// (`then` with the default species): its resolving functions are never
    /// observable, so the job resolves or rejects it directly.
    Native(ObjectId),
    /// The capability of a promise built by another constructor (a subclass
    /// or a custom species): its resolve/reject functions are called.
    Capability(PromiseCapability),
}

struct PromiseThenReaction {
    target: ReactionTarget,
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
    /// The operand of a `return(value)` request to a generator suspended at a
    /// `yield` is being awaited (AsyncGeneratorUnwrapYieldResumption).
    AwaitReturn,
    /// The delegate of a `yield*` has no `return` method, so the operand is
    /// awaited a second time before it is returned.
    AwaitReturnNoMethod,
}

struct PromiseRecord {
    status: PromiseStatus,
    reactions: Vec<PromiseReaction>,
}

/// Whether a disposable resource was added by a `using` declaration/
/// `DisposableStack` (synchronous) or an `await using` declaration/
/// `AsyncDisposableStack` (asynchronous). Mirrors the spec's `sync-dispose`/
/// `async-dispose` hint on a DisposableResource Record.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DisposeHint {
    Sync,
    Async,
}

/// A single entry of a DisposeCapability Record's `[[DisposableResourceStack]]`.
///
/// `receiver` is the value `method` is called on; `argument`, when present,
/// is passed as the sole call argument instead of being used as the
/// receiver. This lets `DisposableStack.prototype.adopt`'s synthetic
/// `() => onDispose(value)` closure (spec `CreateDisposableResource`'s
/// captured Abstract Closure) collapse into an ordinary call description
/// rather than needing its own heap-allocated closure object: `defer`/plain
/// resources call `method` on `receiver` with no arguments, while `adopt`
/// calls `method` on `undefined` with `argument` as the one parameter.
pub(super) struct DisposableResource {
    pub(super) receiver: Value,
    pub(super) argument: Option<Value>,
    pub(super) method: Option<Value>,
    // Not yet read: `dispose_resources_sync` treats every resource
    // uniformly (see its own doc comment for the one case, a method-less
    // `async-dispose` resource, where the real algorithm's behavior
    // depends on this field and this implementation's does not).
    #[allow(dead_code)]
    pub(super) hint: DisposeHint,
    /// An `async-dispose` resource whose method is the sync `@@dispose`
    /// fallback: `Dispose` still awaits, but the method's result is
    /// discarded rather than awaited (its promise may never settle).
    pub(super) sync_fallback: bool,
}

/// The DisposeCapability Record backing one `DisposableStack`/
/// `AsyncDisposableStack` instance's `[[DisposeCapability]]` internal slot.
/// Kept in a side table (like `PromiseRecord`) rather than as ordinary
/// object properties, so a stack's pending resources are never observable
/// through `Object.getOwnPropertySymbols`/`Reflect.ownKeys`.
#[derive(Default)]
pub(super) struct DisposeCapabilityState {
    pub(super) resources: Vec<DisposableResource>,
    pub(super) disposed: bool,
}

enum PromiseJob {
    Reaction {
        target: ReactionTarget,
        handler: Value,
        value: Value,
        fulfilled: bool,
    },
    /// HostCleanupFinalizationRegistry delivers a holding only after a GC has
    /// observed its target dead. It deliberately shares the normal job queue
    /// so cleanup cannot run in the middle of an ECMAScript execution.
    FinalizationCleanup { callback: Value, holdings: Value },
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
        /// The module type `import(specifier, { with: { type } })`
        /// requested, routing a synthetic one to `ensure_synthetic_module`.
        module_type: ModuleType,
        /// `import()` (`Evaluation`) or `import.defer()` (`Defer`).
        phase: ImportPhase,
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
    /// An `import.defer()` whose asynchronous dependencies are still running:
    /// fulfilled with `namespace` once all of `modules` have finished.
    WaitingDeferred {
        namespace: ObjectId,
        modules: Vec<String>,
    },
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
    /// A `ShadowRealm` instance (keyed here by its identity in the realm
    /// that actually owns the child, i.e. `target` in
    /// `export_foreign_shadow_realm`) that has already been re-exported
    /// into this realm, mapped to that re-export's own instance identity
    /// here. Deliberately separate from `imported_sources`/
    /// `imported_values`: those back an opaque, brand-less stand-in with
    /// no meaning of its own, retrievable only through this side table,
    /// whereas a re-exported `ShadowRealm` is a real instance this realm's
    /// own `shadow_realms`/`shadow_realm_by_heap` already fully describe --
    /// this map exists purely so re-exporting the same one twice returns
    /// the same instance rather than minting a second one.
    shadow_realm_reexports: HashMap<ObjectId, ObjectId>,
}

/// The two roots keep an opaque membrane transport value alive in each heap.
/// Such a value preserves identity across a call boundary. Ordinary-object
/// stand-ins additionally record child-created data properties at the call
/// boundary, then write them back through the parent VM's [[Set]] operation;
/// no parent-heap handle is exposed to child heap storage.
struct Test262ImportedValue {
    value: Value,
    property_forwarding: bool,
    _source_root: RootId,
    _target_root: RootId,
}

/// A parent-heap object that stands for an object retained in a Test262 child
/// realm. The two roots keep both endpoints alive while the membrane identity
/// is observable; the child object is never stored in the parent heap.
/// A `ShadowRealm` instance's own isolated realm: a full child `Vm` sharing
/// this `Vm`'s `GlobalSymbolRegistry` (agent-local, like
/// `$262.createRealm()`'s identical choice). Kept alive for the lifetime of
/// this `Vm`: a `ShadowRealm` value can be retained by script indefinitely,
/// so there is no earlier point at which dropping it would be safe --
/// matching [`Test262Realm`]'s identical accepted tradeoff.
/// Shared, not exclusive, ownership: a `ShadowRealm`'s child realm must
/// remain reachable from more than one owner when the `ShadowRealm`
/// *instance itself* crosses a Test262 `$262.createRealm()` boundary into a
/// third realm (see `test262.rs`'s `test262_transport_value` and its
/// ShadowRealm-specific branch) -- both the original creating realm and the
/// realm it was transported into need the exact same live child, not two
/// independent copies. `RefCell` borrows are held only for the dynamic
/// extent of one call into this child (see `shadow_realm.rs`), never
/// nested on the same `Rc` clone, so its runtime borrow check is not
/// expected to ever actually deny an access; `Rc<RefCell<_>>` is used here
/// (over an `unsafe` aliasing scheme) precisely so a bug in that assumption
/// panics loudly instead of aliasing `&mut Vm`.
#[derive(Clone)]
struct ShadowRealmRecord {
    vm: Rc<RefCell<Vm>>,
}

/// A caller-heap facade standing in for a callable value that crossed a
/// `ShadowRealm` boundary (`WrappedFunctionCreate`). `home_heap` identifies,
/// by `ObjectId::heap`, the `Vm` that owns `target`; the live pointer to
/// that `Vm` is resolved dynamically (see `shadow_realm::resolve_active`)
/// rather than stored here, since a `Vm` is an ordinary owned value its
/// caller may move between top-level calls. `_target_root` keeps `target`
/// alive against *its own* realm's collector: nothing in that realm's own
/// reachability graph necessarily still references it (it may have been an
/// unstored expression completion value), so without this root a later,
/// unrelated allocation in that realm could reclaim it out from under this
/// facade.
#[derive(Clone, Copy)]
struct ShadowWrappedFunction {
    home_heap: u64,
    target: ObjectId,
    _target_root: RootId,
}

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

/// A local ArrayBuffer and its equivalent backing buffer in a Test262 child
/// Realm. The two heaps cannot store one another's object identities, so the
/// bridge copies ordinary-buffer bytes at the boundary and synchronizes them
/// before and after an operation crosses it. Either side may own the local
/// buffer, hence `facade` is present only for child-to-parent imports.
struct Test262ForeignBufferMirror {
    realm: ObjectId,
    target: ObjectId,
    facade: Option<ObjectId>,
    _buffer_root: Option<RootId>,
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
    class_field_initializer: bool,
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
    /// Whether `%Object.prototype%.hasOwnProperty` / `.propertyIsEnumerable`
    /// have been installed. Both are created on first use, so absence of the
    /// property alone cannot mean "not installed yet": after a script deletes
    /// one, the next lookup must not silently create it again.
    has_own_property_installed: bool,
    property_is_enumerable_installed: bool,
    string_intrinsics: Option<(ObjectId, ObjectId)>,
    /// `%TypedArray%` and `%TypedArray%.prototype`, kept outside the global
    /// object but permanently reachable from every concrete constructor.
    typed_array_intrinsics: Option<(ObjectId, ObjectId)>,
    /// Annex B legacy static properties of this realm's `%RegExp%`.
    regexp_legacy: crate::regexp::LegacyStatics,
    result_root: Option<RootId>,
    /// A deliberately narrow debugger-owned root-script continuation. It is
    /// installed only at a compiler-verified root-code-unit instruction
    /// boundary and keeps the active frame out of every ordinary execution
    /// entry point until it is resumed or the VM is dropped.
    debugger_continuation: Option<DebuggerContinuation>,
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
    /// The parameter environment of the running sloppy function while its
    /// parameter list is being evaluated: the object that receives the `var`s
    /// a direct eval there declares (`Vm::call_closure` creates it).
    parameter_eval_env: Option<ObjectId>,
    /// A `TailCall` whose frame has been torn down: `[callee, this, args...]`,
    /// consumed by the `call_with_target` that ran that frame.
    pending_tail_call: Option<Vec<Value>>,
    completion_saves: Vec<(Value, bool)>,
    remaining_instructions: u64,
    cells: HashMap<usize, ObjectId>,
    /// Bytecodes supplied by the host for this realm's module loader.
    /// Dynamic imports resolve only inside this explicit registry.
    module_registry: HashMap<String, Bytecode>,
    /// Host-provided raw JSON text for `type: "json"` module requests, keyed
    /// by resolved module name. `ensure_synthetic_module` (`vm/modules.rs`)
    /// reads this lazily, on the first request for a given resolved path.
    json_module_sources: HashMap<String, String>,
    /// Host-provided, already UTF-8-decoded text for `type: "text"` module
    /// requests, keyed by resolved module name; read like `json_module_sources`.
    text_module_sources: HashMap<String, String>,
    /// Host-provided raw bytes for `type: "bytes"` module requests, keyed by
    /// resolved module name; read like `json_module_sources`.
    bytes_module_sources: HashMap<String, Vec<u8>>,
    /// Host-provided raw JavaScript text for modules the host did not (or,
    /// per `ensure_dynamic_module_compiled`'s own reason for existing,
    /// deliberately did not) pre-compile, keyed by resolved module name.
    /// Read lazily, only when a dynamic import actually resolves to a path
    /// not already present in `module_registry`.
    dynamic_module_sources: HashMap<String, String>,
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
    /// The prototype shared by every Module Source object this host makes:
    /// the host's own concrete source "class" (as `WebAssembly.Module.prototype`
    /// is for Wasm), whose [[Prototype]] is %AbstractModuleSource%.prototype
    /// when that intrinsic exists. Created with the first source object,
    /// which is rooted for the realm's lifetime and so keeps this alive.
    host_module_source_prototype: Option<ObjectId>,
    /// Retains the entry namespace until a dynamic-import job has handed it
    /// to its promise.  The next graph evaluation replaces this cache.
    last_module_namespace: Option<ObjectId>,
    last_module_namespace_root: Option<RootId>,
    /// Per-realm Module Record namespace cache. Dynamic import is required to
    /// return this same object for repeated requests, including a namespace
    /// already made visible through a static `import * as` binding.
    module_namespace_cache: HashMap<String, ObjectId>,
    module_namespace_roots: HashMap<String, RootId>,
    /// Deferred module namespace objects (import-defer proposal) and the
    /// module each one evaluates when a string-keyed internal method other
    /// than `"then"` first observes it. The per-module cache gives every
    /// module a single deferred namespace and, through its roots, keeps that
    /// object (and so this map's keys) alive.
    deferred_namespaces: HashMap<ObjectId, String>,
    module_deferred_namespace_cache: HashMap<String, ObjectId>,
    module_deferred_namespace_roots: HashMap<String, RootId>,
    module_graph: Option<ModuleGraphState>,
    /// The module records of the graph whose module code is running right
    /// now. Module evaluation holds them in a local; while user code runs
    /// they are parked here so a deferred namespace observed by that code
    /// (or an `import()` it starts) can evaluate a module of the same graph.
    evaluating_linked: Option<HashMap<String, LinkedModule>>,
    /// Roots registered by an import that joined the graph whose module code
    /// is running (see `evaluating_linked`). The importing call cannot reach
    /// that graph's own root list, which the outer evaluation holds, so it
    /// parks them here; `store_module_graph` hands them to the installed
    /// graph, which owns every root of its modules.
    nested_module_roots: Vec<RootId>,
    deferred_import_waiters: Vec<DeferredImportWaiter>,
    /// The asynchronous dependency frontier the last `import.defer()` graph
    /// load evaluated (see `gather_async_dependencies`).
    last_deferred_dependencies: Vec<String>,
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
    /// Host callbacks are private to this realm. Native function objects hold
    /// only an index into this vector, so GC sees no Rust references.
    host_functions: Vec<Box<dyn HostFunction>>,
    /// Opaque host-object factories and their collector-rooted, realm-private
    /// identity tables. Neither a JavaScript value nor a raw host key enters
    /// a Rust callback through the primitive-only HostValue ABI.
    host_object_factories: Vec<HostObjectFactoryRegistration>,
    host_object_methods: Vec<HostObjectMethodRegistration>,
    host_object_pair_methods: Vec<HostObjectPairMethodRegistration>,
    host_object_families: Vec<HostObjectFamilyState>,
    global_bindings: HashMap<String, GlobalBinding>,
    /// The GlobalSymbolRegistry belongs to an ECMAScript agent, not to an
    /// individual Realm. Test262 child realms share this handle; independent
    /// top-level VMs each create their own agent registry.
    /// Registered symbols are not valid WeakMap/WeakSet keys, whereas
    /// ordinary and well-known symbols are.
    symbol_registry: Rc<RefCell<HashMap<JsString, JsSymbol>>>,
    /// The per-realm hidden key used by the normative-optional legacy
    /// constructors. It is deliberately not a well-known Symbol: callers may
    /// discover it only through the legacy receiver's own symbol keys.
    intl_legacy_constructed_symbol: Option<JsSymbol>,
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
    // Running a class field initializer (or an arrow function created in
    // one): a direct eval there is outside a constructor for the `super()`
    // early-error rules and may not refer to `arguments`.
    class_field_initializer: bool,
    iterator_base: Option<ObjectId>,
    /// The lazily installed `%Iterator.prototype%` helpers (`flatMap`,
    /// `chunks`, `windows`) that have already been offered to the realm. Each
    /// is installed at most once, so deleting one never lets a later
    /// observation put a fresh copy back.
    iterator_helpers_installed: Vec<&'static str>,
    /// `%WrapForValidIteratorPrototype%`, shared by the iterator wrappers
    /// created by `Iterator.from`.
    iterator_wrapper_prototype: Option<ObjectId>,
    /// `%IteratorHelperPrototype%`, the common prototype for lazy iterator
    /// helper instances such as `Iterator.prototype.map`.
    iterator_helper_prototype: Option<ObjectId>,
    array_iterator_prototype: Option<ObjectId>,
    map_iterator_prototype: Option<ObjectId>,
    set_iterator_prototype: Option<ObjectId>,
    generator_function_prototype: Option<ObjectId>,
    generator_prototype: Option<ObjectId>,
    async_iterator_base: Option<ObjectId>,
    async_generator_prototype: Option<ObjectId>,
    async_generator_function_prototype: Option<ObjectId>,
    /// `%AsyncFunction.prototype%`, permanently rooted with the realm once
    /// the first async closure needs it. Its `constructor` property keeps
    /// `%AsyncFunction%` reachable without exposing a global binding.
    async_function_prototype: Option<ObjectId>,
    promise_prototype: Option<ObjectId>,
    date_prototype: Option<ObjectId>,
    map_prototype: Option<ObjectId>,
    set_prototype: Option<ObjectId>,
    weak_map_prototype: Option<ObjectId>,
    weak_set_prototype: Option<ObjectId>,
    weak_ref_prototype: Option<ObjectId>,
    finalization_registry_prototype: Option<ObjectId>,
    disposable_stack_prototype: Option<ObjectId>,
    async_disposable_stack_prototype: Option<ObjectId>,
    /// A lazily-compiled-once internal async function implementing the same
    /// `Await`-interleaved disposal loop as `Compiler::compile_async_dispose_finally`,
    /// reused by `AsyncDisposableStack.prototype.disposeAsync` (a native
    /// method, which cannot itself contain a bytecode `Await`) so it gets
    /// the exact same real per-resource-await semantics `await using`
    /// already has, rather than a separate, weaker implementation. See
    /// `Vm::async_dispose_helper`.
    async_dispose_helper: Option<Value>,
    /// `[[DisposeCapability]]` state for each live `DisposableStack`
    /// instance, keyed by its object identity.
    disposable_stacks: HashMap<ObjectId, DisposeCapabilityState>,
    /// Same, for `AsyncDisposableStack`. Kept separate from
    /// `disposable_stacks` because the two constructors are distinct
    /// brands: a `DisposableStack` method called on an `AsyncDisposableStack`
    /// instance (or vice versa) must observe a missing internal slot.
    async_disposable_stacks: HashMap<ObjectId, DisposeCapabilityState>,
    /// Pending `using`/`await using` declaration resources for every
    /// currently-open using-declaring block, across every active call frame.
    /// `Opcode::MarkDisposables`/`DisposeResources` push/pop `dispose_marks`
    /// in strict LIFO order matching lexical nesting and ordinary
    /// (synchronous) call/return, so a flat, VM-wide stack is sufficient
    /// for that case, including recursion. It is *not* isolated per
    /// generator/async-function suspension the way `iterators` is: a
    /// `yield`/`await` that suspends execution while directly inside a
    /// `using`-declaring block, with unrelated code running more `using`
    /// declarations before that generator/async function resumes, is not
    /// supported correctly. Plain synchronous functions/blocks (the
    /// overwhelming common case, and the only shape `using` -- as opposed
    /// to the unimplemented `await using` -- realistically appears in) are
    /// unaffected.
    disposables: Vec<DisposableResource>,
    dispose_marks: Vec<usize>,
    /// Targets passed to WeakRef or returned by `deref` must survive the
    /// current ECMAScript job. The list is cleared at the outer execution
    /// boundary and registered by every allocation safepoint.
    kept_weak_objects: Vec<ObjectId>,
    promises: HashMap<ObjectId, PromiseRecord>,
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
    test262_foreign_buffer_mirrors: HashMap<(ObjectId, ObjectId), Test262ForeignBufferMirror>,
    /// Object identities in this realm that stand in for a callable value
    /// owned by the parent Test262 realm.  The ordinary imported-value
    /// record remains owned by that parent (so it can keep both heaps alive),
    /// but `ShadowRealm` needs the callable bit locally when it applies
    /// `GetWrappedValue` before any call can cross its own boundary.
    test262_imported_callables: HashSet<ObjectId>,
    /// Whether the most recent function [[Construct]] this realm finished
    /// failed one of the completion checks the specification performs after
    /// the callee's execution context has been removed (a derived
    /// constructor returning a non-object or never initializing `this`).
    /// Those errors belong to the *caller's* realm; a Test262 membrane reads
    /// the flag to tell them apart from errors raised by the callee's body.
    construct_completion_check_failed: bool,
    /// The Test262 realm (a key of `test262_realms`) whose built-in function
    /// is running in this `Vm` on its behalf, because the function's
    /// operands live here rather than in its own realm. Fresh objects and
    /// errors the function creates belong to that realm. Cleared while any
    /// nested call runs, so callbacks and getters are unaffected.
    acting_realm: Option<ObjectId>,
    shadow_realm_prototype: Option<ObjectId>,
    shadow_realms: HashMap<ObjectId, ShadowRealmRecord>,
    /// Reverse index from a `ShadowRealm` child's own heap tag back to the
    /// key it is stored under in `shadow_realms`, so a wrapped-function call
    /// can find "a realm I created directly" without a linear scan.
    shadow_realm_by_heap: HashMap<u64, ObjectId>,
    shadow_wrapped_functions: HashMap<ObjectId, ShadowWrappedFunction>,
    throw_type_error: Option<ObjectId>,
    /// The shared `caller` / `arguments` getters of sloppy functions' legacy
    /// own accessors, created on first use.
    legacy_function_getters: Option<(ObjectId, ObjectId)>,
    /// The closures whose bodies are currently running, innermost last. Only
    /// legacy `f.caller` reads it: eval frames, natives, generators resumed
    /// from a call and async continuations do not appear.
    call_stack: Vec<ObjectId>,
    /// How many of `with_objects` the running function inherited from the
    /// scope it was created in (the rest were entered by its own `with`).
    inherited_with_depth: usize,
    joining: Vec<ObjectId>,
}

impl Vm {
    /// Installs one non-constructable host function as an own property of
    /// `globalThis`. The name is rejected when a global property already
    /// exists, so an embedder cannot silently replace an ECMAScript global.
    pub fn install_host_function(
        &mut self,
        name: &str,
        length: u32,
        function: impl HostFunction,
    ) -> Result<(), RuntimeError> {
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("globalThis is always an object");
        if !host_property_name_is_valid(name) || self.heap.get_own(global, name)?.is_some() {
            return Err(RuntimeError::TypeError(
                "host global name is invalid or already defined".into(),
            ));
        }
        self.install_host_callable(global, name, length, function)
    }

    /// Installs one non-callable host object as an own property of
    /// `globalThis`. The name is rejected when a property already exists, so
    /// an embedder cannot silently replace an ECMAScript global.
    pub fn install_host_object(&mut self, name: &str) -> Result<HostObject, RuntimeError> {
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("globalThis is always an object");
        if !host_property_name_is_valid(name) || self.heap.get_own(global, name)?.is_some() {
            return Err(RuntimeError::TypeError(
                "host global name is invalid or already defined".into(),
            ));
        }
        let prototype = self.object_prototype;
        let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(object));
        let result = self.define_data(global, name, Value::Object(object), false, false, false);
        self.stack.pop();
        result?;
        Ok(HostObject(object))
    }

    /// Installs one non-constructable host callback on a host object returned
    /// by [`Self::install_host_object`]. Method names and collisions are
    /// rejected before the callback is retained.
    pub fn install_host_method(
        &mut self,
        owner: HostObject,
        name: &str,
        length: u32,
        function: impl HostFunction,
    ) -> Result<(), RuntimeError> {
        if owner.0.heap != self.object_prototype.heap || !host_property_name_is_valid(name) {
            return Err(RuntimeError::TypeError(
                "host object or method name is invalid".into(),
            ));
        }
        if self.heap.get_own(owner.0, name)?.is_some() {
            return Err(RuntimeError::TypeError(
                "host method is already defined".into(),
            ));
        }
        self.install_host_callable(owner.0, name, length, function)
    }

    fn install_host_callable(
        &mut self,
        owner: ObjectId,
        name: &str,
        length: u32,
        function: impl HostFunction,
    ) -> Result<(), RuntimeError> {
        let index = u32::try_from(self.host_functions.len())
            .map_err(|_| RuntimeError::RangeError("too many host functions".into()))?;
        self.install_host_callable_native(owner, name, length, NativeFunction::Host(index))?;
        self.host_functions.push(Box::new(function));
        Ok(())
    }

    fn install_host_callable_native(
        &mut self,
        owner: ObjectId,
        name: &str,
        length: u32,
        function: NativeFunction,
    ) -> Result<(), RuntimeError> {
        let prototype = self.function_prototype()?;
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

    fn host_function_call(
        &mut self,
        index: u32,
        _receiver: Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if construct {
            return Err(RuntimeError::TypeError(
                "host functions are not constructors".into(),
            ));
        }
        let args = args
            .iter()
            .map(HostValue::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| RuntimeError::TypeError(error.to_string()))?;
        let result = self
            .host_functions
            .get_mut(index as usize)
            .ok_or_else(|| RuntimeError::TypeError("host function is unavailable".into()))?
            .call(&args)
            .map_err(|error| RuntimeError::TypeError(error.to_string()))?
            .into();
        self.check_string(&result)?;
        Ok(result)
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
        let getter_name = format!("get {name}");
        let getter =
            self.with_roots(|heap| heap.alloc_native_function(function, &getter_name, prototype))?;
        self.stack.push(Value::Object(getter));
        let result = (|| {
            self.define_data(getter, "length", Value::Number(0.0), false, false, true)?;
            self.define_data(
                getter,
                "name",
                Value::String(getter_name.into()),
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

    fn install_symbol_native_getter(
        &mut self,
        owner: ObjectId,
        prototype: ObjectId,
        name: &str,
        function: NativeFunction,
    ) -> Result<(), RuntimeError> {
        let getter_name = format!("get [Symbol.{name}]");
        let getter =
            self.with_roots(|heap| heap.alloc_native_function(function, &getter_name, prototype))?;
        self.stack.push(Value::Object(getter));
        let result = (|| {
            self.define_data(getter, "length", Value::Number(0.0), false, false, true)?;
            self.define_data(
                getter,
                "name",
                Value::String(getter_name.into()),
                false,
                false,
                true,
            )?;
            self.with_roots(|heap| {
                heap.define_own_property(
                    owner,
                    JsSymbol::well_known(name),
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

    fn install_native_accessor(
        &mut self,
        owner: ObjectId,
        prototype: ObjectId,
        name: &str,
        getter_native: NativeFunction,
        setter_native: NativeFunction,
    ) -> Result<(), RuntimeError> {
        let getter_name = format!("get {name}");
        let setter_name = format!("set {name}");
        let base = self.stack.len();
        let getter = self.with_roots(|heap| {
            heap.alloc_native_function(getter_native, &getter_name, prototype)
        })?;
        // Allocating the setter can collect immediately under a one-object
        // nursery. Keep the getter live before that second allocation.
        self.stack.push(Value::Object(getter));
        let setter = self.with_roots(|heap| {
            heap.alloc_native_function(setter_native, &setter_name, prototype)
        })?;
        self.stack.push(Value::Object(setter));
        let result = (|| {
            self.define_data(getter, "length", Value::Number(0.0), false, false, true)?;
            self.define_data(
                getter,
                "name",
                Value::String(getter_name.into()),
                false,
                false,
                true,
            )?;
            self.define_data(setter, "length", Value::Number(1.0), false, false, true)?;
            self.define_data(
                setter,
                "name",
                Value::String(setter_name.into()),
                false,
                false,
                true,
            )?;
            let defined = self.with_roots(|heap| {
                heap.define_own_property(
                    owner,
                    name,
                    PropertyDescriptor {
                        get: Some(Value::Object(getter)),
                        set: Some(Value::Object(setter)),
                        enumerable: Some(false),
                        configurable: Some(true),
                        ..PropertyDescriptor::default()
                    },
                )
            })?;
            if !defined {
                return Err(RuntimeError::TypeError(
                    "cannot install native accessor".into(),
                ));
            }
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    fn property_is_enumerable_intrinsic(&mut self) -> Result<(), RuntimeError> {
        if self.property_is_enumerable_installed {
            return Ok(());
        }
        if self
            .heap
            .get_own_property_descriptor(self.object_prototype, "propertyIsEnumerable")?
            .is_none()
        {
            let function_prototype = self.function_prototype()?;
            self.install_native(
                self.object_prototype,
                function_prototype,
                "propertyIsEnumerable",
                1,
                NativeFunction::ObjectMethod(native::ObjectMethod::PropertyIsEnumerable),
            )?;
        }
        self.property_is_enumerable_installed = true;
        Ok(())
    }

    fn has_own_property_intrinsic(&mut self) -> Result<(), RuntimeError> {
        if self.has_own_property_installed {
            return Ok(());
        }
        if self
            .heap
            .get_own_property_descriptor(self.object_prototype, "hasOwnProperty")?
            .is_none()
        {
            let function_prototype = self.function_prototype()?;
            self.install_native(
                self.object_prototype,
                function_prototype,
                "hasOwnProperty",
                1,
                NativeFunction::ObjectMethod(native::ObjectMethod::HasOwnProperty),
            )?;
        }
        self.has_own_property_installed = true;
        Ok(())
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
        let mut result = self.enter_call(callee, receiver, args, construct, target);
        // A frame that ended in a `TailCall` has already been torn down; its
        // callee runs here, at the same depth, instead of nesting under it.
        while let Some(mut call) = self.pending_tail_call.take() {
            let args = call.split_off(2);
            let receiver = call.pop().expect("a tail call carries its receiver");
            let callee = call.pop().expect("a tail call carries its callee");
            result = self.enter_call(callee, receiver, args, false, Value::Undefined);
        }
        result
    }

    fn enter_call(
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
        // An arrow function's `new.target` is lexical: the value its creating
        // function had, captured when the closure was created (not whatever
        // the caller happens to be running with). This is observable when a
        // derived-constructor arrow invokes `super()`: the superclass must
        // allocate with the original derived class.
        let closure_code = match callee.object_id() {
            Some(id) => self.heap.closure(id)?.map(|(code, _, _, _)| code),
            None => None,
        };
        let arrow = !construct && closure_code.as_ref().is_some_and(|code| code.arrow);
        let target = match (arrow, callee.object_id()) {
            (true, Some(id)) => self.heap.closure_new_target(id)?,
            _ => target,
        };
        self.charge_step()?;
        let base = self.stack.len();
        self.stack.extend([callee.clone(), receiver.clone()]);
        self.stack.extend(args.iter().cloned());
        self.stack.push(target.clone());
        let previous_target = std::mem::replace(&mut self.new_target, target);
        // Whether direct eval may use `new.target` is lexical too: it was
        // decided when the function's own code was compiled.
        let next_new_target_allowed = closure_code
            .as_ref()
            .is_some_and(|code| code.new_target_allowed);
        let previous_new_target_allowed =
            std::mem::replace(&mut self.new_target_allowed, next_new_target_allowed);
        let previous_module = callee.object_id().and_then(|id| {
            self.module_closure_referrers
                .get(&id)
                .cloned()
                .map(|module| self.active_module_name.replace(module))
        });
        self.call_depth += 1;
        // Whatever this call runs belongs to its own realm, not to the realm a
        // running native is acting for (see `acting_realm`).
        let acting_realm = self.acting_realm.take();
        let result = self
            .dispatch_call(callee, receiver, args, construct)
            .and_then(|value| {
                self.check_string(&value)?;
                Ok(value)
            });
        self.acting_realm = acting_realm;
        // The acting native turns its own unmaterialized language errors into
        // the acting realm's; create the callee's here so they are not
        // mistaken for that.
        let result = match result {
            Err(
                error @ (RuntimeError::TypeError(_)
                | RuntimeError::RangeError(_)
                | RuntimeError::ReferenceError(_)
                | RuntimeError::SyntaxError(_)),
            ) if acting_realm.is_some() => self
                .error_value(error)
                .and_then(|value| Err(RuntimeError::Thrown(value))),
            result => result,
        };
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
            if self.shadow_wrapped_functions.contains_key(&id) {
                return self.shadow_call_wrapped(id, receiver, args, construct);
            }
        }
        if let Value::Object(id) = callee {
            if let Some((code, captures, lexical_this, home)) = self.heap.closure(id)? {
                let receiver = if code.arrow { lexical_this } else { receiver };
                let with_objects = self.heap.closure_with_objects(id)?;
                return self.call_closure(builtins::ClosureCall {
                    code,
                    captures,
                    callee,
                    receiver,
                    args,
                    construct,
                    home,
                    with_objects,
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
                    | NativeFunction::Date
                    | NativeFunction::TemporalConstructor(_)
                    | NativeFunction::ArrayBuffer
                    | NativeFunction::SharedArrayBuffer
                    | NativeFunction::DataView
                    | NativeFunction::TypedArray(_)
                    | NativeFunction::Proxy
                    | NativeFunction::Map
                    | NativeFunction::Set
                    | NativeFunction::WeakMap
                    | NativeFunction::WeakSet
                    | NativeFunction::WeakRef
                    | NativeFunction::FinalizationRegistry
                    | NativeFunction::DisposableStack { .. }
                    | NativeFunction::ShadowRealm
                    | NativeFunction::Object
                    | NativeFunction::RegExp
                    | NativeFunction::Collator
                    | NativeFunction::IntlService(_)
                    | NativeFunction::Locale
                    | NativeFunction::Error(_)
                    | NativeFunction::Promise
                    | NativeFunction::Function
                    | NativeFunction::AsyncFunction
                    | NativeFunction::GeneratorFunction
                    | NativeFunction::AsyncGeneratorFunction
                    | NativeFunction::Iterator
                    | NativeFunction::PrimitiveConstructor(_)
                    // Reaches native_call so its own NewTarget-is-defined
                    // check (below) produces the throw, rather than this
                    // generic gate -- BigInt does have [[Construct]].
                    | NativeFunction::BigInt
            )
        {
            return Err(RuntimeError::TypeError("value is not a constructor".into()));
        }
        self.native_call(function, receiver, args, construct)
    }
}

#[cfg(test)]
#[path = "vm/tests.rs"]
mod tests;
