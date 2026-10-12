// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! GC-owned readable streams and actual Promise-based reader operations.
//! Behavioral reference: https://streams.spec.whatwg.org/
use super::*;
use crate::heap::{PullInto, StreamData, StreamRead, StreamReader, TypedArrayKind, WebStreamState};
use crate::native::StreamNative;

mod bytes;
mod tee;
use StreamNative::*;
const NAMES: [&str; 6] = [
    "ReadableStream",
    "ReadableStreamDefaultReader",
    "ReadableStreamBYOBReader",
    "ReadableStreamDefaultController",
    "ReadableByteStreamController",
    "ReadableStreamBYOBRequest",
];

impl Vm {
    pub(super) fn install_web_streams(&mut self) -> Result<(), RuntimeError> {
        if self.globals.contains_key("%ReadableStream%") {
            return Ok(());
        }
        let global = self.global("globalThis")?.object_id().unwrap();
        for name in NAMES {
            if self.heap.get_own(global, name)?.is_some() {
                return Err(RuntimeError::TypeError(
                    "Stream global already defined".into(),
                ));
            }
        }
        let base = self.stack.len();
        let result = (|| {
            let function = self.function_prototype()?;
            for (rank, name) in NAMES.iter().enumerate() {
                let object = self.object_prototype;
                let prototype = self.with_roots(|heap| heap.alloc_object(Some(object)))?;
                self.stack.push(Value::Object(prototype));
                let getters: &[(&str, StreamNative)] = match rank {
                    0 => &[("locked", Locked)],
                    1 | 2 => &[("closed", Closed)],
                    3 => &[("desiredSize", DesiredSize)],
                    4 => &[("desiredSize", DesiredSize), ("byobRequest", ByobRequest)],
                    _ => &[("view", View)],
                };
                let methods: &[(&str, u32, StreamNative)] = match rank {
                    0 => &[
                        ("getReader", 0, GetReader),
                        ("cancel", 0, Cancel),
                        ("tee", 0, Tee),
                    ],
                    1 | 2 => &[
                        ("read", if rank == 1 { 0 } else { 1 }, Read),
                        ("cancel", 0, Cancel),
                        ("releaseLock", 0, Release),
                    ],
                    3 | 4 => &[
                        ("enqueue", 1, Enqueue),
                        ("close", 0, Close),
                        ("error", 0, Error),
                    ],
                    _ => &[
                        ("respond", 1, Respond(false)),
                        ("respondWithNewView", 1, Respond(true)),
                    ],
                };
                for (name, native) in getters {
                    self.install_native_getter(
                        prototype,
                        function,
                        name,
                        NativeFunction::WebStream(*native),
                    )?;
                }
                for (name, length, native) in methods {
                    self.install_native(
                        prototype,
                        function,
                        name,
                        *length,
                        NativeFunction::WebStream(*native),
                    )?;
                }
                for name in getters
                    .iter()
                    .map(|(n, _)| *n)
                    .chain(methods.iter().map(|(n, _, _)| *n))
                {
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
                    Value::String((*name).into()),
                    false,
                    false,
                    true,
                )?;
                let constructor = self.web_file_constructor(
                    name,
                    if matches!(rank, 1 | 2) { 1 } else { 0 },
                    NativeFunction::WebStream(Constructor(rank as u8)),
                    function,
                    prototype,
                )?;
                self.stack.push(Value::Object(constructor));
                self.define_data(global, *name, Value::Object(constructor), true, false, true)?;
                self.heap.root(prototype)?;
                self.globals.insert(format!("%{name}%"), prototype);
            }
            let blob = self.globals["%WebBlob%"];
            for (name, text) in [("stream", false), ("textStream", true)] {
                self.install_native(
                    blob,
                    function,
                    name,
                    0,
                    NativeFunction::WebStream(Blob(text)),
                )?;
                self.with_roots(|heap| {
                    heap.define_own_property(
                        blob,
                        name,
                        PropertyDescriptor {
                            enumerable: Some(true),
                            ..Default::default()
                        },
                    )
                })?;
            }
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }
    fn stream_data(&self, id: ObjectId) -> Result<StreamData, RuntimeError> {
        match self.heap.web_stream(id)? {
            WebStreamState::Stream(s) => Ok((**s).clone()),
            _ => Err(RuntimeError::TypeError(
                "Invalid ReadableStream receiver".into(),
            )),
        }
    }
    fn stream_reader(&self, id: ObjectId) -> Result<StreamReader, RuntimeError> {
        match self.heap.web_stream(id)? {
            WebStreamState::Reader(r) => Ok(r.clone()),
            _ => Err(RuntimeError::TypeError("Invalid stream reader".into())),
        }
    }
    fn put_stream(&mut self, id: ObjectId, state: StreamData) -> Result<(), RuntimeError> {
        self.with_roots(|heap| heap.set_web_stream(id, WebStreamState::Stream(Box::new(state))))
    }
    fn put_reader(&mut self, id: ObjectId, state: StreamReader) -> Result<(), RuntimeError> {
        self.with_roots(|heap| heap.set_web_stream(id, WebStreamState::Reader(state)))
    }
    fn stream_type_error(&mut self, message: &str) -> Result<Value, RuntimeError> {
        self.error_value(RuntimeError::TypeError(message.into()))
    }
    fn stream_rejected(&mut self, message: &str) -> Result<Value, RuntimeError> {
        let value = self.stream_type_error(message)?;
        self.promise_reject(value)
    }
    fn controller_stream(&self, id: ObjectId) -> Result<ObjectId, RuntimeError> {
        match self.heap.web_stream(id)? {
            WebStreamState::Controller { stream, .. } => Ok(*stream),
            _ => Err(RuntimeError::TypeError("Invalid stream controller".into())),
        }
    }
    pub(super) fn web_stream_call(
        &mut self,
        native: StreamNative,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if let Constructor(rank) = native {
            if !construct {
                return Err(RuntimeError::TypeError(
                    "Stream constructor requires new".into(),
                ));
            }
            return match rank {
                0 => self.construct_stream(args),
                1 | 2 => {
                    let stream = native::argument(args, 0).object_id().ok_or_else(|| {
                        RuntimeError::TypeError("Reader requires a stream".into())
                    })?;
                    self.acquire_stream_reader(stream, rank == 2)
                        .map(Value::Object)
                }
                _ => Err(RuntimeError::TypeError(
                    "Stream controllers cannot be constructed".into(),
                )),
            };
        }
        let id = receiver
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Invalid stream receiver".into()))?;
        if let Blob(text) = native {
            self.heap.web_blob(id)?;
            let mut state = Self::empty_stream(!text, if text { 1.0 } else { 0.0 });
            state.started = true;
            state.blob = Some(id);
            state.text_stream = text;
            return self.allocate_stream(state).map(Value::Object);
        }
        match native {
            Tee => self.tee_stream(id),
            Locked => Ok(Value::Bool(self.stream_data(id)?.reader.is_some())),
            GetReader => {
                let options = native::argument(args, 0);
                let mode = self.stream_option(options, "mode")?;
                let byob = if mode == Value::Undefined {
                    false
                } else if self.coerce_string(&mode)? == "byob" {
                    true
                } else {
                    return Err(RuntimeError::TypeError("Invalid reader mode".into()));
                };
                self.acquire_stream_reader(id, byob).map(Value::Object)
            }
            Closed => Ok(Value::Object(self.stream_reader(id)?.closed)),
            Release => {
                self.release_stream_reader(id)?;
                Ok(Value::Undefined)
            }
            Read => self.read_stream(id, args),
            Cancel => {
                let stream = match self.heap.web_stream(id)? {
                    WebStreamState::Stream(s) => {
                        if s.reader.is_some() {
                            return self.stream_rejected("Cannot cancel a locked stream");
                        }
                        id
                    }
                    WebStreamState::Reader(r) => {
                        let Some(stream) = r.stream else {
                            return self.stream_rejected("Reader is released");
                        };
                        stream
                    }
                    _ => return self.stream_rejected("Invalid stream cancellation receiver"),
                };
                self.cancel_stream(stream, native::argument(args, 0).clone())
            }
            DesiredSize => {
                let stream = self.controller_stream(id)?;
                let state = self.stream_data(stream)?;
                Ok(if state.status == 2 {
                    Value::Null
                } else {
                    Value::Number(if state.status == 1 {
                        0.0
                    } else {
                        state.high_water_mark - state.queue_size
                    })
                })
            }
            Enqueue => {
                let stream = self.controller_stream(id)?;
                self.enqueue_stream(stream, native::argument(args, 0).clone())?;
                Ok(Value::Undefined)
            }
            Close => {
                let stream = self.controller_stream(id)?;
                let s = self.stream_data(stream)?;
                if s.status != 0 || s.close_requested {
                    return Err(RuntimeError::TypeError("Stream cannot close".into()));
                }
                self.close_stream(stream, false)?;
                Ok(Value::Undefined)
            }
            Error => {
                let stream = self.controller_stream(id)?;
                self.error_stream(stream, native::argument(args, 0).clone())?;
                Ok(Value::Undefined)
            }
            ByobRequest => self.stream_byob_request(id),
            View => match self.heap.web_stream(id)? {
                WebStreamState::Request { view, .. } => Ok(view.map_or(Value::Null, Value::Object)),
                _ => Err(RuntimeError::TypeError("Invalid BYOB request".into())),
            },
            Respond(new_view) => {
                self.respond_stream(id, native::argument(args, 0), new_view)?;
                Ok(Value::Undefined)
            }
            _ => unreachable!(),
        }
    }
    fn stream_option(&mut self, options: &Value, name: &str) -> Result<Value, RuntimeError> {
        if matches!(options, Value::Undefined | Value::Null) {
            Ok(Value::Undefined)
        } else {
            self.get_property(options, &name.into())
        }
    }
    fn empty_stream(bytes: bool, hwm: f64) -> StreamData {
        StreamData {
            status: 0,
            error: Value::Undefined,
            controller: None,
            reader: None,
            source: Value::Undefined,
            pull: Value::Undefined,
            cancel: Value::Undefined,
            size: Value::Undefined,
            started: false,
            pulling: false,
            pull_again: false,
            high_water_mark: hwm,
            close_requested: false,
            queue: VecDeque::new(),
            queue_size: 0.0,
            byte_stream: bytes,
            auto_allocate: None,
            blob: None,
            blob_offset: 0,
            text_stream: false,
            text_tail: Vec::new(),
        }
    }
    fn construct_stream(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            let source = args.first().cloned().unwrap_or(Value::Undefined);
            if source == Value::Null {
                return Err(RuntimeError::TypeError("Null underlying source".into()));
            }
            let kind = self.stream_option(&source, "type")?;
            let bytes = if kind == Value::Undefined {
                false
            } else if self.coerce_string(&kind)? == "bytes" {
                true
            } else {
                return Err(RuntimeError::RangeError("Invalid stream type".into()));
            };
            let mut callbacks = Vec::new();
            for name in ["start", "pull", "cancel"] {
                let value = self.stream_option(&source, name)?;
                if value != Value::Undefined && !self.is_callable(&value)? {
                    return Err(RuntimeError::TypeError(
                        "Underlying source callback must be callable".into(),
                    ));
                }
                self.stack.push(value.clone());
                callbacks.push(value);
            }
            let strategy = native::argument(args, 1);
            let size = self.stream_option(strategy, "size")?;
            self.stack.push(size.clone());
            if size != Value::Undefined && (bytes || !self.is_callable(&size)?) {
                return Err(RuntimeError::TypeError(
                    "Invalid stream size algorithm".into(),
                ));
            }
            let hwm = self.stream_option(strategy, "highWaterMark")?;
            let hwm = if hwm == Value::Undefined {
                if bytes {
                    0.0
                } else {
                    1.0
                }
            } else {
                self.coerce_number(&hwm)?
            };
            if hwm.is_nan() || hwm < 0.0 {
                return Err(RuntimeError::RangeError("Invalid high water mark".into()));
            }
            let mut state = Self::empty_stream(bytes, hwm);
            state.source = source.clone();
            state.pull = callbacks[1].clone();
            state.cancel = callbacks[2].clone();
            state.size = size;
            if bytes {
                let auto = self.stream_option(&source, "autoAllocateChunkSize")?;
                if auto != Value::Undefined {
                    let n = self.coerce_number(&auto)?;
                    if !n.is_finite()
                        || n < 1.0
                        || n.fract() != 0.0
                        || n > self.heap.max_array_buffer_byte_length() as f64
                    {
                        return Err(RuntimeError::TypeError(
                            "Invalid autoAllocateChunkSize".into(),
                        ));
                    }
                    state.auto_allocate = Some(n as usize);
                }
            }
            let stream = self.allocate_stream(state)?;
            self.stack.push(Value::Object(stream));
            let prototype =
                self.constructor_prototype_for(self.globals["%ReadableStream%"], None)?;
            self.heap.set_prototype(stream, Some(prototype))?;
            let controller = self.stream_data(stream)?.controller.unwrap();
            let start = if callbacks[0] == Value::Undefined {
                Value::Undefined
            } else {
                self.call_native(
                    callbacks[0].clone(),
                    source,
                    vec![Value::Object(controller)],
                    false,
                )?
            };
            self.watch_stream_promise(stream, 0, start)?;
            Ok(Value::Object(stream))
        })();
        self.stack.truncate(base);
        result
    }
    fn allocate_stream(&mut self, state: StreamData) -> Result<ObjectId, RuntimeError> {
        let prototype = self.globals["%ReadableStream%"];
        let byte_stream = state.byte_stream;
        let stream = self.with_roots(|heap| {
            heap.alloc_web_stream(WebStreamState::Stream(Box::new(state)), prototype)
        })?;
        let base = self.stack.len();
        self.stack.push(Value::Object(stream));
        let result = (|| {
            let prototype = self.globals[if byte_stream {
                "%ReadableByteStreamController%"
            } else {
                "%ReadableStreamDefaultController%"
            }];
            let controller = self.with_roots(|heap| {
                heap.alloc_web_stream(
                    WebStreamState::Controller {
                        stream,
                        byte_stream,
                        request: None,
                    },
                    prototype,
                )
            })?;
            let mut state = self.stream_data(stream)?;
            state.controller = Some(controller);
            self.put_stream(stream, state)?;
            Ok(stream)
        })();
        self.stack.truncate(base);
        result
    }
    fn acquire_stream_reader(
        &mut self,
        stream: ObjectId,
        byob: bool,
    ) -> Result<ObjectId, RuntimeError> {
        let mut state = self.stream_data(stream)?;
        if state.reader.is_some() || byob && !state.byte_stream {
            return Err(RuntimeError::TypeError(
                "Stream cannot acquire this reader".into(),
            ));
        }
        let base = self.stack.len();
        self.stack.push(Value::Object(stream));
        let result = (|| {
            let closed = self.new_promise()?;
            self.stack.push(Value::Object(closed));
            let prototype = self.globals[if byob {
                "%ReadableStreamBYOBReader%"
            } else {
                "%ReadableStreamDefaultReader%"
            }];
            let reader = self.with_roots(|heap| {
                heap.alloc_web_stream(
                    WebStreamState::Reader(StreamReader {
                        stream: Some(stream),
                        closed,
                        byob,
                        reads: VecDeque::new(),
                    }),
                    prototype,
                )
            })?;
            self.stack.push(Value::Object(reader));
            state.reader = Some(reader);
            let status = state.status;
            let error = state.error.clone();
            self.put_stream(stream, state)?;
            match status {
                1 => {
                    self.settle_tracked_promise(closed, PromiseStatus::Fulfilled(Value::Undefined))
                }
                2 => self.settle_tracked_promise(closed, PromiseStatus::Rejected(error)),
                _ => {}
            }
            Ok(reader)
        })();
        self.stack.truncate(base);
        result
    }
    fn watch_stream_promise(
        &mut self,
        stream: ObjectId,
        action: u8,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        self.stack.extend([Value::Object(stream), value.clone()]);
        let result = (|| {
            let promise = self.promise_resolve(value)?.object_id().unwrap();
            self.stack.push(Value::Object(promise));
            let function = self.function_prototype()?;
            let mut callbacks = Vec::new();
            for rejected in [false, true] {
                let callback = self.with_roots(|heap| {
                    heap.alloc_native_function(
                        NativeFunction::StreamReaction {
                            stream,
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
    pub(super) fn stream_reaction(
        &mut self,
        stream: ObjectId,
        action: u8,
        rejected: bool,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        if rejected {
            self.error_stream(stream, value)?;
        } else {
            let mut state = self.stream_data(stream)?;
            if action == 0 {
                state.started = true;
            } else {
                state.pulling = false;
            }
            let again = state.pull_again;
            state.pull_again = false;
            self.put_stream(stream, state)?;
            if action == 0 || again {
                self.stream_maybe_pull(stream)?;
            }
        }
        Ok(Value::Undefined)
    }
    fn stream_maybe_pull(&mut self, stream: ObjectId) -> Result<(), RuntimeError> {
        let mut state = self.stream_data(stream)?;
        let pending = state
            .reader
            .and_then(|r| self.stream_reader(r).ok())
            .is_some_and(|r| !r.reads.is_empty());
        if state.status != 0
            || state.close_requested
            || !state.started
            || (!pending && state.queue_size >= state.high_water_mark)
        {
            return Ok(());
        }
        if state.pulling {
            state.pull_again = true;
            return self.put_stream(stream, state);
        }
        if state.blob.is_some() {
            return self.pull_blob_stream(stream);
        }
        state.pulling = true;
        let pull = state.pull.clone();
        let source = state.source.clone();
        let controller = state.controller.unwrap();
        self.put_stream(stream, state)?;
        let value = if pull == Value::Undefined {
            Ok(Value::Undefined)
        } else {
            self.call_native(pull, source, vec![Value::Object(controller)], false)
        };
        match value {
            Ok(value) => self.watch_stream_promise(stream, 1, value),
            Err(error) => {
                let error = self.error_value(error)?;
                self.error_stream(stream, error)
            }
        }
    }
    fn read_stream(&mut self, reader: ObjectId, args: &[Value]) -> Result<Value, RuntimeError> {
        let mut owner = self.stream_reader(reader)?;
        let Some(stream) = owner.stream else {
            return self.stream_rejected("Reader is released");
        };
        let base = self.stack.len();
        let result = (|| {
            let state = self.stream_data(stream)?;
            let into = if owner.byob {
                Some(
                    self.prepare_stream_byob(native::argument(args, 0), native::argument(args, 1))?,
                )
            } else if state.byte_stream {
                if let Some(length) = state.auto_allocate {
                    Some(self.allocate_pull_into(length)?)
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(into) = &into {
                self.stack.push(Value::Object(into.buffer));
            }
            let promise = self.new_promise()?;
            self.stack.push(Value::Object(promise));
            owner.reads.push_back(StreamRead { promise, into });
            self.put_reader(reader, owner)?;
            self.process_stream_reads(stream)?;
            self.stream_maybe_pull(stream)?;
            Ok(Value::Object(promise))
        })();
        self.stack.truncate(base);
        match result {
            Err(error) if super::web_drag_drop::drag_page_exception(&error) => {
                let error = self.error_value(error)?;
                self.promise_reject(error)
            }
            result => result,
        }
    }
    fn settle_stream_read(
        &mut self,
        read: StreamRead,
        value: Value,
        done: bool,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        self.stack
            .extend([Value::Object(read.promise), value.clone()]);
        let result = (|| {
            let prototype = self.object_prototype;
            let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(result));
            self.define_data(result, "value", value, true, true, true)?;
            self.define_data(result, "done", Value::Bool(done), true, true, true)?;
            self.settle_tracked_promise(
                read.promise,
                PromiseStatus::Fulfilled(Value::Object(result)),
            );
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }
    fn release_stream_reader(&mut self, reader: ObjectId) -> Result<(), RuntimeError> {
        let mut owner = self.stream_reader(reader)?;
        let Some(stream) = owner.stream else {
            return Ok(());
        };
        let base = self.stack.len();
        self.stack.push(Value::Object(reader));
        let result = (|| {
            let error = self.stream_type_error("Reader lock is released")?;
            self.stack.push(error.clone());
            let mut state = self.stream_data(stream)?;
            state.reader = None;
            self.put_stream(stream, state)?;
            self.invalidate_stream_request(stream)?;
            for read in owner.reads.drain(..) {
                self.settle_tracked_promise(read.promise, PromiseStatus::Rejected(error.clone()));
            }
            if self
                .promises
                .get(&owner.closed)
                .is_some_and(|p| matches!(p.status, PromiseStatus::Pending))
            {
                self.settle_tracked_promise(owner.closed, PromiseStatus::Rejected(error));
            } else {
                owner.closed = self.promise_reject(error)?.object_id().unwrap();
            }
            owner.stream = None;
            self.put_reader(reader, owner)
        })();
        self.stack.truncate(base);
        result
    }
    fn enqueue_stream(&mut self, stream: ObjectId, value: Value) -> Result<(), RuntimeError> {
        let state = self.stream_data(stream)?;
        if state.status != 0 || state.close_requested {
            return Err(RuntimeError::TypeError(
                "Cannot enqueue after stream close".into(),
            ));
        }
        if state.byte_stream {
            return self.enqueue_stream_bytes(stream, value);
        }
        let base = self.stack.len();
        self.stack.extend([Value::Object(stream), value.clone()]);
        let result = (|| {
            let pending = state
                .reader
                .and_then(|r| self.stream_reader(r).ok())
                .is_some_and(|r| !r.reads.is_empty());
            let size = if pending || state.size == Value::Undefined {
                1.0
            } else {
                let size =
                    self.call_native(state.size, Value::Undefined, vec![value.clone()], false)?;
                self.coerce_number(&size)?
            };
            if !size.is_finite() || size < 0.0 {
                return Err(RuntimeError::RangeError("Invalid queued chunk size".into()));
            }
            let mut state = self.stream_data(stream)?;
            if state.queue.len() >= 4096 {
                return Err(RuntimeError::RangeError(
                    "Stream queue limit exceeded".into(),
                ));
            }
            state.queue.push_back((value, size));
            state.queue_size += size;
            self.put_stream(stream, state)?;
            self.process_stream_reads(stream)?;
            self.stream_maybe_pull(stream)
        })();
        self.stack.truncate(base);
        if let Err(error) = &result {
            if super::web_drag_drop::drag_page_exception(error) {
                let reason = self.error_value(error.clone())?;
                self.error_stream(stream, reason)?;
            }
        }
        result
    }
    fn error_stream(&mut self, stream: ObjectId, reason: Value) -> Result<(), RuntimeError> {
        let mut state = self.stream_data(stream)?;
        if state.status != 0 {
            return Ok(());
        }
        let base = self.stack.len();
        self.stack.extend([Value::Object(stream), reason.clone()]);
        let result = (|| {
            state.status = 2;
            state.error = reason.clone();
            state.queue.clear();
            state.queue_size = 0.0;
            state.blob = None;
            state.pull = Value::Undefined;
            state.cancel = Value::Undefined;
            state.size = Value::Undefined;
            state.source = Value::Undefined;
            let reader = state.reader;
            self.put_stream(stream, state)?;
            self.invalidate_stream_request(stream)?;
            if let Some(reader) = reader {
                let mut owner = self.stream_reader(reader)?;
                for read in owner.reads.drain(..) {
                    self.settle_tracked_promise(
                        read.promise,
                        PromiseStatus::Rejected(reason.clone()),
                    );
                }
                self.settle_tracked_promise(owner.closed, PromiseStatus::Rejected(reason));
                self.put_reader(reader, owner)?;
            }
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }
    fn close_stream(&mut self, stream: ObjectId, force_byob: bool) -> Result<(), RuntimeError> {
        let mut state = self.stream_data(stream)?;
        if state.status != 0 {
            return Ok(());
        }
        state.close_requested = true;
        let empty = state.queue.is_empty();
        self.put_stream(stream, state)?;
        if !empty {
            return Ok(());
        }
        if let Some(reader) = self.stream_data(stream)?.reader {
            let owner = self.stream_reader(reader)?;
            if owner
                .reads
                .front()
                .and_then(|r| r.into.as_ref())
                .is_some_and(|into| into.filled % into.kind.map_or(1, |k| k.byte_width()) != 0)
            {
                let reason = self.stream_type_error("Byte stream closed with a partial element")?;
                self.error_stream(stream, reason.clone())?;
                return Err(RuntimeError::Thrown(reason));
            }
        }
        let mut state = self.stream_data(stream)?;
        state.status = 1;
        state.blob = None;
        state.pull = Value::Undefined;
        state.cancel = Value::Undefined;
        state.size = Value::Undefined;
        state.source = Value::Undefined;
        let reader = state.reader;
        self.put_stream(stream, state)?;
        if let Some(reader) = reader {
            let mut owner = self.stream_reader(reader)?;
            self.settle_tracked_promise(owner.closed, PromiseStatus::Fulfilled(Value::Undefined));
            while let Some(read) = owner.reads.front() {
                if owner.byob
                    && !force_byob
                    && read.into.as_ref().is_some_and(|into| into.filled == 0)
                {
                    break;
                }
                let read = owner.reads.pop_front().unwrap();
                let into = read.into.clone();
                self.put_reader(reader, owner.clone())?;
                let done = into.as_ref().is_none_or(|into| into.filled == 0);
                let value = if owner.byob {
                    self.stream_into_value(into.as_ref().unwrap())?
                } else if let Some(into) = &into {
                    if into.filled == 0 {
                        Value::Undefined
                    } else {
                        self.stream_into_value(into)?
                    }
                } else {
                    Value::Undefined
                };
                self.settle_stream_read(read, value, done)?;
            }
            self.put_reader(reader, owner)?;
        }
        if self
            .stream_data(stream)?
            .reader
            .and_then(|r| self.stream_reader(r).ok())
            .is_none_or(|r| r.reads.is_empty())
        {
            self.invalidate_stream_request(stream)?;
        }
        Ok(())
    }
    fn cancel_stream(&mut self, stream: ObjectId, reason: Value) -> Result<Value, RuntimeError> {
        let state = self.stream_data(stream)?;
        if state.status == 1 {
            return self.promise_resolve(Value::Undefined);
        }
        if state.status == 2 {
            return self.promise_reject(state.error);
        }
        let base = self.stack.len();
        self.stack.extend([
            Value::Object(stream),
            state.cancel.clone(),
            state.source.clone(),
            reason.clone(),
        ]);
        let result = (|| {
            let mut cleared = self.stream_data(stream)?;
            cleared.queue.clear();
            cleared.queue_size = 0.0;
            if let Some(reader) = cleared.reader {
                let mut owner = self.stream_reader(reader)?;
                while let Some(read) = owner.reads.pop_front() {
                    self.stack.push(Value::Object(read.promise));
                    self.put_reader(reader, owner.clone())?;
                    self.settle_stream_read(read, Value::Undefined, true)?;
                }
                self.put_reader(reader, owner)?;
            }
            self.put_stream(stream, cleared)?;
            self.close_stream(stream, true)?;
            let value = if state.cancel == Value::Undefined {
                Value::Undefined
            } else {
                match self.call_native(state.cancel, state.source, vec![reason], false) {
                    Ok(value) => value,
                    Err(error) => {
                        let reason = self.error_value(error)?;
                        return self.promise_reject(reason);
                    }
                }
            };
            let value = self.promise_resolve(value)?;
            self.stack.push(value.clone());
            let function = self.function_prototype()?;
            let noop = self.with_roots(|heap| {
                heap.alloc_native_function(NativeFunction::Empty, "", function)
            })?;
            self.stack.push(Value::Object(noop));
            self.promise_then(&value, &[Value::Object(noop), Value::Undefined])
        })();
        self.stack.truncate(base);
        result
    }
}
