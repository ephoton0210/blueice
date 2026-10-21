// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Realm-private object identity for embedding callbacks. The embedder
//! supplies only a private key; the VM allocates and roots the JavaScript
//! wrapper. Neither a heap object handle nor the key is serialized to script.

use super::*;
mod drag_events;
mod drag_storage;
#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_drag_storage.rs"]
mod drag_storage_tests;
mod family_storage;
mod file_selection_events;
mod listener_storage;
mod node_events;
pub(super) use node_events::HostEventContext;
mod registration_storage;
use super::argument_list_storage::ArgumentValues;
use super::native_value_storage::NativeValue;
use family_storage::FamilyContext;
pub(super) use family_storage::HostFamilies;
pub(super) use listener_storage::HostListeners;
use listener_storage::{ListenerContext, ListenerSnapshot};
pub(super) use registration_storage::{HostRegistrations, RegistrationRecord};
#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_event_lifecycle.rs"]
mod event_lifecycle_tests;
#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_family_storage.rs"]
mod family_ownership_tests;
#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_function_storage.rs"]
mod function_ownership_tests;
#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_marshalling_storage.rs"]
mod marshalling_ownership_tests;
#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_listener_storage.rs"]
mod ownership_tests;
#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_registration_storage.rs"]
mod registration_ownership_tests;
pub use drag_events::{
    HostDragEventData, HostDragEventKind, HostDragEventOutcome, HostDragImage, HostDragItem,
};
pub use file_selection_events::HostFileSelectionEvent;

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
    wrappers: MembraneMap<HostObjectKey, (ObjectId, RootId)>,
    keys_by_wrapper: MembraneMap<ObjectId, HostObjectKey>,
    event_target: bool,
    file_events: bool,
    drag_events: bool,
    element_validator: Option<Box<dyn Fn(HostObjectKey) -> bool>>,
    event_path: Option<Box<NodeEventPath>>,
    // Both callback allocations drop before their native allocation credit.
    callback_storage: Option<host_scheduling::SharedSchedulingCharge>,
}

type NodeEventPath = dyn FnMut(HostObjectKey) -> Result<Vec<HostObjectKey>, HostFunctionError>;

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
    serial: u64,
    family_index: u32,
    key: HostObjectKey,
    callback: ObjectId,
    root: RootId,
    signal: Option<(ObjectId, RootId)>,
    selection: Option<HostFileSelectionEvent>,
    drag: Option<HostDragEventKind>,
    capture: bool,
    once: bool,
    passive: bool,
    event_type: JsString,
    type_root: Option<RootId>,
}

pub(super) struct ActiveHostClickEvent {
    object: ObjectId,
    default_prevented: bool,
    propagation_stopped: bool,
    immediate_stopped: bool,
    passive: bool,
}

fn listener_phases(length: usize) -> impl Iterator<Item = (usize, bool)> {
    (0..length)
        .rev()
        .map(|index| (index, true))
        .chain((0..length).map(|index| (index, false)))
}

