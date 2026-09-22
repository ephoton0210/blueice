// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Handle-based objects, lexical environments, and BlueJS's two-generation
//! garbage collector.
//!
//! The nursery has a fixed entry limit. A collection traces from VM-supplied
//! roots, drops unreachable young entries, and promotes reachable ones to the
//! non-moving tenured generation. Tenured storage grows until its configurable
//! threshold, at which point the same complete trace drives mark-and-sweep.
//! `ObjectId` and `EnvironmentId` are monotonic handles, so promotion and
//! collection never create an ABA-style stale-reference hazard.

use crate::value::Value;
use std::collections::{HashMap, HashSet};
use std::fmt;

/// A stable handle to a JavaScript object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(u64);

impl ObjectId {
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// A stable handle to one lexical environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EnvironmentId(u64);

impl EnvironmentId {
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// How an object is interpreted. The property map is intentionally hidden by
/// [`ObjectAccess`], keeping a future shape/hidden-class implementation local
/// to this module.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectKind {
    Ordinary,
    Array {
        elements: Vec<Value>,
    },
    /// User function metadata; `code` indexes the bytecode module rather than
    /// embedding a Rust closure, so it is safe to trace and serialize in
    /// diagnostics. The interpreter owns the actual code table.
    Function {
        /// The owning compiled module. A realm may execute several classic
        /// `<script>` blocks, and closures from an earlier block must retain
        /// their original bytecode after a later block is compiled.
        module: u32,
        code: u32,
        environment: EnvironmentId,
        arity: usize,
    },
    /// A host callable selected by the interpreter. Keeping its identity as a
    /// small number ensures host functions use the same `Value::Object`
    /// calling convention as JavaScript functions.
    NativeFunction {
        id: u32,
        name: String,
        arity: usize,
    },
    /// A method lookup produces a callable paired with its receiver. This
    /// keeps `array.push(value)` and a bare `fn(value)` on one VM call path
    /// while retaining ordinary property lookup as the representation boundary.
    BoundNativeFunction {
        id: u32,
        name: String,
        arity: usize,
        receiver: Value,
    },
    /// Internal iterator state for `for...of`/`for...in`. It is a managed
    /// object rather than a Rust iterator so a suspended VM frame can retain
    /// it across collections without borrowing the heap.
    Iterator {
        values: Vec<Value>,
        next: usize,
    },
    /// A DOM identity that belongs to core. BlueJS stores only this opaque
    /// numeric handle; reading/mutating it always uses a host binding and IPC.
    HostObject {
        kind: HostObjectKind,
        id: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostObjectKind {
    Document,
    Node,
    /// `element.classList`, carrying only the node handle across the VM/DOM
    /// boundary. The class mutation itself remains core-owned.
    ClassList,
    /// `element.style`, likewise an opaque node-backed host proxy.
    Style,
}

#[derive(Debug, Clone, PartialEq)]
struct Object {
    kind: ObjectKind,
    properties: HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
struct Binding {
    value: Value,
    mutable: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Environment {
    parent: Option<EnvironmentId>,
    bindings: HashMap<String, Binding>,
}

#[derive(Debug, Default)]
struct Generation {
    objects: HashMap<ObjectId, Object>,
    environments: HashMap<EnvironmentId, Environment>,
}

impl Generation {
    fn len(&self) -> usize {
        self.objects.len() + self.environments.len()
    }
}

/// The limits used by the two generations. They are entry counts for this
/// first, portable implementation; a future allocator can retain this policy
/// API while measuring bytes instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeapLimits {
    pub nursery_entries: usize,
    pub tenured_entries: usize,
}

impl Default for HeapLimits {
    fn default() -> Self {
        Self {
            nursery_entries: 1_024,
            tenured_entries: 8_192,
        }
    }
}

/// Collection accounting exposed for deterministic tests and host diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeapStats {
    pub nursery_collections: u64,
    pub tenured_collections: u64,
    pub nursery_entries: usize,
    pub tenured_entries: usize,
}

/// Roots supplied by the interpreter at a GC-safe point. Values include the
/// operand stack and pending exception; environments include active frames and
/// the global environment.
#[derive(Debug, Clone, Copy)]
pub struct HeapRoots<'a> {
    pub values: &'a [Value],
    pub environments: &'a [EnvironmentId],
}

impl<'a> HeapRoots<'a> {
    pub const fn empty() -> Self {
        Self {
            values: &[],
            environments: &[],
        }
    }
}

/// A heap operation failed without mutating the heap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeapError {
    NurseryFull,
    UnknownObject(ObjectId),
    UnknownEnvironment(EnvironmentId),
    ImmutableBinding(String),
}

impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NurseryFull => {
                f.write_str("BlueJS nursery is full; collect at a VM safe point before allocating")
            }
            Self::UnknownObject(id) => write!(f, "unknown BlueJS object {}", id.as_u64()),
            Self::UnknownEnvironment(id) => write!(f, "unknown BlueJS environment {}", id.as_u64()),
            Self::ImmutableBinding(name) => write!(f, "cannot assign to immutable binding {name}"),
        }
    }
}

