// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Array.fromAsync`. The specification runs its body as an async closure, so
//! every `Await` is a suspension. Native code cannot contain a bytecode
//! `Await`, so the run is a small state machine: a heap-resident state record
//! (holding the result promise, the result array, the iterator record, the
//! mapper and the position) is threaded through native `then` handlers, one
//! per Await, each of which calls [`Vm::array_from_async_resume`]. The state
//! record is reachable from the handlers' payload, so nothing the run holds
//! between suspensions is unrooted.
use super::*;

/// What the pending Await is waiting for.
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    /// `Await(Call(next))` of the iterator loop.
    IteratorResult = 0,
    /// `Await(mappedValue)` of the iterator loop.
    IteratorMapped = 1,
    /// `Await(Get(arrayLike, Pk))`.
    ElementValue = 2,
    /// `Await(mappedValue)` of the array-like loop.
    ElementMapped = 3,
    /// `Await` of the iterator's `return()` result while closing after a
    /// throw completion; the stored error is what the run finally rejects with.
    Closing = 4,
}

impl Phase {
    fn from_number(value: f64) -> Phase {
        match value as u8 {
            0 => Phase::IteratorResult,
            1 => Phase::IteratorMapped,
            2 => Phase::ElementValue,
            3 => Phase::ElementMapped,
            _ => Phase::Closing,
        }
    }
}

impl Vm {
    fn fa_get(&self, state: ObjectId, name: &str) -> Value {
        self.record_get(state, name).unwrap_or(Value::Undefined)
    }

    fn fa_set(&mut self, state: ObjectId, name: &str, value: Value) -> Result<(), RuntimeError> {
        // Storing a property can allocate; keep the value reachable meanwhile.
        self.stack.push(value.clone());
        let stored = self.with_roots(|heap| heap.set(state, name, value));
        self.stack.pop();
        stored
    }

    /// Adds fields to the state record, each of which grows it.
    fn fa_init(&mut self, state: ObjectId, fields: &[(&str, Value)]) -> Result<(), RuntimeError> {
        for (name, value) in fields {
            self.fa_set(state, name, value.clone())?;
        }
        Ok(())
    }

    fn fa_number(&self, state: ObjectId, name: &str) -> f64 {
        self.record_count(state, name) as f64
    }

    fn fa_promise(&self, state: ObjectId) -> ObjectId {
        self.fa_get(state, "promise")
            .object_id()
            .expect("the fromAsync state keeps its promise")
    }

    /// `Array.fromAsync(asyncItems [, mapfn [, thisArg]])`.
    pub(in super::super) fn array_from_async(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let promise = self.new_promise()?;
        let base = self.stack.len();
        self.stack.push(Value::Object(promise));
        self.stack.push(receiver.clone());
        self.stack.extend(args.iter().cloned());
        let outcome = self.array_from_async_start(promise, receiver, args);
        self.stack.truncate(base);
        if let Err(error) = outcome {
            let error = self.error_value(error)?;
            self.settle_tracked_promise(promise, PromiseStatus::Rejected(error));
        }
        Ok(Value::Object(promise))
    }

    /// Everything up to the first Await; an error here rejects the promise.
    fn array_from_async_start(
        &mut self,
        promise: ObjectId,
        receiver: &Value,
        args: &[Value],
    ) -> Result<(), RuntimeError> {
        let items = native::argument(args, 0).clone();
        let mapper = native::argument(args, 1).clone();
        let this_arg = native::argument(args, 2).clone();
        if mapper != Value::Undefined && !self.callable(&mapper) {
            return Err(RuntimeError::TypeError(
                "Array.fromAsync mapper must be callable".into(),
            ));
        }
        let using_async = self.get_method(&items, &JsSymbol::well_known("asyncIterator").into())?;
        self.stack.push(using_async.clone());
        let using_sync = if using_async == Value::Undefined {
            self.get_method(&items, &JsSymbol::well_known("iterator").into())?
        } else {
            Value::Undefined
        };
        let constructor = self.constructible(receiver);
        self.stack.push(using_sync.clone());
        // GetIteratorFromMethod (calling the chosen iterator method) precedes
        // Construct(C), unlike the synchronous Array.from.
        let record = if using_async != Value::Undefined {
            Some(self.async_iterator_record_from_method(&items, using_async)?)
        } else if using_sync != Value::Undefined {
            let record = self.get_iterator_from_method(&items, using_sync)?;
            Some(self.mark_async_from_sync(record)?)
        } else {
            None
        };
        if let Some(record) = &record {
            self.stack.push(record.clone());
        }
        let array = match record {
            Some(_) => Some(self.array_from_target(receiver, constructor, None)?),
            None => None,
        };
        if let Some(array) = array {
            self.stack.push(Value::Object(array));
        }
        let state = self.with_roots(|heap| heap.alloc_object(None))?;
        self.stack.push(Value::Object(state));
        // `k` and `phase` are added now so that later updates only overwrite
        // a number with a number, which cannot grow the record.
        self.fa_init(
            state,
            &[
                ("promise", Value::Object(promise)),
                ("mapper", mapper),
                ("thisArg", this_arg),
                ("k", Value::Number(0.0)),
                ("phase", Value::Number(0.0)),
            ],
        )?;
        match record.zip(array) {
            Some((record, array)) => {
                self.fa_init(
                    state,
                    &[("array", Value::Object(array)), ("record", record)],
                )?;
                self.fa_iterator_next(state)
            }
            None => {
                let object = self.coerce_object(&items)?;
                self.fa_init(state, &[("source", Value::Object(object))])?;
                let length = self.get_property(&Value::Object(object), &"length".into())?;
                let length = self.coerce_length(&length)?;
                let array = self.array_from_target(receiver, constructor, Some(length))?;
                self.fa_init(
                    state,
                    &[
                        ("array", Value::Object(array)),
                        ("length", Value::Number(length)),
                    ],
                )?;
                self.fa_element_next(state)
            }
        }
    }

