// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Standalone EventTargets share the existing collector-owned Event state.
//! Browser interfaces can reuse this listener storage without a second realm.

use super::native_value_storage::NativeValue;
use super::realm_call_storage::RealmCallRoots;
use super::transport_buffer_storage::TransportVector;
use super::web_event_storage::{EventCompletion, EventContext, EventPath, NativeEventTarget};
use super::*;
use crate::heap::WebListener;
use crate::native::TargetNative;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/event_target_storage.rs"]
mod storage_tests;

#[derive(Clone, Copy)]
struct ListenerCall {
    callback: ObjectId,
    signal: Option<ObjectId>,
    once: bool,
    passive: bool,
    handler: bool,
}

impl Vm {
    pub(super) fn install_web_event_target(&mut self) -> Result<(), RuntimeError> {
        if self.globals.contains_key("%WebEventTarget%") {
            return Ok(());
        }
        let global = self.global("globalThis")?.object_id().unwrap();
        if self.heap.get_own(global, "EventTarget")?.is_some() {
            return Err(RuntimeError::TypeError(
                "EventTarget global already defined".into(),
            ));
        }
        let base = self.stack.len();
        let result = (|| {
            let function = self.function_prototype()?;
            let object = self.object_prototype;
            let prototype = self.with_roots(|heap| heap.alloc_object(Some(object)))?;
            self.stack.push(Value::Object(prototype));
            for (name, length, native) in [
                ("addEventListener", 2, TargetNative::Add),
                ("removeEventListener", 2, TargetNative::Remove),
                ("dispatchEvent", 1, TargetNative::Dispatch),
            ] {
                self.install_native(
                    prototype,
                    function,
                    name,
                    length,
                    NativeFunction::WebTarget(native),
                )?;
                self.with_roots(|heap| {
                    heap.define_own_property(
                        prototype,
                        name,
                        PropertyDescriptor {
                            enumerable: Some(true),
                            ..Default::default()
                        },
                    )
                })?;
            }
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("EventTarget".into()),
                false,
                false,
                true,
            )?;
            let constructor = self.web_file_constructor(
                "EventTarget",
                0,
                NativeFunction::WebTarget(TargetNative::Constructor),
                function,
                prototype,
            )?;
            self.stack.push(Value::Object(constructor));
            self.define_data(
                global,
                "EventTarget",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.heap.root(prototype)?;
            self.globals.insert("%WebEventTarget%".into(), prototype);
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn web_target_call(
        &mut self,
        native: TargetNative,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if native == TargetNative::Constructor {
            if !construct {
                return Err(RuntimeError::TypeError(
                    "EventTarget constructor requires new".into(),
                ));
            }
            let default = self.globals["%WebEventTarget%"];
            let prototype = self.constructor_prototype_for(default, None)?;
            return Ok(Value::Object(
                self.with_roots(|heap| heap.alloc_web_event_target(prototype))?,
            ));
        }
        if let Some(index) = self.host_node_event_family(receiver) {
            return if native == TargetNative::Dispatch {
                self.host_node_dispatch_call(index, receiver, args, construct)
            } else {
                self.host_click_listener_call(
                    index,
                    receiver.clone(),
                    args,
                    construct,
                    native == TargetNative::Add,
                )
            };
        }
        let object = receiver
            .object_id()
            .filter(|id| self.heap.web_event_target(*id).is_ok())
            .ok_or_else(|| RuntimeError::TypeError("Invalid EventTarget receiver".into()))?;
        if native == TargetNative::Dispatch {
            let event = native::argument(args, 0)
                .object_id()
                .filter(|id| self.heap.drag_event(*id).is_ok())
                .ok_or_else(|| RuntimeError::TypeError("dispatchEvent requires an Event".into()))?;
            return self.dispatch_web_target_event(object, event, false);
        }
        if args.len() < 2 {
            return Err(RuntimeError::TypeError(
                "Event listener method requires two arguments".into(),
            ));
        }
        let context = EventContext::capture(self)?;
        let ids = std::iter::once(object).chain(args.iter().filter_map(Value::object_id));
        let roots = context.prepare_roots(self, ids, 1)?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            vm.web_target_listener_input(native, object, args, &context, &roots)
        });
        context.validate().and(result)
    }

    fn web_target_listener_input(
        &mut self,
        native: TargetNative,
        object: ObjectId,
        args: &[Value],
        context: &EventContext,
        roots: &RealmCallRoots,
    ) -> Result<Value, RuntimeError> {
        let name = if matches!(&args[0], Value::String(_)) {
            NativeValue::copied(&args[0], context.owner.as_ref())?
        } else {
            let converted = self.coerce_string(&args[0]);
            context.validate()?;
            NativeValue::adopt(Value::String(converted?), context.owner.as_ref())?
        };
        let Value::String(name_value) = name.value() else {
            unreachable!("coerced event type")
        };
        let callback = match &args[1] {
            Value::Undefined | Value::Null => None,
            Value::Object(object) => Some(*object),
            _ => {
                return Err(RuntimeError::TypeError(
                    "EventListener must be an object".into(),
                ))
            }
        };
        let options = native::argument(args, 2);
        let (mut capture, mut once, mut passive) = (false, false, false);
        let mut signal = None;
        if options.object_id().is_some() {
            let value = self.get_property(options, &"capture".into());
            context.validate()?;
            capture = self.to_boolean(&value?)?;
            if native == TargetNative::Add {
                let value = self.get_property(options, &"once".into());
                context.validate()?;
                once = self.to_boolean(&value?)?;
                let value = self.get_property(options, &"passive".into());
                context.validate()?;
                passive = self.to_boolean(&value?)?;
                let value = self.get_property(options, &"signal".into());
                context.validate()?;
                signal = self.abort_signal_argument(&value?)?;
            }
        } else if *options != Value::Undefined {
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
        let state = self.heap.web_event_target(object)?;
        let existing = state
            .listeners
            .iter()
            .find(|listener| {
                !listener.handler
                    && listener.name == *name_value
                    && listener.callback == callback
                    && listener.capture == capture
            })
            .map(|listener| (listener.serial, listener.signal));
        if native == TargetNative::Remove {
            if let Some((serial, signal)) = existing {
                if let Some(signal) = signal {
                    roots.push(&self.heap, signal)?;
                }
                self.heap.remove_web_listener(object, serial)?;
                if let Some(signal) = signal {
                    self.detach_abort_hook(
                        signal,
                        crate::heap::AbortHook::Web {
                            target: object,
                            serial,
                        },
                    )?;
                }
                self.refresh_abort_timer_roots(object)?;
            }
            return Ok(Value::Undefined);
        }
        if existing.is_some() {
            return Ok(Value::Undefined);
        }
        if state.listeners.len() >= 4096
            || signal.is_some_and(|signal| {
                self.heap
                    .abort_signal(signal)
                    .is_ok_and(|state| state.hooks.len() >= 4096)
            })
        {
            return Err(RuntimeError::RangeError(
                "Event listener limit exceeded".into(),
            ));
        }
        let serial = state
            .next_serial
            .checked_add(1)
            .ok_or_else(|| RuntimeError::RangeError("Event listener identity exhausted".into()))?;
        if let Some(signal) = signal {
            roots.push(&self.heap, signal)?;
        }
        let mut prepared = NativeEventTarget::copied(self, object, context.owner.as_ref())?;
        let (name, name_storage) = name.into_retained_parts();
        let Value::String(name) = name else {
            unreachable!("coerced event type")
        };
        prepared.listener(
            WebListener {
                signal,
                serial,
                name,
                callback,
                capture,
                once,
                passive,
                handler: false,
            },
            name_storage,
        )?;
        prepared.state.next_serial = serial;
        // Hook admission happens before listener publication. No guest call
        // separates these pure heap operations; a refusal retires the hook and
        // any just-published listener without allocating a cleanup snapshot.
        let result = (|| {
            context.validate()?;
            if let Some(signal) = signal {
                self.attach_abort_hook(
                    signal,
                    crate::heap::AbortHook::Web {
                        target: object,
                        serial,
                    },
                )?;
            }
            context.validate()?;
            prepared.into_heap(self, object)?;
            self.refresh_abort_timer_roots(object)
        })();
        if let Err(error) = result {
            self.heap.rollback_web_listener(object, serial)?;
            let cleanup = if let Some(signal) = signal {
                self.detach_abort_hook(
                    signal,
                    crate::heap::AbortHook::Web {
                        target: object,
                        serial,
                    },
                )
            } else {
                self.refresh_abort_timer_roots(object)
            };
            context.validate()?;
            return cleanup.and(Err(error));
        }
        Ok(Value::Undefined)
    }

    pub(super) fn dispatch_web_target_event(
        &mut self,
        target: ObjectId,
        event: ObjectId,
        trusted: bool,
    ) -> Result<Value, RuntimeError> {
        let context = EventContext::capture(self)?;
        if self.starts_host_event_turn() {
            return self
                .run_host_event_turn(|vm| vm.dispatch_web_target_event(target, event, trusted));
        }
        self.heap.web_event_target(target)?;
        let state = self.heap.drag_event(event)?;
        if state.phase != 0 {
            return self.drag_named_error("InvalidStateError", "Event is already dispatching");
        }
        let name = NativeValue::copied(
            state.get_ref("type").expect("branded event type"),
            context.owner.as_ref(),
        )?;
        let roots = context.prepare_roots(self, [target, event].into_iter(), 1)?;
        self.with_realm_call_roots(roots.clone(), |vm| {
            let mut started = false;
            let result = (|| {
                context.validate()?;
                EventPath::single(target, context.owner.as_ref())?
                    .publish(vm, event, target, trusted)?;
                started = true;
                let Value::String(name) = name.value() else {
                    unreachable!("branded event type")
                };
                vm.dispatch_web_target_rooted(target, event, name, &context, &roots)
            })();
            // No fresh snapshot or admission is needed after cancellation.
            if started {
                vm.heap.finish_event_dispatch(event)?;
            }
            context.validate()?;
            result
        })
    }

    fn dispatch_web_target_rooted(
        &mut self,
        target: ObjectId,
        event: ObjectId,
        name: &JsString,
        context: &EventContext,
        roots: &Rc<RealmCallRoots>,
    ) -> Result<Value, RuntimeError> {
        for capture in [true, false] {
            context.validate()?;
            if self.heap.drag_event(event)?.propagation_stopped {
                break;
            }
            let snapshot = TransportVector::capture(
                self.heap
                    .web_event_target(target)?
                    .listeners
                    .iter()
                    .filter(|listener| listener.name == *name && listener.capture == capture)
                    .map(|listener| listener.serial),
                context.owner.as_ref(),
            )?;
            for &serial in snapshot.iter() {
                context.validate()?;
                if self.heap.drag_event(event)?.immediate_stopped {
                    break;
                }
                let listener = self
                    .heap
                    .web_event_target(target)?
                    .listeners
                    .iter()
                    .find(|listener| listener.serial == serial)
                    .map(|listener| ListenerCall {
                        callback: listener.callback,
                        signal: listener.signal,
                        once: listener.once,
                        passive: listener.passive,
                        handler: listener.handler,
                    });
                let Some(listener) = listener else {
                    continue;
                };
                roots.push(&self.heap, listener.callback)?;
                let mut global_error = false;
                let called = (|| {
                    context.validate()?;
                    if listener.once {
                        if let Some(signal) = listener.signal {
                            self.detach_abort_hook(
                                signal,
                                crate::heap::AbortHook::Web { target, serial },
                            )?;
                        }
                        self.heap.remove_web_listener(target, serial)?;
                        self.refresh_abort_timer_roots(target)?;
                    }
                    self.heap.set_event_passive(event, listener.passive)?;
                    let callback = Value::Object(listener.callback);
                    let (function, receiver) = if self.is_callable(&callback)? {
                        (callback, Value::Object(target))
                    } else if listener.handler {
                        return Ok(Value::Undefined);
                    } else {
                        (
                            self.get_property(&callback, &"handleEvent".into())?,
                            callback,
                        )
                    };
                    context.validate()?;
                    global_error = listener.handler
                        && *name == "error"
                        && self.host_error_reporting.is_some()
                        && self.global("globalThis")?.object_id() == Some(target)
                        && self.heap.drag_event(event)?.interface == 6;
                    context.validate()?;
                    let mut arguments = TransportVector::with_capacity(
                        if global_error { 5 } else { 1 },
                        context.owner.as_ref(),
                    )?;
                    if global_error {
                        let state = self.heap.drag_event(event)?;
                        for field in ["message", "filename", "lineno", "colno", "error"] {
                            let value = NativeValue::copied(
                                state.get_ref(field).unwrap_or(&Value::Undefined),
                                context.owner.as_ref(),
                            )?;
                            arguments.push_owned(value)?;
                        }
                    } else {
                        arguments.push_owned(NativeValue::adopt(
                            Value::Object(event),
                            context.owner.as_ref(),
                        )?)?;
                    }
                    arguments.with_arguments(self, |vm, arguments| {
                        arguments.call_native(vm, function, receiver, false)
                    })
                })();
                let outcome = (|| {
                    context.validate()?;
                    let called =
                        EventCompletion::adopt(called, context.owner.as_ref())?.cleanup(self)?;
                    context.validate()?;
                    if listener.handler
                        && called
                            .result()
                            .as_ref()
                            .is_ok_and(|value| *value == Value::Bool(global_error))
                    {
                        self.heap.prevent_event_default(event)?;
                    }
                    if let Err(error) = called.result() {
                        if !super::web_drag_drop::drag_page_exception(error) {
                            return called.into_result();
                        }
                    }
                    Ok(Value::Undefined)
                })();
                roots.truncate(2);
                outcome?;
            }
        }
        context.validate()?;
        Ok(Value::Bool(!self.heap.drag_event(event)?.default_prevented))
    }
}
