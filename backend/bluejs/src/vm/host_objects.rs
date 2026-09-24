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
/// A separate fixed cap on VM-rooted callback functions in one family.
pub const MAX_HOST_CLICK_LISTENERS_PER_FAMILY: usize = 4_096;

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

    /// Only the embedding host may inspect a private key. JavaScript sees no
    /// corresponding property, even when it can call a method on the wrapper.
    pub fn matches_owner(self, owner: u64, generation: u64) -> bool {
        self.owner == owner && self.generation == generation
    }

    pub fn object(self) -> u64 {
        self.object
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

/// A synchronous primitive-argument operation on a VM-verified wrapper.
/// The VM resolves the receiver to a private key before invoking the host;
/// ordinary objects, forged prototypes, and another family never enter it.
pub trait HostObjectMethod: 'static {
    fn call(
        &mut self,
        key: HostObjectKey,
        args: &[HostValue],
    ) -> Result<HostValue, HostFunctionError>;
}

impl<F> HostObjectMethod for F
where
    F: for<'args> FnMut(HostObjectKey, &'args [HostValue]) -> Result<HostValue, HostFunctionError>
        + 'static,
{
    fn call(
        &mut self,
        key: HostObjectKey,
        args: &[HostValue],
    ) -> Result<HostValue, HostFunctionError> {
        self(key, args)
    }
}

/// An operation on two exact wrappers from one realm-private family. The VM
/// resolves both opaque keys before calling the embedder and returns the
/// original child object only after the operation succeeds. No JS object ID
/// or arbitrary object crosses the callback boundary. The embedder must still
/// check that both keys name the same live owner/document generation.
pub trait HostObjectPairMethod: 'static {
    fn call(
        &mut self,
        parent: HostObjectKey,
        child: HostObjectKey,
    ) -> Result<(), HostFunctionError>;
}

impl<F> HostObjectPairMethod for F
where
    F: FnMut(HostObjectKey, HostObjectKey) -> Result<(), HostFunctionError> + 'static,
{
    fn call(
        &mut self,
        parent: HostObjectKey,
        child: HostObjectKey,
    ) -> Result<(), HostFunctionError> {
        self(parent, child)
    }
}

pub(super) struct HostObjectFamilyState {
    prototype: ObjectId,
    _prototype_root: RootId,
    wrappers: HashMap<HostObjectKey, (ObjectId, RootId)>,
    keys_by_wrapper: HashMap<ObjectId, HostObjectKey>,
}

pub(super) struct HostObjectFactoryRegistration {
    family_index: u32,
    required_receiver: Option<ObjectId>,
    factory: Box<dyn HostObjectFactory>,
}

pub(super) struct HostObjectMethodRegistration {
    family_index: u32,
    method: Box<dyn HostObjectMethod>,
}

pub(super) struct HostObjectPairMethodRegistration {
    family_index: u32,
    method: Box<dyn HostObjectPairMethod>,
}

pub(super) struct HostClickListener {
    family_index: u32,
    key: HostObjectKey,
    callback: ObjectId,
    root: RootId,
}

pub(super) struct ActiveHostClickEvent {
    object: ObjectId,
    default_prevented: bool,
}

impl Vm {
    /// Installs only the exact `click` add/remove listener pair on one private
    /// wrapper family. JavaScript callbacks stay in this VM's rooted table;
    /// neither a function object nor a numeric function handle reaches the
    /// embedding host or its IPC protocol.
    pub fn install_host_click_event_methods(
        &mut self,
        family: HostObjectFamily,
    ) -> Result<(), RuntimeError> {
        if family.heap != self.object_prototype.heap {
            return Err(RuntimeError::TypeError(
                "host-object family belongs to a different realm".into(),
            ));
        }
        let prototype = self
            .host_object_families
            .get(family.index as usize)
            .ok_or_else(|| RuntimeError::TypeError("host-object family is unavailable".into()))?
            .prototype;
        if self.heap.get_own(prototype, "addEventListener")?.is_some()
            || self
                .heap
                .get_own(prototype, "removeEventListener")?
                .is_some()
        {
            return Err(RuntimeError::TypeError(
                "host click listener methods are already installed".into(),
            ));
        }
        self.install_host_callable_native(
            prototype,
            "addEventListener",
            2,
            NativeFunction::HostClickListenerAdd(family.index),
        )?;
        self.install_host_callable_native(
            prototype,
            "removeEventListener",
            2,
            NativeFunction::HostClickListenerRemove(family.index),
        )
    }

