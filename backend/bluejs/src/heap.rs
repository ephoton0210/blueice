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
use num_bigint::BigInt;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem::size_of;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

mod binary_data;
pub(crate) use binary_data::{f16_bits_to_f64, f64_to_f16_bits};
mod collection_iteration;
pub(crate) use collection_iteration::CollectionEntry;
mod core;
mod exotic;
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
    InvalidWeakTarget,
    InvalidObject(ObjectId),
    InvalidInternalSlot(ObjectId),
    RevokedProxy,
    InvalidRoot(RootId),
    PrototypeCycle,
    InvalidArrayLength,
    InvalidBufferRange,
    DetachedArrayBuffer,
    ImmutableArrayBuffer,
    UninitializedModuleExport,
    ReadOnlyProperty,
    HeapLimitExceeded { limit: usize },
    IdExhausted,
}

impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig => write!(f, "invalid BlueJS heap configuration"),
            Self::InvalidWeakTarget => write!(f, "value cannot be used as a weak target"),
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
            Self::ImmutableArrayBuffer => write!(f, "ArrayBuffer is immutable"),
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
    /// Cumulative number of individual `Heap::root` registrations.
    pub root_registrations: u64,
}

pub(crate) type RegExpIteratorState = (ObjectId, JsString, bool, bool, bool);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IteratorHelperKind {
    Concat,
    Zip,
    ZipKeyed,
    Chunks,
    Windows,
    Map,
    Filter,
    FlatMap,
    Take,
    Drop,
}

#[derive(Clone)]
pub(crate) struct IteratorHelperState {
    pub record: ObjectId,
    pub callback: Value,
    pub index: u64,
    pub done: bool,
    pub executing: bool,
    pub kind: IteratorHelperKind,
}
pub(crate) type ClosureState = (Rc<Bytecode>, Vec<ObjectId>, Value, Option<ObjectId>);

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
    /// A class constructor's `[[Fields]]`: the method-like function that
    /// defines its instance elements on a newly constructed object.
    fields: Option<ObjectId>,
    /// The with objects (outermost first) that were active where a function
    /// created inside `with` was created.
    with_objects: Vec<Value>,
    /// The `new.target` an arrow function inherits from the function it was
    /// created in (unset when that was `undefined`).
    new_target: Option<Value>,
}

