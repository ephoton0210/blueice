// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Branded event constructors and inherited IDL attributes. Mutable dispatch
//! state remains collector-owned, including retained targets and paths.

use super::*;
use crate::heap::DragEventState;
use crate::native::EventNative;

const EVENT_NAMES: [&str; 7] = [
    "Event",
    "UIEvent",
    "MouseEvent",
    "DragEvent",
    "ProgressEvent",
    "CustomEvent",
    "ErrorEvent",
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
                1,
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

    pub(super) fn new_web_event_state(&self, rank: u8, name: JsString) -> DragEventState {
        let mut state = DragEventState::new(rank);
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
        state
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
        let name = self.coerce_string(&args[0])?;
        let options = native::argument(args, 1).clone();
        if options != Value::Null && options != Value::Undefined && options.object_id().is_none() {
            return Err(RuntimeError::TypeError(
                "Event init must be a dictionary".into(),
            ));
        }
        let base = self.stack.len();
        let result = (|| {
            let mut state = self.new_web_event_state(rank, name);
            // Web IDL visits inherited dictionaries first, sorting members
            // within each dictionary before reading or converting a value.
            for field in ["bubbles", "cancelable", "composed"] {
                let value = self.event_init_value(&options, field)?;
                let value = self.to_boolean(&value)?;
                if field == "cancelable" {
                    state.cancelable = value;
                } else {
                    state.set(field, Value::Bool(value));
                }
            }
            if (1..=3).contains(&rank) {
                let detail = self.event_init_value(&options, "detail")?;
                let detail = self.event_integer(&detail, 32, true)?;
                state.set("detail", Value::Number(detail));
                let view = self.event_init_value(&options, "view")?;
                // This owner API does not yet expose a branded Window.
                if view != Value::Null && view != Value::Undefined {
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
                    let value = self.event_init_value(&options, field)?;
                    let value = self.to_boolean(&value)?;
                    state.set(field, Value::Bool(value));
                }
                for (field, signed) in [("button", true), ("buttons", false)] {
                    let value = self.event_init_value(&options, field)?;
                    let value = self.event_integer(&value, 16, signed)?;
                    state.set(field, Value::Number(value));
                }
                for field in ["clientX", "clientY"] {
                    self.event_init_coordinate(&mut state, &options, field)?;
                }
                let related = self.event_init_value(&options, "relatedTarget")?;
                if related != Value::Null && related != Value::Undefined {
                    let object = related
                        .object_id()
                        .filter(|object| self.is_host_event_target(*object))
                        .ok_or_else(|| {
                            RuntimeError::TypeError("relatedTarget requires an EventTarget".into())
                        })?;
                    self.stack.push(Value::Object(object));
                    state.set("relatedTarget", Value::Object(object));
                }
                for field in ["screenX", "screenY"] {
                    self.event_init_coordinate(&mut state, &options, field)?;
                }
            }
            if rank == 3 {
                let transfer = self.event_init_value(&options, "dataTransfer")?;
                if transfer != Value::Null && transfer != Value::Undefined {
                    let object = transfer
                        .object_id()
                        .filter(|id| self.heap.drag_store(*id).is_ok())
                        .ok_or_else(|| {
                            RuntimeError::TypeError("dataTransfer requires a DataTransfer".into())
                        })?;
                    self.stack.push(Value::Object(object));
                    state.transfer = Some(object);
                }
            }
            if rank == 4 {
                let value = self.event_init_value(&options, "lengthComputable")?;
                let value = self.to_boolean(&value)?;
                state.set("lengthComputable", Value::Bool(value));
                for field in ["loaded", "total"] {
                    let value = self.event_init_value(&options, field)?;
                    let value = self.event_integer(&value, 64, false)?;
                    state.set(field, Value::Number(value));
                }
            }
            if rank == 5 {
                let detail = self.event_init_value(&options, "detail")?;
                let detail = if detail == Value::Undefined {
                    Value::Null
                } else {
                    detail
                };
                self.stack.push(detail.clone());
                state.set("detail", detail);
            }
            if rank == 6 {
                // Inherited EventInit is converted first, then ErrorEventInit
                // members in Web IDL dictionary order.
                let colno = self.event_init_value(&options, "colno")?;
                let colno = self.event_integer(&colno, 32, false)?;
                state.set("colno", Value::Number(colno));
                let error = self.event_init_value(&options, "error")?;
                self.stack.push(error.clone());
                state.set("error", error);
                let filename = self.event_init_value(&options, "filename")?;
                let filename = if filename == Value::Undefined {
                    "".into()
                } else {
                    self.coerce_string(&filename)?
                };
                state.set(
                    "filename",
                    Value::String(String::from_utf16_lossy(filename.as_code_units()).into()),
                );
                let lineno = self.event_init_value(&options, "lineno")?;
                let lineno = self.event_integer(&lineno, 32, false)?;
                state.set("lineno", Value::Number(lineno));
                let message = self.event_init_value(&options, "message")?;
                let message = if message == Value::Undefined {
                    "".into()
                } else {
                    self.coerce_string(&message)?
                };
                state.set("message", Value::String(message));
            }
            let default = self.globals[&format!("%Web{}%", EVENT_NAMES[rank as usize])];
            let prototype = self.constructor_prototype_for(default, None)?;
            let event = self.with_roots(|heap| heap.alloc_drag_event(state, prototype))?;
            self.stack.push(Value::Object(event));
            self.finish_web_event(event)?;
            Ok(Value::Object(event))
        })();
        self.stack.truncate(base);
        result
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
        state: &mut DragEventState,
        options: &Value,
        field: &'static str,
    ) -> Result<(), RuntimeError> {
        let value = self.event_init_value(options, field)?;
        let number = if value == Value::Undefined {
            0.0
        } else {
            self.coerce_number(&value)?
        };
        if !number.is_finite() {
            return Err(RuntimeError::TypeError(
                "Event coordinate must be finite".into(),
            ));
        }
        state.set(field, Value::Number(number));
        Ok(())
    }

    fn initialize_web_event(
        &mut self,
        object: ObjectId,
        rank: u8,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        if !event_interface_matches(self.heap.drag_event(object)?.interface, rank) {
            return Err(RuntimeError::TypeError(
                "Invalid event interface receiver".into(),
            ));
        }
        if args.is_empty() {
            return Err(RuntimeError::TypeError("Event type is required".into()));
        }
        // Web IDL converts arguments before the algorithm checks dispatch
        // state. Do not retain an old state across conversions that run JS.
        let name = self.coerce_string(&args[0])?;
        let bubbles = self.to_boolean(native::argument(args, 1))?;
        let cancelable = self.to_boolean(native::argument(args, 2))?;
        let mut attributes = Vec::new();
        if rank >= 1 {
            let view = native::argument(args, 3);
            if *view != Value::Null && *view != Value::Undefined {
                return Err(RuntimeError::TypeError(
                    "Event view requires a Window".into(),
                ));
            }
            attributes.push(("view", Value::Null));
            let detail = self.event_integer(native::argument(args, 4), 32, true)?;
            attributes.push(("detail", Value::Number(detail)));
        }
        if rank >= 2 {
            // These legacy method arguments remain Web IDL long even when
            // MouseEventInit coordinates and getters use doubles.
            for (index, field) in [
                (5, "screenX"),
                (6, "screenY"),
                (7, "clientX"),
                (8, "clientY"),
            ] {
                let value = self.event_integer(native::argument(args, index), 32, true)?;
                attributes.push((field, Value::Number(value)));
            }
            for (index, field) in [
                (9, "ctrlKey"),
                (10, "altKey"),
                (11, "shiftKey"),
                (12, "metaKey"),
            ] {
                let value = self.to_boolean(native::argument(args, index))?;
                attributes.push((field, Value::Bool(value)));
            }
            let button = self.event_integer(native::argument(args, 13), 16, true)?;
            attributes.push(("button", Value::Number(button)));
            let related = native::argument(args, 14);
            let related = if *related == Value::Null || *related == Value::Undefined {
                Value::Null
            } else {
                let object = related
                    .object_id()
                    .filter(|object| self.is_host_event_target(*object))
                    .ok_or_else(|| {
                        RuntimeError::TypeError("relatedTarget requires an EventTarget".into())
                    })?;
                Value::Object(object)
            };
            attributes.push(("relatedTarget", related));
        }
        let mut state = self.heap.drag_event(object)?.clone();
        if state.phase != 0 {
            return Ok(Value::Undefined);
        }
        state.set("type", Value::String(name));
        state.set("bubbles", Value::Bool(bubbles));
        state.set("target", Value::Null);
        state.set("isTrusted", Value::Bool(false));
        state.cancelable = cancelable;
        state.default_prevented = false;
        state.propagation_stopped = false;
        state.immediate_stopped = false;
        for (field, value) in attributes {
            state.set(field, value);
        }
        self.with_roots(|heap| heap.set_drag_event_state(object, state))?;
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
        let mut state = self.heap.drag_event(object)?.clone();
        match native {
            EventNative::Get(name, rank) => {
                if !event_interface_matches(state.interface, rank) {
                    return Err(RuntimeError::TypeError(
                        "Invalid event interface receiver".into(),
                    ));
                }
                Ok(match name {
                    "currentTarget" => state.current_target.map_or(Value::Null, Value::Object),
                    "cancelable" => Value::Bool(state.cancelable),
                    "defaultPrevented" => Value::Bool(state.default_prevented),
                    "cancelBubble" => Value::Bool(state.propagation_stopped),
                    "returnValue" => Value::Bool(!state.default_prevented),
                    "eventPhase" => Value::Number(state.phase as f64),
                    "srcElement" => state.get("target"),
                    "x" => state.get("clientX"),
                    "y" => state.get("clientY"),
                    "pageX" | "offsetX" => {
                        if state.phase != 0 && state.get(name) != Value::Undefined {
                            state.get(name)
                        } else {
                            state.get("clientX")
                        }
                    }
                    "pageY" | "offsetY" => {
                        if state.phase != 0 && state.get(name) != Value::Undefined {
                            state.get(name)
                        } else {
                            state.get("clientY")
                        }
                    }
                    "which" => Value::Number(if state.interface >= 2 {
                        match state.get("button") {
                            Value::Number(0.0) => 1.0,
                            Value::Number(1.0) => 2.0,
                            Value::Number(2.0) => 3.0,
                            _ => 0.0,
                        }
                    } else {
                        0.0
                    }),
                    _ => state.get(name),
                })
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
                let key = self.coerce_string(&args[0])?;
                // Converting a key can run page code and mutate this event.
                let state = self.heap.drag_event(object)?;
                Ok(Value::Bool(MODIFIERS.iter().any(|(name, field)| {
                    key == *name && state.get(field) == Value::Bool(true)
                })))
            }
            EventNative::ComposedPath => {
                let prototype = self.array_prototype;
                let array = self.with_roots(|heap| {
                    heap.alloc_array(state.path.len() as u32, Some(prototype))
                })?;
                self.stack.push(Value::Object(array));
                let result = (|| {
                    for (index, target) in state.path.iter().enumerate() {
                        self.define_data(
                            array,
                            index.to_string(),
                            Value::Object(*target),
                            true,
                            true,
                            true,
                        )?;
                    }
                    Ok(Value::Object(array))
                })();
                self.stack.pop();
                result
            }
            EventNative::InitEvent => self.initialize_web_event(object, 0, args),
            EventNative::InitUIEvent => self.initialize_web_event(object, 1, args),
            EventNative::InitMouseEvent => self.initialize_web_event(object, 2, args),
            EventNative::InitCustomEvent => {
                if state.interface != 5 {
                    return Err(RuntimeError::TypeError(
                        "Invalid CustomEvent receiver".into(),
                    ));
                }
                self.initialize_web_event(object, 0, args)?;
                let mut state = self.heap.drag_event(object)?.clone();
                if state.phase == 0 {
                    let detail = native::argument(args, 3).clone();
                    state.set(
                        "detail",
                        if detail == Value::Undefined {
                            Value::Null
                        } else {
                            detail
                        },
                    );
                    self.with_roots(|heap| heap.set_drag_event_state(object, state))?;
                }
                Ok(Value::Undefined)
            }
            EventNative::PreventDefault
            | EventNative::StopPropagation(_)
            | EventNative::SetCancellation(_) => {
                match native {
                    EventNative::StopPropagation(immediate) => {
                        state.propagation_stopped = true;
                        state.immediate_stopped |= immediate;
                    }
                    EventNative::SetCancellation(true) => {
                        let stop = self.to_boolean(native::argument(args, 0))?;
                        state.propagation_stopped |= stop;
                    }
                    EventNative::SetCancellation(false) => {
                        let allow = self.to_boolean(native::argument(args, 0))?;
                        if !allow && state.cancelable && !state.passive {
                            state.default_prevented = true;
                        }
                    }
                    _ if state.cancelable && !state.passive => state.default_prevented = true,
                    _ => {}
                }
                self.with_roots(|heap| heap.set_drag_event_state(object, state))?;
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
