// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Asynchronous immutable-Blob reads, with collector-owned state and events.

use super::error_input_storage::utf16_from_utf8;
use super::native_value_storage::NativeValue;
use super::realm_call_storage::RealmCallRoots;
use super::transport_buffer_storage::TransportVector;
use super::web_event_storage::{EventContext, NativeEventState, NativeEventTarget};
use super::*;
use crate::heap::{FileReadKind, WebListener};
use crate::native::ReaderNative;
use browser_tasks::BrowserTask;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/file_reader_storage.rs"]
mod storage_tests;

mod packaging;
mod storage;
use storage::NativeReaderState;

const HANDLERS: [&str; 6] = ["loadstart", "progress", "abort", "error", "load", "loadend"];

impl Vm {
    pub(super) fn install_web_file_reader(&mut self) -> Result<(), RuntimeError> {
        if self.globals.contains_key("%WebFileReader%") {
            return Ok(());
        }
        let global = self.global("globalThis")?.object_id().unwrap();
        if self.heap.get_own(global, "FileReader")?.is_some() {
            return Err(RuntimeError::TypeError(
                "FileReader global already defined".into(),
            ));
        }
        let base = self.stack.len();
        let result = (|| {
            let function = self.function_prototype()?;
            let parent = self.globals["%WebEventTarget%"];
            let prototype = self.with_roots(|heap| heap.alloc_object(Some(parent)))?;
            self.stack.push(Value::Object(prototype));
            for (name, native) in [
                ("readyState", ReaderNative::Ready),
                ("result", ReaderNative::Result),
                ("error", ReaderNative::Error),
            ] {
                self.install_native_getter(
                    prototype,
                    function,
                    name,
                    NativeFunction::WebReader(native),
                )?;
                self.reader_enumerable(prototype, name)?;
            }
            for (name, kind) in [
                ("readAsText", FileReadKind::Text),
                ("readAsArrayBuffer", FileReadKind::ArrayBuffer),
                ("readAsBinaryString", FileReadKind::BinaryString),
                ("readAsDataURL", FileReadKind::DataUrl),
            ] {
                self.install_native(
                    prototype,
                    function,
                    name,
                    1,
                    NativeFunction::WebReader(ReaderNative::Read(kind)),
                )?;
                self.reader_enumerable(prototype, name)?;
            }
            self.install_native(
                prototype,
                function,
                "abort",
                0,
                NativeFunction::WebReader(ReaderNative::Abort),
            )?;
            self.reader_enumerable(prototype, "abort")?;
            for name in HANDLERS {
                let property = format!("on{name}");
                self.install_native_getter(
                    prototype,
                    function,
                    &property,
                    NativeFunction::WebReader(ReaderNative::Handler(name, false)),
                )?;
                let setter = self.with_roots(|heap| {
                    heap.alloc_native_function(
                        NativeFunction::WebReader(ReaderNative::Handler(name, true)),
                        &format!("set {property}"),
                        function,
                    )
                })?;
                self.stack.push(Value::Object(setter));
                self.define_data(setter, "length", Value::Number(1.0), false, false, true)?;
                self.with_roots(|heap| {
                    heap.define_own_property(
                        prototype,
                        property.as_str(),
                        PropertyDescriptor {
                            set: Some(Value::Object(setter)),
                            enumerable: Some(true),
                            ..Default::default()
                        },
                    )
                })?;
                self.stack.pop();
            }
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("FileReader".into()),
                false,
                false,
                true,
            )?;
            // Host constructors live on globalThis, outside the lazy
            // ECMAScript-global registry. Calling global("EventTarget")
            // would manufacture and publish an unrelated empty function.
            let parent_constructor = self
                .heap
                .get_own(parent, "constructor")?
                .and_then(|value| value.object_id())
                .expect("installed EventTarget constructor");
            let constructor = self.web_file_constructor(
                "FileReader",
                0,
                NativeFunction::WebReader(ReaderNative::Constructor),
                parent_constructor,
                prototype,
            )?;
            self.stack.push(Value::Object(constructor));
            for target in [constructor, prototype] {
                for (name, number) in [("EMPTY", 0.0), ("LOADING", 1.0), ("DONE", 2.0)] {
                    self.define_data(target, name, Value::Number(number), false, true, false)?;
                }
            }
            self.define_data(
                global,
                "FileReader",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.heap.root(prototype)?;
            self.globals.insert("%WebFileReader%".into(), prototype);
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    fn reader_enumerable(&mut self, prototype: ObjectId, name: &str) -> Result<(), RuntimeError> {
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
        Ok(())
    }

    pub(super) fn web_reader_call(
        &mut self,
        native: ReaderNative,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if native == ReaderNative::Constructor && !construct {
            return Err(RuntimeError::TypeError(
                "FileReader constructor requires new".into(),
            ));
        }
        let context = EventContext::capture(self)?;
        let ids = receiver
            .object_id()
            .into_iter()
            .chain(args.iter().filter_map(Value::object_id))
            .chain(
                self.new_target
                    .object_id()
                    .filter(|_| native == ReaderNative::Constructor),
            );
        let roots = context.prepare_roots(self, ids, 2)?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            vm.web_reader_owned(native, receiver, args, &context, &roots)
        });
        context.validate().and(result)
    }

    fn web_reader_owned(
        &mut self,
        native: ReaderNative,
        receiver: &Value,
        args: &[Value],
        context: &EventContext,
        roots: &RealmCallRoots,
    ) -> Result<Value, RuntimeError> {
        if native == ReaderNative::Constructor {
            let default = self.globals["%WebFileReader%"];
            let prototype = self.constructor_prototype_for(default, None);
            let prototype = context.validate().and(prototype)?;
            roots.push(&self.heap, prototype)?;
            let state = NativeReaderState::new(context.owner.as_ref())?;
            return Ok(Value::Object(state.allocate(self, prototype)?));
        }
        let reader = receiver
            .object_id()
            .filter(|id| self.heap.file_reader(*id).is_ok())
            .ok_or_else(|| RuntimeError::TypeError("Invalid FileReader receiver".into()))?;
        let state = self.heap.file_reader(reader)?;
        match native {
            ReaderNative::Ready => return Ok(Value::Number(state.ready as f64)),
            ReaderNative::Result => {
                return Ok(NativeValue::copied(&state.result, context.owner.as_ref())?.into_value())
            }
            ReaderNative::Error => return Ok(state.error.map_or(Value::Null, Value::Object)),
            ReaderNative::Handler(name, set) => {
                return self.reader_handler(reader, name, set, args, context)
            }
            ReaderNative::Abort => return self.abort_file_reader(reader, context, roots),
            ReaderNative::Read(kind) => {
                let blob = native::argument(args, 0)
                    .object_id()
                    .filter(|id| self.heap.web_blob(*id).is_ok())
                    .ok_or_else(|| {
                        RuntimeError::TypeError("FileReader read requires a Blob".into())
                    })?;
                let encoding = if kind == FileReadKind::Text
                    && args.get(1).is_some_and(|value| *value != Value::Undefined)
                {
                    Some(self.reader_string(&args[1], context)?)
                } else {
                    None
                };
                if self.heap.file_reader(reader)?.ready == 1 {
                    return self
                        .drag_named_error("InvalidStateError", "FileReader is already loading");
                }
                if self.browser_tasks.len() >= 64 {
                    return self.drag_named_error(
                        "SecurityError",
                        "File reading task queue limit exceeded",
                    );
                }
                let epoch = self
                    .heap
                    .file_reader(reader)?
                    .epoch
                    .checked_add(1)
                    .ok_or_else(|| {
                        RuntimeError::RangeError("FileReader identity exhausted".into())
                    })?;
                let prepared = self.prepare_browser_task(HostSchedulingUsage::default())?;
                self.heap.check_root_registration_count(1)?;
                let mut state = NativeReaderState::copied(self, reader, context.owner.as_ref())?;
                state.result(
                    self,
                    roots,
                    NativeValue::adopt(Value::Null, context.owner.as_ref())?,
                )?;
                state.encoding(encoding)?;
                state.state.epoch = epoch;
                state.state.ready = 1;
                state.state.error = None;
                state.state.blob = Some(blob);
                state.state.kind = kind;
                state.state.loaded = 0;
                state.state.total = self.heap.web_blob(blob)?.bytes.len();
                let state = state.prepare(self, reader)?;
                let objects = [reader];
                let roots = self.heap.prepare_root_batch(&objects)?;
                context.validate()?;
                self.check_scheduling_failure()?;
                let serial = self.next_host_task_serial()?;
                let root = self.heap.commit_single_root(roots);
                state.commit(self);
                prepared.commit(
                    self,
                    BrowserTask::FileRead {
                        serial,
                        reader,
                        epoch,
                        stage: 0,
                        root,
                    },
                );
                self.synchronize_scheduling_usage()?;
            }
            _ => unreachable!("constructor handled above"),
        }
        Ok(Value::Undefined)
    }

    fn reader_string(
        &mut self,
        value: &Value,
        context: &EventContext,
    ) -> Result<NativeValue, RuntimeError> {
        if matches!(value, Value::String(_)) {
            return NativeValue::copied(value, context.owner.as_ref());
        }
        let text = self.coerce_string(value);
        let text = context.validate().and(text)?;
        NativeValue::adopt(Value::String(text), context.owner.as_ref())
    }

    fn reader_handler(
        &mut self,
        reader: ObjectId,
        name: &'static str,
        set: bool,
        args: &[Value],
        context: &EventContext,
    ) -> Result<Value, RuntimeError> {
        let existing = self
            .heap
            .web_event_target(reader)?
            .listeners
            .iter()
            .position(|listener| listener.handler && listener.name == name);
        if !set {
            return Ok(existing.map_or(Value::Null, |index| {
                Value::Object(
                    self.heap
                        .web_event_target(reader)
                        .expect("validated reader")
                        .listeners[index]
                        .callback,
                )
            }));
        }
        // EventHandler's LegacyTreatNonObjectAsNull retains any object; a
        // noncallable object remains inert rather than becoming a listener.
        let callback = native::argument(args, 0).object_id();
        let mut target = NativeEventTarget::copied(self, reader, context.owner.as_ref())?;
        match (existing, callback) {
            (Some(index), Some(callback)) => target.state.listeners[index].callback = callback,
            (Some(index), None) => {
                target.state.listeners.remove(index);
            }
            (None, Some(callback)) => {
                if target.state.listeners.len() >= 4096 {
                    return Err(RuntimeError::RangeError(
                        "Event listener limit exceeded".into(),
                    ));
                }
                target.state.next_serial =
                    target.state.next_serial.checked_add(1).ok_or_else(|| {
                        RuntimeError::RangeError("Event listener identity exhausted".into())
                    })?;
                let (Value::String(name), storage) =
                    utf16_from_utf8(name, context.owner.as_ref())?.into_retained_parts()
                else {
                    unreachable!()
                };
                target.listener(
                    WebListener {
                        signal: None,
                        serial: target.state.next_serial,
                        name,
                        callback,
                        capture: false,
                        once: false,
                        passive: false,
                        handler: true,
                    },
                    storage,
                )?;
            }
            _ => {}
        }
        context.validate()?;
        target.into_heap(self, reader)?;
        Ok(Value::Undefined)
    }

    fn abort_file_reader(
        &mut self,
        reader: ObjectId,
        context: &EventContext,
        roots: &RealmCallRoots,
    ) -> Result<Value, RuntimeError> {
        let mut state = NativeReaderState::copied(self, reader, context.owner.as_ref())?;
        state.result(
            self,
            roots,
            NativeValue::adopt(Value::Null, context.owner.as_ref())?,
        )?;
        if state.state.ready != 1 {
            state.into_heap(self, reader)?;
            return Ok(Value::Undefined);
        }
        let (loaded, total) = (state.state.loaded, state.state.total);
        state.state.ready = 2;
        state.state.blob = None;
        state.encoding(None)?;
        state.state.epoch = state
            .state
            .epoch
            .checked_add(1)
            .ok_or_else(|| RuntimeError::RangeError("FileReader identity exhausted".into()))?;
        state.into_heap(self, reader)?;
        self.cancel_file_read_tasks(reader)?;
        self.fire_reader_progress(reader, "abort", loaded, total)?;
        context.validate()?;
        if self.heap.file_reader(reader)?.ready != 1 {
            self.fire_reader_progress(reader, "loadend", loaded, total)?;
        }
        Ok(Value::Undefined)
    }

    fn fire_reader_progress(
        &mut self,
        reader: ObjectId,
        name: &'static str,
        loaded: usize,
        total: usize,
    ) -> Result<(), RuntimeError> {
        let context = EventContext::capture(self)?;
        let roots = context.prepare_roots(self, std::iter::once(reader), 1)?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            let name = utf16_from_utf8(name, context.owner.as_ref())?;
            let mut state = NativeEventState::new(vm, 4, name)?;
            state.scalar(vm, &roots, "lengthComputable", Value::Bool(true))?;
            state.scalar(vm, &roots, "loaded", Value::Number(loaded as f64))?;
            state.scalar(vm, &roots, "total", Value::Number(total as f64))?;
            let prototype = vm.globals["%WebProgressEvent%"];
            let event = state.allocate(vm, prototype)?;
            roots.push(&vm.heap, event)?;
            vm.finish_web_event(event)?;
            let result = vm.dispatch_web_target_event(reader, event, true);
            context.validate().and(result).map(|_| ())
        });
        context.validate().and(result)
    }

    pub(super) fn file_read_task(
        &mut self,
        reader: ObjectId,
        epoch: u64,
        stage: u8,
    ) -> Result<Option<u8>, RuntimeError> {
        let context = EventContext::capture(self)?;
        let roots = context.prepare_roots(self, std::iter::once(reader), 2)?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            vm.file_read_task_owned(reader, epoch, stage, &context, &roots)
        });
        context.validate().and(result)
    }

    fn file_read_task_owned(
        &mut self,
        reader: ObjectId,
        epoch: u64,
        stage: u8,
        context: &EventContext,
        roots: &RealmCallRoots,
    ) -> Result<Option<u8>, RuntimeError> {
        let state = self.heap.file_reader(reader)?;
        if state.epoch != epoch || state.ready != 1 {
            return Ok(None);
        }
        let total = state.total;
        match stage {
            0 => self.fire_reader_progress(reader, "loadstart", 0, total)?,
            1 => {
                let mut state = NativeReaderState::copied(self, reader, context.owner.as_ref())?;
                state.state.loaded = total;
                state.into_heap(self, reader)?;
                self.fire_reader_progress(reader, "progress", total, total)?;
            }
            2 => {
                let blob = state.blob.expect("an active read retains its Blob");
                let kind = state.kind;
                let encoding = state
                    .encoding
                    .as_ref()
                    .map(|text| {
                        crate::vm::web_drag_drop::storage::copy_string(text, context.owner.as_ref())
                    })
                    .transpose()?;
                let label = encoding.as_ref().map(|value| match value.value() {
                    Value::String(text) => text,
                    _ => unreachable!(),
                });
                let result = self.package_file_read(blob, kind, label);
                context.validate()?;
                drop(encoding);
                let current = self.heap.file_reader(reader)?;
                if current.epoch != epoch || current.ready != 1 {
                    // Packaging may abort or start a replacement read. Its
                    // independent queued owner must keep that newer state.
                    return Ok(None);
                }
                let (event, value, error) = match result {
                    Ok(value) => ("load", value, None),
                    Err(error) => {
                        if !error.is_catchable() {
                            return Err(error);
                        }
                        drop(error);
                        drop(self.take_pending_throw_origin());
                        let error = match self
                            .drag_named_error("NotReadableError", "File data could not be packaged")
                        {
                            Err(RuntimeError::Thrown(Value::Object(error))) => error,
                            Err(error) => return Err(error),
                            Ok(_) => unreachable!("named DOMException throws its object"),
                        };
                        context.validate()?;
                        roots.push(&self.heap, error)?;
                        (
                            "error",
                            NativeValue::adopt(Value::Null, context.owner.as_ref())?,
                            Some(error),
                        )
                    }
                };
                let mut state = NativeReaderState::copied(self, reader, context.owner.as_ref())?;
                state.result(self, roots, value)?;
                state.state.ready = 2;
                state.state.blob = None;
                state.state.error = error;
                state.encoding(None)?;
                state.into_heap(self, reader)?;
                self.fire_reader_progress(reader, event, total, total)?;
                context.validate()?;
                if self.heap.file_reader(reader)?.ready != 1 {
                    self.fire_reader_progress(reader, "loadend", total, total)?;
                }
                return Ok(None);
            }
            _ => unreachable!("bounded file read stage"),
        }
        context.validate()?;
        let state = self.heap.file_reader(reader)?;
        Ok((state.epoch == epoch && state.ready == 1).then_some(stage + 1))
    }

    fn package_file_read(
        &mut self,
        blob: ObjectId,
        kind: FileReadKind,
        label: Option<&JsString>,
    ) -> Result<NativeValue, RuntimeError> {
        let context = EventContext::capture(self)?;
        let roots = context.prepare_roots(self, std::iter::once(blob), 1)?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            packaging::package(vm, blob, kind, label, &context, &roots)
        });
        context.validate().and(result)
    }
}

