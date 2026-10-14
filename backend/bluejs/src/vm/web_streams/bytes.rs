// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Byte queues, transferred BYOB views and bounded Blob source pulls.
use super::*;

impl Vm {
    pub(super) fn stream_view_info(
        &self,
        view: &Value,
    ) -> Result<(ObjectId, usize, usize, Option<TypedArrayKind>), RuntimeError> {
        self.stream_view_info_allow_empty(view, false)
    }
    fn stream_view_info_allow_empty(
        &self,
        view: &Value,
        allow_empty: bool,
    ) -> Result<(ObjectId, usize, usize, Option<TypedArrayKind>), RuntimeError> {
        let id = view
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Read requires an ArrayBufferView".into()))?;
        let (buffer, offset, length, kind) = if self.heap.is_typed_array(id)? {
            let (buffer, offset, elements, kind) = self.heap.typed_array_info(id)?;
            if self.heap.typed_array_is_out_of_bounds(id)? {
                return Err(RuntimeError::TypeError("View is out of bounds".into()));
            }
            (buffer, offset, elements * kind.byte_width(), Some(kind))
        } else if self.heap.is_data_view(id)? {
            let (buffer, offset, length) = self.heap.data_view_current_info(id)?;
            (buffer, offset, length, None)
        } else {
            return Err(RuntimeError::TypeError(
                "Read requires an ArrayBufferView".into(),
            ));
        };
        if self.heap.buffer_is_detached(buffer)?
            || self.heap.is_shared_array_buffer(buffer)?
            || length == 0 && !allow_empty
        {
            return Err(RuntimeError::TypeError(
                "View is empty, detached or shared".into(),
            ));
        }
        Ok((buffer, offset, length, kind))
    }
    fn stream_transfer_buffer(&mut self, buffer: ObjectId) -> Result<ObjectId, RuntimeError> {
        let length = self.heap.buffer_byte_length(buffer)?;
        let data = self.heap.array_buffer_copy(buffer, 0, length)?;
        let prototype = self.buffer_prototype("ArrayBuffer")?;
        let copy = self.with_roots(|heap| heap.alloc_array_buffer(length, Some(prototype)))?;
        self.heap.array_buffer_write(copy, 0, &data)?;
        self.heap.detach_array_buffer(buffer)?;
        Ok(copy)
    }
    pub(super) fn prepare_stream_byob(
        &mut self,
        view: &Value,
        options: &Value,
    ) -> Result<PullInto, RuntimeError> {
        let (buffer, offset, length, kind) = self.stream_view_info(view)?;
        let minimum = self.stream_option(options, "min")?;
        let n = if minimum == Value::Undefined {
            1.0
        } else {
            self.coerce_number(&minimum)?
        };
        let width = kind.map_or(1, |kind| kind.byte_width());
        if !n.is_finite() || n < 1.0 || n.fract() != 0.0 || n > (length / width) as f64 {
            return Err(RuntimeError::RangeError(
                "Invalid minimum read length".into(),
            ));
        }
        // Conversion may run user code and detach/resize the original view.
        self.stream_view_info(view)?;
        let buffer = self.stream_transfer_buffer(buffer)?;
        Ok(PullInto {
            buffer,
            buffer_length: self.heap.buffer_byte_length(buffer)?,
            offset,
            length,
            filled: 0,
            minimum: n as usize * width,
            kind,
        })
    }
    pub(super) fn allocate_pull_into(&mut self, length: usize) -> Result<PullInto, RuntimeError> {
        let prototype = self.buffer_prototype("ArrayBuffer")?;
        let buffer = self.with_roots(|heap| heap.alloc_array_buffer(length, Some(prototype)))?;
        Ok(PullInto {
            buffer,
            buffer_length: length,
            offset: 0,
            length,
            filled: 0,
            minimum: 1,
            kind: Some(TypedArrayKind::Uint8),
        })
    }
    pub(super) fn stream_into_value(&mut self, into: &PullInto) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(Value::Object(into.buffer));
        let result = (|| {
            let value = if let Some(kind) = into.kind {
                let prototype = self.buffer_prototype(kind.name())?;
                self.with_roots(|heap| {
                    heap.alloc_typed_array(
                        into.buffer,
                        into.offset,
                        into.filled / kind.byte_width(),
                        false,
                        kind,
                        Some(prototype),
                    )
                })?
            } else {
                let prototype = self.buffer_prototype("DataView")?;
                self.with_roots(|heap| {
                    heap.alloc_data_view(
                        into.buffer,
                        into.offset,
                        into.filled,
                        false,
                        Some(prototype),
                    )
                })?
            };
            Ok(Value::Object(value))
        })();
        self.stack.truncate(base);
        result
    }
    fn stream_uint8_view(
        &mut self,
        buffer: ObjectId,
        offset: usize,
        length: usize,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(Value::Object(buffer));
        let prototype = self.buffer_prototype("Uint8Array");
        let result = prototype.and_then(|prototype| {
            self.with_roots(|heap| {
                heap.alloc_typed_array(
                    buffer,
                    offset,
                    length,
                    false,
                    TypedArrayKind::Uint8,
                    Some(prototype),
                )
            })
            .map(Value::Object)
        });
        self.stack.truncate(base);
        result
    }
    pub(super) fn enqueue_stream_bytes(
        &mut self,
        stream: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let (buffer, offset, length, _) = self.stream_view_info(&value)?;
        let base = self.stack.len();
        self.stack.extend([Value::Object(stream), value]);
        let result = (|| {
            let buffer = self.stream_transfer_buffer(buffer)?;
            self.stack.push(Value::Object(buffer));
            let value = self.stream_uint8_view(buffer, offset, length)?;
            self.stack.push(value.clone());
            let mut state = self.stream_data(stream)?;
            if let Some(reader) = state.reader {
                let mut owner = self.stream_reader(reader)?;
                if let Some(into) = owner.reads.front_mut().and_then(|read| read.into.as_mut()) {
                    into.buffer = self.stream_transfer_buffer(into.buffer)?;
                    self.stack.push(Value::Object(into.buffer));
                    self.put_reader(reader, owner)?;
                    self.invalidate_stream_request(stream)?;
                }
            }
            if state.queue.len() >= 4096 {
                return Err(RuntimeError::RangeError(
                    "Byte stream queue limit exceeded".into(),
                ));
            }
            state.queue.push_back((value, length as f64));
            state.queue_size += length as f64;
            self.put_stream(stream, state)?;
            self.process_stream_reads(stream)?;
            self.stream_maybe_pull(stream)
        })();
        self.stack.truncate(base);
        result
    }
    pub(super) fn process_stream_reads(&mut self, stream: ObjectId) -> Result<(), RuntimeError> {
        loop {
            let mut state = self.stream_data(stream)?;
            let Some(reader) = state.reader else {
                return Ok(());
            };
            let mut owner = self.stream_reader(reader)?;
            let Some(read) = owner.reads.front().cloned() else {
                if state.close_requested && state.queue.is_empty() && state.status == 0 {
                    let force = state.blob.is_some();
                    self.close_stream(stream, force)?;
                }
                return Ok(());
            };
            if state.status == 2 {
                owner.reads.pop_front();
                self.settle_tracked_promise(read.promise, PromiseStatus::Rejected(state.error));
                self.put_reader(reader, owner)?;
                continue;
            }
            if state.status == 0
                && state.close_requested
                && state.queue.is_empty()
                && read.into.is_none()
            {
                self.close_stream(stream, state.blob.is_some())?;
                continue;
            }
            let base = self.stack.len();
            self.stack.push(Value::Object(read.promise));
            if let Some(into) = &read.into {
                self.stack.push(Value::Object(into.buffer));
            }
            let result: Result<bool, RuntimeError> = (|| {
                if state.status == 1 {
                    owner.reads.pop_front();
                    self.put_reader(reader, owner)?;
                    let value = if let Some(into) = &read.into {
                        self.stream_into_value(into)?
                    } else {
                        Value::Undefined
                    };
                    self.settle_stream_read(read, value, true)?;
                    return Ok(true);
                }
                if !state.byte_stream {
                    let Some((value, size)) = state.queue.pop_front() else {
                        return Ok(false);
                    };
                    self.stack.push(value.clone());
                    state.queue_size = (state.queue_size - size).max(0.0);
                    owner.reads.pop_front();
                    self.put_stream(stream, state)?;
                    self.put_reader(reader, owner)?;
                    self.settle_stream_read(read, value, false)?;
                    return Ok(true);
                }
                if read.into.is_none() || !owner.byob {
                    let Some((value, size)) = state.queue.pop_front() else {
                        return Ok(false);
                    };
                    self.stack.push(value.clone());
                    state.queue_size = (state.queue_size - size).max(0.0);
                    owner.reads.pop_front();
                    self.put_stream(stream, state)?;
                    self.put_reader(reader, owner)?;
                    self.settle_stream_read(read, value, false)?;
                    return Ok(true);
                }
                let mut into = read.into.clone().unwrap();
                let width = into.kind.map_or(1, |kind| kind.byte_width());
                while into.filled < into.length {
                    let Some((chunk, _)) = state.queue.pop_front() else {
                        break;
                    };
                    self.stack.push(chunk.clone());
                    let (buffer, offset, length, _) = self.stream_view_info(&chunk)?;
                    let take = length.min(into.length - into.filled);
                    let bytes = self.heap.array_buffer_copy(buffer, offset, take)?;
                    self.heap
                        .array_buffer_write(into.buffer, into.offset + into.filled, &bytes)?;
                    into.filled += take;
                    state.queue_size = (state.queue_size - take as f64).max(0.0);
                    if take < length {
                        let rest = self.stream_uint8_view(buffer, offset + take, length - take)?;
                        self.stack.push(rest.clone());
                        state.queue.push_front((rest, (length - take) as f64));
                    }
                    if into.filled >= into.minimum {
                        break;
                    }
                }
                self.put_stream(stream, state.clone())?;
                owner.reads.front_mut().unwrap().into = Some(into.clone());
                self.put_reader(reader, owner.clone())?;
                self.invalidate_stream_request(stream)?;
                let at_end = state.close_requested && state.queue.is_empty();
                if at_end && into.filled % width != 0 {
                    let reason =
                        self.stream_type_error("Byte stream closed with a partial element")?;
                    self.error_stream(stream, reason)?;
                    return Ok(true);
                }
                if into.filled >= into.minimum || at_end && into.filled > 0 {
                    // Return only complete typed-array elements; any trailing
                    // bytes remain available to the next read.
                    let remainder = into.filled % width;
                    if remainder > 0 {
                        let data = self.heap.array_buffer_copy(
                            into.buffer,
                            into.offset + into.filled - remainder,
                            remainder,
                        )?;
                        let tail = self.stream_bytes_value(&data)?;
                        self.stack.push(tail.clone());
                        let mut state = self.stream_data(stream)?;
                        state.queue.push_front((tail, remainder as f64));
                        state.queue_size += remainder as f64;
                        self.put_stream(stream, state)?;
                        into.filled -= remainder;
                    }
                    owner.reads.pop_front();
                    self.put_reader(reader, owner)?;
                    let value = self.stream_into_value(&into)?;
                    self.settle_stream_read(read, value, false)?;
                    Ok(true)
                } else if at_end {
                    self.close_stream(stream, state.blob.is_some())?;
                    Ok(false)
                } else {
                    Ok(false)
                }
            })();
            self.stack.truncate(base);
            if !result? {
                return Ok(());
            }
        }
    }
    pub(super) fn invalidate_stream_request(
        &mut self,
        stream: ObjectId,
    ) -> Result<(), RuntimeError> {
        let Some(controller) = self.stream_data(stream)?.controller else {
            return Ok(());
        };
        let WebStreamState::Controller {
            request,
            byte_stream,
            ..
        } = self.heap.web_stream(controller)?.clone()
        else {
            unreachable!()
        };
        if let Some(request) = request {
            self.with_roots(|heap| {
                heap.set_web_stream(
                    request,
                    WebStreamState::Request {
                        stream: None,
                        view: None,
                    },
                )
            })?;
            self.with_roots(|heap| {
                heap.set_web_stream(
                    controller,
                    WebStreamState::Controller {
                        stream,
                        byte_stream,
                        request: None,
                    },
                )
            })?;
        }
        Ok(())
    }
    pub(super) fn stream_byob_request(
        &mut self,
        controller: ObjectId,
    ) -> Result<Value, RuntimeError> {
        let WebStreamState::Controller {
            stream,
            byte_stream: true,
            request,
        } = self.heap.web_stream(controller)?.clone()
        else {
            return Err(RuntimeError::TypeError("Invalid byte controller".into()));
        };
        if let Some(request) = request {
            return Ok(Value::Object(request));
        }
        let state = self.stream_data(stream)?;
        let Some(reader) = state.reader else {
            return Ok(Value::Null);
        };
        let owner = self.stream_reader(reader)?;
        let Some(into) = owner.reads.front().and_then(|read| read.into.as_ref()) else {
            return Ok(Value::Null);
        };
        let base = self.stack.len();
        let result = (|| {
            let view = self.stream_uint8_view(
                into.buffer,
                into.offset + into.filled,
                into.length - into.filled,
            )?;
            self.stack.push(view.clone());
            let prototype = self.globals["%ReadableStreamBYOBRequest%"];
            let request = self.with_roots(|heap| {
                heap.alloc_web_stream(
                    WebStreamState::Request {
                        stream: Some(stream),
                        view: view.object_id(),
                    },
                    prototype,
                )
            })?;
            self.with_roots(|heap| {
                heap.set_web_stream(
                    controller,
                    WebStreamState::Controller {
                        stream,
                        byte_stream: true,
                        request: Some(request),
                    },
                )
            })?;
            Ok(Value::Object(request))
        })();
        self.stack.truncate(base);
        result
    }
    pub(super) fn respond_stream(
        &mut self,
        request: ObjectId,
        value: &Value,
        new_view: bool,
    ) -> Result<(), RuntimeError> {
        let WebStreamState::Request {
            stream: Some(stream),
            view: Some(view),
        } = self.heap.web_stream(request)?.clone()
        else {
            return Err(RuntimeError::TypeError("BYOB request is inactive".into()));
        };
        let reader = self
            .stream_data(stream)?
            .reader
            .ok_or_else(|| RuntimeError::TypeError("BYOB reader is released".into()))?;
        let mut owner = self.stream_reader(reader)?;
        let into = owner
            .reads
            .front_mut()
            .and_then(|r| r.into.as_mut())
            .ok_or_else(|| RuntimeError::TypeError("Missing BYOB read".into()))?;
        let count = if new_view {
            let (buffer, offset, length, _) = self.stream_view_info_allow_empty(value, true)?;
            if offset != into.offset + into.filled
                || length > into.length - into.filled
                || self.heap.buffer_byte_length(buffer)? != into.buffer_length
            {
                return Err(RuntimeError::RangeError(
                    "Replacement BYOB view does not match".into(),
                ));
            }
            length
        } else {
            let n = self.coerce_number(value)?;
            if !n.is_finite()
                || n < 0.0
                || n.fract() != 0.0
                || n > (into.length - into.filled) as f64
            {
                return Err(RuntimeError::RangeError(
                    "Invalid BYOB response length".into(),
                ));
            }
            n as usize
        };
        let status = self.stream_data(stream)?.status;
        if status == 0 && count == 0 || status == 1 && count != 0 {
            return Err(RuntimeError::TypeError(
                "BYOB response length does not match stream state".into(),
            ));
        }
        // Respond transfers the writable backing before exposing the result.
        self.heap.array_buffer_is_detached(into.buffer)?;
        let buffer = if new_view {
            self.stream_view_info_allow_empty(value, true)?.0
        } else {
            self.stream_view_info(&Value::Object(view))?.0
        };
        into.buffer = self.stream_transfer_buffer(buffer)?;
        into.filled += count;
        let filled = into.filled;
        let width = into.kind.map_or(1, |k| k.byte_width());
        let minimum = into.minimum;
        let base = self.stack.len();
        self.stack.push(Value::Object(into.buffer));
        let result = (|| {
            self.put_reader(reader, owner.clone())?;
            self.invalidate_stream_request(stream)?;
            if status == 1 || filled >= minimum && filled % width == 0 {
                let read = owner.reads.pop_front().unwrap();
                let into = read.into.as_ref().unwrap();
                let value = self.stream_into_value(into)?;
                self.stack.push(value.clone());
                self.put_reader(reader, owner)?;
                self.settle_stream_read(read, value, status == 1)?;
            }
            self.stream_maybe_pull(stream)
        })();
        self.stack.truncate(base);
        result
    }
    pub(super) fn stream_bytes_value(&mut self, bytes: &[u8]) -> Result<Value, RuntimeError> {
        let prototype = self.buffer_prototype("ArrayBuffer")?;
        let buffer =
            self.with_roots(|heap| heap.alloc_array_buffer(bytes.len(), Some(prototype)))?;
        self.heap.array_buffer_write(buffer, 0, bytes)?;
        self.stream_uint8_view(buffer, 0, bytes.len())
    }
    pub(super) fn pull_blob_stream(&mut self, stream: ObjectId) -> Result<(), RuntimeError> {
        let mut state = self.stream_data(stream)?;
        let blob = state.blob.unwrap();
        let data = self.heap.web_blob(blob)?;
        if state.blob_offset == data.bytes.len() {
            return self.close_stream(stream, true);
        }
        let end = (state.blob_offset + 65_536).min(data.bytes.len());
        let bytes = data.bytes[state.blob_offset..end].to_vec();
        let at_end = end == data.bytes.len();
        state.blob_offset = end;
        let text = state.text_stream;
        state.pulling = true;
        // The actual text/byte value is produced before publishing the next
        // source offset; the stream is rooted while its result allocates.
        let base = self.stack.len();
        self.stack.push(Value::Object(stream));
        let result = (|| {
            let value = if text {
                let mut input = std::mem::take(&mut state.text_tail);
                input.extend_from_slice(&bytes);
                let mut output = String::new();
                let mut rest = input.as_slice();
                if self.stream_data(stream)?.blob_offset == 0 {
                    rest = rest.strip_prefix(&[239, 187, 191]).unwrap_or(rest);
                }
                while !rest.is_empty() {
                    match std::str::from_utf8(rest) {
                        Ok(valid) => {
                            output.push_str(valid);
                            break;
                        }
                        Err(error) => {
                            output.push_str(
                                std::str::from_utf8(&rest[..error.valid_up_to()])
                                    .expect("validated prefix"),
                            );
                            rest = &rest[error.valid_up_to()..];
                            if let Some(length) = error.error_len() {
                                output.push('\u{fffd}');
                                rest = &rest[length..];
                            } else if at_end {
                                output.push('\u{fffd}');
                                break;
                            } else {
                                state.text_tail = rest.to_vec();
                                break;
                            }
                        }
                    }
                }
                Value::String(output.into())
            } else {
                self.stream_bytes_value(&bytes)?
            };
            self.stack.push(value.clone());
            self.put_stream(stream, state)?;
            self.enqueue_stream(stream, value)?;
            if at_end {
                self.close_stream(stream, true)?;
            }
            self.watch_stream_promise(stream, 1, Value::Undefined)
        })();
        self.stack.truncate(base);
        result
    }
}
