// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Realm-private object identity for embedding callbacks. The embedder
//! supplies only a private key; the VM allocates and roots the JavaScript
//! wrapper. Neither a heap object handle nor the key is serialized to script.

use super::*;

/// A fixed bound on permanently rooted wrappers in one host-object family.
/// The bound includes unreachable wrappers because retaining their identity
/// across collection is the first DOM-binding contract. Realm destruction
/// releases the entire table and its roots.
pub const MAX_HOST_OBJECTS_PER_FAMILY: usize = 4_096;

/// An embedder-owned identity scoped by a VM-owned host-object family. The
/// three private words can name an owner, generation, and object without
/// exposing any of them as JavaScript properties or callback arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostObjectKey {
    owner: u64,
    generation: u64,
    object: u64,
}

impl HostObjectKey {
    pub fn new(owner: u64, generation: u64, object: u64) -> Self {
        Self {
            owner,
            generation,
            object,
        }
    }
}

/// A family/prototype created in one VM. A copied handle is not an object or
/// GC root; the creating VM owns and roots its private prototype.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostObjectFamily {
    heap: u64,
    index: u32,
}

/// A synchronous primitive-input callback whose result is either `null` or
/// a private key. The VM, not the callback, creates the page-visible object.
pub trait HostObjectFactory: 'static {
    fn create(&mut self, args: &[HostValue]) -> Result<Option<HostObjectKey>, HostFunctionError>;
}

impl<F> HostObjectFactory for F
where
    F: for<'args> FnMut(&'args [HostValue]) -> Result<Option<HostObjectKey>, HostFunctionError>
        + 'static,
{
    fn create(&mut self, args: &[HostValue]) -> Result<Option<HostObjectKey>, HostFunctionError> {
        self(args)
    }
}

pub(super) struct HostObjectFamilyState {
    prototype: ObjectId,
    _prototype_root: RootId,
    wrappers: HashMap<HostObjectKey, (ObjectId, RootId)>,
}

pub(super) struct HostObjectFactoryRegistration {
    family_index: u32,
    factory: Box<dyn HostObjectFactory>,
}

impl Vm {
    /// Creates a realm-local prototype and identity table. The prototype is
    /// kept alive even before a wrapper exists; all roots die with the VM.
    pub fn create_host_object_family(&mut self) -> Result<HostObjectFamily, RuntimeError> {
        let index = u32::try_from(self.host_object_families.len())
            .map_err(|_| RuntimeError::RangeError("too many host-object families".into()))?;
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(prototype)?;
        self.host_object_families.push(HostObjectFamilyState {
            prototype,
            _prototype_root: root,
            wrappers: HashMap::new(),
        });
        Ok(HostObjectFamily {
            heap: self.object_prototype.heap,
            index,
        })
    }

    /// Installs one non-constructable global factory. Only private keys cross
    /// its callback boundary; a miss becomes JS `null`. Repeated keys in the
    /// same family return the identical rooted wrapper.
    pub fn install_host_object_factory(
        &mut self,
        name: &str,
        length: u32,
        family: HostObjectFamily,
        factory: impl HostObjectFactory,
    ) -> Result<(), RuntimeError> {
        if family.heap != self.object_prototype.heap
            || self
                .host_object_families
                .get(family.index as usize)
                .is_none()
        {
            return Err(RuntimeError::TypeError(
                "host-object family belongs to a different realm".into(),
            ));
        }
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("globalThis is always an object");
        if !host_property_name_is_valid(name) || self.heap.get_own(global, name)?.is_some() {
            return Err(RuntimeError::TypeError(
                "host global name is invalid or already defined".into(),
            ));
        }
        let index = u32::try_from(self.host_object_factories.len())
            .map_err(|_| RuntimeError::RangeError("too many host-object factories".into()))?;
        self.install_host_callable_native(
            global,
            name,
            length,
            NativeFunction::HostObjectFactory(index),
        )?;
        self.host_object_factories
            .push(HostObjectFactoryRegistration {
                family_index: family.index,
                factory: Box::new(factory),
            });
        Ok(())
    }

    pub(super) fn host_object_factory_call(
        &mut self,
        index: u32,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if construct {
            return Err(RuntimeError::TypeError(
                "host-object factories are not constructors".into(),
            ));
        }
        let args = args
            .iter()
            .map(HostValue::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| RuntimeError::TypeError(error.to_string()))?;
        let registration = self
            .host_object_factories
            .get_mut(index as usize)
            .ok_or_else(|| RuntimeError::TypeError("host-object factory is unavailable".into()))?;
        let family_index = registration.family_index as usize;
        let Some(key) = registration
            .factory
            .create(&args)
            .map_err(|error| RuntimeError::TypeError(error.to_string()))?
        else {
            return Ok(Value::Null);
        };
        let family = &self.host_object_families[family_index];
        if let Some(&(object, _root)) = family.wrappers.get(&key) {
            return Ok(Value::Object(object));
        }
        if family.wrappers.len() >= MAX_HOST_OBJECTS_PER_FAMILY {
            return Err(RuntimeError::RangeError(
                "host-object wrapper limit exceeded".into(),
            ));
        }
        let prototype = family.prototype;
        let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        let root = self.heap.root(object)?;
        self.host_object_families[family_index]
            .wrappers
            .insert(key, (object, root));
        Ok(Value::Object(object))
    }
}
