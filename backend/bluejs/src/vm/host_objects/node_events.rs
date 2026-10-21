// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Script dispatch uses a freshly owner-validated ancestry snapshot. It
//! never invokes a native default action or creates a native input grant.

use super::*;
use crate::vm::error_input_storage::utf16_from_utf8;
use crate::vm::realm_call_storage::RealmCallRoots;
use crate::vm::transport_buffer_storage::TransportVector;
use crate::vm::web_event_storage::{EventContext, EventPath, NativeEventState};

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/node_event_storage.rs"]
mod storage_tests;

pub(in crate::vm) struct HostEventContext {
    pub(in crate::vm) event: EventContext,
    family: FamilyContext,
}

impl HostEventContext {
    pub(in crate::vm) fn capture(vm: &mut Vm) -> Result<Self, RuntimeError> {
        let mut event = EventContext::capture(vm)?;
        let family = FamilyContext::capture_with_owner(vm, event.owner.as_ref())?;
        if event.owner.is_none() {
            event.owner = family
                .owner()
                .map(|owner| owner.reserve_peer(0))
                .transpose()?;
        }
        let result = Self { event, family };
        result.validate(vm)?;
        Ok(result)
    }

    pub(in crate::vm) fn registration_owner(
        &self,
    ) -> Option<&crate::vm::host_scheduling::SharedSchedulingCharge> {
        self.family.owner().or(self.event.owner.as_ref())
    }

    pub(in crate::vm) fn validate(&self, vm: &Vm) -> Result<(), RuntimeError> {
        self.event.validate()?;
        self.family.validate(vm)
    }

    pub(in crate::vm) fn with_roots<T>(
        &self,
        vm: &mut Vm,
        ids: impl Iterator<Item = ObjectId> + Clone,
        extra: usize,
        action: impl FnOnce(&mut Vm, &RealmCallRoots) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        self.validate(vm)?;
        let roots = self.event.prepare_roots(vm, ids, extra)?;
        let separate_family = self.family.owner().is_some_and(|family| {
            self.event
                .owner
                .as_ref()
                .is_none_or(|event| !event.same_owner(family))
        });
        let run = |vm: &mut Vm| vm.with_realm_call_roots(roots.clone(), |vm| action(vm, &roots));
        let result = if separate_family {
            // The family and managed event can retain different original
            // accounts in a neutral borrower. Keep both in inherited guards
            // through callback cleanup/checkpoints without rebinding policy.
            let family_roots = RealmCallRoots::new(0, self.family.owner())?;
            vm.with_realm_call_roots(family_roots, run)
        } else {
            run(vm)
        };
        self.validate(vm).and(result)
    }
}

fn valid_node_path(path: &[HostObjectKey], key: HostObjectKey) -> bool {
    path.first() == Some(&key)
        && path.len() <= 256
        && path.iter().enumerate().all(|(index, ancestor)| {
            ancestor.matches_owner(key.owner, key.generation) && !path[..index].contains(ancestor)
        })
}

pub(super) fn valid_host_event_path(path: &[HostObjectKey]) -> bool {
    path.first().is_some_and(|key| valid_node_path(path, *key))
}

impl Vm {
    /// Enables arbitrary named listeners and script dispatch for one private
    /// node family. The owner supplies target-first live document ancestry.
    /// Opaque keys and callback objects never cross into page-visible data.
    pub fn install_host_node_event_methods(
        &mut self,
        family: HostObjectFamily,
        path: impl FnMut(HostObjectKey) -> Result<Vec<HostObjectKey>, HostFunctionError> + 'static,
    ) -> Result<(), RuntimeError> {
        let prototype = self.host_family_prototype(family)?;
        if self.host_object_families[family.index as usize]
            .event_path
            .is_some()
        {
            return Err(RuntimeError::TypeError(
                "Node event methods already installed".into(),
            ));
        }
        let context = FamilyContext::capture(self)?;
        let charge = context
            .owner()
            .map(|owner| owner.reserve_peer(std::mem::size_of_val(&path)))
            .transpose()?;
        self.install_web_events()?;
        self.install_web_event_target()?;
        if !self.host_object_families[family.index as usize].event_target {
            self.install_host_click_event_methods(family)?;
        }
        self.install_host_callable_native(
            prototype,
            "dispatchEvent",
            1,
            NativeFunction::HostNodeDispatch(family.index),
        )?;
        self.heap
            .set_prototype(prototype, Some(self.globals["%WebEventTarget%"]))?;
        context.validate(self)?;
        self.host_object_families[family.index as usize].event_path = Some(Box::new(path));
        self.host_object_families[family.index as usize].replace_callback_charge(0, charge);
        Ok(())
    }

