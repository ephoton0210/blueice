// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-installed browser File API. Immutable byte slots belong to the
//! collector; neither a page path nor an embedding object handle is used.

use super::*;
use crate::heap::{TypedArrayKind, WebBlobData};

impl Vm {
    pub(super) fn install_web_files(&mut self) -> Result<(), RuntimeError> {
        let global = self.global("globalThis")?.object_id().unwrap();
        if self.globals.contains_key("%WebBlob%") {
            return Ok(());
        }
        for name in ["Blob", "File", "FileList"] {
            if self.heap.get_own(global, name)?.is_some() {
                return Err(RuntimeError::TypeError(
                    "File API global already defined".into(),
                ));
            }
        }
        let base = self.stack.len();
        let result = (|| {
            let function = self.function_prototype()?;
            let object = self.object_prototype;
            let blob = self.with_roots(|heap| heap.alloc_object(Some(object)))?;
            self.stack.push(Value::Object(blob));
            let file = self.with_roots(|heap| heap.alloc_object(Some(blob)))?;
            self.stack.push(Value::Object(file));
            let file_list = self.with_roots(|heap| heap.alloc_object(Some(object)))?;
            self.stack.push(Value::Object(file_list));
            self.install_native_getter(
                file_list,
                function,
                "length",
                NativeFunction::FileListLength,
            )?;
            self.install_native(file_list, function, "item", 1, NativeFunction::FileListItem)?;
            self.install_symbol_native(
                file_list,
                function,
                "iterator",
                0,
                NativeFunction::FileListIterator,
            )?;
            for (prototype, name) in [(blob, "Blob"), (file, "File"), (file_list, "FileList")] {
                self.define_data(
                    prototype,
                    JsSymbol::well_known("toStringTag"),
                    Value::String(name.into()),
                    false,
                    false,
                    true,
                )?;
            }
            for (name, native) in [
                ("size", NativeFunction::BlobSize),
                ("type", NativeFunction::BlobType),
            ] {
                self.install_native_getter(blob, function, name, native)?;
            }
            for (name, length, native) in [
                ("slice", 0, NativeFunction::BlobSlice),
                ("text", 0, NativeFunction::BlobText),
                ("arrayBuffer", 0, NativeFunction::BlobArrayBuffer),
                ("bytes", 0, NativeFunction::BlobBytes),
            ] {
                self.install_native(blob, function, name, length, native)?;
            }
            for (name, native) in [
                ("name", NativeFunction::FileName),
                ("lastModified", NativeFunction::FileLastModified),
            ] {
                self.install_native_getter(file, function, name, native)?;
            }
            for (prototype, names) in [
                (
                    blob,
                    &["size", "type", "slice", "text", "arrayBuffer", "bytes"][..],
                ),
                (file, &["name", "lastModified"][..]),
                (file_list, &["length", "item"][..]),
            ] {
                for name in names {
                    self.with_roots(|heap| {
                        heap.define_own_property(
                            prototype,
                            *name,
                            PropertyDescriptor {
                                enumerable: Some(true),
                                ..Default::default()
                            },
                        )
                    })?;
                }
            }
            // Publish only after both branded prototypes are fully built.
            let blob_constructor =
                self.web_file_constructor("Blob", 0, NativeFunction::Blob, function, blob)?;
            self.stack.push(Value::Object(blob_constructor));
            let file_constructor =
                self.web_file_constructor("File", 2, NativeFunction::File, blob_constructor, file)?;
            self.stack.push(Value::Object(file_constructor));
            let file_list_constructor = self.web_file_constructor(
                "FileList",
                0,
                NativeFunction::FileList,
                function,
                file_list,
            )?;
            self.stack.push(Value::Object(file_list_constructor));
            self.define_data(
                global,
                "Blob",
                Value::Object(blob_constructor),
                true,
                false,
                true,
            )?;
            self.define_data(
                global,
                "File",
                Value::Object(file_constructor),
                true,
                false,
                true,
            )?;
            self.define_data(
                global,
                "FileList",
                Value::Object(file_list_constructor),
                true,
                false,
                true,
            )?;
            // Keep intrinsic prototypes alive even if page code replaces a
            // global constructor or modifies its visible prototype chain.
            self.heap.root(blob)?;
            self.heap.root(file)?;
            self.heap.root(file_list)?;
            self.globals.insert("%WebBlob%".into(), blob);
            self.globals.insert("%WebFile%".into(), file);
            self.globals.insert("%WebFileList%".into(), file_list);
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    fn web_file_constructor(
        &mut self,
        name: &str,
        length: u32,
        native: NativeFunction,
        function: ObjectId,
        prototype: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        let constructor =
            self.with_roots(|heap| heap.alloc_native_function(native, name, function))?;
        self.stack.push(Value::Object(constructor));
        let result = (|| {
            self.define_data(
                constructor,
                "name",
                Value::String(name.into()),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "length",
                Value::Number(length.into()),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "prototype",
                Value::Object(prototype),
                false,
                false,
                false,
            )?;
            self.define_data(
                prototype,
                "constructor",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            Ok(constructor)
        })();
        self.stack.pop();
        result
    }

    pub(super) fn web_blob_construct(
        &mut self,
        args: &[Value],
        construct: bool,
        file: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct || file && args.len() < 2 {
            return Err(RuntimeError::TypeError(
                "File API constructor requires new and its required arguments".into(),
            ));
        }
        let base = self.stack.len();
        self.stack.extend_from_slice(args);
        let result = (|| {
            let parts = native::argument(args, 0);
            let mut part_slots = Vec::new();
            let mut string_bytes = 0usize;
            if file || !matches!(parts, Value::Undefined) {
                if parts.object_id().is_none() {
                    return Err(RuntimeError::TypeError(
                        "Blob parts must be an iterable object".into(),
                    ));
                }
                let record = self.get_iterator(parts)?;
                self.stack.push(record.clone());
                let collected = (|| {
                    while let Some(part) = self.iterator_step(&record, true)? {
                        self.charge_step()?;
                        self.stack.push(part.clone());
                        let next = self.web_blob_convert_part(&part);
                        self.stack.pop();
                        let next = next?;
                        if let Value::String(value) = &next {
                            string_bytes = string_bytes.saturating_add(value.byte_len());
                        }
                        if part_slots.len() >= 4096
                            || string_bytes > self.config.heap.max_heap_bytes
                        {
                            return Err(RuntimeError::RangeError(
                                "Blob parts exceed runtime limits".into(),
                            ));
                        }
                        part_slots.push(self.stack.len());
                        self.stack.push(next);
                    }
                    Ok(())
                })();
                if let Err(error) = collected {
                    let error_base = self.stack.len();
                    if let RuntimeError::Thrown(value) = &error {
                        self.stack.push(value.clone());
                    }
                    let _ = self.iterator_close(&record);
                    self.stack.truncate(error_base);
                    return Err(error);
                }
            }
            let name = if file {
                Some(
                    String::from_utf16_lossy(
                        self.coerce_string(native::argument(args, 1))?
                            .as_code_units(),
                    )
                    .into(),
                )
            } else {
                None
            };
            let options = native::argument(args, if file { 2 } else { 1 });
            if !matches!(options, Value::Undefined | Value::Null | Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "Blob options must be a dictionary".into(),
                ));
            }
            let mut media_type = JsString::default();
            let mut native_endings = false;
            let mut last_modified = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |time| time.as_millis() as i64);
            if options.object_id().is_some() {
                // Dictionary members are read in Web IDL lexicographic order.
                let endings = self.get_property(options, &"endings".into())?;
                if endings != Value::Undefined {
                    let endings = self.coerce_string(&endings)?;
                    if endings != "transparent" && endings != "native" {
                        return Err(RuntimeError::TypeError("Invalid Blob endings".into()));
                    }
                    native_endings = endings == "native";
                }
                if file {
                    let value = self.get_property(options, &"lastModified".into())?;
                    if value != Value::Undefined {
                        last_modified = web_long_long(self.coerce_number(&value)?);
                    }
                }
                let value = self.get_property(options, &"type".into())?;
                if value != Value::Undefined {
                    media_type = normalize_blob_type(&self.coerce_string(&value)?);
                }
            }
            let mut bytes = Vec::new();
            for slot in part_slots {
                self.charge_step()?;
                let part = self.stack[slot].clone();
                let mut next = self.web_blob_part(&part)?;
                if native_endings && matches!(part, Value::String(_)) {
                    next = normalize_line_endings(&next);
                }
                if bytes.len().saturating_add(next.len()) > self.heap.max_array_buffer_byte_length()
                {
                    return Err(RuntimeError::RangeError(
                        "Blob data exceeds heap byte limit".into(),
                    ));
                }
                bytes.extend(next);
            }
            let default = self.globals[if file { "%WebFile%" } else { "%WebBlob%" }];
            let prototype = self.constructor_prototype(default)?;
            Ok(Value::Object(self.with_roots(|heap| {
                heap.alloc_web_blob(
                    WebBlobData {
                        bytes,
                        media_type,
                        file: name.map(|name| (name, last_modified)),
                    },
                    prototype,
                )
            })?))
        })();
        self.stack.truncate(base);
        result
    }

    fn web_blob_convert_part(&mut self, part: &Value) -> Result<Value, RuntimeError> {
        if let Some(id) = part.object_id() {
            if self.heap.web_blob(id).is_ok()
                || self.heap.is_buffer(id)?
                || self.heap.is_data_view(id)?
                || self.heap.is_typed_array(id)?
            {
                return Ok(part.clone());
            }
        }
        let string = self.coerce_string(part)?;
        let value = Value::String(String::from_utf16_lossy(string.as_code_units()).into());
        self.check_string(&value)?;
        Ok(value)
    }

    fn web_blob_part(&mut self, part: &Value) -> Result<Vec<u8>, RuntimeError> {
        if let Some(id) = part.object_id() {
            if let Ok(blob) = self.heap.web_blob(id) {
                return Ok(blob.bytes.clone());
            }
            if self.heap.is_buffer(id)? {
                let length = self.heap.buffer_byte_length(id)?;
                return Ok(self.heap.array_buffer_copy(id, 0, length)?);
            }
            if self.heap.is_data_view(id)? {
                let (buffer, offset, length) = self.heap.data_view_current_info(id)?;
                return Ok(self.heap.array_buffer_copy(buffer, offset, length)?);
            }
            if self.heap.is_typed_array(id)? {
                let (buffer, offset, length, kind) = self.heap.typed_array_info(id)?;
                if self.heap.buffer_is_detached(buffer)?
                    || self.heap.typed_array_is_out_of_bounds(id)?
                {
                    return Err(RuntimeError::TypeError(
                        "Blob part view is detached or out of bounds".into(),
                    ));
                }
                return Ok(self.heap.array_buffer_copy(
                    buffer,
                    offset,
                    length * kind.byte_width(),
                )?);
            }
        }
        let string = self.coerce_string(part)?;
        Ok(String::from_utf16_lossy(string.as_code_units()).into_bytes())
    }

    pub(super) fn web_blob_call(
        &mut self,
        native: NativeFunction,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let id = receiver
            .object_id()
            .filter(|id| self.heap.web_blob(*id).is_ok())
            .ok_or_else(|| RuntimeError::TypeError("Invalid Blob receiver".into()))?;
        let blob = self.heap.web_blob(id)?;
        match native {
            NativeFunction::BlobSize => return Ok(Value::Number(blob.bytes.len() as f64)),
            NativeFunction::BlobType => return Ok(Value::String(blob.media_type.clone())),
            NativeFunction::FileName => {
                return blob
                    .file
                    .as_ref()
                    .map(|(name, _)| Value::String(name.clone()))
                    .ok_or_else(|| RuntimeError::TypeError("Invalid File receiver".into()))
            }
            NativeFunction::FileLastModified => {
                return blob
                    .file
                    .as_ref()
                    .map(|(_, time)| Value::Number(*time as f64))
                    .ok_or_else(|| RuntimeError::TypeError("Invalid File receiver".into()))
            }
            _ => {}
        }
        if native == NativeFunction::BlobSlice {
            let length = blob.bytes.len();
            let start = blob_index(self.coerce_number(native::argument(args, 0))?, length);
            let end = if args.get(1).is_none_or(|value| *value == Value::Undefined) {
                length
            } else {
                blob_index(self.coerce_number(&args[1])?, length)
            };
            let media_type = if args.get(2).is_none_or(|value| *value == Value::Undefined) {
                JsString::default()
            } else {
                normalize_blob_type(&self.coerce_string(&args[2])?)
            };
            let bytes = self.heap.web_blob(id)?.bytes[start..end.max(start)].to_vec();
            let prototype = self.globals["%WebBlob%"];
            return Ok(Value::Object(self.with_roots(|heap| {
                heap.alloc_web_blob(
                    WebBlobData {
                        bytes,
                        media_type,
                        file: None,
                    },
                    prototype,
                )
            })?));
        }
        let bytes = blob.bytes.clone();
        let base = self.stack.len();
        let result = (|| {
            let value = if native == NativeFunction::BlobText {
                // UTF-8 decode consumes one leading BOM; binary reads keep it.
                let text_bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
                let text: JsString = String::from_utf8_lossy(text_bytes).into_owned().into();
                let value = Value::String(text);
                self.check_string(&value)?;
                value
            } else {
                let prototype = self.buffer_prototype("ArrayBuffer")?;
                let buffer =
                    self.with_roots(|heap| heap.alloc_array_buffer(bytes.len(), Some(prototype)))?;
                self.stack.push(Value::Object(buffer));
                self.heap.array_buffer_write(buffer, 0, &bytes)?;
                if native == NativeFunction::BlobBytes {
                    let prototype = self.buffer_prototype("Uint8Array")?;
                    Value::Object(self.with_roots(|heap| {
                        heap.alloc_typed_array(
                            buffer,
                            0,
                            bytes.len(),
                            false,
                            TypedArrayKind::Uint8,
                            Some(prototype),
                        )
                    })?)
                } else {
                    Value::Object(buffer)
                }
            };
            self.promise_resolve(value)
        })();
        self.stack.truncate(base);
        result
    }
}

fn normalize_blob_type(value: &JsString) -> JsString {
    if value
        .as_code_units()
        .iter()
        .any(|unit| !(0x20..=0x7e).contains(unit))
    {
        return JsString::default();
    }
    value
        .to_utf8()
        .expect("ASCII type")
        .to_ascii_lowercase()
        .into()
}

fn blob_index(value: f64, length: usize) -> usize {
    if value.is_nan() {
        return 0;
    }
    if value < 0.0 {
        (length as f64 + value.trunc()).max(0.0) as usize
    } else {
        value.trunc().min(length as f64) as usize
    }
}

fn web_long_long(value: f64) -> i64 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    let value = value.trunc() % 18_446_744_073_709_551_616.0;
    if value >= 9_223_372_036_854_775_808.0 {
        (value - 18_446_744_073_709_551_616.0) as i64
    } else if value < -9_223_372_036_854_775_808.0 {
        (value + 18_446_744_073_709_551_616.0) as i64
    } else {
        value as i64
    }
}

fn normalize_line_endings(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\r' {
            if cfg!(windows) {
                result.push(b'\r');
            }
            result.push(b'\n');
            if bytes.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
        } else {
            if cfg!(windows) && bytes[index] == b'\n' {
                result.push(b'\r');
            }
            result.push(bytes[index]);
        }
        index += 1;
    }
    result
}
