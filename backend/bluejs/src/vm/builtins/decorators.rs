// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Decorators (the TC39 decorators proposal, with its decorator-metadata
//! extension): the interpreter operations behind a decorated class.
//!
//! The compiler evaluates every decorator expression in source order while it
//! defines the class, collecting each element's decorators into an Array. Once
//! every element exists it emits one [`Vm::decorate_element`] per decorated
//! element (in source order), then the class decorators
//! ([`Vm::decorate_class`]), and finally runs the initializers the decorators
//! registered ([`Vm::run_initializers`], [`Vm::apply_initializers`]) at the
//! time the proposal specifies. Everything crosses the compiler/VM boundary
//! on the operand stack, so every value stays rooted while decorators (which
//! are arbitrary JavaScript) run.
//!
//! The state the proposal keeps in Abstract Closure captures (whether a
//! decorator application has finished, the initializer list it appends to,
//! where an `access` object reads) lives in small null-prototype heap records
//! that the two native function variants reference, so the collector traces
//! it and no JavaScript can see it. The Arrays this module builds for its own
//! bookkeeping are read and written through own data properties only, never
//! through `Array.prototype`, so user code cannot observe or interfere.

use super::*;
use crate::native::DecoratorAccessOp;

use crate::bytecode::decoration::{
    ACCESSOR, FIELD, GETTER, KIND_MASK, METHOD, PRIVATE, SETTER, STATIC,
};

/// The kind of class element a decorator is applied to (the low bits of a
/// `DecorateElement` / `ReplaceClassElement` operand).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElementKind {
    Method,
    Getter,
    Setter,
    Field,
    Accessor,
}

impl ElementKind {
    fn from_operand(operand: u32) -> Self {
        match operand & KIND_MASK {
            METHOD => Self::Method,
            GETTER => Self::Getter,
            SETTER => Self::Setter,
            FIELD => Self::Field,
            ACCESSOR => Self::Accessor,
            other => unreachable!("the compiler emits element kind {other}"),
        }
    }

    /// The `kind` string of the decorator context.
    fn name(self) -> &'static str {
        match self {
            Self::Method => "method",
            Self::Getter => "getter",
            Self::Setter => "setter",
            Self::Field => "field",
            Self::Accessor => "accessor",
        }
    }

    fn has_get(self) -> bool {
        !matches!(self, Self::Setter)
    }

    fn has_set(self) -> bool {
        matches!(self, Self::Setter | Self::Field | Self::Accessor)
    }
}

impl Vm {
    /// A fresh, empty Array used as an internal list. The result is unrooted;
    /// the caller must push it before allocating again.
    fn decorator_list(&mut self) -> Result<Value, RuntimeError> {
        let prototype = self.array_prototype;
        let id = self.with_roots(|heap| heap.alloc_array(0, Some(prototype)))?;
        Ok(Value::Object(id))
    }

    fn decorator_list_push(&mut self, list: &Value, value: &Value) -> Result<(), RuntimeError> {
        self.array_push(list, value, 0)
    }

    fn decorator_list_len(&self, list: &Value) -> Result<usize, RuntimeError> {
        let id = list
            .object_id()
            .expect("the compiler passes decorator lists as Arrays");
        match self.heap.get(id, "length")? {
            Value::Number(length) => Ok(length as usize),
            _ => Ok(0),
        }
    }

    fn decorator_list_get(&self, list: &Value, index: usize) -> Result<Value, RuntimeError> {
        let id = list
            .object_id()
            .expect("the compiler passes decorator lists as Arrays");
        Ok(self
            .heap
            .get_own(id, index.to_string())?
            .unwrap_or(Value::Undefined))
    }

    /// A native function whose `name` and `length` are the given ones. The
    /// result is unrooted; the caller must push it before allocating again.
    fn decorator_native_function(
        &mut self,
        function: NativeFunction,
        name: &str,
        length: u32,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.function_prototype()?;
        let id = self.with_roots(|heap| heap.alloc_native_function(function, name, prototype))?;
        self.stack.push(Value::Object(id));
        let result = (|| {
            self.define_data(
                id,
                "length",
                Value::Number(f64::from(length)),
                false,
                false,
                true,
            )?;
            self.define_data(id, "name", Value::String(name.into()), false, false, true)?;
            Ok(Value::Object(id))
        })();
        self.stack.pop();
        result
    }