impl ClosureMetadata {
    fn references(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.home
            .into_iter()
            .chain(self.fields)
            .chain(self.with_objects.iter().filter_map(Value::object_id))
            .chain(self.new_target.iter().filter_map(Value::object_id))
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
    /// The frame is executing: its state has moved into the interpreter, and
    /// GeneratorValidate makes a re-entrant `next`/`return`/`throw` a
    /// TypeError. Whatever the run ends with (yield or completion) replaces it.
    Running,
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
            Self::Done | Self::Running => reference_bytes,
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
            Self::Done | Self::Running => Vec::new(),
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

/// The Temporal object kinds needed by `Intl.DateTimeFormat`'s
/// `ToDateTimeFormattable` bridge. These typed internal slots deliberately
/// avoid observable properties, so ordinary objects cannot impersonate a
/// Temporal value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TemporalKind {
    Duration,
    Instant,
    PlainDate,
    PlainDateTime,
    PlainMonthDay,
    PlainTime,
    PlainYearMonth,
    ZonedDateTime,
}

impl TemporalKind {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Duration => "Duration",
            Self::Instant => "Instant",
            Self::PlainDate => "PlainDate",
            Self::PlainDateTime => "PlainDateTime",
            Self::PlainMonthDay => "PlainMonthDay",
            Self::PlainTime => "PlainTime",
            Self::PlainYearMonth => "PlainYearMonth",
            Self::ZonedDateTime => "ZonedDateTime",
        }
    }

    pub(crate) const fn to_string_tag(self) -> &'static str {
        match self {
            Self::Duration => "Temporal.Duration",
            Self::Instant => "Temporal.Instant",
            Self::PlainDate => "Temporal.PlainDate",
            Self::PlainDateTime => "Temporal.PlainDateTime",
            Self::PlainMonthDay => "Temporal.PlainMonthDay",
            Self::PlainTime => "Temporal.PlainTime",
            Self::PlainYearMonth => "Temporal.PlainYearMonth",
            Self::ZonedDateTime => "Temporal.ZonedDateTime",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TemporalValue {
    pub kind: TemporalKind,
    /// Duration values retain their ten fields in an internal record so
    /// Intl.DurationFormat never observes replaceable prototype getters.
    pub duration: Option<Box<blueice_ecma402::DurationRecord>>,
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub millisecond: u16,
    pub microsecond: u16,
    pub nanosecond: u16,
    pub epoch_nanoseconds: BigInt,
    pub calendar: String,
    pub time_zone: String,
}

impl TemporalValue {
    pub(crate) fn bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(self.epoch_nanoseconds.to_signed_bytes_le().len())
            .saturating_add(self.calendar.len())
            .saturating_add(self.time_zone.len())
    }

    /// Interprets a Temporal plain value as an ISO local date-time carried in
    /// UTC milliseconds. This is not instant conversion: ECMA-402 requires
    /// plain Temporal values to ignore the formatter's time zone.
    pub(crate) fn plain_epoch_milliseconds(&self) -> i64 {
        let (year, month, day) = match self.kind {
            TemporalKind::PlainTime => (1970, 1, 1),
            TemporalKind::Duration => unreachable!("a duration has no date-time fields"),
            _ => (self.year, self.month, self.day),
        };
        let year = i64::from(year) - i64::from(month <= 2);
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let march_month = i64::from(month) + if month > 2 { -3 } else { 9 };
        let day_of_year = (153 * march_month + 2) / 5 + i64::from(day) - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        let days = era * 146_097 + day_of_era - 719_468;
        days * 86_400_000
            + i64::from(self.hour) * 3_600_000
            + i64::from(self.minute) * 60_000
            + i64::from(self.second) * 1_000
            + i64::from(self.millisecond)
    }
}

/// Insertion-ordered collection entries with an average O(1) SameValueZero
/// lookup index. Deletions leave order tombstones so a later insertion stays
/// at the end, keeping the representation compatible with Map and Set's
/// insertion-order iterator semantics.
#[derive(Default)]
struct OrderedCollection {
    entries: Vec<Option<(Value, Value)>>,
    indexes: HashMap<u64, Vec<usize>>,
    len: usize,
}

impl OrderedCollection {
    fn len(&self) -> usize {
        self.len
    }

    fn find_index(&self, key: &Value) -> Option<usize> {
        self.indexes
            .get(&same_value_zero_hash(key))
            .and_then(|indexes| {
                indexes.iter().copied().find(|&index| {
                    self.entries[index]
                        .as_ref()
                        .is_some_and(|(stored_key, _)| same_value_zero(stored_key, key))
                })
            })
    }

    fn get(&self, key: &Value) -> Option<&Value> {
        self.find_index(key)
            .and_then(|index| self.entries[index].as_ref().map(|(_, value)| value))
    }

    fn has(&self, key: &Value) -> bool {
        self.find_index(key).is_some()
    }

    /// Replaces a value in-place or appends a new entry. The returned value is
    /// the previous entry, allowing the heap to update its byte accounting.
    fn set(&mut self, key: Value, value: Value) -> Option<(Value, Value)> {
        if let Some(index) = self.find_index(&key) {
            return self.entries[index].replace((key, value));
        }
        let index = self.entries.len();
        self.indexes
            .entry(same_value_zero_hash(&key))
            .or_default()
            .push(index);
        self.entries.push(Some((key, value)));
        self.len += 1;
        None
    }

    fn delete(&mut self, key: &Value) -> Option<(Value, Value)> {
        let index = self.find_index(key)?;
        let hash = same_value_zero_hash(key);
        let remove_hash = {
            let indexes = self
                .indexes
                .get_mut(&hash)
                .expect("collection index was found above");
            let position = indexes
                .iter()
                .position(|&candidate| candidate == index)
                .expect("collection entry index was found above");
            indexes.swap_remove(position);
            indexes.is_empty()
        };
        if remove_hash {
            self.indexes.remove(&hash);
        }
        self.len -= 1;
        self.entries[index].take()
    }