impl std::error::Error for HeapError {}

/// The only property-access interface used by the VM and host bindings. The
/// current representation is a `HashMap`; callers must not depend on that.
pub trait ObjectAccess {
    fn get_property(&self, object: ObjectId, key: &str) -> Result<Value, HeapError>;
    fn has_own_property(&self, object: ObjectId, key: &str) -> Result<bool, HeapError>;
    fn set_property(
        &mut self,
        object: ObjectId,
        key: String,
        value: Value,
    ) -> Result<(), HeapError>;
    fn own_property_keys(&self, object: ObjectId) -> Result<Vec<String>, HeapError>;
    fn object_kind(&self, object: ObjectId) -> Result<&ObjectKind, HeapError>;
}

/// BlueJS's managed storage. It is intentionally owned by the interpreter,
/// making every allocation point explicit and therefore a valid GC-safe point.
#[derive(Debug)]
pub struct Heap {
    limits: HeapLimits,
    nursery: Generation,
    tenured: Generation,
    next_object: u64,
    next_environment: u64,
    stats: HeapStats,
}

impl Default for Heap {
    fn default() -> Self {
        Self::new(HeapLimits::default())
    }
}

impl Heap {
    pub fn new(limits: HeapLimits) -> Self {
        assert!(
            limits.nursery_entries > 0,
            "a BlueJS nursery must have at least one entry"
        );
        assert!(
            limits.tenured_entries > 0,
            "the BlueJS tenured threshold must be positive"
        );
        Self {
            limits,
            nursery: Generation::default(),
            tenured: Generation::default(),
            next_object: 0,
            next_environment: 0,
            stats: HeapStats::default(),
        }
    }

    pub const fn limits(&self) -> HeapLimits {
        self.limits
    }

    pub fn stats(&self) -> HeapStats {
        HeapStats {
            nursery_entries: self.nursery.len(),
            tenured_entries: self.tenured.len(),
            ..self.stats
        }
    }

    /// Whether the caller must invoke [`Self::collect`] before another
    /// allocation. Allocation never runs a collection implicitly because only
    /// the VM knows its complete live root set.
    pub fn needs_collection(&self) -> bool {
        self.nursery.len() >= self.limits.nursery_entries
    }

    pub fn allocate_object(&mut self, kind: ObjectKind) -> Result<ObjectId, HeapError> {
        self.ensure_nursery_space()?;
        let id = ObjectId(self.next_object);
        self.next_object += 1;
        self.nursery.objects.insert(
            id,
            Object {
                kind,
                properties: HashMap::new(),
            },
        );
        Ok(id)
    }

    pub fn allocate_ordinary(&mut self) -> Result<ObjectId, HeapError> {
        self.allocate_object(ObjectKind::Ordinary)
    }

    pub fn allocate_array(&mut self, elements: Vec<Value>) -> Result<ObjectId, HeapError> {
        self.allocate_object(ObjectKind::Array { elements })
    }