impl Vm {
    /// The embedding owner classifies opaque DOM identities; page properties
    /// cannot impersonate an Element or reclassify a text/document wrapper.
    pub fn install_host_element_validator(
        &mut self,
        family: HostObjectFamily,
        validator: impl Fn(HostObjectKey) -> bool + 'static,
    ) -> Result<(), RuntimeError> {
        self.host_family_prototype(family)?;
        let context = FamilyContext::capture(self)?;
        let charge = context
            .owner()
            .map(|owner| owner.reserve_peer(std::mem::size_of_val(&validator)))
            .transpose()?;
        let callback = Box::new(validator);
        let state = &mut self.host_object_families[family.index as usize];
        let old = state.element_validator.replace(callback);
        let bytes = old
            .as_ref()
            .map_or(0, |callback| std::mem::size_of_val(&**callback));
        drop(old);
        state.replace_callback_charge(bytes, charge);
        Ok(())
    }
    pub(super) fn is_host_element(&self, object: ObjectId) -> bool {
        if self.argument_list_owner().is_err() || self.host_object_families.owner_view().is_err() {
            return false;
        }
        self.host_object_families.iter().any(|family| {
            family.keys_by_wrapper.get(&object).is_some_and(|key| {
                family
                    .element_validator
                    .as_ref()
                    .is_some_and(|check| check(*key))
            })
        })
    }
    pub(super) fn is_host_event_target(&self, object: ObjectId) -> bool {
        self.heap.web_event_target(object).is_ok()
            || self
                .host_object_families
                .iter()
                .any(|family| family.event_target && family.keys_by_wrapper.contains_key(&object))
    }
    /// Installs one minted wrapper as a global while preserving its family
    /// identity. Its opaque owner key remains inaccessible to JavaScript.
    pub fn install_host_family_object(
        &mut self,
        name: &str,
        family: HostObjectFamily,
        key: HostObjectKey,
    ) -> Result<HostObject, RuntimeError> {
        self.host_family_prototype(family)?;
        let global = self
            .global("globalThis")?
            .object_id()
            .expect("global object");
        if !host_property_name_is_valid(name) || self.heap.get_own(global, name)?.is_some() {
            return Err(RuntimeError::TypeError(
                "Host global already defined".into(),
            ));
        }
        let object = self.mint_host_wrapper(family.index as usize, key)?;
        self.define_data(global, name, Value::Object(object), false, false, false)?;
        Ok(HostObject(object))
    }

    pub(super) fn host_family_prototype(
        &self,
        family: HostObjectFamily,
    ) -> Result<ObjectId, RuntimeError> {
        if family.heap != self.object_prototype.heap {
            return Err(RuntimeError::TypeError(
                "Host family belongs to another realm".into(),
            ));
        }
        self.argument_list_owner()?;
        self.host_object_families.owner_view()?;
        self.host_object_families
            .get(family.index as usize)
            .map(|state| state.prototype)
            .ok_or_else(|| RuntimeError::TypeError("Host family is unavailable".into()))
    }

