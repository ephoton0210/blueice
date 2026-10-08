// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::heap::{ArrayIteratorKind, WebBlobData};

/// An already admitted immutable content snapshot. No path or file descriptor
/// crosses this ABI; the VM allocates and accounts for every JavaScript object.
pub struct HostFileData {
    pub name: String,
    pub media_type: String,
    pub last_modified: i64,
    pub bytes: Vec<u8>,
}
impl HostFileData {
    fn valid(&self) -> bool {
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
    cached: HashMap<HostObjectKey, (u64, ObjectId, RootId)>,
}

impl Vm {
    /// Adds a branded, read-only files property to this realm's DOM family.
    /// Callbacks may return only owned content, never arbitrary VM values.
    pub fn install_host_file_input_reader(
        &mut self,
        family: HostObjectFamily,
        reader: impl HostInputFilesReader,
    ) -> Result<(), RuntimeError> {
        self.install_web_file_api()?;
        let prototype = self.host_family_prototype(family)?;
        if self
            .heap
            .get_own_property_descriptor(prototype, "files")?
            .is_some()
        {
            return Err(RuntimeError::TypeError(
                "File input reader already installed".into(),
            ));
        }
        let index = u32::try_from(self.host_file_inputs.len())
            .map_err(|_| RuntimeError::RangeError("Too many file input readers".into()))?;
        let function = self.function_prototype()?;
        self.install_native_getter(
            prototype,
            function,
            "files",
            NativeFunction::HostInputFiles(index),
        )?;
        self.host_file_inputs.push(HostFileInputRegistration {
            family,
            reader: Box::new(reader),
            cached: HashMap::new(),
        });
        Ok(())
    }

    pub(super) fn web_input_files(
        &mut self,
        index: u32,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let registration = self
            .host_file_inputs
            .get(index as usize)
            .ok_or_else(|| RuntimeError::TypeError("File input reader unavailable".into()))?;
        let key = self.host_receiver_key(registration.family, receiver)?;
        let cached = registration.cached.get(&key).copied();
        let update = self.host_file_inputs[index as usize]
            .reader
            .read(key, cached.map(|value| value.0))
            .map_err(|error| RuntimeError::TypeError(error.to_string()))?;
        match update {
            HostInputFilesUpdate::Unchanged => cached
                .map(|(_, object, _)| Value::Object(object))
                .ok_or_else(|| RuntimeError::TypeError("File input revision unavailable".into())),
            HostInputFilesUpdate::NotFileInput => {
                if let Some((_, _, root)) =
                    self.host_file_inputs[index as usize].cached.remove(&key)
                {
                    self.heap.unroot(root)?;
                }
                Ok(Value::Null)
            }
            HostInputFilesUpdate::Selected { revision, files } => {
                if cached.is_some_and(|cached| cached.0 == revision) {
                    return Err(RuntimeError::TypeError(
                        "File input revision was reused".into(),
                    ));
                }
                if files.len() > 16
                    || files.iter().any(|file| !file.valid())
                    || files
                        .iter()
                        .fold(0usize, |total, file| total.saturating_add(file.bytes.len()))
                        > 1_048_576
                {
                    return Err(RuntimeError::RangeError(
                        "Selected file snapshot exceeds runtime limits".into(),
                    ));
                }
                if let Some((_, list, root)) = cached {
                    let previous = self.heap.web_file_list(list)?;
                    if previous.len() == files.len()
                        && previous.iter().zip(&files).all(|(object, file)| {
                            self.heap.web_blob(*object).is_ok_and(|blob| {
                                blob.bytes == file.bytes
                                    && blob.media_type
                                        == file.media_type.to_ascii_lowercase().as_str()
                                    && blob.file.as_ref().is_some_and(|(name, time)| {
                                        *name == file.name.as_str() && *time == file.last_modified
                                    })
                            })
                        })
                    {
                        self.host_file_inputs[index as usize]
                            .cached
                            .insert(key, (revision, list, root));
                        return Ok(Value::Object(list));
                    }
                }
                let base = self.stack.len();
                let result = (|| {
                    let mut objects = Vec::with_capacity(files.len());
                    let prototype = self.globals["%WebFile%"];
                    for file in files {
                        if file.name.len() > 255 || file.media_type.len() > 127 {
                            return Err(RuntimeError::RangeError(
                                "Selected file metadata exceeds runtime limits".into(),
                            ));
                        }
                        let object = self.with_roots(|heap| {
                            heap.alloc_web_blob(
                                WebBlobData {
                                    bytes: file.bytes,
                                    media_type: file.media_type.to_ascii_lowercase().into(),
                                    file: Some((file.name.into(), file.last_modified)),
                                },
                                prototype,
                            )
                        })?;
                        self.stack.push(Value::Object(object));
                        objects.push(object);
                    }
                    let prototype = self.globals["%WebFileList%"];
                    let list = self
                        .with_roots(|heap| heap.alloc_web_file_list(objects.clone(), prototype))?;
                    self.stack.push(Value::Object(list));
                    for (index, object) in objects.into_iter().enumerate() {
                        self.define_data(
                            list,
                            index.to_string(),
                            Value::Object(object),
                            false,
                            true,
                            false,
                        )?;
                    }
                    let root = self.heap.root(list)?;
                    if let Some((_, _, old_root)) = self.host_file_inputs[index as usize]
                        .cached
                        .insert(key, (revision, list, root))
                    {
                        self.heap.unroot(old_root)?;
                    }
                    Ok(Value::Object(list))
                })();
                self.stack.truncate(base);
                result
            }
        }
    }

    pub(super) fn web_file_list_call(
        &mut self,
        native: NativeFunction,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let id = receiver
            .object_id()
            .filter(|id| self.heap.web_file_list(*id).is_ok())
            .ok_or_else(|| RuntimeError::TypeError("Invalid FileList receiver".into()))?;
        match native {
            NativeFunction::FileListLength => {
                Ok(Value::Number(self.heap.web_file_list(id)?.len() as f64))
            }
            NativeFunction::FileListItem => {
                if args.is_empty() {
                    return Err(RuntimeError::TypeError(
                        "FileList.item requires an index".into(),
                    ));
                }
                let number = self.coerce_number(&args[0])?;
                let index = if !number.is_finite() {
                    0
                } else {
                    number.trunc().rem_euclid(4_294_967_296.0) as u32
                };
                Ok(self
                    .heap
                    .web_file_list(id)?
                    .get(index as usize)
                    .map_or(Value::Null, |id| Value::Object(*id)))
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
        let iterator = receiver
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Invalid FileList iterator".into()))?;
        let (list, index, done, _) = self
            .heap
            .array_iterator(iterator)?
            .ok_or_else(|| RuntimeError::TypeError("Invalid FileList iterator".into()))?;
        let files = self
            .heap
            .web_file_list(list)
            .map_err(|_| RuntimeError::TypeError("Invalid FileList iterator".into()))?;
        let value = if done {
            None
        } else {
            files.get(index as usize).copied()
        };
        self.heap.advance_array_iterator(iterator, value.is_none());
        self.iterator_result(
            value.map_or(Value::Undefined, Value::Object),
            value.is_none(),
        )
    }
}