    pub fn allocate_bound_native_function(
        &mut self,
        id: u32,
        name: String,
        arity: usize,
        receiver: Value,
    ) -> Result<ObjectId, HeapError> {
        self.allocate_object(ObjectKind::BoundNativeFunction {
            id,
            name,
            arity,
            receiver,
        })
    }

    pub fn allocate_iterator(&mut self, values: Vec<Value>) -> Result<ObjectId, HeapError> {
        self.allocate_object(ObjectKind::Iterator { values, next: 0 })
    }

    pub fn allocate_environment(
        &mut self,
        parent: Option<EnvironmentId>,
    ) -> Result<EnvironmentId, HeapError> {
        if let Some(parent) = parent {
            self.environment(parent)?;
        }
        self.ensure_nursery_space()?;
        let id = EnvironmentId(self.next_environment);
        self.next_environment += 1;
        self.nursery.environments.insert(
            id,
            Environment {
                parent,
                bindings: HashMap::new(),
            },
        );
        Ok(id)
    }

    pub fn define_binding(
        &mut self,
        environment: EnvironmentId,
        name: String,
        value: Value,
        mutable: bool,
    ) -> Result<(), HeapError> {
        self.environment_mut(environment)?
            .bindings
            .insert(name, Binding { value, mutable });
        Ok(())
    }

    pub fn get_binding(&self, environment: EnvironmentId, name: &str) -> Result<Value, HeapError> {
        let mut current = Some(environment);
        while let Some(id) = current {
            let environment = self.environment(id)?;
            if let Some(binding) = environment.bindings.get(name) {
                return Ok(binding.value.clone());
            }
            current = environment.parent;
        }
        Ok(Value::Undefined)
    }

    pub fn assign_binding(
        &mut self,
        environment: EnvironmentId,
        name: &str,
        value: Value,
    ) -> Result<(), HeapError> {
        let mut current = Some(environment);
        while let Some(id) = current {
            let parent = {
                let environment = self.environment_mut(id)?;
                if let Some(binding) = environment.bindings.get_mut(name) {
                    if !binding.mutable {
                        return Err(HeapError::ImmutableBinding(name.to_string()));
                    }
                    binding.value = value;
                    return Ok(());
                }
                environment.parent
            };
            current = parent;
        }
        // Sloppy-mode-shaped MVP semantics: an unresolved assignment creates a
        // global binding. The outermost environment is the global one.
        let global = self.global_environment(environment)?;
        self.define_binding(global, name.to_string(), value, true)
    }

    /// Performs a nursery collection and, if the tenured threshold is crossed,
    /// a full tenured mark-and-sweep. Reachable nursery entries are promoted;
    /// no live handle is rewritten.
    pub fn collect(&mut self, roots: HeapRoots<'_>) {
        let marks = self.trace(roots);
        self.promote_marked_nursery(&marks);
        self.stats.nursery_collections += 1;
        if self.tenured.len() >= self.limits.tenured_entries {
            self.sweep_tenured(&marks);
            self.stats.tenured_collections += 1;
        }
    }

    pub fn contains_object(&self, id: ObjectId) -> bool {
        self.nursery.objects.contains_key(&id) || self.tenured.objects.contains_key(&id)
    }

    pub fn contains_environment(&self, id: EnvironmentId) -> bool {
        self.nursery.environments.contains_key(&id) || self.tenured.environments.contains_key(&id)
    }

    pub fn environment_parent(
        &self,
        environment: EnvironmentId,
    ) -> Result<Option<EnvironmentId>, HeapError> {
        Ok(self.environment(environment)?.parent)
    }

    pub fn array_elements(&self, array: ObjectId) -> Result<&[Value], HeapError> {
        match &self.object(array)?.kind {
            ObjectKind::Array { elements } => Ok(elements),
            _ => Err(HeapError::UnknownObject(array)),
        }
    }

    pub fn array_push(&mut self, array: ObjectId, value: Value) -> Result<usize, HeapError> {
        match &mut self.object_mut(array)?.kind {
            ObjectKind::Array { elements } => {
                elements.push(value);
                Ok(elements.len())
            }
            _ => Err(HeapError::UnknownObject(array)),
        }
    }