    pub(super) fn host_receiver_key(
        &self,
        family: HostObjectFamily,
        receiver: &Value,
    ) -> Result<HostObjectKey, RuntimeError> {
        self.host_family_prototype(family)?;
        receiver
            .object_id()
            .and_then(|id| {
                self.host_object_families[family.index as usize]
                    .keys_by_wrapper
                    .get(&id)
                    .copied()
            })
            .ok_or_else(|| RuntimeError::TypeError("Invalid host object receiver".into()))
    }

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
            .expect("this opaque same-heap family belongs to the append-only VM registry")
            .prototype;
        if self
            .heap
            .get_own(prototype, "addEventListener")
            .expect("the family retains its ordinary prototype root")
            .is_some()
            || self
                .heap
                .get_own(prototype, "removeEventListener")
                .expect("the family retains its ordinary prototype root")
                .is_some()
        {
            return Err(RuntimeError::TypeError(
                "host click listener methods are already installed".into(),
            ));
        }
        self.function_prototype()?;
        self.install_host_callable_native(
            prototype,
            "addEventListener",
            2,
            NativeFunction::HostClickListenerAdd(family.index),
        )?;
        let result = self.install_host_callable_native(
            prototype,
            "removeEventListener",
            2,
            NativeFunction::HostClickListenerRemove(family.index),
        );
        if result.is_err() {
            // Both own methods are configurable and no script runs during
            // installation. Roll back the first publication on second refusal,
            // allowing the same family to retry the complete pair.
            assert!(self
                .heap
                .delete(prototype, "addEventListener")
                .expect("the retained ordinary prototype owns a configurable method"));
        }
        result?;
        self.host_object_families[family.index as usize].event_target = true;
        Ok(())
    }

    /// Dispatches one host-authorized click to an exact minted wrapper. The
    /// result says whether a listener called `preventDefault`;
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
        if state.event_path.is_some() {
            if self.active_host_click_event.is_some() || self.active_host_drag_event.is_some() {
                return Err(RuntimeError::TypeError(
                    "An event is already dispatching".into(),
                ));
            }
            let path = self.host_node_event_path(family.index, key)?;
            self.remaining_instructions = self.config.instruction_budget;
            let result =
                self.dispatch_native_node_event(family.index, &path, "click", true, true)?;
            return Ok(result == Value::Bool(false));
        }
        let Some(&(wrapper, _)) = state.wrappers.get(&key) else {
            return Ok(false);
        };
        if self.active_host_click_event.is_some() || self.active_host_drag_event.is_some() {
            return Err(RuntimeError::TypeError(
                "host click dispatch is already active".into(),
            ));
        }
        let callbacks = self.listener_snapshot(
            |listener| {
                listener.family_index == family.index
                    && listener.key == key
                    && listener.selection.is_none()
                    && listener.drag.is_none()
                    && listener.event_type == "click"
            },
            true,
        )?;
        if callbacks.serials.is_empty() {
            return Ok(false);
        }
        self.dispatch_rooted_host_click(wrapper, &callbacks)
    }

    fn dispatch_rooted_host_click(
        &mut self,
        wrapper: ObjectId,
        callbacks: &ListenerSnapshot,
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
                propagation_stopped: false,
                immediate_stopped: false,
                passive: false,
            });
            let mut result = Ok(());
            for serial in callbacks.serials.iter().copied() {
                callbacks.validate(self)?;
                let called = self.invoke_host_listener(serial, wrapper, event);
                callbacks.validate(self)?;
                if let Err(error) = called {
                    // An uncaught page exception ends this callback, not the
                    // event task. The source-free child reply still carries
                    // only the cancellation bit. Resource failures remain
                    // fatal so the host does not apply a default action after
                    // partially exhausted execution.
                    if !matches!(
                        error,
                        RuntimeError::Thrown(_)
                            | RuntimeError::ReferenceError(_)
                            | RuntimeError::TypeError(_)
                            | RuntimeError::RangeError(_)
                            | RuntimeError::SyntaxError(_)
                    ) {
                        result = Err(error);
                        break;
                    }
                }
            }
            let prevented = self
                .active_host_click_event
                .take()
                .expect("the active click event belongs to this dispatch")
                .default_prevented;
            result.map(|()| prevented)
        })();
        self.active_host_click_event = None;
        self.heap
            .unroot(event_root)
            .expect("dispatch exclusively owns its event root");
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
        if construct || args.len() < 2 {
            return Err(RuntimeError::TypeError(
                "host listener requires a type and callback".into(),
            ));
        }
        let context = ListenerContext::capture(self)?;
        let objects = std::iter::once(&receiver)
            .chain(args)
            .filter_map(Value::object_id);
        let roots =
            realm_call_storage::RealmCallRoots::new(objects.clone().count(), context.owner())?;
        for object in objects {
            roots.push(&self.heap, object)?;
        }
        self.with_realm_call_roots(roots, |vm| {
            let result = vm.host_click_listener_input(family_index, receiver, args, add, &context);
            context.validate(vm)?;
            result
        })
    }

    fn host_click_listener_input(
        &mut self,
        family_index: u32,
        receiver: Value,
        args: &[Value],
        add: bool,
        context: &ListenerContext,
    ) -> Result<Value, RuntimeError> {
        let family = self
            .host_object_families
            .get(family_index as usize)
            .ok_or_else(|| RuntimeError::TypeError("host-object family is unavailable".into()))?;
        let key = receiver
            .object_id()
            .and_then(|object| family.keys_by_wrapper.get(&object).copied())
            .ok_or_else(|| RuntimeError::TypeError("invalid host-object receiver".into()))?;
        let (file_events, drag_events, node_events) = (
            family.file_events,
            family.drag_events,
            family.event_path.is_some(),
        );
        let name = if matches!(&args[0], Value::String(_)) {
            NativeValue::copied(&args[0], context.owner())?
        } else {
            let converted = self.coerce_string(&args[0]);
            context.validate(self)?;
            NativeValue::adopt(Value::String(converted?), context.owner())?
        };
        context.validate(self)?;
        let Value::String(name_value) = name.value() else {
            unreachable!("coerced event name")
        };
        let event_type = name.value();
        let (selection, drag) = if name_value == "click" {
            (None, None)
        } else if drag_events && HostDragEventKind::from_value(event_type).is_some() {
            (None, HostDragEventKind::from_value(event_type))
        } else if file_events && HostFileSelectionEvent::from_value(event_type).is_ok() {
            (Some(HostFileSelectionEvent::from_value(event_type)?), None)
        } else if node_events {
            (None, None)
        } else {
            return Err(RuntimeError::TypeError(
                "host listener requires an admitted event type".into(),
            ));
        };
        let callback =
            if args[1] == Value::Null || args[1] == Value::Undefined {
                None
            } else {
                Some(args[1].object_id().ok_or_else(|| {
                    RuntimeError::TypeError("EventListener must be an object".into())
                })?)
            };
        let options = native::argument(args, 2);
        let (mut capture, mut once, mut passive) = (false, false, false);
        let mut signal = None;
        if options.object_id().is_some() {
            let value = self.get_property(options, &"capture".into());
            context.validate(self)?;
            let value = value?;
            capture = self.to_boolean(&value)?;
            if add {
                let value = self.get_property(options, &"once".into());
                context.validate(self)?;
                let value = value?;
                once = self.to_boolean(&value)?;
                let value = self.get_property(options, &"passive".into());
                context.validate(self)?;
                let value = value?;
                passive = self.to_boolean(&value)?;
                let value = self.get_property(options, &"signal".into());
                context.validate(self)?;
                let value = value?;
                signal = self.abort_signal_argument(&value)?;
            }
        } else if *options != Value::Null && *options != Value::Undefined {
            capture = self.to_boolean(options)?;
        }
        let Some(callback) = callback else {
            return Ok(Value::Undefined);
        };
        if signal.is_some_and(|signal| {
            self.heap
                .abort_signal(signal)
                .is_ok_and(|state| state.aborted())
        }) {
            return Ok(Value::Undefined);
        }
        let existing = self.host_click_listeners.iter().position(|listener| {
            listener.family_index == family_index
                && listener.key == key
                && listener.callback == callback
                && listener.selection == selection
                && listener.drag == drag
                && listener.capture == capture
                && listener.event_type == *name_value
        });
        if add {
            if existing.is_none() {
                let count = self
                    .host_click_listeners
                    .iter()
                    .filter(|listener| listener.family_index == family_index)
                    .count();
                if count >= MAX_HOST_CLICK_LISTENERS_PER_FAMILY
                    || signal.is_some_and(|signal| {
                        self.heap
                            .abort_signal(signal)
                            .is_ok_and(|state| state.hooks.len() >= 4096)
                    })
                {
                    return Err(RuntimeError::RangeError(
                        "host click listener limit exceeded".into(),
                    ));
                }
                let serial = self
                    .next_host_listener_serial
                    .checked_add(1)
                    .ok_or_else(|| {
                        RuntimeError::RangeError("listener identity exhausted".into())
                    })?;
                context.validate(self)?;
                let current = self.argument_list_owner()?;
                let prepared = self
                    .host_click_listeners
                    .prepare(context.owner().or(current.as_ref()))?;
                let name = name.retain_for_owner(prepared.owner())?;
                let root = self.heap.root(callback)?;
                let signal_root = match signal.map(|signal| self.heap.root(signal)).transpose() {
                    Ok(root) => root,
                    Err(error) => {
                        self.heap.unroot(root)?;
                        return Err(error.into());
                    }
                };
                let type_root = if node_events {
                    match self.root_host_event_type(&name) {
                        Ok(root) => Some(root),
                        Err(error) => {
                            self.heap.unroot(root)?;
                            if let Some(root) = signal_root {
                                self.heap.unroot(root)?;
                            }
                            return Err(error);
                        }
                    }
                } else {
                    None
                };
                let (name, name_storage) = name.into_retained_parts();
                let Value::String(name) = name else {
                    unreachable!("coerced event name")
                };
                self.host_click_listeners.commit(
                    prepared,
                    HostClickListener {
                        serial,
                        family_index,
                        key,
                        callback,
                        root,
                        signal: signal.zip(signal_root),
                        selection,
                        drag,
                        capture,
                        once,
                        passive,
                        event_type: name,
                        type_root,
                    },
                    name_storage,
                );
                self.next_host_listener_serial = serial;
                if let Some(signal) = signal {
                    if let Err(error) =
                        self.attach_abort_hook(signal, crate::heap::AbortHook::Host { serial })
                    {
                        self.remove_host_listener_serial(serial)?;
                        return Err(error);
                    }
                }
            }
        } else if let Some(index) = existing {
            let serial = self.host_click_listeners[index].serial;
            self.remove_host_listener_serial(serial)?;
        }
        Ok(Value::Undefined)
    }

    pub(super) fn remove_host_listener_serial(&mut self, serial: u64) -> Result<(), RuntimeError> {
        if let Some(index) = self
            .host_click_listeners
            .iter()
            .position(|listener| listener.serial == serial)
        {
            let listener = self.host_click_listeners.remove(index);
            self.heap.unroot(listener.root)?;
            if let Some(root) = listener.type_root {
                self.heap.unroot(root)?;
            }
            if let Some((signal, root)) = listener.signal {
                let detached =
                    self.detach_abort_hook(signal, crate::heap::AbortHook::Host { serial });
                self.heap.unroot(root)?;
                detached?;
            }
        }
        Ok(())
    }

    fn invoke_host_listener(
        &mut self,
        serial: u64,
        current: ObjectId,
        event: ObjectId,
    ) -> Result<Value, RuntimeError> {
        let context = ListenerContext::capture(self)?;
        let Some((callback, once, passive)) = self
            .host_click_listeners
            .iter()
            .find(|listener| listener.serial == serial)
            .map(|listener| (listener.callback, listener.once, listener.passive))
        else {
            return Ok(Value::Undefined);
        };
        let roots = realm_call_storage::RealmCallRoots::new(3, context.owner())?;
        for object in [callback, current, event] {
            roots.push(&self.heap, object)?;
        }
        // Prepare callback arguments and roots before consuming a once listener.
        let arguments = ArgumentValues::single(self, Value::Object(event), context.owner())?;
        self.with_realm_call_roots(roots, |vm| {
            if once {
                vm.remove_host_listener_serial(serial)?;
            }
            if vm.heap.drag_event(event).is_ok() {
                vm.heap.set_event_passive(event, passive)?;
            } else if let Some(active) = vm.active_host_click_event.as_mut() {
                active.passive = passive;
            }
            let result = (|| {
                let callback = Value::Object(callback);
                if vm.is_callable(&callback)? {
                    arguments.call_native(vm, callback, Value::Object(current), false)
                } else {
                    let function = vm.get_property(&callback, &"handleEvent".into());
                    context.validate(vm)?;
                    let function = NativeValue::adopt(function?, context.owner())?;
                    function.with_value(vm, |vm, function| {
                        arguments.call_native_borrowed(vm, function, &callback)
                    })
                }
            })();
            let result = context.validate(vm).and(result);
            let result = super::web_event_storage::EventCompletion::adopt(result, context.owner())
                .and_then(|completion| completion.cleanup(vm))
                .and_then(super::web_event_storage::EventCompletion::into_result);
            if vm.heap.drag_event(event).is_ok() {
                vm.heap.set_event_passive(event, false)?;
            } else if let Some(active) = vm.active_host_click_event.as_mut() {
                active.passive = false;
            }
            context.validate(vm)?;
            result
        })
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
        if !active.passive {
            active.default_prevented = true;
        }
        Ok(Value::Undefined)
    }

    /// Creates a realm-local prototype and identity table. The prototype is
    /// kept alive even before a wrapper exists; all roots die with the VM.
    pub fn create_host_object_family(&mut self) -> Result<HostObjectFamily, RuntimeError> {
        let context = FamilyContext::capture(self)?;
        let index = u32::try_from(self.host_object_families.len())
            .map_err(|_| RuntimeError::RangeError("too many host-object families".into()))?;
        let prepared = self.host_object_families.prepare(context.owner())?;
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(prototype)?;
        self.host_object_families.commit(
            prepared,
            HostObjectFamilyState {
                prototype,
                _prototype_root: root,
                wrappers: MembraneMap::default(),
                keys_by_wrapper: MembraneMap::default(),
                event_target: false,
                file_events: false,
                drag_events: false,
                element_validator: None,
                event_path: None,
                callback_storage: None,
            },
        );
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
        let prototype = self.host_family_prototype(family)?;
        if self
            .heap
            .get_own(prototype, name)
            .expect("the family retains its ordinary prototype root")
            .is_some()
        {
            return Err(RuntimeError::TypeError(
                "host-object method is already defined".into(),
            ));
        }
        let context = FamilyContext::capture(self)?;
        let index = host_registration_index(
            self.host_object_methods.len(),
            0,
            "too many host-object methods",
        )?;
        let prepared = self.host_object_methods.prepare(1, context.owner())?;
        let callbacks = prepared.callbacks(&[std::mem::size_of_val(&method)], || {
            [HostObjectMethodRegistration {
                family_index: family.index,
                method: Box::new(method),
            }]
        })?;
        self.function_prototype()?;
        context.validate(self)?;
        self.install_host_callable_native(
            prototype,
            name,
            length,
            NativeFunction::HostObjectMethod(index),
        )?;
        self.host_object_methods.commit(prepared, callbacks);
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
        let prototype = self.host_family_prototype(family)?;
        if self
            .heap
            .get_own(prototype, name)
            .expect("the family retains its ordinary prototype root")
            .is_some()
        {
            return Err(RuntimeError::TypeError(
                "host-object method is already defined".into(),
            ));
        }
        let context = FamilyContext::capture(self)?;
        let index = host_registration_index(
            self.host_object_pair_methods.len(),
            0,
            "too many host-object pair methods",
        )?;
        let prepared = self.host_object_pair_methods.prepare(1, context.owner())?;
        let callbacks = prepared.callbacks(&[std::mem::size_of_val(&method)], || {
            [HostObjectPairMethodRegistration {
                family_index: family.index,
                method: Box::new(method),
            }]
        })?;
        self.function_prototype()?;
        context.validate(self)?;
        self.install_host_callable_native(
            prototype,
            name,
            length,
            NativeFunction::HostObjectPairMethod(index),
        )?;
        self.host_object_pair_methods.commit(prepared, callbacks);
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
        let prototype = self.host_family_prototype(family)?;
        if self
            .heap
            .get_own(prototype, name)
            .expect("the family retains its ordinary prototype root")
            .is_some()
        {
            return Err(RuntimeError::TypeError(
                "host-object accessor is already defined".into(),
            ));
        }
        let context = FamilyContext::capture(self)?;
        let getter_index = host_registration_index(
            self.host_object_methods.len(),
            1,
            "too many host-object methods",
        )?;
        let setter_index = getter_index + 1;
        let prepared = self.host_object_methods.prepare(2, context.owner())?;
        let callbacks = prepared.callbacks(
            &[
                std::mem::size_of_val(&getter),
                std::mem::size_of_val(&setter),
            ],
            || {
                [
                    HostObjectMethodRegistration {
                        family_index: family.index,
                        method: Box::new(getter),
                    },
                    HostObjectMethodRegistration {
                        family_index: family.index,
                        method: Box::new(setter),
                    },
                ]
            },
        )?;
        let function_prototype = self.function_prototype()?;
        context.validate(self)?;
        self.install_native_accessor(
            prototype,
            function_prototype,
            name,
            NativeFunction::HostObjectMethod(getter_index),
            NativeFunction::HostObjectMethod(setter_index),
        )?;
        self.host_object_methods.commit(prepared, callbacks);
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
        self.host_family_prototype(family)?;
        self.host_object_factories.owner_view()?;
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
        if family.heap != self.object_prototype.heap {
            return Err(RuntimeError::TypeError(
                "host-object family belongs to a different realm".into(),
            ));
        }
        self.host_family_prototype(family)?;
        if !host_property_name_is_valid(name)
            || self
                .heap
                .get_own(owner, name)
                .expect("the validated host/global owner remains realm-rooted")
                .is_some()
        {
            return Err(RuntimeError::TypeError(
                "host factory name is invalid or already defined".into(),
            ));
        }
        let context = FamilyContext::capture(self)?;
        let index = host_registration_index(
            self.host_object_factories.len(),
            0,
            "too many host-object factories",
        )?;
        let prepared = self.host_object_factories.prepare(1, context.owner())?;
        let callbacks = prepared.callbacks(&[std::mem::size_of_val(&factory)], || {
            [HostObjectFactoryRegistration {
                family_index: family.index,
                required_receiver,
                factory: Box::new(factory),
            }]
        })?;
        context.validate(self)?;
        self.install_host_callable_native(
            owner,
            name,
            length,
            NativeFunction::HostObjectFactory(index),
        )?;
        self.host_object_factories.commit(prepared, callbacks);
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
        let context = FamilyContext::capture(self)?;
        let registration_owner = self
            .host_object_factories
            .retain_for_entry(context.owner())?;
        let mut args = self.prepare_host_arguments(args, context.owner())?;
        if let Some(error) = args.error.take() {
            return Err(error);
        }
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
        let key = registration.factory.create(&args.values);
        context.validate(self)?;
        if let Some(owner) = registration_owner {
            owner.reserve_peer(0)?;
        }
        let owner = args.owner_view()?;
        let Some(key) = self.finish_host_callback(key, owner.as_ref())? else {
            return Ok(Value::Null);
        };
        self.mint_host_wrapper(family_index, key).map(Value::Object)
    }

    pub(super) fn mint_host_family_wrapper(
        &mut self,
        family: HostObjectFamily,
        key: HostObjectKey,
    ) -> Result<ObjectId, RuntimeError> {
        self.host_family_prototype(family)?;
        self.mint_host_wrapper(family.index as usize, key)
    }

    fn mint_host_wrapper(
        &mut self,
        family_index: usize,
        key: HostObjectKey,
    ) -> Result<ObjectId, RuntimeError> {
        let context = FamilyContext::capture(self)?;
        let family = &self.host_object_families[family_index];
        if let Some(&(object, _root)) = family.wrappers.get(&key) {
            return Ok(object);
        }
        if family.wrappers.len() >= MAX_HOST_OBJECTS_PER_FAMILY {
            return Err(RuntimeError::RangeError(
                "host-object wrapper limit exceeded".into(),
            ));
        }
        let prototype = family.prototype;
        let (wrappers, keys) = {
            let family = &mut self.host_object_families[family_index];
            let wrappers = family.wrappers.prepare_insert_owned(context.owner())?;
            let keys = family
                .keys_by_wrapper
                .prepare_insert_owned(context.owner())?;
            (wrappers, keys)
        };
        let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        let root = self.heap.root(object)?;
        self.host_object_families[family_index]
            .wrappers
            .commit(wrappers);
        self.host_object_families[family_index]
            .keys_by_wrapper
            .commit(keys);
        self.host_object_families[family_index]
            .wrappers
            .insert(key, (object, root));
        self.host_object_families[family_index]
            .keys_by_wrapper
            .insert(object, key);
        Ok(object)
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
        let context = FamilyContext::capture(self)?;
        let registration_owner = self.host_object_methods.retain_for_entry(context.owner())?;
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
        let mut args = self.prepare_host_arguments(args, context.owner())?;
        if let Some(error) = args.error.take() {
            return Err(error);
        }
        let result = self.host_object_methods[index as usize]
            .method
            .call(key, &args.values);
        context.validate(self)?;
        if let Some(owner) = registration_owner {
            owner.reserve_peer(0)?;
        }
        let owner = args.owner_view()?;
        self.finish_host_callback_value(result, owner.as_ref())
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
        let context = FamilyContext::capture(self)?;
        let registration_owner = self
            .host_object_pair_methods
            .retain_for_entry(context.owner())?;
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
        let result = self.host_object_pair_methods[index as usize]
            .method
            .call(parent, child);
        context.validate(self)?;
        if let Some(owner) = registration_owner {
            owner.reserve_peer(0)?;
        }
        let caller = self.argument_list_owner()?;
        self.finish_host_callback(result, caller.as_ref().or(context.owner()))?;
        Ok(Value::Object(child_object))
    }
}
