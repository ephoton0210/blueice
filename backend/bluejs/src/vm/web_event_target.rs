// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Standalone EventTargets share the existing collector-owned Event state.
//! Browser interfaces can reuse this listener storage without a second realm.

use super::*;
use crate::heap::WebListener;
use crate::native::TargetNative;

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
        let name = self.coerce_string(&args[0])?;
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
        let mut capture = false;
        let mut once = false;
        let mut passive = false;
        let mut signal = None;
        if options.object_id().is_some() {
            let value = self.get_property(options, &"capture".into())?;
            capture = self.to_boolean(&value)?;
            if native == TargetNative::Add {
                let value = self.get_property(options, &"once".into())?;
                once = self.to_boolean(&value)?;
                let value = self.get_property(options, &"passive".into())?;
                passive = self.to_boolean(&value)?;
                let value = self.get_property(options, &"signal".into())?;
                signal = self.abort_signal_argument(&value)?;
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
        let mut state = self.heap.web_event_target(object)?.clone();
        let existing = state.listeners.iter().position(|listener| {
            !listener.handler
                && listener.name == name
                && listener.callback == callback
                && listener.capture == capture
        });
        match native {
            TargetNative::Remove => {
                if let Some(index) = existing {
                    let removed = state.listeners.remove(index);
                    if let Some(signal) = removed.signal {
                        self.detach_abort_hook(
                            signal,
                            crate::heap::AbortHook::Web {
                                target: object,
                                serial: removed.serial,
                            },
                        )?;
                    }
                }
            }
            TargetNative::Add if existing.is_none() => {
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
                state.next_serial = state.next_serial.checked_add(1).ok_or_else(|| {
                    RuntimeError::RangeError("Event listener identity exhausted".into())
                })?;
                state.listeners.push(WebListener {
                    signal,
                    serial: state.next_serial,
                    name,
                    callback,
                    capture,
                    once,
                    passive,
                    handler: false,
                });
            }
            _ => {}
        }
        let added = if native == TargetNative::Add && existing.is_none() {
            Some(state.next_serial)
        } else {
            None
        };
        self.with_roots(|heap| heap.set_web_event_target(object, state))?;
        if let (Some(signal), Some(serial)) = (signal, added) {
            self.attach_abort_hook(
                signal,
                crate::heap::AbortHook::Web {
                    target: object,
                    serial,
                },
            )?;
        }
        self.refresh_abort_timer_roots(object)?;
        Ok(Value::Undefined)
    }

    pub(super) fn dispatch_web_target_event(
        &mut self,
        target: ObjectId,
        event: ObjectId,
        trusted: bool,
    ) -> Result<Value, RuntimeError> {
        let state = self.heap.drag_event(event)?;
        if state.phase != 0 {
            return self.drag_named_error("InvalidStateError", "Event is already dispatching");
        }
        let name = match state.get("type") {
            Value::String(name) => name,
            _ => unreachable!("branded event type"),
        };
        let base = self.stack.len();
        self.stack
            .extend([Value::Object(target), Value::Object(event)]);
        let result = (|| {
            let mut state = self.heap.drag_event(event)?.clone();
            state.set("target", Value::Object(target));
            state.set("isTrusted", Value::Bool(trusted));
            state.current_target = Some(target);
            state.phase = 2;
            state.path = vec![target];
            self.with_roots(|heap| heap.set_drag_event_state(event, state))?;
            for capture in [true, false] {
                if self.heap.drag_event(event)?.propagation_stopped {
                    break;
                }
                let snapshot: Vec<_> = self
                    .heap
                    .web_event_target(target)?
                    .listeners
                    .iter()
                    .filter(|listener| listener.name == name && listener.capture == capture)
                    .map(|listener| listener.serial)
                    .collect();
                for serial in snapshot {
                    if self.heap.drag_event(event)?.immediate_stopped {
                        break;
                    }
                    let listener = self
                        .heap
                        .web_event_target(target)?
                        .listeners
                        .iter()
                        .find(|listener| listener.serial == serial)
                        .cloned();
                    let Some(listener) = listener else {
                        continue;
                    };
                    let callback_base = self.stack.len();
                    self.stack.push(Value::Object(listener.callback));
                    let called = (|| {
                        if listener.once {
                            if let Some(signal) = listener.signal {
                                self.detach_abort_hook(
                                    signal,
                                    crate::heap::AbortHook::Web {
                                        target,
                                        serial: listener.serial,
                                    },
                                )?;
                            }
                            let mut state = self.heap.web_event_target(target)?.clone();
                            state
                                .listeners
                                .retain(|candidate| candidate.serial != serial);
                            self.with_roots(|heap| heap.set_web_event_target(target, state))?;
                            self.refresh_abort_timer_roots(target)?;
                        }
                        let mut state = self.heap.drag_event(event)?.clone();
                        state.passive = listener.passive;
                        self.with_roots(|heap| heap.set_drag_event_state(event, state))?;
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
                        self.call_native(function, receiver, vec![Value::Object(event)], false)
                    })();
                    self.stack.truncate(callback_base);
                    if listener.handler
                        && called
                            .as_ref()
                            .is_ok_and(|value| *value == Value::Bool(false))
                    {
                        let mut state = self.heap.drag_event(event)?.clone();
                        if state.cancelable && !state.passive {
                            state.default_prevented = true;
                            self.with_roots(|heap| heap.set_drag_event_state(event, state))?;
                        }
                    }
                    if let Err(error) = called {
                        if !super::web_drag_drop::drag_page_exception(&error) {
                            return Err(error);
                        }
                    }
                }
            }
            Ok(Value::Bool(!self.heap.drag_event(event)?.default_prevented))
        })();
        let cleanup = (|| {
            let mut state = self.heap.drag_event(event)?.clone();
            state.phase = 0;
            state.current_target = None;
            state.path.clear();
            state.passive = false;
            state.propagation_stopped = false;
            state.immediate_stopped = false;
            self.with_roots(|heap| heap.set_drag_event_state(event, state))
        })();
        self.stack.truncate(base);
        match result {
            Ok(value) => {
                cleanup?;
                Ok(value)
            }
            Err(error) => {
                cleanup?;
                Err(error)
            }
        }
    }
}
