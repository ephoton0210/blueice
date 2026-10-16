// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Asynchronous immutable-Blob reads, with collector-owned state and events.

use super::*;
use crate::heap::{FileReadKind, WebListener};
use crate::native::ReaderNative;
use base64::Engine;
use browser_tasks::BrowserTask;

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
        if native == ReaderNative::Constructor {
            if !construct {
                return Err(RuntimeError::TypeError(
                    "FileReader constructor requires new".into(),
                ));
            }
            let default = self.globals["%WebFileReader%"];
            let prototype = self.constructor_prototype_for(default, None)?;
            return Ok(Value::Object(
                self.with_roots(|heap| heap.alloc_file_reader(prototype))?,
            ));
        }
        let reader = receiver
            .object_id()
            .filter(|id| self.heap.file_reader(*id).is_ok())
            .ok_or_else(|| RuntimeError::TypeError("Invalid FileReader receiver".into()))?;
        let state = self.heap.file_reader(reader)?;
        match native {
            ReaderNative::Ready => return Ok(Value::Number(state.ready as f64)),
            ReaderNative::Result => return Ok(state.result.clone()),
            ReaderNative::Error => return Ok(state.error.map_or(Value::Null, Value::Object)),
            ReaderNative::Handler(name, set) => {
                return self.reader_handler(reader, name, set, args)
            }
            ReaderNative::Abort => return self.abort_file_reader(reader),
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
                    Some(self.coerce_string(&args[1])?)
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
                let mut state = self.heap.file_reader(reader)?.clone();
                state.epoch = state.epoch.checked_add(1).ok_or_else(|| {
                    RuntimeError::RangeError("FileReader identity exhausted".into())
                })?;
                state.ready = 1;
                state.result = Value::Null;
                state.error = None;
                state.blob = Some(blob);
                state.kind = kind;
                state.encoding = encoding;
                state.loaded = 0;
                state.total = self.heap.web_blob(blob)?.bytes.len();
                let epoch = state.epoch;
                self.reserve_browser_task(HostSchedulingUsage::default())?;
                let serial = self.next_host_task_serial()?;
                let root = self.heap.root(reader)?;
                if let Err(error) = self.with_roots(|heap| heap.set_file_reader(reader, state)) {
                    self.heap.unroot(root)?;
                    return Err(error);
                }
                self.browser_tasks.push_back(BrowserTask::FileRead {
                    serial,
                    reader,
                    epoch,
                    stage: 0,
                    root,
                });
                self.synchronize_scheduling_usage()?;
            }
            _ => unreachable!("constructor handled above"),
        }
        Ok(Value::Undefined)
    }

    fn reader_handler(
        &mut self,
        reader: ObjectId,
        name: &'static str,
        set: bool,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let mut target = self.heap.web_event_target(reader)?.clone();
        let existing = target
            .listeners
            .iter()
            .position(|listener| listener.handler && listener.name == name);
        if !set {
            return Ok(existing.map_or(Value::Null, |index| {
                Value::Object(target.listeners[index].callback)
            }));
        }
        let value = native::argument(args, 0);
        // EventHandler's LegacyTreatNonObjectAsNull conversion retains any
        // object. Noncallable objects are inert handlers, not EventListeners.
        let callback = value.object_id();
        match (existing, callback) {
            (Some(index), Some(callback)) => target.listeners[index].callback = callback,
            (Some(index), None) => {
                target.listeners.remove(index);
            }
            (None, Some(callback)) => {
                if target.listeners.len() >= 4096 {
                    return Err(RuntimeError::RangeError(
                        "Event listener limit exceeded".into(),
                    ));
                }
                target.next_serial = target.next_serial.checked_add(1).ok_or_else(|| {
                    RuntimeError::RangeError("Event listener identity exhausted".into())
                })?;
                target.listeners.push(WebListener {
                    signal: None,
                    serial: target.next_serial,
                    name: name.into(),
                    callback,
                    capture: false,
                    once: false,
                    passive: false,
                    handler: true,
                });
            }
            _ => {}
        }
        self.with_roots(|heap| heap.set_web_event_target(reader, target))?;
        Ok(Value::Undefined)
    }

    fn abort_file_reader(&mut self, reader: ObjectId) -> Result<Value, RuntimeError> {
        let mut state = self.heap.file_reader(reader)?.clone();
        state.result = Value::Null;
        if state.ready != 1 {
            self.with_roots(|heap| heap.set_file_reader(reader, state))?;
            return Ok(Value::Undefined);
        }
        let (loaded, total) = (state.loaded, state.total);
        state.ready = 2;
        state.blob = None;
        state.encoding = None;
        state.epoch = state
            .epoch
            .checked_add(1)
            .ok_or_else(|| RuntimeError::RangeError("FileReader identity exhausted".into()))?;
        self.with_roots(|heap| heap.set_file_reader(reader, state))?;
        self.cancel_file_read_tasks(reader)?;
        self.fire_reader_progress(reader, "abort", loaded, total)?;
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
        let base = self.stack.len();
        self.stack.push(Value::Object(reader));
        let result = (|| {
            let mut state = self.new_web_event_state(4, name.into());
            state.set("lengthComputable", Value::Bool(true));
            state.set("loaded", Value::Number(loaded as f64));
            state.set("total", Value::Number(total as f64));
            let prototype = self.globals["%WebProgressEvent%"];
            let event = self.with_roots(|heap| heap.alloc_drag_event(state, prototype))?;
            self.stack.push(Value::Object(event));
            self.finish_web_event(event)?;
            self.dispatch_web_target_event(reader, event, true)?;
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn file_read_task(
        &mut self,
        reader: ObjectId,
        epoch: u64,
        stage: u8,
    ) -> Result<Option<u8>, RuntimeError> {
        let state = self.heap.file_reader(reader)?;
        if state.epoch != epoch || state.ready != 1 {
            return Ok(None);
        }
        let total = state.total;
        match stage {
            0 => self.fire_reader_progress(reader, "loadstart", 0, total)?,
            1 => {
                let mut state = state.clone();
                state.loaded = total;
                self.with_roots(|heap| heap.set_file_reader(reader, state))?;
                self.fire_reader_progress(reader, "progress", total, total)?;
            }
            2 => {
                let blob = state.blob.expect("an active read retains its Blob");
                let kind = state.kind;
                let encoding = state.encoding.clone();
                let base = self.stack.len();
                let result = self.package_file_read(blob, kind, encoding.as_ref());
                let mut state = self.heap.file_reader(reader)?.clone();
                state.ready = 2;
                state.blob = None;
                state.encoding = None;
                let event = match result {
                    Ok(value) => {
                        self.stack.push(value.clone());
                        state.result = value;
                        "load"
                    }
                    Err(error) => {
                        if matches!(error, RuntimeError::InstructionLimit) {
                            return Err(error);
                        }
                        let prototype = self.globals["%WebDOMException%"];
                        let error = self.with_roots(|heap| {
                            heap.alloc_dom_exception(
                                "NotReadableError".into(),
                                "File data could not be packaged".into(),
                                prototype,
                            )
                        })?;
                        self.stack.push(Value::Object(error));
                        state.error = Some(error);
                        "error"
                    }
                };
                let result = (|| {
                    self.with_roots(|heap| heap.set_file_reader(reader, state))?;
                    self.fire_reader_progress(reader, event, total, total)?;
                    if self.heap.file_reader(reader)?.ready != 1 {
                        self.fire_reader_progress(reader, "loadend", total, total)?;
                    }
                    Ok(None)
                })();
                self.stack.truncate(base);
                return result;
            }
            _ => unreachable!("bounded file read stage"),
        }
        let state = self.heap.file_reader(reader)?;
        Ok((state.epoch == epoch && state.ready == 1).then_some(stage + 1))
    }

    fn package_file_read(
        &mut self,
        blob: ObjectId,
        kind: FileReadKind,
        label: Option<&JsString>,
    ) -> Result<Value, RuntimeError> {
        let data = self.heap.web_blob(blob)?.clone();
        let result = match kind {
            FileReadKind::ArrayBuffer => {
                let prototype = self.buffer_prototype("ArrayBuffer")?;
                let buffer = self.with_roots(|heap| {
                    heap.alloc_array_buffer(data.bytes.len(), Some(prototype))
                })?;
                self.heap.array_buffer_write(buffer, 0, &data.bytes)?;
                Value::Object(buffer)
            }
            FileReadKind::BinaryString => Value::String(JsString::from_code_units(
                data.bytes.into_iter().map(u16::from).collect(),
            )),
            FileReadKind::DataUrl => {
                let mime = data.media_type.to_utf8().unwrap_or_default();
                let mime = if mime.is_empty() {
                    "application/octet-stream"
                } else {
                    &mime
                };
                Value::String(
                    format!(
                        "data:{mime};base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(data.bytes)
                    )
                    .into(),
                )
            }
            FileReadKind::Text => {
                let explicit = label
                    .and_then(|label| label.to_utf8().ok())
                    .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()));
                let mime = data.media_type.to_utf8().unwrap_or_default();
                let charset = reader_mime_encoding(&mime);
                let encoding = explicit.or(charset).unwrap_or(encoding_rs::UTF_8);
                Value::String(encoding.decode(&data.bytes).0.into_owned().into())
            }
        };
        self.check_string(&result)?;
        Ok(result)
    }
}