    pub fn array_pop(&mut self, array: ObjectId) -> Result<Value, HeapError> {
        match &mut self.object_mut(array)?.kind {
            ObjectKind::Array { elements } => Ok(elements.pop().unwrap_or(Value::Undefined)),
            _ => Err(HeapError::UnknownObject(array)),
        }
    }

    /// Sets an indexed array element, filling any skipped slots with the MVP's
    /// explicit `undefined` representation. Indexed access stays in the array
    /// element storage rather than leaking through the ordinary-property map.
    pub fn array_set(
        &mut self,
        array: ObjectId,
        index: usize,
        value: Value,
    ) -> Result<(), HeapError> {
        match &mut self.object_mut(array)?.kind {
            ObjectKind::Array { elements } => {
                if index >= elements.len() {
                    elements.resize(index + 1, Value::Undefined);
                }
                elements[index] = value;
                Ok(())
            }
            _ => Err(HeapError::UnknownObject(array)),
        }
    }

    pub fn iterator_next(&mut self, iterator: ObjectId) -> Result<Option<Value>, HeapError> {
        match &mut self.object_mut(iterator)?.kind {
            ObjectKind::Iterator { values, next } => {
                let result = values.get(*next).cloned();
                if result.is_some() {
                    *next += 1;
                }
                Ok(result)
            }
            _ => Err(HeapError::UnknownObject(iterator)),
        }
    }

    fn ensure_nursery_space(&self) -> Result<(), HeapError> {
        if self.needs_collection() {
            Err(HeapError::NurseryFull)
        } else {
            Ok(())
        }
    }

    fn object(&self, id: ObjectId) -> Result<&Object, HeapError> {
        self.nursery
            .objects
            .get(&id)
            .or_else(|| self.tenured.objects.get(&id))
            .ok_or(HeapError::UnknownObject(id))
    }

    fn object_mut(&mut self, id: ObjectId) -> Result<&mut Object, HeapError> {
        if self.nursery.objects.contains_key(&id) {
            return self
                .nursery
                .objects
                .get_mut(&id)
                .ok_or(HeapError::UnknownObject(id));
        }
        self.tenured
            .objects
            .get_mut(&id)
            .ok_or(HeapError::UnknownObject(id))
    }

    fn environment(&self, id: EnvironmentId) -> Result<&Environment, HeapError> {
        self.nursery
            .environments
            .get(&id)
            .or_else(|| self.tenured.environments.get(&id))
            .ok_or(HeapError::UnknownEnvironment(id))
    }

    fn environment_mut(&mut self, id: EnvironmentId) -> Result<&mut Environment, HeapError> {
        if self.nursery.environments.contains_key(&id) {
            return self
                .nursery
                .environments
                .get_mut(&id)
                .ok_or(HeapError::UnknownEnvironment(id));
        }
        self.tenured
            .environments
            .get_mut(&id)
            .ok_or(HeapError::UnknownEnvironment(id))
    }

    fn global_environment(&self, environment: EnvironmentId) -> Result<EnvironmentId, HeapError> {
        let mut current = environment;
        while let Some(parent) = self.environment(current)?.parent {
            current = parent;
        }
        Ok(current)
    }

    fn trace(&self, roots: HeapRoots<'_>) -> Marks {
        let mut marks = Marks::default();
        let mut values = roots.values.to_vec();
        let mut environments = roots.environments.to_vec();
        while !values.is_empty() || !environments.is_empty() {
            while let Some(value) = values.pop() {
                if let Value::Object(id) = value {
                    self.mark_object(id, &mut marks, &mut values, &mut environments);
                }
            }
            while let Some(id) = environments.pop() {
                self.mark_environment(id, &mut marks, &mut values, &mut environments);
            }
        }
        marks
    }