    pub(in super::super) fn host_node_event_family(&self, receiver: &Value) -> Option<u32> {
        let object = receiver.object_id()?;
        self.host_object_families
            .iter()
            .enumerate()
            .find_map(|(index, family)| {
                (family.event_path.is_some() && family.keys_by_wrapper.contains_key(&object))
                    .then_some(index as u32)
            })
    }

    pub(super) fn root_host_event_type(
        &mut self,
        name: &NativeValue,
    ) -> Result<RootId, RuntimeError> {
        let owner = name.owner_view()?;
        let copy = NativeValue::copied(name.value(), owner.as_ref())?;
        let prototype = self.object_prototype;
        let metadata = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        let (value, storage) = copy.into_retained_parts();
        property_definition_storage::PropertyDefinitionInput::from_retained(
            "type".into(),
            PropertyDescriptor::data(value, false, false, false),
            storage,
        )
        .define(self, metadata)?;
        Ok(self.heap.root(metadata)?)
    }

    pub(in super::super) fn host_node_dispatch_call(
        &mut self,
        family_index: u32,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if construct || args.is_empty() {
            return Err(RuntimeError::TypeError(
                "dispatchEvent requires an Event".into(),
            ));
        }
        let target = receiver
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Invalid node receiver".into()))?;
        let key = self
            .host_object_families
            .get(family_index as usize)
            .and_then(|family| family.keys_by_wrapper.get(&target).copied())
            .ok_or_else(|| RuntimeError::TypeError("Invalid node receiver".into()))?;
        let event = args[0]
            .object_id()
            .filter(|event| self.heap.drag_event(*event).is_ok())
            .ok_or_else(|| RuntimeError::TypeError("dispatchEvent requires an Event".into()))?;
        let context = HostEventContext::capture(self)?;
        context.with_roots(self, [target, event].into_iter(), 0, |vm, _| {
            if vm.heap.drag_event(event)?.phase != 0 {
                return vm.drag_named_error("InvalidStateError", "Event is already dispatching");
            }
            let path = vm.host_node_event_path(family_index, key)?;
            context.validate(vm)?;
            vm.dispatch_node_event_path(family_index, target, event, &path, false)
        })
    }

    pub(super) fn host_node_event_path(
        &mut self,
        family_index: u32,
        key: HostObjectKey,
    ) -> Result<TransportVector<HostObjectKey>, RuntimeError> {
        let context = HostEventContext::capture(self)?;
        let path = self
            .host_object_families
            .get_mut(family_index as usize)
            .and_then(|family| family.event_path.as_mut())
            .ok_or_else(|| RuntimeError::TypeError("Node event path unavailable".into()))?(
            key
        )
        .map_err(|error| RuntimeError::TypeError(error.0));
        context.validate(self)?;
        let path = TransportVector::from_owned(path?, context.family.owner())?;
        if !valid_node_path(&path, key) {
            return Err(RuntimeError::TypeError(
                "Invalid node event ancestry".into(),
            ));
        }
        Ok(path)
    }

    pub(super) fn dispatch_native_node_event(
        &mut self,
        family_index: u32,
        path: &[HostObjectKey],
        name: &str,
        cancelable: bool,
        composed: bool,
    ) -> Result<Value, RuntimeError> {
        let context = HostEventContext::capture(self)?;
        if !path.first().is_some_and(|key| valid_node_path(path, *key)) {
            return Err(RuntimeError::TypeError(
                "Invalid node event ancestry".into(),
            ));
        }
        context.with_roots(self, std::iter::empty(), 2, |vm, roots| {
            let target = vm.mint_host_wrapper(family_index as usize, path[0]);
            let target = context.validate(vm).and(target)?;
            roots.push(&vm.heap, target)?;
            let name = utf16_from_utf8(name, context.event.owner.as_ref())?;
            let mut state = NativeEventState::new(vm, 0, name)?;
            state.state.cancelable = cancelable;
            state.scalar(vm, roots, "bubbles", Value::Bool(true))?;
            state.scalar(vm, roots, "composed", Value::Bool(composed))?;
            let prototype = vm.globals["%WebEvent%"];
            let event = state.allocate(vm, prototype)?;
            roots.push(&vm.heap, event)?;
            let finished = vm.finish_web_event(event);
            context.validate(vm).and(finished)?;
            vm.dispatch_node_event_path(family_index, target, event, path, true)
        })
    }

