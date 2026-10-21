// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::heap::ArrayIteratorKind;
#[cfg(any(test, coverage))]
use crate::heap::WebBlobData;
use crate::vm::membrane_map::MembraneMap;
use crate::vm::transport_buffer_storage::TransportVector;
use crate::vm::web_event_storage::EventContext as FileListContext;
mod storage;
use storage::{FileInputContext, NativeFileUpdate};

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/file_input_registry.rs"]
mod registry_tests;
#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/file_list_storage.rs"]
mod storage_tests;

/// An already admitted immutable content snapshot. No path or file descriptor
/// crosses this ABI; the VM allocates and accounts for every JavaScript object.
pub struct HostFileData {
    pub name: String,
    pub media_type: String,
    pub last_modified: i64,
    pub bytes: Vec<u8>,
}
impl HostFileData {
    pub(super) fn valid(&self) -> bool {
        !self.name.is_empty()
            && self.name.len() <= 255
            && !matches!(self.name.as_str(), "." | "..")
            && !self
                .name
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, '/' | '\\' | ':'))
            && self.media_type.len() <= 127
            && self.media_type.split('/').count() == 2
            && self.media_type.split('/').all(|token| {
                !token.is_empty()
                    && token
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&byte))
            })
    }
}

pub enum HostInputFilesUpdate {
    NotFileInput,
    Unchanged,
    Selected {
        revision: u64,
        files: Vec<HostFileData>,
    },
}

pub trait HostInputFilesReader: 'static {
    fn read(
        &mut self,
        key: HostObjectKey,
        known_revision: Option<u64>,
    ) -> Result<HostInputFilesUpdate, HostFunctionError>;
}
impl<F> HostInputFilesReader for F
where
    F: FnMut(HostObjectKey, Option<u64>) -> Result<HostInputFilesUpdate, HostFunctionError>
        + 'static,
{
    fn read(
        &mut self,
        key: HostObjectKey,
        revision: Option<u64>,
    ) -> Result<HostInputFilesUpdate, HostFunctionError> {
        self(key, revision)
    }
}

pub(super) struct HostFileInputRegistration {
    family: HostObjectFamily,
    reader: Box<dyn HostInputFilesReader>,
    cached: MembraneMap<HostObjectKey, (u64, ObjectId, RootId)>,
}

impl host_objects::RegistrationRecord for HostFileInputRegistration {
    fn callback_bytes(&self) -> usize {
        std::mem::size_of_val(&*self.reader)
    }
}

impl Vm {
    /// Adds a branded, read-only files property to this realm's DOM family.
    /// Callbacks may return only owned content, never arbitrary VM values.
    pub fn install_host_file_input_reader(
        &mut self,
        family: HostObjectFamily,
        reader: impl HostInputFilesReader,
    ) -> Result<(), RuntimeError> {
        let context = FileInputContext::capture(self)?;
        let prototype = self.host_family_prototype(family)?;
        context.with_roots(self, std::iter::once(prototype), 0, |vm, _| {
            vm.install_web_file_api()?;
            if vm
                .heap
                .get_own_property_descriptor(prototype, "files")?
                .is_some()
            {
                return Err(RuntimeError::TypeError(
                    "File input reader already installed".into(),
                ));
            }
            let index = u32::try_from(vm.host_file_inputs.len())
                .map_err(|_| RuntimeError::RangeError("Too many file input readers".into()))?;
            let prepared = vm.host_file_inputs.prepare(1, context.owner())?;
            let callbacks = prepared.callbacks(&[std::mem::size_of_val(&reader)], || {
                [HostFileInputRegistration {
                    family,
                    reader: Box::new(reader),
                    cached: Default::default(),
                }]
            })?;
            let function = vm.function_prototype()?;
            context.validate(vm)?;
            vm.install_native_getter(
                prototype,
                function,
                "files",
                NativeFunction::HostInputFiles(index),
            )?;
            vm.host_file_inputs.commit(prepared, callbacks);
            Ok(())
        })
    }