// Behavior reference: https://mimesniff.spec.whatwg.org/#parse-a-mime-type
// MIME Sniffing's forgiving parameter parser: quoted semicolons are data,
// invalid essences fail parsing, and the first valid charset parameter wins
// even when its value is not a recognized Encoding Standard label.
fn reader_mime_encoding(
    input: &str,
    owner: Option<&host_scheduling::SharedSchedulingCharge>,
) -> Result<Option<&'static encoding_rs::Encoding>, RuntimeError> {
    if let Some(owner) = owner {
        owner.reserve_peer(0)?;
    }
    fn whitespace(byte: u8) -> bool {
        matches!(byte, b'\t' | b'\n' | b'\r' | b' ')
    }
    fn token(input: &[u8]) -> bool {
        !input.is_empty()
            && input
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(byte))
    }
    let input = input.trim_matches(['\t', '\n', '\r', ' ']);
    let Some((essence, parameters)) = input.split_once(';') else {
        return Ok(None);
    };
    let Some((main, subtype)) = essence.split_once('/') else {
        return Ok(None);
    };
    let subtype = subtype.trim_end_matches(['\t', '\n', '\r', ' ']);
    if !token(main.as_bytes()) || !token(subtype.as_bytes()) {
        return Ok(None);
    }
    let bytes = parameters.as_bytes();
    let mut position = 0;
    while position < bytes.len() {
        while position < bytes.len() && (whitespace(bytes[position]) || bytes[position] == b';') {
            position += 1;
        }
        let start = position;
        while position < bytes.len() && !matches!(bytes[position], b'=' | b';') {
            position += 1;
        }
        let name = &bytes[start..position];
        if position == bytes.len() {
            break;
        }
        if bytes[position] == b';' {
            continue;
        }
        position += 1;
        let mut value = TransportVector::with_capacity(0, owner)?;
        if bytes.get(position) == Some(&b'"') {
            position += 1;
            while position < bytes.len() {
                let byte = bytes[position];
                position += 1;
                if byte == b'"' {
                    break;
                }
                if byte == b'\\' && position < bytes.len() {
                    value.push(bytes[position])?;
                    position += 1;
                } else {
                    value.push(byte)?;
                }
            }
            while position < bytes.len() && bytes[position] != b';' {
                position += 1;
            }
        } else {
            let start = position;
            while position < bytes.len() && bytes[position] != b';' {
                position += 1;
            }
            for byte in &bytes[start..position] {
                value.push(*byte)?;
            }
            while value.last().is_some_and(|byte| whitespace(*byte)) {
                value.truncate(value.len() - 1);
            }
            if value.is_empty() {
                continue;
            }
        }
        if token(name)
            && name.eq_ignore_ascii_case(b"charset")
            && value
                .iter()
                .all(|byte| *byte == b'\t' || (32..=126).contains(byte))
        {
            return Ok(encoding_rs::Encoding::for_label(&value));
        }
    }
    Ok(None)
}