    fn references(&self) -> Vec<ObjectId> {
        self.entries
            .iter()
            .flatten()
            .flat_map(|(key, value)| [key.object_id(), value.object_id()])
            .flatten()
            .collect()
    }
}

enum ObjectKind {
    Ordinary,
    /// The `[[IsRawJSON]]` internal slot. Raw JSON objects otherwise use the
    /// ordinary object internal methods; their frozen `rawJSON` own property
    /// stores the validated source text.
    RawJson,
    Collator {
        data: Rc<crate::intl::Collator>,
        compare: Option<ObjectId>,
    },
    NumberFormat {
        data: Rc<crate::intl::NumberFormat>,
        format: Option<ObjectId>,
    },
    DateTimeFormat {
        data: Rc<crate::intl::DateTimeFormat>,
        format: Option<ObjectId>,
    },
    DisplayNames(Rc<crate::intl::DisplayNames>),
    DurationFormat(Rc<crate::intl::DurationFormat>),
    ListFormat(Rc<crate::intl::ListFormat>),
    PluralRules(Rc<crate::intl::PluralRules>),
    RelativeTimeFormat(Rc<crate::intl::RelativeTimeFormat>),
    Segmenter(Rc<crate::intl::Segmenter>),
    Segments(Rc<crate::intl::Segments>),
    SegmentIterator {
        data: Rc<crate::intl::Segments>,
        next: usize,
    },
    IntlLocale(Rc<crate::intl::Locale>),
    Array {
        length: u32,
    },
    /// The `[[DateValue]]` internal slot, expressed as a TimeClip'd UTC
    /// millisecond count or NaN for an invalid Date.
    Date {
        time: f64,
    },
    Temporal(Box<TemporalValue>),
    /// Strong, insertion-ordered entries for the observable Map core.
    Map {
        entries: OrderedCollection,
    },
    /// Strong, insertion-ordered values for the observable Set core.
    Set {
        entries: OrderedCollection,
    },
    /// The `[[ErrorData]]` internal slot.  Error instances otherwise use
    /// ordinary property storage, but Object.prototype.toString observes
    /// this brand independently of their prototype chain or `name` value.
    Error,
    /// An ephemeron table. Keys do not become ordinary tracing edges; GC
    /// marks a value only after its key has independently become live.
    WeakCollection {
        map: bool,
        entries: HashMap<WeakCollectionKey, Value>,
    },
    /// A weak target is intentionally omitted from ordinary tracing. A live
    /// WeakRef does not keep its target alive; collection clears the slot
    /// before reclaiming a dead object target.
    WeakRef {
        target: Option<WeakCollectionKey>,
    },
    /// The registry owns its cleanup callback and holdings, but its targets
    /// and unregister tokens are weak identities. Collection-to-cleanup-job
    /// delivery stays in the VM host layer; this storage establishes the
    /// ECMAScript internal slots and their tracing boundary.
    FinalizationRegistry {
        cleanup_callback: Value,
        cells: Vec<FinalizationCell>,
    },
    /// The byte storage shared by ArrayBuffer and SharedArrayBuffer.  The
    /// `shared` marker is an internal-slot brand: ordinary ArrayBuffers can
    /// detach and resize, while shared stores cannot detach and can only grow.
    ArrayBuffer {
        bytes: Vec<u8>,
        /// SharedArrayBuffer bytes live outside a single Heap so Test262
        /// agents can install local buffer wrappers over the same storage.
        /// Ordinary ArrayBuffers keep this empty and retain their local Vec.
        shared_backing: Option<Arc<SharedBuffer>>,
        detached: bool,
        max_byte_length: Option<usize>,
        shared: bool,
        /// The proposal's `[[ArrayBufferIsImmutable]]` slot. Set once, at
        /// allocation, by `alloc_immutable_array_buffer`; an immutable buffer
        /// is always an unshared, fixed-length, never-detached ArrayBuffer
        /// whose bytes nothing may write after that allocation.
        immutable: bool,
    },
    DataView {
        buffer: ObjectId,
        byte_offset: usize,
        byte_length: usize,
        length_tracking: bool,
    },
    TypedArray {
        buffer: ObjectId,
        byte_offset: usize,
        length: usize,
        length_tracking: bool,
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
        kind: ArrayIteratorKind,
    },
    /// A Map or Set iterator (`%MapIteratorPrototype%` /
    /// `%SetIteratorPrototype%`). `index` is a position in the collection's
    /// insertion-ordered entry list -- deletions leave tombstones and `clear`
    /// empties every slot without shortening the list, so an index stays valid
    /// while the collection changes and the iterator observes those changes.
    /// `collection` is `None` once the iterator has finished
    /// (`[[IteratedMap]]`/`[[IteratedSet]]` set to undefined): it stays done.
    CollectionIterator {
        collection: Option<ObjectId>,
        index: usize,
        kind: ArrayIteratorKind,
        map: bool,
    },
    /// `Iterator.from` wraps a valid iterator which does not already inherit
    /// `%Iterator.prototype%`. The cached `next` method is an internal slot,
    /// rather than an observable property, and both references participate in
    /// ordinary heap tracing.
    IteratorWrapper {
        iterator: ObjectId,
        next: Value,
    },
    /// A lazy `Iterator` helper owns the direct iterator record it advances,
    /// its callback and the next index. The record is a normal private object
    /// so all `next`/`return` operations keep using the shared iterator
    /// abstract-operation path.
    IteratorHelper {
        record: ObjectId,
        callback: Value,
        index: u64,
        done: bool,
        executing: bool,
        kind: IteratorHelperKind,
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

struct FinalizationCell {
    /// `None` records a target observed dead by collection. The holding stays
    /// strongly retained until the future cleanup-job path consumes the cell.
    target: Option<WeakCollectionKey>,
    holdings: Value,
    unregister_token: Option<WeakCollectionKey>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum WeakCollectionKey {
    Object(ObjectId),
    Symbol(JsSymbol),
}

impl WeakCollectionKey {
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Object(object) => Some(Self::Object(*object)),
            Value::Symbol(symbol) => Some(Self::Symbol(symbol.clone())),
            _ => None,
        }
    }
}

/// Thread-safe backing bytes for a SharedArrayBuffer. The ordinary heap keeps
/// ownership and GC accounting of its local wrapper, while agents exchange a
/// clone of this handle through the Test262 host scheduler.
#[derive(Debug)]
pub(crate) struct SharedBuffer {
    bytes: Mutex<Vec<u8>>,
    waiters: Mutex<VecDeque<SharedBufferWaiter>>,
}

#[derive(Clone, Debug)]
pub(crate) struct SharedBufferWaiter {
    byte_offset: usize,
    signal: Arc<(Mutex<Option<SharedWaitResult>>, Condvar)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SharedWaitResult {
    Ok,
    TimedOut,
}

impl SharedBuffer {
    pub(crate) fn new(byte_length: usize) -> Self {
        Self {
            bytes: Mutex::new(vec![0; byte_length]),
            waiters: Mutex::new(VecDeque::new()),
        }
    }

