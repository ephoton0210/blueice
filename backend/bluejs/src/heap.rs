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
use crate::{Bytecode, JsString, ObjectId, PropertyDescriptor, PropertyName, Value};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::mem::size_of;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

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
        Self { nursery_capacity: 256, major_threshold_bytes: 256 * 1024, max_heap_bytes: 16 * 1024 * 1024 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeapError {
    InvalidConfig,
    InvalidObject(ObjectId),
    InvalidRoot(RootId),
    PrototypeCycle,
    InvalidArrayLength,
    ReadOnlyProperty,
    HeapLimitExceeded { limit: usize },
    IdExhausted,
}

impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig => write!(f, "invalid BlueJS heap configuration"),
            Self::InvalidObject(id) => write!(f, "unknown or collected BlueJS object: {id:?}"),
            Self::InvalidRoot(id) => write!(f, "unknown or released BlueJS root: {id:?}"),
            Self::PrototypeCycle => write!(f, "a BlueJS prototype chain cannot contain a cycle"),
            Self::InvalidArrayLength => write!(f, "invalid BlueJS array length: expected an integer from 0 to 4294967295"),
            Self::ReadOnlyProperty => write!(f, "cannot assign to a read-only BlueJS property"),
            Self::HeapLimitExceeded { limit } => write!(f, "BlueJS managed heap limit exceeded ({limit} bytes)"),
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
pub(crate) type ClosureState = (Rc<Bytecode>, Vec<ObjectId>, Value);

#[derive(Clone)]
pub(crate) struct BoundFunction {
    pub target: ObjectId,
    pub this: Value,
    pub args: Vec<Value>,
    pub constructible: bool,
}

enum ObjectKind {
    Ordinary,
    Collator { data: Rc<crate::intl::Collator>, compare: Option<ObjectId> },
    Array { length: u32 },
    String(JsString),
    NativeFunction { function: NativeFunction, initial_name: JsString },
    Closure { code: Rc<Bytecode>, captures: Vec<ObjectId>, this: Value },
    BoundFunction(BoundFunction),
    StringIterator { string: JsString, position: usize },
    RegExp(Rc<crate::regexp::RegExp>),
    BoxedPrimitive(Value),
    ArrayIterator { object: ObjectId, index: u64, done: bool },
    RegExpIterator { matcher: ObjectId, string: JsString, global: bool, unicode: bool, done: bool },
}

struct Object {
    kind: ObjectKind,
    properties: HashMap<PropertyName, Value>,
    order: Vec<PropertyName>,
    attributes: HashMap<PropertyName, PropertyDescriptor>,
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
            .chain(self.attributes.values().flat_map(|d| d.get.iter().chain(d.set.iter()).filter_map(Value::object_id)))
            .chain(match &self.kind {
                ObjectKind::Closure { captures, this, .. } => captures.iter().copied().chain(this.object_id()).collect::<Vec<_>>(),
                ObjectKind::BoundFunction(bound) => std::iter::once(bound.target).chain(bound.this.object_id()).chain(bound.args.iter().filter_map(Value::object_id)).collect(),
                ObjectKind::Collator { compare, .. } => compare.iter().copied().collect(),
                ObjectKind::RegExpIterator { matcher, .. } => vec![*matcher],
                ObjectKind::ArrayIterator { object, .. } => vec![*object],
                _ => Vec::new(),
            })
    }
}

const OBJECT_BYTES: usize = size_of::<Object>();

fn property_bytes(key: &PropertyName, value: &Value) -> usize {
    // Key storage is duplicated in `properties` and insertion `order`.
    (size_of::<(PropertyName, Value)>() + size_of::<PropertyName>()).saturating_add(key.byte_len().saturating_mul(2)).saturating_add(value.payload_bytes())
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
        Self::new(HeapConfig::default()).expect("the default heap configuration is valid and heap identities are available")
    }
}

