// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! DOM abort controllers/signals, exact reasons and listener cancellation.
//! Behavioral reference: https://dom.spec.whatwg.org/#aborting-ongoing-activities
use super::*;
use crate::heap::AbortHook;
use crate::native::AbortNative;
use native_value_storage::NativeValue;
use realm_call_storage::RealmCallRoots;
use transport_buffer_storage::TransportVector;
mod storage;
use storage::{AbortContext, NativeAbortState};
mod timers;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/abort_storage.rs"]
mod storage_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/abort_cleanup.rs"]
mod cleanup_tests;

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
        let context = AbortContext::capture(self)?;
        let value = NativeValue::copied(value, context.owner.as_ref())?;
        context.with_roots(self, value.value().object_id().into_iter(), |vm| {
            vm.abort_reason_owned(value, timeout, &context)
                .map(NativeValue::into_value)
        })
    }
    fn abort_reason_owned(
        &mut self,
        value: NativeValue,
        timeout: bool,
        context: &AbortContext,
    ) -> Result<NativeValue, RuntimeError> {
        value.validate_owner()?;
        if *value.value() != Value::Undefined {
            return Ok(value);
        }
        storage::default_reason(self, timeout, context.owner.as_ref())
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
            let context = AbortContext::capture(self)?;
            return context.with_roots(self, std::iter::empty(), |vm| {
                let signal = vm.fresh_abort_signal()?;
                context.with_roots(vm, std::iter::once(signal), |vm| {
                    let prototype = vm.constructor_prototype(vm.globals["%WebAbortController%"])?;
                    context.validate()?;
                    Ok(Value::Object(vm.with_roots(|heap| {
                        heap.alloc_abort_controller(signal, prototype)
                    })?))
                })
            });
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
            let context = AbortContext::capture(self)?;
            let value = NativeValue::copied(native::argument(args, 0), context.owner.as_ref())?;
            return context.with_roots(self, value.value().object_id().into_iter(), |vm| {
                let signal = vm.fresh_abort_signal()?;
                context.with_roots(vm, std::iter::once(signal), |vm| {
                    let reason = vm.abort_reason_owned(value, false, &context)?;
                    let mut state = NativeAbortState::copied(vm, signal, context.owner.as_ref())?;
                    state.reason(reason)?;
                    state.into_heap(vm, signal)?;
                    Ok(Value::Object(signal))
                })
            });
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
                self.signal_abort_copied(signal, native::argument(args, 0))?;
                Ok(Value::Undefined)
            };
        }
        let state = self
            .heap
            .abort_signal(id)
            .map_err(|_| RuntimeError::TypeError("Invalid AbortSignal receiver".into()))?;
        match native {
            Aborted => Ok(Value::Bool(state.aborted())),
            Reason => {
                let context = AbortContext::capture(self)?;
                Ok(NativeValue::copied(&state.reason, context.owner.as_ref())?.into_value())
            }
            ThrowIfAborted => {
                if state.aborted() {
                    let context = AbortContext::capture(self)?;
                    Err(RuntimeError::Thrown(
                        NativeValue::copied(&state.reason, context.owner.as_ref())?.into_value(),
                    ))
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
        let context = AbortContext::capture(self)?;
        let old = self
            .heap
            .web_event_target(signal)?
            .listeners
            .iter()
            .position(|listener| listener.handler && listener.name == "abort");
        if !set {
            return Ok(old.map_or(Value::Null, |index| {
                Value::Object(self.heap.web_event_target(signal).unwrap().listeners[index].callback)
            }));
        }
        let callback = native::argument(args, 0).object_id();
        context.with_roots(self, std::iter::once(signal).chain(callback), |vm| {
            let mut target =
                storage::NativeAbortTarget::copied(vm, signal, context.owner.as_ref())?;
            match (old, callback) {
                (Some(index), Some(callback)) => target.state.listeners[index].callback = callback,
                (Some(index), None) => {
                    target.state.listeners.remove(index);
                }
                (None, Some(callback)) => {
                    if target.state.listeners.len() >= 4096 {
                        return Err(RuntimeError::RangeError(
                            "Abort listener limit exceeded".into(),
                        ));
                    }
                    target.state.next_serial =
                        target.state.next_serial.checked_add(1).ok_or_else(|| {
                            RuntimeError::RangeError("Listener identity exhausted".into())
                        })?;
                    target.handler(callback)?;
                }
                _ => {}
            }
            target.into_heap(vm, signal)?;
            vm.refresh_abort_timer_roots(signal)?;
            Ok(Value::Undefined)
        })
    }
    fn abort_signal_any(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let input = native::argument(args, 0);
        if input.object_id().is_none() {
            return Err(RuntimeError::TypeError(
                "AbortSignal.any requires an iterable object".into(),
            ));
        }
        let context = AbortContext::capture(self)?;
        context.with_roots(self, input.object_id().into_iter(), |vm| {
            let iterator_roots = RealmCallRoots::new(1, context.owner.as_ref())?;
            let iterator = vm.get_iterator(input)?;
            iterator_roots.push(&vm.heap, iterator.object_id().expect("iterator record"))?;
            vm.with_realm_call_roots(iterator_roots.clone(), |vm| {
                let mut signals = TransportVector::with_capacity(0, context.owner.as_ref())?;
                let collected = (|| {
                    loop {
                        let stepped = vm.iterator_step(&iterator, true);
                        let Some(value) = context.validate().and(stepped)? else {
                            break;
                        };
                        vm.charge_step()?;
                        let signal = vm.abort_signal_argument(&value)?.ok_or_else(|| {
                            RuntimeError::TypeError("AbortSignal.any requires signals".into())
                        })?;
                        if signals.len() >= 4096 {
                            return Err(RuntimeError::RangeError(
                                "Abort source limit exceeded".into(),
                            ));
                        }
                        signals.push(signal)?;
                        iterator_roots.push_admitted(&vm.heap, signal)?;
                    }
                    Ok(())
                })();
                if let Err(error) = collected {
                    // IteratorClose preserves the original throw while its
                    // independently retained input survives guest cleanup.
                    let error = storage::NativeAbortError::adopt(error, context.owner.as_ref())?;
                    let thrown = match &error.error {
                        RuntimeError::Thrown(value) => value.object_id(),
                        _ => None,
                    };
                    context.with_roots(vm, thrown.into_iter(), |vm| {
                        vm.iterator_close_preserving_throw(&iterator);
                        Ok(())
                    })?;
                    return Err(error.into_error());
                }
                let result = vm.fresh_abort_signal()?;
                iterator_roots.push_admitted(&vm.heap, result)?;
                let mut state = NativeAbortState::copied(vm, result, context.owner.as_ref())?;
                for signal in signals.iter().copied() {
                    let source = vm.heap.abort_signal(signal)?;
                    if source.aborted() {
                        state
                            .reason(NativeValue::copied(&source.reason, context.owner.as_ref())?)?;
                        state.into_heap(vm, result)?;
                        return Ok(Value::Object(result));
                    }
                }
                state.state.dependent = true;
                for signal in signals.iter().copied() {
                    let source = vm.heap.abort_signal(signal)?;
                    let sources = if source.dependent {
                        source.sources.as_slice()
                    } else {
                        std::slice::from_ref(&signal)
                    };
                    for source in sources {
                        if !state.state.sources.contains(source) {
                            if state.state.sources.len() >= 4096 {
                                return Err(RuntimeError::RangeError(
                                    "Abort source limit exceeded".into(),
                                ));
                            }
                            state.source(*source)?;
                            iterator_roots.push_admitted(&vm.heap, *source)?;
                        }
                    }
                }
                for source in state.state.sources.iter().copied() {
                    let mut data = NativeAbortState::copied(vm, source, context.owner.as_ref())?;
                    if data.state.dependents.len() >= 4096 {
                        return Err(RuntimeError::RangeError(
                            "Abort dependency limit exceeded".into(),
                        ));
                    }
                    data.dependent(result)?;
                    data.into_heap(vm, source)?;
                }
                state.into_heap(vm, result)?;
                Ok(Value::Object(result))
            })
        })
    }
    pub(super) fn attach_abort_hook(
        &mut self,
        signal: ObjectId,
        hook: AbortHook,
    ) -> Result<(), RuntimeError> {
        let context = AbortContext::capture(self)?;
        context.with_roots(self, std::iter::once(signal), |vm| {
            let mut state = NativeAbortState::copied(vm, signal, context.owner.as_ref())?;
            if !state.state.aborted() {
                if state.state.hooks.len() >= 4096 {
                    return Err(RuntimeError::RangeError(
                        "Abort algorithm limit exceeded".into(),
                    ));
                }
                state.hook(hook)?;
                state.into_heap(vm, signal)?;
                vm.refresh_abort_timer_roots(signal)?;
            }
            Ok(())
        })
    }

    pub(super) fn detach_abort_hook(
        &mut self,
        signal: ObjectId,
        hook: AbortHook,
    ) -> Result<(), RuntimeError> {
        AbortContext::check_affinity(self)?;
        self.heap.remove_abort_hook(signal, hook)?;
        self.refresh_abort_timer_roots(signal)
    }
    fn signal_abort_copied(&mut self, signal: ObjectId, value: &Value) -> Result<(), RuntimeError> {
        let context = AbortContext::capture(self)?;
        if self.heap.abort_signal(signal)?.aborted() {
            return Ok(());
        }
        let value = NativeValue::copied(value, context.owner.as_ref())?;
        self.signal_abort_input(signal, value, &context)
    }

    pub(super) fn signal_abort(
        &mut self,
        signal: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let context = AbortContext::capture(self)?;
        if self.heap.abort_signal(signal)?.aborted() {
            return Ok(());
        }
        let value = NativeValue::adopt(value, context.owner.as_ref())?;
        self.signal_abort_input(signal, value, &context)
    }

    fn signal_abort_input(
        &mut self,
        signal: ObjectId,
        value: NativeValue,
        context: &AbortContext,
    ) -> Result<(), RuntimeError> {
        context.with_roots(
            self,
            std::iter::once(signal).chain(value.value().object_id()),
            |vm| {
                let reason = vm.abort_reason_owned(value, false, context)?;
                let targets = TransportVector::capture(
                    std::iter::once(signal).chain(
                        vm.heap
                            .abort_signal(signal)?
                            .dependents
                            .iter()
                            .copied()
                            .filter(|dependent| {
                                vm.heap
                                    .abort_signal(*dependent)
                                    .is_ok_and(|state| !state.aborted())
                            }),
                    ),
                    context.owner.as_ref(),
                )?;
                let fixed = targets.len() + usize::from(reason.value().object_id().is_some());
                let roots = RealmCallRoots::new(fixed + 1, context.owner.as_ref())?;
                for target in targets.iter().copied() {
                    roots.push(&vm.heap, target)?;
                }
                if let Some(reason) = reason.value().object_id() {
                    roots.push(&vm.heap, reason)?;
                }
                vm.with_realm_call_roots(roots.clone(), |vm| {
                    for id in targets.iter().copied() {
                        let mut state = NativeAbortState::copied(vm, id, context.owner.as_ref())?;
                        state
                            .reason(NativeValue::copied(reason.value(), context.owner.as_ref())?)?;
                        state.into_heap(vm, id)?;
                    }
                    for id in targets.iter().copied() {
                        context.validate()?;
                        let mut state = NativeAbortState::copied(vm, id, context.owner.as_ref())?;
                        let hooks = state.take_hooks();
                        state.state.sources.clear();
                        state.state.dependents.clear();
                        let hook_roots = hooks.roots(&vm.heap)?;
                        vm.with_realm_call_roots(hook_roots, |vm| {
                            state.into_heap(vm, id)?;
                            for hook in hooks.values.iter().copied() {
                                match hook {
                                    AbortHook::Web { target, serial } => {
                                        if vm.heap.web_event_target(target).is_ok() {
                                            let mut target_state =
                                                storage::NativeAbortTarget::copied(
                                                    vm,
                                                    target,
                                                    context.owner.as_ref(),
                                                )?;
                                            target_state
                                                .state
                                                .listeners
                                                .retain(|listener| listener.serial != serial);
                                            target_state.into_heap(vm, target)?;
                                        }
                                    }
                                    AbortHook::Host { serial } => {
                                        vm.remove_host_listener_serial(serial)?
                                    }
                                }
                            }
                            Ok(())
                        })?;
                        let event = storage::new_abort_event(vm, context.owner.as_ref())?;
                        roots.push(&vm.heap, event)?;
                        vm.finish_web_event(event)?;
                        let fired = vm.dispatch_web_target_event(id, event, true);
                        context.validate().and(fired)?;
                        roots.truncate(fixed);
                        vm.refresh_abort_timer_roots(id)?;
                    }
                    Ok(())
                })
            },
        )
    }
}
