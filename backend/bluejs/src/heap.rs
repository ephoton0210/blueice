// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Ordinary-object/sparse-array storage and generational GC, per Phase
//! 13's runtime slices and `research/js-engine-gc.md` §4.
//! Property storage stays private so later shapes/inline caches can
//! replace it without changing the interpreter or host bindings.
//!
//! Map-backed properties carry data/accessor descriptors and String/Symbol
//! keys. Array length and boxed String indices/length have exotic attributes.
//! Native identities, compiled closures and iterator slots are heap-owned.
//! Keys and array lengths arrive converted by the VM. The heap never runs JS.
//!
//! A fixed object-count nursery promotes all survivors on minor GC.
//! Tenured objects use stop-the-world, non-compacting mark/sweep. Every
//! old-to-young property/prototype store records its owner in a coarse
//! remembered set; minor tracing reads the owner's *current* edges,
//! so overwriting/deleting a property does not retain its former value.
//! Marking uses an explicit worklist, never the Rust call stack.
//!
//! Collection safepoints are allocation, growing a property, and the
//! explicit collection calls. Operation inputs are temporary roots;
//! callers must root handles they retain across *other* safepoints.
//! The managed-byte budget is described on [`HeapConfig`]; it is not
//! an allocator guarantee or a whole-process memory limit.

use crate::native::NativeFunction;
use crate::{Bytecode, JsString, JsSymbol, ObjectId, PropertyDescriptor, PropertyName, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::mem::size_of;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

mod binary_data;
mod lifecycle;
mod object_storage;
#[cfg(test)]
mod tests;

/// A persistent root registration, independent of other registrations
/// of the same object. Release it with [`Heap::unroot`]. Copying or
/// dropping a RootId does not register/release another root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RootId {
    heap: u64,
    serial: u64,
}

/// Collector tuning for the managed object store. Byte accounting
/// charges object records, property records, both stored copies of
/// each key, and string value payloads (two bytes per UTF-16 code unit).
/// Hash-table/vector spare capacity,
/// allocator overhead, GC/root bookkeeping, and Rust-caller-owned
/// values are outside this budget; it is explicitly not an RSS limit.
#[derive(Debug, Clone, Copy)]
pub struct HeapConfig {
    /// Maximum young object count before allocation performs a minor GC.
    pub nursery_capacity: usize,
    /// Initial major-GC threshold, and the floor after subsequent GCs.
    /// Each major GC resets the trigger to twice the surviving bytes,
    /// bounded below by this floor and above by `max_heap_bytes`.
    pub major_threshold_bytes: usize,
    /// A final major GC must make enough room before a growing store
    /// or allocation can cross this ceiling; otherwise it returns an error.
    pub max_heap_bytes: usize,
}

impl Default for HeapConfig {
    fn default() -> Self {
        Self {
            nursery_capacity: 256,
            major_threshold_bytes: 256 * 1024,
            max_heap_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeapError {
    InvalidConfig,
    InvalidObject(ObjectId),
    InvalidInternalSlot(ObjectId),
    RevokedProxy,
    InvalidRoot(RootId),
    PrototypeCycle,
    InvalidArrayLength,
    InvalidBufferRange,
    DetachedArrayBuffer,
    UninitializedModuleExport,
    ReadOnlyProperty,
    HeapLimitExceeded { limit: usize },
    IdExhausted,
}

impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig => write!(f, "invalid BlueJS heap configuration"),
            Self::InvalidObject(id) => write!(f, "unknown or collected BlueJS object: {id:?}"),
            Self::InvalidInternalSlot(id) => {
                write!(f, "BlueJS object lacks the required internal slot: {id:?}")
            }
            Self::RevokedProxy => write!(f, "operation attempted on a revoked Proxy"),
            Self::InvalidRoot(id) => write!(f, "unknown or released BlueJS root: {id:?}"),
            Self::PrototypeCycle => write!(f, "a BlueJS prototype chain cannot contain a cycle"),
            Self::InvalidArrayLength => write!(
                f,
                "invalid BlueJS array length: expected an integer from 0 to 4294967295"
            ),
            Self::InvalidBufferRange => write!(f, "invalid ArrayBuffer view range"),
            Self::DetachedArrayBuffer => write!(f, "ArrayBuffer has been detached"),
            Self::UninitializedModuleExport => {
                write!(f, "module namespace export is uninitialized")
            }
            Self::ReadOnlyProperty => write!(f, "cannot assign to a read-only BlueJS property"),
            Self::HeapLimitExceeded { limit } => {
                write!(f, "BlueJS managed heap limit exceeded ({limit} bytes)")
            }
            Self::IdExhausted => write!(f, "BlueJS heap identity counter exhausted"),
        }
    }
}

impl std::error::Error for HeapError {}

/// Observable memory/collection counters, usable by a future script
/// host and pressure monitor as well as the public-API test harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeapStats {
    pub nursery_objects: usize,
    pub tenured_objects: usize,
    pub managed_bytes: usize,
    pub next_major_bytes: usize,
    pub minor_collections: u64,
    pub major_collections: u64,
}

pub(crate) type RegExpIteratorState = (ObjectId, JsString, bool, bool, bool);
pub(crate) type ClosureState = (
    Rc<Bytecode>,
    Vec<ObjectId>,
    Value,
    Option<ObjectId>,
    Option<Value>,
);

/// The class-declaration side of an ECMAScript private element.  These
/// entries live on the declaring class's home object, never in ordinary
/// property storage, so reflection and prototype lookup cannot observe them.
#[derive(Clone)]
pub(crate) enum PrivateElement {
    Field,
    Method(Value),
    Accessor {
        get: Option<Value>,
        set: Option<Value>,
    },
}

impl PrivateElement {
    fn references(&self) -> impl Iterator<Item = ObjectId> + '_ {
        match self {
            Self::Field => Vec::new().into_iter(),
            Self::Method(value) => value
                .object_id()
                .into_iter()
                .collect::<Vec<_>>()
                .into_iter(),
            Self::Accessor { get, set } => get
                .iter()
                .chain(set.iter())
                .filter_map(Value::object_id)
                .collect::<Vec<_>>()
                .into_iter(),
        }
    }
}

#[derive(Default)]
struct ClosureMetadata {
    home: Option<ObjectId>,
    class_base: Option<Value>,
}

impl ClosureMetadata {
    fn references(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.home
            .into_iter()
            .chain(self.class_base.iter().filter_map(Value::object_id))
    }
}

const CLOSURE_METADATA_BYTES: usize = size_of::<ClosureMetadata>();

/// The interpreter handler state that must remain attached to a suspended
/// generator frame.  It contains only bytecode and frame offsets, so keeping
/// it with the heap-owned frame also keeps a `yield` in a catch or finally
/// block independent of the VM's ambient execution context.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum GeneratorHandlerState {
    Try,
    Catch,
    Finally,
}