    pub(crate) fn byte_length(&self) -> usize {
        self.bytes
            .lock()
            .expect("SharedArrayBuffer lock poisoned")
            .len()
    }

    pub(crate) fn copy(&self, offset: usize, length: usize) -> Option<Vec<u8>> {
        let bytes = self.bytes.lock().expect("SharedArrayBuffer lock poisoned");
        let end = offset.checked_add(length)?;
        (end <= bytes.len()).then(|| bytes[offset..end].to_vec())
    }

    pub(crate) fn write(&self, offset: usize, values: &[u8]) -> bool {
        let mut bytes = self.bytes.lock().expect("SharedArrayBuffer lock poisoned");
        let Some(end) = offset.checked_add(values.len()) else {
            return false;
        };
        if end > bytes.len() {
            return false;
        }
        bytes[offset..end].copy_from_slice(values);
        true
    }

    /// Performs one bounded byte-range mutation while retaining the backing
    /// lock. Atomic typed-array operations use this to make their read,
    /// comparison, and write one indivisible host operation across agent VMs.
    pub(crate) fn modify<T>(
        &self,
        offset: usize,
        length: usize,
        modify: impl FnOnce(&mut [u8]) -> T,
    ) -> Option<T> {
        let mut bytes = self.bytes.lock().expect("SharedArrayBuffer lock poisoned");
        let end = offset.checked_add(length)?;
        (end <= bytes.len()).then(|| modify(&mut bytes[offset..end]))
    }

