// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Collector-owned tee algorithms, including real byte/BYOB source demand.
use super::*;
use crate::heap::StreamTee;

impl Vm {
    fn tee_data(&self, tee: ObjectId) -> Result<StreamTee, RuntimeError> {
        match self.heap.web_stream(tee)? {
            WebStreamState::Tee(state) => Ok((**state).clone()),
            _ => Err(RuntimeError::TypeError("Invalid internal tee".into())),
        }
    }
    fn put_tee(&mut self, tee: ObjectId, state: StreamTee) -> Result<(), RuntimeError> {
        self.with_roots(|heap| heap.set_web_stream(tee, WebStreamState::Tee(Box::new(state))))?;
        Ok(())
    }
    pub(super) fn tee_stream(&mut self, source: ObjectId) -> Result<Value, RuntimeError> {
        let bytes = self.stream_data(source)?.byte_stream;
        let base = self.stack.len();
        self.stack.push(Value::Object(source));
        let result = (|| {
            let reader = self.acquire_stream_reader(source, false)?;
            self.stack.push(Value::Object(reader));
            let cancel_promise = self.new_promise()?;
            self.stack.push(Value::Object(cancel_promise));
            let prototype = self.object_prototype;
            let tee = self.with_roots(|heap| {
                heap.alloc_web_stream(
                    WebStreamState::Tee(Box::new(StreamTee {
                        source,
                        reader,
                        branches: [None, None],
                        cancel_promise,
                        canceled: [false, false],
                        reasons: [Value::Undefined, Value::Undefined],
                        reading: false,
                        read_again: [false, false],
                        finished: false,
                        byte_stream: bytes,
                        pending: Value::Undefined,
                        pending_branch: None,
                    })),
                    prototype,
                )
            })?;
            self.stack.push(Value::Object(tee));
            let function = self.function_prototype()?;
            for branch in 0..2 {
                let pull = self.with_roots(|heap| {
                    heap.alloc_native_function(
                        NativeFunction::TeePull { tee, branch },
                        "",
                        function,
                    )
                })?;
                self.stack.push(Value::Object(pull));
                let cancel = self.with_roots(|heap| {
                    heap.alloc_native_function(
                        NativeFunction::TeeCancel { tee, branch },
                        "",
                        function,
                    )
                })?;
                self.stack.push(Value::Object(cancel));
                let mut state = Self::empty_stream(bytes, if bytes { 0.0 } else { 1.0 });
                state.pull = Value::Object(pull);
                state.cancel = Value::Object(cancel);
                let stream = self.allocate_stream(state)?;
                self.stack.push(Value::Object(stream));
                let mut state = self.tee_data(tee)?;
                state.branches[branch as usize] = Some(stream);
                self.put_tee(tee, state)?;
            }
            self.watch_tee_promise(
                tee,
                reader,
                1,
                Value::Object(self.stream_reader(reader)?.closed),
            )?;
            let branches = self.tee_data(tee)?.branches;
            for stream in branches.into_iter().flatten() {
                self.watch_stream_promise(stream, 0, Value::Undefined)?;
            }
            let prototype = self.array_prototype;
            let array = self.with_roots(|heap| heap.alloc_array(2, Some(prototype)))?;
            self.stack.push(Value::Object(array));
            for (index, stream) in branches.into_iter().enumerate() {
                self.define_data(
                    array,
                    index.to_string(),
                    Value::Object(stream.unwrap()),
                    true,
                    true,
                    true,
                )?;
            }
            Ok(Value::Object(array))
        })();
        self.stack.truncate(base);
        result
    }
    fn watch_tee_promise(
        &mut self,
        tee: ObjectId,
        reader: ObjectId,
        action: u8,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        self.stack
            .extend([Value::Object(tee), Value::Object(reader), value.clone()]);
        let result = (|| {
            let promise = self.promise_resolve(value)?.object_id().unwrap();
            self.stack.push(Value::Object(promise));
            let function = self.function_prototype()?;
            let mut callbacks = Vec::new();
            for rejected in [false, true] {
                let callback = self.with_roots(|heap| {
                    heap.alloc_native_function(
                        NativeFunction::TeeReaction {
                            tee,
                            reader,
                            action,
                            rejected,
                        },
                        "",
                        function,
                    )
                })?;
                self.stack.push(Value::Object(callback));
                callbacks.push(Value::Object(callback));
            }
            let target = self.new_promise()?;
            self.stack.push(Value::Object(target));
            self.perform_promise_then(
                promise,
                callbacks[0].clone(),
                callbacks[1].clone(),
                ReactionTarget::Native(target),
            )
        })();
        self.stack.truncate(base);
        result
    }
    pub(in super::super) fn tee_pull(
        &mut self,
        tee: ObjectId,
        branch: u8,
    ) -> Result<Value, RuntimeError> {
        let mut state = self.tee_data(tee)?;
        let index = branch as usize;
        if state.finished || state.canceled[index] {
            return Ok(Value::Undefined);
        }
        if state.reading {
            state.read_again[index] = true;
            self.put_tee(tee, state)?;
            return Ok(Value::Undefined);
        }
        let base = self.stack.len();
        self.stack.push(Value::Object(tee));
        let result = (|| {
            let stream = state.branches[index].unwrap();
            let request = if state.byte_stream {
                self.stream_byob_request(self.stream_data(stream)?.controller.unwrap())?
            } else {
                Value::Null
            };
            self.stack.push(request.clone());
            let view = if let Some(request) = request.object_id() {
                match self.heap.web_stream(request)? {
                    WebStreamState::Request { view, .. } => view.map(Value::Object),
                    _ => unreachable!(),
                }
            } else {
                None
            };
            if let Some(view) = &view {
                self.stack.push(view.clone());
            }
            let byob = view.is_some();
            if self.stream_reader(state.reader)?.byob != byob {
                self.release_stream_reader(state.reader)?;
                state.reader = self.acquire_stream_reader(state.source, byob)?;
                self.stack.push(Value::Object(state.reader));
                self.put_tee(tee, state.clone())?;
                self.watch_tee_promise(
                    tee,
                    state.reader,
                    1,
                    Value::Object(self.stream_reader(state.reader)?.closed),
                )?;
            }
            state.reading = true;
            state.pending_branch = byob.then_some(branch);
            let reader = state.reader;
            self.put_tee(tee, state)?;
            let read = self.read_stream(reader, &view.into_iter().collect::<Vec<_>>())?;
            self.stack.push(read.clone());
            self.watch_tee_promise(tee, reader, 0, read)?;
            Ok(Value::Undefined)
        })();
        self.stack.truncate(base);
        result
    }
    pub(in super::super) fn tee_cancel(
        &mut self,
        tee: ObjectId,
        branch: u8,
        reason: Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.extend([Value::Object(tee), reason.clone()]);
        let result = (|| {
            let mut state = self.tee_data(tee)?;
            state.canceled[branch as usize] = true;
            state.reasons[branch as usize] = reason;
            let both = state.canceled.iter().all(|canceled| *canceled);
            let source = state.source;
            let promise = state.cancel_promise;
            let reasons = state.reasons.clone();
            if both {
                state.finished = true;
            }
            self.put_tee(tee, state)?;
            if both {
                let prototype = self.array_prototype;
                let array = self.with_roots(|heap| heap.alloc_array(2, Some(prototype)))?;
                self.stack.push(Value::Object(array));
                for (index, reason) in reasons.into_iter().enumerate() {
                    self.define_data(array, index.to_string(), reason, true, true, true)?;
                }
                let canceled = self.cancel_stream(source, Value::Object(array))?;
                self.stack.push(canceled.clone());
                self.resolve_promise(promise, canceled)?;
            }
            Ok(Value::Object(promise))
        })();
        self.stack.truncate(base);
        result
    }
    fn tee_error(
        &mut self,
        tee: ObjectId,
        reason: Value,
        cancel_source: bool,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        self.stack.extend([Value::Object(tee), reason.clone()]);
        let result = (|| {
            let mut state = self.tee_data(tee)?;
            if state.finished {
                return Ok(());
            }
            state.finished = true;
            state.reading = false;
            state.pending = Value::Undefined;
            let branches = state.branches;
            let promise = state.cancel_promise;
            let source = state.source;
            let resolve = state.canceled.iter().any(|canceled| !*canceled);
            self.put_tee(tee, state)?;
            for stream in branches.into_iter().flatten() {
                self.error_stream(stream, reason.clone())?;
            }
            if cancel_source {
                let canceled = self.cancel_stream(source, reason.clone())?;
                self.stack.push(canceled.clone());
                self.resolve_promise(promise, canceled)?;
            } else if resolve {
                self.resolve_promise(promise, Value::Undefined)?;
            }
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }
    pub(in super::super) fn tee_reaction(
        &mut self,
        tee: ObjectId,
        reader: ObjectId,
        action: u8,
        rejected: bool,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        let state = self.tee_data(tee)?;
        if state.finished || state.reader != reader {
            return Ok(Value::Undefined);
        }
        if rejected {
            self.tee_error(tee, value, false)?;
            return Ok(Value::Undefined);
        }
        if action == 1 {
            return Ok(Value::Undefined);
        }
        if action == 0 {
            let mut state = self.tee_data(tee)?;
            state.pending = value;
            self.put_tee(tee, state)?;
            // The extra microtask lets reader.closed rejection win before an
            // already available chunk can fulfill a branch read.
            self.watch_tee_promise(tee, reader, 2, Value::Undefined)?;
        } else {
            if let Err(error) = self.deliver_tee_read(tee) {
                if !super::super::web_drag_drop::drag_page_exception(&error) {
                    return Err(error);
                }
                // Enqueue may already have errored one branch. Propagate
                // that exact reason to both branches and the source.
                let state = self.tee_data(tee)?;
                let mut reason = None;
                for branch in state.branches.into_iter().flatten() {
                    let branch = self.stream_data(branch)?;
                    if branch.status == 2 {
                        reason = Some(branch.error);
                        break;
                    }
                }
                let reason = match reason {
                    Some(reason) => reason,
                    None => self.error_value(error)?,
                };
                self.tee_error(tee, reason, true)?;
            }
        }
        Ok(Value::Undefined)
    }
    fn deliver_tee_read(&mut self, tee: ObjectId) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        self.stack.push(Value::Object(tee));
        let result = (|| {
            let mut state = self.tee_data(tee)?;
            let pending = state.pending.clone();
            self.stack.push(pending.clone());
            let result = pending.object_id().expect("owned stream read result");
            let chunk = self.heap.get_own(result, "value")?.unwrap();
            self.stack.push(chunk.clone());
            let done = self.heap.get_own(result, "done")? == Some(Value::Bool(true));
            state.pending = Value::Undefined;
            state.read_again = [false, false];
            self.put_tee(tee, state.clone())?;
            if done {
                state.finished = true;
                state.reading = false;
                self.put_tee(tee, state.clone())?;
                for index in 0..2 {
                    if state.canceled[index] {
                        continue;
                    }
                    let stream = state.branches[index].unwrap();
                    if let Err(error) = self.close_stream(stream, !state.byte_stream) {
                        if !super::super::web_drag_drop::drag_page_exception(&error) {
                            return Err(error);
                        }
                        continue;
                    }
                    if state.byte_stream {
                        let controller = self.stream_data(stream)?.controller.unwrap();
                        let request = self.stream_byob_request(controller)?;
                        self.stack.push(request.clone());
                        if let Some(request) = request.object_id() {
                            if state.pending_branch == Some(index as u8)
                                && chunk != Value::Undefined
                            {
                                self.respond_stream(request, &chunk, true)?;
                            } else {
                                self.respond_stream(request, &Value::Number(0.0), false)?;
                            }
                        }
                    }
                }
                if state.canceled.iter().any(|canceled| !*canceled) {
                    self.resolve_promise(state.cancel_promise, Value::Undefined)?;
                }
                return Ok(());
            }
            let active = [!state.canceled[0], !state.canceled[1]];
            if state.byte_stream {
                let cloned = if active[0] && active[1]
                    || state.pending_branch.is_some()
                        && active[(1 - state.pending_branch.unwrap()) as usize]
                {
                    let (buffer, offset, length, _) = self.stream_view_info(&chunk)?;
                    let bytes = self.heap.array_buffer_copy(buffer, offset, length)?;
                    let clone = self.stream_bytes_value(&bytes)?;
                    self.stack.push(clone.clone());
                    Some(clone)
                } else {
                    None
                };
                if let Some(index) = state.pending_branch {
                    let other = 1 - index;
                    if active[index as usize] {
                        let stream = state.branches[index as usize].unwrap();
                        let request = self
                            .stream_byob_request(self.stream_data(stream)?.controller.unwrap())?
                            .object_id()
                            .unwrap();
                        self.respond_stream(request, &chunk, true)?;
                    }
                    if active[other as usize] {
                        self.enqueue_stream(
                            state.branches[other as usize].unwrap(),
                            cloned.unwrap(),
                        )?;
                    }
                } else {
                    for index in 0..2 {
                        if active[index] {
                            let value = if index == 1 && active[0] {
                                cloned.clone().unwrap()
                            } else {
                                chunk.clone()
                            };
                            self.enqueue_stream(state.branches[index].unwrap(), value)?;
                        }
                    }
                }
            } else {
                for (index, active) in active.into_iter().enumerate() {
                    if active {
                        self.enqueue_stream(state.branches[index].unwrap(), chunk.clone())?;
                    }
                }
            }
            let mut state = self.tee_data(tee)?;
            state.reading = false;
            let again = state.read_again;
            self.put_tee(tee, state)?;
            if again[0] {
                self.tee_pull(tee, 0)?;
            } else if again[1] {
                self.tee_pull(tee, 1)?;
            }
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }
}