#[derive(Clone)]
pub(crate) struct GeneratorHandlerFrame {
    pub(crate) metadata: usize,
    pub(crate) stack_depth: usize,
    pub(crate) scope_depth: usize,
    pub(crate) iterator_depth: usize,
    pub(crate) with_depth: usize,
    pub(crate) state: GeneratorHandlerState,
    /// Index into `GeneratorState::Suspended::pending_completions` while a
    /// finally block is running an abrupt completion.
    pub(crate) pending: Option<usize>,
}

/// A catchable completion displaced with a generator frame.  VM-only resource
/// failures cannot enter this representation: the interpreter propagates
/// those directly rather than letting JavaScript catch them.
#[derive(Clone)]
pub(crate) enum GeneratorPendingCompletion {
    Throw(Value),
    ReferenceError(String),
    TypeError(String),
    RangeError(String),
    SyntaxError(String),
    Test262(String),
    Return(Value),
    TailRecur(Vec<Value>),
    Jump { cleanup: usize, target: usize },
}

impl GeneratorPendingCompletion {
    fn references(&self) -> Vec<ObjectId> {
        match self {
            Self::Throw(value) | Self::Return(value) => value.object_id().into_iter().collect(),
            Self::TailRecur(values) => values.iter().filter_map(Value::object_id).collect(),
            Self::ReferenceError(_)
            | Self::TypeError(_)
            | Self::RangeError(_)
            | Self::SyntaxError(_)
            | Self::Test262(_)
            | Self::Jump { .. } => Vec::new(),
        }
    }

    fn managed_bytes(&self) -> usize {
        match self {
            Self::Throw(value) | Self::Return(value) => value.payload_bytes(),
            Self::ReferenceError(message)
            | Self::TypeError(message)
            | Self::RangeError(message)
            | Self::SyntaxError(message)
            | Self::Test262(message) => message.len(),
            Self::TailRecur(values) => {
                values.len() * size_of::<Value>()
                    + values.iter().map(Value::payload_bytes).sum::<usize>()
            }
            Self::Jump { .. } => 0,
        }
    }
}

/// The explicit state of an active async `yield*` delegation.  `record` is
/// the iterator record retained by the compiler's loop; `exit_pc` continues
/// the outer generator after a forwarded `throw()` produces a final result.
#[derive(Clone)]
pub(crate) struct AsyncGeneratorDelegate {
    pub(crate) record: Value,
    pub(crate) exit_pc: usize,
}

/// Explicit state for a synchronous generator `yield*` suspension. Public
/// `throw()` and `return()` must forward into the delegate without relying on
/// an incidental bytecode layout.
#[derive(Clone)]
pub(crate) struct GeneratorDelegate {
    pub(crate) record: Value,
    pub(crate) exit_pc: usize,
}

/// A generator's suspended execution context. The VM moves this out while
/// `.next()` runs, then restores it before any subsequent allocation.
// The suspended frame is intentionally inline: it moves atomically between a
// generator heap slot and the interpreter, and is already stored behind a
// `Box<GeneratorState>` in `ObjectKind::Generator`.
#[allow(clippy::large_enum_variant)]
pub(crate) enum GeneratorState {
    Start {
        code: Rc<Bytecode>,
        captures: Vec<ObjectId>,
        callee: Value,
        receiver: Value,
        args: Vec<Value>,
        home: Option<ObjectId>,
    },
    Suspended {
        code: Rc<Bytecode>,
        pc: usize,
        stack: Vec<Value>,
        bindings: Vec<Option<Value>>,
        cells: Vec<(usize, ObjectId)>,
        this: Value,
        args: Vec<Value>,
        completion: Value,
        completion_empty: bool,
        active_scopes: Vec<u32>,
        /// Open iterator records from a destructuring operation at the yield
        /// suspension point. They must survive GC and be closed by a later
        /// generator return/throw completion.
        iterators: Vec<Value>,
        /// Active catch/finally records at the suspension point.  These must
        /// survive both a normal `.next()` and an injected `.return()` or
        /// `.throw()` request.
        handlers: Vec<GeneratorHandlerFrame>,
        /// Abrupt completions saved while an enclosing finally is running.
        pending_completions: Vec<GeneratorPendingCompletion>,
        /// The visible completion saved by a normal finally entry.
        completion_saves: Vec<(Value, bool)>,
        /// Set only while a compiler-declared async `yield*` loop is
        /// suspended at its public yield boundary.
        async_delegate: Option<AsyncGeneratorDelegate>,
        /// Set only while a compiler-declared ordinary `yield*` loop is
        /// suspended at its public yield boundary.
        delegate: Option<GeneratorDelegate>,
        dynamic_bindings: Vec<(String, ObjectId, Vec<ObjectId>)>,
        home: Option<ObjectId>,
        callee: Value,
    },
    Done,
}

/// The completion that an async-generator request supplies when it reaches
/// the head of `[[AsyncGeneratorQueue]]`.
#[derive(Clone)]
pub(crate) enum AsyncGeneratorCompletion {
    Next(Value),
    Return(Value),
    Throw(Value),
}

impl AsyncGeneratorCompletion {
    fn references(&self) -> impl Iterator<Item = ObjectId> + '_ {
        match self {
            Self::Next(value) | Self::Return(value) | Self::Throw(value) => value.object_id(),
        }
        .into_iter()
    }
}

/// A request is heap-owned by the generator until it is completed. Keeping
/// the Promise target here, rather than only in a suspended VM continuation,
/// preserves FIFO ordering and gives the collector one owner for later
/// queued arguments and capabilities.
#[derive(Clone)]
pub(crate) struct AsyncGeneratorRequest {
    pub(crate) id: u64,
    pub(crate) completion: AsyncGeneratorCompletion,
    pub(crate) target: ObjectId,
}

/// States which prevent `AsyncGeneratorResumeNext` from running a second
/// request. The frame itself remains in `GeneratorState` when suspended and
/// in an async continuation while the body awaits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AsyncGeneratorStatus {
    SuspendedStart,
    SuspendedYield,
    Awaiting,
    Executing,
    Completed,
}

#[derive(Clone)]
pub(crate) struct AsyncGeneratorControl {
    pub(crate) status: AsyncGeneratorStatus,
    pub(crate) requests: VecDeque<AsyncGeneratorRequest>,
    pub(crate) next_request_id: u64,
}

impl Default for AsyncGeneratorControl {
    fn default() -> Self {
        Self {
            status: AsyncGeneratorStatus::SuspendedStart,
            requests: VecDeque::new(),
            next_request_id: 0,
        }
    }
}

impl AsyncGeneratorControl {
    fn managed_bytes(&self) -> usize {
        self.requests.len() * size_of::<AsyncGeneratorRequest>()
            + self
                .requests
                .iter()
                .map(|request| match &request.completion {
                    AsyncGeneratorCompletion::Next(value)
                    | AsyncGeneratorCompletion::Return(value)
                    | AsyncGeneratorCompletion::Throw(value) => value.payload_bytes(),
                })
                .sum::<usize>()
    }

    fn references(&self) -> Vec<ObjectId> {
        self.requests
            .iter()
            .flat_map(|request| {
                std::iter::once(request.target).chain(request.completion.references())
            })
            .collect()
    }
}