    fn mark_object(
        &self,
        id: ObjectId,
        marks: &mut Marks,
        values: &mut Vec<Value>,
        environments: &mut Vec<EnvironmentId>,
    ) {
        if !marks.objects.insert(id) {
            return;
        }
        let Ok(object) = self.object(id) else {
            return;
        };
        values.extend(object.properties.values().cloned());
        match &object.kind {
            ObjectKind::Array { elements }
            | ObjectKind::Iterator {
                values: elements, ..
            } => values.extend(elements.iter().cloned()),
            ObjectKind::Function { environment, .. } => environments.push(*environment),
            ObjectKind::BoundNativeFunction { receiver, .. } => values.push(receiver.clone()),
            ObjectKind::Ordinary
            | ObjectKind::NativeFunction { .. }
            | ObjectKind::HostObject { .. } => {}
        }
    }

    fn mark_environment(
        &self,
        id: EnvironmentId,
        marks: &mut Marks,
        values: &mut Vec<Value>,
        environments: &mut Vec<EnvironmentId>,
    ) {
        if !marks.environments.insert(id) {
            return;
        }
        let Ok(environment) = self.environment(id) else {
            return;
        };
        values.extend(
            environment
                .bindings
                .values()
                .map(|binding| binding.value.clone()),
        );
        if let Some(parent) = environment.parent {
            environments.push(parent);
        }
    }

    fn promote_marked_nursery(&mut self, marks: &Marks) {
        self.nursery
            .objects
            .retain(|id, _| marks.objects.contains(id));
        self.nursery
            .environments
            .retain(|id, _| marks.environments.contains(id));
        self.tenured.objects.extend(self.nursery.objects.drain());
        self.tenured
            .environments
            .extend(self.nursery.environments.drain());
    }

    fn sweep_tenured(&mut self, marks: &Marks) {
        self.tenured
            .objects
            .retain(|id, _| marks.objects.contains(id));
        self.tenured
            .environments
            .retain(|id, _| marks.environments.contains(id));
    }
}

impl ObjectAccess for Heap {
    fn get_property(&self, object: ObjectId, key: &str) -> Result<Value, HeapError> {
        let object = self.object(object)?;
        if let ObjectKind::Array { elements } = &object.kind {
            if key == "length" {
                return Ok(Value::Number(elements.len() as f64));
            }
            if let Ok(index) = key.parse::<usize>() {
                return Ok(elements.get(index).cloned().unwrap_or(Value::Undefined));
            }
        }
        Ok(object
            .properties
            .get(key)
            .cloned()
            .unwrap_or(Value::Undefined))
    }

    fn has_own_property(&self, object: ObjectId, key: &str) -> Result<bool, HeapError> {
        let object = self.object(object)?;
        if let ObjectKind::Array { elements } = &object.kind {
            if key == "length" {
                return Ok(true);
            }
            if let Ok(index) = key.parse::<usize>() {
                return Ok(index < elements.len());
            }
        }
        Ok(object.properties.contains_key(key))
    }

    fn set_property(
        &mut self,
        object: ObjectId,
        key: String,
        value: Value,
    ) -> Result<(), HeapError> {
        let object = self.object_mut(object)?;
        if let ObjectKind::Array { elements } = &mut object.kind {
            if key == "length" {
                if let Value::Number(length) = value {
                    let length = length.max(0.0) as usize;
                    elements.resize(length, Value::Undefined);
                    return Ok(());
                }
            }
            if let Ok(index) = key.parse::<usize>() {
                if index >= elements.len() {
                    elements.resize(index + 1, Value::Undefined);
                }
                elements[index] = value;
                return Ok(());
            }
        }
        object.properties.insert(key, value);
        Ok(())
    }

    fn own_property_keys(&self, object: ObjectId) -> Result<Vec<String>, HeapError> {
        let object = self.object(object)?;
        let mut keys = object.properties.keys().cloned().collect::<Vec<_>>();
        if let ObjectKind::Array { elements } = &object.kind {
            keys.extend((0..elements.len()).map(|index| index.to_string()));
            keys.push("length".to_string());
        }
        keys.sort();
        keys.dedup();
        Ok(keys)
    }