    /// `CreateMetadata`: `F` -> `F, metadata`. The metadata object inherits
    /// from the metadata of the superclass (`F.[[Prototype]]`), when that is
    /// an object; a class without a superclass, or whose superclass has none,
    /// gets a null-prototype object.
    pub(in super::super) fn create_metadata(&mut self) -> Result<(), RuntimeError> {
        let class = self
            .stack
            .last()
            .and_then(Value::object_id)
            .expect("the compiler emits CreateMetadata with the class on the stack");
        let base = self.stack.len();
        let mut parent_metadata = None;
        if let Some(parent) = self.object_get_prototype(class)? {
            let inherited = self.get_property(
                &Value::Object(parent),
                &JsSymbol::well_known("metadata").into(),
            )?;
            // A getter may have produced a fresh object: keep it rooted
            // across the allocation below.
            self.stack.push(inherited.clone());
            parent_metadata = inherited.object_id();
        }
        let metadata = self.with_roots(|heap| heap.alloc_object(parent_metadata))?;
        self.stack.truncate(base);
        self.stack.push(Value::Object(metadata));
        Ok(())
    }

    /// `DefineMetadata`: `F, metadata` -> `F`.
    pub(in super::super) fn define_metadata(&mut self) -> Result<(), RuntimeError> {
        let base = self.stack.len() - 2;
        let class = self.stack[base]
            .object_id()
            .expect("the compiler emits DefineMetadata with the class on the stack");
        let metadata = self.stack[base + 1].clone();
        self.define_data(
            class,
            JsSymbol::well_known("metadata"),
            metadata,
            true,
            true,
            true,
        )?;
        self.stack.truncate(base + 1);
        Ok(())
    }

    /// Builds the context object of one decorator application and leaves it on
    /// the stack. `state` is that application's `finished`/initializers record.
    /// `element` is the operand base of a `DecorateElement`; `None` builds a
    /// class decorator's context, which has neither `static`, `private` nor
    /// `access`.
    fn decorator_context(
        &mut self,
        name: Value,
        metadata: Value,
        state: ObjectId,
        element: Option<(ElementKind, u32, Value, Value)>,
    ) -> Result<(), RuntimeError> {
        let object_prototype = self.object_prototype;
        let context = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        self.stack.push(Value::Object(context));
        let kind_name = element.as_ref().map_or("class", |(kind, ..)| kind.name());
        self.define_data(
            context,
            "kind",
            Value::String(kind_name.into()),
            true,
            true,
            true,
        )?;
        self.define_data(context, "name", name, true, true, true)?;
        if let Some((kind, flags, owner, key)) = element {
            self.define_data(
                context,
                "static",
                Value::Bool(flags & STATIC != 0),
                true,
                true,
                true,
            )?;
            self.define_data(
                context,
                "private",
                Value::Bool(flags & PRIVATE != 0),
                true,
                true,
                true,
            )?;
            let access = self.decorator_access_object(kind, owner, key)?;
            self.stack.push(access.clone());
            self.define_data(context, "access", access, true, true, true)?;
            self.stack.pop();
        }
        let add_initializer = self.decorator_native_function(
            NativeFunction::DecoratorAddInitializer { state },
            "addInitializer",
            1,
        )?;
        self.stack.push(add_initializer.clone());
        self.define_data(context, "addInitializer", add_initializer, true, true, true)?;
        self.stack.pop();
        self.define_data(context, "metadata", metadata, true, true, true)?;
        Ok(())
    }