impl GeneratorState {
    fn managed_bytes(&self) -> usize {
        let reference_bytes = self.references().len() * size_of::<ObjectId>();
        match self {
            Self::Suspended {
                handlers,
                pending_completions,
                completion_saves,
                async_delegate,
                delegate,
                ..
            } => {
                reference_bytes
                    + handlers.len() * size_of::<GeneratorHandlerFrame>()
                    + pending_completions.len() * size_of::<GeneratorPendingCompletion>()
                    + pending_completions
                        .iter()
                        .map(GeneratorPendingCompletion::managed_bytes)
                        .sum::<usize>()
                    + completion_saves.len() * size_of::<(Value, bool)>()
                    + completion_saves
                        .iter()
                        .map(|(value, _)| value.payload_bytes())
                        .sum::<usize>()
                    + async_delegate.as_ref().map_or(0, |delegate| {
                        size_of::<AsyncGeneratorDelegate>() + delegate.record.payload_bytes()
                    })
                    + delegate.as_ref().map_or(0, |delegate| {
                        size_of::<GeneratorDelegate>() + delegate.record.payload_bytes()
                    })
            }
            Self::Start { args, .. } => {
                reference_bytes + args.iter().map(Value::payload_bytes).sum::<usize>()
            }
            Self::Done => reference_bytes,
        }
    }

