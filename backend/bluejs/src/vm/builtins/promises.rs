// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn promise_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.promise_prototype {
            return Ok(prototype);
        }
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let function_prototype = self.function_prototype()?;
        self.install_native(
            prototype,
            function_prototype,
            "then",
            2,
            NativeFunction::PromiseThen,
        )?;
        self.install_native(
            prototype,
            function_prototype,
            "catch",
            1,
            NativeFunction::PromiseCatch,
        )?;
        self.install_native(
            prototype,
            function_prototype,
            "finally",
            1,
            NativeFunction::PromiseFinally,
        )?;
        self.promise_prototype = Some(prototype);
        Ok(prototype)
    }

    /// Map and Set have distinct ordinary prototypes.  This shared bootstrap
    /// keeps constructor/new-target inheritance correct before collection
    /// entries and iterators are introduced.
    pub(in super::super) fn collection_prototype(
        &mut self,
        map: bool,
    ) -> Result<ObjectId, RuntimeError> {
        let cached = if map {
            self.map_prototype
        } else {
            self.set_prototype
        };
        if let Some(prototype) = cached {
            return Ok(prototype);
        }
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        self.stack.push(Value::Object(prototype));
        let result = self.define_data(
            prototype,
            JsSymbol::well_known("toStringTag"),
            Value::String(if map { "Map" } else { "Set" }.into()),
            false,
            false,
            true,
        );
        self.stack.pop();
        result?;
        if map {
            self.map_prototype = Some(prototype);
        } else {
            self.set_prototype = Some(prototype);
        }
        Ok(prototype)
    }

    pub(in super::super) fn collection_constructor(
        &mut self,
        map: bool,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                if map {
                    "Map constructor must be called with new"
                } else {
                    "Set constructor must be called with new"
                }
                .into(),
            ));
        }
        let default = self.collection_prototype(map)?;
        let prototype = self.constructor_prototype(default)?;
        let collection = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(collection));
        let result = self.define_data(collection, "size", Value::Number(0.0), false, false, true);
        self.stack.pop();
        result?;
        Ok(Value::Object(collection))
    }

    pub(in super::super) fn new_promise(&mut self) -> Result<ObjectId, RuntimeError> {
        let prototype = self.promise_prototype()?;
        let promise = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.promises.insert(
            promise,
            PromiseRecord {
                status: PromiseStatus::Pending,
                reactions: Vec::new(),
            },
        );
        Ok(promise)
    }

    /// Create one of the resolving functions belonging to a Promise
    /// capability. Their target is private native-function state rather than
    /// a JavaScript-visible property, which keeps `resolve.call(...)` and
    /// `reject.call(...)` correct.
    pub(in super::super) fn promise_resolving_function(
        &mut self,
        promise: ObjectId,
        fulfill: bool,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.function_prototype()?;
        let function = NativeFunction::PromiseResolvingFunction { promise, fulfill };
        let id = self.with_roots(|heap| heap.alloc_native_function(function, "", prototype))?;
        self.stack.push(Value::Object(id));
        let result = (|| {
            self.define_data(id, "name", Value::String("".into()), false, false, true)?;
            self.define_data(id, "length", Value::Number(1.0), false, false, true)?;
            Ok(Value::Object(id))
        })();
        self.stack.pop();
        result
    }

    /// Execute `NewPromiseCapability(C)` for a constructor supplied to a
    /// static Promise method. The executor's captured resolve/reject pair is
    /// stored in a heap object because a user constructor receives it through
    /// normal JavaScript invocation rather than a private VM call path.
    pub(in super::super) fn new_promise_capability(
        &mut self,
        constructor: &Value,
    ) -> Result<(Value, Value, Value), RuntimeError> {
        if !self.is_constructor(constructor)? {
            return Err(RuntimeError::TypeError(
                "Promise constructor must be a constructor".into(),
            ));
        }
        let base = self.stack.len();
        let storage = self.with_roots(|heap| heap.alloc_object(None))?;
        self.stack.push(Value::Object(storage));
        let result = (|| {
            let prototype = self.function_prototype()?;
            let executor = self.with_roots(|heap| {
                heap.alloc_native_function(
                    NativeFunction::PromiseCapabilityExecutor { storage },
                    "",
                    prototype,
                )
            })?;
            self.stack.push(Value::Object(executor));
            self.define_data(
                executor,
                "name",
                Value::String("".into()),
                false,
                false,
                true,
            )?;
            self.define_data(executor, "length", Value::Number(2.0), false, false, true)?;
            let promise = self.call_native(
                constructor.clone(),
                Value::Undefined,
                vec![Value::Object(executor)],
                true,
            )?;
            let Value::Object(_) = promise else {
                return Err(RuntimeError::TypeError(
                    "Promise constructor must return an object".into(),
                ));
            };
            self.stack.push(promise.clone());
            let resolve = self.get_property(&Value::Object(storage), &"resolve".into())?;
            let reject = self.get_property(&Value::Object(storage), &"reject".into())?;
            if !self.is_callable(&resolve)? || !self.is_callable(&reject)? {
                return Err(RuntimeError::TypeError(
                    "Promise constructor did not provide resolving functions".into(),
                ));
            }
            Ok((promise, resolve, reject))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn promise_resolve_constructor(
        &mut self,
        constructor: &Value,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        if value
            .object_id()
            .is_some_and(|promise| self.promises.contains_key(&promise))
            && self.get_property(&value, &"constructor".into())? == *constructor
        {
            return Ok(value);
        }
        let (promise, resolve, _) = self.new_promise_capability(constructor)?;
        let base = self.stack.len();
        self.stack
            .extend([promise.clone(), resolve.clone(), value.clone()]);
        let result = self.call_native(resolve, Value::Undefined, vec![value], false);
        self.stack.truncate(base);
        result?;
        Ok(promise)
    }

    pub(in super::super) fn promise_constructor(
        &mut self,
        executor: Value,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Promise constructor must be called with new".into(),
            ));
        }
        if !self.is_callable(&executor)? {
            return Err(RuntimeError::TypeError(
                "Promise resolver is not a function".into(),
            ));
        }
        let promise = self.new_promise()?;
        // The capability must survive allocations for its resolving functions
        // and for executor invocation.
        self.stack.push(Value::Object(promise));
        let result = (|| {
            let resolve = self.promise_resolving_function(promise, true)?;
            let reject = self.promise_resolving_function(promise, false)?;
            match self.call_native(executor, Value::Undefined, vec![resolve, reject], false) {
                Ok(_) => {}
                Err(RuntimeError::Thrown(value)) => {
                    self.settle_promise(promise, PromiseStatus::Rejected(value))?;
                }
                Err(error) => {
                    let value = self.error_value(error)?;
                    self.settle_promise(promise, PromiseStatus::Rejected(value))?;
                }
            }
            Ok(Value::Object(promise))
        })();
        self.stack.pop();
        result
    }

    pub(in super::super) fn promise_with_resolvers(&mut self) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let promise = self.new_promise()?;
        self.stack.push(Value::Object(promise));
        let result = (|| {
            let resolve = self.promise_resolving_function(promise, true)?;
            let reject = self.promise_resolving_function(promise, false)?;
            self.stack.extend([resolve.clone(), reject.clone()]);
            let object_prototype = self.object_prototype;
            let capability = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
            self.stack.push(Value::Object(capability));
            self.define_data(
                capability,
                "promise",
                Value::Object(promise),
                true,
                true,
                true,
            )?;
            self.define_data(capability, "resolve", resolve, true, true, true)?;
            self.define_data(capability, "reject", reject, true, true, true)?;
            Ok(Value::Object(capability))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn settle_promise(
        &mut self,
        promise: ObjectId,
        status: PromiseStatus,
    ) -> Result<(), RuntimeError> {
        let record = self
            .promises
            .get_mut(&promise)
            .ok_or(RuntimeError::TypeError("invalid Promise receiver".into()))?;
        if !matches!(record.status, PromiseStatus::Pending) {
            return Ok(());
        }
        let fulfilled = matches!(status, PromiseStatus::Fulfilled(_));
        let value = match &status {
            PromiseStatus::Fulfilled(value) | PromiseStatus::Rejected(value) => value.clone(),
            PromiseStatus::Pending => unreachable!("Promise settlement is final"),
        };
        let reactions = std::mem::take(&mut record.reactions);
        record.status = status;
        self.promise_jobs
            .extend(reactions.into_iter().map(|reaction| match reaction {
                PromiseReaction::Then(reaction) => PromiseJob::Reaction {
                    target: reaction.target,
                    handler: if fulfilled {
                        reaction.on_fulfilled
                    } else {
                        reaction.on_rejected
                    },
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::ModuleAwait { continuation } => PromiseJob::ModuleAwait {
                    continuation,
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::AsyncAwait { continuation } => PromiseJob::AsyncAwait {
                    continuation,
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::AsyncGeneratorYield {
                    generator,
                    target,
                    result,
                } => PromiseJob::AsyncGeneratorYield {
                    generator,
                    target,
                    result,
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::AsyncGeneratorDelegate {
                    generator,
                    target,
                    kind,
                } => PromiseJob::AsyncGeneratorDelegate {
                    generator,
                    target,
                    kind,
                    value: value.clone(),
                    fulfilled,
                },
            }));
        Ok(())
    }

    /// Resolve a capability from a value returned by user code. Promise
    /// reactions adopt another BlueJS Promise instead of fulfilling with the
    /// Promise object itself, which is essential for `then`, async functions
    /// and top-level await.
    pub(in super::super) fn resolve_promise(
        &mut self,
        promise: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        if value.object_id() == Some(promise) {
            let error = self.error_object("TypeError", "Promise resolved with itself".into())?;
            return self.settle_promise(promise, PromiseStatus::Rejected(error));
        }
        let resolution = self.promise_resolve(value)?;
        let Value::Object(resolved) = resolution else {
            unreachable!("Promise.resolve always returns a promise")
        };
        let status = match self.promises.get(&resolved) {
            Some(PromiseRecord {
                status: PromiseStatus::Fulfilled(value),
                ..
            }) => Some((true, value.clone())),
            Some(PromiseRecord {
                status: PromiseStatus::Rejected(value),
                ..
            }) => Some((false, value.clone())),
            Some(PromiseRecord {
                status: PromiseStatus::Pending,
                ..
            }) => None,
            None => unreachable!("Promise.resolve returns a registered promise"),
        };
        if let Some((fulfilled, value)) = status {
            return self.settle_promise(
                promise,
                if fulfilled {
                    PromiseStatus::Fulfilled(value)
                } else {
                    PromiseStatus::Rejected(value)
                },
            );
        }
        self.promises
            .get_mut(&resolved)
            .expect("checked pending promise exists")
            .reactions
            .push(PromiseReaction::Then(PromiseThenReaction {
                target: promise,
                on_fulfilled: Value::Undefined,
                on_rejected: Value::Undefined,
            }));
        Ok(())
    }

    pub(in super::super) fn promise_then(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let promise = receiver
            .object_id()
            .filter(|id| self.promises.contains_key(id))
            .ok_or(RuntimeError::TypeError(
                "Promise.prototype.then receiver".into(),
            ))?;
        let target = self.new_promise()?;
        let reaction = PromiseThenReaction {
            target,
            on_fulfilled: native::argument(args, 0).clone(),
            on_rejected: native::argument(args, 1).clone(),
        };
        let status = {
            let record = self.promises.get_mut(&promise).unwrap();
            match &record.status {
                PromiseStatus::Pending => {
                    record.reactions.push(PromiseReaction::Then(reaction));
                    return Ok(Value::Object(target));
                }
                PromiseStatus::Fulfilled(value) => (true, value.clone()),
                PromiseStatus::Rejected(value) => (false, value.clone()),
            }
        };
        self.promise_jobs.push_back(PromiseJob::Reaction {
            target,
            handler: if status.0 {
                reaction.on_fulfilled
            } else {
                reaction.on_rejected
            },
            value: status.1,
            fulfilled: status.0,
        });
        Ok(Value::Object(target))
    }

    pub(in super::super) fn promise_catch(
        &mut self,
        receiver: &Value,
        reason_handler: &Value,
    ) -> Result<Value, RuntimeError> {
        self.promise_then(receiver, &[Value::Undefined, reason_handler.clone()])
    }

    pub(in super::super) fn promise_finally(
        &mut self,
        receiver: &Value,
        handler: &Value,
    ) -> Result<Value, RuntimeError> {
        // A non-callable handler already passes the original completion
        // through in `promise_then`. Callable handlers run on both paths;
        // reaction adoption is handled by the shared job queue.
        self.promise_then(receiver, &[handler.clone(), handler.clone()])
    }

    /// Implements the settled-value portion of Await. A pending promise needs
    /// a saved interpreter continuation, which remains a separate boundary.
    pub(in super::super) fn await_value(&self, value: Value) -> Result<Value, RuntimeError> {
        let Some(promise) = value.object_id() else {
            return Ok(value);
        };
        let Some(record) = self.promises.get(&promise) else {
            return Ok(value);
        };
        match &record.status {
            PromiseStatus::Pending => Err(RuntimeError::Unsupported("pending await continuation")),
            PromiseStatus::Fulfilled(value) => Ok(value.clone()),
            PromiseStatus::Rejected(value) => Err(RuntimeError::Thrown(value.clone())),
        }
    }

    pub(in super::super) fn promise_resolve(
        &mut self,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        // PromiseResolve(%Promise%, value) may return a native Promise only
        // after it observes `value.constructor`. That lookup is observable
        // (and may throw), including through AsyncFromSyncIteratorContinuation.
        // Keep `value` rooted while lazy intrinsic initialization or thenable
        // lookup can allocate.
        let base = self.stack.len();
        self.stack.push(value.clone());
        let outcome = (|| {
            let constructor = self.global("Promise")?;
            if value
                .object_id()
                .is_some_and(|promise| self.promises.contains_key(&promise))
                && self.get_property(&value, &"constructor".into())? == constructor
            {
                return Ok(value);
            }
            let promise = self.new_promise()?;
            let then = match &value {
                Value::Object(_) => self.get_property(&value, &"then".into()),
                _ => Ok(Value::Undefined),
            };
            match then {
                Ok(then) if self.is_callable(&then)? => {
                    self.promise_jobs.push_back(PromiseJob::Thenable {
                        target: promise,
                        thenable: value,
                        then,
                    });
                }
                Ok(_) => self.settle_promise(promise, PromiseStatus::Fulfilled(value))?,
                Err(error) => {
                    let error = self.error_value(error)?;
                    self.settle_promise(promise, PromiseStatus::Rejected(error))?;
                }
            }
            Ok(Value::Object(promise))
        })();
        self.stack.truncate(base);
        outcome
    }

    pub(in super::super) fn promise_reject(&mut self, value: Value) -> Result<Value, RuntimeError> {
        let promise = self.new_promise()?;
        self.settle_promise(promise, PromiseStatus::Rejected(value))?;
        Ok(Value::Object(promise))
    }

    pub(in super::super) fn promise_all_handler(
        &mut self,
        target: ObjectId,
        index: Option<u32>,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.function_prototype()?;
        let function = match index {
            Some(index) => NativeFunction::PromiseAllResolve { target, index },
            None => NativeFunction::PromiseAllReject { target },
        };
        let id = self.with_roots(|heap| heap.alloc_native_function(function, "", prototype))?;
        self.stack.push(Value::Object(id));
        let result = (|| {
            self.define_data(id, "name", Value::String("".into()), false, false, true)?;
            self.define_data(id, "length", Value::Number(1.0), false, false, true)?;
            Ok(Value::Object(id))
        })();
        self.stack.pop();
        result
    }

    pub(in super::super) fn promise_all_settled(
        &mut self,
        target: ObjectId,
        index: u32,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let complete = {
            let Some(state) = self.promise_all.get_mut(&target) else {
                return Ok(());
            };
            let slot = state
                .values
                .get_mut(index as usize)
                .expect("Promise.all reaction index was allocated");
            if slot.is_some() {
                return Ok(());
            }
            *slot = Some(value);
            state.remaining -= 1;
            (state.remaining == 0).then(|| {
                state
                    .values
                    .iter()
                    .cloned()
                    .map(|value| value.expect("completed Promise.all has every value"))
                    .collect::<Vec<_>>()
            })
        };
        let Some(values) = complete else {
            return Ok(());
        };
        self.promise_all.remove(&target);
        let base = self.stack.len();
        self.stack.extend(values.iter().cloned());
        let values = self.array_from(values);
        self.stack.truncate(base);
        let values = values?;
        self.settle_promise(target, PromiseStatus::Fulfilled(values))
    }

    pub(in super::super) fn promise_all_reject(
        &mut self,
        target: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        if self.promise_all.remove(&target).is_some() {
            self.settle_promise(target, PromiseStatus::Rejected(value))?;
        }
        Ok(())
    }

    pub(in super::super) fn promise_all(
        &mut self,
        constructor: &Value,
        values: &Value,
    ) -> Result<Value, RuntimeError> {
        let values = self.array_like_values(values)?;
        let promise = self.new_promise()?;
        let base = self.stack.len();
        self.stack.push(Value::Object(promise));
        let outcome = (|| {
            // PerformPromiseAll observes `C.resolve` once before consuming
            // inputs. Calling the internal resolver here used to hide a
            // getter throw and leave the returned aggregate pending forever.
            let resolve = self.get_property(constructor, &"resolve".into())?;
            if !self.is_callable(&resolve)? {
                return Err(RuntimeError::TypeError(
                    "Promise.all resolve must be callable".into(),
                ));
            }
            if values.is_empty() {
                let values = self.array_from(Vec::new())?;
                self.settle_promise(promise, PromiseStatus::Fulfilled(values))?;
                return Ok(Value::Object(promise));
            }
            self.promise_all.insert(
                promise,
                PromiseAllState {
                    values: vec![None; values.len()],
                    remaining: values.len(),
                },
            );
            for (index, value) in values.into_iter().enumerate() {
                let input =
                    self.call_native(resolve.clone(), constructor.clone(), vec![value], false)?;
                let fulfilled = self.promise_all_handler(promise, Some(index as u32))?;
                let rejected = self.promise_all_handler(promise, None)?;
                // Invoke rather than internally attaching a reaction: an
                // own `then` getter/method on the resolved value is part of
                // Promise.all's observable error surface.
                let then = self.get_property(&input, &"then".into())?;
                self.call_native(then, input, vec![fulfilled, rejected], false)?;
            }
            Ok(Value::Object(promise))
        })();
        self.stack.truncate(base);
        match outcome {
            Ok(value) => Ok(value),
            Err(error) => {
                self.promise_all.remove(&promise);
                let value = self.error_value(error)?;
                self.settle_promise(promise, PromiseStatus::Rejected(value))?;
                Ok(Value::Object(promise))
            }
        }
    }

    pub fn run_promise_jobs(&mut self) -> Result<(), RuntimeError> {
        while !self.promise_jobs.is_empty() {
            // Each queued Promise reaction has its own execution context and
            // therefore its own instruction budget. A module continuation
            // deliberately leaves the ambient interpreter with zero fuel
            // while suspended; it must not starve the next job turn.
            self.remaining_instructions = self.config.instruction_budget;
            self.run_next_promise_job()?;
        }
        Ok(())
    }

    /// Execute exactly one Promise job.  Async functions and top-level await
    /// resume from a queued continuation, rather than draining later turns
    /// in the same checkpoint, so callers that need an await boundary can
    /// advance the queue one observable turn at a time.
    pub(in super::super) fn run_next_promise_job(&mut self) -> Result<bool, RuntimeError> {
        let Some(job) = self.promise_jobs.pop_front() else {
            return Ok(false);
        };
        match job {
            PromiseJob::Reaction {
                target,
                handler,
                value,
                fulfilled,
            } => {
                if !self.is_callable(&handler)? {
                    self.settle_promise(
                        target,
                        if fulfilled {
                            PromiseStatus::Fulfilled(value)
                        } else {
                            PromiseStatus::Rejected(value)
                        },
                    )?;
                    return Ok(true);
                }
                let result = self.call_native(handler, Value::Undefined, vec![value], false);
                match result {
                    Ok(value) => self.resolve_promise(target, value)?,
                    Err(error) => {
                        let error = self.error_value(error)?;
                        self.settle_promise(target, PromiseStatus::Rejected(error))?;
                    }
                }
            }
            PromiseJob::Thenable {
                target,
                thenable,
                then,
            } => {
                let result = (|| {
                    let resolve = self.promise_resolving_function(target, true)?;
                    let reject = self.promise_resolving_function(target, false)?;
                    self.call_native(then, thenable, vec![resolve, reject], false)
                })();
                if let Err(error) = result {
                    let error = self.error_value(error)?;
                    self.settle_promise(target, PromiseStatus::Rejected(error))?;
                }
            }
            PromiseJob::DynamicImport {
                target,
                referrer,
                specifier,
            } => {
                let result = self.dynamic_import_job(&referrer, &specifier);
                match result {
                    Ok(DynamicImportResult::Fulfilled(namespace)) => {
                        self.settle_promise(target, PromiseStatus::Fulfilled(namespace))?
                    }
                    Ok(DynamicImportResult::Waiting(module)) => {
                        self.module_import_waiters
                            .entry(module)
                            .or_default()
                            .push(target);
                    }
                    // Dynamic import delegates loading and linking to the
                    // host. A host module-resolution failure rejects the
                    // capability with its host error rather than leaking
                    // the static-module SyntaxError classification.
                    Err(RuntimeError::ModuleResolution(message)) => {
                        let error = self.error_object("TypeError", message)?;
                        self.settle_promise(target, PromiseStatus::Rejected(error))?;
                    }
                    Err(error) => {
                        let error = self.error_value(error)?;
                        self.settle_promise(target, PromiseStatus::Rejected(error))?;
                    }
                }
            }
            PromiseJob::ModuleAwait {
                continuation,
                value,
                fulfilled,
            } => self.resume_module_await(continuation, value, fulfilled)?,
            PromiseJob::AsyncAwait {
                continuation,
                value,
                fulfilled,
            } => self.resume_async_await(continuation, value, fulfilled)?,
            PromiseJob::AsyncGeneratorYield {
                generator,
                target,
                result,
                value,
                fulfilled,
            } => self.finish_async_generator_yield(generator, target, result, value, fulfilled)?,
            PromiseJob::AsyncGeneratorDelegate {
                generator,
                target,
                kind,
                value,
                fulfilled,
            } => self.finish_async_generator_delegate(generator, target, kind, value, fulfilled)?,
        }
        Ok(true)
    }
}