impl Heap {
    pub fn new(config: HeapConfig) -> Result<Self, HeapError> {
        if config.nursery_capacity == 0 || config.major_threshold_bytes == 0 || config.max_heap_bytes < OBJECT_BYTES || config.major_threshold_bytes > config.max_heap_bytes {
            return Err(HeapError::InvalidConfig);
        }
        static NEXT_HEAP: AtomicU64 = AtomicU64::new(1);
        let identity = NEXT_HEAP.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1)).map_err(|_| HeapError::IdExhausted)?;
        Ok(Self {
            identity,
            next_object: 1,
            next_root: 1,
            config,
            objects: HashMap::new(),
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

    /// Allocates a sparse array of holes; even a length of u32::MAX costs
    /// only one object record. The caller supplies its prototype, exactly
    /// as for alloc_object. May collect, protecting that prototype graph.
    pub fn alloc_array(&mut self, length: u32, prototype: Option<ObjectId>) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::Array { length }, prototype)
    }

    /// A boxed String with read-only, non-configurable virtual indices
    /// and length. The string payload is charged to the managed budget.
    pub fn alloc_string(&mut self, string: JsString, prototype: Option<ObjectId>) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::String(string), prototype)
    }

    pub(crate) fn alloc_native_function(&mut self, function: NativeFunction, name: &str, prototype: ObjectId) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::NativeFunction { function, initial_name: name.into() }, Some(prototype))
    }

    pub(crate) fn native_function(&self, object: ObjectId) -> Result<Option<NativeFunction>, HeapError> {
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

    pub(crate) fn alloc_closure(&mut self, code: Rc<Bytecode>, captures: Vec<ObjectId>, this: Value, prototype: ObjectId) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::Closure { code, captures, this }, Some(prototype))
    }

    pub(crate) fn alloc_bound_function(&mut self, bound: BoundFunction, prototype: Option<ObjectId>) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::BoundFunction(bound), prototype)
    }

    pub(crate) fn bound_function(&self, object: ObjectId) -> Result<Option<&BoundFunction>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::BoundFunction(bound) => Some(bound),
            _ => None,
        })
    }

    pub(crate) fn alloc_collator(&mut self, data: Rc<crate::intl::Collator>, prototype: ObjectId) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::Collator { data, compare: None }, Some(prototype))
    }
    pub(crate) fn collator(&self, object: ObjectId) -> Result<Option<Rc<crate::intl::Collator>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::Collator { data, .. } => Some(data.clone()),
            _ => None,
        })
    }
    pub(crate) fn collator_compare(&self, object: ObjectId) -> Option<ObjectId> {
        let ObjectKind::Collator { compare, .. } = &self.objects.get(&object).unwrap().kind else { unreachable!("VM checks the Collator brand") };
        *compare
    }
    pub(crate) fn set_collator_compare(&mut self, object: ObjectId, function: ObjectId) {
        if let ObjectKind::Collator { compare, .. } = &mut self.objects.get_mut(&object).unwrap().kind {
            *compare = Some(function);
        }
        self.write_barrier(object, Some(function));
    }

    pub(crate) fn alloc_regexp(&mut self, regexp: Rc<crate::regexp::RegExp>, prototype: ObjectId) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::RegExp(regexp), Some(prototype))
    }
    pub(crate) fn alloc_boxed_primitive(&mut self, value: Value, prototype: ObjectId) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::BoxedPrimitive(value), Some(prototype))
    }
    pub(crate) fn boxed_primitive(&self, object: ObjectId) -> Result<Option<Value>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::BoxedPrimitive(value) => Some(value.clone()),
            _ => None,
        })
    }
    pub(crate) fn alloc_array_iterator(&mut self, object: ObjectId, prototype: ObjectId) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::ArrayIterator { object, index: 0, done: false }, Some(prototype))
    }
    pub(crate) fn array_iterator(&self, id: ObjectId) -> Result<Option<(ObjectId, u64, bool)>, HeapError> {
        Ok(match self.object(id)?.kind {
            ObjectKind::ArrayIterator { object, index, done } => Some((object, index, done)),
            _ => None,
        })
    }
    pub(crate) fn advance_array_iterator(&mut self, id: ObjectId, done: bool) {
        if let ObjectKind::ArrayIterator { index, done: finished, .. } = &mut self.objects.get_mut(&id).unwrap().kind {
            *index += 1;
            *finished = done;
        }
    }
    pub(crate) fn regexp(&self, object: ObjectId) -> Result<Option<Rc<crate::regexp::RegExp>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::RegExp(regexp) => Some(regexp.clone()),
            _ => None,
        })
    }
    pub(crate) fn alloc_regexp_iterator(&mut self, matcher: ObjectId, string: JsString, global: bool, unicode: bool, prototype: ObjectId) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::RegExpIterator { matcher, string, global, unicode, done: false }, Some(prototype))
    }
    pub(crate) fn regexp_iterator(&self, object: ObjectId) -> Result<Option<RegExpIteratorState>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::RegExpIterator { matcher, string, global, unicode, done } => Some((*matcher, string.clone(), *global, *unicode, *done)),
            _ => None,
        })
    }
    pub(crate) fn finish_regexp_iterator(&mut self, object: ObjectId) {
        if let ObjectKind::RegExpIterator { done, .. } = &mut self.objects.get_mut(&object).unwrap().kind {
            *done = true;
        }
    }

    pub(crate) fn closure(&self, object: ObjectId) -> Result<Option<ClosureState>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::Closure { code, captures, this } => Some((code.clone(), captures.clone(), this.clone())),
            _ => None,
        })
    }

    pub(crate) fn alloc_string_iterator(&mut self, string: JsString, prototype: ObjectId) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::StringIterator { string, position: 0 }, Some(prototype))
    }

    pub(crate) fn string_iterator_next(&mut self, object: ObjectId) -> Result<Option<Option<JsString>>, HeapError> {
        let obj = self.objects.get_mut(&object).ok_or(HeapError::InvalidObject(object))?;
        let ObjectKind::StringIterator { string, position } = &mut obj.kind else { return Ok(None) };
        if *position == string.len() {
            return Ok(Some(None));
        }
        let start = *position;
        let units = string.as_code_units();
        *position += if (0xd800..=0xdbff).contains(&units[start]) && units.get(start + 1).is_some_and(|c| (0xdc00..=0xdfff).contains(c)) { 2 } else { 1 };
        Ok(Some(Some(JsString::from_code_units(units[start..*position].to_vec()))))
    }

    pub fn get_own_property_descriptor(&self, object: ObjectId, key: impl Into<PropertyName>) -> Result<Option<PropertyDescriptor>, HeapError> {
        self.get_own_property_descriptor_key(object, key.into())
    }

    fn get_own_property_descriptor_key(&self, object: ObjectId, key: PropertyName) -> Result<Option<PropertyDescriptor>, HeapError> {
        let obj = self.object(object)?;
        if let Some(descriptor) = obj.attributes.get(&key) {
            let mut descriptor = descriptor.clone();
            if !descriptor.accessor() {
                descriptor.value = obj.own_property(&key);
            }
            return Ok(Some(descriptor));
        }
        Ok(obj.own_property(&key).map(|value| {
            let string_virtual = matches!(&obj.kind, ObjectKind::String(s) if string_property(s, &key).is_some());
            let length = key == "length" && matches!(&obj.kind, ObjectKind::Array { .. } | ObjectKind::String(_));
            PropertyDescriptor::data(value, !string_virtual, !length, !string_virtual && !length)
        }))
    }

    pub fn define_own_property(&mut self, object: ObjectId, key: impl Into<PropertyName>, descriptor: PropertyDescriptor) -> Result<bool, HeapError> {
        self.define_own_property_key(object, key.into(), descriptor)
    }

    fn define_own_property_key(&mut self, object: ObjectId, key: PropertyName, descriptor: PropertyDescriptor) -> Result<bool, HeapError> {
        if descriptor.accessor() && (descriptor.value.is_some() || descriptor.writable.is_some()) {
            return Ok(false);
        }
        let old = self.get_own_property_descriptor(object, &key)?;
        if old.is_none() && !self.object(object)?.extensible {
            return Ok(false);
        }
        let obj = self.object(object)?;
        if let ObjectKind::Array { length } = obj.kind {
            if array_index(&key).is_some_and(|index| index >= length) && obj.attributes.get(&"length".into()).is_some_and(|d| d.writable == Some(false)) {
                return Ok(false);
            }
        }
        if let Some(old) = &old {
            if old.configurable == Some(false) {
                if descriptor.configurable == Some(true) || descriptor.enumerable.is_some_and(|v| Some(v) != old.enumerable) {
                    return Ok(false);
                }
                let changes_kind = if descriptor.accessor() { !old.accessor() } else { (descriptor.value.is_some() || descriptor.writable.is_some()) && old.accessor() };
                if changes_kind {
                    return Ok(false);
                }
                if old.accessor() {
                    if descriptor.get.as_ref().is_some_and(|v| !same_value(v, old.get.as_ref().unwrap()))
                        || descriptor.set.as_ref().is_some_and(|v| !same_value(v, old.set.as_ref().unwrap()))
                    {
                        return Ok(false);
                    }
                } else if old.writable == Some(false)
                    && (descriptor.writable == Some(true) || descriptor.value.as_ref().is_some_and(|v| !same_value(v, old.value.as_ref().unwrap())))
                {
                    return Ok(false);
                }
            }
        }
        let mut merged = old.clone().unwrap_or_else(|| PropertyDescriptor::data(Value::Undefined, false, false, false));
        if descriptor.accessor() && !merged.accessor() {
            merged.value = None;
            merged.writable = None;
            merged.get = Some(Value::Undefined);
            merged.set = Some(Value::Undefined);
        } else if merged.accessor() && (descriptor.value.is_some() || descriptor.writable.is_some()) {
            merged.get = None;
            merged.set = None;
            merged.value = Some(Value::Undefined);
            merged.writable = Some(false);
        }
        macro_rules! merge { ($($field:ident),*) => { $(if descriptor.$field.is_some() { merged.$field = descriptor.$field; })* }; }
        merge!(value, writable, get, set, enumerable, configurable);
        let obj = self.object(object)?;
        if matches!(&obj.kind, ObjectKind::String(s) if string_property(s, &key).is_some()) {
            return Ok(true);
        }
        let virtual_length = key == "length" && matches!(obj.kind, ObjectKind::Array { .. });
        let old_attributes = obj.attributes.get(&key).map_or(0, |d| attribute_bytes(&key, d));
        let old_property = obj.properties.get(&key).map_or(0, |v| property_bytes(&key, v));
        let value = merged.value.take().unwrap_or(Value::Undefined);
        let new_property = if virtual_length { 0 } else { property_bytes(&key, &value) };
        let new_attributes = attribute_bytes(&key, &merged);
        let protected: Vec<_> = std::iter::once(object).chain(value.object_id()).chain(merged.get.iter().chain(merged.set.iter()).filter_map(Value::object_id)).collect();
        for &id in &protected {
            self.object(id)?;
        }
        self.ensure_room((new_property + new_attributes).saturating_sub(old_property + old_attributes), &protected)?;
        let length_failed = if virtual_length {
            match self.set_array_length(object, value.clone()) {
                Ok(()) => false,
                Err(HeapError::ReadOnlyProperty) => true,
                Err(error) => return Err(error),
            }
        } else {
            false
        };
        for &target in protected.iter().skip(1) {
            self.write_barrier(object, Some(target));
        }
        let obj = self.objects.get_mut(&object).unwrap();
        if !virtual_length {
            if !obj.properties.contains_key(&key) {
                obj.order.push(key.clone());
            }
            if let ObjectKind::Array { length } = &mut obj.kind {
                if let Some(index) = array_index(&key) {
                    *length = (*length).max(index + 1);
                }
            }
            obj.properties.insert(key.clone(), value);
        }
        obj.attributes.insert(key, merged);
        obj.bytes = obj.bytes - old_property - old_attributes + new_property + new_attributes;
        self.managed_bytes = self.managed_bytes - old_property - old_attributes + new_property + new_attributes;
        Ok(!length_failed)
    }

    pub fn is_array(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(self.object(object)?.kind, ObjectKind::Array { .. }))
    }

    fn alloc(&mut self, kind: ObjectKind, prototype: Option<ObjectId>) -> Result<ObjectId, HeapError> {
        if let Some(id) = prototype {
            self.object(id)?;
        }
        let next = self.next_object.checked_add(1).ok_or(HeapError::IdExhausted)?;
        let protected: Vec<_> = prototype
            .into_iter()
            .chain(match &kind {
                ObjectKind::Closure { captures, this, .. } => captures.iter().copied().chain(this.object_id()).collect::<Vec<_>>(),
                ObjectKind::BoundFunction(bound) => std::iter::once(bound.target).chain(bound.this.object_id()).chain(bound.args.iter().filter_map(Value::object_id)).collect(),
                ObjectKind::Collator { compare, .. } => compare.iter().copied().collect(),
                ObjectKind::RegExpIterator { matcher, .. } => vec![*matcher],
                ObjectKind::ArrayIterator { object, .. } => vec![*object],
                _ => Vec::new(),
            })
            .collect();
        if self.nursery.len() >= self.config.nursery_capacity {
            self.minor_gc(&protected);
        }
        let bytes = OBJECT_BYTES
            + match &kind {
                ObjectKind::String(string) | ObjectKind::StringIterator { string, .. } | ObjectKind::RegExpIterator { string, .. } => string.byte_len(),
                ObjectKind::RegExp(regexp) => {
                    regexp.source.byte_len() + regexp.flags.len() + regexp.capture_names.iter().map(|(name, _)| name.len() + size_of::<(String, usize)>()).sum::<usize>()
                }
                ObjectKind::Collator { data, .. } => data.bytes(),
                ObjectKind::BoxedPrimitive(value) => value.payload_bytes(),
                ObjectKind::NativeFunction { initial_name, .. } => initial_name.byte_len(),
                ObjectKind::Closure { captures, this, .. } => captures.len() * size_of::<ObjectId>() + this.payload_bytes(),
                ObjectKind::BoundFunction(bound) => bound.this.payload_bytes() + bound.args.len() * size_of::<Value>() + bound.args.iter().map(Value::payload_bytes).sum::<usize>(),
                _ => 0,
            };
        self.ensure_room(bytes, &protected)?;
        let id = ObjectId { heap: self.identity, serial: self.next_object };
        self.next_object = next;
        self.objects.insert(id, Object { kind, properties: HashMap::new(), order: Vec::new(), attributes: HashMap::new(), extensible: true, prototype, young: true, bytes });
        self.nursery.push(id);
        self.managed_bytes += bytes;
        Ok(id)
    }

    pub fn contains(&self, id: ObjectId) -> bool {
        self.objects.contains_key(&id)
    }

    /// Registers an independent root. Does not collect. The caller is
    /// responsible for releasing it once the interpreter/host no longer
    /// needs this value; a copied ObjectId alone is not a GC root.
    pub fn root(&mut self, object: ObjectId) -> Result<RootId, HeapError> {
        self.object(object)?;
        let next = self.next_root.checked_add(1).ok_or(HeapError::IdExhausted)?;
        let id = RootId { heap: self.identity, serial: self.next_root };
        self.next_root = next;
        self.roots.insert(id, object);
        Ok(id)
    }

    /// Releases exactly this registration, without collecting immediately.
    pub fn unroot(&mut self, root: RootId) -> Result<ObjectId, HeapError> {
        self.roots.remove(&root).ok_or(HeapError::InvalidRoot(root))
    }

    /// `None` means absent, distinct from a present `Value::Undefined`.
    pub fn get_own(&self, object: ObjectId, key: impl Into<PropertyName>) -> Result<Option<Value>, HeapError> {
        Ok(self.object(object)?.own_property(&key.into()))
    }

    /// Ordinary data-property lookup through the prototype chain.
    pub fn get(&self, object: ObjectId, key: impl Into<PropertyName>) -> Result<Value, HeapError> {
        self.get_key(object, key.into())
    }

    fn get_key(&self, object: ObjectId, key: PropertyName) -> Result<Value, HeapError> {
        let mut current = Some(object);
        while let Some(id) = current {
            let obj = self.object(id)?;
            if let Some(value) = obj.own_property(&key) {
                return Ok(value);
            }
            current = obj.prototype;
        }
        Ok(Value::Undefined)
    }

    /// Creates or replaces an own data property. Inputs are protected
    /// across a pressure collection; a budget error leaves the property
    /// and insertion order unchanged. It may still reclaim unrelated garbage.
    /// Array length writes require a pre-coerced Number, validated as an
    /// integer in 0..=u32::MAX. Truncation visits present properties only;
    /// holes cost no storage, and successful index stores grow length.
    pub fn set(&mut self, object: ObjectId, key: impl Into<PropertyName>, value: Value) -> Result<(), HeapError> {
        self.set_key(object, key.into(), value)
    }

    fn set_key(&mut self, object: ObjectId, key: PropertyName, value: Value) -> Result<(), HeapError> {
        let obj = self.object(object)?;
        if obj.attributes.get(&key).is_some_and(|d| d.accessor() || d.writable == Some(false)) {
            return Err(HeapError::ReadOnlyProperty);
        }
        if !obj.extensible && obj.own_property(&key).is_none() {
            return Err(HeapError::ReadOnlyProperty);
        }
        if let ObjectKind::Array { length } = obj.kind {
            if array_index(&key).is_some_and(|index| index >= length) && obj.attributes.get(&"length".into()).is_some_and(|d| d.writable == Some(false)) {
                return Err(HeapError::ReadOnlyProperty);
            }
        }
        if matches!(&obj.kind, ObjectKind::String(string) if string_property(string, &key).is_some()) {
            return Err(HeapError::ReadOnlyProperty);
        }
        let old_bytes = obj.properties.get(&key).map_or(0, |old| property_bytes(&key, old));
        let value_id = value.object_id();
        if let Some(id) = value_id {
            self.object(id)?;
        }
        if key == "length" && matches!(obj.kind, ObjectKind::Array { .. }) {
            return self.set_array_length(object, value);
        }
        let new_bytes = property_bytes(&key, &value);
        let protected: Vec<_> = std::iter::once(object).chain(value_id).collect();
        self.ensure_room(new_bytes.saturating_sub(old_bytes), &protected)?;
        self.write_barrier(object, value_id);
        let obj = self.objects.get_mut(&object).expect("the receiver is protected across collection");
        if !obj.properties.contains_key(&key) {
            obj.order.push(key.clone());
        }
        if let ObjectKind::Array { length } = &mut obj.kind {
            if let Some(index) = array_index(&key) {
                *length = (*length).max(index + 1);
            }
        }
        obj.properties.insert(key, value);
        obj.bytes = obj.bytes - old_bytes + new_bytes;
        self.managed_bytes = self.managed_bytes - old_bytes + new_bytes;
        Ok(())
    }

    /// Deleting a missing property succeeds, as it does for JS ordinary
    /// objects. Array length and boxed String indices/length cannot be
    /// deleted; deleting an array index does not change length.
    pub fn delete(&mut self, object: ObjectId, key: impl Into<PropertyName>) -> Result<bool, HeapError> {
        self.delete_key(object, key.into())
    }

    fn delete_key(&mut self, object: ObjectId, key: PropertyName) -> Result<bool, HeapError> {
        let obj = self.objects.get_mut(&object).ok_or(HeapError::InvalidObject(object))?;
        if obj.attributes.get(&key).is_some_and(|d| d.configurable == Some(false)) {
            return Ok(false);
        }
        if matches!(&obj.kind, ObjectKind::String(string) if string_property(string, &key).is_some()) {
            return Ok(false);
        }
        if key == "length" && matches!(obj.kind, ObjectKind::Array { .. }) {
            return Ok(false);
        }
        if let Some(value) = obj.properties.remove(&key) {
            let bytes = property_bytes(&key, &value) + obj.attributes.remove(&key).map_or(0, |d| attribute_bytes(&key, &d));
            obj.order.retain(|name| name != &key);
            obj.bytes -= bytes;
            self.managed_bytes -= bytes;
        }
        Ok(true)
    }

    /// ECMAScript OrdinaryOwnPropertyKeys: array indices first, then other
    /// strings in creation order, then Symbols in creation order.
    /// Includes non-enumerable virtual length, created before stored strings.
    /// 2^32-1 and noncanonical spellings are not indices.
    pub fn own_property_keys(&self, object: ObjectId) -> Result<Vec<PropertyName>, HeapError> {
        let obj = self.object(object)?;
        let mut indices = Vec::new();
        let mut strings = Vec::new();
        let mut symbols = Vec::new();
        if let ObjectKind::String(string) = &obj.kind {
            indices.extend((0..string.len()).map(|index| (index, index.to_string().into())));
        }
        if matches!(obj.kind, ObjectKind::Array { .. } | ObjectKind::String(_)) {
            strings.push("length".into());
        }
        for key in &obj.order {
            if matches!(key, PropertyName::Symbol(_)) {
                symbols.push(key.clone());
                continue;
            }
            match array_index(key) {
                Some(index) => indices.push((index as usize, key.clone())),
                None => strings.push(key.clone()),
            }
        }
        indices.sort_unstable_by_key(|(index, _)| *index);
        Ok(indices.into_iter().map(|(_, key)| key).chain(strings).chain(symbols).collect())
    }

    pub fn own_keys(&self, object: ObjectId) -> Result<Vec<JsString>, HeapError> {
        Ok(self
            .own_property_keys(object)?
            .into_iter()
            .filter_map(|key| match key {
                PropertyName::String(s) => Some(s),
                _ => None,
            })
            .collect())
    }

    /// The enumerable subset of own_keys, excluding virtual array length.
    /// Inherited properties are not returned by either key enumeration API.
    pub fn enumerable_own_keys(&self, object: ObjectId) -> Result<Vec<JsString>, HeapError> {
        let virtual_length = matches!(self.object(object)?.kind, ObjectKind::Array { .. } | ObjectKind::String(_));
        Ok(self
            .own_keys(object)?
            .into_iter()
            .filter(|key| (!virtual_length || key != "length") && self.objects[&object].attributes.get(&PropertyName::from(key)).is_none_or(|d| d.enumerable == Some(true)))
            .collect())
    }

    fn set_array_length(&mut self, object: ObjectId, value: Value) -> Result<(), HeapError> {
        let Value::Number(number) = value else { return Err(HeapError::InvalidArrayLength) };
        if !(0.0..=f64::from(u32::MAX)).contains(&number) || number.fract() != 0.0 {
            return Err(HeapError::InvalidArrayLength);
        }
        let new_length = number as u32;
        let obj = self.objects.get_mut(&object).expect("validated array receiver");
        let ObjectKind::Array { length } = &mut obj.kind else { unreachable!("length dispatch checks object kind") };
        let old_length = *length;
        if new_length < old_length {
            let mut indices: Vec<_> = obj.order.iter().filter_map(|key| array_index(key).filter(|&index| index >= new_length).map(|index| (index, key.clone()))).collect();
            indices.sort_unstable_by_key(|entry| std::cmp::Reverse(entry.0));
            for (index, key) in indices {
                if !self.delete(object, &key)? {
                    if let ObjectKind::Array { length } = &mut self.objects.get_mut(&object).unwrap().kind {
                        *length = index + 1;
                    }
                    return Err(HeapError::ReadOnlyProperty);
                }
            }
        }
        if let ObjectKind::Array { length } = &mut self.objects.get_mut(&object).unwrap().kind {
            *length = new_length;
        }
        Ok(())
    }

    pub fn prototype(&self, object: ObjectId) -> Result<Option<ObjectId>, HeapError> {
        Ok(self.object(object)?.prototype)
    }

    pub fn prevent_extensions(&mut self, object: ObjectId) -> Result<(), HeapError> {
        self.objects.get_mut(&object).ok_or(HeapError::InvalidObject(object))?.extensible = false;
        Ok(())
    }

    pub fn is_extensible(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(self.object(object)?.extensible)
    }

    /// Changes `[[Prototype]]` without collecting. Cycles and foreign/stale
    /// handles are rejected before changing either the object or barrier.
    pub fn set_prototype(&mut self, object: ObjectId, prototype: Option<ObjectId>) -> Result<(), HeapError> {
        self.object(object)?;
        if !self.object(object)?.extensible && self.object(object)?.prototype != prototype {
            return Err(HeapError::ReadOnlyProperty);
        }
        let mut current = prototype;
        while let Some(id) = current {
            if id == object {
                return Err(HeapError::PrototypeCycle);
            }
            current = self.object(id)?.prototype;
        }
        self.write_barrier(object, prototype);
        self.objects.get_mut(&object).expect("validated receiver").prototype = prototype;
        Ok(())
    }

    pub fn stats(&self) -> HeapStats {
        HeapStats {
            nursery_objects: self.nursery.len(),
            tenured_objects: self.objects.len() - self.nursery.len(),
            managed_bytes: self.managed_bytes,
            next_major_bytes: self.next_major_bytes,
            minor_collections: self.minor_collections,
            major_collections: self.major_collections,
        }
    }

    /// Reclaims unreachable young objects and promotes every survivor.
    /// Old objects are not reclaimed here; their current young edges are
    /// conservatively retained until a major collection proves them dead.
    pub fn collect_minor(&mut self) {
        self.minor_gc(&[]);
    }

    /// Reclaims unreachable objects in both generations, including cycles.
    /// This is also the future process-pressure monitor's collection hook.
    pub fn collect_major(&mut self) {
        self.major_gc(&[]);
    }

    fn object(&self, id: ObjectId) -> Result<&Object, HeapError> {
        self.objects.get(&id).ok_or(HeapError::InvalidObject(id))
    }

    fn write_barrier(&mut self, owner: ObjectId, target: Option<ObjectId>) {
        if let Some(target) = target {
            if !self.objects[&owner].young && self.objects[&target].young {
                self.remembered.insert(owner);
            }
        }
    }

    fn ensure_room(&mut self, additional: usize, protected: &[ObjectId]) -> Result<(), HeapError> {
        if additional == 0 {
            return Ok(());
        }
        let proposed = self.managed_bytes.checked_add(additional);
        if proposed.is_none_or(|bytes| bytes >= self.next_major_bytes) {
            self.major_gc(protected);
        }
        if self.managed_bytes.checked_add(additional).is_none_or(|bytes| bytes > self.config.max_heap_bytes) {
            return Err(HeapError::HeapLimitExceeded { limit: self.config.max_heap_bytes });
        }
        Ok(())
    }

    // Mark and sweep are separate phases, with an explicit worklist so
    // a deep user-created graph never turns into recursive Rust calls.
    fn mark(&self, young_only: bool, protected: &[ObjectId]) -> HashSet<ObjectId> {
        let mut work: Vec<_> = self.roots.values().copied().chain(protected.iter().copied()).collect();
        if young_only {
            for id in &self.remembered {
                work.extend(self.objects[id].references());
            }
        }
        let mut marked = HashSet::new();
        while let Some(id) = work.pop() {
            let obj = &self.objects[&id];
            if (young_only && !obj.young) || !marked.insert(id) {
                continue;
            }
            work.extend(obj.references());
        }
        marked
    }

    fn minor_gc(&mut self, protected: &[ObjectId]) {
        let marked = self.mark(true, protected);
        for id in self.nursery.drain(..) {
            if marked.contains(&id) {
                self.objects.get_mut(&id).expect("nursery handle is live").young = false;
            } else {
                self.managed_bytes -= self.objects.remove(&id).expect("nursery handle is live").bytes;
            }
        }
        self.remembered.clear();
        self.minor_collections += 1;
    }

    fn major_gc(&mut self, protected: &[ObjectId]) {
        let marked = self.mark(false, protected);
        self.objects.retain(|id, obj| {
            if marked.contains(id) {
                obj.young = false;
                true
            } else {
                self.managed_bytes -= obj.bytes;
                false
            }
        });
        self.nursery.clear();
        self.remembered.clear();
        self.next_major_bytes = self.managed_bytes.saturating_mul(2).max(self.config.major_threshold_bytes).min(self.config.max_heap_bytes);
        self.major_collections += 1;
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
        (Value::Number(a), Value::Number(b)) => (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits(),
        _ => a == b,
    }
}

fn attribute_bytes(key: &PropertyName, descriptor: &PropertyDescriptor) -> usize {
    size_of::<(PropertyName, PropertyDescriptor)>() + key.byte_len() + descriptor.get.iter().chain(descriptor.set.iter()).map(Value::payload_bytes).sum::<usize>()
}