    fn references(&self) -> Vec<ObjectId> {
        match self {
            Self::Start {
                captures,
                callee,
                receiver,
                args,
                home,
                ..
            } => captures
                .iter()
                .copied()
                .chain(callee.object_id())
                .chain(receiver.object_id())
                .chain(args.iter().filter_map(Value::object_id))
                .chain(*home)
                .collect(),
            Self::Suspended {
                stack,
                bindings,
                cells,
                this,
                args,
                completion,
                iterators,
                pending_completions,
                completion_saves,
                async_delegate,
                delegate,
                dynamic_bindings,
                home,
                callee,
                ..
            } => {
                let mut references = stack
                    .iter()
                    .chain(bindings.iter().flatten())
                    .chain(std::iter::once(this))
                    .chain(args.iter())
                    .chain(std::iter::once(completion))
                    .chain(iterators.iter())
                    .chain(completion_saves.iter().map(|(value, _)| value))
                    .chain(async_delegate.iter().map(|delegate| &delegate.record))
                    .chain(delegate.iter().map(|delegate| &delegate.record))
                    .filter_map(Value::object_id)
                    .chain(cells.iter().map(|(_, id)| *id))
                    .chain(dynamic_bindings.iter().flat_map(|(_, id, shadowed_cells)| {
                        std::iter::once(*id).chain(shadowed_cells.iter().copied())
                    }))
                    .chain(*home)
                    .chain(callee.object_id())
                    .collect::<Vec<_>>();
                references.extend(
                    pending_completions
                        .iter()
                        .flat_map(GeneratorPendingCompletion::references),
                );
                references
            }
            Self::Done => Vec::new(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct BoundFunction {
    pub target: ObjectId,
    pub this: Value,
    pub args: Vec<Value>,
    pub constructible: bool,
}

enum ObjectKind {
    Ordinary,
    Collator {
        data: Rc<crate::intl::Collator>,
        compare: Option<ObjectId>,
    },
    IntlLocale(Rc<crate::intl::Locale>),
    Array {
        length: u32,
    },
    /// A fixed-length, non-shared ArrayBuffer backing store.  Resizable and
    /// shared buffers deliberately remain separate P1.5 slices.
    ArrayBuffer {
        bytes: Vec<u8>,
        detached: bool,
    },
    DataView {
        buffer: ObjectId,
        byte_offset: usize,
        byte_length: usize,
    },
    TypedArray {
        buffer: ObjectId,
        byte_offset: usize,
        length: usize,
        kind: TypedArrayKind,
    },
    Proxy {
        target: Option<ObjectId>,
        handler: Option<ObjectId>,
        callable: bool,
        constructible: bool,
    },
    /// The `[[ParameterMap]]` of a mapped arguments exotic object. Keys not
    /// present here are ordinary own data properties, as are every property
    /// of an unmapped arguments object.
    Arguments {
        parameter_map: HashMap<PropertyName, ObjectId>,
    },
    String(JsString),
    NativeFunction {
        function: NativeFunction,
        initial_name: JsString,
    },
    Closure {
        code: Rc<Bytecode>,
        captures: Vec<ObjectId>,
        this: Value,
    },
    Generator {
        state: Box<GeneratorState>,
        state_bytes: usize,
        async_control: Option<AsyncGeneratorControl>,
        async_control_bytes: usize,
    },
    BoundFunction(BoundFunction),
    StringIterator {
        string: JsString,
        position: usize,
    },
    RegExp(Rc<crate::regexp::RegExp>),
    BoxedPrimitive(Value),
    ArrayIterator {
        object: ObjectId,
        index: u64,
        done: bool,
    },
    RegExpIterator {
        matcher: ObjectId,
        string: JsString,
        global: bool,
        unicode: bool,
        done: bool,
    },
    /// An ECMA-262 Module Namespace Exotic Object.  Each string export holds
    /// the exporter cell itself, not a copied value, so `[[Get]]` remains a
    /// live read even after graph evaluation has completed.
    ModuleNamespace {
        exports: Vec<(JsString, ObjectId)>,
    },
}

/// Fixed-width element representations supported by the first non-shared
/// typed-array slice.  The backing bytes always use the platform-independent
/// little-endian operations below; public DataView methods choose their own
/// byte order at the VM boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypedArrayKind {
    Int8,
    Uint8,
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float32,
    Float64,
}

/// CanonicalNumericIndexString classification for integer-indexed exotic
/// objects. `Invalid` remains a numeric key: it must not fall through to an
/// ordinary named property on a TypedArray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypedArrayNumericKey {
    Index(usize),
    Invalid,
}

impl TypedArrayKind {
    pub(crate) const fn byte_width(self) -> usize {
        match self {
            Self::Int8 | Self::Uint8 | Self::Uint8Clamped => 1,
            Self::Int16 | Self::Uint16 => 2,
            Self::Int32 | Self::Uint32 | Self::Float32 => 4,
            Self::Float64 => 8,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Int8 => "Int8Array",
            Self::Uint8 => "Uint8Array",
            Self::Uint8Clamped => "Uint8ClampedArray",
            Self::Int16 => "Int16Array",
            Self::Uint16 => "Uint16Array",
            Self::Int32 => "Int32Array",
            Self::Uint32 => "Uint32Array",
            Self::Float32 => "Float32Array",
            Self::Float64 => "Float64Array",
        }
    }
}

struct Object {
    kind: ObjectKind,
    /// Host-defined Annex B slot. It changes only the abstract operations
    /// explicitly named by ECMAScript's IsHTMLDDA compatibility semantics.
    is_html_dda: bool,
    properties: HashMap<PropertyName, Value>,
    order: Vec<PropertyName>,
    attributes: HashMap<PropertyName, PropertyDescriptor>,
    /// Lazily allocated private internal slots. Ordinary objects pay no
    /// fixed-size cost for private-element support.
    private: Option<Box<PrivateData>>,
    extensible: bool,
    prototype: Option<ObjectId>,
    young: bool,
    bytes: usize,
}

impl Object {
    fn own_property(&self, key: &PropertyName) -> Option<Value> {
        if let ObjectKind::String(string) = &self.kind {
            if let Some(value) = string_property(string, key) {
                return Some(value);
            }
        }
        if key == "length" {
            if let ObjectKind::Array { length } = self.kind {
                return Some(Value::Number(f64::from(length)));
            }
        }
        self.properties.get(key).cloned()
    }

    fn references(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.prototype
            .into_iter()
            .chain(self.properties.values().filter_map(Value::object_id))
            .chain(self.attributes.values().flat_map(|d| {
                d.get
                    .iter()
                    .chain(d.set.iter())
                    .filter_map(Value::object_id)
            }))
            .chain(self.private.iter().flat_map(|private| {
                private
                    .brands
                    .iter()
                    .copied()
                    .chain(private.slots.keys().map(|(owner, _)| *owner))
                    .chain(private.slots.values().filter_map(Value::object_id))
                    .chain(
                        private
                            .elements
                            .values()
                            .flat_map(PrivateElement::references),
                    )
            }))
            .chain(match &self.kind {
                ObjectKind::Closure { captures, this, .. } => captures
                    .iter()
                    .copied()
                    .chain(this.object_id())
                    .collect::<Vec<_>>(),
                ObjectKind::Generator {
                    state,
                    async_control,
                    ..
                } => state
                    .references()
                    .into_iter()
                    .chain(
                        async_control
                            .iter()
                            .flat_map(AsyncGeneratorControl::references),
                    )
                    .collect(),
                ObjectKind::BoundFunction(bound) => std::iter::once(bound.target)
                    .chain(bound.this.object_id())
                    .chain(bound.args.iter().filter_map(Value::object_id))
                    .collect(),
                ObjectKind::NativeFunction { function, .. } => function.references(),
                ObjectKind::Collator { compare, .. } => compare.iter().copied().collect(),
                ObjectKind::RegExpIterator { matcher, .. } => vec![*matcher],
                ObjectKind::ArrayIterator { object, .. } => vec![*object],
                ObjectKind::DataView { buffer, .. } | ObjectKind::TypedArray { buffer, .. } => {
                    vec![*buffer]
                }
                ObjectKind::Proxy {
                    target, handler, ..
                } => target.iter().chain(handler.iter()).copied().collect(),
                ObjectKind::Arguments { parameter_map } => {
                    parameter_map.values().copied().collect()
                }
                ObjectKind::ModuleNamespace { exports } => {
                    exports.iter().map(|(_, cell)| *cell).collect()
                }
                _ => Vec::new(),
            })
    }
}

/// Sidecar storage for the private internal slots of one object. It is
/// allocated only for class home objects and branded instances.
#[derive(Default)]
struct PrivateData {
    /// `[[PrivateElements]]` for a class home object. It is distinct from
    /// public properties and only reachable through private References.
    elements: HashMap<JsString, PrivateElement>,
    /// Brand membership for instances (and, later, class constructors with
    /// static private elements).
    brands: HashSet<ObjectId>,
    /// Per-instance private field values, keyed by declaring home object and
    /// private name so same-spelled names from different class evaluations
    /// remain distinct.
    slots: HashMap<(ObjectId, JsString), Value>,
}

const OBJECT_BYTES: usize = size_of::<Object>();
const PRIVATE_DATA_BYTES: usize = size_of::<PrivateData>();

fn property_bytes(key: &PropertyName, value: &Value) -> usize {
    // Key storage is duplicated in `properties` and insertion `order`.
    (size_of::<(PropertyName, Value)>() + size_of::<PropertyName>())
        .saturating_add(key.byte_len().saturating_mul(2))
        .saturating_add(value.payload_bytes())
}

fn private_element_bytes(name: &JsString, element: &PrivateElement) -> usize {
    size_of::<(JsString, PrivateElement)>()
        .saturating_add(name.byte_len())
        .saturating_add(match element {
            PrivateElement::Field => 0,
            PrivateElement::Method(value) => value.payload_bytes(),
            PrivateElement::Accessor { get, set } => {
                get.iter().chain(set.iter()).map(Value::payload_bytes).sum()
            }
        })
}

fn private_slot_bytes(name: &JsString, value: &Value) -> usize {
    size_of::<((ObjectId, JsString), Value)>()
        .saturating_add(name.byte_len())
        .saturating_add(value.payload_bytes())
}

fn allocation_references(kind: &ObjectKind, prototype: Option<ObjectId>) -> Vec<ObjectId> {
    prototype
        .into_iter()
        .chain(match kind {
            ObjectKind::Closure { captures, this, .. } => captures
                .iter()
                .copied()
                .chain(this.object_id())
                .collect::<Vec<_>>(),
            ObjectKind::Generator {
                state,
                async_control,
                ..
            } => state
                .references()
                .into_iter()
                .chain(
                    async_control
                        .iter()
                        .flat_map(AsyncGeneratorControl::references),
                )
                .collect(),
            ObjectKind::BoundFunction(bound) => std::iter::once(bound.target)
                .chain(bound.this.object_id())
                .chain(bound.args.iter().filter_map(Value::object_id))
                .collect(),
            ObjectKind::NativeFunction { function, .. } => function.references(),
            ObjectKind::Collator { compare, .. } => compare.iter().copied().collect(),
            ObjectKind::RegExpIterator { matcher, .. } => vec![*matcher],
            ObjectKind::ArrayIterator { object, .. } => vec![*object],
            ObjectKind::DataView { buffer, .. } | ObjectKind::TypedArray { buffer, .. } => {
                vec![*buffer]
            }
            ObjectKind::Proxy {
                target, handler, ..
            } => target.iter().chain(handler.iter()).copied().collect(),
            ObjectKind::Arguments { parameter_map } => parameter_map.values().copied().collect(),
            ObjectKind::ModuleNamespace { exports } => {
                exports.iter().map(|(_, cell)| *cell).collect()
            }
            _ => Vec::new(),
        })
        .collect()
}

/// An ordinary-object and sparse-array heap. It owns storage, property writes
/// (including the generational write barrier), and explicit roots.
/// No mutable reference to an object's backing map escapes this type.
pub struct Heap {
    identity: u64,
    next_object: u64,
    next_root: u64,
    config: HeapConfig,
    objects: HashMap<ObjectId, Object>,
    closure_metadata: HashMap<ObjectId, ClosureMetadata>,
    nursery: Vec<ObjectId>,
    remembered: HashSet<ObjectId>,
    roots: HashMap<RootId, ObjectId>,
    managed_bytes: usize,
    next_major_bytes: usize,
    minor_collections: u64,
    major_collections: u64,
}

impl Default for Heap {
    fn default() -> Self {
        Self::new(HeapConfig::default())
            .expect("the default heap configuration is valid and heap identities are available")
    }
}

impl Heap {
    pub fn new(config: HeapConfig) -> Result<Self, HeapError> {
        if config.nursery_capacity == 0
            || config.major_threshold_bytes == 0
            || config.max_heap_bytes < OBJECT_BYTES
            || config.major_threshold_bytes > config.max_heap_bytes
        {
            return Err(HeapError::InvalidConfig);
        }
        static NEXT_HEAP: AtomicU64 = AtomicU64::new(1);
        let identity = NEXT_HEAP
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| HeapError::IdExhausted)?;
        Ok(Self {
            identity,
            next_object: 1,
            next_root: 1,
            config,
            objects: HashMap::new(),
            closure_metadata: HashMap::new(),
            nursery: Vec::new(),
            remembered: HashSet::new(),
            roots: HashMap::new(),
            managed_bytes: 0,
            next_major_bytes: config.major_threshold_bytes,
            minor_collections: 0,
            major_collections: 0,
        })
    }

    /// Allocates a blank object, with `None` meaning a null prototype.
    /// A future runtime supplies its built-in Object.prototype explicitly.
    /// May collect, protecting `prototype` and everything reachable from it.
    /// The returned object is unrooted until registered or attached to a root.
    pub fn alloc_object(&mut self, prototype: Option<ObjectId>) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::Ordinary, prototype)
    }

    fn ensure_private_data(
        &mut self,
        object: ObjectId,
        protected: &[ObjectId],
    ) -> Result<(), HeapError> {
        self.object(object)?;
        if self.objects[&object].private.is_some() {
            return Ok(());
        }
        let protected: Vec<_> = std::iter::once(object)
            .chain(protected.iter().copied())
            .collect();
        self.ensure_room(PRIVATE_DATA_BYTES, &protected)?;
        let object = self
            .objects
            .get_mut(&object)
            .expect("private owner is protected across collection");
        object.private = Some(Box::default());
        object.bytes += PRIVATE_DATA_BYTES;
        self.managed_bytes += PRIVATE_DATA_BYTES;
        Ok(())
    }

    pub(crate) fn define_private_field(
        &mut self,
        owner: ObjectId,
        name: JsString,
    ) -> Result<(), HeapError> {
        self.define_private_element(owner, name, PrivateElement::Field)
    }

    pub(crate) fn define_private_method(
        &mut self,
        owner: ObjectId,
        name: JsString,
        function: Value,
    ) -> Result<(), HeapError> {
        self.define_private_element(owner, name, PrivateElement::Method(function))
    }

    pub(crate) fn define_private_accessor(
        &mut self,
        owner: ObjectId,
        name: JsString,
        function: Value,
        setter: bool,
    ) -> Result<(), HeapError> {
        let existing = self
            .object(owner)?
            .private
            .as_ref()
            .and_then(|private| private.elements.get(&name))
            .cloned();
        let element = match existing {
            None if setter => PrivateElement::Accessor {
                get: None,
                set: Some(function),
            },
            None => PrivateElement::Accessor {
                get: Some(function),
                set: None,
            },
            Some(PrivateElement::Accessor { mut get, mut set }) => {
                if setter {
                    set = Some(function);
                } else {
                    get = Some(function);
                }
                PrivateElement::Accessor { get, set }
            }
            Some(_) => return Err(HeapError::ReadOnlyProperty),
        };
        self.define_private_element(owner, name, element)
    }

    fn define_private_element(
        &mut self,
        owner: ObjectId,
        name: JsString,
        element: PrivateElement,
    ) -> Result<(), HeapError> {
        self.object(owner)?;
        let references: Vec<_> = element.references().collect();
        for reference in &references {
            self.object(*reference)?;
        }
        self.ensure_private_data(owner, &references)?;
        let old_bytes = self.objects[&owner]
            .private
            .as_ref()
            .expect("private data was installed")
            .elements
            .get(&name)
            .map_or(0, |old| private_element_bytes(&name, old));
        let new_bytes = private_element_bytes(&name, &element);
        let protected: Vec<_> = std::iter::once(owner)
            .chain(references.iter().copied())
            .collect();
        self.ensure_room(new_bytes.saturating_sub(old_bytes), &protected)?;
        let object = self
            .objects
            .get_mut(&owner)
            .expect("private owner is protected across collection");
        object
            .private
            .as_mut()
            .expect("private data was installed")
            .elements
            .insert(name, element);
        object.bytes = object.bytes - old_bytes + new_bytes;
        self.managed_bytes = self.managed_bytes - old_bytes + new_bytes;
        for reference in references {
            self.write_barrier(owner, Some(reference));
        }
        Ok(())
    }

    pub(crate) fn add_private_brand(
        &mut self,
        receiver: ObjectId,
        owner: ObjectId,
    ) -> Result<(), HeapError> {
        self.object(receiver)?;
        self.object(owner)?;
        if self.objects[&receiver]
            .private
            .as_ref()
            .is_some_and(|private| private.brands.contains(&owner))
        {
            return Ok(());
        }
        self.ensure_private_data(receiver, &[owner])?;
        let bytes = size_of::<ObjectId>();
        self.ensure_room(bytes, &[receiver, owner])?;
        let receiver_object = self
            .objects
            .get_mut(&receiver)
            .expect("private receiver is protected across collection");
        receiver_object
            .private
            .as_mut()
            .expect("private data was installed")
            .brands
            .insert(owner);
        receiver_object.bytes += bytes;
        self.managed_bytes += bytes;
        self.write_barrier(receiver, Some(owner));
        Ok(())
    }

    pub(crate) fn has_private_brand(
        &self,
        receiver: ObjectId,
        owner: ObjectId,
    ) -> Result<bool, HeapError> {
        Ok(self
            .object(receiver)?
            .private
            .as_ref()
            .is_some_and(|private| private.brands.contains(&owner)))
    }

    pub(crate) fn private_element(
        &self,
        owner: ObjectId,
        name: &JsString,
    ) -> Result<Option<PrivateElement>, HeapError> {
        Ok(self
            .object(owner)?
            .private
            .as_ref()
            .and_then(|private| private.elements.get(name))
            .cloned())
    }

    pub(crate) fn private_slot(
        &self,
        receiver: ObjectId,
        owner: ObjectId,
        name: &JsString,
    ) -> Result<Option<Value>, HeapError> {
        Ok(self
            .object(receiver)?
            .private
            .as_ref()
            .and_then(|private| private.slots.get(&(owner, name.clone())))
            .cloned())
    }

    pub(crate) fn set_private_slot(
        &mut self,
        receiver: ObjectId,
        owner: ObjectId,
        name: JsString,
        value: Value,
    ) -> Result<(), HeapError> {
        self.object(receiver)?;
        self.object(owner)?;
        if let Some(reference) = value.object_id() {
            self.object(reference)?;
        }
        let protected: Vec<_> = std::iter::once(owner).chain(value.object_id()).collect();
        self.ensure_private_data(receiver, &protected)?;
        let key = (owner, name.clone());
        let old_bytes = self.objects[&receiver]
            .private
            .as_ref()
            .expect("private data was installed")
            .slots
            .get(&key)
            .map_or(0, |old| private_slot_bytes(&name, old));
        let new_bytes = private_slot_bytes(&name, &value);
        let protected: Vec<_> = std::iter::once(receiver)
            .chain(std::iter::once(owner))
            .chain(value.object_id())
            .collect();
        self.ensure_room(new_bytes.saturating_sub(old_bytes), &protected)?;
        let receiver_object = self
            .objects
            .get_mut(&receiver)
            .expect("private receiver is protected across collection");
        receiver_object
            .private
            .as_mut()
            .expect("private data was installed")
            .slots
            .insert(key, value.clone());
        receiver_object.bytes = receiver_object.bytes - old_bytes + new_bytes;
        self.managed_bytes = self.managed_bytes - old_bytes + new_bytes;
        self.write_barrier(receiver, Some(owner));
        self.write_barrier(receiver, value.object_id());
        Ok(())
    }

    /// Creates a non-extensible Module Namespace Exotic Object.  Its string
    /// exports are retained as binding cells; reading a namespace property
    /// therefore observes the current exporter value rather than a snapshot.
    pub(crate) fn alloc_module_namespace(
        &mut self,
        mut exports: Vec<(JsString, ObjectId)>,
    ) -> Result<ObjectId, HeapError> {
        for (_, cell) in &exports {
            self.object(*cell)?;
        }
        exports.sort_by(|(left, _), (right, _)| left.as_code_units().cmp(right.as_code_units()));
        let namespace = self.alloc(ObjectKind::ModuleNamespace { exports }, None)?;
        self.define_own_property(
            namespace,
            JsSymbol::well_known("toStringTag"),
            PropertyDescriptor::data(Value::String("Module".into()), false, false, false),
        )?;
        self.prevent_extensions(namespace)?;
        Ok(namespace)
    }

    /// Completes a namespace allocated with an empty export list.  Namespace
    /// exports can themselves name the namespace currently under
    /// construction (`export * as self from "./self.js"`), so the VM first
    /// publishes an identity-stable placeholder and fills its private export
    /// cells once recursive namespace resolution returns.  This is an
    /// internal construction operation; JavaScript still observes a
    /// non-extensible Module Namespace Exotic Object throughout.
    pub(crate) fn initialize_module_namespace(
        &mut self,
        namespace: ObjectId,
        mut exports: Vec<(JsString, ObjectId)>,
    ) -> Result<(), HeapError> {
        for (_, cell) in &exports {
            self.object(*cell)?;
        }
        exports.sort_by(|(left, _), (right, _)| left.as_code_units().cmp(right.as_code_units()));
        let additional = exports
            .iter()
            .map(|(name, _)| name.byte_len() + size_of::<(JsString, ObjectId)>())
            .sum();
        self.ensure_room(additional, &[namespace])?;
        for (_, cell) in &exports {
            self.write_barrier(namespace, Some(*cell));
        }
        let object = self
            .objects
            .get_mut(&namespace)
            .ok_or(HeapError::InvalidObject(namespace))?;
        let ObjectKind::ModuleNamespace { exports: existing } = &mut object.kind else {
            return Err(HeapError::InvalidObject(namespace));
        };
        if !existing.is_empty() {
            return Err(HeapError::InvalidObject(namespace));
        }
        *existing = exports;
        self.managed_bytes += additional;
        Ok(())
    }

    /// Allocates a host-defined exotic with the Annex B `[[IsHTMLDDA]]` slot.
    /// Only the Test262 host creates one; ordinary JavaScript cannot.
    pub(crate) fn alloc_html_dda_object(
        &mut self,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        let id = self.alloc(ObjectKind::Ordinary, prototype)?;
        self.objects
            .get_mut(&id)
            .expect("freshly allocated object is present")
            .is_html_dda = true;
        Ok(id)
    }

    pub(crate) fn is_html_dda(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(self.object(object)?.is_html_dda)
    }

    /// Allocates a sparse array of holes; even a length of u32::MAX costs
    /// only one object record. The caller supplies its prototype, exactly
    /// as for alloc_object. May collect, protecting that prototype graph.
    pub fn alloc_array(
        &mut self,
        length: u32,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::Array { length }, prototype)
    }
}