    pub(super) fn web_input_files(
        &mut self,
        index: u32,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let context = FileInputContext::capture(self)?;
        let registration = self
            .host_file_inputs
            .get(index as usize)
            .ok_or_else(|| RuntimeError::TypeError("File input reader unavailable".into()))?;
        let key = self.host_receiver_key(registration.family, receiver)?;
        context.with_roots(self, receiver.object_id().into_iter(), 0, |vm, _| {
            vm.web_input_files_read(index, key, &context)
        })
    }

    fn web_input_files_read(
        &mut self,
        index: u32,
        key: HostObjectKey,
        context: &FileInputContext,
    ) -> Result<Value, RuntimeError> {
        let cached = self.host_file_inputs[index as usize]
            .cached
            .get(&key)
            .copied();
        let update = self.host_file_inputs[index as usize]
            .reader
            .read(key, cached.map(|value| value.0));
        context.validate(self)?;
        let update = NativeFileUpdate::adopt(
            update.map_err(|error| RuntimeError::TypeError(error.0))?,
            context.owner(),
        )?;
        let (revision, files) = match &update.value {
            HostInputFilesUpdate::Unchanged => {
                return cached
                    .map(|(_, object, _)| Value::Object(object))
                    .ok_or_else(|| {
                        RuntimeError::TypeError("File input revision unavailable".into())
                    })
            }
            HostInputFilesUpdate::NotFileInput => {
                context.validate(self)?;
                if let Some((_, _, root)) =
                    self.host_file_inputs[index as usize].cached.remove(&key)
                {
                    self.heap.unroot(root)?;
                }
                self.host_file_inputs[index as usize].cached.release_empty();
                return Ok(Value::Null);
            }
            HostInputFilesUpdate::Selected { revision, files } => (*revision, files),
        };
        if cached.is_some_and(|cached| cached.0 == revision) {
            return Err(RuntimeError::TypeError(
                "File input revision was reused".into(),
            ));
        }
        if files.len() > 16
            || files.iter().any(|file| !file.valid())
            || files
                .iter()
                .fold(0usize, |bytes, file| bytes.saturating_add(file.bytes.len()))
                > 1_048_576
        {
            return Err(RuntimeError::RangeError(
                "Selected file snapshot exceeds runtime limits".into(),
            ));
        }
        if let Some((_, list, root)) = cached {
            let previous = self.heap.web_file_list(list)?;
            if previous.clone().count() == files.len()
                && previous.zip(files).all(|(object, file)| {
                    self.heap.web_blob(object).is_ok_and(|blob| {
                        blob.bytes == file.bytes
                            && blob.media_type.as_code_units().iter().copied().eq(file
                                .media_type
                                .bytes()
                                .map(|byte| u16::from(byte.to_ascii_lowercase())))
                            && blob.file.as_ref().is_some_and(|(name, time)| {
                                *name == file.name.as_str() && *time == file.last_modified
                            })
                    })
                })
            {
                context.validate(self)?;
                self.host_file_inputs[index as usize]
                    .cached
                    .set(key, (revision, list, root));
                return Ok(Value::Object(list));
            }
        }
        let (revision, mut input) = update.into_selected();
        let count = input.len();
        let growth = self.host_file_inputs[index as usize]
            .cached
            .prepare_set_owned(&key, context.owner())?;
        context.with_roots(self, std::iter::empty(), count + 1, |vm, roots| {
            let mut objects = TransportVector::<ObjectId>::with_capacity(count, context.owner())?;
            let prototype = vm.globals["%WebFile%"];
            while let Some(file) = input.next_owned()? {
                let object = file.into_blob()?.allocate(vm, prototype)?;
                roots.push(&vm.heap, object)?;
                objects.push(object)?;
            }
            drop(input);
            let prototype = vm.globals["%WebFileList%"];
            let list = objects.into_file_list(vm, prototype)?;
            roots.push(&vm.heap, list)?;
            for index in 0..count {
                let object = vm
                    .heap
                    .web_file_list(list)?
                    .nth(index)
                    .expect("prepared selected file");
                vm.define_data(
                    list,
                    index.to_string(),
                    Value::Object(object),
                    false,
                    true,
                    false,
                )?;
            }
            let objects = [list];
            let root = vm.heap.prepare_root_batch(&objects)?;
            context.validate(vm)?;
            growth.validate_owner()?;
            let root = vm.heap.commit_single_root(root);
            let cache = &mut vm.host_file_inputs[index as usize].cached;
            cache.commit(growth);
            cache.set(key, (revision, list, root));
            if let Some((_, _, old_root)) = cached {
                vm.heap.unroot(old_root)?;
            }
            Ok(Value::Object(list))
        })
    }

