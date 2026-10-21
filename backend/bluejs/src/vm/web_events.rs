// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Branded event constructors and inherited IDL attributes. Mutable dispatch
//! state remains collector-owned, including retained targets and paths.

use super::native_value_storage::NativeValue;
use super::realm_call_storage::RealmCallRoots;
use super::transport_buffer_storage::TransportVector;
use super::web_event_storage::{EventContext, NativeEventState};
use super::*;
use crate::heap::DragEventState;
use crate::native::EventNative;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/web_event_storage.rs"]
mod storage_tests;

const EVENT_NAMES: [&str; 8] = [
    "Event",
    "UIEvent",
    "MouseEvent",
    "DragEvent",
    "ProgressEvent",
    "CustomEvent",
    "ErrorEvent",
    "PromiseRejectionEvent",
];
const MODIFIERS: [(&str, &str); 14] = [
    ("Alt", "altKey"),
    ("Control", "ctrlKey"),
    ("Meta", "metaKey"),
    ("Shift", "shiftKey"),
    ("AltGraph", "modifierAltGraph"),
    ("CapsLock", "modifierCapsLock"),
    ("Fn", "modifierFn"),
    ("FnLock", "modifierFnLock"),
    ("Hyper", "modifierHyper"),
    ("NumLock", "modifierNumLock"),
    ("ScrollLock", "modifierScrollLock"),
    ("Super", "modifierSuper"),
    ("Symbol", "modifierSymbol"),
    ("SymbolLock", "modifierSymbolLock"),
];

impl Vm {
    pub(super) fn install_web_progress_events(&mut self) -> Result<(), RuntimeError> {
        self.install_web_event_subtype(4, &["lengthComputable", "loaded", "total"], &[])
    }

    pub(super) fn install_web_custom_events(&mut self) -> Result<(), RuntimeError> {
        self.install_web_event_subtype(
            5,
            &["detail"],
            &[("initCustomEvent", 1, EventNative::InitCustomEvent)],
        )
    }

    pub(super) fn install_web_error_events(&mut self) -> Result<(), RuntimeError> {
        self.install_web_event_subtype(6, &["message", "filename", "lineno", "colno", "error"], &[])
    }

    pub(super) fn install_web_rejection_events(&mut self) -> Result<(), RuntimeError> {
        self.install_web_event_subtype(7, &["promise", "reason"], &[])
    }