// Behavior reference: https://mimesniff.spec.whatwg.org/#parse-a-mime-type
// MIME Sniffing's forgiving parameter parser: quoted semicolons are data,
// invalid essences fail parsing, and the first valid charset parameter wins
// even when its value is not a recognized Encoding Standard label.
fn reader_mime_encoding(input: &str) -> Option<&'static encoding_rs::Encoding> {
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
    let (essence, parameters) = input.split_once(';')?;
    let (main, subtype) = essence.split_once('/')?;
    let subtype = subtype.trim_end_matches(['\t', '\n', '\r', ' ']);
    if !token(main.as_bytes()) || !token(subtype.as_bytes()) {
        return None;
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
        let mut value = Vec::new();
        if bytes.get(position) == Some(&b'"') {
            position += 1;
            while position < bytes.len() {
                let byte = bytes[position];
                position += 1;
                if byte == b'"' {
                    break;
                }
                if byte == b'\\' && position < bytes.len() {
                    value.push(bytes[position]);
                    position += 1;
                } else {
                    value.push(byte);
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
            value.extend_from_slice(&bytes[start..position]);
            while value.last().is_some_and(|byte| whitespace(*byte)) {
                value.pop();
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
            return encoding_rs::Encoding::for_label(&value);
        }
    }
    None
}
