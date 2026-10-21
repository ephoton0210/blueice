// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Browser drag values use heap brands and owned data. An OS drag store is
//! separate from a page-constructed, writable DataTransfer.

use super::*;
use crate::heap::{DragItem, DragItemData, DragStore, DragStoreMode};
use crate::native::DragNative;
use crate::vm::error_input_storage::utf16_from_utf8;
use crate::vm::native_value_storage::NativeValue;
use crate::vm::realm_call_storage::RealmCallRoots;
use crate::vm::web_event_storage::EventContext;
pub(super) mod storage;
use storage::{NativeDragFile, NativeDragStore};

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/drag_store_storage.rs"]
mod storage_tests;

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
                ("setDragImage", 3, DragNative::SetDragImage),
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
        let context = EventContext::capture(self)?;
        let ids = receiver
            .object_id()
            .into_iter()
            .chain(args.iter().filter_map(Value::object_id));
        let roots = context.prepare_roots(self, ids, 3)?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            vm.web_drag_input(native, receiver, args, construct, &context, &roots)
        });
        context.validate().and(result)
    }

    fn drag_string_input(
        &mut self,
        value: &Value,
        context: &EventContext,
    ) -> Result<NativeValue, RuntimeError> {
        if matches!(value, Value::String(_)) {
            return NativeValue::copied(value, context.owner.as_ref());
        }
        let converted = self.coerce_string(value);
        context.validate()?;
        NativeValue::adopt(Value::String(converted?), context.owner.as_ref())
    }

    fn web_drag_input(
        &mut self,
        native: DragNative,
        receiver: &Value,
        args: &[Value],
        construct: bool,
        context: &EventContext,
        roots: &RealmCallRoots,
    ) -> Result<Value, RuntimeError> {
        match native {
            DragNative::DataTransfer => {
                if !construct {
                    return Err(RuntimeError::TypeError("DataTransfer requires new".into()));
                }
                let default = self.globals["%WebDataTransfer%"];
                let prototype = self.constructor_prototype_for(default, None);
                context.validate()?;
                let prototype = prototype?;
                Ok(Value::Object(
                    NativeDragStore::new(DragStoreMode::ReadWrite, context.owner.as_ref())?
                        .allocate(self, prototype)?,
                ))
            }
            DragNative::DragEvent => self.construct_web_event(3, args, construct),
            DragNative::IllegalConstructor => Err(RuntimeError::TypeError(
                "Illegal drag item constructor".into(),
            )),
            DragNative::SetDragImage => {
                let id = self.drag_receiver(receiver)?;
                let element = native::argument(args, 0)
                    .object_id()
                    .filter(|object| self.is_host_element(*object))
                    .ok_or_else(|| {
                        RuntimeError::TypeError("setDragImage requires an owned DOM element".into())
                    });
                context.validate()?;
                let element = element?;
                let x = self.coerce_number(native::argument(args, 1));
                context.validate()?;
                let x = primitive::to_int32(x?);
                let y = self.coerce_number(native::argument(args, 2));
                context.validate()?;
                let y = primitive::to_int32(y?);
                if self.heap.drag_store(id)?.mode == DragStoreMode::ReadWrite {
                    let mut store = NativeDragStore::copied(self, id, context.owner.as_ref())?;
                    store.state.image = Some((element, x, y));
                    store.into_heap(self, id)?;
                }
                Ok(Value::Undefined)
            }
            DragNative::Items | DragNative::Files | DragNative::Types => {
                let id = self.drag_receiver(receiver)?;
                self.drag_collection(id, native, context, roots)
            }
            DragNative::DropEffect | DragNative::EffectAllowed => {
                let id = self.drag_receiver(receiver)?;
                let store = self.heap.drag_store(id)?;
                Ok(storage::copy_string(
                    if native == DragNative::DropEffect {
                        &store.drop_effect
                    } else {
                        &store.effect_allowed
                    },
                    context.owner.as_ref(),
                )?
                .into_value())
            }
            DragNative::SetEffect(allowed) => {
                let id = self.drag_receiver(receiver)?;
                let effect = self.drag_string_input(native::argument(args, 0), context)?;
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
                    .any(|v| storage::text(&effect) == *v)
                } else {
                    ["none", "copy", "link", "move"]
                        .iter()
                        .any(|v| storage::text(&effect) == *v)
                };
                if admitted
                    && (!allowed || self.heap.drag_store(id)?.mode == DragStoreMode::ReadWrite)
                {
                    let mut store = NativeDragStore::copied(self, id, context.owner.as_ref())?;
                    store.effect(allowed, effect)?;
                    store.into_heap(self, id)?;
                }
                Ok(Value::Undefined)
            }
            DragNative::GetData | DragNative::SetData | DragNative::ClearData => {
                self.drag_text_call(native, receiver, args, context, roots)
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
                self.drag_items_call(native, receiver, args, context, roots)
            }
            DragNative::ItemKind
            | DragNative::ItemType
            | DragNative::GetAsFile
            | DragNative::GetAsString => self.drag_item_call(native, receiver, args, context),
            DragNative::EventTransfer => {
                let id = receiver
                    .object_id()
                    .filter(|id| self.heap.drag_event(*id).is_ok_and(|s| s.interface == 3))
                    .ok_or_else(|| RuntimeError::TypeError("Invalid DragEvent receiver".into()))?;
                Ok(self
                    .heap
                    .drag_event(id)?
                    .transfer
                    .map_or(Value::Null, Value::Object))
            }
        }
    }

    fn drag_collection(
        &mut self,
        id: ObjectId,
        native: DragNative,
        context: &EventContext,
        roots: &RealmCallRoots,
    ) -> Result<Value, RuntimeError> {
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
                let live = store.mode != DragStoreMode::Disabled;
                let count = if live {
                    store
                        .items
                        .iter()
                        .filter(|item| matches!(item.data, DragItemData::Text(_)))
                        .count()
                } else {
                    0
                };
                let files = live
                    && store.items.iter().any(|item| {
                        matches!(item.data, DragItemData::File(_) | DragItemData::FileMarker)
                    });
                let prototype = self.array_prototype;
                let array = self.with_roots(|heap| {
                    heap.alloc_array((count + usize::from(files)) as u32, Some(prototype))
                })?;
                roots.push(&self.heap, array)?;
                for index in 0..count + usize::from(files) {
                    let value = if index < count {
                        let item = self
                            .heap
                            .drag_store(id)?
                            .items
                            .iter()
                            .filter(|item| matches!(item.data, DragItemData::Text(_)))
                            .nth(index)
                            .expect("immutable drag formats");
                        storage::copy_string(&item.format, context.owner.as_ref())?
                    } else {
                        utf16_from_utf8("Files", context.owner.as_ref())?
                    };
                    let (value, credit) = value.into_retained_parts();
                    property_definition_storage::PropertyDefinitionInput::from_retained(
                        index.to_string().into(),
                        PropertyDescriptor::data(value, false, true, false),
                        credit,
                    )
                    .define(self, array)?;
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
                array
            }
        };
        roots.push(&self.heap, object)?;
        let mut store = NativeDragStore::copied(self, id, context.owner.as_ref())?;
        match native {
            DragNative::Items => store.state.item_list = Some(object),
            DragNative::Files => store.state.file_list = Some(object),
            _ => store.state.types = Some(object),
        }
        store.into_heap(self, id)?;
        Ok(Value::Object(object))
    }

    pub(super) fn append_drag_item(
        &mut self,
        id: ObjectId,
        format: NativeValue,
        data: NativeValue,
    ) -> Result<Value, RuntimeError> {
        let context = EventContext::capture(self)?;
        let format = format.retain_for_owner(context.owner.as_ref())?;
        let data = data.retain_for_owner(context.owner.as_ref())?;
        let ids = std::iter::once(id).chain(data.value().object_id());
        let roots = context.prepare_roots(self, ids, 1)?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            let store = NativeDragStore::copied(vm, id, context.owner.as_ref())?;
            vm.append_prepared_drag_item(id, store, format, data, &roots)
        });
        context.validate().and(result)
    }

    fn append_prepared_drag_item(
        &mut self,
        id: ObjectId,
        mut store: NativeDragStore,
        format: NativeValue,
        data: NativeValue,
        roots: &RealmCallRoots,
    ) -> Result<Value, RuntimeError> {
        if store.state.items.len() >= MAX_DRAG_ITEMS {
            return Err(RuntimeError::RangeError("Drag item limit exceeded".into()));
        }
        let item_id = store
            .state
            .next_id
            .checked_add(1)
            .ok_or_else(|| RuntimeError::RangeError("Drag item ID exhausted".into()))?;
        let prototype = self.globals["%WebDataTransferItem%"];
        let wrapper = self.with_roots(|heap| heap.alloc_drag_item(id, item_id, prototype))?;
        roots.push(&self.heap, wrapper)?;
        store.item(item_id, format, data, wrapper)?;
        store.into_heap(self, id)?;
        Ok(Value::Object(wrapper))
    }

    fn drag_text_call(
        &mut self,
        native: DragNative,
        receiver: &Value,
        args: &[Value],
        context: &EventContext,
        roots: &RealmCallRoots,
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
            let value = self.drag_string_input(native::argument(args, 0), context)?;
            let (format, url) = drag_format(value)?;
            (Some(format), url)
        };
        let data = if native == DragNative::SetData {
            Some(self.drag_string_input(&args[1], context)?)
        } else {
            None
        };
        let source = self.heap.drag_store(id)?;
        if native == DragNative::GetData {
            let selected = if matches!(
                source.mode,
                DragStoreMode::Protected | DragStoreMode::Disabled
            ) {
                None
            } else {
                source.items.iter().find_map(|item| match &item.data {
                    DragItemData::Text(text)
                        if format
                            .as_ref()
                            .is_some_and(|format| &item.format == storage::text(format)) =>
                    {
                        Some(text)
                    }
                    _ => None,
                })
            };
            let result = if let Some(text) = selected {
                if url {
                    first_drag_url(text, context.owner.as_ref())?
                } else {
                    storage::copy_string(text, context.owner.as_ref())?
                }
            } else {
                utf16_from_utf8("", context.owner.as_ref())?
            };
            return Ok(result.into_value());
        }
        if source.mode != DragStoreMode::ReadWrite {
            return Ok(Value::Undefined);
        }
        if let Some(data) = &data {
            self.check_string(data.value())?;
        }
        let mut store = NativeDragStore::copied(self, id, context.owner.as_ref())?;
        store.state.items.retain(|item| {
            !matches!(item.data, DragItemData::Text(_))
                || format
                    .as_ref()
                    .is_some_and(|format| &item.format != storage::text(format))
        });
        store.state.types = None;
        store.reconcile_removed();
        // Admit the complete replacement before publishing the removal.
        if let Some(data) = data {
            self.append_prepared_drag_item(id, store, format.unwrap(), data, roots)?;
        } else {
            store.into_heap(self, id)?;
        }
        Ok(Value::Undefined)
    }

    fn drag_items_call(
        &mut self,
        native: DragNative,
        receiver: &Value,
        args: &[Value],
        context: &EventContext,
        roots: &RealmCallRoots,
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
                    let file = NativeDragFile::copied(self, file, context.owner.as_ref())?;
                    let format =
                        storage::copy_string(&file.data.media_type, context.owner.as_ref())?;
                    let prototype = self.globals["%WebFile%"];
                    let copy = file.allocate(self, prototype)?;
                    roots.push(&self.heap, copy)?;
                    (
                        format,
                        NativeValue::adopt(Value::Object(copy), context.owner.as_ref())?,
                    )
                } else {
                    if args.len() < 2 {
                        return Err(RuntimeError::TypeError(
                            "String drag items require a type".into(),
                        ));
                    }
                    let text = self.drag_string_input(&args[0], context)?;
                    self.check_string(text.value())?;
                    let format = storage::lowercase(self.drag_string_input(&args[1], context)?);
                    (format, text)
                };
                let source = self.heap.drag_store(id)?;
                if source.mode != DragStoreMode::ReadWrite {
                    return Ok(Value::Null);
                }
                if matches!(data.value(), Value::String(_))
                    && source.items.iter().any(|item| {
                        &item.format == storage::text(&format)
                            && matches!(item.data, DragItemData::Text(_))
                    })
                {
                    return self.drag_duplicate_error();
                }
                let store = NativeDragStore::copied(self, id, context.owner.as_ref())?;
                self.append_prepared_drag_item(id, store, format, data, roots)
            }
            DragNative::ItemsRemove => {
                if args.is_empty() {
                    return Err(RuntimeError::TypeError(
                        "Drag items.remove requires an index".into(),
                    ));
                }
                let number = self.coerce_number(&args[0]);
                context.validate()?;
                let number = number?;
                let index = if number.is_finite() {
                    number.trunc().rem_euclid(4_294_967_296.0) as usize
                } else {
                    0
                };
                let source = self.heap.drag_store(id)?;
                if source.mode != DragStoreMode::ReadWrite {
                    return self.drag_invalid_state_error();
                }
                if index < source.items.len() {
                    let mut store = NativeDragStore::copied(self, id, context.owner.as_ref())?;
                    store.state.items.remove(index);
                    store.state.types = None;
                    store.reconcile_removed();
                    store.into_heap(self, id)?;
                }
                Ok(Value::Undefined)
            }
            _ => {
                if self.heap.drag_store(id)?.mode == DragStoreMode::ReadWrite {
                    let mut store = NativeDragStore::copied(self, id, context.owner.as_ref())?;
                    store.state.items.clear();
                    store.state.types = None;
                    store.reconcile_removed();
                    store.into_heap(self, id)?;
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
        context: &EventContext,
    ) -> Result<Value, RuntimeError> {
        let (id, item_id) = self.drag_item_receiver(receiver)?;
        let store = self.heap.drag_store(id)?;
        let item = if store.mode == DragStoreMode::Disabled {
            None
        } else {
            store.items.iter().find(|item| item.id == item_id)
        };
        match native {
            DragNative::ItemKind => Ok(utf16_from_utf8(
                item.map_or("", |item| match item.data {
                    DragItemData::Text(_) => "string",
                    _ => "file",
                }),
                context.owner.as_ref(),
            )?
            .into_value()),
            DragNative::ItemType => Ok(if let Some(item) = item {
                storage::copy_string(&item.format, context.owner.as_ref())?
            } else {
                utf16_from_utf8("", context.owner.as_ref())?
            }
            .into_value()),
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
                let file = NativeDragFile::copied(self, *file, context.owner.as_ref())?;
                let prototype = self.globals["%WebFile%"];
                Ok(Value::Object(file.allocate(self, prototype)?))
            }
            _ => {
                if args.is_empty() {
                    return Err(RuntimeError::TypeError(
                        "getAsString requires a callback".into(),
                    ));
                }
                if args[0] != Value::Null
                    && args[0] != Value::Undefined
                    && !self.is_callable(&args[0])?
                {
                    return Err(RuntimeError::TypeError(
                        "Drag string callback must be callable".into(),
                    ));
                }
                if store.mode != DragStoreMode::Protected {
                    if let (
                        Some(DragItem {
                            data: DragItemData::Text(text),
                            ..
                        }),
                        Some(callback),
                    ) = (item, args[0].object_id())
                    {
                        let text = storage::copy_string(text, context.owner.as_ref())?;
                        self.queue_drag_string_task_owned(callback, text)?;
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
    pub(super) fn drag_named_error(
        &mut self,
        name: &str,
        message: &str,
    ) -> Result<Value, RuntimeError> {
        let context = EventContext::capture(self)?;
        let name = utf16_from_utf8(name, context.owner.as_ref())?;
        let message = utf16_from_utf8(message, context.owner.as_ref())?;
        let (Value::String(name), mut name_credit) = name.into_retained_parts() else {
            unreachable!()
        };
        let (Value::String(message), mut message_credit) = message.into_retained_parts() else {
            unreachable!()
        };
        let prototype = self.globals["%WebDOMException%"];
        let error = self.with_roots(|heap| {
            for credit in [&mut name_credit, &mut message_credit]
                .into_iter()
                .flatten()
            {
                credit.release_to(0);
            }
            heap.alloc_dom_exception(name, message, prototype)
        });
        context.validate()?;
        Err(RuntimeError::Thrown(Value::Object(error?)))
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

fn drag_format(value: NativeValue) -> Result<(NativeValue, bool), RuntimeError> {
    let value = storage::lowercase(value);
    if storage::text(&value) == "text" {
        Ok((
            utf16_from_utf8("text/plain", value.owner_view()?.as_ref())?,
            false,
        ))
    } else if storage::text(&value) == "url" {
        Ok((
            utf16_from_utf8("text/uri-list", value.owner_view()?.as_ref())?,
            true,
        ))
    } else {
        Ok((value, false))
    }
}
fn first_drag_url(
    text: &JsString,
    owner: Option<&host_scheduling::SharedSchedulingCharge>,
) -> Result<NativeValue, RuntimeError> {
    let line = text
        .as_code_units()
        .split(|unit| *unit == 10 || *unit == 13)
        .find(|line| !line.is_empty() && line[0] != 35)
        .unwrap_or_default();
    let mut storage = owner
        .map(|owner| owner.reserve_peer(line.len().saturating_mul(2)))
        .transpose()?;
    let value = JsString::from_code_units(line.to_vec());
    if let Some(storage) = &mut storage {
        storage.resize(value.owned_heap_payload_bytes().unwrap_or(usize::MAX))?;
    }
    Ok(NativeValue::from_retained_parts(
        Value::String(value),
        storage,
    ))
}