    fn install_web_event_subtype(
        &mut self,
        rank: u8,
        fields: &[&'static str],
        methods: &[(&str, u32, EventNative)],
    ) -> Result<(), RuntimeError> {
        let name = EVENT_NAMES[rank as usize];
        let key = format!("%Web{name}%");
        if self.globals.contains_key(&key) {
            return Ok(());
        }
        let global = self.global("globalThis")?.object_id().unwrap();
        if self.heap.get_own(global, name)?.is_some() {
            return Err(RuntimeError::TypeError(format!(
                "{name} global already defined"
            )));
        }
        let base = self.stack.len();
        let result = (|| {
            let parent = self.globals["%WebEvent%"];
            let function = self.function_prototype()?;
            let prototype = self.with_roots(|heap| heap.alloc_object(Some(parent)))?;
            self.stack.push(Value::Object(prototype));
            for field in fields {
                self.event_getter(prototype, function, field, rank, true)?;
            }
            for (method, length, native) in methods {
                self.install_native(
                    prototype,
                    function,
                    method,
                    *length,
                    NativeFunction::WebEvent(*native),
                )?;
                self.with_roots(|heap| {
                    heap.define_own_property(
                        prototype,
                        *method,
                        PropertyDescriptor {
                            enumerable: Some(true),
                            ..Default::default()
                        },
                    )
                })?;
            }
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
                if rank == 7 { 2 } else { 1 },
                NativeFunction::WebEvent(EventNative::Constructor(rank)),
                self.heap
                    .get_own(parent, "constructor")?
                    .and_then(|value| value.object_id())
                    .expect("installed Event constructor"),
                prototype,
            )?;
            self.stack.push(Value::Object(constructor));
            self.define_data(global, name, Value::Object(constructor), true, false, true)?;
            self.heap.root(prototype)?;
            self.globals.insert(key, prototype);
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn install_web_events(&mut self) -> Result<(), RuntimeError> {
        if self.globals.contains_key("%WebEvent%") {
            return Ok(());
        }
        let global = self.global("globalThis")?.object_id().unwrap();
        for name in &EVENT_NAMES[..3] {
            if self.heap.get_own(global, *name)?.is_some() {
                return Err(RuntimeError::TypeError(
                    "Event API global already defined".into(),
                ));
            }
        }
        let base = self.stack.len();
        let result = (|| {
            let function = self.function_prototype()?;
            let mut parent = self.object_prototype;
            let mut constructor_parent = function;
            for (rank, name) in EVENT_NAMES[..3].iter().enumerate() {
                let rank = rank as u8;
                let prototype = self.with_roots(|heap| heap.alloc_object(Some(parent)))?;
                self.stack.push(Value::Object(prototype));
                let fields: &[&'static str] = match rank {
                    0 => &[
                        "type",
                        "target",
                        "srcElement",
                        "currentTarget",
                        "eventPhase",
                        "bubbles",
                        "cancelable",
                        "defaultPrevented",
                        "composed",
                        "timeStamp",
                        "cancelBubble",
                        "returnValue",
                    ],
                    1 => &["view", "detail", "which"],
                    _ => &[
                        "screenX",
                        "screenY",
                        "clientX",
                        "clientY",
                        "x",
                        "y",
                        "pageX",
                        "pageY",
                        "offsetX",
                        "offsetY",
                        "ctrlKey",
                        "shiftKey",
                        "altKey",
                        "metaKey",
                        "button",
                        "buttons",
                        "relatedTarget",
                    ],
                };
                for field in fields {
                    self.event_getter(prototype, function, field, rank, true)?;
                }
                let methods: &[(&str, u32, EventNative)] = match rank {
                    0 => &[
                        ("preventDefault", 0, EventNative::PreventDefault),
                        ("stopPropagation", 0, EventNative::StopPropagation(false)),
                        (
                            "stopImmediatePropagation",
                            0,
                            EventNative::StopPropagation(true),
                        ),
                        ("composedPath", 0, EventNative::ComposedPath),
                        ("initEvent", 1, EventNative::InitEvent),
                    ],
                    1 => &[("initUIEvent", 1, EventNative::InitUIEvent)],
                    _ => &[
                        ("getModifierState", 1, EventNative::GetModifierState),
                        ("initMouseEvent", 1, EventNative::InitMouseEvent),
                    ],
                };
                for (method, length, native) in methods {
                    self.install_native(
                        prototype,
                        function,
                        method,
                        *length,
                        NativeFunction::WebEvent(*native),
                    )?;
                    self.with_roots(|heap| {
                        heap.define_own_property(
                            prototype,
                            *method,
                            PropertyDescriptor {
                                enumerable: Some(true),
                                ..Default::default()
                            },
                        )
                    })?;
                }
                if rank == 0 {
                    for (field, propagation) in [("cancelBubble", true), ("returnValue", false)] {
                        let setter_name = format!("set {field}");
                        let setter = self.with_roots(|heap| {
                            heap.alloc_native_function(
                                NativeFunction::WebEvent(EventNative::SetCancellation(propagation)),
                                &setter_name,
                                function,
                            )
                        })?;
                        self.stack.push(Value::Object(setter));
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
                                prototype,
                                field,
                                PropertyDescriptor {
                                    set: Some(Value::Object(setter)),
                                    ..Default::default()
                                },
                            )
                        })?;
                        self.stack.pop();
                    }
                }
                self.define_data(
                    prototype,
                    JsSymbol::well_known("toStringTag"),
                    Value::String((*name).into()),
                    false,
                    false,
                    true,
                )?;
                let constructor = self.web_file_constructor(
                    name,
                    1,
                    NativeFunction::WebEvent(EventNative::Constructor(rank)),
                    function,
                    prototype,
                )?;
                self.stack.push(Value::Object(constructor));
                self.heap
                    .set_prototype(constructor, Some(constructor_parent))?;
                if rank == 0 {
                    for (constant, value) in [
                        ("NONE", 0.0),
                        ("CAPTURING_PHASE", 1.0),
                        ("AT_TARGET", 2.0),
                        ("BUBBLING_PHASE", 3.0),
                    ] {
                        self.define_data(
                            prototype,
                            constant,
                            Value::Number(value),
                            false,
                            true,
                            false,
                        )?;
                        self.define_data(
                            constructor,
                            constant,
                            Value::Number(value),
                            false,
                            true,
                            false,
                        )?;
                    }
                }
                self.define_data(global, *name, Value::Object(constructor), true, false, true)?;
                self.heap.root(prototype)?;
                self.globals.insert(format!("%Web{name}%"), prototype);
                parent = prototype;
                constructor_parent = constructor;
            }
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    fn event_getter(
        &mut self,
        owner: ObjectId,
        function: ObjectId,
        name: &'static str,
        rank: u8,
        configurable: bool,
    ) -> Result<(), RuntimeError> {
        self.install_native_getter(
            owner,
            function,
            name,
            NativeFunction::WebEvent(EventNative::Get(name, rank)),
        )?;
        self.with_roots(|heap| {
            heap.define_own_property(
                owner,
                name,
                PropertyDescriptor {
                    enumerable: Some(true),
                    configurable: Some(configurable),
                    ..Default::default()
                },
            )
        })?;
        Ok(())
    }

    pub(super) fn finish_web_event(&mut self, event: ObjectId) -> Result<(), RuntimeError> {
        let function = self.function_prototype()?;
        self.event_getter(event, function, "isTrusted", 0, false)
    }

    #[cfg(any(test, coverage))]
    pub(super) fn new_web_event_state(&self, rank: u8, name: JsString) -> DragEventState {
        let mut state = DragEventState::new(rank);
        self.initialize_web_event_state(&mut state, name);
        state
    }

    pub(super) fn initialize_web_event_state(&self, state: &mut DragEventState, name: JsString) {
        let rank = state.interface;
        state.set("type", Value::String(name));
        state.set("target", Value::Null);
        state.set("isTrusted", Value::Bool(false));
        state.set("bubbles", Value::Bool(false));
        state.set("composed", Value::Bool(false));
        state.set(
            "timeStamp",
            Value::Number(self.event_time_origin.elapsed().as_secs_f64() * 1000.0),
        );
        if (1..=3).contains(&rank) {
            state.set("view", Value::Null);
            state.set("detail", Value::Number(0.0));
        }
        if (2..=3).contains(&rank) {
            for field in [
                "clientX", "clientY", "screenX", "screenY", "button", "buttons",
            ] {
                state.set(field, Value::Number(0.0));
            }
            for (_, field) in MODIFIERS {
                state.set(field, Value::Bool(false));
            }
            state.set("relatedTarget", Value::Null);
        }
    }

    pub(super) fn construct_web_event(
        &mut self,
        rank: u8,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct || args.is_empty() {
            return Err(RuntimeError::TypeError(
                "Event constructor requires new and a type".into(),
            ));
        }
        let context = EventContext::capture(self)?;
        let roots = context.prepare_roots(self, args.iter().filter_map(Value::object_id), 4)?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            vm.construct_web_event_input(rank, args, &context, &roots)
        });
        context.validate().and(result)
    }

    fn construct_web_event_input(
        &mut self,
        rank: u8,
        args: &[Value],
        context: &EventContext,
        roots: &RealmCallRoots,
    ) -> Result<Value, RuntimeError> {
        let name = self.event_string(&args[0], context)?;
        let options = native::argument(args, 1);
        if *options != Value::Null && *options != Value::Undefined && options.object_id().is_none()
        {
            return Err(RuntimeError::TypeError(
                "Event init must be a dictionary".into(),
            ));
        }
        let mut state = NativeEventState::new(self, rank, name)?;
        // Inherited dictionaries first; retain each converted member before
        // another getter can run. Only object IDs enter the explicit roots.
        for field in ["bubbles", "cancelable", "composed"] {
            let value = self.event_init_owned(options, field, context)?;
            let value = self.to_boolean(value.value())?;
            if field == "cancelable" {
                state.state.cancelable = value;
            } else {
                state.scalar(self, roots, field, Value::Bool(value))?;
            }
        }
        if (1..=3).contains(&rank) {
            let detail = self.event_init_owned(options, "detail", context)?;
            let detail = detail.with_value(self, |vm, value| {
                vm.event_integer_checked(value, 32, true, context)
            })?;
            state.scalar(self, roots, "detail", Value::Number(detail))?;
            let view = self.event_init_owned(options, "view", context)?;
            if *view.value() != Value::Null && *view.value() != Value::Undefined {
                return Err(RuntimeError::TypeError(
                    "Event view requires a Window".into(),
                ));
            }
        }
        if (2..=3).contains(&rank) {
            for field in [
                "altKey",
                "ctrlKey",
                "metaKey",
                "modifierAltGraph",
                "modifierCapsLock",
                "modifierFn",
                "modifierFnLock",
                "modifierHyper",
                "modifierNumLock",
                "modifierScrollLock",
                "modifierSuper",
                "modifierSymbol",
                "modifierSymbolLock",
                "shiftKey",
            ] {
                let value = self.event_init_owned(options, field, context)?;
                let value = self.to_boolean(value.value())?;
                state.scalar(self, roots, field, Value::Bool(value))?;
            }
            for (field, signed) in [("button", true), ("buttons", false)] {
                let value = self.event_init_owned(options, field, context)?;
                let value = value.with_value(self, |vm, value| {
                    vm.event_integer_checked(value, 16, signed, context)
                })?;
                state.scalar(self, roots, field, Value::Number(value))?;
            }
            for field in ["clientX", "clientY"] {
                self.event_init_coordinate(&mut state, options, field, context, roots)?;
            }
            let related = self.event_init_owned(options, "relatedTarget", context)?;
            if *related.value() != Value::Null && *related.value() != Value::Undefined {
                if !related
                    .value()
                    .object_id()
                    .is_some_and(|object| self.is_host_event_target(object))
                {
                    return Err(RuntimeError::TypeError(
                        "relatedTarget requires an EventTarget".into(),
                    ));
                }
                state.set(self, roots, "relatedTarget", related)?;
            }
            for field in ["screenX", "screenY"] {
                self.event_init_coordinate(&mut state, options, field, context, roots)?;
            }
        }
        if rank == 3 {
            let transfer = self.event_init_owned(options, "dataTransfer", context)?;
            if *transfer.value() != Value::Null && *transfer.value() != Value::Undefined {
                let object = transfer
                    .value()
                    .object_id()
                    .filter(|id| self.heap.drag_store(*id).is_ok())
                    .ok_or_else(|| {
                        RuntimeError::TypeError("dataTransfer requires a DataTransfer".into())
                    })?;
                roots.push(&self.heap, object)?;
                state.state.transfer = Some(object);
            }
        }
        if rank == 4 {
            let value = self.event_init_owned(options, "lengthComputable", context)?;
            let value = self.to_boolean(value.value())?;
            state.scalar(self, roots, "lengthComputable", Value::Bool(value))?;
            for field in ["loaded", "total"] {
                let value = self.event_init_owned(options, field, context)?;
                let value = value.with_value(self, |vm, value| {
                    vm.event_integer_checked(value, 64, false, context)
                })?;
                state.scalar(self, roots, field, Value::Number(value))?;
            }
        }
        if rank == 5 {
            let detail = self.event_init_owned(options, "detail", context)?;
            let detail = if *detail.value() == Value::Undefined {
                NativeValue::adopt(Value::Null, context.owner.as_ref())?
            } else {
                detail
            };
            state.set(self, roots, "detail", detail)?;
        }
        if rank == 6 {
            let colno = self.event_init_owned(options, "colno", context)?;
            let colno = colno.with_value(self, |vm, value| {
                vm.event_integer_checked(value, 32, false, context)
            })?;
            state.scalar(self, roots, "colno", Value::Number(colno))?;
            let error = self.event_init_owned(options, "error", context)?;
            state.set(self, roots, "error", error)?;
            let filename = self.event_init_owned(options, "filename", context)?;
            let filename = self.event_optional_string(filename, context)?;
            let (filename, storage) = filename.into_retained_parts();
            let Value::String(filename) = filename else {
                unreachable!("converted filename")
            };
            let filename = NativeValue::from_retained_parts(
                Value::String(filename.replace_unpaired_surrogates()),
                storage,
            );
            state.set(self, roots, "filename", filename)?;
            let lineno = self.event_init_owned(options, "lineno", context)?;
            let lineno = lineno.with_value(self, |vm, value| {
                vm.event_integer_checked(value, 32, false, context)
            })?;
            state.scalar(self, roots, "lineno", Value::Number(lineno))?;
            let message = self.event_init_owned(options, "message", context)?;
            let message = self.event_optional_string(message, context)?;
            state.set(self, roots, "message", message)?;
        }
        if rank == 7 {
            let promise = self.event_init_owned(options, "promise", context)?;
            if promise.value().object_id().is_none() {
                return Err(RuntimeError::TypeError(
                    "PromiseRejectionEvent requires an object promise".into(),
                ));
            }
            state.set(self, roots, "promise", promise)?;
            let reason = self.event_init_owned(options, "reason", context)?;
            state.set(self, roots, "reason", reason)?;
        }
        let default = self.globals[&format!("%Web{}%", EVENT_NAMES[rank as usize])];
        let prototype = self.constructor_prototype_for(default, None);
        let prototype = context.validate().and(prototype)?;
        roots.push(&self.heap, prototype)?;
        let event = state.allocate(self, prototype)?;
        roots.push(&self.heap, event)?;
        let finished = self.finish_web_event(event);
        context.validate().and(finished)?;
        Ok(Value::Object(event))
    }

    fn event_string(
        &mut self,
        value: &Value,
        context: &EventContext,
    ) -> Result<NativeValue, RuntimeError> {
        if matches!(value, Value::String(_)) {
            return NativeValue::copied(value, context.owner.as_ref());
        }
        let result = self.coerce_string(value);
        let result = context.validate().and(result)?;
        NativeValue::adopt(Value::String(result), context.owner.as_ref())
    }

    fn event_optional_string(
        &mut self,
        value: NativeValue,
        context: &EventContext,
    ) -> Result<NativeValue, RuntimeError> {
        match value.value() {
            Value::String(_) => Ok(value),
            Value::Undefined => {
                NativeValue::adopt(Value::String(JsString::default()), context.owner.as_ref())
            }
            _ => value.with_value(self, |vm, value| vm.event_string(value, context)),
        }
    }

    fn event_init_owned(
        &mut self,
        options: &Value,
        field: &str,
        context: &EventContext,
    ) -> Result<NativeValue, RuntimeError> {
        let result = self.event_init_value(options, field);
        let result = context.validate().and(result)?;
        NativeValue::adopt(result, context.owner.as_ref())
    }

    fn event_integer_checked(
        &mut self,
        value: &Value,
        bits: u32,
        signed: bool,
        context: &EventContext,
    ) -> Result<f64, RuntimeError> {
        let result = self.event_integer(value, bits, signed);
        context.validate().and(result)
    }

    fn event_init_value(&mut self, options: &Value, field: &str) -> Result<Value, RuntimeError> {
        if *options == Value::Null || *options == Value::Undefined {
            Ok(Value::Undefined)
        } else {
            self.get_property(options, &field.into())
        }
    }

    fn event_integer(
        &mut self,
        value: &Value,
        bits: u32,
        signed: bool,
    ) -> Result<f64, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(0.0);
        }
        let number = self.coerce_number(value)?;
        if !number.is_finite() || number == 0.0 {
            return Ok(0.0);
        }
        let modulus = 2_f64.powi(bits as i32);
        let mut number = number.trunc().rem_euclid(modulus);
        if signed && number >= modulus / 2.0 {
            number -= modulus;
        }
        Ok(number)
    }

    fn event_init_coordinate(
        &mut self,
        state: &mut NativeEventState,
        options: &Value,
        field: &'static str,
        context: &EventContext,
        roots: &RealmCallRoots,
    ) -> Result<(), RuntimeError> {
        let value = self.event_init_owned(options, field, context)?;
        let number = if *value.value() == Value::Undefined {
            0.0
        } else {
            let number = value.with_value(self, |vm, value| vm.coerce_number(value));
            context.validate().and(number)?
        };
        if !number.is_finite() {
            return Err(RuntimeError::TypeError(
                "Event coordinate must be finite".into(),
            ));
        }
        state.scalar(self, roots, field, Value::Number(number))
    }

    fn initialize_web_event(
        &mut self,
        object: ObjectId,
        rank: u8,
        args: &[Value],
        custom: bool,
        context: &EventContext,
        roots: &RealmCallRoots,
    ) -> Result<Value, RuntimeError> {
        if !event_interface_matches(self.heap.drag_event(object)?.interface, rank) {
            return Err(RuntimeError::TypeError(
                "Invalid event interface receiver".into(),
            ));
        }
        if args.is_empty() {
            return Err(RuntimeError::TypeError("Event type is required".into()));
        }
        let name = self.event_string(&args[0], context)?;
        let bubbles = self.to_boolean(native::argument(args, 1))?;
        let cancelable = self.to_boolean(native::argument(args, 2))?;
        let count = 4
            + if rank >= 1 { 2 } else { 0 }
            + if rank >= 2 { 10 } else { 0 }
            + usize::from(custom);
        let mut patch = NativeEventState::empty(rank, count, context.owner.as_ref())?;
        patch.set(self, roots, "type", name)?;
        patch.scalar(self, roots, "bubbles", Value::Bool(bubbles))?;
        patch.scalar(self, roots, "target", Value::Null)?;
        patch.scalar(self, roots, "isTrusted", Value::Bool(false))?;
        if rank >= 1 {
            let view = native::argument(args, 3);
            if *view != Value::Null && *view != Value::Undefined {
                return Err(RuntimeError::TypeError(
                    "Event view requires a Window".into(),
                ));
            }
            patch.scalar(self, roots, "view", Value::Null)?;
            let detail =
                self.event_integer_checked(native::argument(args, 4), 32, true, context)?;
            patch.scalar(self, roots, "detail", Value::Number(detail))?;
        }
        if rank >= 2 {
            for (index, field) in [
                (5, "screenX"),
                (6, "screenY"),
                (7, "clientX"),
                (8, "clientY"),
            ] {
                let value =
                    self.event_integer_checked(native::argument(args, index), 32, true, context)?;
                patch.scalar(self, roots, field, Value::Number(value))?;
            }
            for (index, field) in [
                (9, "ctrlKey"),
                (10, "altKey"),
                (11, "shiftKey"),
                (12, "metaKey"),
            ] {
                let value = self.to_boolean(native::argument(args, index))?;
                patch.scalar(self, roots, field, Value::Bool(value))?;
            }
            let button =
                self.event_integer_checked(native::argument(args, 13), 16, true, context)?;
            patch.scalar(self, roots, "button", Value::Number(button))?;
            let related = native::argument(args, 14);
            let related = if *related == Value::Null || *related == Value::Undefined {
                Value::Null
            } else {
                Value::Object(
                    related
                        .object_id()
                        .filter(|object| self.is_host_event_target(*object))
                        .ok_or_else(|| {
                            RuntimeError::TypeError("relatedTarget requires an EventTarget".into())
                        })?,
                )
            };
            patch.scalar(self, roots, "relatedTarget", related)?;
        }
        // All observable conversions precede the active-state check and the
        // snapshot. A reentrant initializer's current state is authoritative.
        context.validate()?;
        if self.heap.drag_event(object)?.phase != 0 {
            return Ok(Value::Undefined);
        }
        if custom {
            let detail = native::argument(args, 3);
            let detail = if *detail == Value::Undefined {
                &Value::Null
            } else {
                detail
            };
            let detail = NativeValue::copied(detail, context.owner.as_ref())?;
            patch.set(self, roots, "detail", detail)?;
        }
        let mut state = NativeEventState::copied(self, object, context.owner.as_ref())?;
        state.merge(self, roots, patch)?;
        state.state.cancelable = cancelable;
        state.state.default_prevented = false;
        state.state.propagation_stopped = false;
        state.state.immediate_stopped = false;
        context.validate()?;
        state.into_heap(self, object)?;
        Ok(Value::Undefined)
    }

    pub(super) fn web_event_call(
        &mut self,
        native: EventNative,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if let EventNative::Constructor(rank) = native {
            return self.construct_web_event(rank, args, construct);
        }
        if construct {
            return Err(RuntimeError::TypeError(
                "Event member is not a constructor".into(),
            ));
        }
        let object = receiver
            .object_id()
            .filter(|id| self.heap.drag_event(*id).is_ok())
            .ok_or_else(|| RuntimeError::TypeError("Invalid Event receiver".into()))?;
        let context = EventContext::capture(self)?;
        let ids = std::iter::once(object).chain(args.iter().filter_map(Value::object_id));
        let roots = context.prepare_roots(self, ids, 4)?;
        let result = self.with_realm_call_roots(roots.clone(), |vm| {
            vm.web_event_member_input(native, object, args, &context, &roots)
        });
        context.validate().and(result)
    }

    fn web_event_member_input(
        &mut self,
        native: EventNative,
        object: ObjectId,
        args: &[Value],
        context: &EventContext,
        roots: &RealmCallRoots,
    ) -> Result<Value, RuntimeError> {
        let state = self.heap.drag_event(object)?;
        match native {
            EventNative::Get(name, rank) => {
                if !event_interface_matches(state.interface, rank) {
                    return Err(RuntimeError::TypeError(
                        "Invalid event interface receiver".into(),
                    ));
                }
                let scalar = match name {
                    "currentTarget" => {
                        Some(state.current_target.map_or(Value::Null, Value::Object))
                    }
                    "cancelable" => Some(Value::Bool(state.cancelable)),
                    "defaultPrevented" => Some(Value::Bool(state.default_prevented)),
                    "cancelBubble" => Some(Value::Bool(state.propagation_stopped)),
                    "returnValue" => Some(Value::Bool(!state.default_prevented)),
                    "eventPhase" => Some(Value::Number(state.phase as f64)),
                    "which" => Some(Value::Number(if state.interface >= 2 {
                        match state.get_ref("button") {
                            Some(Value::Number(0.0)) => 1.0,
                            Some(Value::Number(1.0)) => 2.0,
                            Some(Value::Number(2.0)) => 3.0,
                            _ => 0.0,
                        }
                    } else {
                        0.0
                    })),
                    _ => None,
                };
                if let Some(value) = scalar {
                    return Ok(value);
                }
                let field = match name {
                    "srcElement" => "target",
                    "x" => "clientX",
                    "y" => "clientY",
                    "pageX" | "offsetX"
                        if state.phase == 0
                            || state
                                .get_ref(name)
                                .is_none_or(|value| *value == Value::Undefined) =>
                    {
                        "clientX"
                    }
                    "pageY" | "offsetY"
                        if state.phase == 0
                            || state
                                .get_ref(name)
                                .is_none_or(|value| *value == Value::Undefined) =>
                    {
                        "clientY"
                    }
                    _ => name,
                };
                Ok(NativeValue::copied(
                    state.get_ref(field).unwrap_or(&Value::Undefined),
                    context.owner.as_ref(),
                )?
                .into_value())
            }
            EventNative::GetModifierState => {
                if !(2..=3).contains(&state.interface) {
                    return Err(RuntimeError::TypeError(
                        "Invalid MouseEvent receiver".into(),
                    ));
                }
                if args.is_empty() {
                    return Err(RuntimeError::TypeError("Modifier key is required".into()));
                }
                let key = self.event_string(&args[0], context)?;
                context.validate()?;
                let Value::String(key) = key.value() else {
                    unreachable!("converted modifier key")
                };
                let state = self.heap.drag_event(object)?;
                Ok(Value::Bool(MODIFIERS.iter().any(|(name, field)| {
                    key == *name && state.get_ref(field) == Some(&Value::Bool(true))
                })))
            }
            EventNative::ComposedPath => {
                let path =
                    TransportVector::capture(state.path.iter().copied(), context.owner.as_ref())?;
                context.with_roots(self, path.iter().copied(), |vm| {
                    let prototype = vm.array_prototype;
                    let array =
                        vm.with_roots(|heap| heap.alloc_array(path.len() as u32, Some(prototype)))?;
                    roots.push(&vm.heap, array)?;
                    for (index, target) in path.iter().copied().enumerate() {
                        vm.define_data(
                            array,
                            index.to_string(),
                            Value::Object(target),
                            true,
                            true,
                            true,
                        )?;
                    }
                    Ok(Value::Object(array))
                })
            }
            EventNative::InitEvent => {
                self.initialize_web_event(object, 0, args, false, context, roots)
            }
            EventNative::InitUIEvent => {
                self.initialize_web_event(object, 1, args, false, context, roots)
            }
            EventNative::InitMouseEvent => {
                self.initialize_web_event(object, 2, args, false, context, roots)
            }
            EventNative::InitCustomEvent => {
                if state.interface != 5 {
                    return Err(RuntimeError::TypeError(
                        "Invalid CustomEvent receiver".into(),
                    ));
                }
                self.initialize_web_event(object, 0, args, true, context, roots)
            }
            EventNative::StopPropagation(immediate) => {
                self.heap.stop_event_propagation(object, immediate)?;
                Ok(Value::Undefined)
            }
            EventNative::SetCancellation(true) => {
                if self.to_boolean(native::argument(args, 0))? {
                    self.heap.stop_event_propagation(object, false)?;
                }
                Ok(Value::Undefined)
            }
            EventNative::SetCancellation(false) => {
                if !self.to_boolean(native::argument(args, 0))? {
                    self.heap.prevent_event_default(object)?;
                }
                Ok(Value::Undefined)
            }
            EventNative::PreventDefault => {
                self.heap.prevent_event_default(object)?;
                Ok(Value::Undefined)
            }
            EventNative::Constructor(_) => unreachable!(),
        }
    }
}

fn event_interface_matches(interface: u8, required: u8) -> bool {
    required == 0
        || ((1..=3).contains(&required) && (required..=3).contains(&interface))
        || (required >= 4 && interface == required)
}
