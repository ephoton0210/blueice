// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-only drag dispatch. Page constructors never mint trusted events or
//! OS payload access; the embedding owner supplies a validated ancestry.

use super::*;
use crate::heap::{DragItemData, DragStore, DragStoreMode, WebBlobData};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostDragEventKind {
    DragStart,
    Drag,
    DragEnter,
    DragOver,
    DragLeave,
    Drop,
    DragEnd,
}

impl HostDragEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DragStart => "dragstart",
            Self::Drag => "drag",
            Self::DragEnter => "dragenter",
            Self::DragOver => "dragover",
            Self::DragLeave => "dragleave",
            Self::Drop => "drop",
            Self::DragEnd => "dragend",
        }
    }

    pub(super) fn from_value(value: &Value) -> Option<Self> {
        [
            Self::DragStart,
            Self::Drag,
            Self::DragEnter,
            Self::DragOver,
            Self::DragLeave,
            Self::Drop,
            Self::DragEnd,
        ]
        .into_iter()
        .find(|kind| *value == Value::String(kind.as_str().into()))
    }

    fn mode(self) -> DragStoreMode {
        match self {
            Self::DragStart => DragStoreMode::ReadWrite,
            Self::Drop => DragStoreMode::ReadOnly,
            _ => DragStoreMode::Protected,
        }
    }

    fn cancelable(self) -> bool {
        !matches!(self, Self::DragLeave | Self::DragEnd)
    }
}

pub enum HostDragItem {
    Text { format: String, data: String },
    File(HostFileData),
}

/// Protected events carry format metadata only. Actual content is admitted
/// for the owner's dragstart or explicit drop, and never includes OS paths.
pub struct HostDragEventData {
    pub kind: HostDragEventKind,
    pub client_x: f64,
    pub client_y: f64,
    pub screen_x: f64,
    pub screen_y: f64,
    pub page_x: f64,
    pub page_y: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub related_target: Option<HostObjectKey>,
    pub buttons: u16,
    pub alt_key: bool,
    pub ctrl_key: bool,
    pub meta_key: bool,
    pub shift_key: bool,
    pub caps_lock: bool,
    pub types: Vec<String>,
    pub items: Vec<HostDragItem>,
    pub drop_effect: String,
    pub effect_allowed: String,
}

