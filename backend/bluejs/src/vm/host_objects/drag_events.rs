// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-only drag dispatch. Page constructors never mint trusted events or
//! OS payload access; the embedding owner supplies a validated ancestry.

use super::drag_storage::{NativeHostDragOutcome, NativeHostDragText};
use super::node_events::{valid_host_event_path, HostEventContext};
use super::*;
use crate::heap::DragStoreMode;
use crate::vm::error_input_storage::utf16_from_utf8;
use crate::vm::realm_call_storage::RealmCallRoots;
use crate::vm::web_drag_drop::storage::{NativeDragFile, NativeDragStore};
use crate::vm::web_event_storage::{EventPath, NativeEventState};

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
        let context = HostEventContext::capture(self)?;
        if !self.host_object_families[family.index as usize].drag_events
            || !valid_host_event_path(path)
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
        for (index, item) in input.items.iter().enumerate() {
            match item {
                HostDragItem::Text { format, data } => {
                    if format == "Files"
                        || !valid_drag_format(format)
                        || input.items[..index].iter().any(|previous| {
                            matches!(previous, HostDragItem::Text { format: prior, .. } if prior.eq_ignore_ascii_case(format))
                        })
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
        context
            .with_roots(self, std::iter::empty(), path.len() + 4, |vm, roots| {
                vm.dispatch_host_drag_input(family, path, input, &context, roots)
            })?
            .into_outcome()
    }

    fn dispatch_host_drag_input(
        &mut self,
        family: HostObjectFamily,
        path: &[HostObjectKey],
        input: &HostDragEventData,
        context: &HostEventContext,
        roots: &RealmCallRoots,
    ) -> Result<NativeHostDragOutcome, RuntimeError> {
        let target = self.mint_host_wrapper(family.index as usize, path[0]);
        let target = context.validate(self).and(target)?;
        roots.push(&self.heap, target)?;
        let prototype = self.globals["%WebDataTransfer%"];
        let transfer = NativeDragStore::new(input.kind.mode(), context.event.owner.as_ref())?
            .allocate(self, prototype)?;
        roots.push(&self.heap, transfer)?;
        let mut active_event = None;
        let result = (|| {
            if input.kind.mode() == DragStoreMode::Protected {
                for (index, format) in input.types.iter().enumerate() {
                    context.validate(self)?;
                    if input.types[..index]
                        .iter()
                        .any(|prior| prior.eq_ignore_ascii_case(format))
                    {
                        continue;
                    }
                    let text = NativeHostDragText::from_host(
                        if format == "Files" { "" } else { format },
                        "",
                        context.event.owner.as_ref(),
                    )?;
                    let data = if format == "Files" {
                        NativeValue::adopt(Value::Undefined, context.event.owner.as_ref())?
                    } else {
                        text.data
                    };
                    self.append_drag_item(transfer, text.format, data)?;
                }
            } else {
                for item in &input.items {
                    context.validate(self)?;
                    match item {
                        HostDragItem::Text { format, data } => {
                            let text = NativeHostDragText::from_host(
                                format,
                                data,
                                context.event.owner.as_ref(),
                            )?;
                            self.append_drag_item(transfer, text.format, text.data)?;
                        }
                        HostDragItem::File(file) => {
                            let file_input =
                                NativeDragFile::from_host(file, context.event.owner.as_ref())?;
                            let format = NativeHostDragText::from_host(
                                &file.media_type,
                                "",
                                context.event.owner.as_ref(),
                            )?
                            .format;
                            let prototype = self.globals["%WebFile%"];
                            let object = file_input.allocate(self, prototype)?;
                            self.append_drag_item(
                                transfer,
                                format,
                                NativeValue::adopt(
                                    Value::Object(object),
                                    context.event.owner.as_ref(),
                                )?,
                            )?;
                        }
                    }
                }
            }
            let mut store = NativeDragStore::copied(self, transfer, context.event.owner.as_ref())?;
            store.effect(
                false,
                utf16_from_utf8(input.drop_effect.as_str(), context.event.owner.as_ref())?,
            )?;
            store.effect(
                true,
                utf16_from_utf8(input.effect_allowed.as_str(), context.event.owner.as_ref())?,
            )?;
            store.into_heap(self, transfer)?;
            let prototype = self.globals["%WebDragEvent%"];
            let name = utf16_from_utf8(input.kind.as_str(), context.event.owner.as_ref())?;
            let mut state = NativeEventState::new(self, 3, name)?;
            state.state.transfer = Some(transfer);
            state.state.cancelable = input.kind.cancelable();
            state.scalar(self, roots, "bubbles", Value::Bool(true))?;
            state.scalar(self, roots, "composed", Value::Bool(true))?;
            state.scalar(self, roots, "clientX", Value::Number(input.client_x))?;
            state.scalar(self, roots, "clientY", Value::Number(input.client_y))?;
            state.scalar(self, roots, "screenX", Value::Number(input.screen_x))?;
            state.scalar(self, roots, "screenY", Value::Number(input.screen_y))?;
            state.scalar(self, roots, "pageX", Value::Number(input.page_x))?;
            state.scalar(self, roots, "pageY", Value::Number(input.page_y))?;
            state.scalar(self, roots, "offsetX", Value::Number(input.offset_x))?;
            state.scalar(self, roots, "offsetY", Value::Number(input.offset_y))?;
            if let Some(key) = input.related_target {
                let related = self.mint_host_wrapper(family.index as usize, key);
                let related = context.validate(self).and(related)?;
                state.scalar(self, roots, "relatedTarget", Value::Object(related))?;
            }
            state.scalar(self, roots, "buttons", Value::Number(input.buttons as f64))?;
            for (field, value) in [
                ("altKey", input.alt_key),
                ("ctrlKey", input.ctrl_key),
                ("metaKey", input.meta_key),
                ("shiftKey", input.shift_key),
                ("modifierCapsLock", input.caps_lock),
            ] {
                state.scalar(self, roots, field, Value::Bool(value))?;
            }
            let mut event_path =
                EventPath::with_capacity(path.len(), context.event.owner.as_ref())?;
            for key in path {
                context.validate(self)?;
                let wrapper = self.mint_host_wrapper(family.index as usize, *key);
                let wrapper = context.validate(self).and(wrapper)?;
                roots.push(&self.heap, wrapper)?;
                event_path.push(wrapper)?;
            }
            let event = state.allocate(self, prototype)?;
            active_event = Some(event);
            roots.push(&self.heap, event)?;
            let finished = self.finish_web_event(event);
            context.validate(self).and(finished)?;
            event_path.publish_nodes(self, event, target, true)?;
            self.active_host_drag_event = Some(event);
            self.remaining_instructions = self.config.instruction_budget;
            let dispatch = self.dispatch_drag_listeners(family, path, input.kind, event, context);
            self.active_host_drag_event = None;
            let prevented = self.heap.drag_event(event)?.default_prevented;
            self.heap.finish_event_dispatch(event)?;
            context.validate(self).and(dispatch)?;
            NativeHostDragOutcome::from_store(
                self,
                family,
                path[0],
                transfer,
                input.kind,
                prevented,
                context.event.owner.as_ref(),
            )
        })();
        self.active_host_drag_event = None;
        // Returning existing references/credit needs no fresh admission, even
        // when a callback has already failed its original or calling account.
        let event_cleanup = active_event
            .map(|event| self.heap.finish_event_dispatch(event))
            .transpose();
        let store_cleanup = self.heap.disable_drag_store(transfer);
        context.validate(self)?;
        event_cleanup?;
        store_cleanup?;
        result
    }

    fn dispatch_drag_listeners(
        &mut self,
        family: HostObjectFamily,
        path: &[HostObjectKey],
        kind: HostDragEventKind,
        event: ObjectId,
        context: &HostEventContext,
    ) -> Result<(), RuntimeError> {
        for (index, capture) in super::listener_phases(path.len()) {
            context.validate(self)?;
            let key = &path[index];
            if !capture
                && index != 0
                && self.heap.drag_event(event)?.get("bubbles") != Value::Bool(true)
            {
                break;
            }
            let current = self.heap.drag_event(event)?.path[index];
            self.heap.set_event_phase(
                event,
                current,
                if index == 0 {
                    2
                } else if capture {
                    1
                } else {
                    3
                },
            )?;
            let callbacks = self.listener_snapshot(
                |listener| {
                    listener.family_index == family.index
                        && listener.key == *key
                        && listener.drag == Some(kind)
                        && listener.capture == capture
                },
                false,
            )?;
            let result = (|| {
                for serial in callbacks.serials.iter().copied() {
                    context.validate(self)?;
                    callbacks.validate(self)?;
                    if self.heap.drag_event(event)?.immediate_stopped {
                        break;
                    }
                    let called = self.invoke_host_listener(serial, current, event);
                    context.validate(self)?;
                    callbacks.validate(self)?;
                    if let Err(error) = called {
                        if !super::super::web_drag_drop::drag_page_exception(&error) {
                            return Err(error);
                        }
                    }
                }
                Ok(())
            })();
            result?;
            if self.heap.drag_event(event)?.propagation_stopped && !(index == 0 && capture) {
                break;
            }
        }
        Ok(())
    }
}

pub(super) fn valid_drag_format(format: &str) -> bool {
    !format.is_empty()
        && format.len() <= 255
        && format.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
}