    /// Steps 1-4 of one iterator-loop iteration: call `next` and await it.
    fn fa_iterator_next(&mut self, state: ObjectId) -> Result<(), RuntimeError> {
        let record = self.fa_get(state, "record");
        let result = match self.async_iterator_next(&record, None) {
            Ok(result) => result,
            Err(error) => {
                let error = self.error_value(error)?;
                // Marking the record done stores a property and can collect.
                self.stack.push(error.clone());
                self.fa_mark_done(state);
                return self.fa_reject(state, error);
            }
        };
        self.fa_await(state, Phase::IteratorResult, result)
    }

    /// One array-like iteration: read `Pk` and await it, or finish.
    fn fa_element_next(&mut self, state: ObjectId) -> Result<(), RuntimeError> {
        let k = self.fa_number(state, "k");
        let length = self.fa_number(state, "length");
        let array = self.fa_get(state, "array");
        if k >= length {
            self.array_set_or_throw(
                array.object_id().expect("result array"),
                "length".into(),
                &Value::Number(length),
            )?;
            return self.fa_resolve(state, array);
        }
        self.charge_step()?;
        let source = self.fa_get(state, "source");
        let value = self.get_property(&source, &(k as u64).to_string().into())?;
        self.fa_await(state, Phase::ElementValue, value)
    }

    /// `Await(value)`: PromiseResolve, then a reaction that resumes the run.
    /// A PromiseResolve failure is the Await's own throw completion.
    fn fa_await(
        &mut self,
        state: ObjectId,
        phase: Phase,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        self.stack.push(value.clone());
        let result = self.fa_await_rooted(state, phase, value);
        self.stack.truncate(base);
        result
    }

    fn fa_await_rooted(
        &mut self,
        state: ObjectId,
        phase: Phase,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.record_overwrite(state, "phase", Value::Number(phase as u8 as f64));
        let awaited = match self.promise_resolve(value) {
            Ok(promise) => promise,
            Err(error) => {
                let error = self.error_value(error)?;
                return self.fa_step(state, error, true);
            }
        };
        let base = self.stack.len();
        self.stack.push(awaited.clone());
        let handlers = (|| {
            let fulfilled = self.async_from_sync_handler(NativeFunction::ArrayFromAsyncResume {
                state,
                rejected: false,
            })?;
            self.stack.push(fulfilled.clone());
            let rejected = self.async_from_sync_handler(NativeFunction::ArrayFromAsyncResume {
                state,
                rejected: true,
            })?;
            self.stack.push(rejected.clone());
            Ok((fulfilled, rejected))
        })();
        let result = handlers.and_then(|(fulfilled, rejected)| {
            self.promise_then(&awaited, &[fulfilled, rejected])
                .map(|_| ())
        });
        self.stack.truncate(base);
        result
    }

    /// A settled Await. JavaScript errors are converted into a rejection of
    /// the run's promise; only host-level failures propagate.
    pub(in super::super) fn array_from_async_resume(
        &mut self,
        state: ObjectId,
        value: Value,
        rejected: bool,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        self.stack.push(Value::Object(state));
        self.stack.push(value.clone());
        let outcome = self.fa_step(state, value, rejected);
        self.stack.truncate(base);
        match outcome {
            Ok(()) => Ok(()),
            Err(error) => {
                let error = self.error_value(error)?;
                self.fa_reject(state, error)
            }
        }
    }