    pub(crate) fn resize(&self, byte_length: usize) {
        self.bytes
            .lock()
            .expect("SharedArrayBuffer lock poisoned")
            .resize(byte_length, 0);
    }

    /// Adds a FIFO waiter at an Atomics byte position. The returned handle can
    /// be waited on by either a synchronous Atomics.wait caller or an async
    /// host task. It intentionally contains no VM-owned state.
    pub(crate) fn register_waiter(&self, byte_offset: usize) -> SharedBufferWaiter {
        let signal = Arc::new((Mutex::new(None), Condvar::new()));
        let waiter = SharedBufferWaiter {
            byte_offset,
            signal,
        };
        self.waiters
            .lock()
            .expect("SharedArrayBuffer waiter lock poisoned")
            .push_back(waiter.clone());
        waiter
    }

    /// Sleeps for a waiter previously registered with [`Self::register_waiter`]
    /// without holding the backing-byte lock. Only `notify` changes its status;
    /// other Atomic operations deliberately do not cause a spurious wake-up.
    pub(crate) fn wait_for(
        &self,
        waiter: SharedBufferWaiter,
        timeout: Option<Duration>,
    ) -> SharedWaitResult {
        let (status_lock, ready) = &*waiter.signal;
        let mut status = status_lock
            .lock()
            .expect("SharedArrayBuffer waiter status lock poisoned");
        if status.is_none() {
            if let Some(timeout) = timeout {
                let (next, _) = ready
                    .wait_timeout_while(status, timeout, |status| status.is_none())
                    .expect("SharedArrayBuffer waiter condition poisoned");
                status = next;
            } else {
                status = ready
                    .wait_while(status, |status| status.is_none())
                    .expect("SharedArrayBuffer waiter condition poisoned");
            }
        }
        let result = status.unwrap_or(SharedWaitResult::TimedOut);
        drop(status);
        self.waiters
            .lock()
            .expect("SharedArrayBuffer waiter lock poisoned")
            .retain(|candidate| !Arc::ptr_eq(&candidate.signal, &waiter.signal));
        result
    }

    /// Adds one FIFO waiter and waits for it synchronously.
    pub(crate) fn wait(&self, byte_offset: usize, timeout: Option<Duration>) -> SharedWaitResult {
        self.wait_for(self.register_waiter(byte_offset), timeout)
    }