impl HostDragEventData {
    pub fn new(kind: HostDragEventKind, client_x: f64, client_y: f64) -> Self {
        Self {
            kind,
            client_x,
            client_y,
            screen_x: 0.0,
            screen_y: 0.0,
            page_x: client_x,
            page_y: client_y,
            offset_x: client_x,
            offset_y: client_y,
            related_target: None,
            buttons: 0,
            alt_key: false,
            ctrl_key: false,
            meta_key: false,
            shift_key: false,
            caps_lock: false,
            types: Vec::new(),
            items: Vec::new(),
            drop_effect: "none".into(),
            effect_allowed: "uninitialized".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostDragImage {
    pub element: HostObjectKey,
    pub x: i32,
    pub y: i32,
}

pub struct HostDragEventOutcome {
    pub default_prevented: bool,
    pub drop_effect: String,
    pub effect_allowed: String,
    /// Owner-readable dragstart edits; empty for other event kinds.
    pub items: Vec<HostDragItem>,
    pub image: Option<HostDragImage>,
}

impl Vm {
    /// Dispatches an event to this exact DOM family. It confers no gesture
    /// capability; the embedding owner separately authorizes OS input.
    pub fn dispatch_host_drag_event(
        &mut self,
        family: HostObjectFamily,
        path: &[HostObjectKey],
        input: &HostDragEventData,
    ) -> Result<HostDragEventOutcome, RuntimeError> {
        self.ensure_no_debugger_continuation()?;
        self.host_family_prototype(family)?;
        if !self.host_object_families[family.index as usize].drag_events
            || path.is_empty()
            || path.len() > 256
            || path
                .iter()
                .any(|key| !key.matches_owner(path[0].owner, path[0].generation))
            || path.iter().copied().collect::<HashSet<_>>().len() != path.len()
            || !input.client_x.is_finite()
            || !input.client_y.is_finite()
            || !input.screen_x.is_finite()
            || !input.screen_y.is_finite()
            || [input.page_x, input.page_y, input.offset_x, input.offset_y]
                .iter()
                .any(|coordinate| !coordinate.is_finite())
            || input
                .related_target
                .is_some_and(|key| !key.matches_owner(path[0].owner, path[0].generation))
            || input.screen_x.abs() > 1_000_000.0
            || input.screen_y.abs() > 1_000_000.0
            || input.types.len() > 64
            || input.items.len() > 64
            || !["none", "copy", "link", "move"].contains(&input.drop_effect.as_str())
            || ![
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
            .contains(&input.effect_allowed.as_str())
            || input.kind.mode() == DragStoreMode::Protected && !input.items.is_empty()
            || input.types.iter().any(|format| !valid_drag_format(format))
        {
            return Err(RuntimeError::TypeError("Invalid owner drag event".into()));
        }
        let mut bytes = 0usize;
        let mut formats = HashSet::new();
        for item in &input.items {
            match item {
                HostDragItem::Text { format, data } => {
                    if format == "Files"
                        || !valid_drag_format(format)
                        || !formats.insert(format.to_ascii_lowercase())
                    {
                        return Err(RuntimeError::TypeError(
                            "Invalid owner drag text format".into(),
                        ));
                    }
                    bytes = bytes.saturating_add(data.len());
                }
                HostDragItem::File(file) => {
                    if !file.valid() {
                        return Err(RuntimeError::TypeError("Invalid owner drag file".into()));
                    }
                    bytes = bytes.saturating_add(file.bytes.len());
                }
            }
        }
        if bytes > 1_048_576 {
            return Err(RuntimeError::RangeError(
                "Owner drag content limit exceeded".into(),
            ));
        }
        if self.active_host_click_event.is_some() || self.active_host_drag_event.is_some() {
            return Err(RuntimeError::TypeError(
                "An event is already dispatching".into(),
            ));
        }
        if !self.globals.contains_key("%WebDataTransfer%") {
            return Err(RuntimeError::TypeError(
                "Browser drag API is unavailable".into(),
            ));
        }
        let base = self.stack.len();
        let target = self.mint_host_wrapper(family.index as usize, path[0])?;
        self.stack.push(Value::Object(target));
        let prototype = self.globals["%WebDataTransfer%"];
        let transfer = match self
            .with_roots(|heap| heap.alloc_drag_store(DragStore::new(input.kind.mode()), prototype))
        {
            Ok(transfer) => transfer,
            Err(error) => {
                self.stack.truncate(base);
                return Err(error);
            }
        };
        self.stack.push(Value::Object(transfer));
        let result = (|| {
            if input.kind.mode() == DragStoreMode::Protected {
                let mut seen = HashSet::new();
                for format in &input.types {
                    if !seen.insert(format.to_ascii_lowercase()) {
                        continue;
                    }
                    if format == "Files" {
                        self.append_drag_item(transfer, "".into(), DragItemData::FileMarker)?;
                    } else {
                        self.append_drag_item(
                            transfer,
                            format.to_ascii_lowercase().into(),
                            DragItemData::Text(JsString::default()),
                        )?;
                    }
                }
            } else {
                for item in &input.items {
                    match item {
                        HostDragItem::Text { format, data } => {
                            self.append_drag_item(
                                transfer,
                                format.to_ascii_lowercase().into(),
                                DragItemData::Text(data.as_str().into()),
                            )?;
                        }
                        HostDragItem::File(file) => {
                            let prototype = self.globals["%WebFile%"];
                            let object = self.with_roots(|heap| {
                                heap.alloc_web_blob(
                                    WebBlobData {
                                        bytes: file.bytes.clone(),
                                        media_type: file.media_type.to_ascii_lowercase().into(),
                                        file: Some((file.name.as_str().into(), file.last_modified)),
                                    },
                                    prototype,
                                )
                            })?;
                            self.append_drag_item(
                                transfer,
                                file.media_type.to_ascii_lowercase().into(),
                                DragItemData::File(object),
                            )?;
                        }
                    }
                }
            }
            let mut store = self.heap.drag_store(transfer)?.clone();
            store.drop_effect = input.drop_effect.as_str().into();
            store.effect_allowed = input.effect_allowed.as_str().into();
            self.with_roots(|heap| heap.replace_drag_store(transfer, store))?;
            let prototype = self.globals["%WebDragEvent%"];
            let mut state = self.new_web_event_state(3, input.kind.as_str().into());
            state.transfer = Some(transfer);
            state.cancelable = input.kind.cancelable();
            state.set("target", Value::Object(target));
            state.set("isTrusted", Value::Bool(true));
            state.set("bubbles", Value::Bool(true));
            state.set("composed", Value::Bool(true));
            state.set("clientX", Value::Number(input.client_x));
            state.set("clientY", Value::Number(input.client_y));
            state.set("screenX", Value::Number(input.screen_x));
            state.set("screenY", Value::Number(input.screen_y));
            state.set("pageX", Value::Number(input.page_x));
            state.set("pageY", Value::Number(input.page_y));
            state.set("offsetX", Value::Number(input.offset_x));
            state.set("offsetY", Value::Number(input.offset_y));
            if let Some(key) = input.related_target {
                let related = self.mint_host_wrapper(family.index as usize, key)?;
                self.stack.push(Value::Object(related));
                state.set("relatedTarget", Value::Object(related));
            }
            state.set("buttons", Value::Number(input.buttons as f64));
            for (field, value) in [
                ("altKey", input.alt_key),
                ("ctrlKey", input.ctrl_key),
                ("metaKey", input.meta_key),
                ("shiftKey", input.shift_key),
                ("modifierCapsLock", input.caps_lock),
            ] {
                state.set(field, Value::Bool(value));
            }
            for key in path {
                let wrapper = self.mint_host_wrapper(family.index as usize, *key)?;
                self.stack.push(Value::Object(wrapper));
                state.path.push(wrapper);
            }
            let event = self.with_roots(|heap| heap.alloc_drag_event(state, prototype))?;
            self.stack.push(Value::Object(event));
            self.finish_web_event(event)?;
            self.active_host_drag_event = Some(event);
            self.remaining_instructions = self.config.instruction_budget;
            let dispatch = self.dispatch_drag_listeners(family, path, input.kind, event);
            self.active_host_drag_event = None;
            let mut state = self.heap.drag_event(event)?.clone();
            state.current_target = None;
            state.phase = 0;
            state.path.clear();
            self.with_roots(|heap| heap.set_drag_event_state(event, state.clone()))?;
            dispatch?;
            let store = self.heap.drag_store(transfer)?;
            let mut items = Vec::new();
            if input.kind == HostDragEventKind::DragStart {
                let total = store.items.iter().try_fold(0usize, |bytes, item| {
                    Ok::<_, RuntimeError>(bytes.saturating_add(match &item.data {
                        DragItemData::Text(data) => data.byte_len(),
                        DragItemData::File(file) => self.heap.web_blob(*file)?.bytes.len(),
                        DragItemData::FileMarker => 0,
                    }))
                })?;
                if store.items.len() > 64 || total > 1_048_576 {
                    return Err(RuntimeError::RangeError(
                        "Native drag export limit exceeded".into(),
                    ));
                }
                for item in &store.items {
                    let format = item.format.to_utf8().map_err(|_| {
                        RuntimeError::TypeError("Non-UTF-8 native drag format".into())
                    })?;
                    if !matches!(item.data, DragItemData::FileMarker)
                        && !format.is_empty()
                        && !valid_drag_format(&format)
                    {
                        return Err(RuntimeError::TypeError(
                            "Invalid native drag export format".into(),
                        ));
                    }
                    match &item.data {
                        DragItemData::Text(data) => items.push(HostDragItem::Text {
                            format: item.format.to_utf8().map_err(|_| {
                                RuntimeError::TypeError("Non-UTF-8 drag format".into())
                            })?,
                            data: data.to_utf8().map_err(|_| {
                                RuntimeError::TypeError("Non-UTF-8 native drag text".into())
                            })?,
                        }),
                        DragItemData::File(file) => {
                            let blob = self.heap.web_blob(*file)?;
                            let (name, last_modified) =
                                blob.file.as_ref().expect("drag File brand");
                            items.push(HostDragItem::File(HostFileData {
                                name: name.to_utf8().map_err(|_| {
                                    RuntimeError::TypeError("Non-UTF-8 drag filename".into())
                                })?,
                                media_type: blob.media_type.to_utf8().unwrap_or_default(),
                                last_modified: *last_modified,
                                bytes: blob.bytes.clone(),
                            }));
                        }
                        DragItemData::FileMarker => {}
                    }
                }
            }
            Ok(HostDragEventOutcome {
                default_prevented: state.default_prevented,
                drop_effect: store.drop_effect.to_utf8().expect("admitted operation"),
                effect_allowed: store.effect_allowed.to_utf8().expect("admitted operation"),
                items,
                image: if input.kind == HostDragEventKind::DragStart {
                    store
                        .image
                        .map(|(element, x, y)| {
                            let element = self.host_object_families[family.index as usize]
                                .keys_by_wrapper
                                .get(&element)
                                .copied()
                                .filter(|key| key.matches_owner(path[0].owner, path[0].generation))
                                .ok_or_else(|| {
                                    RuntimeError::TypeError(
                                        "Drag image belongs to another document".into(),
                                    )
                                })?;
                            Ok::<_, RuntimeError>(HostDragImage { element, x, y })
                        })
                        .transpose()?
                } else {
                    None
                },
            })
        })();
        self.active_host_drag_event = None;
        // Retained DataTransfer/items/FileList become disabled even when a
        // callback failed. Independently retained File values own their bytes.
        let mut store = self.heap.drag_store(transfer)?.clone();
        store.mode = DragStoreMode::Disabled;
        store.items.clear();
        store.types = None;
        let cleanup = self.with_roots(|heap| heap.replace_drag_store(transfer, store));
        self.stack.truncate(base);
        cleanup?;
        result
    }

    fn dispatch_drag_listeners(
        &mut self,
        family: HostObjectFamily,
        path: &[HostObjectKey],
        kind: HostDragEventKind,
        event: ObjectId,
    ) -> Result<(), RuntimeError> {
        for (index, capture) in super::listener_phases(path.len()) {
            let key = &path[index];
            if !capture
                && index != 0
                && self.heap.drag_event(event)?.get("bubbles") != Value::Bool(true)
            {
                break;
            }
            let current = self.mint_host_wrapper(family.index as usize, *key)?;
            let mut state = self.heap.drag_event(event)?.clone();
            state.current_target = Some(current);
            state.phase = if index == 0 {
                2
            } else if capture {
                1
            } else {
                3
            };
            self.with_roots(|heap| heap.set_drag_event_state(event, state))?;
            let callbacks = self
                .host_click_listeners
                .iter()
                .filter(|listener| {
                    listener.family_index == family.index
                        && listener.key == *key
                        && listener.drag == Some(kind)
                        && listener.capture == capture
                })
                .cloned()
                .collect::<Vec<_>>();
            let mut roots = Vec::new();
            for callback in &callbacks {
                match self.heap.root(callback.callback) {
                    Ok(root) => roots.push(root),
                    Err(error) => {
                        for root in roots {
                            let _ = self.heap.unroot(root);
                        }
                        return Err(error.into());
                    }
                }
            }
            let result = (|| {
                for callback in callbacks {
                    if self.heap.drag_event(event)?.immediate_stopped {
                        break;
                    }
                    if let Err(error) = self.invoke_host_listener(&callback, current, event) {
                        if !super::super::web_drag_drop::drag_page_exception(&error) {
                            return Err(error);
                        }
                    }
                }
                Ok(())
            })();
            for root in roots {
                self.heap.unroot(root)?;
            }
            result?;
            if self.heap.drag_event(event)?.propagation_stopped && !(index == 0 && capture) {
                break;
            }
        }
        Ok(())
    }
}

fn valid_drag_format(format: &str) -> bool {
    !format.is_empty()
        && format.len() <= 255
        && format.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
}
