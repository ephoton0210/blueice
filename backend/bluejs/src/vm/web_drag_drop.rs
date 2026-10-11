// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Browser drag values use heap brands and owned data. An OS drag store is
//! separate from a page-constructed, writable DataTransfer.

use super::*;
use crate::heap::{DragItem, DragItemData, DragStore, DragStoreMode};
use crate::native::DragNative;

const MAX_DRAG_ITEMS: usize = 4096;

impl Vm {
    pub(super) fn install_web_drag_drop(&mut self) -> Result<(), RuntimeError> {
        if self.globals.contains_key("%WebDataTransfer%") {
            return Ok(());
        }
        let global = self.global("globalThis")?.object_id().unwrap();
        for name in [
            "DataTransfer",
            "DataTransferItemList",
            "DataTransferItem",
            "DragEvent",
        ] {
            if self.heap.get_own(global, name)?.is_some() {
                return Err(RuntimeError::TypeError(
                    "Drag API global already defined".into(),
                ));
            }
        }
        self.install_web_dom_exception()?;
        self.install_web_events()?;
        let base = self.stack.len();
        let result = (|| {
            let function = self.function_prototype()?;
            let mut prototypes = Vec::new();
            for index in 0..4 {
                let parent = if index == 3 {
                    self.globals["%WebMouseEvent%"]
                } else {
                    self.object_prototype
                };
                let prototype = self.with_roots(|heap| heap.alloc_object(Some(parent)))?;
                self.stack.push(Value::Object(prototype));
                prototypes.push(prototype);
            }
            let (transfer, list, item, event) =
                (prototypes[0], prototypes[1], prototypes[2], prototypes[3]);
            for (name, native) in [
                ("items", DragNative::Items),
                ("files", DragNative::Files),
                ("types", DragNative::Types),
                ("dropEffect", DragNative::DropEffect),
                ("effectAllowed", DragNative::EffectAllowed),
            ] {
                self.drag_getter(transfer, function, name, native)?;
            }
            self.drag_effect_setter(transfer, function, "dropEffect", false)?;
            self.drag_effect_setter(transfer, function, "effectAllowed", true)?;
            for (name, length, native) in [
                ("getData", 1, DragNative::GetData),
                ("setData", 2, DragNative::SetData),
                ("clearData", 0, DragNative::ClearData),
            ] {
                self.install_native(
                    transfer,
                    function,
                    name,
                    length,
                    NativeFunction::WebDrag(native),
                )?;
            }
            self.drag_getter(list, function, "length", DragNative::ItemsLength)?;
            for (name, length, native) in [
                ("add", 1, DragNative::ItemsAdd),
                ("remove", 1, DragNative::ItemsRemove),
                ("clear", 0, DragNative::ItemsClear),
            ] {
                self.install_native(
                    list,
                    function,
                    name,
                    length,
                    NativeFunction::WebDrag(native),
                )?;
            }
            self.drag_getter(item, function, "kind", DragNative::ItemKind)?;
            self.drag_getter(item, function, "type", DragNative::ItemType)?;
            self.install_native(
                item,
                function,
                "getAsFile",
                0,
                NativeFunction::WebDrag(DragNative::GetAsFile),
            )?;
            self.install_native(
                item,
                function,
                "getAsString",
                1,
                NativeFunction::WebDrag(DragNative::GetAsString),
            )?;
            self.drag_getter(event, function, "dataTransfer", DragNative::EventTransfer)?;
            let mut constructors = Vec::new();
            for (prototype, name, native, length) in [
                (transfer, "DataTransfer", DragNative::DataTransfer, 0),
                (
                    list,
                    "DataTransferItemList",
                    DragNative::IllegalConstructor,
                    0,
                ),
                (item, "DataTransferItem", DragNative::IllegalConstructor, 0),
                (event, "DragEvent", DragNative::DragEvent, 1),
            ] {
                self.define_data(
                    prototype,
                    JsSymbol::well_known("toStringTag"),
                    Value::String(name.into()),
                    false,
                    false,
                    true,
                )?;
                let constructor = self.web_file_constructor(
                    name,
                    length,
                    NativeFunction::WebDrag(native),
                    function,
                    prototype,
                )?;
                self.stack.push(Value::Object(constructor));
                if name == "DragEvent" {
                    let parent = self
                        .heap
                        .get(self.globals["%WebMouseEvent%"], "constructor")?
                        .object_id()
                        .unwrap();
                    self.heap.set_prototype(constructor, Some(parent))?;
                }
                constructors.push((name, constructor, prototype));
            }
            for (name, constructor, prototype) in constructors {
                self.define_data(global, name, Value::Object(constructor), true, false, true)?;
                self.heap.root(prototype)?;
                self.globals.insert(format!("%Web{name}%"), prototype);
            }
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    fn drag_getter(
        &mut self,
        owner: ObjectId,
        function: ObjectId,
        name: &str,
        native: DragNative,
    ) -> Result<(), RuntimeError> {
        self.install_native_getter(owner, function, name, NativeFunction::WebDrag(native))?;
        self.with_roots(|heap| {
            heap.define_own_property(
                owner,
                name,
                PropertyDescriptor {
                    enumerable: Some(true),
                    ..Default::default()
                },
            )
        })?;
        Ok(())
    }

    fn drag_effect_setter(
        &mut self,
        owner: ObjectId,
        function: ObjectId,
        name: &str,
        allowed: bool,
    ) -> Result<(), RuntimeError> {
        let setter_name = format!("set {name}");
        let setter = self.with_roots(|heap| {
            heap.alloc_native_function(
                NativeFunction::WebDrag(DragNative::SetEffect(allowed)),
                &setter_name,
                function,
            )
        })?;
        self.stack.push(Value::Object(setter));
        let result = (|| {
            self.define_data(
                setter,
                "name",
                Value::String(setter_name.into()),
                false,
                false,
                true,
            )?;
            self.define_data(setter, "length", Value::Number(1.0), false, false, true)?;
            self.with_roots(|heap| {
                heap.define_own_property(
                    owner,
                    name,
                    PropertyDescriptor {
                        set: Some(Value::Object(setter)),
                        ..Default::default()
                    },
                )
            })?;
            Ok(())
        })();
        self.stack.pop();
        result
    }

    fn drag_receiver(&self, receiver: &Value) -> Result<ObjectId, RuntimeError> {
        receiver
            .object_id()
            .filter(|id| self.heap.drag_store(*id).is_ok())
            .ok_or_else(|| RuntimeError::TypeError("Invalid DataTransfer receiver".into()))
    }

    fn drag_items_receiver(&self, receiver: &Value) -> Result<ObjectId, RuntimeError> {
        receiver
            .object_id()
            .and_then(|id| self.heap.drag_items_store(id).ok())
            .ok_or_else(|| RuntimeError::TypeError("Invalid DataTransferItemList receiver".into()))
    }

    fn drag_item_receiver(&self, receiver: &Value) -> Result<(ObjectId, u64), RuntimeError> {
        receiver
            .object_id()
            .and_then(|id| self.heap.drag_item_identity(id).ok())
            .ok_or_else(|| RuntimeError::TypeError("Invalid DataTransferItem receiver".into()))
    }

    pub(super) fn web_drag_call(
        &mut self,
        native: DragNative,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        match native {
            DragNative::DataTransfer => {
                if !construct {
                    return Err(RuntimeError::TypeError("DataTransfer requires new".into()));
                }
                let default = self.globals["%WebDataTransfer%"];
                let prototype = self.constructor_prototype_for(default, None)?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_drag_store(DragStore::new(DragStoreMode::ReadWrite), prototype)
                })?))
            }
            DragNative::DragEvent => self.construct_web_event(3, args, construct),
            DragNative::IllegalConstructor => Err(RuntimeError::TypeError(
                "Illegal drag item constructor".into(),
            )),
            DragNative::Items | DragNative::Files | DragNative::Types => {
                let id = self.drag_receiver(receiver)?;
                self.drag_collection(id, native)
            }
            DragNative::DropEffect | DragNative::EffectAllowed => {
                let id = self.drag_receiver(receiver)?;
                let store = self.heap.drag_store(id)?;
                Ok(Value::String(if native == DragNative::DropEffect {
                    store.drop_effect.clone()
                } else {
                    store.effect_allowed.clone()
                }))
            }
            DragNative::SetEffect(allowed) => {
                let id = self.drag_receiver(receiver)?;
                let effect = self.coerce_string(native::argument(args, 0))?;
                let admitted = if allowed {
                    [
                        "none",
                        "copy",
                        "copyLink",
                        "copyMove",
                        "link",
                        "linkMove",
                        "move",
                        "all",
                        "uninitialized",
                    ]
                    .iter()
                    .any(|value| effect == *value)
                } else {
                    ["none", "copy", "link", "move"]
                        .iter()
                        .any(|value| effect == *value)
                };
                let mut store = self.heap.drag_store(id)?.clone();
                if admitted && (!allowed || store.mode == DragStoreMode::ReadWrite) {
                    if allowed {
                        store.effect_allowed = effect;
                    } else {
                        store.drop_effect = effect;
                    }
                    self.with_roots(|heap| heap.replace_drag_store(id, store))?;
                }
                Ok(Value::Undefined)
            }
            DragNative::GetData | DragNative::SetData | DragNative::ClearData => {
                self.drag_text_call(native, receiver, args)
            }
            DragNative::ItemsLength => {
                let id = self.drag_items_receiver(receiver)?;
                let store = self.heap.drag_store(id)?;
                Ok(Value::Number(if store.mode == DragStoreMode::Disabled {
                    0.0
                } else {
                    store.items.len() as f64
                }))
            }
            DragNative::ItemsAdd | DragNative::ItemsRemove | DragNative::ItemsClear => {
                self.drag_items_call(native, receiver, args)
            }
            DragNative::ItemKind
            | DragNative::ItemType
            | DragNative::GetAsFile
            | DragNative::GetAsString => self.drag_item_call(native, receiver, args),
            DragNative::EventTransfer => {
                let id = receiver
                    .object_id()
                    .filter(|id| {
                        self.heap
                            .drag_event(*id)
                            .is_ok_and(|state| state.interface == 3)
                    })
                    .ok_or_else(|| RuntimeError::TypeError("Invalid DragEvent receiver".into()))?;
                Ok(self
                    .heap
                    .drag_event(id)?
                    .transfer
                    .map_or(Value::Null, Value::Object))
            }
        }
    }

    fn drag_collection(&mut self, id: ObjectId, native: DragNative) -> Result<Value, RuntimeError> {
        let store = self.heap.drag_store(id)?;
        let cached = match native {
            DragNative::Items => store.item_list,
            DragNative::Files => store.file_list,
            _ => store.types,
        };
        if let Some(cached) = cached {
            return Ok(Value::Object(cached));
        }
        let object = match native {
            DragNative::Items => {
                let prototype = self.globals["%WebDataTransferItemList%"];
                self.with_roots(|heap| heap.alloc_drag_items(id, prototype))?
            }
            DragNative::Files => {
                let prototype = self.globals["%WebFileList%"];
                self.with_roots(|heap| heap.alloc_drag_files(id, prototype))?
            }
            _ => {
                let mut formats = Vec::new();
                if store.mode != DragStoreMode::Disabled {
                    formats.extend(store.items.iter().filter_map(|item| match item.data {
                        DragItemData::Text(_) => Some(item.format.clone()),
                        _ => None,
                    }));
                    if store.items.iter().any(|item| {
                        matches!(item.data, DragItemData::File(_) | DragItemData::FileMarker)
                    }) {
                        formats.push("Files".into());
                    }
                }
                let prototype = self.array_prototype;
                let array = self
                    .with_roots(|heap| heap.alloc_array(formats.len() as u32, Some(prototype)))?;
                self.stack.push(Value::Object(array));
                let result: Result<ObjectId, RuntimeError> = (|| {
                    for (index, format) in formats.into_iter().enumerate() {
                        self.define_data(
                            array,
                            index.to_string(),
                            Value::String(format),
                            false,
                            true,
                            false,
                        )?;
                    }
                    self.with_roots(|heap| {
                        heap.define_own_property(
                            array,
                            "length",
                            PropertyDescriptor {
                                writable: Some(false),
                                ..Default::default()
                            },
                        )
                    })?;
                    self.heap.prevent_extensions(array)?;
                    Ok(array)
                })();
                self.stack.pop();
                result?
            }
        };
        self.stack.push(Value::Object(object));
        let mut store = self.heap.drag_store(id)?.clone();
        match native {
            DragNative::Items => store.item_list = Some(object),
            DragNative::Files => store.file_list = Some(object),
            _ => store.types = Some(object),
        }
        let result = self.with_roots(|heap| heap.replace_drag_store(id, store));
        self.stack.pop();
        result?;
        Ok(Value::Object(object))
    }

    pub(super) fn append_drag_item(
        &mut self,
        id: ObjectId,
        format: JsString,
        data: DragItemData,
    ) -> Result<Value, RuntimeError> {
        let mut store = self.heap.drag_store(id)?.clone();
        if store.items.len() >= MAX_DRAG_ITEMS {
            return Err(RuntimeError::RangeError("Drag item limit exceeded".into()));
        }
        let item_id = store
            .next_id
            .checked_add(1)
            .ok_or_else(|| RuntimeError::RangeError("Drag item ID exhausted".into()))?;
        let prototype = self.globals["%WebDataTransferItem%"];
        // A newly copied File must survive the item wrapper's allocation.
        let base = self.stack.len();
        if let DragItemData::File(file) = data {
            self.stack.push(Value::Object(file));
        }
        let result = (|| {
            let wrapper = self.with_roots(|heap| heap.alloc_drag_item(id, item_id, prototype))?;
            self.stack.push(Value::Object(wrapper));
            store.next_id = item_id;
            store.items.push(DragItem {
                id: item_id,
                format,
                data,
                wrapper: Some(wrapper),
            });
            store.types = None;
            self.with_roots(|heap| heap.replace_drag_store(id, store))?;
            Ok(Value::Object(wrapper))
        })();
        self.stack.truncate(base);
        result
    }

    fn drag_text_call(
        &mut self,
        native: DragNative,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let id = self.drag_receiver(receiver)?;
        if native != DragNative::ClearData
            && args.len() < if native == DragNative::SetData { 2 } else { 1 }
        {
            return Err(RuntimeError::TypeError(
                "Drag data method requires its arguments".into(),
            ));
        }
        let (format, url) = if native == DragNative::ClearData && args.is_empty() {
            (None, false)
        } else {
            let (format, url) = drag_format(self.coerce_string(native::argument(args, 0))?);
            (Some(format), url)
        };
        let data = if native == DragNative::SetData {
            Some(self.coerce_string(&args[1])?)
        } else {
            None
        };
        let mut store = self.heap.drag_store(id)?.clone();
        if native == DragNative::GetData {
            let text = if matches!(
                store.mode,
                DragStoreMode::Protected | DragStoreMode::Disabled
            ) {
                JsString::default()
            } else {
                store
                    .items
                    .iter()
                    .find_map(|item| match &item.data {
                        DragItemData::Text(text) if Some(&item.format) == format.as_ref() => {
                            Some(text.clone())
                        }
                        _ => None,
                    })
                    .unwrap_or_default()
            };
            return Ok(Value::String(if url {
                first_drag_url(&text)
            } else {
                text
            }));
        }
        if store.mode != DragStoreMode::ReadWrite {
            return Ok(Value::Undefined);
        }
        if let Some(data) = &data {
            self.check_string(&Value::String(data.clone()))?;
        }
        // Replacements are removed and appended, as specified by HTML.
        store.items.retain(|item| {
            !matches!(item.data, DragItemData::Text(_))
                || format.as_ref().is_some_and(|format| item.format != *format)
        });
        store.types = None;
        self.with_roots(|heap| heap.replace_drag_store(id, store))?;
        if let Some(data) = data {
            self.append_drag_item(id, format.unwrap(), DragItemData::Text(data))?;
        }
        Ok(Value::Undefined)
    }

    fn drag_items_call(
        &mut self,
        native: DragNative,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let id = self.drag_items_receiver(receiver)?;
        match native {
            DragNative::ItemsAdd => {
                if args.is_empty() {
                    return Err(RuntimeError::TypeError(
                        "Drag items.add requires data".into(),
                    ));
                }
                let file = args[0].object_id().filter(|id| {
                    self.heap
                        .web_blob(*id)
                        .is_ok_and(|blob| blob.file.is_some())
                });
                let (format, data) = if let Some(file) = file {
                    let blob = self.heap.web_blob(file)?.clone();
                    let format = blob.media_type.clone();
                    let prototype = self.globals["%WebFile%"];
                    let copy = self.with_roots(|heap| heap.alloc_web_blob(blob, prototype))?;
                    (format, DragItemData::File(copy))
                } else {
                    if args.len() < 2 {
                        return Err(RuntimeError::TypeError(
                            "String drag items require a type".into(),
                        ));
                    }
                    let text = self.coerce_string(&args[0])?;
                    self.check_string(&Value::String(text.clone()))?;
                    // Unlike setData, item-list types have no legacy alias.
                    (
                        ascii_lowercase(self.coerce_string(&args[1])?),
                        DragItemData::Text(text),
                    )
                };
                let store = self.heap.drag_store(id)?;
                if store.mode != DragStoreMode::ReadWrite {
                    return Ok(Value::Null);
                }
                if matches!(data, DragItemData::Text(_))
                    && store.items.iter().any(|item| {
                        item.format == format && matches!(item.data, DragItemData::Text(_))
                    })
                {
                    return self.drag_duplicate_error();
                }
                self.append_drag_item(id, format, data)
            }
            DragNative::ItemsRemove => {
                if args.is_empty() {
                    return Err(RuntimeError::TypeError(
                        "Drag items.remove requires an index".into(),
                    ));
                }
                let number = self.coerce_number(&args[0])?;
                let index = if number.is_finite() {
                    number.trunc().rem_euclid(4_294_967_296.0) as usize
                } else {
                    0
                };
                let mut store = self.heap.drag_store(id)?.clone();
                if store.mode != DragStoreMode::ReadWrite {
                    return self.drag_invalid_state_error();
                }
                if index < store.items.len() {
                    store.items.remove(index);
                    store.types = None;
                    self.with_roots(|heap| heap.replace_drag_store(id, store))?;
                }
                Ok(Value::Undefined)
            }
            _ => {
                let mut store = self.heap.drag_store(id)?.clone();
                if store.mode == DragStoreMode::ReadWrite {
                    store.items.clear();
                    store.types = None;
                    self.with_roots(|heap| heap.replace_drag_store(id, store))?;
                }
                Ok(Value::Undefined)
            }
        }
    }

    fn drag_item_call(
        &mut self,
        native: DragNative,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let (id, item_id) = self.drag_item_receiver(receiver)?;
        let store = self.heap.drag_store(id)?;
        let item = if store.mode == DragStoreMode::Disabled {
            None
        } else {
            store.items.iter().find(|item| item.id == item_id)
        };
        match native {
            DragNative::ItemKind => Ok(Value::String(
                item.map_or("", |item| match item.data {
                    DragItemData::Text(_) => "string",
                    DragItemData::File(_) | DragItemData::FileMarker => "file",
                })
                .into(),
            )),
            DragNative::ItemType => {
                Ok(Value::String(item.map_or_else(JsString::default, |item| {
                    item.format.clone()
                })))
            }
            DragNative::GetAsFile => {
                if store.mode == DragStoreMode::Protected {
                    return Ok(Value::Null);
                }
                let Some(DragItem {
                    data: DragItemData::File(file),
                    ..
                }) = item
                else {
                    return Ok(Value::Null);
                };
                let blob = self.heap.web_blob(*file)?.clone();
                let prototype = self.globals["%WebFile%"];
                Ok(Value::Object(
                    self.with_roots(|heap| heap.alloc_web_blob(blob, prototype))?,
                ))
            }
            _ => {
                if args.is_empty() {
                    return Err(RuntimeError::TypeError(
                        "getAsString requires a callback".into(),
                    ));
                }
                let text = if store.mode == DragStoreMode::Protected {
                    None
                } else {
                    item.and_then(|item| match &item.data {
                        DragItemData::Text(text) => Some(text.clone()),
                        _ => None,
                    })
                };
                if args[0] != Value::Null
                    && args[0] != Value::Undefined
                    && !self.is_callable(&args[0])?
                {
                    return Err(RuntimeError::TypeError(
                        "Drag string callback must be callable".into(),
                    ));
                }
                if let Some(text) = text {
                    if let Some(callback) = args[0].object_id() {
                        self.queue_drag_string_task(callback, text)?;
                    }
                }
                Ok(Value::Undefined)
            }
        }
    }

    fn drag_duplicate_error(&mut self) -> Result<Value, RuntimeError> {
        self.drag_named_error(
            "NotSupportedError",
            "A string item with this type already exists",
        )
    }

    fn drag_invalid_state_error(&mut self) -> Result<Value, RuntimeError> {
        self.drag_named_error("InvalidStateError", "The drag data store is not writable")
    }

    fn drag_named_error(&mut self, name: &str, message: &str) -> Result<Value, RuntimeError> {
        let prototype = self.globals["%WebDOMException%"];
        let error =
            Value::Object(self.with_roots(|heap| {
                heap.alloc_dom_exception(name.into(), message.into(), prototype)
            })?);
        Err(RuntimeError::Thrown(error))
    }

    fn queue_drag_string_task(
        &mut self,
        callback: ObjectId,
        text: JsString,
    ) -> Result<(), RuntimeError> {
        let bytes = self
            .drag_string_tasks
            .iter()
            .fold(text.byte_len(), |total, (_, text, _)| {
                total.saturating_add(text.byte_len())
            });
        if self.drag_string_tasks.len() >= 64 || bytes > 1_048_576 {
            return Err(RuntimeError::RangeError(
                "Drag string task queue limit exceeded".into(),
            ));
        }
        let root = self.heap.root(callback)?;
        self.drag_string_tasks.push_back((callback, text, root));
        Ok(())
    }

    /// Runs queued drag string tasks after the enclosing event/task has
    /// returned. This queue is distinct from the Promise microtask queue.
    pub fn run_drag_tasks_bounded(&mut self, max_tasks: usize) -> Result<(), RuntimeError> {
        self.ensure_no_debugger_continuation()?;
        for _ in 0..max_tasks {
            let Some((callback, text, root)) = self.drag_string_tasks.pop_front() else {
                return Ok(());
            };
            self.remaining_instructions = self.config.instruction_budget;
            let result = self.call_native(
                Value::Object(callback),
                Value::Undefined,
                vec![Value::String(text)],
                false,
            );
            self.heap.unroot(root)?;
            if let Err(error) = result {
                if !drag_page_exception(&error) {
                    return Err(error);
                }
            }
            self.run_promise_jobs_bounded(1024)?;
        }
        if self.drag_string_tasks.is_empty() {
            Ok(())
        } else {
            Err(RuntimeError::InstructionLimit)
        }
    }
}

pub(super) fn drag_page_exception(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::Thrown(_)
            | RuntimeError::ReferenceError(_)
            | RuntimeError::TypeError(_)
            | RuntimeError::RangeError(_)
            | RuntimeError::SyntaxError(_)
    )
}

fn ascii_lowercase(string: JsString) -> JsString {
    JsString::from_code_units(
        string
            .as_code_units()
            .iter()
            .map(|unit| {
                if (0x41..=0x5a).contains(unit) {
                    unit + 0x20
                } else {
                    *unit
                }
            })
            .collect(),
    )
}

fn drag_format(string: JsString) -> (JsString, bool) {
    let string = ascii_lowercase(string);
    if string == "text" {
        ("text/plain".into(), false)
    } else if string == "url" {
        ("text/uri-list".into(), true)
    } else {
        (string, false)
    }
}

fn first_drag_url(text: &JsString) -> JsString {
    let units = text.as_code_units();
    units
        .split(|unit| *unit == 10 || *unit == 13)
        .find(|line| !line.is_empty() && line[0] != 35)
        .map_or_else(JsString::default, |line| {
            JsString::from_code_units(line.to_vec())
        })
}