    /// Wakes at most `count` waiters in insertion order for one byte
    /// position. A count of `usize::MAX` is the host representation of
    /// Atomics.notify's omitted/infinite count.
    pub(crate) fn notify(&self, byte_offset: usize, count: usize) -> usize {
        let mut waiters = self
            .waiters
            .lock()
            .expect("SharedArrayBuffer waiter lock poisoned");
        let mut woken = 0;
        let mut remaining = VecDeque::new();
        while let Some(waiter) = waiters.pop_front() {
            if waiter.byte_offset == byte_offset && woken < count {
                let (status, ready) = &*waiter.signal;
                *status
                    .lock()
                    .expect("SharedArrayBuffer waiter status lock poisoned") =
                    Some(SharedWaitResult::Ok);
                ready.notify_one();
                woken += 1;
            } else {
                remaining.push_back(waiter);
            }
        }
        *waiters = remaining;
        woken
    }
}

/// The three observable forms of Array Iterator. TypedArray reuses this
/// internal iterator because its integer-indexed properties are read through
/// the ordinary VM property boundary on every `next` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArrayIteratorKind {
    Keys,
    Values,
    Entries,
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
    Float16,
    Float32,
    Float64,
    BigInt64,
    BigUint64,
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
            Self::Int16 | Self::Uint16 | Self::Float16 => 2,
            Self::Int32 | Self::Uint32 | Self::Float32 => 4,
            Self::Float64 | Self::BigInt64 | Self::BigUint64 => 8,
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
            Self::Float16 => "Float16Array",
            Self::Float32 => "Float32Array",
            Self::Float64 => "Float64Array",
            Self::BigInt64 => "BigInt64Array",
            Self::BigUint64 => "BigUint64Array",
        }
    }

    pub(crate) const fn bigint(self) -> bool {
        matches!(self, Self::BigInt64 | Self::BigUint64)
    }

    pub(crate) const fn atomic(self) -> bool {
        matches!(
            self,
            Self::Int8
                | Self::Uint8
                | Self::Int16
                | Self::Uint16
                | Self::Int32
                | Self::Uint32
                | Self::BigInt64
                | Self::BigUint64
        )
    }

    pub(crate) const fn waitable(self) -> bool {
        matches!(self, Self::Int32 | Self::BigInt64)
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
                ObjectKind::NumberFormat { format, .. } => format.iter().copied().collect(),
                ObjectKind::DateTimeFormat { format, .. } => format.iter().copied().collect(),
                ObjectKind::DisplayNames(_) => Vec::new(),
                ObjectKind::DurationFormat(_) => Vec::new(),
                ObjectKind::ListFormat(_) => Vec::new(),
                ObjectKind::PluralRules(_) => Vec::new(),
                ObjectKind::RelativeTimeFormat(_) => Vec::new(),
                ObjectKind::Segmenter(_)
                | ObjectKind::Segments(_)
                | ObjectKind::SegmentIterator { .. } => Vec::new(),
                ObjectKind::RegExpIterator { matcher, .. } => vec![*matcher],
                ObjectKind::ArrayIterator { object, .. } => vec![*object],
                ObjectKind::CollectionIterator { collection, .. } => {
                    collection.iter().copied().collect()
                }
                ObjectKind::IteratorWrapper { iterator, next } => {
                    std::iter::once(*iterator).chain(next.object_id()).collect()
                }
                ObjectKind::IteratorHelper {
                    record, callback, ..
                } => std::iter::once(*record)
                    .chain(callback.object_id())
                    .collect(),
                ObjectKind::DataView { buffer, .. } | ObjectKind::TypedArray { buffer, .. } => {
                    vec![*buffer]
                }
                ObjectKind::Map { entries } | ObjectKind::Set { entries } => entries.references(),
                ObjectKind::Proxy {
                    target, handler, ..
                } => target.iter().chain(handler.iter()).copied().collect(),
                ObjectKind::Arguments { parameter_map } => {
                    parameter_map.values().copied().collect()
                }
                ObjectKind::FinalizationRegistry {
                    cleanup_callback,
                    cells,
                } => cleanup_callback
                    .object_id()
                    .into_iter()
                    .chain(cells.iter().filter_map(|cell| cell.holdings.object_id()))
                    .collect(),
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

fn map_entry_bytes(key: &Value, value: &Value) -> usize {
    size_of::<(Value, Value)>()
        .saturating_add(key.payload_bytes())
        .saturating_add(value.payload_bytes())
}

fn weak_collection_key_bytes(key: &WeakCollectionKey) -> usize {
    match key {
        WeakCollectionKey::Object(_) => 0,
        WeakCollectionKey::Symbol(symbol) => Value::Symbol(symbol.clone()).payload_bytes(),
    }
}

fn weak_collection_entry_bytes(key: &WeakCollectionKey, value: &Value) -> usize {
    size_of::<(WeakCollectionKey, Value)>()
        .saturating_add(weak_collection_key_bytes(key))
        .saturating_add(value.payload_bytes())
}

fn finalization_cell_bytes(
    target: &WeakCollectionKey,
    holdings: &Value,
    unregister_token: &Option<WeakCollectionKey>,
) -> usize {
    size_of::<FinalizationCell>()
        .saturating_add(weak_collection_key_bytes(target))
        .saturating_add(holdings.payload_bytes())
        .saturating_add(
            unregister_token
                .as_ref()
                .map_or(0, weak_collection_key_bytes),
        )
}

fn same_value_zero(left: &Value, right: &Value) -> bool {
    left == right
        || matches!((left, right), (Value::Number(left), Value::Number(right)) if left.is_nan() && right.is_nan())
}

fn same_value_zero_hash(value: &Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    match value {
        Value::Undefined => 0_u8.hash(&mut hasher),
        Value::Null => 1_u8.hash(&mut hasher),
        Value::Bool(value) => {
            2_u8.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        Value::Number(value) => {
            3_u8.hash(&mut hasher);
            let bits = if value.is_nan() {
                u64::MAX
            } else if *value == 0.0 {
                0
            } else {
                value.to_bits()
            };
            bits.hash(&mut hasher);
        }
        Value::BigInt(value) => {
            4_u8.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        Value::String(value) => {
            5_u8.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        Value::Symbol(value) => {
            6_u8.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        Value::Object(value) => {
            7_u8.hash(&mut hasher);
            value.hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn normalize_collection_key(key: Value) -> Value {
    if matches!(key, Value::Number(value) if value == 0.0) {
        Value::Number(0.0)
    } else {
        key
    }
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
            ObjectKind::NumberFormat { format, .. } => format.iter().copied().collect(),
            ObjectKind::DateTimeFormat { format, .. } => format.iter().copied().collect(),
            ObjectKind::DisplayNames(_) => Vec::new(),
            ObjectKind::DurationFormat(_) => Vec::new(),
            ObjectKind::ListFormat(_) => Vec::new(),
            ObjectKind::PluralRules(_) => Vec::new(),
            ObjectKind::RelativeTimeFormat(_) => Vec::new(),
            ObjectKind::Segmenter(_)
            | ObjectKind::Segments(_)
            | ObjectKind::SegmentIterator { .. } => Vec::new(),
            ObjectKind::RegExpIterator { matcher, .. } => vec![*matcher],
            ObjectKind::ArrayIterator { object, .. } => vec![*object],
            ObjectKind::CollectionIterator { collection, .. } => {
                collection.iter().copied().collect()
            }
            ObjectKind::IteratorWrapper { iterator, next } => {
                std::iter::once(*iterator).chain(next.object_id()).collect()
            }
            ObjectKind::IteratorHelper {
                record, callback, ..
            } => std::iter::once(*record)
                .chain(callback.object_id())
                .collect(),
            ObjectKind::DataView { buffer, .. } | ObjectKind::TypedArray { buffer, .. } => {
                vec![*buffer]
            }
            ObjectKind::Map { entries } | ObjectKind::Set { entries } => entries.references(),
            ObjectKind::Proxy {
                target, handler, ..
            } => target.iter().chain(handler.iter()).copied().collect(),
            ObjectKind::Arguments { parameter_map } => parameter_map.values().copied().collect(),
            ObjectKind::FinalizationRegistry {
                cleanup_callback,
                cells,
            } => cleanup_callback
                .object_id()
                .into_iter()
                .chain(cells.iter().filter_map(|cell| cell.holdings.object_id()))
                .collect(),
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
    /// Batches of temporary roots, innermost last. A VM safepoint registers
    /// everything it holds as one batch instead of one `roots` entry each.
    scoped_roots: Vec<Vec<ObjectId>>,
    managed_bytes: usize,
    next_major_bytes: usize,
    minor_collections: u64,
    major_collections: u64,
    root_registrations: u64,
    /// Advances whenever any object gains or loses an own property key or
    /// changes its `[[Prototype]]`, the only ways the set of keys a property
    /// lookup can find changes. Native loops that skip absent indices compare
    /// it after each call into user code to know their skip list went stale.
    structure_epoch: u64,
}

impl Default for Heap {
    fn default() -> Self {
        Self::new(HeapConfig::default())
            .expect("the default heap configuration is valid and heap identities are available")
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
