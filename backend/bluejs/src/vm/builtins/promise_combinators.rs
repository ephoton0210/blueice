// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The static Promise combinators: `Promise.all`, `allSettled`, `any`, `race`
//! (ECMA-262 27.2.4) and the keyed forms `allKeyed` / `allSettledKeyed`.
//!
//! All of them share one shape: NewPromiseCapability(C), GetPromiseResolve(C),
//! a loop that calls `C.resolve` on every input and `Invoke`s `then` on the
//! result, and IfAbruptRejectPromise. The per-call bookkeeping the
//! specification keeps in closure captures -- the `values` (or `errors`) list,
//! `remainingElementsCount`, the capability's resolving functions and each
//! element function's [[AlreadyCalled]] flag -- lives in one heap record
//! (`combinator_state`) that every element function references.

use super::*;
use crate::native::PromiseElementKind;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Combinator {
    All,
    AllSettled,
    Any,
    Race,
}

impl Vm {
    pub(in super::super) fn promise_all(
        &mut self,
        constructor: &Value,
        iterable: &Value,
    ) -> Result<Value, RuntimeError> {
        self.promise_combinator(Combinator::All, constructor, iterable)
    }

    pub(in super::super) fn promise_all_settled(
        &mut self,
        constructor: &Value,
        iterable: &Value,
    ) -> Result<Value, RuntimeError> {
        self.promise_combinator(Combinator::AllSettled, constructor, iterable)
    }

    pub(in super::super) fn promise_any(
        &mut self,
        constructor: &Value,
        iterable: &Value,
    ) -> Result<Value, RuntimeError> {
        self.promise_combinator(Combinator::Any, constructor, iterable)
    }

    pub(in super::super) fn promise_race(
        &mut self,
        constructor: &Value,
        iterable: &Value,
    ) -> Result<Value, RuntimeError> {
        self.promise_combinator(Combinator::Race, constructor, iterable)
    }

    /// GetPromiseResolve ( promiseConstructor ).
    fn get_promise_resolve(&mut self, constructor: &Value) -> Result<Value, RuntimeError> {
        let resolve = self.get_property(constructor, &"resolve".into())?;
        if !self.is_callable(&resolve)? {
            return Err(RuntimeError::TypeError(
                "Promise resolve must be callable".into(),
            ));
        }
        Ok(resolve)
    }

    /// The shared tail of a combinator: an abrupt completion rejects the
    /// capability's promise (IfAbruptRejectPromise) instead of escaping.
    fn combinator_finish(
        &mut self,
        capability: &PromiseCapability,
        outcome: Result<Value, RuntimeError>,
    ) -> Result<Value, RuntimeError> {
        match outcome {
            Ok(value) => Ok(value),
            Err(error) => {
                let reason = self.error_value(error)?;
                self.stack.push(reason.clone());
                self.call_native(
                    capability.reject.clone(),
                    Value::Undefined,
                    vec![reason],
                    false,
                )?;
                Ok(capability.promise.clone())
            }
        }
    }

    fn promise_combinator(
        &mut self,
        kind: Combinator,
        constructor: &Value,
        iterable: &Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.extend([constructor.clone(), iterable.clone()]);
        let result = (|| {
            let capability = self.new_promise_capability(constructor)?;
            self.stack.extend([
                capability.promise.clone(),
                capability.resolve.clone(),
                capability.reject.clone(),
            ]);
            let outcome = (|| {
                let promise_resolve = self.get_promise_resolve(constructor)?;
                self.stack.push(promise_resolve.clone());
                let record = self.get_iterator(iterable)?;
                self.stack.push(record.clone());
                let performed = self.perform_combinator(
                    kind,
                    &record,
                    constructor,
                    &capability,
                    &promise_resolve,
                );
                if let Err(error) = &performed {
                    // IteratorClose with the throw completion: `return`'s own
                    // failure never replaces it, and a record whose step
                    // already failed is done and is not closed at all.
                    let close_base = self.stack.len();
                    if let RuntimeError::Thrown(value) = error {
                        self.stack.push(value.clone());
                    }
                    let _ = self.iterator_close(&record);
                    self.stack.truncate(close_base);
                }
                performed
            })();
            self.combinator_finish(&capability, outcome)
        })();
        self.stack.truncate(base);
        result
    }