    /// Dispatches one host-authorized click to an exact minted wrapper. The
    /// result says whether a synchronous listener called `preventDefault`;
    /// the host, not script, uses that result before any default navigation.
    /// Missing wrappers have no listeners. A copied family from another realm
    /// is rejected even when its private key happens to match.
    pub fn dispatch_host_click(
        &mut self,
        family: HostObjectFamily,
        key: HostObjectKey,
    ) -> Result<bool, RuntimeError> {
        self.ensure_no_debugger_continuation()?;
        if family.heap != self.object_prototype.heap {
            return Err(RuntimeError::TypeError(
                "host-object family belongs to a different realm".into(),
            ));
        }
        let Some(state) = self.host_object_families.get(family.index as usize) else {
            return Err(RuntimeError::TypeError(
                "host-object family is unavailable".into(),
            ));
        };
        let Some(&(wrapper, _)) = state.wrappers.get(&key) else {
            return Ok(false);
        };
        if self.active_host_click_event.is_some() {
            return Err(RuntimeError::TypeError(
                "host click dispatch is already active".into(),
            ));
        }
        let callbacks = self
            .host_click_listeners
            .iter()
            .filter(|listener| listener.family_index == family.index && listener.key == key)
            .map(|listener| listener.callback)
            .collect::<Vec<_>>();
        if callbacks.is_empty() {
            return Ok(false);
        }
        let mut snapshot_roots = Vec::with_capacity(callbacks.len());
        for callback in &callbacks {
            match self.heap.root(*callback) {
                Ok(root) => snapshot_roots.push(root),
                Err(error) => {
                    for root in snapshot_roots {
                        let _ = self.heap.unroot(root);
                    }
                    return Err(error.into());
                }
            }
        }
        let outcome = self.dispatch_rooted_host_click(family.index, key, wrapper, &callbacks);
        for root in snapshot_roots {
            self.heap.unroot(root)?;
        }
        outcome
    }

    fn dispatch_rooted_host_click(
        &mut self,
        family_index: u32,
        key: HostObjectKey,
        wrapper: ObjectId,
        callbacks: &[ObjectId],
    ) -> Result<bool, RuntimeError> {
        self.remaining_instructions = self.config.instruction_budget;
        let object_prototype = self.object_prototype;
        let event = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let event_root = self.heap.root(event)?;
        let outcome = (|| {
            self.define_data(
                event,
                "type",
                Value::String("click".into()),
                false,
                true,
                false,
            )?;
            self.define_data(event, "target", Value::Object(wrapper), false, true, false)?;
            self.define_data(
                event,
                "currentTarget",
                Value::Object(wrapper),
                false,
                true,
                false,
            )?;
            self.define_data(event, "cancelable", Value::Bool(true), false, true, false)?;
            self.install_host_callable_native(
                event,
                "preventDefault",
                0,
                NativeFunction::HostClickPreventDefault,
            )?;
            self.active_host_click_event = Some(ActiveHostClickEvent {
                object: event,
                default_prevented: false,
            });
            let result = (|| {
                for callback in callbacks {
                    if !self.host_click_listeners.iter().any(|listener| {
                        listener.family_index == family_index
                            && listener.key == key
                            && listener.callback == *callback
                    }) {
                        continue;
                    }
                    self.call_native(
                        Value::Object(*callback),
                        Value::Object(wrapper),
                        vec![Value::Object(event)],
                        false,
                    )?;
                }
                Ok(())
            })();
            let prevented = self
                .active_host_click_event
                .take()
                .expect("the active click event belongs to this dispatch")
                .default_prevented;
            result.map(|()| prevented)
        })();
        self.active_host_click_event = None;
        self.heap.unroot(event_root)?;
        outcome
    }

