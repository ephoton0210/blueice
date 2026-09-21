// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super::super) fn iterator_helper_next(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(helper) = receiver else {
            return Err(RuntimeError::TypeError(
                "Iterator helper next requires an iterator helper".into(),
            ));
        };
        let Some(state) = self.heap.iterator_helper(*helper)? else {
            return Err(RuntimeError::TypeError(
                "Iterator helper next requires an iterator helper".into(),
            ));
        };
        if state.kind == IteratorHelperKind::Concat {
            return self.iterator_concat_next(*helper, receiver, &state);
        }
        if matches!(
            state.kind,
            IteratorHelperKind::Zip | IteratorHelperKind::ZipKeyed
        ) {
            return self.iterator_zip_next(*helper, receiver, &state);
        }
        if state.kind == IteratorHelperKind::Chunks {
            return self.iterator_chunks_next(*helper, receiver, &state);
        }
        if state.kind == IteratorHelperKind::Windows {
            return self.iterator_windows_next(*helper, receiver, &state);
        }
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        self.stack.push(receiver.clone());
        self.stack.push(Value::Object(state.record));
        self.stack.push(state.callback.clone());
        let record = Value::Object(state.record);
        let result = (|| {
            self.with_roots(|heap| heap.begin_iterator_helper(*helper))?;
            loop {
                if state.kind == IteratorHelperKind::Take && state.index == 0 {
                    self.with_roots(|heap| heap.finish_iterator_helper(*helper))?;
                    self.iterator_close(&record)?;
                    return self.iterator_result(Value::Undefined, true);
                }
                if state.kind == IteratorHelperKind::FlatMap {
                    let inner = self.heap.get_own(state.record, "flatMapInner")?;
                    if let Some(inner @ Value::Object(_)) = inner {
                        self.stack.push(inner.clone());
                        let next = self.iterator_step(&inner, true);
                        self.stack.pop();
                        if let Some(value) = next? {
                            self.stack.push(value.clone());
                            let result = self.iterator_result(value, false);
                            self.stack.pop();
                            return result;
                        }
                        self.with_roots(|heap| {
                            heap.set(state.record, "flatMapInner", Value::Undefined)
                        })?;
                    }
                }
                let Some(value) = self.iterator_step(&record, true)? else {
                    self.with_roots(|heap| heap.finish_iterator_helper(*helper))?;
                    return self.iterator_result(Value::Undefined, true);
                };
                match state.kind {
                    IteratorHelperKind::Concat => {
                        unreachable!("concat has a dedicated state machine")
                    }
                    IteratorHelperKind::Zip | IteratorHelperKind::ZipKeyed => {
                        unreachable!("zip has a dedicated state machine")
                    }
                    IteratorHelperKind::Chunks => {
                        unreachable!("chunks has a dedicated state machine")
                    }
                    IteratorHelperKind::Windows => {
                        unreachable!("windows has a dedicated state machine")
                    }
                    IteratorHelperKind::Take => {
                        self.with_roots(|heap| heap.consume_iterator_helper_take(*helper))?;
                        self.stack.push(value.clone());
                        let result = self.iterator_result(value, false);
                        self.stack.pop();
                        return result;
                    }
                    IteratorHelperKind::Drop => {
                        let remaining = self
                            .heap
                            .iterator_helper(*helper)?
                            .expect("iterator helper state remains live")
                            .index;
                        if remaining > 0 {
                            self.with_roots(|heap| heap.consume_iterator_helper_take(*helper))?;
                            continue;
                        }
                        self.stack.push(value.clone());
                        let result = self.iterator_result(value, false);
                        self.stack.pop();
                        return result;
                    }
                    IteratorHelperKind::Map => {
                        let callback_result = self.iterator_helper_callback(*helper, &value)?;
                        self.stack.pop();
                        self.stack.push(callback_result.clone());
                        let result = self.iterator_result(callback_result, false);
                        self.stack.pop();
                        return result;
                    }
                    IteratorHelperKind::Filter => {
                        let callback_result = self.iterator_helper_callback(*helper, &value)?;
                        let selected = match self.to_boolean(&callback_result) {
                            Ok(selected) => selected,
                            Err(error) => {
                                self.stack.pop();
                                return Err(error);
                            }
                        };
                        if selected {
                            let result = self.iterator_result(value, false);
                            self.stack.pop();
                            return result;
                        }
                        self.stack.pop();
                    }
                    IteratorHelperKind::FlatMap => {
                        let mapped = self.iterator_helper_callback(*helper, &value)?;
                        self.stack.pop();
                        self.stack.push(mapped.clone());
                        let inner = self.iterator_flattenable_record(&mapped);
                        self.stack.pop();
                        let inner = inner?;
                        self.stack.push(inner.clone());
                        self.with_roots(|heap| heap.set(state.record, "flatMapInner", inner))?;
                        self.stack.pop();
                    }
                }
            }
        })();
        if result.is_err() {
            self.with_roots(|heap| heap.finish_iterator_helper(*helper))?;
        } else if !self
            .heap
            .iterator_helper(*helper)?
            .is_some_and(|state| state.done)
        {
            self.with_roots(|heap| heap.leave_iterator_helper(*helper))?;
        }
        self.stack.pop();
        self.stack.pop();
        self.stack.pop();
        self.close_iterator_on_error(&record, result)
    }

    pub(in super::super::super) fn iterator_concat_next(
        &mut self,
        helper: ObjectId,
        receiver: &Value,
        state: &IteratorHelperState,
    ) -> Result<Value, RuntimeError> {
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        let metadata = Value::Object(state.record);
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.push(metadata.clone());
        let outcome = (|| {
            self.with_roots(|heap| heap.begin_iterator_helper(helper))?;
            loop {
                if let Some(current @ Value::Object(_)) =
                    self.heap.get_own(state.record, "concatCurrent")?
                {
                    self.stack.push(current.clone());
                    let next = self.iterator_step(&current, true);
                    self.stack.pop();
                    if let Some(value) = next? {
                        self.stack.push(value.clone());
                        let result = self.iterator_result(value, false);
                        self.stack.pop();
                        return result;
                    }
                    self.with_roots(|heap| {
                        heap.set(state.record, "concatCurrent", Value::Undefined)
                    })?;
                    continue;
                }

                let current_state = self
                    .heap
                    .iterator_helper(helper)?
                    .expect("concat helper state remains live");
                let length = self
                    .heap
                    .get_own(state.record, "concatLength")?
                    .and_then(|value| match value {
                        Value::Number(value) => Some(value as u64),
                        _ => None,
                    })
                    .expect("concat state stores its argument count");
                if current_state.index >= length {
                    self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                    return self.iterator_result(Value::Undefined, true);
                }
                let index = current_state.index;
                let iterable = self
                    .heap
                    .get_own(state.record, format!("concatIterable{index}"))?
                    .expect("concat state stores every iterable");
                let open = self
                    .heap
                    .get_own(state.record, format!("concatOpen{index}"))?
                    .expect("concat state stores every iterator method");
                self.stack.push(iterable.clone());
                self.stack.push(open.clone());
                let opened = self.call_native(open, iterable, Vec::new(), false)?;
                self.stack.pop();
                self.stack.pop();
                if !matches!(opened, Value::Object(_)) {
                    return Err(RuntimeError::TypeError(
                        "Iterator.concat iterator method must return an object".into(),
                    ));
                }
                self.stack.push(opened.clone());
                let direct = self.direct_iterator_record(&opened)?;
                self.stack.pop();
                self.stack.push(direct.clone());
                self.with_roots(|heap| heap.set(state.record, "concatCurrent", direct))?;
                self.with_roots(|heap| heap.advance_iterator_helper(helper))?;
                self.stack.pop();
            }
        })();
        let outcome = match outcome {
            Ok(value) => {
                if !self
                    .heap
                    .iterator_helper(helper)?
                    .is_some_and(|state| state.done)
                {
                    self.with_roots(|heap| heap.leave_iterator_helper(helper))?;
                }
                Ok(value)
            }
            Err(error) => {
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                self.close_concat_on_error(&metadata, error)
            }
        };
        self.stack.truncate(base);
        outcome
    }

    pub(in super::super::super) fn iterator_zip_next(
        &mut self,
        helper: ObjectId,
        receiver: &Value,
        state: &IteratorHelperState,
    ) -> Result<Value, RuntimeError> {
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        let metadata = Value::Object(state.record);
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.push(metadata.clone());
        let outcome = (|| {
            self.with_roots(|heap| heap.begin_iterator_helper(helper))?;
            let count = self
                .heap
                .get_own(state.record, "zipCount")?
                .and_then(|value| match value {
                    Value::Number(value) => Some(value as u64),
                    _ => None,
                })
                .expect("zip metadata stores its source count");
            let mode = self
                .heap
                .get_own(state.record, "zipMode")?
                .expect("zip metadata stores its mode");
            let Value::String(mode) = mode else {
                unreachable!("zip mode is a string")
            };
            if count == 0 {
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                return self.iterator_result(Value::Undefined, true);
            }
            let values_base = self.stack.len();
            let result = (|| {
                let mut values = Vec::new();
                let mut any_done = false;
                let mut all_done = true;
                for index in 0..count {
                    let record = self
                        .heap
                        .get_own(state.record, format!("zipRecord{index}"))?
                        .expect("zip metadata stores every source record");
                    self.stack.push(record.clone());
                    let step = self.iterator_step(&record, true);
                    self.stack.pop();
                    match step? {
                        Some(value) => {
                            all_done = false;
                            self.stack.push(value.clone());
                            values.push(Some(value));
                        }
                        None => {
                            any_done = true;
                            if mode == "strict" {
                                if index != 0 {
                                    self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                                    let _ = self.iterator_zip_close_records(state.record, count);
                                    return Err(RuntimeError::TypeError(
                                        "Iterator.zip strict sources have different lengths".into(),
                                    ));
                                }
                                for remaining in 1..count {
                                    let record = self
                                        .heap
                                        .get_own(state.record, format!("zipRecord{remaining}"))?
                                        .expect("zip metadata stores every source record");
                                    self.stack.push(record.clone());
                                    let step = self.iterator_step(&record, true);
                                    self.stack.pop();
                                    if step?.is_some() {
                                        self.with_roots(|heap| {
                                            heap.finish_iterator_helper(helper)
                                        })?;
                                        let _ =
                                            self.iterator_zip_close_records(state.record, count);
                                        return Err(RuntimeError::TypeError(
                                            "Iterator.zip strict sources have different lengths"
                                                .into(),
                                        ));
                                    }
                                }
                                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                                return self.iterator_result(Value::Undefined, true);
                            }
                            values.push(None);
                        }
                    }
                    if mode == "shortest" && any_done {
                        self.iterator_zip_close_records(state.record, count)?;
                        self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                        return self.iterator_result(Value::Undefined, true);
                    }
                }
                if mode == "longest" {
                    if all_done {
                        self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                        return self.iterator_result(Value::Undefined, true);
                    }
                    for (index, value) in values.iter_mut().enumerate() {
                        if value.is_none() {
                            let padding = self
                                .heap
                                .get_own(state.record, format!("zipPadding{index}"))?
                                .expect("zip metadata stores every padding value");
                            self.stack.push(padding.clone());
                            *value = Some(padding);
                        }
                    }
                }
                let values = values
                    .into_iter()
                    .map(|value| value.expect("non-longest modes have no completed source"))
                    .collect();
                let values = if state.kind == IteratorHelperKind::Zip {
                    self.array_from(values)?
                } else {
                    self.iterator_zip_keyed_results(state.record, values)?
                };
                // Adding the marker property grows the helper's state, which
                // can run a major collection: root the result array first.
                // Adding the marker property grows the helper's state, which
                // can run a major collection: root the result first.
                self.stack.push(values.clone());
                self.with_roots(|heap| heap.set(state.record, "zipStarted", Value::Bool(true)))?;
                let result = self.iterator_result(values, false);
                self.stack.pop();
                result
            })();
            self.stack.truncate(values_base);
            result
        })();
        let outcome = match outcome {
            Ok(value) => {
                if !self
                    .heap
                    .iterator_helper(helper)?
                    .is_some_and(|state| state.done)
                {
                    self.with_roots(|heap| heap.leave_iterator_helper(helper))?;
                }
                Ok(value)
            }
            Err(error) => {
                // The thrown value is referenced only by this Rust local,
                // while finishing the helper and closing every source run
                // JavaScript and may collect.
                self.root_thrown(&error);
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                let _ = self.iterator_zip_close_records(
                    state.record,
                    self.iterator_zip_count(state.record)?,
                );
                Err(error)
            }
        };
        self.stack.truncate(base);
        outcome
    }

    pub(in super::super::super) fn iterator_zip_keyed_results(
        &mut self,
        metadata: ObjectId,
        values: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        let result = self.with_roots(|heap| heap.alloc_object(None))?;
        let base = self.stack.len();
        self.stack.push(Value::Object(result));
        let outcome = (|| {
            for (index, value) in values.into_iter().enumerate() {
                let key = self
                    .heap
                    .get_own(metadata, format!("zipKey{index}"))?
                    .expect("zipKeyed metadata stores every source key");
                let key = self.coerce_property_key(&key)?;
                self.stack.push(value.clone());
                let defined = self.object_define_own_property(
                    result,
                    key,
                    PropertyDescriptor::data(value, true, true, true),
                )?;
                self.stack.pop();
                if !defined {
                    return Err(RuntimeError::TypeError(
                        "cannot define Iterator.zipKeyed result property".into(),
                    ));
                }
            }
            Ok(Value::Object(result))
        })();
        self.stack.truncate(base);
        outcome
    }

    pub(in super::super::super) fn iterator_zip_count(
        &self,
        metadata: ObjectId,
    ) -> Result<u64, RuntimeError> {
        self.heap
            .get_own(metadata, "zipCount")?
            .and_then(|value| match value {
                Value::Number(value) => Some(value as u64),
                _ => None,
            })
            .ok_or_else(|| RuntimeError::TypeError("invalid Iterator.zip state".into()))
    }

    pub(in super::super::super) fn iterator_zip_close_records(
        &mut self,
        metadata: ObjectId,
        count: u64,
    ) -> Result<(), RuntimeError> {
        let mut completion = None;
        let base = self.stack.len();
        for index in (0..count).rev() {
            let record = self
                .heap
                .get_own(metadata, format!("zipRecord{index}"))?
                .expect("zip metadata stores every source record");
            self.stack.push(record.clone());
            let close = self.iterator_close(&record);
            self.stack.pop();
            if let Err(error) = close {
                // IteratorCloseAll continues after an abrupt `return`. The
                // first such error becomes the completion; later close errors
                // cannot replace it, but their `return` methods still run,
                // so the retained error must stay rooted meanwhile.
                if completion.is_none() {
                    self.root_thrown(&error);
                    completion = Some(error);
                }
            }
        }
        self.stack.truncate(base);
        if let Some(error) = completion {
            return Err(error);
        }
        Ok(())
    }

    /// Like [`Self::iterator_zip_close_records`] for a completion that is
    /// already a throw: the in-flight error stays rooted while every `return`
    /// method runs, and an error from one of them is discarded.
    pub(in super::super::super) fn iterator_zip_close_records_after(
        &mut self,
        metadata: ObjectId,
        count: u64,
        error: &RuntimeError,
    ) {
        let base = self.stack.len();
        self.root_thrown(error);
        let _ = self.iterator_zip_close_records(metadata, count);
        self.stack.truncate(base);
    }

    /// Keeps the value of a throw completion alive on the VM stack. The
    /// caller truncates the stack once the error has been handed onward.
    pub(in super::super::super) fn root_thrown(&mut self, error: &RuntimeError) {
        if let RuntimeError::Thrown(value) = error {
            self.stack.push(value.clone());
        }
    }

    pub(in super::super::super) fn iterator_chunks_next(
        &mut self,
        helper: ObjectId,
        receiver: &Value,
        state: &IteratorHelperState,
    ) -> Result<Value, RuntimeError> {
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        let record = Value::Object(state.record);
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.push(record.clone());
        let outcome = (|| {
            self.with_roots(|heap| heap.begin_iterator_helper(helper))?;
            let values_base = self.stack.len();
            let chunk = (|| {
                let mut values = Vec::new();
                for _ in 0..state.index {
                    let Some(value) = self.iterator_step(&record, true)? else {
                        break;
                    };
                    self.stack.push(value.clone());
                    values.push(value);
                }
                if values.is_empty() {
                    return Ok(None);
                }
                self.array_from(values).map(Some)
            })();
            self.stack.truncate(values_base);
            let Some(chunk) = chunk? else {
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                return self.iterator_result(Value::Undefined, true);
            };
            self.stack.push(chunk.clone());
            let result = self.iterator_result(chunk, false);
            self.stack.pop();
            result
        })();
        let outcome = match outcome {
            Ok(value) => {
                if !self
                    .heap
                    .iterator_helper(helper)?
                    .is_some_and(|state| state.done)
                {
                    self.with_roots(|heap| heap.leave_iterator_helper(helper))?;
                }
                Ok(value)
            }
            Err(error) => {
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                self.close_iterator_on_error(&record, Err(error))
            }
        };
        self.stack.truncate(base);
        outcome
    }

    pub(in super::super::super) fn iterator_windows_next(
        &mut self,
        helper: ObjectId,
        receiver: &Value,
        state: &IteratorHelperState,
    ) -> Result<Value, RuntimeError> {
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        let record = Value::Object(state.record);
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.push(record.clone());
        let outcome = (|| {
            self.with_roots(|heap| heap.begin_iterator_helper(helper))?;
            let values_base = self.stack.len();
            let window = (|| {
                let allow_partial = matches!(
                    self.heap.get_own(state.record, "windowsAllowPartial")?,
                    Some(Value::Bool(true))
                );
                let prior = self.heap.get_own(state.record, "windowsBuffer")?;
                let mut values = Vec::new();
                if let Some(Value::Object(buffer)) = prior {
                    // A full prior window slides by one element. Its private
                    // buffer is ordinary traced storage, but every value is
                    // rooted while the next pull can allocate or invoke JS.
                    for index in 1..state.index {
                        let value = self
                            .heap
                            .get_own(buffer, index.to_string())?
                            .expect("window buffer has the requested length");
                        self.stack.push(value.clone());
                        values.push(value);
                    }
                    let Some(value) = self.iterator_step(&record, true)? else {
                        self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                        return Ok(None);
                    };
                    self.stack.push(value.clone());
                    values.push(value);
                } else {
                    for _ in 0..state.index {
                        let Some(value) = self.iterator_step(&record, true)? else {
                            self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                            if allow_partial && !values.is_empty() {
                                return self.array_from(values).map(Some);
                            }
                            return Ok(None);
                        };
                        self.stack.push(value.clone());
                        values.push(value);
                    }
                }
                // A yielded full window becomes the next private sliding
                // buffer, while the caller receives a distinct Array.
                let buffer = self.array_from(values.clone())?;
                self.stack.push(buffer.clone());
                self.with_roots(|heap| heap.set(state.record, "windowsBuffer", buffer))?;
                self.stack.pop();
                self.array_from(values).map(Some)
            })();
            let Some(window) = window? else {
                self.stack.truncate(values_base);
                return self.iterator_result(Value::Undefined, true);
            };
            self.stack.push(window.clone());
            let result = self.iterator_result(window, false);
            self.stack.pop();
            self.stack.truncate(values_base);
            result
        })();
        let outcome = match outcome {
            Ok(value) => {
                if !self
                    .heap
                    .iterator_helper(helper)?
                    .is_some_and(|state| state.done)
                {
                    self.with_roots(|heap| heap.leave_iterator_helper(helper))?;
                }
                Ok(value)
            }
            Err(error) => {
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                self.close_iterator_on_error(&record, Err(error))
            }
        };
        self.stack.truncate(base);
        outcome
    }

    pub(in super::super::super) fn close_concat_on_error<T>(
        &mut self,
        metadata: &Value,
        error: RuntimeError,
    ) -> Result<T, RuntimeError> {
        let Value::Object(metadata) = metadata else {
            unreachable!("concat metadata is an ordinary object")
        };
        let current = self.heap.get_own(*metadata, "concatCurrent")?;
        if let Some(current @ Value::Object(_)) = current {
            self.stack.push(current.clone());
            let result = self.close_iterator_on_error(&current, Err(error));
            self.stack.pop();
            result
        } else {
            Err(error)
        }
    }

    pub(in super::super::super) fn iterator_helper_callback(
        &mut self,
        helper: ObjectId,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let state = self
            .heap
            .iterator_helper(helper)?
            .expect("iterator helper state remains live");
        if state.index >= 9_007_199_254_740_991 {
            return Err(RuntimeError::RangeError(
                "Iterator helper index exceeds the safe integer range".into(),
            ));
        }
        self.stack.push(value.clone());
        let callback_result = self.call_native(
            state.callback.clone(),
            Value::Undefined,
            vec![value.clone(), Value::Number(state.index as f64)],
            false,
        );
        let callback_result = match callback_result {
            Ok(result) => result,
            Err(error) => {
                self.stack.pop();
                return Err(error);
            }
        };
        self.with_roots(|heap| heap.advance_iterator_helper(helper))?;
        Ok(callback_result)
    }

    pub(in super::super::super) fn close_direct_iterator_on_error<T>(
        &mut self,
        iterator: &Value,
        error: RuntimeError,
    ) -> Result<T, RuntimeError> {
        let base = self.stack.len();
        if let RuntimeError::Thrown(value) = &error {
            self.stack.push(value.clone());
        }
        let _ = self.iterator_close_direct(iterator);
        self.stack.truncate(base);
        Err(error)
    }

    pub(in super::super::super) fn iterator_includes(
        &mut self,
        receiver: &Value,
        search_element: &Value,
        skipped_elements: &Value,
    ) -> Result<Value, RuntimeError> {
        if !matches!(receiver, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Iterator helper requires an object receiver".into(),
            ));
        }
        self.stack.push(receiver.clone());
        self.stack.push(search_element.clone());
        let result = (|| {
            // `includes` intentionally does not apply ToNumber to its
            // optional skip count. It accepts only integral Numbers and
            // infinities, and validates before reading `next`.
            let to_skip = match skipped_elements {
                Value::Undefined => 0.0,
                Value::Number(number)
                    if !number.is_nan() && (number.is_infinite() || number.fract() == 0.0) =>
                {
                    *number
                }
                _ => {
                    return self.close_direct_iterator_on_error(
                        receiver,
                        RuntimeError::TypeError(
                            "Iterator.includes skippedElements must be an integral Number".into(),
                        ),
                    )
                }
            };
            if to_skip < 0.0 {
                return self.close_direct_iterator_on_error(
                    receiver,
                    RuntimeError::RangeError(
                        "Iterator.includes skippedElements must not be negative".into(),
                    ),
                );
            }
            if to_skip.is_finite() && to_skip > 9_007_199_254_740_991.0 {
                return self.close_direct_iterator_on_error(
                    receiver,
                    RuntimeError::RangeError(
                        "Iterator.includes skippedElements exceeds MAX_SAFE_INTEGER".into(),
                    ),
                );
            }
            let mut skipped = to_skip;
            let record = self.direct_iterator_record(receiver)?;
            self.stack.push(record.clone());
            let result = (|| {
                while let Some(value) = self.iterator_step(&record, true)? {
                    if skipped > 0.0 {
                        skipped -= 1.0;
                        continue;
                    }
                    if Self::same_value_zero(&value, search_element) {
                        self.iterator_close(&record)?;
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            })();
            self.stack.pop();
            self.close_iterator_on_error(&record, result)
        })();
        self.stack.pop();
        self.stack.pop();
        result
    }

    pub(in super::super::super) fn same_value_zero(left: &Value, right: &Value) -> bool {
        match (left, right) {
            (Value::Number(left), Value::Number(right)) => {
                left == right || (left.is_nan() && right.is_nan())
            }
            _ => left == right,
        }
    }

    pub(in super::super::super) fn iterator_join(
        &mut self,
        receiver: &Value,
        separator: &Value,
    ) -> Result<Value, RuntimeError> {
        if !matches!(receiver, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Iterator helper requires an object receiver".into(),
            ));
        }
        self.stack.push(receiver.clone());
        let result = (|| {
            // The separator is coerced before `next` is fetched. Its abrupt
            // conversion closes the direct iterator record without observing
            // that property.
            let separator = if *separator == Value::Undefined {
                ",".into()
            } else {
                match self.coerce_string(separator) {
                    Ok(separator) => separator,
                    Err(error) => return self.close_direct_iterator_on_error(receiver, error),
                }
            };
            let record = self.direct_iterator_record(receiver)?;
            self.stack.push(record.clone());
            let result = (|| {
                let mut result = JsString::default();
                let mut first = true;
                while let Some(value) = self.iterator_step(&record, true)? {
                    if !first {
                        native::append(&mut result, &separator, self.config.max_string_bytes)?;
                    }
                    first = false;
                    if !matches!(value, Value::Null | Value::Undefined) {
                        self.stack.push(value.clone());
                        let text = self.coerce_string(&value);
                        self.stack.pop();
                        let text = text?;
                        native::append(&mut result, &text, self.config.max_string_bytes)?;
                    }
                }
                Ok(Value::String(result))
            })();
            self.stack.pop();
            self.close_iterator_on_error(&record, result)
        })();
        self.stack.pop();
        result
    }

    pub(in super::super::super) fn iterator_helper_return(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(helper) = receiver else {
            return Err(RuntimeError::TypeError(
                "Iterator helper return requires an iterator helper".into(),
            ));
        };
        let Some(state) = self.heap.iterator_helper(*helper)? else {
            return Err(RuntimeError::TypeError(
                "Iterator helper return requires an iterator helper".into(),
            ));
        };
        if state.kind == IteratorHelperKind::Concat {
            return self.iterator_concat_return(*helper, receiver, &state);
        }
        if matches!(
            state.kind,
            IteratorHelperKind::Zip | IteratorHelperKind::ZipKeyed
        ) {
            return self.iterator_zip_return(*helper, receiver, &state);
        }
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        self.stack.push(receiver.clone());
        self.stack.push(Value::Object(state.record));
        let record = Value::Object(state.record);
        let result = (|| {
            self.with_roots(|heap| heap.begin_iterator_helper(*helper))?;
            if state.kind == IteratorHelperKind::FlatMap {
                if let Some(inner @ Value::Object(_)) =
                    self.heap.get_own(state.record, "flatMapInner")?
                {
                    self.stack.push(inner.clone());
                    let inner_result = self.iterator_close(&inner);
                    self.stack.pop();
                    if let Err(error) = inner_result {
                        return self.close_iterator_on_error(&record, Err(error));
                    }
                }
            }
            self.iterator_close(&record)?;
            self.iterator_result(Value::Undefined, true)
        })();
        self.with_roots(|heap| heap.finish_iterator_helper(*helper))?;
        self.stack.pop();
        self.stack.pop();
        result
    }

    pub(in super::super::super) fn iterator_concat_return(
        &mut self,
        helper: ObjectId,
        receiver: &Value,
        state: &IteratorHelperState,
    ) -> Result<Value, RuntimeError> {
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        let metadata = Value::Object(state.record);
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.push(metadata.clone());
        let result = (|| {
            self.with_roots(|heap| heap.begin_iterator_helper(helper))?;
            if let Some(current @ Value::Object(_)) =
                self.heap.get_own(state.record, "concatCurrent")?
            {
                self.stack.push(current.clone());
                let close = self.iterator_close(&current);
                self.stack.pop();
                close?;
            }
            self.iterator_result(Value::Undefined, true)
        })();
        self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
        self.stack.truncate(base);
        result
    }

    pub(in super::super::super) fn iterator_zip_return(
        &mut self,
        helper: ObjectId,
        receiver: &Value,
        state: &IteratorHelperState,
    ) -> Result<Value, RuntimeError> {
        if state.done {
            return self.iterator_result(Value::Undefined, true);
        }
        if state.executing {
            return Err(RuntimeError::TypeError(
                "Iterator helper is already executing".into(),
            ));
        }
        let metadata = Value::Object(state.record);
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.push(metadata);
        let result = (|| {
            let started = matches!(
                self.heap.get_own(state.record, "zipStarted")?,
                Some(Value::Bool(true))
            );
            if started {
                // Resuming a suspended yield runs the close handlers while
                // the helper is executing, so their re-entrant `next`/return
                // calls see the GeneratorValidate TypeError.
                self.with_roots(|heap| heap.begin_iterator_helper(helper))?;
                let close = self.iterator_zip_close_records(
                    state.record,
                    self.iterator_zip_count(state.record)?,
                );
                if let Err(error) = &close {
                    self.root_thrown(error);
                }
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                close?;
            } else {
                // GeneratorClose from suspended-start completes the helper
                // before the close handlers run. Re-entrant `next` therefore
                // returns the completed iterator result.
                self.with_roots(|heap| heap.finish_iterator_helper(helper))?;
                self.iterator_zip_close_records(
                    state.record,
                    self.iterator_zip_count(state.record)?,
                )?;
            }
            self.iterator_result(Value::Undefined, true)
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super::super) fn iterator_callback(
        &mut self,
        callback: &Value,
        value: Value,
        index: u64,
    ) -> Result<Value, RuntimeError> {
        if !self.is_callable(callback)? {
            return Err(RuntimeError::TypeError(
                "Iterator helper callback must be callable".into(),
            ));
        }
        self.call_native(
            callback.clone(),
            Value::Undefined,
            vec![value, Value::Number(index as f64)],
            false,
        )
    }

    pub(in super::super::super) fn close_iterator_on_error<T>(
        &mut self,
        record: &Value,
        result: Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                // IteratorClose preserves an active iterator's observable
                // cleanup side effect. When a completion is already abrupt,
                // a later `return` failure cannot replace it.
                let base = self.stack.len();
                if let RuntimeError::Thrown(value) = &error {
                    self.stack.push(value.clone());
                }
                let _ = self.iterator_close(record);
                self.stack.truncate(base);
                Err(error)
            }
        }
    }

    pub(in super::super::super) fn iterator_to_array(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.direct_iterator_record(receiver)?;
        self.stack.push(record.clone());
        let result = (|| {
            let array = self.array_from(Vec::new())?;
            self.stack.push(array.clone());
            while let Some(value) = self.iterator_step(&record, true)? {
                self.array_push(&array, &value, 0)?;
            }
            self.stack.pop();
            Ok(array)
        })();
        self.stack.pop();
        self.close_iterator_on_error(&record, result)
    }

    pub(in super::super::super) fn iterator_for_each(
        &mut self,
        receiver: &Value,
        callback: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.callback_iterator_record(receiver, callback)?;
        self.stack.push(record.clone());
        let result = (|| {
            let mut index = 0_u64;
            while let Some(value) = self.iterator_step(&record, true)? {
                self.iterator_callback(callback, value, index)?;
                index += 1;
            }
            Ok(Value::Undefined)
        })();
        self.stack.pop();
        self.close_iterator_on_error(&record, result)
    }

    pub(in super::super::super) fn iterator_every(
        &mut self,
        receiver: &Value,
        callback: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.callback_iterator_record(receiver, callback)?;
        self.stack.push(record.clone());
        let result = (|| {
            let mut index = 0_u64;
            while let Some(value) = self.iterator_step(&record, true)? {
                let predicate = self.iterator_callback(callback, value, index)?;
                if !self.to_boolean(&predicate)? {
                    self.iterator_close(&record)?;
                    return Ok(Value::Bool(false));
                }
                index += 1;
            }
            Ok(Value::Bool(true))
        })();
        self.stack.pop();
        self.close_iterator_on_error(&record, result)
    }

    pub(in super::super::super) fn iterator_some(
        &mut self,
        receiver: &Value,
        callback: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.callback_iterator_record(receiver, callback)?;
        self.stack.push(record.clone());
        let result = (|| {
            let mut index = 0_u64;
            while let Some(value) = self.iterator_step(&record, true)? {
                let predicate = self.iterator_callback(callback, value, index)?;
                if self.to_boolean(&predicate)? {
                    self.iterator_close(&record)?;
                    return Ok(Value::Bool(true));
                }
                index += 1;
            }
            Ok(Value::Bool(false))
        })();
        self.stack.pop();
        self.close_iterator_on_error(&record, result)
    }

    pub(in super::super::super) fn iterator_find(
        &mut self,
        receiver: &Value,
        callback: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.callback_iterator_record(receiver, callback)?;
        self.stack.push(record.clone());
        let result = (|| {
            let mut index = 0_u64;
            while let Some(value) = self.iterator_step(&record, true)? {
                let predicate = self.iterator_callback(callback, value.clone(), index)?;
                if self.to_boolean(&predicate)? {
                    self.iterator_close(&record)?;
                    return Ok(value);
                }
                index += 1;
            }
            Ok(Value::Undefined)
        })();
        self.stack.pop();
        self.close_iterator_on_error(&record, result)
    }

    pub(in super::super::super) fn iterator_reduce(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let callback = native::argument(args, 0);
        let record = self.callback_iterator_record(receiver, callback)?;
        self.stack.push(record.clone());
        let result = (|| {
            if !self.is_callable(callback)? {
                return Err(RuntimeError::TypeError(
                    "Iterator helper callback must be callable".into(),
                ));
            }
            let mut index = 0_u64;
            let mut accumulator = if args.len() > 1 {
                args[1].clone()
            } else {
                let Some(value) = self.iterator_step(&record, true)? else {
                    return Err(RuntimeError::TypeError(
                        "cannot reduce an empty iterator without an initial value".into(),
                    ));
                };
                index = 1;
                value
            };
            while let Some(value) = self.iterator_step(&record, true)? {
                accumulator = self.call_native(
                    callback.clone(),
                    Value::Undefined,
                    vec![accumulator, value, Value::Number(index as f64)],
                    false,
                )?;
                index += 1;
            }
            Ok(accumulator)
        })();
        self.stack.pop();
        self.close_iterator_on_error(&record, result)
    }

    pub(in super::super::super) fn iterator_to_string_tag_setter(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(receiver) = receiver else {
            return Err(RuntimeError::TypeError(
                "Iterator prototype tag setter requires an object receiver".into(),
            ));
        };
        let base = self.base_iterator_prototype()?;
        if *receiver == base {
            return Err(RuntimeError::TypeError(
                "cannot assign Iterator.prototype Symbol.toStringTag".into(),
            ));
        }
        let key: PropertyName = JsSymbol::well_known("toStringTag").into();
        if self
            .heap
            .get_own_property_descriptor(*receiver, &key)?
            .is_none()
        {
            self.define_data(*receiver, key, value.clone(), true, true, true)?;
        } else {
            self.set_property(&Value::Object(*receiver), &key, value)?;
        }
        Ok(Value::Undefined)
    }

    pub(in super::super::super) fn iterator_constructor_setter(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(receiver) = receiver else {
            return Err(RuntimeError::TypeError(
                "Iterator prototype constructor setter requires an object receiver".into(),
            ));
        };
        let base = self.base_iterator_prototype()?;
        if *receiver == base {
            return Err(RuntimeError::TypeError(
                "cannot assign Iterator.prototype constructor".into(),
            ));
        }
        let key: PropertyName = "constructor".into();
        if self
            .heap
            .get_own_property_descriptor(*receiver, &key)?
            .is_none()
        {
            // SetterThatIgnoresPrototypeProperties creates an own data
            // property instead of recursively invoking the accessor found on
            // %Iterator.prototype%.
            self.define_data(*receiver, key, value.clone(), true, true, true)?;
        } else {
            self.set_property(&Value::Object(*receiver), &key, value)?;
        }
        Ok(Value::Undefined)
    }

    pub(in super::super::super) fn template_object(
        &mut self,
        site: &crate::bytecode::TemplateSite,
    ) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.templates.get(&site.id) {
            return Ok(Value::Object(id));
        }
        let base = self.stack.len();
        let result = (|| {
            for string in site.raw.iter().chain(site.cooked.iter().flatten()) {
                self.check_string(&Value::String(string.clone()))?;
            }
            let raw = self.array_from(site.raw.iter().cloned().map(Value::String).collect())?;
            self.stack.push(raw.clone());
            let cooked = self.array_from(
                site.cooked
                    .iter()
                    .cloned()
                    .map(|s| s.map_or(Value::Undefined, Value::String))
                    .collect(),
            )?;
            self.stack.push(cooked.clone());
            self.define_data(
                cooked.object_id().unwrap(),
                "raw",
                raw.clone(),
                false,
                false,
                false,
            )?;
            for array in [&raw, &cooked] {
                let id = array.object_id().unwrap();
                for key in self.heap.own_property_keys(id)? {
                    let mut desc = self.heap.get_own_property_descriptor(id, &key)?.unwrap();
                    desc.writable = Some(false);
                    desc.configurable = Some(false);
                    self.with_roots(|heap| heap.define_own_property(id, key, desc))?;
                }
                self.heap.prevent_extensions(id)?;
            }
            self.heap.root(cooked.object_id().unwrap())?;
            self.templates.insert(site.id, cooked.object_id().unwrap());
            Ok(cooked)
        })();
        self.stack.truncate(base);
        result
    }
    pub(in super::super::super) fn get_iterator(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let method = self.get_method(value, &JsSymbol::well_known("iterator").into())?;
        self.get_iterator_from_method(value, method)
    }

    pub(in super::super::super) fn get_iterator_from_method(
        &mut self,
        value: &Value,
        method: Value,
    ) -> Result<Value, RuntimeError> {
        let iterator = self.call_native(method, value.clone(), Vec::new(), false)?;
        if !matches!(iterator, Value::Object(_)) {
            return Err(RuntimeError::TypeError("iterator must be an object".into()));
        }
        self.stack.push(iterator.clone());
        let next = self.get_property(&iterator, &"next".into())?;
        self.stack.push(next.clone());
        let record = self.with_roots(|heap| heap.alloc_object(None))?;
        self.stack.push(Value::Object(record));
        self.with_roots(|heap| heap.set(record, "iterator", iterator))?;
        self.with_roots(|heap| heap.set(record, "next", next))?;
        self.with_roots(|heap| heap.set(record, "done", Value::Bool(false)))?;
        self.stack.pop();
        self.stack.pop();
        self.stack.pop();
        Ok(Value::Object(record))
    }

    /// GetAsyncIterator first observes @@asyncIterator and uses the ordinary
    /// iterator protocol as an AsyncFromSync fallback. The caller awaits the
    /// returned `.next()` result, so both paths share one record shape.
    pub(in super::super::super) fn get_async_iterator(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let method = self.get_method(value, &JsSymbol::well_known("asyncIterator").into())?;
        if method == Value::Undefined {
            let record = self.get_iterator(value)?;
            return self.mark_async_from_sync(record);
        }
        self.async_iterator_record_from_method(value, method)
    }

    /// CreateAsyncFromSyncIterator over an ordinary iterator record: the same
    /// record, flagged so `AsyncIteratorNext` adopts each result's `value`.
    pub(in super::super::super) fn mark_async_from_sync(
        &mut self,
        record: Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(record_id) = record else {
            unreachable!("GetIterator creates an iterator record")
        };
        self.stack.push(Value::Object(record_id));
        let result =
            self.with_roots(|heap| heap.set(record_id, "asyncFromSync", Value::Bool(true)));
        self.stack.pop();
        result?;
        Ok(Value::Object(record_id))
    }

    /// GetIteratorFromMethod for an already-fetched `@@asyncIterator` method.
    pub(in super::super::super) fn async_iterator_record_from_method(
        &mut self,
        value: &Value,
        method: Value,
    ) -> Result<Value, RuntimeError> {
        let iterator = self.call_native(method, value.clone(), Vec::new(), false)?;
        if !matches!(iterator, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "async iterator must be an object".into(),
            ));
        }
        self.stack.push(iterator.clone());
        let next = self.get_property(&iterator, &"next".into())?;
        self.stack.push(next.clone());
        let record = self.with_roots(|heap| heap.alloc_object(None))?;
        self.stack.push(Value::Object(record));
        self.with_roots(|heap| heap.set(record, "iterator", iterator))?;
        self.with_roots(|heap| heap.set(record, "next", next))?;
        self.with_roots(|heap| heap.set(record, "done", Value::Bool(false)))?;
        self.stack.pop();
        self.stack.pop();
        self.stack.pop();
        Ok(Value::Object(record))
    }

    pub(in super::super::super) fn async_iterator_next(
        &mut self,
        record: &Value,
        argument: Option<Value>,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(record) = record else {
            unreachable!("compiler only emits iterator records")
        };
        if matches!(self.heap.get_own(*record, "done")?, Some(Value::Bool(true))) {
            return self.iterator_result(Value::Undefined, true);
        }
        let iterator = self.get_property(&Value::Object(*record), &"iterator".into())?;
        let next = self.get_property(&Value::Object(*record), &"next".into())?;
        let result = self.call_native(next, iterator, argument.into_iter().collect(), false);
        if !matches!(
            self.heap.get_own(*record, "asyncFromSync")?,
            Some(Value::Bool(true))
        ) {
            return result;
        }
        match result {
            Ok(result) => self.async_from_sync_continue(*record, result),
            Err(error) => {
                let error = self.error_value(error)?;
                self.promise_reject(error)
            }
        }
    }

    pub(in super::super::super) fn async_from_sync_handler(
        &mut self,
        function: NativeFunction,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.function_prototype()?;
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

    /// AsyncFromSyncIteratorContinuation. A synchronous iterator result's
    /// `value` is adopted through PromiseResolve before a for-await loop sees
    /// it; rejection closes the original iterator and rejects the public
    /// `next()` capability.
    pub(in super::super::super) fn async_from_sync_continue(
        &mut self,
        record: ObjectId,
        result: Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let outcome = (|| {
            if !matches!(result, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "iterator result must be an object".into(),
                ));
            }
            let done = self.get_property(&result, &"done".into())?;
            let value = self.get_property(&result, &"value".into())?;
            let value_wrapper = self.promise_resolve(value)?;
            let target = self.new_promise()?;
            self.stack
                .extend([value_wrapper.clone(), Value::Object(target)]);
            let fulfilled = self.async_from_sync_handler(NativeFunction::AsyncFromSyncFulfill {
                target,
                done: self.to_boolean(&done)?,
            })?;
            // Each later allocation (the other handler, then_promise's derived
            // promise) can collect, so both handlers stay stack-rooted.
            self.stack.push(fulfilled.clone());
            let rejected = self
                .async_from_sync_handler(NativeFunction::AsyncFromSyncReject { target, record })?;
            self.stack.push(rejected.clone());
            self.promise_then(&value_wrapper, &[fulfilled, rejected])?;
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        match outcome {
            Ok(value) => Ok(value),
            Err(error) => {
                let error = self.error_value(error)?;
                self.promise_reject(error)
            }
        }
    }

    /// Invoke an ordinary iterator's `next` method while retaining the raw
    /// iterator result for synchronous `yield*`. The following bytecode
    /// instruction validates `done`/`value`, matching the split async path.
    pub(in super::super::super) fn iterator_next(
        &mut self,
        record: &Value,
        argument: Option<Value>,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(record) = record else {
            unreachable!("compiler only emits iterator records")
        };
        if matches!(self.heap.get_own(*record, "done")?, Some(Value::Bool(true))) {
            return self.iterator_result(Value::Undefined, true);
        }
        let iterator = self.get_property(&Value::Object(*record), &"iterator".into())?;
        let next = self.get_property(&Value::Object(*record), &"next".into())?;
        self.call_native(next, iterator, argument.into_iter().collect(), false)
    }

    pub(in super::super::super) fn async_iterator_step(
        &mut self,
        record: &Value,
        result: &Value,
    ) -> Result<Option<Value>, RuntimeError> {
        let Value::Object(record) = record else {
            unreachable!("compiler only emits iterator records")
        };
        let outcome = (|| {
            if !matches!(result, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "async iterator result must be an object".into(),
                ));
            }
            let done = self.get_property(result, &"done".into())?;
            if self.to_boolean(&done)? {
                Ok(None)
            } else {
                self.get_property(result, &"value".into()).map(Some)
            }
        })();
        if !matches!(&outcome, Ok(Some(_))) {
            let base = self.stack.len();
            if let Err(RuntimeError::Thrown(value)) = &outcome {
                self.stack.push(value.clone());
            }
            let marked = self.with_roots(|heap| heap.set(*record, "done", Value::Bool(true)));
            self.stack.truncate(base);
            marked?;
        }
        outcome
    }

    /// IteratorStepValue, or IteratorStep without IteratorValue for elisions.
    /// Iterator-origin errors complete this record before outer unwinding.
    pub(in super::super::super) fn iterator_step(
        &mut self,
        record: &Value,
        read_value: bool,
    ) -> Result<Option<Value>, RuntimeError> {
        let Value::Object(record) = record else {
            unreachable!("compiler only emits iterator records")
        };
        if matches!(self.heap.get_own(*record, "done")?, Some(Value::Bool(true))) {
            return Ok(None);
        }
        if self.is_for_in_record(*record)? {
            return self.for_in_step(*record);
        }
        let outcome = (|| {
            let iterator = self.get_property(&Value::Object(*record), &"iterator".into())?;
            let next = self.get_property(&Value::Object(*record), &"next".into())?;
            let result = self.call_native(next, iterator, Vec::new(), false)?;
            if !matches!(result, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "iterator result must be an object".into(),
                ));
            }
            self.stack.push(result.clone());
            let done = self.get_property(&result, &"done".into())?;
            let value = if self.to_boolean(&done)? {
                None
            } else if read_value {
                Some(self.get_property(&result, &"value".into())?)
            } else {
                Some(Value::Undefined)
            };
            self.stack.pop();
            Ok(value)
        })();
        if !matches!(&outcome, Ok(Some(_))) {
            let base = self.stack.len();
            if let Err(RuntimeError::Thrown(value)) = &outcome {
                self.stack.push(value.clone());
            }
            let marked = self.with_roots(|heap| heap.set(*record, "done", Value::Bool(true)));
            self.stack.truncate(base);
            marked?;
        }
        outcome
    }

    pub(in super::super::super) fn iterator_close(
        &mut self,
        record: &Value,
    ) -> Result<(), RuntimeError> {
        let id = record
            .object_id()
            .expect("compiler only emits iterator records");
        if matches!(self.heap.get_own(id, "done")?, Some(Value::Bool(true))) {
            return Ok(());
        }
        self.with_roots(|heap| heap.set(id, "done", Value::Bool(true)))?;
        if self.is_for_in_record(id)? {
            // A for-in record has no ECMAScript iterator to return.
            return Ok(());
        }
        let iterator = self.get_property(record, &"iterator".into())?;
        let close = self.get_method(&iterator, &"return".into())?;
        if close != Value::Undefined {
            let result = self.call_native(close, iterator, Vec::new(), false)?;
            if !matches!(result, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "iterator return must return an object".into(),
                ));
            }
        }
        Ok(())
    }
}