impl Heap {
    /// Allocates a Proxy exotic object. Trap dispatch stays in the VM so it
    /// can call JavaScript functions while preserving interpreter roots.
    pub(crate) fn alloc_proxy(
        &mut self,
        target: ObjectId,
        handler: ObjectId,
        prototype: Option<ObjectId>,
        callable: bool,
        constructible: bool,
    ) -> Result<ObjectId, HeapError> {
        self.object(target)?;
        self.object(handler)?;
        self.alloc(
            ObjectKind::Proxy {
                target: Some(target),
                handler: Some(handler),
                callable,
                constructible,
            },
            prototype,
        )
    }

    pub(crate) fn proxy(
        &self,
        object: ObjectId,
    ) -> Result<Option<(ObjectId, ObjectId)>, HeapError> {
        Ok(match self.object(object)?.kind {
            ObjectKind::Proxy {
                target: Some(target),
                handler: Some(handler),
                ..
            } => Some((target, handler)),
            ObjectKind::Proxy { .. } => return Err(HeapError::RevokedProxy),
            _ => None,
        })
    }

    pub(crate) fn proxy_capabilities(
        &self,
        object: ObjectId,
    ) -> Result<Option<(bool, bool)>, HeapError> {
        Ok(match self.object(object)?.kind {
            ObjectKind::Proxy {
                callable,
                constructible,
                ..
            } => Some((callable, constructible)),
            _ => None,
        })
    }