    /// PerformPromiseAll / AllSettled / Any / Race.
    fn perform_combinator(
        &mut self,
        kind: Combinator,
        record: &Value,
        constructor: &Value,
        capability: &PromiseCapability,
        promise_resolve: &Value,
    ) -> Result<Value, RuntimeError> {
        let state = if kind == Combinator::Race {
            None
        } else {
            Some(self.combinator_state(capability, false)?)
        };
        loop {
            let Some(next) = self.iterator_step(record, true)? else {
                if let Some(state) = state {
                    self.combinator_conclude(kind, state, capability)?;
                }
                return Ok(capability.promise.clone());
            };
            let step_base = self.stack.len();
            self.stack.push(next.clone());
            let step: Result<(), RuntimeError> = (|| {
                let index = match state {
                    Some(state) => Some(self.combinator_reserve(state, None)?),
                    None => None,
                };
                let next_promise = self.call_native(
                    promise_resolve.clone(),
                    constructor.clone(),
                    vec![next],
                    false,
                )?;
                self.stack.push(next_promise.clone());
                let (on_fulfilled, on_rejected) = match (kind, state, index) {
                    (Combinator::All, Some(state), Some(index)) => {
                        let resolve =
                            self.promise_element(state, index, PromiseElementKind::AllResolve)?;
                        (resolve, capability.reject.clone())
                    }
                    (Combinator::AllSettled, Some(state), Some(index)) => {
                        let on_fulfilled = self.promise_element(
                            state,
                            index,
                            PromiseElementKind::AllSettledFulfill,
                        )?;
                        self.stack.push(on_fulfilled.clone());
                        let on_rejected = self.promise_element(
                            state,
                            index,
                            PromiseElementKind::AllSettledReject,
                        )?;
                        (on_fulfilled, on_rejected)
                    }
                    (Combinator::Any, Some(state), Some(index)) => {
                        let reject =
                            self.promise_element(state, index, PromiseElementKind::AnyReject)?;
                        (capability.resolve.clone(), reject)
                    }
                    _ => (capability.resolve.clone(), capability.reject.clone()),
                };
                self.stack
                    .extend([on_fulfilled.clone(), on_rejected.clone()]);
                if let Some(state) = state {
                    self.combinator_adjust_remaining(state, 1)?;
                }
                self.invoke(&next_promise, "then", vec![on_fulfilled, on_rejected])?;
                Ok(())
            })();
            self.stack.truncate(step_base);
            step?;
        }
    }

    /// The end of a combinator's loop: `remainingElementsCount` loses the
    /// count the loop itself held, and reaching zero settles the capability
    /// (for `any` by throwing the AggregateError, which the caller turns into a
    /// rejection).
    fn combinator_conclude(
        &mut self,
        kind: Combinator,
        state: ObjectId,
        capability: &PromiseCapability,
    ) -> Result<(), RuntimeError> {
        if self.combinator_adjust_remaining(state, -1)? != 0.0 {
            return Ok(());
        }
        if kind == Combinator::Any {
            let error = self.combinator_aggregate_error(state)?;
            return Err(RuntimeError::Thrown(error));
        }
        let result = self.combinator_result(state)?;
        self.stack.push(result.clone());
        self.call_native(
            capability.resolve.clone(),
            Value::Undefined,
            vec![result],
            false,
        )?;
        Ok(())
    }

    /// `Promise.allKeyed ( promises )` / `Promise.allSettledKeyed ( promises )`.
    pub(in super::super) fn promise_all_keyed(
        &mut self,
        settled: bool,
        constructor: &Value,
        promises: &Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.extend([constructor.clone(), promises.clone()]);
        let result = (|| {
            let capability = self.new_promise_capability(constructor)?;
            self.stack.extend([
                capability.promise.clone(),
                capability.resolve.clone(),
                capability.reject.clone(),
            ]);
            let outcome = (|| {
                let promise_resolve = self.get_promise_resolve(constructor)?;
                self.stack.push(promise_resolve.clone());
                let Value::Object(source) = promises else {
                    return Err(RuntimeError::TypeError(
                        "Promise keyed combinators require an object".into(),
                    ));
                };
                self.perform_keyed_combinator(
                    settled,
                    *source,
                    constructor,
                    &capability,
                    &promise_resolve,
                )
            })();
            self.combinator_finish(&capability, outcome)
        })();
        self.stack.truncate(base);
        result
    }