    /// The `access` object of an element: `has` always, `get` for everything
    /// readable, `set` for everything writable. `owner` is the private-name
    /// owner (or `undefined` for a public element) and `key` the property key
    /// or private name the functions reach.
    fn decorator_access_object(
        &mut self,
        kind: ElementKind,
        owner: Value,
        key: Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            let object_prototype = self.object_prototype;
            let access = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
            self.stack.push(Value::Object(access));
            let state = self.promise_state()?;
            self.stack.push(Value::Object(state));
            self.promise_state_set(state, "owner", owner)?;
            self.promise_state_set(state, "key", key)?;
            for (name, op, length, present) in [
                ("has", DecoratorAccessOp::Has, 1, true),
                ("get", DecoratorAccessOp::Get, 1, kind.has_get()),
                ("set", DecoratorAccessOp::Set, 2, kind.has_set()),
            ] {
                if !present {
                    continue;
                }
                let function = self.decorator_native_function(
                    NativeFunction::DecoratorAccess { op, state },
                    name,
                    length,
                )?;
                self.stack.push(function.clone());
                self.define_data(access, name, function, true, true, true)?;
                self.stack.pop();
            }
            Ok(Value::Object(access))
        })();
        self.stack.truncate(base);
        result
    }

    /// Calls one decorator with `(value, context)` and `this` undefined, then
    /// marks its application finished (even when it threw), which disables
    /// its `addInitializer`. Both `value` and `context` are on the stack.
    fn call_decorator(
        &mut self,
        decorator: Value,
        value: Value,
        context: Value,
        state: ObjectId,
    ) -> Result<Value, RuntimeError> {
        let result = self.call_native(decorator, Value::Undefined, vec![value, context], false);
        self.promise_state_set(state, "finished", Value::Bool(true))?;
        result
    }

    /// One application record: the `finished` flag and the list extra
    /// initializers are appended to. Pushed on the stack.
    fn decorator_state(&mut self, extras: &Value) -> Result<ObjectId, RuntimeError> {
        let state = self.promise_state()?;
        self.stack.push(Value::Object(state));
        self.promise_state_set(state, "finished", Value::Bool(false))?;
        self.promise_state_set(state, "initializers", extras.clone())?;
        Ok(state)
    }

    fn decorator_is_callable(&self, value: &Value) -> Result<bool, RuntimeError> {
        self.is_callable(value)
    }

    /// `DecorateElement`. The operands, deepest first, are the element's
    /// decorator list, its `name` (a string or symbol; `#x` for a private
    /// name), the private-name owner and the private name (both `undefined`
    /// for a public element), the value being decorated (the function of a
    /// method or accessor half, the getter of an auto-accessor, `undefined`
    /// for a field), the setter of an auto-accessor (else `undefined`) and the
    /// class's metadata. It pushes the result record `[extraInitializers,
    /// value]` for a method or accessor half, `[extraInitializers,
    /// initializers]` for a field and `[extraInitializers, initializers,
    /// getter, setter]` for an auto-accessor.
    pub(in super::super) fn decorate_element(&mut self, flags: u32) -> Result<(), RuntimeError> {
        let kind = ElementKind::from_operand(flags);
        let base = self.stack.len() - 7;
        let decorators = self.stack[base].clone();
        let name = self.stack[base + 1].clone();
        let owner = self.stack[base + 2].clone();
        let private_name = self.stack[base + 3].clone();
        let metadata = self.stack[base + 6].clone();
        let (access_owner, access_key) = if flags & PRIVATE != 0 {
            (owner, private_name)
        } else {
            (Value::Undefined, name.clone())
        };
        // base + 4 and base + 5 are the running value(s): a decorator can
        // replace them, and the next decorator sees the replacement.
        let extras = self.decorator_list()?;
        self.stack.push(extras.clone());
        let initializers = self.decorator_list()?;
        self.stack.push(initializers.clone());
        for index in (0..self.decorator_list_len(&decorators)?).rev() {
            self.charge_step()?;
            let decorator = self.decorator_list_get(&decorators, index)?;
            if !self.decorator_is_callable(&decorator)? {
                return Err(RuntimeError::TypeError(
                    "a decorator must be a function".into(),
                ));
            }
            let frame = self.stack.len();
            let state = self.decorator_state(&extras)?;
            self.decorator_context(
                name.clone(),
                metadata.clone(),
                state,
                Some((kind, flags, access_owner.clone(), access_key.clone())),
            )?;
            let context = self.stack[self.stack.len() - 1].clone();
            let argument = if kind == ElementKind::Accessor {
                // A fresh `{ get, set }` for every call.
                let object_prototype = self.object_prototype;
                let object = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                self.stack.push(Value::Object(object));
                let (getter, setter) = (self.stack[base + 4].clone(), self.stack[base + 5].clone());
                self.define_data(object, "get", getter, true, true, true)?;
                self.define_data(object, "set", setter, true, true, true)?;
                Value::Object(object)
            } else {
                self.stack[base + 4].clone()
            };
            let result = self.call_decorator(decorator, argument, context, state)?;
            self.stack.push(result.clone());
            self.apply_element_result(kind, base, &initializers, &result)?;
            self.stack.truncate(frame);
        }
        let record = self.decorator_list()?;
        self.stack.push(record.clone());
        self.decorator_list_push(&record, &extras)?;
        match kind {
            ElementKind::Method | ElementKind::Getter | ElementKind::Setter => {
                let value = self.stack[base + 4].clone();
                self.decorator_list_push(&record, &value)?;
            }
            ElementKind::Field => self.decorator_list_push(&record, &initializers)?,
            ElementKind::Accessor => {
                self.decorator_list_push(&record, &initializers)?;
                for slot in [base + 4, base + 5] {
                    let value = self.stack[slot].clone();
                    self.decorator_list_push(&record, &value)?;
                }
            }
        }
        self.stack.truncate(base);
        self.stack.push(record);
        Ok(())
    }

    /// What a decorator's return value does for an element of `kind`: a method
    /// or accessor half is replaced by a returned function, a field gets a
    /// returned function as one more initializer, and an auto-accessor takes
    /// an optional `get`, `set` and `init` from a returned object. Anything
    /// else that is not `undefined` is a TypeError.
    fn apply_element_result(
        &mut self,
        kind: ElementKind,
        base: usize,
        initializers: &Value,
        result: &Value,
    ) -> Result<(), RuntimeError> {
        if matches!(result, Value::Undefined) {
            return Ok(());
        }
        match kind {
            ElementKind::Method | ElementKind::Getter | ElementKind::Setter => {
                if !self.decorator_is_callable(result)? {
                    return Err(RuntimeError::TypeError(
                        "a method, getter or setter decorator must return a function or undefined"
                            .into(),
                    ));
                }
                self.stack[base + 4] = result.clone();
            }
            ElementKind::Field => {
                if !self.decorator_is_callable(result)? {
                    return Err(RuntimeError::TypeError(
                        "a field decorator must return a function or undefined".into(),
                    ));
                }
                self.decorator_list_push(initializers, result)?;
            }
            ElementKind::Accessor => {
                if !matches!(result, Value::Object(_)) {
                    return Err(RuntimeError::TypeError(
                        "an accessor decorator must return an object or undefined".into(),
                    ));
                }
                for (property, slot) in [
                    ("get", Some(base + 4)),
                    ("set", Some(base + 5)),
                    ("init", None),
                ] {
                    let value = self.get_property(result, &property.into())?;
                    if matches!(value, Value::Undefined) {
                        continue;
                    }
                    if !self.decorator_is_callable(&value)? {
                        return Err(RuntimeError::TypeError(format!(
                            "an accessor decorator's `{property}` must be a function or undefined"
                        )));
                    }
                    match slot {
                        Some(slot) => self.stack[slot] = value,
                        None => self.decorator_list_push(initializers, &value)?,
                    }
                }
            }
        }
        Ok(())
    }

    /// `DecorateClass`: `F, decorators, name, metadata` -> `[extraInitializers,
    /// F']`, where `F'` is the class the last decorator to return a function
    /// produced (or `F`). The class is not a value the decorators can replace
    /// with a non-callable.
    pub(in super::super) fn decorate_class(&mut self) -> Result<(), RuntimeError> {
        let base = self.stack.len() - 4;
        let decorators = self.stack[base + 1].clone();
        let name = self.stack[base + 2].clone();
        let metadata = self.stack[base + 3].clone();
        let extras = self.decorator_list()?;
        self.stack.push(extras.clone());
        for index in (0..self.decorator_list_len(&decorators)?).rev() {
            self.charge_step()?;
            let decorator = self.decorator_list_get(&decorators, index)?;
            if !self.decorator_is_callable(&decorator)? {
                return Err(RuntimeError::TypeError(
                    "a decorator must be a function".into(),
                ));
            }
            let frame = self.stack.len();
            let state = self.decorator_state(&extras)?;
            self.decorator_context(name.clone(), metadata.clone(), state, None)?;
            let context = self.stack[self.stack.len() - 1].clone();
            let class = self.stack[base].clone();
            let result = self.call_decorator(decorator, class, context, state)?;
            if !matches!(result, Value::Undefined) {
                if !self.decorator_is_callable(&result)? {
                    return Err(RuntimeError::TypeError(
                        "a class decorator must return a function or undefined".into(),
                    ));
                }
                self.stack[base] = result;
            }
            self.stack.truncate(frame);
        }
        let record = self.decorator_list()?;
        self.stack.push(record.clone());
        self.decorator_list_push(&record, &extras)?;
        let class = self.stack[base].clone();
        self.decorator_list_push(&record, &class)?;
        self.stack.truncate(base);
        self.stack.push(record);
        Ok(())
    }

    /// `addInitializer(initializer)` of a decorator context: allowed only
    /// while the decorator that received the context is still running.
    pub(in super::super) fn decorator_add_initializer(
        &mut self,
        state: ObjectId,
        initializer: &Value,
    ) -> Result<Value, RuntimeError> {
        if matches!(
            self.promise_state_get(state, "finished")?,
            Value::Bool(true)
        ) {
            return Err(RuntimeError::TypeError(
                "addInitializer cannot be called after the decorator has finished".into(),
            ));
        }
        if !self.decorator_is_callable(initializer)? {
            return Err(RuntimeError::TypeError(
                "an initializer must be a function".into(),
            ));
        }
        let list = self.promise_state_get(state, "initializers")?;
        self.decorator_list_push(&list, initializer)?;
        Ok(Value::Undefined)
    }

    /// `access.get(object)`, `access.set(object, value)` and
    /// `access.has(object)`: the element as seen on `object`, through the
    /// private name (when the element is private) or the property key.
    pub(in super::super) fn decorator_access(
        &mut self,
        op: DecoratorAccessOp,
        state: ObjectId,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let owner = self.promise_state_get(state, "owner")?;
        let key = self.promise_state_get(state, "key")?;
        let receiver = native::argument(args, 0).clone();
        let Some(object) = receiver.object_id() else {
            return Err(RuntimeError::TypeError(
                "a decorator's access functions require an object".into(),
            ));
        };
        if let Some(owner) = owner.object_id() {
            let Value::String(name) = key else {
                unreachable!("a private name is a string");
            };
            return match op {
                DecoratorAccessOp::Get => self.private_get(&receiver, owner, &name),
                DecoratorAccessOp::Set => {
                    let value = native::argument(args, 1).clone();
                    self.private_set(&receiver, owner, name, value)?;
                    Ok(Value::Undefined)
                }
                DecoratorAccessOp::Has => {
                    Ok(Value::Bool(self.heap.has_private_brand(object, owner)?))
                }
            };
        }
        let key = self.coerce_property_key(&key)?;
        match op {
            DecoratorAccessOp::Get => self.get_property(&receiver, &key),
            DecoratorAccessOp::Set => {
                // Set(object, key, value, true): a failed assignment throws
                // whatever the caller's strictness.
                let value = native::argument(args, 1).clone();
                let outer = std::mem::replace(&mut self.strict, true);
                let result = self.set_property(&receiver, &key, &value);
                self.strict = outer;
                result?;
                Ok(Value::Undefined)
            }
            DecoratorAccessOp::Has => Ok(Value::Bool(self.has_property(object, &key)?)),
        }
    }

    /// `ReplaceClassElement`: `target, key, original, replacement` -> nothing.
    /// Puts the decorated function where the class definition put `original`.
    /// A public element is replaced only while it is still the one that
    /// definition made: a later element with the same key wins, exactly as if
    /// every definition had waited for the decorators. A private element's
    /// name is unique to it (an accessor pair's halves are separate), so it is
    /// always replaced. Property attributes are the ones the class definition
    /// gave `original`, and the replacement keeps its own `[[HomeObject]]`.
    pub(in super::super) fn replace_class_element(
        &mut self,
        flags: u32,
    ) -> Result<(), RuntimeError> {
        let base = self.stack.len() - 4;
        let target = self.stack[base]
            .object_id()
            .expect("the compiler passes the class target object");
        let key = self.stack[base + 1].clone();
        let original = self.stack[base + 2].clone();
        let replacement = self.stack[base + 3].clone();
        let kind = ElementKind::from_operand(flags);
        if same_value(&original, &replacement) {
            self.stack.truncate(base);
            return Ok(());
        }
        if flags & PRIVATE != 0 {
            let Value::String(name) = key else {
                unreachable!("a private name is a string");
            };
            self.with_roots(|heap| match kind {
                ElementKind::Getter => {
                    heap.define_private_accessor(target, name, replacement, false)
                }
                ElementKind::Setter => {
                    heap.define_private_accessor(target, name, replacement, true)
                }
                _ => heap.define_private_method(target, name, replacement),
            })?;
        } else {
            let key = self.coerce_property_key(&key)?;
            let current = self.heap.get_own_property_descriptor(target, key.clone())?;
            let still_original = current.is_some_and(|descriptor| {
                let holder = match kind {
                    ElementKind::Getter => descriptor.get,
                    ElementKind::Setter => descriptor.set,
                    _ => descriptor.value,
                };
                holder.is_some_and(|value| same_value(&value, &original))
            });
            if still_original {
                let descriptor = match kind {
                    ElementKind::Getter => PropertyDescriptor {
                        get: Some(replacement),
                        enumerable: Some(false),
                        configurable: Some(true),
                        ..Default::default()
                    },
                    ElementKind::Setter => PropertyDescriptor {
                        set: Some(replacement),
                        enumerable: Some(false),
                        configurable: Some(true),
                        ..Default::default()
                    },
                    _ => PropertyDescriptor::data(replacement, true, false, true),
                };
                if !self.with_roots(|heap| heap.define_own_property(target, key, descriptor))? {
                    return Err(RuntimeError::TypeError(
                        "cannot redefine a decorated class element".into(),
                    ));
                }
            }
        }
        self.stack.truncate(base);
        Ok(())
    }

    /// `RunInitializers`: `receiver, initializers` -> nothing.
    pub(in super::super) fn run_initializers(&mut self) -> Result<(), RuntimeError> {
        let base = self.stack.len() - 2;
        let receiver = self.stack[base].clone();
        let list = self.stack[base + 1].clone();
        for index in 0..self.decorator_list_len(&list)? {
            self.charge_step()?;
            let initializer = self.decorator_list_get(&list, index)?;
            self.call_native(initializer, receiver.clone(), Vec::new(), false)?;
        }
        self.stack.truncate(base);
        Ok(())
    }

    /// `ApplyInitializers`: `value, receiver, initializers` -> `value'`, the
    /// value passed through each initializer in turn with `this` = receiver.
    pub(in super::super) fn apply_initializers(&mut self) -> Result<(), RuntimeError> {
        let base = self.stack.len() - 3;
        let receiver = self.stack[base + 1].clone();
        let list = self.stack[base + 2].clone();
        for index in 0..self.decorator_list_len(&list)? {
            self.charge_step()?;
            let initializer = self.decorator_list_get(&list, index)?;
            let value = self.stack[base].clone();
            let next = self.call_native(initializer, receiver.clone(), vec![value], false)?;
            self.stack[base] = next;
        }
        self.stack.truncate(base + 1);
        Ok(())
    }
}
