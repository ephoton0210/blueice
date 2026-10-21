// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-installed browser File API. Immutable byte slots belong to the
//! collector; neither a page path nor an embedding object handle is used.

use super::native_value_storage::NativeValue;
use super::realm_call_storage::RealmCallRoots;
use super::transport_buffer_storage::TransportVector;
use super::web_drag_drop::storage::copy_string;
use super::web_event_storage::{EventCompletion, EventContext};
use super::*;
use crate::heap::TypedArrayKind;

pub(super) mod storage;
use storage::{blob_media_type, blob_usv, decode_text, pack_parts, BlobBytes};

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/blob_construct_storage.rs"]
mod storage_tests;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/blob_read_storage.rs"]
mod read_storage_tests;

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

    pub(super) fn web_file_constructor(
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
        let context = EventContext::capture(self)?;
        let ids = args
            .iter()
            .filter_map(Value::object_id)
            .chain(self.new_target.object_id());
        let input_count = ids.clone().count();
        let roots = context.prepare_roots(self, ids, 2)?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            vm.web_blob_construct_owned(args, file, &context, &roots, input_count)
        });
        context.validate().and(result)
    }

    fn web_blob_construct_owned(
        &mut self,
        args: &[Value],
        file: bool,
        context: &EventContext,
        roots: &RealmCallRoots,
        input_count: usize,
    ) -> Result<Value, RuntimeError> {
        let part_roots = RealmCallRoots::new(0, context.owner.as_ref())?;
        let candidate = self.with_realm_call_roots(part_roots.clone(), |vm| {
            let parts = native::argument(args, 0);
            let mut collected_parts = TransportVector::with_capacity(0, context.owner.as_ref())?;
            let mut string_bytes = 0usize;
            if file || !matches!(parts, Value::Undefined) {
                if parts.object_id().is_none() {
                    return Err(RuntimeError::TypeError(
                        "Blob parts must be an iterable object".into(),
                    ));
                }
                let record = vm.get_iterator(parts);
                let record = context.validate().and(record)?;
                if let Some(id) = record.object_id() {
                    roots.push(&vm.heap, id)?;
                }
                let record_count = input_count + usize::from(record.object_id().is_some());
                let collected = (|| {
                    loop {
                        let next = vm.iterator_step(&record, true);
                        let Some(part) = context.validate().and(next)? else {
                            break;
                        };
                        let part = NativeValue::adopt(part, context.owner.as_ref())?;
                        if let Some(id) = part.value().object_id() {
                            roots.push(&vm.heap, id)?;
                        }
                        vm.charge_step()?;
                        let next = vm.web_blob_convert_part_owned(part, context)?;
                        if let Value::String(value) = next.value() {
                            string_bytes = string_bytes.saturating_add(value.byte_len());
                        }
                        if collected_parts.len() >= 4096
                            || string_bytes > vm.config.heap.max_heap_bytes
                        {
                            return Err(RuntimeError::RangeError(
                                "Blob parts exceed runtime limits".into(),
                            ));
                        }
                        if let Some(id) = next.value().object_id() {
                            part_roots.push_admitted(&vm.heap, id)?;
                        }
                        collected_parts.push_owned(next)?;
                        roots.truncate(record_count);
                    }
                    Ok(())
                })();
                if let Err(error) = collected {
                    // A canceled document cannot enter IteratorClose. An
                    // ordinary throw keeps its original payload and object
                    // edge alive while return() runs, including when it throws.
                    context.validate()?;
                    roots.truncate(record_count);
                    let error = EventCompletion::adopt(Err(error), context.owner.as_ref())?;
                    if let Err(RuntimeError::Thrown(value)) = error.result() {
                        if let Some(id) = value.object_id() {
                            roots.push(&vm.heap, id)?;
                        }
                    }
                    vm.iterator_close_preserving_throw(&record);
                    context.validate()?;
                    return error.into_result().map(|_| unreachable!());
                }
                roots.truncate(input_count);
            }
            let name = if file {
                Some(blob_usv(
                    vm.blob_string(native::argument(args, 1), context)?,
                ))
            } else {
                None
            };
            let options = native::argument(args, if file { 2 } else { 1 });
            if !matches!(options, Value::Undefined | Value::Null | Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "Blob options must be a dictionary".into(),
                ));
            }
            let mut media_type =
                NativeValue::adopt(Value::String(JsString::default()), context.owner.as_ref())?;
            let mut native_endings = false;
            let mut last_modified = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |time| time.as_millis() as i64);
            if options.object_id().is_some() {
                // Dictionary members are read in Web IDL lexicographic order.
                let endings = vm.blob_option(options, "endings", context)?;
                if *endings.value() != Value::Undefined {
                    let endings = vm.blob_owned_string(endings, context)?;
                    let Value::String(text) = endings.value() else {
                        unreachable!()
                    };
                    if text != "transparent" && text != "native" {
                        return Err(RuntimeError::TypeError("Invalid Blob endings".into()));
                    }
                    native_endings = text == "native";
                }
                if file {
                    let value = vm.blob_option(options, "lastModified", context)?;
                    if *value.value() != Value::Undefined {
                        let number = value.with_value(vm, |vm, value| vm.coerce_number(value));
                        last_modified = web_long_long(context.validate().and(number)?);
                    }
                }
                let value = vm.blob_option(options, "type", context)?;
                if *value.value() != Value::Undefined {
                    media_type = blob_media_type(vm.blob_owned_string(value, context)?)?;
                }
            }
            let bytes = pack_parts(vm, collected_parts, native_endings, context)?;
            bytes.into_candidate(media_type, name.map(|name| (name, last_modified)))
        });
        // Finished part snapshots and their root backing are gone before the
        // newTarget prototype getter. Only final bytes and metadata remain.
        drop(part_roots);
        let candidate = context.validate().and(candidate)?;
        let default = self.globals[if file { "%WebFile%" } else { "%WebBlob%" }];
        let prototype = self.constructor_prototype(default);
        let prototype = context.validate().and(prototype)?;
        Ok(Value::Object(candidate.allocate(self, prototype)?))
    }

    fn web_blob_convert_part_owned(
        &mut self,
        part: NativeValue,
        context: &EventContext,
    ) -> Result<NativeValue, RuntimeError> {
        if let Some(id) = part.value().object_id() {
            if self.heap.web_blob(id).is_ok()
                || self.heap.is_buffer(id)?
                || self.heap.is_data_view(id)?
                || self.heap.is_typed_array(id)?
            {
                return Ok(part);
            }
        }
        let value = blob_usv(self.blob_owned_string(part, context)?);
        self.check_string(value.value())?;
        Ok(value)
    }

    fn blob_string(
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

    fn blob_owned_string(
        &mut self,
        value: NativeValue,
        context: &EventContext,
    ) -> Result<NativeValue, RuntimeError> {
        if matches!(value.value(), Value::String(_)) {
            return Ok(value);
        }
        value.with_value(self, |vm, value| vm.blob_string(value, context))
    }

    fn blob_option(
        &mut self,
        options: &Value,
        field: &str,
        context: &EventContext,
    ) -> Result<NativeValue, RuntimeError> {
        let (Value::String(key), _storage) =
            error_input_storage::utf16_from_utf8(field, context.owner.as_ref())?
                .into_retained_parts()
        else {
            unreachable!()
        };
        let key = PropertyName::from(key);
        let value = self.get_property(options, &key);
        let value = context.validate().and(value)?;
        NativeValue::adopt(value, context.owner.as_ref())
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
        let context = EventContext::capture(self)?;
        let ids = std::iter::once(id).chain(args.iter().filter_map(Value::object_id));
        let roots = context.prepare_roots(self, ids, 1)?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            vm.web_blob_read_owned(native, id, args, &context, &roots)
        });
        context.validate().and(result)
    }

    fn web_blob_read_owned(
        &mut self,
        native: NativeFunction,
        id: ObjectId,
        args: &[Value],
        context: &EventContext,
        roots: &RealmCallRoots,
    ) -> Result<Value, RuntimeError> {
        let blob = self.heap.web_blob(id)?;
        match native {
            NativeFunction::BlobSize => return Ok(Value::Number(blob.bytes.len() as f64)),
            NativeFunction::BlobType => {
                return Ok(copy_string(&blob.media_type, context.owner.as_ref())?.into_value())
            }
            NativeFunction::FileName => {
                let (name, _) = blob
                    .file
                    .as_ref()
                    .ok_or_else(|| RuntimeError::TypeError("Invalid File receiver".into()))?;
                return Ok(copy_string(name, context.owner.as_ref())?.into_value());
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
            let start = self.coerce_number(native::argument(args, 0));
            let start = blob_index(context.validate().and(start)?, length);
            let end = if args.get(1).is_none_or(|value| *value == Value::Undefined) {
                length
            } else {
                let end = self.coerce_number(&args[1]);
                blob_index(context.validate().and(end)?, length)
            };
            let media_type = if args.get(2).is_none_or(|value| *value == Value::Undefined) {
                NativeValue::adopt(Value::String(JsString::default()), context.owner.as_ref())?
            } else {
                blob_media_type(self.blob_string(&args[2], context)?)?
            };
            let bytes = BlobBytes::copied(
                &self.heap.web_blob(id)?.bytes[start..end.max(start)],
                context.owner.as_ref(),
            )?;
            let candidate = bytes.into_candidate(media_type, None)?;
            let prototype = self.globals["%WebBlob%"];
            return Ok(Value::Object(candidate.allocate(self, prototype)?));
        }
        let value = if native == NativeFunction::BlobText {
            let text = decode_text(
                &self.heap.web_blob(id)?.bytes,
                self.config.max_string_bytes,
                context.owner.as_ref(),
            )?;
            self.check_string(text.value())?;
            text
        } else {
            let bytes = BlobBytes::copied(&self.heap.web_blob(id)?.bytes, context.owner.as_ref())?;
            let length = bytes.len();
            let prototype = self.buffer_prototype("ArrayBuffer");
            let prototype = context.validate().and(prototype)?;
            let buffer =
                self.with_roots(|heap| heap.alloc_array_buffer(length, Some(prototype)))?;
            roots.push(&self.heap, buffer)?;
            // Only the admitted managed buffer is needed by later view and
            // Promise preparation; destroy this independent copy now.
            bytes.copy_into_buffer(self, buffer)?;
            let value = if native == NativeFunction::BlobBytes {
                let prototype = self.buffer_prototype("Uint8Array");
                let prototype = context.validate().and(prototype)?;
                Value::Object(self.with_roots(|heap| {
                    heap.alloc_typed_array(
                        buffer,
                        0,
                        length,
                        false,
                        TypedArrayKind::Uint8,
                        Some(prototype),
                    )
                })?)
            } else {
                Value::Object(buffer)
            };
            NativeValue::adopt(value, context.owner.as_ref())?
        };
        let result = self.promise_resolve_retained(value);
        context.validate().and(result)
    }
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