    fn fa_step(
        &mut self,
        state: ObjectId,
        value: Value,
        rejected: bool,
    ) -> Result<(), RuntimeError> {
        let phase = Phase::from_number(self.fa_number(state, "phase"));
        match phase {
            Phase::Closing => {
                let error = self.fa_get(state, "error");
                self.fa_reject(state, error)
            }
            Phase::IteratorResult => {
                if rejected {
                    self.fa_mark_done(state);
                    return self.fa_reject(state, value);
                }
                let record = self.fa_get(state, "record");
                // A failure rejects the run's promise, which the caller of
                // this step does for every error it receives.
                let next = self.async_iterator_step(&record, &value)?;
                let Some(next_value) = next else {
                    let k = self.fa_number(state, "k");
                    let array = self.fa_get(state, "array");
                    self.array_set_or_throw(
                        array.object_id().expect("result array"),
                        "length".into(),
                        &Value::Number(k),
                    )?;
                    return self.fa_resolve(state, array);
                };
                self.stack.push(next_value.clone());
                let mapper = self.fa_get(state, "mapper");
                if mapper == Value::Undefined {
                    return self.fa_define_and_continue(state, next_value, true);
                }
                let k = self.fa_number(state, "k");
                let this_arg = self.fa_get(state, "thisArg");
                match self.call_native(mapper, this_arg, vec![next_value, Value::Number(k)], false)
                {
                    Ok(mapped) => self.fa_await(state, Phase::IteratorMapped, mapped),
                    Err(error) => {
                        let error = self.error_value(error)?;
                        self.fa_close_and_reject(state, error)
                    }
                }
            }
            Phase::IteratorMapped => {
                if rejected {
                    return self.fa_close_and_reject(state, value);
                }
                self.fa_define_and_continue(state, value, true)
            }
            Phase::ElementValue => {
                if rejected {
                    return self.fa_reject(state, value);
                }
                let mapper = self.fa_get(state, "mapper");
                if mapper == Value::Undefined {
                    return self.fa_define_and_continue(state, value, false);
                }
                let k = self.fa_number(state, "k");
                let this_arg = self.fa_get(state, "thisArg");
                let mapped =
                    self.call_native(mapper, this_arg, vec![value, Value::Number(k)], false)?;
                self.fa_await(state, Phase::ElementMapped, mapped)
            }
            Phase::ElementMapped => {
                if rejected {
                    return self.fa_reject(state, value);
                }
                self.fa_define_and_continue(state, value, false)
            }
        }
    }

    /// `CreateDataPropertyOrThrow(A, Pk, mappedValue)`, `k = k + 1`, and the
    /// next iteration. A failed definition closes an iterator (`iterating`).
    fn fa_define_and_continue(
        &mut self,
        state: ObjectId,
        mapped: Value,
        iterating: bool,
    ) -> Result<(), RuntimeError> {
        let k = self.fa_number(state, "k");
        let array = self
            .fa_get(state, "array")
            .object_id()
            .expect("result array");
        self.stack.push(mapped.clone());
        let defined =
            self.array_create_data_property_or_throw(array, (k as u64).to_string().into(), mapped);
        if let Err(error) = defined {
            let error = self.error_value(error)?;
            return if iterating {
                self.fa_close_and_reject(state, error)
            } else {
                self.fa_reject(state, error)
            };
        }
        self.record_overwrite(state, "k", Value::Number(k + 1.0));
        if iterating {
            self.fa_iterator_next(state)
        } else {
            self.fa_element_next(state)
        }
    }

    fn fa_mark_done(&mut self, state: ObjectId) {
        let record = self
            .fa_get(state, "record")
            .object_id()
            .expect("only the iterator loop marks its record done");
        self.record_overwrite(record, "done", Value::Bool(true));
    }

    /// `AsyncIteratorClose(iteratorRecord, throwCompletion)`: call `return`
    /// (any failure is discarded), await its result, then reject with the
    /// original error.
    fn fa_close_and_reject(&mut self, state: ObjectId, error: Value) -> Result<(), RuntimeError> {
        self.stack.push(error.clone());
        // Only the iterator loop closes, and its record is still open: the
        // loop marks it done itself when `next` fails or reports completion.
        let record = self.fa_get(state, "record");
        self.fa_mark_done(state);
        let closing = (|| {
            let iterator = self
                .record_get(
                    record
                        .object_id()
                        .expect("only the iterator loop closes a record"),
                    "iterator",
                )
                .expect("iterator records keep their iterator");
            self.stack.push(iterator.clone());
            let close = self.get_method(&iterator, &"return".into())?;
            if close == Value::Undefined {
                return Ok(None);
            }
            self.call_native(close, iterator, Vec::new(), false)
                .map(Some)
        })();
        match closing {
            Ok(Some(result)) => {
                self.fa_set(state, "error", error)?;
                self.fa_await(state, Phase::Closing, result)
            }
            Ok(None) | Err(_) => self.fa_reject(state, error),
        }
    }

    fn fa_reject(&mut self, state: ObjectId, error: Value) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        self.stack.push(error.clone());
        let promise = self.fa_promise(state);
        self.settle_tracked_promise(promise, PromiseStatus::Rejected(error));
        self.stack.truncate(base);
        Ok(())
    }

    fn fa_resolve(&mut self, state: ObjectId, value: Value) -> Result<(), RuntimeError> {
        let promise = self.fa_promise(state);
        self.resolve_promise(promise, value)
    }
}