    fn dispatch_node_event_path(
        &mut self,
        family_index: u32,
        target: ObjectId,
        event: ObjectId,
        path: &[HostObjectKey],
        trusted: bool,
    ) -> Result<Value, RuntimeError> {
        let context = HostEventContext::capture(self)?;
        let key = self
            .host_object_families
            .get(family_index as usize)
            .and_then(|family| family.keys_by_wrapper.get(&target))
            .copied()
            .ok_or_else(|| RuntimeError::TypeError("Invalid node receiver".into()))?;
        if !valid_node_path(path, key) {
            return Err(RuntimeError::TypeError(
                "Invalid node event ancestry".into(),
            ));
        }
        if self.heap.drag_event(event)?.phase != 0 {
            return self.drag_named_error("InvalidStateError", "Event is already dispatching");
        }
        context.with_roots(
            self,
            [target, event].into_iter(),
            path.len(),
            |vm, roots| {
                let mut started = false;
                let result = (|| {
                    let state = vm.heap.drag_event(event)?;
                    let name = NativeValue::copied(
                        state.get_ref("type").expect("branded event type"),
                        context.event.owner.as_ref(),
                    )?;
                    let Value::String(name_value) = name.value() else {
                        unreachable!("branded event type")
                    };
                    let bubbles = state.get_ref("bubbles") == Some(&Value::Bool(true));
                    let mut wrappers =
                        EventPath::with_capacity(path.len(), context.event.owner.as_ref())?;
                    for key in path {
                        context.validate(vm)?;
                        let wrapper = vm.mint_host_wrapper(family_index as usize, *key);
                        let wrapper = context.validate(vm).and(wrapper)?;
                        roots.push(&vm.heap, wrapper)?;
                        wrappers.push(wrapper)?;
                    }
                    context.validate(vm)?;
                    wrappers.publish_nodes(vm, event, target, trusted)?;
                    started = true;
                    // The same paid Vec now belongs to the managed event. It
                    // supplies each current target until allocation-free cleanup.
                    roots.truncate(2);
                    for (index, capture) in listener_phases(path.len()) {
                        context.validate(vm)?;
                        if vm.heap.drag_event(event)?.propagation_stopped {
                            break;
                        }
                        if !capture && index > 0 && !bubbles {
                            continue;
                        }
                        let current = vm.heap.drag_event(event)?.path[index];
                        vm.heap.set_event_phase(
                            event,
                            current,
                            if index == 0 {
                                2
                            } else if capture {
                                1
                            } else {
                                3
                            },
                        )?;
                        let snapshot = vm.listener_snapshot(
                            |listener| {
                                listener.family_index == family_index
                                    && listener.key == path[index]
                                    && listener.event_type == *name_value
                                    && listener.capture == capture
                            },
                            false,
                        )?;
                        for serial in snapshot.serials.iter().copied() {
                            context.validate(vm)?;
                            snapshot.validate(vm)?;
                            if vm.heap.drag_event(event)?.immediate_stopped {
                                break;
                            }
                            let called = vm.invoke_host_listener(serial, current, event);
                            context.validate(vm)?;
                            snapshot.validate(vm)?;
                            if let Err(error) = called {
                                if !super::super::web_drag_drop::drag_page_exception(&error) {
                                    return Err(error);
                                }
                            }
                        }
                    }
                    Ok(Value::Bool(!vm.heap.drag_event(event)?.default_prevented))
                })();
                let cleanup = if started {
                    vm.heap
                        .finish_event_dispatch(event)
                        .map_err(RuntimeError::from)
                } else {
                    Ok(())
                };
                context.validate(vm).and(cleanup).and(result)
            },
        )
    }
}