    pub(super) fn host_click_listener_call(
        &mut self,
        family_index: u32,
        receiver: Value,
        args: &[Value],
        construct: bool,
        add: bool,
    ) -> Result<Value, RuntimeError> {
        if construct || args.len() != 2 || args[0] != Value::String("click".into()) {
            return Err(RuntimeError::TypeError(
                "host listener method requires a click type and one callback".into(),
            ));
        }
        let family = self
            .host_object_families
            .get(family_index as usize)
            .ok_or_else(|| RuntimeError::TypeError("host-object family is unavailable".into()))?;
        let key = receiver
            .object_id()
            .and_then(|object| family.keys_by_wrapper.get(&object).copied())
            .ok_or_else(|| RuntimeError::TypeError("invalid host-object receiver".into()))?;
        if !self.is_callable(&args[1])? {
            return Err(RuntimeError::TypeError(
                "host click listener must be callable".into(),
            ));
        }
        let callback = args[1]
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("invalid host click listener".into()))?;
        let existing = self.host_click_listeners.iter().position(|listener| {
            listener.family_index == family_index
                && listener.key == key
                && listener.callback == callback
        });
        if add {
            if existing.is_none() {
                let count = self
                    .host_click_listeners
                    .iter()
                    .filter(|listener| listener.family_index == family_index)
                    .count();
                if count >= MAX_HOST_CLICK_LISTENERS_PER_FAMILY {
                    return Err(RuntimeError::RangeError(
                        "host click listener limit exceeded".into(),
                    ));
                }
                let root = self.heap.root(callback)?;
                self.host_click_listeners.push(HostClickListener {
                    family_index,
                    key,
                    callback,
                    root,
                });
            }
        } else if let Some(index) = existing {
            let listener = self.host_click_listeners.remove(index);
            self.heap.unroot(listener.root)?;
        }
        Ok(Value::Undefined)
    }

    pub(super) fn host_click_prevent_default(
        &mut self,
        receiver: Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if construct || !args.is_empty() {
            return Err(RuntimeError::TypeError(
                "preventDefault takes no arguments".into(),
            ));
        }
        let Some(active) = self.active_host_click_event.as_mut() else {
            return Err(RuntimeError::TypeError(
                "click event is no longer dispatching".into(),
            ));
        };
        if receiver.object_id() != Some(active.object) {
            return Err(RuntimeError::TypeError(
                "invalid click event receiver".into(),
            ));
        }
        active.default_prevented = true;
        Ok(Value::Undefined)
    }

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
            keys_by_wrapper: HashMap::new(),
        });
        Ok(HostObjectFamily {
            heap: self.object_prototype.heap,
            index,
        })
    }

    /// Installs a method on this family's private prototype. A JavaScript
    /// call reaches the embedder only if its receiver is an exact wrapper
    /// minted in this family and every argument remains primitive.
    pub fn install_host_object_method(
        &mut self,
        family: HostObjectFamily,
        name: &str,
        length: u32,
        method: impl HostObjectMethod,
    ) -> Result<(), RuntimeError> {
        if family.heap != self.object_prototype.heap || !host_property_name_is_valid(name) {
            return Err(RuntimeError::TypeError(
                "host-object family or method name is invalid".into(),
            ));
        }
        let prototype = self
            .host_object_families
            .get(family.index as usize)
            .ok_or_else(|| RuntimeError::TypeError("host-object family is unavailable".into()))?
            .prototype;
        if self.heap.get_own(prototype, name)?.is_some() {
            return Err(RuntimeError::TypeError(
                "host-object method is already defined".into(),
            ));
        }
        let index = u32::try_from(self.host_object_methods.len())
            .map_err(|_| RuntimeError::RangeError("too many host-object methods".into()))?;
        self.install_host_callable_native(
            prototype,
            name,
            length,
            NativeFunction::HostObjectMethod(index),
        )?;
        self.host_object_methods.push(HostObjectMethodRegistration {
            family_index: family.index,
            method: Box::new(method),
        });
        Ok(())
    }

    /// Installs a one-child operation on the family's private prototype.
    /// The receiver and sole argument must both be wrappers minted by this
    /// exact family; the callback sees their private keys only.
    pub fn install_host_object_pair_method(
        &mut self,
        family: HostObjectFamily,
        name: &str,
        length: u32,
        method: impl HostObjectPairMethod,
    ) -> Result<(), RuntimeError> {
        if family.heap != self.object_prototype.heap || !host_property_name_is_valid(name) {
            return Err(RuntimeError::TypeError(
                "host-object family or method name is invalid".into(),
            ));
        }
        let prototype = self
            .host_object_families
            .get(family.index as usize)
            .ok_or_else(|| RuntimeError::TypeError("host-object family is unavailable".into()))?
            .prototype;
        if self.heap.get_own(prototype, name)?.is_some() {
            return Err(RuntimeError::TypeError(
                "host-object method is already defined".into(),
            ));
        }
        let index = u32::try_from(self.host_object_pair_methods.len())
            .map_err(|_| RuntimeError::RangeError("too many host-object pair methods".into()))?;
        self.install_host_callable_native(
            prototype,
            name,
            length,
            NativeFunction::HostObjectPairMethod(index),
        )?;
        self.host_object_pair_methods
            .push(HostObjectPairMethodRegistration {
                family_index: family.index,
                method: Box::new(method),
            });
        Ok(())
    }

    /// Installs a genuine JavaScript accessor on the private wrapper
    /// prototype. Both native callbacks resolve the exact minted receiver
    /// before invoking a host getter or setter; no object argument crosses
    /// the primitive-only callback boundary.
    pub fn install_host_object_accessor(
        &mut self,
        family: HostObjectFamily,
        name: &str,
        getter: impl HostObjectMethod,
        setter: impl HostObjectMethod,
    ) -> Result<(), RuntimeError> {
        if family.heap != self.object_prototype.heap || !host_property_name_is_valid(name) {
            return Err(RuntimeError::TypeError(
                "host-object family or accessor name is invalid".into(),
            ));
        }
        let prototype = self
            .host_object_families
            .get(family.index as usize)
            .ok_or_else(|| RuntimeError::TypeError("host-object family is unavailable".into()))?
            .prototype;
        if self.heap.get_own(prototype, name)?.is_some() {
            return Err(RuntimeError::TypeError(
                "host-object accessor is already defined".into(),
            ));
        }
        let getter_index = u32::try_from(self.host_object_methods.len())
            .map_err(|_| RuntimeError::RangeError("too many host-object methods".into()))?;
        let setter_index = getter_index
            .checked_add(1)
            .ok_or_else(|| RuntimeError::RangeError("too many host-object methods".into()))?;
        let function_prototype = self.function_prototype()?;
        self.install_native_accessor(
            prototype,
            function_prototype,
            name,
            NativeFunction::HostObjectMethod(getter_index),
            NativeFunction::HostObjectMethod(setter_index),
        )?;
        self.host_object_methods.extend([
            HostObjectMethodRegistration {
                family_index: family.index,
                method: Box::new(getter),
            },
            HostObjectMethodRegistration {
                family_index: family.index,
                method: Box::new(setter),
            },
        ]);
        Ok(())
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
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("globalThis is always an object");
        self.install_host_object_factory_on(global, None, name, length, family, factory)
    }

    /// Installs a wrapper-returning method on an exact realm-owned object.
    /// Extracted calls with another `this` never enter the host callback.
    pub fn install_host_object_factory_method(
        &mut self,
        owner: HostObject,
        name: &str,
        length: u32,
        family: HostObjectFamily,
        factory: impl HostObjectFactory,
    ) -> Result<(), RuntimeError> {
        if owner.0.heap != self.object_prototype.heap {
            return Err(RuntimeError::TypeError(
                "host object belongs to a different realm".into(),
            ));
        }
        self.install_host_object_factory_on(owner.0, Some(owner.0), name, length, family, factory)
    }

    fn install_host_object_factory_on(
        &mut self,
        owner: ObjectId,
        required_receiver: Option<ObjectId>,
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
        if !host_property_name_is_valid(name) || self.heap.get_own(owner, name)?.is_some() {
            return Err(RuntimeError::TypeError(
                "host factory name is invalid or already defined".into(),
            ));
        }
        let index = u32::try_from(self.host_object_factories.len())
            .map_err(|_| RuntimeError::RangeError("too many host-object factories".into()))?;
        self.install_host_callable_native(
            owner,
            name,
            length,
            NativeFunction::HostObjectFactory(index),
        )?;
        self.host_object_factories
            .push(HostObjectFactoryRegistration {
                family_index: family.index,
                required_receiver,
                factory: Box::new(factory),
            });
        Ok(())
    }

    pub(super) fn host_object_factory_call(
        &mut self,
        index: u32,
        receiver: Value,
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
        if registration
            .required_receiver
            .is_some_and(|required| receiver.object_id() != Some(required))
        {
            return Err(RuntimeError::TypeError(
                "invalid host-object factory receiver".into(),
            ));
        }
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
        self.host_object_families[family_index]
            .keys_by_wrapper
            .insert(object, key);
        Ok(Value::Object(object))
    }

    pub(super) fn host_object_method_call(
        &mut self,
        index: u32,
        receiver: Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if construct {
            return Err(RuntimeError::TypeError(
                "host-object methods are not constructors".into(),
            ));
        }
        let registration = self
            .host_object_methods
            .get(index as usize)
            .ok_or_else(|| RuntimeError::TypeError("host-object method is unavailable".into()))?;
        let key = receiver
            .object_id()
            .and_then(|object| {
                self.host_object_families[registration.family_index as usize]
                    .keys_by_wrapper
                    .get(&object)
                    .copied()
            })
            .ok_or_else(|| RuntimeError::TypeError("invalid host-object receiver".into()))?;
        let args = args
            .iter()
            .map(HostValue::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| RuntimeError::TypeError(error.to_string()))?;
        let result: Value = self.host_object_methods[index as usize]
            .method
            .call(key, &args)
            .map_err(|error| RuntimeError::TypeError(error.to_string()))?
            .into();
        self.check_string(&result)?;
        Ok(result)
    }

    pub(super) fn host_object_pair_method_call(
        &mut self,
        index: u32,
        receiver: Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if construct || args.len() != 1 {
            return Err(RuntimeError::TypeError(
                "host-object pair method requires one child".into(),
            ));
        }
        let registration = self
            .host_object_pair_methods
            .get(index as usize)
            .ok_or_else(|| {
                RuntimeError::TypeError("host-object pair method is unavailable".into())
            })?;
        let family = &self.host_object_families[registration.family_index as usize];
        let parent = receiver
            .object_id()
            .and_then(|object| family.keys_by_wrapper.get(&object).copied())
            .ok_or_else(|| RuntimeError::TypeError("invalid host-object receiver".into()))?;
        let child_object = args[0]
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("invalid host-object child".into()))?;
        let child = family
            .keys_by_wrapper
            .get(&child_object)
            .copied()
            .ok_or_else(|| RuntimeError::TypeError("invalid host-object child".into()))?;
        self.host_object_pair_methods[index as usize]
            .method
            .call(parent, child)
            .map_err(|error| RuntimeError::TypeError(error.to_string()))?;
        Ok(Value::Object(child_object))
    }
}