    /// PerformPromiseAllKeyed ( variant, promises, constructor, resultCapability,
    /// promiseResolve ).
    fn perform_keyed_combinator(
        &mut self,
        settled: bool,
        promises: ObjectId,
        constructor: &Value,
        capability: &PromiseCapability,
        promise_resolve: &Value,
    ) -> Result<Value, RuntimeError> {
        let keys = self.object_own_property_keys(promises)?;
        let state = self.combinator_state(capability, true)?;
        for key in keys {
            let Some(descriptor) = self.object_get_own_property(promises, &key)? else {
                continue;
            };
            if descriptor.enumerable != Some(true) {
                continue;
            }
            let step_base = self.stack.len();
            let step: Result<(), RuntimeError> = (|| {
                let value = self.get_property(&Value::Object(promises), &key)?;
                self.stack.push(value.clone());
                let key_value = match &key {
                    PropertyName::String(text) => Value::String(text.clone()),
                    PropertyName::Symbol(symbol) => Value::Symbol(symbol.clone()),
                };
                let index = self.combinator_reserve(state, Some(key_value))?;
                let next_promise = self.call_native(
                    promise_resolve.clone(),
                    constructor.clone(),
                    vec![value],
                    false,
                )?;
                self.stack.push(next_promise.clone());
                let (on_fulfilled, on_rejected) = if settled {
                    let on_fulfilled =
                        self.promise_element(state, index, PromiseElementKind::AllSettledFulfill)?;
                    self.stack.push(on_fulfilled.clone());
                    let on_rejected =
                        self.promise_element(state, index, PromiseElementKind::AllSettledReject)?;
                    (on_fulfilled, on_rejected)
                } else {
                    let on_fulfilled =
                        self.promise_element(state, index, PromiseElementKind::AllResolve)?;
                    (on_fulfilled, capability.reject.clone())
                };
                self.stack
                    .extend([on_fulfilled.clone(), on_rejected.clone()]);
                self.combinator_adjust_remaining(state, 1)?;
                self.invoke(&next_promise, "then", vec![on_fulfilled, on_rejected])?;
                Ok(())
            })();
            self.stack.truncate(step_base);
            step?;
        }
        self.combinator_conclude(Combinator::All, state, capability)?;
        Ok(capability.promise.clone())
    }

    /// The heap record behind one combinator call: the `values` (or `errors`)
    /// list, the optional `keys` list of the keyed forms, `remainingElements-
    /// Count` (starting at 1 for the loop itself) and the capability's
    /// resolving functions. Left pushed on the VM stack for the caller.
    fn combinator_state(
        &mut self,
        capability: &PromiseCapability,
        keyed: bool,
    ) -> Result<ObjectId, RuntimeError> {
        let state = self.promise_state()?;
        self.stack.push(Value::Object(state));
        let values = self.promise_state()?;
        self.promise_state_set(state, "values", Value::Object(values))?;
        if keyed {
            let keys = self.promise_state()?;
            self.promise_state_set(state, "keys", Value::Object(keys))?;
        }
        self.promise_state_set(state, "count", Value::Number(0.0))?;
        self.promise_state_set(state, "remaining", Value::Number(1.0))?;
        self.promise_state_set(state, "resolve", capability.resolve.clone())?;
        self.promise_state_set(state, "reject", capability.reject.clone())?;
        Ok(state)
    }

    /// Appends an `undefined` placeholder to the value list (and the key to
    /// the key list of a keyed call) and returns its index.
    fn combinator_reserve(
        &mut self,
        state: ObjectId,
        key: Option<Value>,
    ) -> Result<u32, RuntimeError> {
        let count = self.combinator_count(state)?;
        let values = self.combinator_list(state, "values")?;
        self.promise_state_set(values, count.to_string(), Value::Undefined)?;
        if let Some(key) = key {
            let keys = self.combinator_list(state, "keys")?;
            self.promise_state_set(keys, count.to_string(), key)?;
        }
        self.promise_state_set(state, "count", Value::Number(f64::from(count) + 1.0))?;
        Ok(count)
    }

    fn combinator_count(&self, state: ObjectId) -> Result<u32, RuntimeError> {
        match self.promise_state_get(state, "count")? {
            Value::Number(count) => Ok(count as u32),
            _ => unreachable!("combinator state always holds a count"),
        }
    }