    pub(crate) fn revoke_proxy(&mut self, object: ObjectId) -> Result<(), HeapError> {
        let ObjectKind::Proxy {
            target, handler, ..
        } = &mut self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?
            .kind
        else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        *target = None;
        *handler = None;
        Ok(())
    }

    /// Allocates the storage for an arguments object. A non-empty parameter
    /// map makes it an arguments exotic object; an empty map is the ordinary
    /// unmapped variant but retains the same internal-slot representation.
    pub(crate) fn alloc_arguments(
        &mut self,
        parameter_map: HashMap<PropertyName, ObjectId>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::Arguments { parameter_map }, Some(prototype))
    }

    /// A boxed String with read-only, non-configurable virtual indices
    /// and length. The string payload is charged to the managed budget.
    pub fn alloc_string(
        &mut self,
        string: JsString,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::String(string), prototype)
    }

    pub(crate) fn alloc_native_function(
        &mut self,
        function: NativeFunction,
        name: &str,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::NativeFunction {
                function,
                initial_name: name.into(),
            },
            Some(prototype),
        )
    }

    pub(crate) fn native_function(
        &self,
        object: ObjectId,
    ) -> Result<Option<NativeFunction>, HeapError> {
        Ok(match self.object(object)?.kind {
            ObjectKind::NativeFunction { function, .. } => Some(function),
            _ => None,
        })
    }

    pub(crate) fn function_initial_name(&self, object: ObjectId) -> Result<JsString, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::NativeFunction { initial_name, .. } => initial_name.clone(),
            // HostHasSourceTextAvailable is false for compiled functions.
            // Anonymous NativeFunction syntax is valid for every callable.
            _ => JsString::default(),
        })
    }

    pub(crate) fn boxed_string(&self, object: ObjectId) -> Result<Option<&JsString>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::String(string) => Some(string),
            _ => None,
        })
    }

    pub(crate) fn alloc_closure(
        &mut self,
        code: Rc<Bytecode>,
        captures: Vec<ObjectId>,
        this: Value,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::Closure {
                code,
                captures,
                this,
            },
            Some(prototype),
        )
    }

    pub(crate) fn alloc_generator(
        &mut self,
        state: GeneratorState,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        let state_bytes = state.managed_bytes();
        self.alloc(
            ObjectKind::Generator {
                state: Box::new(state),
                state_bytes,
                async_control: None,
                async_control_bytes: 0,
            },
            Some(prototype),
        )
    }

    pub(crate) fn take_generator_state(
        &mut self,
        object: ObjectId,
    ) -> Result<GeneratorState, HeapError> {
        let entry = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        let ObjectKind::Generator { state, .. } = &mut entry.kind else {
            return Err(HeapError::InvalidObject(object));
        };
        Ok(*std::mem::replace(state, Box::new(GeneratorState::Done)))
    }

    pub(crate) fn generator_state_is_done(&self, object: ObjectId) -> Result<bool, HeapError> {
        let ObjectKind::Generator { state, .. } = &self.object(object)?.kind else {
            return Err(HeapError::InvalidObject(object));
        };
        Ok(matches!(**state, GeneratorState::Done))
    }

    pub(crate) fn set_generator_state(
        &mut self,
        object: ObjectId,
        state: GeneratorState,
    ) -> Result<(), HeapError> {
        // A generator may have been promoted while it was running. Restoring
        // a suspended frame can then install young bindings/cells into an old
        // generator object, so this internal-slot write needs the same
        // remembered-set barrier as an ordinary property write.
        let references = state.references();
        let state_bytes = state.managed_bytes();
        let old_state_bytes = match &self
            .objects
            .get(&object)
            .ok_or(HeapError::InvalidObject(object))?
            .kind
        {
            ObjectKind::Generator { state_bytes, .. } => *state_bytes,
            _ => return Err(HeapError::InvalidObject(object)),
        };
        // Collection may be needed before a suspended frame is restored.
        // Keep both the generator and its incoming references alive across it.
        let protected: Vec<_> = std::iter::once(object)
            .chain(references.iter().copied())
            .collect();
        self.ensure_room(state_bytes.saturating_sub(old_state_bytes), &protected)?;
        {
            let entry = self
                .objects
                .get_mut(&object)
                .ok_or(HeapError::InvalidObject(object))?;
            let ObjectKind::Generator {
                state: current,
                state_bytes: current_bytes,
                ..
            } = &mut entry.kind
            else {
                return Err(HeapError::InvalidObject(object));
            };
            **current = state;
            *current_bytes = state_bytes;
            entry.bytes = entry.bytes - old_state_bytes + state_bytes;
        }
        self.managed_bytes = self.managed_bytes - old_state_bytes + state_bytes;
        for reference in references {
            self.write_barrier(object, Some(reference));
        }
        Ok(())
    }

    /// Marks a generator object as async before it becomes observable to
    /// JavaScript. The queue starts empty and therefore cannot allocate or
    /// create collector edges.
    pub(crate) fn enable_async_generator(&mut self, object: ObjectId) -> Result<(), HeapError> {
        let entry = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        let ObjectKind::Generator { async_control, .. } = &mut entry.kind else {
            return Err(HeapError::InvalidObject(object));
        };
        if async_control.is_none() {
            *async_control = Some(AsyncGeneratorControl::default());
        }
        Ok(())
    }

    pub(crate) fn async_generator_control(
        &self,
        object: ObjectId,
    ) -> Result<Option<AsyncGeneratorControl>, HeapError> {
        let ObjectKind::Generator { async_control, .. } = &self.object(object)?.kind else {
            return Err(HeapError::InvalidObject(object));
        };
        Ok(async_control.clone())
    }

    /// Replaces the async-generator queue and performs the same accounting
    /// and old-to-young barriers as a suspended frame restoration.
    pub(crate) fn set_async_generator_control(
        &mut self,
        object: ObjectId,
        control: AsyncGeneratorControl,
    ) -> Result<(), HeapError> {
        let references = control.references();
        let control_bytes = control.managed_bytes();
        let (old_bytes, old_references) = match &self
            .objects
            .get(&object)
            .ok_or(HeapError::InvalidObject(object))?
            .kind
        {
            ObjectKind::Generator {
                async_control,
                async_control_bytes,
                ..
            } => (
                *async_control_bytes,
                async_control
                    .iter()
                    .flat_map(AsyncGeneratorControl::references)
                    .collect::<Vec<_>>(),
            ),
            _ => return Err(HeapError::InvalidObject(object)),
        };
        let protected: Vec<_> = std::iter::once(object)
            .chain(references.iter().copied())
            .chain(old_references)
            .collect();
        self.ensure_room(control_bytes.saturating_sub(old_bytes), &protected)?;
        {
            let entry = self
                .objects
                .get_mut(&object)
                .ok_or(HeapError::InvalidObject(object))?;
            let ObjectKind::Generator {
                async_control,
                async_control_bytes,
                ..
            } = &mut entry.kind
            else {
                return Err(HeapError::InvalidObject(object));
            };
            *async_control = Some(control);
            *async_control_bytes = control_bytes;
            entry.bytes = entry.bytes - old_bytes + control_bytes;
        }
        self.managed_bytes = self.managed_bytes - old_bytes + control_bytes;
        for reference in references {
            self.write_barrier(object, Some(reference));
        }
        Ok(())
    }

    pub(crate) fn alloc_bound_function(
        &mut self,
        bound: BoundFunction,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::BoundFunction(bound), prototype)
    }

    pub(crate) fn bound_function(
        &self,
        object: ObjectId,
    ) -> Result<Option<&BoundFunction>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::BoundFunction(bound) => Some(bound),
            _ => None,
        })
    }

    pub(crate) fn alloc_collator(
        &mut self,
        data: Rc<crate::intl::Collator>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::Collator {
                data,
                compare: None,
            },
            Some(prototype),
        )
    }
    pub(crate) fn collator(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::intl::Collator>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::Collator { data, .. } => Some(data.clone()),
            _ => None,
        })
    }
    pub(crate) fn collator_compare(&self, object: ObjectId) -> Option<ObjectId> {
        let ObjectKind::Collator { compare, .. } = &self.objects.get(&object).unwrap().kind else {
            unreachable!("VM checks the Collator brand")
        };
        *compare
    }
    pub(crate) fn set_collator_compare(&mut self, object: ObjectId, function: ObjectId) {
        if let ObjectKind::Collator { compare, .. } =
            &mut self.objects.get_mut(&object).unwrap().kind
        {
            *compare = Some(function);
        }
        self.write_barrier(object, Some(function));
    }

    pub(crate) fn alloc_intl_locale(
        &mut self,
        data: Rc<crate::intl::Locale>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::IntlLocale(data), Some(prototype))
    }

    pub(crate) fn intl_locale(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::intl::Locale>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::IntlLocale(data) => Some(data.clone()),
            _ => None,
        })
    }

    pub(crate) fn alloc_regexp(
        &mut self,
        regexp: Rc<crate::regexp::RegExp>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::RegExp(regexp), Some(prototype))
    }
    pub(crate) fn alloc_boxed_primitive(
        &mut self,
        value: Value,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::BoxedPrimitive(value), Some(prototype))
    }
    pub(crate) fn boxed_primitive(&self, object: ObjectId) -> Result<Option<Value>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::BoxedPrimitive(value) => Some(value.clone()),
            _ => None,
        })
    }
    pub(crate) fn alloc_array_iterator(
        &mut self,
        object: ObjectId,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::ArrayIterator {
                object,
                index: 0,
                done: false,
            },
            Some(prototype),
        )
    }
    pub(crate) fn array_iterator(
        &self,
        id: ObjectId,
    ) -> Result<Option<(ObjectId, u64, bool)>, HeapError> {
        Ok(match self.object(id)?.kind {
            ObjectKind::ArrayIterator {
                object,
                index,
                done,
            } => Some((object, index, done)),
            _ => None,
        })
    }
    pub(crate) fn advance_array_iterator(&mut self, id: ObjectId, done: bool) {
        if let ObjectKind::ArrayIterator {
            index,
            done: finished,
            ..
        } = &mut self.objects.get_mut(&id).unwrap().kind
        {
            *index += 1;
            *finished = done;
        }
    }
    pub(crate) fn regexp(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::regexp::RegExp>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::RegExp(regexp) => Some(regexp.clone()),
            _ => None,
        })
    }
    pub(crate) fn alloc_regexp_iterator(
        &mut self,
        matcher: ObjectId,
        string: JsString,
        global: bool,
        unicode: bool,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::RegExpIterator {
                matcher,
                string,
                global,
                unicode,
                done: false,
            },
            Some(prototype),
        )
    }
    pub(crate) fn regexp_iterator(
        &self,
        object: ObjectId,
    ) -> Result<Option<RegExpIteratorState>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::RegExpIterator {
                matcher,
                string,
                global,
                unicode,
                done,
            } => Some((*matcher, string.clone(), *global, *unicode, *done)),
            _ => None,
        })
    }
    pub(crate) fn finish_regexp_iterator(&mut self, object: ObjectId) {
        if let ObjectKind::RegExpIterator { done, .. } =
            &mut self.objects.get_mut(&object).unwrap().kind
        {
            *done = true;
        }
    }

    pub(crate) fn closure(&self, object: ObjectId) -> Result<Option<ClosureState>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::Closure {
                code,
                captures,
                this,
            } => {
                let metadata = self.closure_metadata.get(&object);
                Some((
                    code.clone(),
                    captures.clone(),
                    this.clone(),
                    metadata.and_then(|metadata| metadata.home),
                    metadata.and_then(|metadata| metadata.class_base.clone()),
                ))
            }
            _ => None,
        })
    }

    pub(crate) fn set_closure_home(
        &mut self,
        object: ObjectId,
        home: ObjectId,
    ) -> Result<(), HeapError> {
        self.object(home)?;
        if !matches!(self.object(object)?.kind, ObjectKind::Closure { .. }) {
            return Err(HeapError::InvalidObject(object));
        }
        self.ensure_closure_metadata(object, &[home])?;
        self.write_barrier(object, Some(home));
        self.closure_metadata
            .get_mut(&object)
            .expect("metadata was installed")
            .home = Some(home);
        Ok(())
    }

    pub(crate) fn set_class_base(
        &mut self,
        object: ObjectId,
        base: Value,
    ) -> Result<(), HeapError> {
        if !matches!(self.object(object)?.kind, ObjectKind::Closure { .. }) {
            return Err(HeapError::InvalidObject(object));
        }
        if let Some(target) = base.object_id() {
            self.ensure_closure_metadata(object, &[target])?;
        } else {
            self.ensure_closure_metadata(object, &[])?;
        }
        self.write_barrier(object, base.object_id());
        self.closure_metadata
            .get_mut(&object)
            .expect("metadata was installed")
            .class_base = Some(base);
        Ok(())
    }

    pub(crate) fn class_base(&self, object: ObjectId) -> Result<Option<Value>, HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::Closure { .. } => Ok(self
                .closure_metadata
                .get(&object)
                .and_then(|metadata| metadata.class_base.clone())),
            _ => Err(HeapError::InvalidObject(object)),
        }
    }

    fn ensure_closure_metadata(
        &mut self,
        object: ObjectId,
        protected: &[ObjectId],
    ) -> Result<(), HeapError> {
        if self.closure_metadata.contains_key(&object) {
            return Ok(());
        }
        let protected: Vec<_> = std::iter::once(object)
            .chain(protected.iter().copied())
            .collect();
        self.ensure_room(CLOSURE_METADATA_BYTES, &protected)?;
        self.closure_metadata
            .insert(object, ClosureMetadata::default());
        self.managed_bytes += CLOSURE_METADATA_BYTES;
        Ok(())
    }

    pub(crate) fn alloc_string_iterator(
        &mut self,
        string: JsString,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::StringIterator {
                string,
                position: 0,
            },
            Some(prototype),
        )
    }

    pub(crate) fn string_iterator_next(
        &mut self,
        object: ObjectId,
    ) -> Result<Option<Option<JsString>>, HeapError> {
        let obj = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        let ObjectKind::StringIterator { string, position } = &mut obj.kind else {
            return Ok(None);
        };
        if *position == string.len() {
            return Ok(Some(None));
        }
        let start = *position;
        let units = string.as_code_units();
        *position += if (0xd800..=0xdbff).contains(&units[start])
            && units
                .get(start + 1)
                .is_some_and(|c| (0xdc00..=0xdfff).contains(c))
        {
            2
        } else {
            1
        };
        Ok(Some(Some(JsString::from_code_units(
            units[start..*position].to_vec(),
        ))))
    }

    pub fn get_own_property_descriptor(
        &self,
        object: ObjectId,
        key: impl Into<PropertyName>,
    ) -> Result<Option<PropertyDescriptor>, HeapError> {
        self.get_own_property_descriptor_key(object, key.into())
    }
}

fn array_index(key: &PropertyName) -> Option<u32> {
    let index = u32::try_from(key.index()?).ok()?;
    (index != u32::MAX).then_some(index)
}

fn string_property(string: &JsString, key: &PropertyName) -> Option<Value> {
    match key {
        PropertyName::String(key) => string.own_property(key),
        _ => None,
    }
}

pub(crate) fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
        }
        _ => a == b,
    }
}

fn attribute_bytes(key: &PropertyName, descriptor: &PropertyDescriptor) -> usize {
    size_of::<(PropertyName, PropertyDescriptor)>()
        + key.byte_len()
        + descriptor
            .get
            .iter()
            .chain(descriptor.set.iter())
            .map(Value::payload_bytes)
            .sum::<usize>()
}