    fn object_kind(&self, object: ObjectId) -> Result<&ObjectKind, HeapError> {
        Ok(&self.object(object)?.kind)
    }
}

#[derive(Debug, Default)]
struct Marks {
    objects: HashSet<ObjectId>,
    environments: HashSet<EnvironmentId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reachable_young_graphs_promote_without_changing_handles() {
        let mut heap = Heap::new(HeapLimits {
            nursery_entries: 3,
            tenured_entries: 10,
        });
        let global = heap.allocate_environment(None).unwrap();
        let parent = heap.allocate_ordinary().unwrap();
        let child = heap.allocate_array(vec![Value::Number(1.0)]).unwrap();
        heap.set_property(parent, "child".to_string(), Value::Object(child))
            .unwrap();
        heap.define_binding(global, "root".to_string(), Value::Object(parent), true)
            .unwrap();

        heap.collect(HeapRoots {
            values: &[],
            environments: &[global],
        });

        assert!(heap.contains_object(parent));
        assert!(heap.contains_object(child));
        assert_eq!(
            heap.get_property(parent, "child").unwrap(),
            Value::Object(child)
        );
        assert_eq!(heap.stats().nursery_entries, 0);
        assert_eq!(heap.stats().tenured_entries, 3);
    }

    #[test]
    fn unreachable_cycles_are_collected() {
        let mut heap = Heap::new(HeapLimits {
            nursery_entries: 2,
            tenured_entries: 2,
        });
        let a = heap.allocate_ordinary().unwrap();
        let b = heap.allocate_ordinary().unwrap();
        heap.set_property(a, "other".to_string(), Value::Object(b))
            .unwrap();
        heap.set_property(b, "other".to_string(), Value::Object(a))
            .unwrap();

        heap.collect(HeapRoots::empty());

        assert!(!heap.contains_object(a));
        assert!(!heap.contains_object(b));
        assert_eq!(heap.stats().tenured_entries, 0);
    }

    #[test]
    fn full_tenured_collection_reclaims_no_longer_rooted_entries() {
        let mut heap = Heap::new(HeapLimits {
            nursery_entries: 1,
            tenured_entries: 1,
        });
        let root = heap.allocate_ordinary().unwrap();
        heap.collect(HeapRoots {
            values: &[Value::Object(root)],
            environments: &[],
        });
        assert!(heap.contains_object(root));

        heap.collect(HeapRoots::empty());

        assert!(!heap.contains_object(root));
        assert_eq!(heap.stats().tenured_collections, 2);
    }

    #[test]
    fn lexical_assignment_walks_to_a_parent_and_const_is_immutable() {
        let mut heap = Heap::default();
        let global = heap.allocate_environment(None).unwrap();
        let child = heap.allocate_environment(Some(global)).unwrap();
        heap.define_binding(global, "outer".to_string(), Value::Number(1.0), true)
            .unwrap();
        heap.define_binding(global, "constant".to_string(), Value::Number(2.0), false)
            .unwrap();

        heap.assign_binding(child, "outer", Value::Number(3.0))
            .unwrap();

        assert_eq!(
            heap.get_binding(global, "outer").unwrap(),
            Value::Number(3.0)
        );
        assert_eq!(
            heap.assign_binding(child, "constant", Value::Number(4.0)),
            Err(HeapError::ImmutableBinding("constant".to_string()))
        );
    }

    #[test]
    fn array_properties_stay_behind_the_object_access_interface() {
        let mut heap = Heap::default();
        let array = heap
            .allocate_array(vec![Value::String("first".to_string())])
            .unwrap();

        heap.set_property(array, "2".to_string(), Value::String("third".to_string()))
            .unwrap();

        assert_eq!(
            heap.get_property(array, "length").unwrap(),
            Value::Number(3.0)
        );
        assert_eq!(heap.get_property(array, "1").unwrap(), Value::Undefined);
        assert_eq!(
            heap.own_property_keys(array).unwrap(),
            vec!["0", "1", "2", "length"]
        );
    }
}