    fn combinator_list(&self, state: ObjectId, name: &str) -> Result<ObjectId, RuntimeError> {
        self.promise_state_get(state, name)?
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("combinator state has no such list".into()))
    }

    /// Adds `delta` to `remainingElementsCount` and returns the new value.
    fn combinator_adjust_remaining(
        &mut self,
        state: ObjectId,
        delta: i32,
    ) -> Result<f64, RuntimeError> {
        let Value::Number(current) = self.promise_state_get(state, "remaining")? else {
            unreachable!("combinator state always holds a remaining count");
        };
        let updated = current + f64::from(delta);
        self.promise_state_set(state, "remaining", Value::Number(updated))?;
        Ok(updated)
    }

    /// A resolve/reject element function for the entry at `index`.
    fn promise_element(
        &mut self,
        state: ObjectId,
        index: u32,
        kind: PromiseElementKind,
    ) -> Result<Value, RuntimeError> {
        self.promise_native_function(NativeFunction::PromiseElement { state, index, kind }, 1)
    }

    /// The body of an element function: once per index (the two element
    /// functions of an `allSettled` entry share the flag), record the value
    /// and, when it was the last outstanding one, settle the capability.
    pub(in super::super) fn promise_element_function(
        &mut self,
        state: ObjectId,
        index: u32,
        kind: PromiseElementKind,
        argument: &Value,
    ) -> Result<Value, RuntimeError> {
        let flag = format!("called{index}");
        if self.promise_state_get(state, &flag)? == Value::Bool(true) {
            return Ok(Value::Undefined);
        }
        self.promise_state_set(state, flag, Value::Bool(true))?;
        let base = self.stack.len();
        self.stack.push(argument.clone());
        let result = (|| {
            let stored = match kind {
                PromiseElementKind::AllResolve | PromiseElementKind::AnyReject => argument.clone(),
                PromiseElementKind::AllSettledFulfill => {
                    self.promise_settlement_record(true, argument.clone())?
                }
                PromiseElementKind::AllSettledReject => {
                    self.promise_settlement_record(false, argument.clone())?
                }
            };
            self.stack.push(stored.clone());
            let values = self.combinator_list(state, "values")?;
            self.promise_state_set(values, index.to_string(), stored)?;
            if self.combinator_adjust_remaining(state, -1)? != 0.0 {
                return Ok(Value::Undefined);
            }
            if kind == PromiseElementKind::AnyReject {
                let error = self.combinator_aggregate_error(state)?;
                self.stack.push(error.clone());
                let reject = self.promise_state_get(state, "reject")?;
                return self.call_native(reject, Value::Undefined, vec![error], false);
            }
            let result = self.combinator_result(state)?;
            self.stack.push(result.clone());
            let resolve = self.promise_state_get(state, "resolve")?;
            self.call_native(resolve, Value::Undefined, vec![result], false)
        })();
        self.stack.truncate(base);
        result
    }

    /// The recorded values, in index order.
    fn combinator_values(&self, state: ObjectId) -> Result<Vec<Value>, RuntimeError> {
        let count = self.combinator_count(state)?;
        let values = self.combinator_list(state, "values")?;
        (0..count)
            .map(|index| self.promise_state_get(values, &index.to_string()))
            .collect()
    }

    /// CreateArrayFromList(values), or for a keyed call
    /// CreateKeyedPromiseCombinatorResultObject: a null-prototype object with
    /// one data property per recorded key.
    fn combinator_result(&mut self, state: ObjectId) -> Result<Value, RuntimeError> {
        let values = self.combinator_values(state)?;
        let Some(keys) = self.promise_state_get(state, "keys")?.object_id() else {
            return self.array_from(values);
        };
        let object = self.with_roots(|heap| heap.alloc_object(None))?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            for (index, value) in values.into_iter().enumerate() {
                let key = self.promise_state_get(keys, &index.to_string())?;
                let key = self.coerce_property_key(&key)?;
                self.define_data(object, key, value, true, true, true)?;
            }
            Ok(Value::Object(object))
        })();
        self.stack.pop();
        result
    }

    /// A newly created AggregateError whose `errors` is the recorded list.
    fn combinator_aggregate_error(&mut self, state: ObjectId) -> Result<Value, RuntimeError> {
        let errors = self.combinator_values(state)?;
        let base = self.stack.len();
        self.stack.extend(errors.iter().cloned());
        let result = (|| {
            let list = self.array_from(errors)?;
            self.stack.push(list.clone());
            let constructor = self.error_global("AggregateError")?;
            let prototype = self
                .get_property(&constructor, &"prototype".into())?
                .object_id();
            let error = self.with_roots(|heap| heap.alloc_error(prototype))?;
            self.stack.push(Value::Object(error));
            self.define_data(error, "errors", list, true, false, true)?;
            Ok(Value::Object(error))
        })();
        self.stack.truncate(base);
        result
    }
}