    pub(super) fn web_file_list_call(
        &mut self,
        native: NativeFunction,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let context = FileListContext::capture(self)?;
        let ids = receiver
            .object_id()
            .into_iter()
            .chain(args.iter().filter_map(Value::object_id));
        context.with_roots(self, ids, |vm| {
            vm.web_file_list_input(native, receiver, args, &context)
        })
    }

    fn web_file_list_input(
        &mut self,
        native: NativeFunction,
        receiver: &Value,
        args: &[Value],
        context: &FileListContext,
    ) -> Result<Value, RuntimeError> {
        let id = receiver
            .object_id()
            .filter(|id| self.heap.web_file_list(*id).is_ok())
            .ok_or_else(|| RuntimeError::TypeError("Invalid FileList receiver".into()))?;
        match native {
            NativeFunction::FileListLength => {
                Ok(Value::Number(self.heap.web_file_list(id)?.count() as f64))
            }
            NativeFunction::FileListItem => {
                if args.is_empty() {
                    return Err(RuntimeError::TypeError(
                        "FileList.item requires an index".into(),
                    ));
                }
                let number = self.coerce_number(&args[0]);
                context.validate()?;
                let number = number?;
                let index = if !number.is_finite() {
                    0
                } else {
                    number.trunc().rem_euclid(4_294_967_296.0) as u32
                };
                Ok(self
                    .heap
                    .web_file_list(id)?
                    .nth(index as usize)
                    .map_or(Value::Null, Value::Object))
            }
            NativeFunction::FileListIterator => {
                let prototype = if let Some(&prototype) = self.globals.get("%WebFileListIterator%")
                {
                    prototype
                } else {
                    let parent = self.array_iterator_prototype()?;
                    let prototype = self.with_roots(|heap| heap.alloc_object(Some(parent)))?;
                    self.stack.push(Value::Object(prototype));
                    let function = self.function_prototype()?;
                    let installed = self.install_native(
                        prototype,
                        function,
                        "next",
                        0,
                        NativeFunction::FileListIteratorNext,
                    );
                    self.stack.pop();
                    installed?;
                    // The globals map is an intrinsic cache, not a GC root.
                    // The private prototype has no globalThis property.
                    self.heap.root(prototype)?;
                    self.globals
                        .insert("%WebFileListIterator%".into(), prototype);
                    prototype
                };
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_array_iterator(id, ArrayIteratorKind::Values, prototype)
                })?))
            }
            _ => Err(RuntimeError::TypeError("Invalid FileList method".into())),
        }
    }

    pub(super) fn web_file_list_iterator_next(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let context = FileListContext::capture(self)?;
        context.with_roots(self, receiver.object_id().into_iter(), |vm| {
            vm.web_file_list_next_input(receiver, &context)
        })
    }

    fn web_file_list_next_input(
        &mut self,
        receiver: &Value,
        context: &FileListContext,
    ) -> Result<Value, RuntimeError> {
        let iterator = receiver
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Invalid FileList iterator".into()))?;
        let (list, index, done, _) = self
            .heap
            .array_iterator(iterator)?
            .ok_or_else(|| RuntimeError::TypeError("Invalid FileList iterator".into()))?;
        let value = {
            let mut files = self
                .heap
                .web_file_list(list)
                .map_err(|_| RuntimeError::TypeError("Invalid FileList iterator".into()))?;
            if done {
                None
            } else {
                files.nth(index as usize)
            }
        };
        context.validate()?;
        self.heap.advance_array_iterator(iterator, value.is_none());
        self.iterator_result(
            value.map_or(Value::Undefined, Value::Object),
            value.is_none(),
        )
    }
}
