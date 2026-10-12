// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! DOM abort controllers/signals, exact reasons and listener cancellation.
//! Behavioral reference: https://dom.spec.whatwg.org/#aborting-ongoing-activities
use super::*;
use crate::heap::{AbortHook, WebListener};
use crate::native::AbortNative;
mod timers;

impl Vm {
    pub(super) fn install_web_abort(&mut self) -> Result<(), RuntimeError> {
        if self.globals.contains_key("%WebAbortSignal%") {
            return Ok(());
        }
        let global = self.global("globalThis")?.object_id().unwrap();
        for name in ["AbortSignal", "AbortController"] {
            if self.heap.get_own(global, name)?.is_some() {
                return Err(RuntimeError::TypeError(
                    "Abort global already defined".into(),
                ));
            }
        }
        let base = self.stack.len();
        let result = (|| {
            let function = self.function_prototype()?;
            let target = self.globals["%WebEventTarget%"];
            let signal = self.with_roots(|heap| heap.alloc_object(Some(target)))?;
            self.stack.push(Value::Object(signal));
            for (name, native) in [
                ("aborted", AbortNative::Aborted),
                ("reason", AbortNative::Reason),
            ] {
                self.install_native_getter(
                    signal,
                    function,
                    name,
                    NativeFunction::WebAbort(native),
                )?;
            }
            self.install_native(
                signal,
                function,
                "throwIfAborted",
                0,
                NativeFunction::WebAbort(AbortNative::ThrowIfAborted),
            )?;
            self.install_native_accessor(
                signal,
                function,
                "onabort",
                NativeFunction::WebAbort(AbortNative::Handler(false)),
                NativeFunction::WebAbort(AbortNative::Handler(true)),
            )?;
            self.define_data(
                signal,
                JsSymbol::well_known("toStringTag"),
                Value::String("AbortSignal".into()),
                false,
                false,
                true,
            )?;
            let signal_constructor = self.web_file_constructor(
                "AbortSignal",
                0,
                NativeFunction::WebAbort(AbortNative::Signal),
                function,
                signal,
            )?;
            self.stack.push(Value::Object(signal_constructor));
            for (name, length, native) in [
                ("abort", 0, AbortNative::StaticAbort),
                ("any", 1, AbortNative::Any),
                ("timeout", 1, AbortNative::Timeout),
            ] {
                self.install_native(
                    signal_constructor,
                    function,
                    name,
                    length,
                    NativeFunction::WebAbort(native),
                )?;
                self.abort_enumerable(signal_constructor, name)?;
            }
            for name in ["aborted", "reason", "throwIfAborted", "onabort"] {
                self.abort_enumerable(signal, name)?;
            }
            let object = self.object_prototype;
            let controller = self.with_roots(|heap| heap.alloc_object(Some(object)))?;
            self.stack.push(Value::Object(controller));
            self.install_native_getter(
                controller,
                function,
                "signal",
                NativeFunction::WebAbort(AbortNative::ControllerSignal),
            )?;
            self.install_native(
                controller,
                function,
                "abort",
                0,
                NativeFunction::WebAbort(AbortNative::Abort),
            )?;
            self.abort_enumerable(controller, "signal")?;
            self.abort_enumerable(controller, "abort")?;
            self.define_data(
                controller,
                JsSymbol::well_known("toStringTag"),
                Value::String("AbortController".into()),
                false,
                false,
                true,
            )?;
            let constructor = self.web_file_constructor(
                "AbortController",
                0,
                NativeFunction::WebAbort(AbortNative::Controller),
                function,
                controller,
            )?;
            self.stack.push(Value::Object(constructor));
            self.define_data(
                global,
                "AbortSignal",
                Value::Object(signal_constructor),
                true,
                false,
                true,
            )?;
            self.define_data(
                global,
                "AbortController",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.globals.insert("%WebAbortSignal%".into(), signal);
            self.globals
                .insert("%WebAbortController%".into(), controller);
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }
    fn abort_enumerable(&mut self, object: ObjectId, name: &str) -> Result<(), RuntimeError> {
        self.with_roots(|heap| {
            heap.define_own_property(
                object,
                name,
                PropertyDescriptor {
                    enumerable: Some(true),
                    ..Default::default()
                },
            )
        })?;
        Ok(())
    }
    fn fresh_abort_signal(&mut self) -> Result<ObjectId, RuntimeError> {
        let prototype = self.globals["%WebAbortSignal%"];
        self.with_roots(|heap| heap.alloc_abort_signal(prototype))
    }
    fn abort_reason(&mut self, value: &Value, timeout: bool) -> Result<Value, RuntimeError> {
        if *value != Value::Undefined {
            return Ok(value.clone());
        }
        let prototype = self.globals["%WebDOMException%"];
        let name = if timeout {
            "TimeoutError"
        } else {
            "AbortError"
        };
        Ok(Value::Object(self.with_roots(|heap| {
            heap.alloc_dom_exception(name.into(), "The operation was aborted".into(), prototype)
        })?))
    }
    pub(super) fn abort_signal_argument(
        &self,
        value: &Value,
    ) -> Result<Option<ObjectId>, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(None);
        }
        value
            .object_id()
            .filter(|id| self.heap.abort_signal(*id).is_ok())
            .map(Some)
            .ok_or_else(|| RuntimeError::TypeError("signal requires an AbortSignal".into()))
    }
    pub(super) fn web_abort_call(
        &mut self,
        native: AbortNative,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        use AbortNative::*;
        if native == Signal {
            return Err(RuntimeError::TypeError(
                "Illegal AbortSignal constructor".into(),
            ));
        }
        if native == Controller {
            if !construct {
                return Err(RuntimeError::TypeError(
                    "AbortController requires new".into(),
                ));
            }
            let base = self.stack.len();
            let signal = self.fresh_abort_signal()?;
            self.stack.push(Value::Object(signal));
            let result = (|| {
                let prototype = self.constructor_prototype(self.globals["%WebAbortController%"])?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_abort_controller(signal, prototype)
                })?))
            })();
            self.stack.truncate(base);
            return result;
        }
        if construct {
            return Err(RuntimeError::TypeError(
                "Abort method is not a constructor".into(),
            ));
        }
        if native == Any {
            return self.abort_signal_any(args);
        }
        if native == Timeout {
            return self.abort_signal_timeout(args);
        }
        if native == StaticAbort {
            let base = self.stack.len();
            let signal = self.fresh_abort_signal()?;
            self.stack.push(Value::Object(signal));
            let result = (|| {
                let reason = self.abort_reason(native::argument(args, 0), false)?;
                let mut state = self.heap.abort_signal(signal)?.clone();
                state.reason = reason;
                self.with_roots(|heap| heap.set_abort_signal(signal, state))?;
                Ok(Value::Object(signal))
            })();
            self.stack.truncate(base);
            return result;
        }
        let id = receiver
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Invalid abort receiver".into()))?;
        if matches!(native, ControllerSignal | Abort) {
            let signal = self
                .heap
                .abort_controller(id)
                .map_err(|_| RuntimeError::TypeError("Invalid AbortController receiver".into()))?;
            return if native == ControllerSignal {
                Ok(Value::Object(signal))
            } else {
                self.signal_abort(signal, native::argument(args, 0).clone())?;
                Ok(Value::Undefined)
            };
        }
        let state = self
            .heap
            .abort_signal(id)
            .map_err(|_| RuntimeError::TypeError("Invalid AbortSignal receiver".into()))?;
        match native {
            Aborted => Ok(Value::Bool(state.aborted())),
            Reason => Ok(state.reason.clone()),
            ThrowIfAborted => {
                if state.aborted() {
                    Err(RuntimeError::Thrown(state.reason.clone()))
                } else {
                    Ok(Value::Undefined)
                }
            }
            Handler(set) => self.abort_handler(id, set, args),
            _ => unreachable!("abort dispatch"),
        }
    }
    fn abort_handler(
        &mut self,
        signal: ObjectId,
        set: bool,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let mut state = self.heap.web_event_target(signal)?.clone();
        let old = state
            .listeners
            .iter()
            .position(|listener| listener.handler && listener.name == "abort");
        if !set {
            return Ok(old.map_or(Value::Null, |index| {
                Value::Object(state.listeners[index].callback)
            }));
        }
        let callback = native::argument(args, 0).object_id();
        match (old, callback) {
            (Some(index), Some(callback)) => state.listeners[index].callback = callback,
            (Some(index), None) => {
                state.listeners.remove(index);
            }
            (None, Some(callback)) => {
                if state.listeners.len() >= 4096 {
                    return Err(RuntimeError::RangeError(
                        "Abort listener limit exceeded".into(),
                    ));
                }
                state.next_serial = state.next_serial.checked_add(1).ok_or_else(|| {
                    RuntimeError::RangeError("Listener identity exhausted".into())
                })?;
                state.listeners.push(WebListener {
                    serial: state.next_serial,
                    name: "abort".into(),
                    callback,
                    capture: false,
                    once: false,
                    passive: false,
                    handler: true,
                    signal: None,
                });
            }
            _ => {}
        }
        self.with_roots(|heap| heap.set_web_event_target(signal, state))?;
        self.refresh_abort_timer_roots(signal)?;
        Ok(Value::Undefined)
    }
    fn abort_signal_any(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let input = native::argument(args, 0);
        if input.object_id().is_none() {
            return Err(RuntimeError::TypeError(
                "AbortSignal.any requires an iterable object".into(),
            ));
        }
        let base = self.stack.len();
        let result = (|| {
            let iterator = self.get_iterator(input)?;
            self.stack.push(iterator.clone());
            let mut signals = Vec::new();
            let collected = (|| {
                while let Some(value) = self.iterator_step(&iterator, true)? {
                    self.charge_step()?;
                    let signal = self.abort_signal_argument(&value)?.ok_or_else(|| {
                        RuntimeError::TypeError("AbortSignal.any requires signals".into())
                    })?;
                    if signals.len() >= 4096 {
                        return Err(RuntimeError::RangeError(
                            "Abort source limit exceeded".into(),
                        ));
                    }
                    signals.push(signal);
                    self.stack.push(value);
                }
                Ok(())
            })();
            if let Err(error) = collected {
                if let RuntimeError::Thrown(value) = &error {
                    self.stack.push(value.clone());
                }
                let _ = self.iterator_close(&iterator);
                return Err(error);
            }
            let result = self.fresh_abort_signal()?;
            self.stack.push(Value::Object(result));
            let mut state = self.heap.abort_signal(result)?.clone();
            for signal in &signals {
                let source = self.heap.abort_signal(*signal)?;
                if source.aborted() {
                    state.reason = source.reason.clone();
                    self.with_roots(|heap| heap.set_abort_signal(result, state))?;
                    return Ok(Value::Object(result));
                }
            }
            state.dependent = true;
            for signal in signals {
                let source = self.heap.abort_signal(signal)?;
                let sources = if source.dependent {
                    source.sources.clone()
                } else {
                    vec![signal]
                };
                for source in sources {
                    if !state.sources.contains(&source) {
                        state.sources.push(source);
                    }
                }
            }
            if state.sources.len() > 4096 {
                return Err(RuntimeError::RangeError(
                    "Abort source limit exceeded".into(),
                ));
            }
            for source in &state.sources {
                let mut data = self.heap.abort_signal(*source)?.clone();
                if data.dependents.len() >= 4096 {
                    return Err(RuntimeError::RangeError(
                        "Abort dependency limit exceeded".into(),
                    ));
                }
                data.dependents.push(result);
                self.with_roots(|heap| heap.set_abort_signal(*source, data))?;
            }
            self.with_roots(|heap| heap.set_abort_signal(result, state))?;
            Ok(Value::Object(result))
        })();
        self.stack.truncate(base);
        result
    }
    pub(super) fn attach_abort_hook(
        &mut self,
        signal: ObjectId,
        hook: AbortHook,
    ) -> Result<(), RuntimeError> {
        let mut state = self.heap.abort_signal(signal)?.clone();
        if !state.aborted() {
            if state.hooks.len() >= 4096 {
                return Err(RuntimeError::RangeError(
                    "Abort algorithm limit exceeded".into(),
                ));
            }
            state.hooks.push(hook);
            self.with_roots(|heap| heap.set_abort_signal(signal, state))?;
            self.refresh_abort_timer_roots(signal)?;
        }
        Ok(())
    }
    pub(super) fn detach_abort_hook(
        &mut self,
        signal: ObjectId,
        hook: AbortHook,
    ) -> Result<(), RuntimeError> {
        let mut state = self.heap.abort_signal(signal)?.clone();
        state.hooks.retain(|candidate| *candidate != hook);
        self.with_roots(|heap| heap.set_abort_signal(signal, state))?;
        self.refresh_abort_timer_roots(signal)
    }
    pub(super) fn signal_abort(
        &mut self,
        signal: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        if self.heap.abort_signal(signal)?.aborted() {
            return Ok(());
        }
        let base = self.stack.len();
        self.stack.extend([Value::Object(signal), value.clone()]);
        let result = (|| {
            let reason = self.abort_reason(&value, false)?;
            self.stack.push(reason.clone());
            let dependents = self.heap.abort_signal(signal)?.dependents.clone();
            let mut abort = vec![signal];
            for dependent in dependents {
                if self
                    .heap
                    .abort_signal(dependent)
                    .is_ok_and(|state| !state.aborted())
                {
                    abort.push(dependent);
                    self.stack.push(Value::Object(dependent));
                }
            }
            for id in &abort {
                let mut state = self.heap.abort_signal(*id)?.clone();
                state.reason = reason.clone();
                self.with_roots(|heap| heap.set_abort_signal(*id, state))?;
            }
            for id in abort {
                let mut state = self.heap.abort_signal(id)?.clone();
                let hooks = std::mem::take(&mut state.hooks);
                state.sources.clear();
                state.dependents.clear();
                self.with_roots(|heap| heap.set_abort_signal(id, state))?;
                for hook in hooks {
                    match hook {
                        AbortHook::Web { target, serial } => {
                            if let Ok(target_state) = self.heap.web_event_target(target) {
                                let mut target_state = target_state.clone();
                                target_state
                                    .listeners
                                    .retain(|listener| listener.serial != serial);
                                self.with_roots(|heap| {
                                    heap.set_web_event_target(target, target_state)
                                })?;
                            }
                        }
                        AbortHook::Host { serial } => self.remove_host_listener_serial(serial)?,
                    }
                }
                let event_state = self.new_web_event_state(0, "abort".into());
                let prototype = self.globals["%WebEvent%"];
                let event =
                    self.with_roots(|heap| heap.alloc_drag_event(event_state, prototype))?;
                self.stack.push(Value::Object(event));
                self.finish_web_event(event)?;
                self.dispatch_web_target_event(id, event, true)?;
                self.stack.pop();
                self.refresh_abort_timer_roots(id)?;
            }
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }
}
