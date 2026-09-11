// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::heap::{
    same_value, AsyncGeneratorCompletion, AsyncGeneratorDelegate, AsyncGeneratorRequest,
    AsyncGeneratorStatus, GeneratorState, TypedArrayKind, TypedArrayNumericKey,
};
use crate::native::{MathMethod, ObjectMethod, PatternMethod, StringMethod};
use std::collections::HashMap;
use std::rc::Rc;

pub(super) struct ClosureCall {
    pub code: Rc<Bytecode>,
    pub captures: Vec<ObjectId>,
    pub callee: Value,
    pub receiver: Value,
    pub args: Vec<Value>,
    pub construct: bool,
    pub home: Option<ObjectId>,
    pub class_base: Option<Value>,
}

fn array_index_below_length(key: &PropertyName, length: u64) -> Option<u32> {
    let PropertyName::String(name) = key else {
        return None;
    };
    let name = name.to_utf8().ok()?;
    let index = name.parse::<u32>().ok()?;
    (name == index.to_string() && u64::from(index) < length).then_some(index)
}

fn same_value_zero(left: &Value, right: &Value) -> bool {
    left == right
        || matches!((left, right), (Value::Number(left), Value::Number(right)) if left.is_nan() && right.is_nan())
}

/// Validate the non-mutating part of ValidateAndApplyPropertyDescriptor.
/// Heap::define_own_property performs the corresponding mutation for ordinary
/// objects; Proxy trap invariants need the same answer before a trap result is
/// allowed to claim success.
fn compatible_property_descriptor(
    extensible: bool,
    current: Option<&PropertyDescriptor>,
    descriptor: &PropertyDescriptor,
) -> bool {
    let Some(current) = current else {
        return extensible;
    };
    if descriptor.value.is_none()
        && descriptor.writable.is_none()
        && descriptor.get.is_none()
        && descriptor.set.is_none()
        && descriptor.enumerable.is_none()
        && descriptor.configurable.is_none()
    {
        return true;
    }
    if current.configurable == Some(false) {
        if descriptor.configurable == Some(true)
            || descriptor
                .enumerable
                .is_some_and(|value| Some(value) != current.enumerable)
        {
            return false;
        }
        let descriptor_is_data = descriptor.value.is_some() || descriptor.writable.is_some();
        let descriptor_is_accessor = descriptor.accessor();
        if (descriptor_is_data && current.accessor())
            || (descriptor_is_accessor && !current.accessor())
        {
            return false;
        }
        if current.accessor() {
            if descriptor.get.as_ref().is_some_and(|value| {
                current
                    .get
                    .as_ref()
                    .is_none_or(|current| !same_value(value, current))
            }) || descriptor.set.as_ref().is_some_and(|value| {
                current
                    .set
                    .as_ref()
                    .is_none_or(|current| !same_value(value, current))
            }) {
                return false;
            }
        } else if current.writable == Some(false)
            && (descriptor.writable == Some(true)
                || descriptor.value.as_ref().is_some_and(|value| {
                    current
                        .value
                        .as_ref()
                        .is_none_or(|current| !same_value(value, current))
                }))
        {
            return false;
        }
    }
    true
}

/// Proxy [[GetOwnProperty]] completes a trap-provided descriptor before it
/// validates invariants or exposes it to reflection.  Missing data/accessor
/// fields are observable as `undefined`/`false`, never as absent own fields
/// on the descriptor object returned by Object.getOwnPropertyDescriptor.
fn complete_property_descriptor(mut descriptor: PropertyDescriptor) -> PropertyDescriptor {
    if descriptor.accessor() {
        descriptor.get.get_or_insert(Value::Undefined);
        descriptor.set.get_or_insert(Value::Undefined);
    } else {
        descriptor.value.get_or_insert(Value::Undefined);
        descriptor.writable.get_or_insert(false);
    }
    descriptor.enumerable.get_or_insert(false);
    descriptor.configurable.get_or_insert(false);
    descriptor
}

fn typed_array_kind(name: &str) -> Option<TypedArrayKind> {
    Some(match name {
        "Int8Array" => TypedArrayKind::Int8,
        "Uint8Array" => TypedArrayKind::Uint8,
        "Uint8ClampedArray" => TypedArrayKind::Uint8Clamped,
        "Int16Array" => TypedArrayKind::Int16,
        "Uint16Array" => TypedArrayKind::Uint16,
        "Int32Array" => TypedArrayKind::Int32,
        "Uint32Array" => TypedArrayKind::Uint32,
        "Float32Array" => TypedArrayKind::Float32,
        "Float64Array" => TypedArrayKind::Float64,
        _ => return None,
    })
}

fn data_view_number(bytes: &[u8], signed: bool, floating: bool, little_endian: bool) -> f64 {
    if floating {
        return match (bytes.len(), little_endian) {
            (4, true) => f32::from_le_bytes(bytes.try_into().unwrap()) as f64,
            (4, false) => f32::from_be_bytes(bytes.try_into().unwrap()) as f64,
            (8, true) => f64::from_le_bytes(bytes.try_into().unwrap()),
            (8, false) => f64::from_be_bytes(bytes.try_into().unwrap()),
            _ => unreachable!("DataView floating-point access is 32 or 64 bits"),
        };
    }
    match (bytes.len(), signed, little_endian) {
        (1, true, _) => i8::from_ne_bytes([bytes[0]]) as f64,
        (1, false, _) => bytes[0] as f64,
        (2, true, true) => i16::from_le_bytes(bytes.try_into().unwrap()) as f64,
        (2, false, true) => u16::from_le_bytes(bytes.try_into().unwrap()) as f64,
        (2, true, false) => i16::from_be_bytes(bytes.try_into().unwrap()) as f64,
        (2, false, false) => u16::from_be_bytes(bytes.try_into().unwrap()) as f64,
        (4, true, true) => i32::from_le_bytes(bytes.try_into().unwrap()) as f64,
        (4, false, true) => u32::from_le_bytes(bytes.try_into().unwrap()) as f64,
        (4, true, false) => i32::from_be_bytes(bytes.try_into().unwrap()) as f64,
        (4, false, false) => u32::from_be_bytes(bytes.try_into().unwrap()) as f64,
        _ => unreachable!("DataView only installs fixed integer widths"),
    }
}

fn data_view_bytes(
    value: f64,
    width: usize,
    signed: bool,
    floating: bool,
    little_endian: bool,
) -> Vec<u8> {
    if floating {
        return match (width, little_endian) {
            (4, true) => (value as f32).to_le_bytes().to_vec(),
            (4, false) => (value as f32).to_be_bytes().to_vec(),
            (8, true) => value.to_le_bytes().to_vec(),
            (8, false) => value.to_be_bytes().to_vec(),
            _ => unreachable!("DataView floating-point access is 32 or 64 bits"),
        };
    }
    let integer = (if value.is_finite() {
        value.trunc()
    } else {
        0.0
    }) as i64;
    match (width, signed, little_endian) {
        (1, true, _) => (integer as i8).to_ne_bytes().to_vec(),
        (1, false, _) => (integer as u8).to_ne_bytes().to_vec(),
        (2, true, true) => (integer as i16).to_le_bytes().to_vec(),
        (2, false, true) => (integer as u16).to_le_bytes().to_vec(),
        (2, true, false) => (integer as i16).to_be_bytes().to_vec(),
        (2, false, false) => (integer as u16).to_be_bytes().to_vec(),
        (4, true, true) => (integer as i32).to_le_bytes().to_vec(),
        (4, false, true) => (integer as u32).to_le_bytes().to_vec(),
        (4, true, false) => (integer as i32).to_be_bytes().to_vec(),
        (4, false, false) => (integer as u32).to_be_bytes().to_vec(),
        _ => unreachable!("DataView only installs fixed integer widths"),
    }
}

impl Vm {
    /// Materializes the per-invocation arguments binding after the function
    /// environment has entered. Mapped indices point at the same heap cells
    /// as simple sloppy parameter bindings; every other index remains an
    /// ordinary data property copied from the argument list.
    pub(super) fn create_arguments_object(&mut self, code: &Bytecode) -> Result<(), RuntimeError> {
        let slot = code
            .arguments_slot
            .expect("ArgumentsObject is emitted only for a function binding")
            as usize;
        let base = self.stack.len();
        self.stack.extend(self.arguments.iter().cloned());
        self.stack.push(self.callee.clone());
        let result = (|| {
            let mut parameter_map = HashMap::new();
            if code.arguments_mapped {
                for (index, parameter_slot) in code.arguments_mapped_slots.iter().enumerate() {
                    if let Some(parameter_slot) = parameter_slot {
                        let cell = self.capture(*parameter_slot as usize)?;
                        // Cells are otherwise reachable only through this
                        // frame until the exotic object has been allocated.
                        self.stack.push(Value::Object(cell));
                        parameter_map.insert(index.to_string().into(), cell);
                    }
                }
            }
            let object_prototype = self.object_prototype;
            let object =
                self.with_roots(|heap| heap.alloc_arguments(parameter_map, object_prototype))?;
            self.stack.push(Value::Object(object));
            self.define_data(
                object,
                "length",
                Value::Number(self.arguments.len() as f64),
                true,
                false,
                true,
            )?;
            for (index, value) in self.arguments.clone().into_iter().enumerate() {
                self.define_data(object, index.to_string(), value, true, true, true)?;
            }
            let array = self.global("Array")?;
            let array_prototype = self.get_property(&array, &"prototype".into())?;
            let iterator =
                self.get_property(&array_prototype, &JsSymbol::well_known("iterator").into())?;
            self.define_data(
                object,
                JsSymbol::well_known("iterator"),
                iterator,
                true,
                false,
                true,
            )?;
            if code.arguments_mapped {
                self.define_data(object, "callee", self.callee.clone(), true, false, true)?;
            } else {
                let thrower = self.throw_type_error()?;
                let descriptor = PropertyDescriptor {
                    get: Some(Value::Object(thrower)),
                    set: Some(Value::Object(thrower)),
                    enumerable: Some(false),
                    configurable: Some(false),
                    ..PropertyDescriptor::default()
                };
                let defined =
                    self.with_roots(|heap| heap.define_own_property(object, "callee", descriptor))?;
                assert!(defined, "new arguments object accepts its callee accessor");
            }
            self.store_binding(slot, Value::Object(object))
        })();
        self.stack.truncate(base);
        result
    }

    fn throw_type_error(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(function) = self.throw_type_error {
            return Ok(function);
        }
        let constructor = self.string_intrinsics()?.0;
        let prototype = self
            .heap
            .prototype(constructor)?
            .expect("String constructor has Function.prototype");
        let function = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::ThrowTypeError, "", prototype)
        })?;
        let root = self.heap.root(function)?;
        let result = (|| {
            self.define_data(
                function,
                "name",
                Value::String("".into()),
                false,
                false,
                true,
            )?;
            self.define_data(function, "length", Value::Number(0.0), false, false, true)?;
            Ok(function)
        })();
        match result {
            Ok(function) => {
                self.throw_type_error = Some(function);
                Ok(function)
            }
            Err(error) => {
                self.heap.unroot(root)?;
                Err(error)
            }
        }
    }

    pub(super) fn direct_eval(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let Value::String(source) = value else {
            return Ok(value.clone());
        };
        let source = source.to_utf8().map_err(|_| {
            RuntimeError::SyntaxError("eval source contains an unpaired surrogate".into())
        })?;
        let program =
            crate::parse_eval(&source).map_err(|error| RuntimeError::SyntaxError(error.message))?;
        let derived_constructor = match self.class_constructor {
            Some(constructor) => self.heap.class_base(constructor)?.is_some(),
            None => false,
        };
        if crate::ast::contains_super_call_outside_class(&program)
            && (self.class_field_initializer_depth != 0 || !derived_constructor)
        {
            return Err(RuntimeError::SyntaxError(
                "super() is not valid in this eval context".into(),
            ));
        }
        if crate::ast::contains_super_property_outside_class(&program) && self.home_object.is_none()
        {
            return Err(RuntimeError::SyntaxError(
                "super property is not valid in this eval context".into(),
            ));
        }
        let visible = self.eval_visible_bindings();
        let global_execution = self.callee == Value::Undefined;
        // The persistent global-realm path uses its existing binding cells
        // for direct eval declarations. Function eval instead distinguishes
        // the immediate VariableEnvironment from captured outer cells.
        let variable_environment_names = if global_execution {
            visible.iter().map(|(name, _, _)| name.clone()).collect()
        } else {
            self.eval_variable_environment_names()
        };
        let mut lexical_conflicts = self.eval_lexical_conflicts();
        if global_execution {
            lexical_conflicts.extend(
                self.global_bindings
                    .iter()
                    .filter(|(_, binding)| !binding.property)
                    .map(|(name, _)| name.clone()),
            );
        }
        let code = crate::compiler::compile_eval(
            &program,
            &visible,
            &variable_environment_names,
            &lexical_conflicts,
            self.strict,
            self.new_target_allowed,
            self.with_objects.len(),
        )
        .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        let captures = code
            .captures
            .iter()
            .map(|slot| self.capture(*slot as usize))
            .collect::<Result<Vec<_>, _>>()?;
        // A sloppy direct eval inherits the caller's VariableEnvironment.
        // The absence of a current function identifies the realm's global
        // execution context. Give eval the global `this` even when the outer
        // script has not observed it yet, and publish `var` bindings there.
        // Strict eval always receives its own VariableEnvironment.
        if global_execution && self.this == Value::Undefined {
            self.this = self.global("globalThis")?;
        }
        let global_var_environment = !code.strict && global_execution;
        self.execute_eval(&code, captures, global_var_environment)
    }

    /// Indirect eval starts from the realm global environment. It never
    /// captures caller bindings, even when the caller itself is strict.
    fn indirect_eval(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let Value::String(source) = value else {
            return Ok(value.clone());
        };
        let source = source.to_utf8().map_err(|_| {
            RuntimeError::SyntaxError("eval source contains an unpaired surrogate".into())
        })?;
        let program =
            crate::parse(&source).map_err(|error| RuntimeError::SyntaxError(error.message))?;
        let code = crate::compiler::compile_eval(&program, &[], &[], &[], false, false, 0)
            .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        let global_this = self.global("globalThis")?;
        let this = std::mem::replace(&mut self.this, global_this);
        let dynamic_eval_bindings = std::mem::take(&mut self.dynamic_eval_bindings);
        let dynamic_eval_outer_bindings = std::mem::take(&mut self.dynamic_eval_outer_bindings);
        let result = self.execute_eval(&code, Vec::new(), !code.strict);
        debug_assert!(self.dynamic_eval_bindings.is_empty());
        debug_assert!(self.dynamic_eval_outer_bindings.is_empty());
        self.dynamic_eval_bindings = dynamic_eval_bindings;
        self.dynamic_eval_outer_bindings = dynamic_eval_outer_bindings;
        self.this = this;
        result
    }

    pub(super) fn is_intrinsic_eval(&self, value: &Value) -> Result<bool, RuntimeError> {
        let Some(object) = value.object_id() else {
            return Ok(false);
        };
        Ok(self.heap.native_function(object)? == Some(NativeFunction::Eval))
    }

    pub(super) fn array_push(
        &mut self,
        array: &Value,
        value: &Value,
        kind: usize,
    ) -> Result<(), RuntimeError> {
        let id = array.object_id().unwrap();
        if kind == 2 {
            let record = self.get_iterator(value)?;
            self.stack.push(record.clone());
            while let Some(value) = self.iterator_step(&record, true)? {
                self.charge_step()?;
                self.array_push(array, &value, 0)?;
            }
            self.stack.pop();
        } else {
            let Value::Number(length) = self.heap.get(id, "length")? else {
                unreachable!()
            };
            if kind == 1 {
                self.with_roots(|heap| heap.set(id, "length", Value::Number(length + 1.0)))?;
            } else {
                self.with_roots(|heap| heap.set(id, (length as u32).to_string(), value.clone()))?;
                self.with_roots(|heap| heap.set(id, "length", Value::Number(length + 1.0)))?;
            }
        }
        Ok(())
    }

    fn array_iterator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.array_iterator_prototype {
            return Ok(prototype);
        }
        let constructor = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(constructor)?.unwrap();
        let base = self.base_iterator_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(base)))?;
        let root = self.heap.root(prototype)?;
        let result = (|| {
            self.install_native(
                prototype,
                function_prototype,
                "next",
                0,
                NativeFunction::ArrayIteratorNext,
            )?;
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("Array Iterator".into()),
                false,
                false,
                true,
            )?;
            Ok(prototype)
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.array_iterator_prototype = Some(prototype);
        }
        result
    }

    pub(super) fn generator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.generator_prototype {
            return Ok(prototype);
        }
        let constructor = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(constructor)?.unwrap();
        let base = self.base_iterator_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(base)))?;
        let root = self.heap.root(prototype)?;
        let result = (|| {
            self.install_native(
                prototype,
                function_prototype,
                "next",
                1,
                NativeFunction::GeneratorNext,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "return",
                1,
                NativeFunction::GeneratorReturn,
            )?;
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("Generator".into()),
                false,
                false,
                true,
            )?;
            Ok(prototype)
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.generator_prototype = Some(prototype);
        }
        result
    }

    /// `%AsyncIteratorPrototype%` has no global binding. It is the common
    /// parent of async-generator iterator prototypes and supplies the
    /// `@@asyncIterator` identity method used by `for await` later on.
    fn async_iterator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.async_iterator_base {
            return Ok(prototype);
        }
        let object_prototype = self.object_prototype;
        let function_prototype = self.function_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(prototype)?;
        let result = self.install_symbol_native(
            prototype,
            function_prototype,
            "asyncIterator",
            0,
            NativeFunction::AsyncIteratorSelf,
        );
        if let Err(error) = result {
            self.heap.unroot(root)?;
            Err(error)
        } else {
            self.async_iterator_base = Some(prototype);
            Ok(prototype)
        }
    }

    /// The shared `%AsyncGeneratorPrototype%`. Individual async-generator
    /// functions receive their own `.prototype` object above this base, just
    /// like ordinary generator functions do.
    pub(super) fn async_generator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.async_generator_prototype {
            return Ok(prototype);
        }
        let base = self.async_iterator_prototype()?;
        let function_prototype = self.function_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(base)))?;
        let root = self.heap.root(prototype)?;
        let result = (|| {
            self.install_native(
                prototype,
                function_prototype,
                "next",
                1,
                NativeFunction::AsyncGeneratorNext,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "return",
                1,
                NativeFunction::AsyncGeneratorReturn,
            )?;
            self.install_native(
                prototype,
                function_prototype,
                "throw",
                1,
                NativeFunction::AsyncGeneratorThrow,
            )?;
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("AsyncGenerator".into()),
                false,
                false,
                true,
            )
        })();
        if let Err(error) = result {
            self.heap.unroot(root)?;
            Err(error)
        } else {
            self.async_generator_prototype = Some(prototype);
            Ok(prototype)
        }
    }

    /// Lazily creates `%AsyncFunction%` and `%AsyncFunction.prototype%`.
    ///
    /// Async function objects inherit from the latter, which in turn inherits
    /// from `%Function.prototype%`. `%AsyncFunction%` itself inherits from
    /// `%Function%`, is reachable through `AsyncFunction.prototype.constructor`,
    /// and intentionally has no global binding.
    pub(super) fn async_function_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.async_function_prototype {
            return Ok(prototype);
        }
        let function_constructor = self
            .global("Function")?
            .object_id()
            .expect("Function is callable");
        let function_prototype = self.function_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(function_prototype)))?;
        let root = self.heap.root(prototype)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(prototype));
        let result: Result<(), RuntimeError> = (|| {
            let constructor = self.with_roots(|heap| {
                heap.alloc_native_function(
                    NativeFunction::AsyncFunction,
                    "AsyncFunction",
                    function_constructor,
                )
            })?;
            self.stack.push(Value::Object(constructor));
            self.define_data(
                constructor,
                "name",
                Value::String("AsyncFunction".into()),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "length",
                Value::Number(1.0),
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
                false,
                false,
                true,
            )?;
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("AsyncFunction".into()),
                false,
                false,
                true,
            )
        })();
        self.stack.truncate(base);
        match result {
            Ok(()) => {
                self.async_function_prototype = Some(prototype);
                Ok(prototype)
            }
            Err(error) => {
                self.heap.unroot(root)?;
                Err(error)
            }
        }
    }
    /// [[GetOwnProperty]] dispatch used by descriptor APIs, Proxy invariants,
    /// and receiver-aware [[Set]].  Ordinary heap records stay below this
    /// boundary; every Proxy operation re-enters through the VM so its trap
    /// may call JavaScript while roots remain registered.
    pub(super) fn object_get_own_property(
        &mut self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Result<Option<PropertyDescriptor>, RuntimeError> {
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_get_own_property(object, key);
        }
        self.heap
            .get_own_property_descriptor(object, key)
            .map_err(Into::into)
    }

    pub(super) fn object_define_own_property(
        &mut self,
        object: ObjectId,
        key: PropertyName,
        descriptor: PropertyDescriptor,
    ) -> Result<bool, RuntimeError> {
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_define_own_property(object, key, descriptor);
        }
        if let Some(numeric) = self.heap.typed_array_numeric_key(object, &key)? {
            return self.typed_array_define_own_property(object, numeric, descriptor);
        }
        self.with_roots(|heap| heap.define_own_property(object, key, descriptor))
    }

    /// IntegerIndexedElementSet and the compatible portion of
    /// IntegerIndexedObject.[[DefineOwnProperty]].  A canonical numeric key
    /// never becomes an ordinary property, even when it is invalid or outside
    /// the fixed view range.
    fn typed_array_define_own_property(
        &mut self,
        object: ObjectId,
        numeric: TypedArrayNumericKey,
        descriptor: PropertyDescriptor,
    ) -> Result<bool, RuntimeError> {
        let TypedArrayNumericKey::Index(index) = numeric else {
            return Ok(false);
        };
        let (buffer, _, length, _) = self.heap.typed_array_info(object)?;
        if self.heap.array_buffer_is_detached(buffer)? || index >= length {
            return Ok(false);
        }
        if descriptor.accessor()
            || descriptor.configurable == Some(false)
            || descriptor.enumerable == Some(false)
            || descriptor.writable == Some(false)
        {
            return Ok(false);
        }
        let Some(value) = descriptor.value else {
            return Ok(true);
        };
        let value = self.coerce_number(&value)?;
        // IntegerIndexedElementSet converts first. A conversion may detach the
        // backing buffer; in that case the already-valid DefineOwnProperty
        // operation still succeeds without writing a byte.
        if self.heap.array_buffer_is_detached(buffer)? {
            return Ok(true);
        }
        self.with_roots(|heap| heap.typed_array_set_index(object, index, value))
    }

    pub(super) fn object_delete(
        &mut self,
        object: ObjectId,
        key: &PropertyName,
    ) -> Result<bool, RuntimeError> {
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_delete(object, key);
        }
        self.heap.delete(object, key).map_err(Into::into)
    }

    pub(super) fn object_own_property_keys(
        &mut self,
        object: ObjectId,
    ) -> Result<Vec<PropertyName>, RuntimeError> {
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_own_keys(object);
        }
        self.heap.own_property_keys(object).map_err(Into::into)
    }

    pub(super) fn object_is_extensible(&mut self, object: ObjectId) -> Result<bool, RuntimeError> {
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_is_extensible(object);
        }
        self.heap.is_extensible(object).map_err(Into::into)
    }

    pub(super) fn object_get_prototype(
        &mut self,
        object: ObjectId,
    ) -> Result<Option<ObjectId>, RuntimeError> {
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_get_prototype(object);
        }
        self.heap.prototype(object).map_err(Into::into)
    }

    pub(super) fn object_set_prototype(
        &mut self,
        object: ObjectId,
        prototype: Option<ObjectId>,
    ) -> Result<bool, RuntimeError> {
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_set_prototype(object, prototype);
        }
        match self.heap.set_prototype(object, prototype) {
            Ok(()) => Ok(true),
            Err(HeapError::ReadOnlyProperty | HeapError::PrototypeCycle) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn object_prevent_extensions(
        &mut self,
        object: ObjectId,
    ) -> Result<bool, RuntimeError> {
        if self.heap.proxy(object)?.is_some() {
            return self.proxy_prevent_extensions(object);
        }
        self.heap.prevent_extensions(object)?;
        Ok(true)
    }

    /// OrdinarySet with an explicit receiver.  It is deliberately expressed
    /// in terms of the object-operation boundary above, so a Proxy can occur
    /// either as the target or in the prototype chain without being skipped.
    pub(super) fn ordinary_set_with_receiver(
        &mut self,
        target: ObjectId,
        receiver: &Value,
        key: &PropertyName,
        value: &Value,
    ) -> Result<bool, RuntimeError> {
        if self.heap.proxy(target)?.is_some() {
            return self.proxy_set(target, receiver, key, value);
        }
        if let Some(numeric) = self.heap.typed_array_numeric_key(target, key)? {
            let valid = match numeric {
                TypedArrayNumericKey::Index(index) => {
                    self.heap.typed_array_index_value(target, index)?.is_some()
                }
                TypedArrayNumericKey::Invalid => false,
            };
            if receiver == &Value::Object(target) {
                // TypedArraySetElement performs ToNumber before checking
                // IsValidIntegerIndex. Thus an own assignment to `"-0"`, a
                // fractional canonical key, or an out-of-bounds index still
                // observes a throwing value conversion. A different receiver
                // has the separate OrdinarySet path below and must *not*
                // convert an invalid key's value.
                let value = self.coerce_number(value)?;
                if valid {
                    let TypedArrayNumericKey::Index(index) = numeric else {
                        unreachable!("valid TypedArray index has an integer index")
                    };
                    // Conversion can detach the buffer, in which case this
                    // successful [[Set]] performs no byte write.
                    let (buffer, _, _, _) = self.heap.typed_array_info(target)?;
                    if !self.heap.array_buffer_is_detached(buffer)? {
                        self.with_roots(|heap| heap.typed_array_set_index(target, index, value))?;
                    }
                }
                return Ok(true);
            }
            // Invalid canonical numeric indices terminate the exotic [[Set]]
            // without coercion. A valid index with a distinct Receiver instead
            // follows OrdinarySet below and stores the original value there.
            if !valid {
                return Ok(true);
            }
        }
        let mut current = Some(target);
        while let Some(object) = current {
            if self.heap.proxy(object)?.is_some() {
                return self.proxy_set(object, receiver, key, value);
            }
            if object != target {
                if let Some(numeric) = self.heap.typed_array_numeric_key(object, key)? {
                    let valid = matches!(numeric, TypedArrayNumericKey::Index(index) if self
                        .heap
                        .typed_array_index_value(object, index)?
                        .is_some());
                    if receiver == &Value::Object(object) {
                        // OrdinarySet reached an Integer-Indexed exotic in
                        // the prototype chain. Its [[Set]] target is this
                        // `object`, not the initial ordinary receiver. The
                        // SameValue branch therefore still performs ToNumber
                        // for any canonical numeric key before validity is
                        // tested.
                        let value = self.coerce_number(value)?;
                        if valid {
                            let TypedArrayNumericKey::Index(index) = numeric else {
                                unreachable!("valid TypedArray index has an integer index")
                            };
                            let (buffer, _, _, _) = self.heap.typed_array_info(object)?;
                            if !self.heap.array_buffer_is_detached(buffer)? {
                                self.with_roots(|heap| {
                                    heap.typed_array_set_index(object, index, value)
                                })?;
                            }
                        }
                        return Ok(true);
                    }
                    if !valid {
                        return Ok(true);
                    }
                }
            }
            if let Some(descriptor) = self.object_get_own_property(object, key)? {
                if descriptor.accessor() {
                    let setter = descriptor.set.unwrap_or(Value::Undefined);
                    if setter == Value::Undefined {
                        return Ok(false);
                    }
                    if !self.is_callable(&setter)? {
                        return Err(RuntimeError::TypeError(
                            "property setter is not callable".into(),
                        ));
                    }
                    self.call_native(setter, receiver.clone(), vec![value.clone()], false)?;
                    return Ok(true);
                }
                if descriptor.writable == Some(false) {
                    return Ok(false);
                }
                break;
            }
            current = self.object_get_prototype(object)?;
        }
        let Value::Object(receiver) = receiver else {
            return Ok(false);
        };
        if self.heap.proxy(*receiver)?.is_some() {
            return self.proxy_define_own_property(
                *receiver,
                key.clone(),
                PropertyDescriptor::data(value.clone(), true, true, true),
            );
        }
        let own = self.object_get_own_property(*receiver, key)?;
        if let Some(own) = &own {
            if own.accessor() || own.writable == Some(false) {
                return Ok(false);
            }
        }
        let stored = if key == "length" && self.heap.is_array(*receiver)? {
            self.array_length_value(value)?
        } else {
            value.clone()
        };
        let descriptor = if own.is_some() {
            PropertyDescriptor {
                value: Some(stored),
                ..Default::default()
            }
        } else {
            PropertyDescriptor::data(stored, true, true, true)
        };
        self.object_define_own_property(*receiver, key.clone(), descriptor)
    }

    pub(super) fn get_from_prototype(
        &mut self,
        start: ObjectId,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        let mut current = Some(start);
        while let Some(object) = current {
            if self.heap.proxy(object)?.is_some() {
                return self.proxy_get(object, receiver, key);
            }
            if let Some(numeric) = self.heap.typed_array_numeric_key(object, key)? {
                return match numeric {
                    TypedArrayNumericKey::Index(index) => Ok(self
                        .heap
                        .typed_array_index_value(object, index)?
                        .unwrap_or(Value::Undefined)),
                    TypedArrayNumericKey::Invalid => Ok(Value::Undefined),
                };
            }
            if let Some(desc) = self.object_get_own_property(object, key)? {
                if desc.accessor() {
                    let getter = desc.get.unwrap_or(Value::Undefined);
                    return if matches!(getter, Value::Undefined) {
                        Ok(Value::Undefined)
                    } else {
                        self.call_native(getter, receiver.clone(), Vec::new(), false)
                    };
                }
                return Ok(desc.value.unwrap_or(Value::Undefined));
            }
            current = self.object_get_prototype(object)?;
        }
        Ok(Value::Undefined)
    }

    pub(super) fn constructor_prototype(
        &mut self,
        default: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        let target = self.new_target.clone();
        let prototype = self.get_property(&target, &"prototype".into())?;
        Ok(prototype.object_id().unwrap_or(default))
    }

    pub(super) fn is_constructor(&self, value: &Value) -> Result<bool, RuntimeError> {
        let Value::Object(id) = value else {
            return Ok(false);
        };
        if let Some((_, constructible)) = self.heap.proxy_capabilities(*id)? {
            return Ok(constructible);
        }
        if let Some(bound) = self.heap.bound_function(*id)? {
            return Ok(bound.constructible);
        }
        if let Some((code, _, _, _, _)) = self.heap.closure(*id)? {
            return Ok(code.constructible);
        }
        Ok(matches!(
            self.heap.native_function(*id)?,
            Some(
                NativeFunction::String
                    | NativeFunction::Array
                    | NativeFunction::ArrayBuffer
                    | NativeFunction::DataView
                    | NativeFunction::TypedArray(_)
                    | NativeFunction::Proxy
                    | NativeFunction::Map
                    | NativeFunction::Set
                    | NativeFunction::Object
                    | NativeFunction::RegExp
                    | NativeFunction::Collator
                    | NativeFunction::Locale
                    | NativeFunction::Error(_)
                    | NativeFunction::PrimitiveConstructor(_)
            )
        ))
    }

    fn proxy_constructor(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Proxy constructor requires 'new'".into(),
            ));
        }
        let target = native::argument(args, 0)
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Proxy target must be an object".into()))?;
        let handler = native::argument(args, 1)
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Proxy handler must be an object".into()))?;
        let callable = self.is_callable(&Value::Object(target))?;
        let constructible = self.is_constructor(&Value::Object(target))?;
        // A Proxy's ordinary prototype slot is never consulted by its
        // internal methods; [[GetPrototypeOf]] delegates to the target.
        // Using Object.prototype also allows ProxyCreate to wrap an already
        // revoked Proxy, as required by the specification.
        let prototype = Some(self.object_prototype);
        Ok(Value::Object(self.with_roots(|heap| {
            heap.alloc_proxy(target, handler, prototype, callable, constructible)
        })?))
    }

    fn proxy_revocable(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let target = native::argument(args, 0)
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Proxy target must be an object".into()))?;
        let handler = native::argument(args, 1)
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Proxy handler must be an object".into()))?;
        let callable = self.is_callable(&Value::Object(target))?;
        let constructible = self.is_constructor(&Value::Object(target))?;
        let proxy_prototype = Some(self.object_prototype);
        let proxy = self.with_roots(|heap| {
            heap.alloc_proxy(target, handler, proxy_prototype, callable, constructible)
        })?;
        let base = self.stack.len();
        self.stack.push(Value::Object(proxy));
        let result = (|| {
            let function_prototype = self.function_prototype()?;
            let revoke = self.with_roots(|heap| {
                heap.alloc_native_function(
                    NativeFunction::ProxyRevoker(proxy),
                    "",
                    function_prototype,
                )
            })?;
            self.stack.push(Value::Object(revoke));
            self.define_data(revoke, "name", Value::String("".into()), false, false, true)?;
            self.define_data(revoke, "length", Value::Number(0.0), false, false, true)?;
            let object_prototype = self.object_prototype;
            let result = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
            self.stack.push(Value::Object(result));
            self.define_data(result, "proxy", Value::Object(proxy), true, true, true)?;
            self.define_data(result, "revoke", Value::Object(revoke), true, true, true)?;
            Ok(Value::Object(result))
        })();
        self.stack.truncate(base);
        result
    }

    /// Returns a Proxy trap value, preserving the handler as the call
    /// receiver.  Proxy objects retain both target and handler as heap edges,
    /// so the rooted proxy itself keeps the arguments live across this lookup.
    fn proxy_trap(&mut self, handler: ObjectId, name: &str) -> Result<Value, RuntimeError> {
        self.get_property(&Value::Object(handler), &name.into())
    }

    /// Implements Proxy.[[Get]] including the non-configurable-property
    /// invariants.  `receiver` is deliberately separate from `proxy`: this
    /// is what makes inherited Proxy properties and Reflect.get faithful.
    pub(super) fn proxy_get(
        &mut self,
        proxy: ObjectId,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self.get_object_property(proxy, receiver, key);
        };
        let trap = self.proxy_trap(handler, "get")?;
        if trap == Value::Undefined {
            return self.get_object_property(target, receiver, key);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy get trap must be callable".into(),
            ));
        }
        let result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target), key.value(), receiver.clone()],
            false,
        )?;
        if let Some(descriptor) = self.object_get_own_property(target, key)? {
            if descriptor.configurable == Some(false) {
                if descriptor.writable == Some(false)
                    && descriptor
                        .value
                        .as_ref()
                        .is_some_and(|value| !same_value(value, &result))
                {
                    return Err(RuntimeError::TypeError(
                        "Proxy get trap violated a non-writable property invariant".into(),
                    ));
                }
                if descriptor.accessor()
                    && descriptor.get == Some(Value::Undefined)
                    && result != Value::Undefined
                {
                    return Err(RuntimeError::TypeError(
                        "Proxy get trap violated an accessor invariant".into(),
                    ));
                }
            }
        }
        Ok(result)
    }

    /// Implements Proxy.[[HasProperty]] and its non-configurable and
    /// non-extensible target invariants.
    pub(super) fn proxy_has(
        &mut self,
        proxy: ObjectId,
        key: &PropertyName,
    ) -> Result<bool, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self.has_property(proxy, key);
        };
        let trap = self.proxy_trap(handler, "has")?;
        if trap == Value::Undefined {
            return self.has_property(target, key);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy has trap must be callable".into(),
            ));
        }
        let result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target), key.value()],
            false,
        )?;
        let result = self.to_boolean(&result)?;
        if !result {
            if let Some(descriptor) = self.object_get_own_property(target, key)? {
                if descriptor.configurable == Some(false) || !self.object_is_extensible(target)? {
                    return Err(RuntimeError::TypeError(
                        "Proxy has trap hid a required target property".into(),
                    ));
                }
            }
        }
        Ok(result)
    }

    /// Implements Proxy.[[Set]]. The actual data-property write is shared by
    /// ordinary property assignment and Reflect.set, which keeps the supplied
    /// receiver visible to prototype accessors and Proxy traps.
    pub(super) fn proxy_set(
        &mut self,
        proxy: ObjectId,
        receiver: &Value,
        key: &PropertyName,
        value: &Value,
    ) -> Result<bool, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self.ordinary_set_with_receiver(proxy, receiver, key, value);
        };
        let trap = self.proxy_trap(handler, "set")?;
        if trap == Value::Undefined {
            return self.ordinary_set_with_receiver(target, receiver, key, value);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy set trap must be callable".into(),
            ));
        }
        let trap_result = self.call_native(
            trap,
            Value::Object(handler),
            vec![
                Value::Object(target),
                key.value(),
                value.clone(),
                receiver.clone(),
            ],
            false,
        )?;
        if !self.to_boolean(&trap_result)? {
            return Ok(false);
        }
        if let Some(descriptor) = self.object_get_own_property(target, key)? {
            if descriptor.configurable == Some(false)
                && ((descriptor.writable == Some(false)
                    && descriptor
                        .value
                        .as_ref()
                        .is_some_and(|current| !same_value(current, value)))
                    || (descriptor.accessor() && descriptor.set == Some(Value::Undefined)))
            {
                return Err(RuntimeError::TypeError(
                    "Proxy set trap violated a target property invariant".into(),
                ));
            }
        }
        Ok(true)
    }

    pub(super) fn proxy_delete(
        &mut self,
        proxy: ObjectId,
        key: &PropertyName,
    ) -> Result<bool, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return Ok(self.heap.delete(proxy, key)?);
        };
        let trap = self.proxy_trap(handler, "deleteProperty")?;
        if trap == Value::Undefined {
            return self.object_delete(target, key);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy deleteProperty trap must be callable".into(),
            ));
        }
        let trap_result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target), key.value()],
            false,
        )?;
        if !self.to_boolean(&trap_result)? {
            return Ok(false);
        }
        if let Some(descriptor) = self.object_get_own_property(target, key)? {
            if descriptor.configurable == Some(false) || !self.object_is_extensible(target)? {
                return Err(RuntimeError::TypeError(
                    "Proxy deleteProperty trap removed a required target property".into(),
                ));
            }
        }
        Ok(true)
    }

    pub(super) fn proxy_own_keys(
        &mut self,
        proxy: ObjectId,
    ) -> Result<Vec<PropertyName>, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self.heap.own_property_keys(proxy).map_err(Into::into);
        };
        let trap = self.proxy_trap(handler, "ownKeys")?;
        if trap == Value::Undefined {
            return self.object_own_property_keys(target);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy ownKeys trap must be callable".into(),
            ));
        }
        let result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target)],
            false,
        )?;
        let object = self.coerce_object(&result)?;
        self.stack.push(Value::Object(object));
        let length = self.get_property(&Value::Object(object), &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        let mut keys = Vec::new();
        for index in 0..length {
            let value = self.get_property(&Value::Object(object), &index.to_string().into())?;
            let key = match value {
                Value::String(string) => PropertyName::String(string),
                Value::Symbol(symbol) => PropertyName::Symbol(symbol),
                _ => {
                    self.stack.pop();
                    return Err(RuntimeError::TypeError(
                        "Proxy ownKeys trap result contains a non-property key".into(),
                    ));
                }
            };
            if keys.contains(&key) {
                self.stack.pop();
                return Err(RuntimeError::TypeError(
                    "Proxy ownKeys trap returned a duplicate key".into(),
                ));
            }
            keys.push(key);
        }
        self.stack.pop();
        let target_keys = self.object_own_property_keys(target)?;
        let mut non_configurable = Vec::new();
        for key in &target_keys {
            if self
                .object_get_own_property(target, key)?
                .is_some_and(|descriptor| descriptor.configurable == Some(false))
            {
                non_configurable.push(key.clone());
            }
        }
        if non_configurable.iter().any(|key| !keys.contains(key)) {
            return Err(RuntimeError::TypeError(
                "Proxy ownKeys trap omitted a non-configurable key".into(),
            ));
        }
        if !self.object_is_extensible(target)?
            && (keys.len() != target_keys.len()
                || target_keys.iter().any(|key| !keys.contains(key)))
        {
            return Err(RuntimeError::TypeError(
                "Proxy ownKeys trap disagreed with a non-extensible target".into(),
            ));
        }
        Ok(keys)
    }

    fn descriptor_object(
        &mut self,
        descriptor: &PropertyDescriptor,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.object_prototype;
        let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            for (name, value) in [
                ("value", descriptor.value.clone()),
                ("writable", descriptor.writable.map(Value::Bool)),
                ("get", descriptor.get.clone()),
                ("set", descriptor.set.clone()),
                ("enumerable", descriptor.enumerable.map(Value::Bool)),
                ("configurable", descriptor.configurable.map(Value::Bool)),
            ] {
                if let Some(value) = value {
                    self.with_roots(|heap| heap.set(object, name, value))?;
                }
            }
            Ok(Value::Object(object))
        })();
        self.stack.pop();
        result
    }

    pub(super) fn proxy_get_own_property(
        &mut self,
        proxy: ObjectId,
        key: &PropertyName,
    ) -> Result<Option<PropertyDescriptor>, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self
                .heap
                .get_own_property_descriptor(proxy, key)
                .map_err(Into::into);
        };
        let trap = self.proxy_trap(handler, "getOwnPropertyDescriptor")?;
        if trap == Value::Undefined {
            return self.object_get_own_property(target, key);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy getOwnPropertyDescriptor trap must be callable".into(),
            ));
        }
        let result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target), key.value()],
            false,
        )?;
        let target_descriptor = self.object_get_own_property(target, key)?;
        let extensible = self.object_is_extensible(target)?;
        if result == Value::Undefined {
            if target_descriptor
                .as_ref()
                .is_some_and(|descriptor| descriptor.configurable == Some(false))
                || (!extensible && target_descriptor.is_some())
            {
                return Err(RuntimeError::TypeError(
                    "Proxy getOwnPropertyDescriptor trap hid a required target property".into(),
                ));
            }
            return Ok(None);
        }
        let descriptor = complete_property_descriptor(self.read_descriptor(&result)?);
        if !compatible_property_descriptor(extensible, target_descriptor.as_ref(), &descriptor) {
            return Err(RuntimeError::TypeError(
                "Proxy getOwnPropertyDescriptor trap returned an incompatible descriptor".into(),
            ));
        }
        if descriptor.configurable == Some(false)
            && target_descriptor
                .as_ref()
                .is_none_or(|current| current.configurable != Some(false))
        {
            return Err(RuntimeError::TypeError(
                "Proxy getOwnPropertyDescriptor trap reported a new non-configurable property"
                    .into(),
            ));
        }
        Ok(Some(descriptor))
    }

    pub(super) fn proxy_define_own_property(
        &mut self,
        proxy: ObjectId,
        key: PropertyName,
        descriptor: PropertyDescriptor,
    ) -> Result<bool, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self.with_roots(|heap| heap.define_own_property(proxy, key, descriptor));
        };
        let trap = self.proxy_trap(handler, "defineProperty")?;
        if trap == Value::Undefined {
            return self.object_define_own_property(target, key, descriptor);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy defineProperty trap must be callable".into(),
            ));
        }
        let descriptor_value = self.descriptor_object(&descriptor)?;
        let trap_result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target), key.value(), descriptor_value],
            false,
        )?;
        if !self.to_boolean(&trap_result)? {
            return Ok(false);
        }
        let target_descriptor = self.object_get_own_property(target, &key)?;
        let extensible = self.object_is_extensible(target)?;
        if !compatible_property_descriptor(extensible, target_descriptor.as_ref(), &descriptor) {
            return Err(RuntimeError::TypeError(
                "Proxy defineProperty trap reported an incompatible property".into(),
            ));
        }
        if descriptor.configurable == Some(false)
            && target_descriptor
                .as_ref()
                .is_none_or(|current| current.configurable != Some(false))
        {
            return Err(RuntimeError::TypeError(
                "Proxy defineProperty trap reported a new non-configurable property".into(),
            ));
        }
        Ok(true)
    }

    pub(super) fn proxy_is_extensible(&mut self, proxy: ObjectId) -> Result<bool, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self.heap.is_extensible(proxy).map_err(Into::into);
        };
        let trap = self.proxy_trap(handler, "isExtensible")?;
        if trap == Value::Undefined {
            return self.object_is_extensible(target);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy isExtensible trap must be callable".into(),
            ));
        }
        let trap_result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target)],
            false,
        )?;
        let result = self.to_boolean(&trap_result)?;
        if result != self.object_is_extensible(target)? {
            return Err(RuntimeError::TypeError(
                "Proxy isExtensible trap disagreed with its target".into(),
            ));
        }
        Ok(result)
    }

    pub(super) fn proxy_get_prototype(
        &mut self,
        proxy: ObjectId,
    ) -> Result<Option<ObjectId>, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return self.heap.prototype(proxy).map_err(Into::into);
        };
        let trap = self.proxy_trap(handler, "getPrototypeOf")?;
        if trap == Value::Undefined {
            return self.object_get_prototype(target);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy getPrototypeOf trap must be callable".into(),
            ));
        }
        let result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target)],
            false,
        )?;
        let prototype = match result {
            Value::Null => None,
            Value::Object(object) => Some(object),
            _ => {
                return Err(RuntimeError::TypeError(
                    "Proxy getPrototypeOf trap must return an object or null".into(),
                ))
            }
        };
        if !self.object_is_extensible(target)? && prototype != self.object_get_prototype(target)? {
            return Err(RuntimeError::TypeError(
                "Proxy getPrototypeOf trap disagreed with a non-extensible target".into(),
            ));
        }
        Ok(prototype)
    }

    pub(super) fn proxy_set_prototype(
        &mut self,
        proxy: ObjectId,
        prototype: Option<ObjectId>,
    ) -> Result<bool, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return match self.heap.set_prototype(proxy, prototype) {
                Ok(()) => Ok(true),
                Err(HeapError::ReadOnlyProperty | HeapError::PrototypeCycle) => Ok(false),
                Err(error) => Err(error.into()),
            };
        };
        let trap = self.proxy_trap(handler, "setPrototypeOf")?;
        if trap == Value::Undefined {
            return self.object_set_prototype(target, prototype);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy setPrototypeOf trap must be callable".into(),
            ));
        }
        let trap_result = self.call_native(
            trap,
            Value::Object(handler),
            vec![
                Value::Object(target),
                prototype.map_or(Value::Null, Value::Object),
            ],
            false,
        )?;
        if !self.to_boolean(&trap_result)? {
            return Ok(false);
        }
        if !self.object_is_extensible(target)? && prototype != self.object_get_prototype(target)? {
            return Err(RuntimeError::TypeError(
                "Proxy setPrototypeOf trap changed a non-extensible target".into(),
            ));
        }
        Ok(true)
    }

    pub(super) fn proxy_prevent_extensions(
        &mut self,
        proxy: ObjectId,
    ) -> Result<bool, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            self.heap.prevent_extensions(proxy)?;
            return Ok(true);
        };
        let trap = self.proxy_trap(handler, "preventExtensions")?;
        if trap == Value::Undefined {
            return self.object_prevent_extensions(target);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(
                "Proxy preventExtensions trap must be callable".into(),
            ));
        }
        let trap_result = self.call_native(
            trap,
            Value::Object(handler),
            vec![Value::Object(target)],
            false,
        )?;
        if !self.to_boolean(&trap_result)? {
            return Ok(false);
        }
        if self.object_is_extensible(target)? {
            return Err(RuntimeError::TypeError(
                "Proxy preventExtensions trap left its target extensible".into(),
            ));
        }
        Ok(true)
    }

    /// Proxy.[[Call]] and Proxy.[[Construct]].  The outer call frame retains
    /// the Proxy as `newTarget`, so forwarding a construct without a trap
    /// preserves the required allocation prototype.
    pub(super) fn proxy_call(
        &mut self,
        proxy: ObjectId,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let Some((target, handler)) = self.heap.proxy(proxy)? else {
            return Err(RuntimeError::TypeError(
                "Proxy target is unavailable".into(),
            ));
        };
        let name = if construct { "construct" } else { "apply" };
        let trap = self.proxy_trap(handler, name)?;
        if trap == Value::Undefined {
            return self.dispatch_call(Value::Object(target), receiver, args, construct);
        }
        if !self.is_callable(&trap)? {
            return Err(RuntimeError::TypeError(format!(
                "Proxy {name} trap must be callable"
            )));
        }
        let arguments = self.array_from(args)?;
        let values = if construct {
            vec![Value::Object(target), arguments, self.new_target.clone()]
        } else {
            vec![Value::Object(target), receiver, arguments]
        };
        let result = self.call_native(trap, Value::Object(handler), values, false)?;
        if construct && !matches!(result, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Proxy construct trap must return an object".into(),
            ));
        }
        Ok(result)
    }

    fn buffer_prototype(&mut self, constructor: &str) -> Result<ObjectId, RuntimeError> {
        let constructor = self.global(constructor)?;
        self.get_property(&constructor, &"prototype".into())?
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("buffer prototype is unavailable".into()))
    }

    /// OrdinaryCreateFromConstructor for the concrete non-shared binary
    /// constructors. The intrinsic prototype remains the fallback when a
    /// custom `newTarget.prototype` is not an object.
    fn constructed_buffer_prototype(
        &mut self,
        constructor: &str,
    ) -> Result<ObjectId, RuntimeError> {
        let default = self.buffer_prototype(constructor)?;
        self.constructor_prototype(default)
    }

    /// Lazily creates the non-global `%TypedArray%` constructor and its shared
    /// prototype. Concrete typed-array constructors inherit from this function
    /// and their per-kind prototypes inherit from this object, which is
    /// observable through `Object.getPrototypeOf(Int8Array)`.
    fn typed_array_intrinsics(&mut self) -> Result<(ObjectId, ObjectId), RuntimeError> {
        if let Some(intrinsics) = self.typed_array_intrinsics {
            return Ok(intrinsics);
        }
        let object_prototype = self.object_prototype;
        let function_prototype = self.function_prototype()?;
        let typed_prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(typed_prototype)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(typed_prototype));
        let result = (|| {
            let constructor = self.with_roots(|heap| {
                heap.alloc_native_function(
                    NativeFunction::TypedArrayIntrinsic,
                    "TypedArray",
                    function_prototype,
                )
            })?;
            self.stack.push(Value::Object(constructor));
            self.define_data(
                constructor,
                "name",
                Value::String("TypedArray".into()),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "length",
                Value::Number(0.0),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "prototype",
                Value::Object(typed_prototype),
                false,
                false,
                false,
            )?;
            self.define_data(
                typed_prototype,
                "constructor",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            for (name, native) in [
                ("buffer", NativeFunction::TypedArrayBuffer),
                ("byteLength", NativeFunction::TypedArrayByteLength),
                ("byteOffset", NativeFunction::TypedArrayByteOffset),
                ("length", NativeFunction::TypedArrayLength),
            ] {
                self.install_native_getter(typed_prototype, function_prototype, name, native)?;
            }
            self.install_native(
                typed_prototype,
                function_prototype,
                "set",
                1,
                NativeFunction::TypedArraySet,
            )?;
            self.install_native(
                typed_prototype,
                function_prototype,
                "subarray",
                2,
                NativeFunction::TypedArraySubarray,
            )?;
            Ok(constructor)
        })();
        self.stack.truncate(base);
        match result {
            Ok(constructor) => {
                let intrinsics = (constructor, typed_prototype);
                self.typed_array_intrinsics = Some(intrinsics);
                Ok(intrinsics)
            }
            Err(error) => {
                self.heap.unroot(root)?;
                Err(error)
            }
        }
    }

    fn buffer_index(&mut self, value: &Value) -> Result<usize, RuntimeError> {
        let number = self.coerce_number(value)?;
        if number.is_nan() || number == 0.0 {
            return Ok(0);
        }
        if !number.is_finite() {
            return Err(RuntimeError::RangeError("invalid buffer index".into()));
        }
        let integer = number.trunc();
        // ToIndex applies ToIntegerOrInfinity before rejecting negatives, so
        // a finite value in (-1, 0) becomes -0 and is accepted as zero.
        if integer < 0.0 {
            return Err(RuntimeError::RangeError("invalid buffer index".into()));
        }
        if integer > usize::MAX as f64 {
            return Err(RuntimeError::RangeError("buffer index is too large".into()));
        }
        Ok(integer as usize)
    }

    fn array_buffer_constructor(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer constructor requires 'new'".into(),
            ));
        }
        let length = if args.is_empty() {
            0
        } else {
            self.buffer_index(native::argument(args, 0))?
        };
        let prototype = self.constructed_buffer_prototype("ArrayBuffer")?;
        Ok(Value::Object(self.with_roots(|heap| {
            heap.alloc_array_buffer(length, Some(prototype))
        })?))
    }

    fn array_buffer_receiver(&self, receiver: &Value) -> Result<ObjectId, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("ArrayBuffer method requires an ArrayBuffer receiver".into())
        })?;
        if !self.heap.is_array_buffer(object)? {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer method requires an ArrayBuffer receiver".into(),
            ));
        }
        Ok(object)
    }

    fn array_buffer_species_constructor(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let constructor = self.get_property(receiver, &"constructor".into())?;
        if constructor == Value::Undefined {
            return self.global("ArrayBuffer");
        }
        if !matches!(constructor, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer constructor must be an object".into(),
            ));
        }
        let species = self.get_property(&constructor, &JsSymbol::well_known("species").into())?;
        if matches!(species, Value::Undefined | Value::Null) {
            return self.global("ArrayBuffer");
        }
        if !self.is_constructor(&species)? {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer species must be a constructor".into(),
            ));
        }
        Ok(species)
    }

    fn array_buffer_slice(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let buffer = self.array_buffer_receiver(receiver)?;
        let length = self.heap.array_buffer_byte_length(buffer)?;
        let start = self.relative_buffer_index(native::argument(args, 0), length)?;
        let end = if args.get(1).is_some_and(|value| *value != Value::Undefined) {
            self.relative_buffer_index(native::argument(args, 1), length)?
        } else {
            length
        };
        let width = end.saturating_sub(start);
        if self.heap.array_buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError("ArrayBuffer is detached".into()));
        }
        let constructor = self.array_buffer_species_constructor(receiver)?;
        let result = self.call_with_target(
            constructor.clone(),
            Value::Undefined,
            vec![Value::Number(width as f64)],
            true,
            constructor,
        )?;
        let result_buffer = self.array_buffer_receiver(&result)?;
        if result_buffer == buffer {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer species returned the source buffer".into(),
            ));
        }
        if self.heap.array_buffer_is_detached(buffer)?
            || self.heap.array_buffer_is_detached(result_buffer)?
        {
            return Err(RuntimeError::TypeError("ArrayBuffer is detached".into()));
        }
        if self.heap.array_buffer_byte_length(result_buffer)? < width {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer species result is too small".into(),
            ));
        }
        let bytes = self.heap.array_buffer_copy(buffer, start, width)?;
        self.with_roots(|heap| heap.array_buffer_write(result_buffer, 0, &bytes))?;
        Ok(result)
    }

    fn relative_buffer_index(
        &mut self,
        value: &Value,
        length: usize,
    ) -> Result<usize, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(0);
        }
        let number = self.coerce_number(value)?;
        if number.is_nan() {
            return Ok(0);
        }
        if number == f64::INFINITY {
            return Ok(length);
        }
        if number == f64::NEG_INFINITY {
            return Ok(0);
        }
        let integer = number.trunc();
        if integer < 0.0 {
            Ok(length.saturating_sub((-integer) as usize))
        } else {
            Ok((integer as usize).min(length))
        }
    }

    fn data_view_constructor(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "DataView constructor requires 'new'".into(),
            ));
        }
        let buffer = native::argument(args, 0).object_id().ok_or_else(|| {
            RuntimeError::TypeError("DataView buffer must be an ArrayBuffer".into())
        })?;
        if !self.heap.is_array_buffer(buffer)? {
            return Err(RuntimeError::TypeError(
                "DataView buffer must be an ArrayBuffer".into(),
            ));
        }
        // ToIndex(byteOffset) is observable and precedes the detached-buffer
        // check. A valueOf hook can therefore run even for a detached buffer.
        let offset = if args.len() > 1 {
            self.buffer_index(native::argument(args, 1))?
        } else {
            0
        };
        if self.heap.array_buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError(
                "DataView buffer is detached".into(),
            ));
        }
        let total = self.heap.array_buffer_byte_length(buffer)?;
        if offset > total {
            return Err(RuntimeError::RangeError(
                "DataView offset is outside its buffer".into(),
            ));
        }
        let length = if args.len() > 2 && native::argument(args, 2) != &Value::Undefined {
            self.buffer_index(native::argument(args, 2))?
        } else {
            total - offset
        };
        let prototype = self.constructed_buffer_prototype("DataView")?;
        Ok(Value::Object(self.with_roots(|heap| {
            heap.alloc_data_view(buffer, offset, length, Some(prototype))
        })?))
    }

    fn data_view_receiver(
        &self,
        receiver: &Value,
    ) -> Result<(ObjectId, usize, usize), RuntimeError> {
        let (buffer, offset, length) = self.data_view_raw_receiver(receiver)?;
        if self.heap.array_buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer has been detached".into(),
            ));
        }
        Ok((buffer, offset, length))
    }

    fn data_view_raw_receiver(
        &self,
        receiver: &Value,
    ) -> Result<(ObjectId, usize, usize), RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("DataView method requires a DataView receiver".into())
        })?;
        self.heap
            .data_view_raw_info(object)
            .map_err(|error| match error {
                HeapError::InvalidObject(_) | HeapError::InvalidInternalSlot(_) => {
                    RuntimeError::TypeError("DataView method requires a DataView receiver".into())
                }
                error => error.into(),
            })
    }

    fn data_view_get(
        &mut self,
        receiver: &Value,
        args: &[Value],
        width: usize,
        signed: bool,
        floating: bool,
    ) -> Result<Value, RuntimeError> {
        let (buffer, offset, length) = self.data_view_raw_receiver(receiver)?;
        let index = self.buffer_index(native::argument(args, 0))?;
        if self.heap.array_buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer has been detached".into(),
            ));
        }
        let end = index.checked_add(width).ok_or_else(|| {
            RuntimeError::RangeError("DataView access is outside its view".into())
        })?;
        if end > length {
            return Err(RuntimeError::RangeError(
                "DataView access is outside its view".into(),
            ));
        }
        let little_endian = match args.get(1) {
            Some(value) => self.to_boolean(value)?,
            None => false,
        };
        let bytes = self.heap.array_buffer_copy(buffer, offset + index, width)?;
        Ok(Value::Number(data_view_number(
            &bytes,
            signed,
            floating,
            little_endian,
        )))
    }

    fn data_view_set(
        &mut self,
        receiver: &Value,
        args: &[Value],
        width: usize,
        signed: bool,
        floating: bool,
    ) -> Result<Value, RuntimeError> {
        let (buffer, offset, length) = self.data_view_raw_receiver(receiver)?;
        let index = self.buffer_index(native::argument(args, 0))?;
        // SetViewValue converts its value before observing detachment or an
        // out-of-range index. This matters when valueOf throws or detaches.
        let value = self.coerce_number(native::argument(args, 1))?;
        if self.heap.array_buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError(
                "ArrayBuffer has been detached".into(),
            ));
        }
        let end = index.checked_add(width).ok_or_else(|| {
            RuntimeError::RangeError("DataView access is outside its view".into())
        })?;
        if end > length {
            return Err(RuntimeError::RangeError(
                "DataView access is outside its view".into(),
            ));
        }
        let little_endian = match args.get(2) {
            Some(value) => self.to_boolean(value)?,
            None => false,
        };
        let bytes = data_view_bytes(value, width, signed, floating, little_endian);
        self.with_roots(|heap| heap.array_buffer_write(buffer, offset + index, &bytes))?;
        Ok(Value::Undefined)
    }

    fn typed_array_constructor(
        &mut self,
        args: &[Value],
        construct: bool,
        kind: TypedArrayKind,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "TypedArray constructor requires 'new'".into(),
            ));
        }
        let input = native::argument(args, 0);
        let (buffer, byte_offset, length, initial_values) = if let Value::Object(buffer) = input {
            if self.heap.is_array_buffer(*buffer)? {
                if self.heap.array_buffer_is_detached(*buffer)? {
                    return Err(RuntimeError::TypeError(
                        "TypedArray buffer is detached".into(),
                    ));
                }
                let bytes = self.heap.array_buffer_byte_length(*buffer)?;
                let offset = if args.len() > 1 {
                    self.buffer_index(native::argument(args, 1))?
                } else {
                    0
                };
                if offset % kind.byte_width() != 0 || offset > bytes {
                    return Err(RuntimeError::RangeError(
                        "invalid TypedArray byte offset".into(),
                    ));
                }
                let length = if args.len() > 2 && native::argument(args, 2) != &Value::Undefined {
                    self.buffer_index(native::argument(args, 2))?
                } else {
                    let remaining = bytes - offset;
                    if remaining % kind.byte_width() != 0 {
                        return Err(RuntimeError::RangeError(
                            "invalid TypedArray buffer length".into(),
                        ));
                    }
                    remaining / kind.byte_width()
                };
                (*buffer, offset, length, None)
            } else if self.heap.is_typed_array(*buffer)? {
                let (source_buffer, _, source_length, _) = self.heap.typed_array_info(*buffer)?;
                if self.heap.array_buffer_is_detached(source_buffer)? {
                    return Err(RuntimeError::TypeError(
                        "TypedArray source is detached".into(),
                    ));
                }
                let values = self.typed_array_values(*buffer, source_length)?;
                let result = self.new_typed_array_buffer(source_length, kind)?;
                (result, 0, source_length, Some(values))
            } else {
                let source = Value::Object(*buffer);
                let base = self.stack.len();
                self.stack.push(source.clone());
                let values = (|| {
                    let iterator =
                        self.get_method(&source, &JsSymbol::well_known("iterator").into())?;
                    if iterator == Value::Undefined {
                        self.array_like_numbers(*buffer, kind)
                    } else {
                        self.iterable_numbers(&source, iterator, kind)
                    }
                })();
                self.stack.truncate(base);
                let values = values?;
                let length = values.len();
                let result = self.new_typed_array_buffer(length, kind)?;
                (result, 0, length, Some(values))
            }
        } else {
            let length = if *input == Value::Undefined {
                0
            } else {
                self.buffer_index(input)?
            };
            let buffer = self.new_typed_array_buffer(length, kind)?;
            (buffer, 0, length, None)
        };
        let base = self.stack.len();
        self.stack.push(Value::Object(buffer));
        let result = (|| {
            let prototype = self.constructed_buffer_prototype(kind.name())?;
            let object = self.with_roots(|heap| {
                heap.alloc_typed_array(buffer, byte_offset, length, kind, Some(prototype))
            })?;
            if let Some(values) = initial_values {
                self.stack.push(Value::Object(object));
                let writes = values
                    .into_iter()
                    .enumerate()
                    .try_for_each(|(index, value)| {
                        self.with_roots(|heap| heap.typed_array_set_index(object, index, value))
                            .map(|_| ())
                    });
                self.stack.pop();
                writes?;
            }
            Ok(Value::Object(object))
        })();
        self.stack.truncate(base);
        result
    }

    fn new_typed_array_buffer(
        &mut self,
        length: usize,
        kind: TypedArrayKind,
    ) -> Result<ObjectId, RuntimeError> {
        let bytes = length
            .checked_mul(kind.byte_width())
            .ok_or_else(|| RuntimeError::RangeError("TypedArray length is too large".into()))?;
        if bytes > self.heap.max_array_buffer_byte_length() {
            return Err(RuntimeError::RangeError(
                "TypedArray length is too large".into(),
            ));
        }
        let prototype = self.buffer_prototype("ArrayBuffer")?;
        self.with_roots(|heap| heap.alloc_array_buffer(bytes, Some(prototype)))
    }

    fn typed_array_values(
        &mut self,
        source: ObjectId,
        length: usize,
    ) -> Result<Vec<f64>, RuntimeError> {
        self.stack.push(Value::Object(source));
        let result = (|| {
            let mut values = Vec::with_capacity(length);
            for index in 0..length {
                let value = self.get_property(&Value::Object(source), &index.to_string().into())?;
                values.push(self.coerce_number(&value)?);
            }
            Ok(values)
        })();
        self.stack.pop();
        result
    }

    fn array_like_numbers(
        &mut self,
        source: ObjectId,
        kind: TypedArrayKind,
    ) -> Result<Vec<f64>, RuntimeError> {
        self.stack.push(Value::Object(source));
        let result = (|| {
            let length = self.get_property(&Value::Object(source), &"length".into())?;
            let length = self.coerce_length(&length)?;
            if length > (self.heap.max_array_buffer_byte_length() / kind.byte_width()) as f64 {
                return Err(RuntimeError::RangeError(
                    "TypedArray length is too large".into(),
                ));
            }
            let mut values = Vec::with_capacity(length as usize);
            for index in 0..length as usize {
                let value = self.get_property(&Value::Object(source), &index.to_string().into())?;
                values.push(self.coerce_number(&value)?);
            }
            Ok(values)
        })();
        self.stack.pop();
        result
    }

    /// Collect an iterable constructor source before allocating the new view.
    /// The source and iterator record stay on the VM stack for every user-code
    /// call, so a collection triggered by a getter, `next`, or number coercion
    /// cannot reclaim either internal object.
    fn iterable_numbers(
        &mut self,
        source: &Value,
        iterator_method: Value,
        kind: TypedArrayKind,
    ) -> Result<Vec<f64>, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(source.clone());
        let result = (|| {
            let record = self.get_iterator_from_method(source, iterator_method)?;
            self.stack.push(record.clone());
            let maximum = self.heap.max_array_buffer_byte_length() / kind.byte_width();
            let mut values = Vec::new();
            while let Some(value) = self.iterator_step(&record, true)? {
                if values.len() == maximum {
                    return Err(RuntimeError::RangeError(
                        "TypedArray length is too large".into(),
                    ));
                }
                values.push(self.coerce_number(&value)?);
            }
            Ok(values)
        })();
        self.stack.truncate(base);
        result
    }

    fn iterable_values(
        &mut self,
        source: &Value,
        iterator_method: Value,
    ) -> Result<Vec<Value>, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(source.clone());
        let result = (|| {
            let record = self.get_iterator_from_method(source, iterator_method)?;
            self.stack.push(record.clone());
            let mut values = Vec::new();
            while let Some(value) = self.iterator_step(&record, true)? {
                if values.len() == u32::MAX as usize {
                    return Err(RuntimeError::RangeError("Array length is too large".into()));
                }
                values.push(value);
            }
            Ok(values)
        })();
        self.stack.truncate(base);
        result
    }

    fn array_from_method(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let source = native::argument(args, 0).clone();
        if matches!(source, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "Array.from requires an object".into(),
            ));
        }
        let mapper = native::argument(args, 1).clone();
        if mapper != Value::Undefined && !self.is_callable(&mapper)? {
            return Err(RuntimeError::TypeError(
                "Array.from mapper must be callable".into(),
            ));
        }
        let this_arg = native::argument(args, 2).clone();
        let base = self.stack.len();
        self.stack.push(source.clone());
        if mapper != Value::Undefined {
            self.stack.push(mapper.clone());
            self.stack.push(this_arg.clone());
        }
        let result = (|| {
            let iterator = self.get_method(&source, &JsSymbol::well_known("iterator").into())?;
            let mut values = if iterator == Value::Undefined {
                let object = self.coerce_object(&source)?;
                self.stack.push(Value::Object(object));
                let values = self.array_like_values(&Value::Object(object));
                self.stack.pop();
                values?
            } else {
                self.iterable_values(&source, iterator)?
            };
            if mapper != Value::Undefined {
                for (index, value) in values.iter_mut().enumerate() {
                    *value = self.call_native(
                        mapper.clone(),
                        this_arg.clone(),
                        vec![value.clone(), Value::Number(index as f64)],
                        false,
                    )?;
                }
            }
            self.array_from(values)
        })();
        self.stack.truncate(base);
        result
    }

    fn typed_array_receiver(
        &self,
        receiver: &Value,
    ) -> Result<(ObjectId, usize, usize, TypedArrayKind), RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("TypedArray method requires a TypedArray receiver".into())
        })?;
        self.heap
            .typed_array_info(object)
            .map_err(|error| match error {
                HeapError::InvalidObject(_) => RuntimeError::TypeError(
                    "TypedArray method requires a TypedArray receiver".into(),
                ),
                error => error.into(),
            })
    }

    fn typed_array_set(&mut self, receiver: &Value, args: &[Value]) -> Result<Value, RuntimeError> {
        let (buffer, _, length, _) = self.typed_array_receiver(receiver)?;
        if self.heap.array_buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError(
                "TypedArray buffer is detached".into(),
            ));
        }
        let source = native::argument(args, 0);
        let target_offset = self.buffer_index(native::argument(args, 1))?;
        if target_offset > length {
            return Err(RuntimeError::RangeError(
                "target offset is outside TypedArray".into(),
            ));
        }
        let source = self.coerce_object(source)?;
        self.stack.push(Value::Object(source));
        let result = (|| {
            let source_length_value =
                self.get_property(&Value::Object(source), &"length".into())?;
            let source_length = self.coerce_length(&source_length_value)? as usize;
            if source_length > length - target_offset {
                return Err(RuntimeError::RangeError(
                    "source does not fit in TypedArray".into(),
                ));
            }
            let mut values = Vec::with_capacity(source_length);
            for index in 0..source_length {
                let value = self.get_property(&Value::Object(source), &index.to_string().into())?;
                values.push(self.coerce_number(&value)?);
            }
            for (index, value) in values.into_iter().enumerate() {
                self.with_roots(|heap| {
                    heap.typed_array_set_index(
                        receiver.object_id().expect("validated TypedArray receiver"),
                        target_offset + index,
                        value,
                    )
                })?;
            }
            Ok(Value::Undefined)
        })();
        self.stack.pop();
        result
    }

    fn typed_array_subarray(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let (buffer, byte_offset, length, kind) = self.typed_array_receiver(receiver)?;
        if self.heap.array_buffer_is_detached(buffer)? {
            return Err(RuntimeError::TypeError(
                "TypedArray buffer is detached".into(),
            ));
        }
        let start = self.relative_buffer_index(native::argument(args, 0), length)?;
        let end = if args.get(1).is_some_and(|value| *value != Value::Undefined) {
            self.relative_buffer_index(native::argument(args, 1), length)?
        } else {
            length
        };
        let view_length = end.saturating_sub(start);
        let view_offset = byte_offset
            .checked_add(start * kind.byte_width())
            .ok_or_else(|| RuntimeError::RangeError("TypedArray offset is too large".into()))?;
        let prototype = self.buffer_prototype(kind.name())?;
        let object = self.with_roots(|heap| {
            heap.alloc_typed_array(buffer, view_offset, view_length, kind, Some(prototype))
        })?;
        Ok(Value::Object(object))
    }

    pub(super) fn array_like_values(&mut self, value: &Value) -> Result<Vec<Value>, RuntimeError> {
        if !matches!(value, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "argument list must be an object".into(),
            ));
        }
        let length = self.get_property(value, &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        let mut args = Vec::new();
        for index in 0..length {
            self.charge_step()?;
            let value = self.get_property(value, &index.to_string().into())?;
            self.stack.push(value.clone());
            args.push(value);
        }
        Ok(args)
    }

    pub(super) fn base_iterator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.iterator_base {
            return Ok(prototype);
        }
        let constructor = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(constructor)?.unwrap();
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(prototype)?;
        let result = self.install_symbol_native(
            prototype,
            function_prototype,
            "iterator",
            0,
            NativeFunction::IteratorSelf,
        );
        if let Err(error) = result {
            self.heap.unroot(root)?;
            return Err(error);
        }
        self.iterator_base = Some(prototype);
        Ok(prototype)
    }

    pub(super) fn template_object(
        &mut self,
        site: &crate::bytecode::TemplateSite,
    ) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.templates.get(&site.id) {
            return Ok(Value::Object(id));
        }
        let base = self.stack.len();
        let result = (|| {
            for string in site.raw.iter().chain(site.cooked.iter().flatten()) {
                self.check_string(&Value::String(string.clone()))?;
            }
            let raw = self.array_from(site.raw.iter().cloned().map(Value::String).collect())?;
            self.stack.push(raw.clone());
            let cooked = self.array_from(
                site.cooked
                    .iter()
                    .cloned()
                    .map(|s| s.map_or(Value::Undefined, Value::String))
                    .collect(),
            )?;
            self.stack.push(cooked.clone());
            self.define_data(
                cooked.object_id().unwrap(),
                "raw",
                raw.clone(),
                false,
                false,
                false,
            )?;
            for array in [&raw, &cooked] {
                let id = array.object_id().unwrap();
                for key in self.heap.own_property_keys(id)? {
                    let mut desc = self.heap.get_own_property_descriptor(id, &key)?.unwrap();
                    desc.writable = Some(false);
                    desc.configurable = Some(false);
                    self.with_roots(|heap| heap.define_own_property(id, key, desc))?;
                }
                self.heap.prevent_extensions(id)?;
            }
            self.heap.root(cooked.object_id().unwrap())?;
            self.templates.insert(site.id, cooked.object_id().unwrap());
            Ok(cooked)
        })();
        self.stack.truncate(base);
        result
    }
    pub(super) fn get_iterator(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let method = self.get_method(value, &JsSymbol::well_known("iterator").into())?;
        self.get_iterator_from_method(value, method)
    }

    fn get_iterator_from_method(
        &mut self,
        value: &Value,
        method: Value,
    ) -> Result<Value, RuntimeError> {
        let iterator = self.call_native(method, value.clone(), Vec::new(), false)?;
        if !matches!(iterator, Value::Object(_)) {
            return Err(RuntimeError::TypeError("iterator must be an object".into()));
        }
        self.stack.push(iterator.clone());
        let next = self.get_property(&iterator, &"next".into())?;
        self.stack.push(next.clone());
        let record = self.with_roots(|heap| heap.alloc_object(None))?;
        self.stack.push(Value::Object(record));
        self.with_roots(|heap| heap.set(record, "iterator", iterator))?;
        self.with_roots(|heap| heap.set(record, "next", next))?;
        self.with_roots(|heap| heap.set(record, "done", Value::Bool(false)))?;
        self.stack.pop();
        self.stack.pop();
        self.stack.pop();
        Ok(Value::Object(record))
    }

    /// GetAsyncIterator first observes @@asyncIterator and uses the ordinary
    /// iterator protocol as an AsyncFromSync fallback. The caller awaits the
    /// returned `.next()` result, so both paths share one record shape.
    pub(super) fn get_async_iterator(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let method = self.get_method(value, &JsSymbol::well_known("asyncIterator").into())?;
        if method == Value::Undefined {
            return self.get_iterator(value);
        }
        let iterator = self.call_native(method, value.clone(), Vec::new(), false)?;
        if !matches!(iterator, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "async iterator must be an object".into(),
            ));
        }
        self.stack.push(iterator.clone());
        let next = self.get_property(&iterator, &"next".into())?;
        self.stack.push(next.clone());
        let record = self.with_roots(|heap| heap.alloc_object(None))?;
        self.stack.push(Value::Object(record));
        self.with_roots(|heap| heap.set(record, "iterator", iterator))?;
        self.with_roots(|heap| heap.set(record, "next", next))?;
        self.with_roots(|heap| heap.set(record, "done", Value::Bool(false)))?;
        self.stack.pop();
        self.stack.pop();
        self.stack.pop();
        Ok(Value::Object(record))
    }

    pub(super) fn async_iterator_next(
        &mut self,
        record: &Value,
        argument: Option<Value>,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(record) = record else {
            unreachable!("compiler only emits iterator records")
        };
        if matches!(self.heap.get_own(*record, "done")?, Some(Value::Bool(true))) {
            return self.iterator_result(Value::Undefined, true);
        }
        let iterator = self.get_property(&Value::Object(*record), &"iterator".into())?;
        let next = self.get_property(&Value::Object(*record), &"next".into())?;
        self.call_native(next, iterator, argument.into_iter().collect(), false)
    }

    pub(super) fn async_iterator_step(
        &mut self,
        record: &Value,
        result: &Value,
    ) -> Result<Option<Value>, RuntimeError> {
        let Value::Object(record) = record else {
            unreachable!("compiler only emits iterator records")
        };
        let outcome = (|| {
            if !matches!(result, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "async iterator result must be an object".into(),
                ));
            }
            let done = self.get_property(result, &"done".into())?;
            if self.to_boolean(&done)? {
                Ok(None)
            } else {
                self.get_property(result, &"value".into()).map(Some)
            }
        })();
        if !matches!(&outcome, Ok(Some(_))) {
            let base = self.stack.len();
            if let Err(RuntimeError::Thrown(value)) = &outcome {
                self.stack.push(value.clone());
            }
            let marked = self.with_roots(|heap| heap.set(*record, "done", Value::Bool(true)));
            self.stack.truncate(base);
            marked?;
        }
        outcome
    }

    /// IteratorStepValue, or IteratorStep without IteratorValue for elisions.
    /// Iterator-origin errors complete this record before outer unwinding.
    pub(super) fn iterator_step(
        &mut self,
        record: &Value,
        read_value: bool,
    ) -> Result<Option<Value>, RuntimeError> {
        let Value::Object(record) = record else {
            unreachable!("compiler only emits iterator records")
        };
        if matches!(self.heap.get_own(*record, "done")?, Some(Value::Bool(true))) {
            return Ok(None);
        }
        let outcome = (|| {
            let iterator = self.get_property(&Value::Object(*record), &"iterator".into())?;
            let next = self.get_property(&Value::Object(*record), &"next".into())?;
            let result = self.call_native(next, iterator, Vec::new(), false)?;
            if !matches!(result, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "iterator result must be an object".into(),
                ));
            }
            self.stack.push(result.clone());
            let done = self.get_property(&result, &"done".into())?;
            let value = if self.to_boolean(&done)? {
                None
            } else if read_value {
                Some(self.get_property(&result, &"value".into())?)
            } else {
                Some(Value::Undefined)
            };
            self.stack.pop();
            Ok(value)
        })();
        if !matches!(&outcome, Ok(Some(_))) {
            let base = self.stack.len();
            if let Err(RuntimeError::Thrown(value)) = &outcome {
                self.stack.push(value.clone());
            }
            let marked = self.with_roots(|heap| heap.set(*record, "done", Value::Bool(true)));
            self.stack.truncate(base);
            marked?;
        }
        outcome
    }

    pub(super) fn iterator_close(&mut self, record: &Value) -> Result<(), RuntimeError> {
        let id = record
            .object_id()
            .expect("compiler only emits iterator records");
        if matches!(self.heap.get_own(id, "done")?, Some(Value::Bool(true))) {
            return Ok(());
        }
        self.with_roots(|heap| heap.set(id, "done", Value::Bool(true)))?;
        let iterator = self.get_property(record, &"iterator".into())?;
        let close = self.get_method(&iterator, &"return".into())?;
        if close != Value::Undefined {
            let result = self.call_native(close, iterator, Vec::new(), false)?;
            if !matches!(result, Value::Object(_)) {
                return Err(RuntimeError::TypeError(
                    "iterator return must return an object".into(),
                ));
            }
        }
        Ok(())
    }
    pub(super) fn binding_value(&mut self, slot: usize) -> Result<Option<Value>, RuntimeError> {
        if let Some(cell) = self.cells.get(&slot) {
            Ok(self.heap.get_own(*cell, "value")?)
        } else {
            Ok(self.bindings[slot].clone())
        }
    }

    pub(super) fn store_binding(&mut self, slot: usize, value: Value) -> Result<(), RuntimeError> {
        if let Some(&cell) = self.cells.get(&slot) {
            self.store_global_cell(cell, value)?;
        } else {
            self.bindings[slot] = Some(value);
        }
        Ok(())
    }

    pub(super) fn capture(&mut self, slot: usize) -> Result<ObjectId, RuntimeError> {
        if let Some(&cell) = self.cells.get(&slot) {
            return Ok(cell);
        }
        let cell = self.with_roots(|heap| heap.alloc_object(None))?;
        self.cells.insert(slot, cell);
        if let Some(value) = self.bindings[slot].take() {
            self.with_roots(|heap| heap.set(cell, "value", value))?;
        }
        Ok(cell)
    }

    pub(super) fn call_closure(&mut self, call: ClosureCall) -> Result<Value, RuntimeError> {
        let ClosureCall {
            code,
            captures,
            callee,
            receiver,
            args,
            construct,
            home,
            class_base,
        } = call;
        if code.class_constructor && !construct {
            return Err(RuntimeError::TypeError(
                "class constructor cannot be invoked without new".into(),
            ));
        }
        if construct && !code.constructible {
            return Err(RuntimeError::TypeError(
                "arrow function is not a constructor".into(),
            ));
        }
        // Async generator requests own their own Promise capabilities. Do not
        // allocate an ordinary async-function capability before entering the
        // generator branch, where it would be unreachable and leak state.
        let async_function = code.async_function && !code.generator;
        // AsyncFunctionStart creates the promise capability before executing
        // the body.  If the body reaches await, that same promise owns the
        // saved continuation; otherwise its immediate completion settles it.
        let async_promise = async_function.then(|| self.new_promise()).transpose()?;
        let receiver = if construct && code.derived_constructor {
            Value::Undefined
        } else if construct {
            let prototype = self.constructor_prototype(self.object_prototype)?;
            Value::Object(self.with_roots(|heap| heap.alloc_object(Some(prototype)))?)
        } else if code.arrow || code.strict {
            receiver
        } else if matches!(receiver, Value::Null | Value::Undefined) {
            self.global("globalThis")?
        } else {
            Value::Object(self.coerce_object(&receiver)?)
        };
        if code.generator {
            let async_generator = code.async_function;
            let default_prototype = if code.async_function {
                self.async_generator_prototype()?
            } else {
                self.generator_prototype()?
            };
            let prototype = self
                .get_property(&callee, &"prototype".into())?
                .object_id()
                .unwrap_or(default_prototype);
            if !code.generator_initializes_parameters {
                let state = GeneratorState::Start {
                    code,
                    captures,
                    callee,
                    receiver,
                    args,
                    home,
                };
                let generator = self.with_roots(|heap| heap.alloc_generator(state, prototype))?;
                if async_generator {
                    self.heap.enable_async_generator(generator)?;
                }
                return Ok(Value::Object(generator));
            }
            // Install a temporary state first so the generator owns every
            // captured edge while the entry phase may allocate. The actual
            // parameter frame replaces it below before the object escapes.
            let generator = self.with_roots(|heap| {
                heap.alloc_generator(
                    GeneratorState::Start {
                        code: code.clone(),
                        captures: captures.clone(),
                        callee: callee.clone(),
                        receiver: receiver.clone(),
                        args: args.clone(),
                        home,
                    },
                    prototype,
                )
            })?;
            if async_generator {
                self.heap.enable_async_generator(generator)?;
            }
            let base = self.stack.len();
            self.stack.push(Value::Object(generator));
            let state =
                self.initialize_generator(code, captures, callee.clone(), receiver, args, home);
            self.stack.truncate(base);
            let state = state?;
            // FunctionDeclarationInstantiation is observable to a parameter
            // initializer. Read `.prototype` again after it completes: a
            // default such as `(g.prototype = null)` must affect the freshly
            // created generator object's [[Prototype]].
            let prototype = self
                .get_property(&callee, &"prototype".into())?
                .object_id()
                .unwrap_or(default_prototype);
            self.heap.set_prototype(generator, Some(prototype))?;
            self.heap.set_generator_state(generator, state)?;
            return Ok(Value::Object(generator));
        }
        self.stack.push(receiver.clone());
        let base = self.stack.len();
        self.stack.extend(self.bindings.iter().flatten().cloned());
        self.stack
            .extend(self.cells.values().copied().map(Value::Object));
        self.stack.push(self.completion.clone());
        self.stack.push(self.this.clone());
        self.stack.extend(self.arguments.iter().cloned());
        let frame_base = self.stack.len();
        let mut frame_bindings = vec![None; code.bindings.len()];
        if let Some(slot) = code.self_slot {
            frame_bindings[slot as usize] = Some(callee.clone());
        }
        let bindings = std::mem::replace(&mut self.bindings, frame_bindings);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, code.bindings.clone());
        let cells = std::mem::replace(&mut self.cells, captures.into_iter().enumerate().collect());
        let dynamic_eval_bindings = std::mem::take(&mut self.dynamic_eval_bindings);
        let mut dynamic_eval_outer_bindings = std::mem::take(&mut self.dynamic_eval_outer_bindings);
        dynamic_eval_outer_bindings.push(dynamic_eval_bindings);
        self.dynamic_eval_outer_bindings = dynamic_eval_outer_bindings;
        let eval_dynamic_slots = std::mem::take(&mut self.eval_dynamic_slots);
        let script_global_slots = std::mem::take(&mut self.script_global_slots);
        let variable_scope = std::mem::replace(&mut self.variable_scope, code.variable_scope);
        let variable_scope_lexicals = std::mem::replace(
            &mut self.variable_scope_lexicals,
            code.scopes
                .get(code.variable_scope as usize)
                .into_iter()
                .flat_map(|scope| scope.iter())
                .filter_map(|slot| {
                    let binding = &code.bindings[*slot as usize];
                    binding.lexical.then(|| binding.name.clone())
                })
                .collect(),
        );
        let this = std::mem::replace(&mut self.this, receiver);
        let arguments = std::mem::replace(&mut self.arguments, args);
        let frame_callee = std::mem::replace(&mut self.callee, callee.clone());
        let completion = std::mem::replace(&mut self.completion, Value::Undefined);
        let completion_empty = std::mem::replace(&mut self.completion_empty, true);
        let active_scopes = std::mem::take(&mut self.active_scopes);
        let active_scope_slots = std::mem::take(&mut self.active_scope_slots);
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let home_object = std::mem::replace(&mut self.home_object, home);
        let next_field_initializer_depth = if code.arrow {
            self.class_field_initializer_depth
        } else {
            0
        };
        let class_field_initializer_depth = std::mem::replace(
            &mut self.class_field_initializer_depth,
            next_field_initializer_depth,
        );
        let derived_constructor_arrow = code.arrow && class_base.is_some();
        let class_constructor = std::mem::replace(
            &mut self.class_constructor,
            (code.class_constructor || derived_constructor_arrow).then(|| {
                callee
                    .object_id()
                    .expect("class and arrow closures are objects")
            }),
        );
        let pending_completions = self.pending_completions.clone();
        let completion_saves = self.completion_saves.clone();
        let with_objects = self.with_objects.clone();
        let frame_dynamic_eval_outer_bindings = self.dynamic_eval_outer_bindings.clone();
        let top_level_module = self.top_level_module;
        let remaining_instructions = self.remaining_instructions;
        let new_target = self.new_target.clone();
        let new_target_allowed = self.new_target_allowed;
        let active_module_name = self.active_module_name.clone();
        let result_root = self.result_root.take();
        let mut suspended_parent_stack = None;
        let mut suspended_async = None;
        let result = if async_function {
            let mut iterators = Vec::new();
            match self.interpret(&code, &mut iterators, 0, None, None, None) {
                Ok(InterpreterExit::Return(value)) => Ok(value),
                Ok(InterpreterExit::Await {
                    promise,
                    pc,
                    handlers,
                }) => {
                    let stack = self.stack.split_off(frame_base);
                    let mut execution = self.suspend_module_execution();
                    let parent_stack = std::mem::replace(&mut execution.stack, stack);
                    let templates = execution.templates.clone();
                    self.templates = templates;
                    let state = AsyncContinuation {
                        generator: None,
                        target: async_promise.expect("async function has a promise"),
                        code: code.clone(),
                        pc,
                        execution,
                        iterators,
                        handlers,
                        call_depth: self.call_depth,
                    };
                    suspended_parent_stack = Some(parent_stack);
                    suspended_async = Some((state, promise));
                    Ok(Value::Undefined)
                }
                Ok(InterpreterExit::Yield { .. }) => Err(RuntimeError::TypeError(
                    "yield requires an async generator function".into(),
                )),
                Ok(InterpreterExit::Suspend { .. }) => {
                    unreachable!("ordinary async functions have no entry suspend")
                }
                Err(error) => {
                    let base = self.stack.len();
                    if let RuntimeError::Thrown(value) = &error {
                        self.stack.push(value.clone());
                    }
                    self.stack.extend(iterators.iter().cloned());
                    for record in iterators.into_iter().rev() {
                        let _ = self.iterator_close(&record);
                    }
                    self.stack.truncate(base);
                    Err(error)
                }
            }
        } else {
            self.run(&code)
        };
        let constructed = self.this.clone();
        let suspended = suspended_parent_stack.is_some();
        if let Some(stack) = suspended_parent_stack {
            self.stack = stack;
            self.result_root = result_root;
            self.pending_completions = pending_completions;
            self.completion_saves = completion_saves;
            self.with_objects = with_objects;
            self.dynamic_eval_outer_bindings = frame_dynamic_eval_outer_bindings;
            self.top_level_module = top_level_module;
            self.remaining_instructions = remaining_instructions;
            self.new_target = new_target;
            self.new_target_allowed = new_target_allowed;
            self.active_module_name = active_module_name;
        } else {
            self.result_root = result_root;
        }
        self.bindings = bindings;
        self.binding_metadata = binding_metadata;
        self.cells = cells;
        let mut dynamic_eval_outer_bindings = std::mem::take(&mut self.dynamic_eval_outer_bindings);
        self.dynamic_eval_bindings = dynamic_eval_outer_bindings
            .pop()
            .expect("callee inherits its caller dynamic environment");
        self.dynamic_eval_outer_bindings = dynamic_eval_outer_bindings;
        self.eval_dynamic_slots = eval_dynamic_slots;
        self.script_global_slots = script_global_slots;
        self.variable_scope = variable_scope;
        self.variable_scope_lexicals = variable_scope_lexicals;
        // `super()` in a derived-constructor arrow initializes the enclosing
        // constructor's lexical `this` binding. Nested arrows propagate that
        // initialized receiver one frame at a time on return.
        self.this = if derived_constructor_arrow && matches!(constructed, Value::Object(_)) {
            constructed.clone()
        } else {
            this
        };
        self.arguments = arguments;
        self.callee = frame_callee;
        self.completion = completion;
        self.completion_empty = completion_empty;
        self.active_scopes = active_scopes;
        self.active_scope_slots = active_scope_slots;
        self.strict = strict;
        self.home_object = home_object;
        self.class_constructor = class_constructor;
        self.class_field_initializer_depth = class_field_initializer_depth;
        self.stack.truncate(base - 1);
        if let Some((state, awaited)) = suspended_async {
            self.suspend_async_await(state, awaited)?;
        }
        let result = result.and_then(|value| {
            if construct && !matches!(value, Value::Object(_)) {
                if matches!(constructed, Value::Object(_)) {
                    Ok(constructed)
                } else {
                    Err(RuntimeError::ReferenceError(
                        "derived constructor did not call super()".into(),
                    ))
                }
            } else {
                Ok(value)
            }
        });
        if !async_function {
            return result;
        }

        let promise = async_promise.expect("async function has a promise");
        match result {
            // `AsyncFunctionStart` resolves rather than directly fulfills so
            // `return somePromise` adopts its eventual settlement.
            Ok(value) if !suspended => self.resolve_promise(promise, value)?,
            Ok(_) => {}
            Err(error) => {
                let value = self.error_value(error)?;
                self.settle_promise(promise, PromiseStatus::Rejected(value))?;
            }
        }
        Ok(Value::Object(promise))
    }

    /// Generator function invocation performs parameter initialization now,
    /// then suspends immediately before body evaluation. This makes a direct
    /// eval in a default parameter observable (including its early errors)
    /// at `generatorFunction()` rather than at the first `.next()`.
    fn initialize_generator(
        &mut self,
        code: Rc<Bytecode>,
        captures: Vec<ObjectId>,
        callee: Value,
        receiver: Value,
        args: Vec<Value>,
        home: Option<ObjectId>,
    ) -> Result<GeneratorState, RuntimeError> {
        self.stack.push(receiver.clone());
        let base = self.stack.len();
        self.stack.extend(self.bindings.iter().flatten().cloned());
        self.stack
            .extend(self.cells.values().copied().map(Value::Object));
        self.stack.push(self.completion.clone());
        self.stack.push(self.this.clone());
        self.stack.extend(self.arguments.iter().cloned());
        let frame_base = self.stack.len();

        let mut frame_bindings = vec![None; code.bindings.len()];
        if let Some(slot) = code.self_slot {
            frame_bindings[slot as usize] = Some(callee.clone());
        }
        let bindings = std::mem::replace(&mut self.bindings, frame_bindings);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, code.bindings.clone());
        let cells = std::mem::replace(&mut self.cells, captures.into_iter().enumerate().collect());
        let dynamic_eval_bindings = std::mem::take(&mut self.dynamic_eval_bindings);
        let dynamic_eval_outer_bindings = std::mem::take(&mut self.dynamic_eval_outer_bindings);
        let eval_dynamic_slots = std::mem::take(&mut self.eval_dynamic_slots);
        let script_global_slots = std::mem::take(&mut self.script_global_slots);
        let this = std::mem::replace(&mut self.this, receiver);
        let arguments = std::mem::replace(&mut self.arguments, args);
        let frame_callee = std::mem::replace(&mut self.callee, callee);
        let completion = std::mem::replace(&mut self.completion, Value::Undefined);
        let completion_empty = std::mem::replace(&mut self.completion_empty, true);
        let active_scopes = std::mem::take(&mut self.active_scopes);
        let active_scope_slots = std::mem::take(&mut self.active_scope_slots);
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let home_object = std::mem::replace(&mut self.home_object, home);
        let variable_scope = std::mem::replace(&mut self.variable_scope, code.variable_scope);
        let variable_scope_lexicals = std::mem::replace(
            &mut self.variable_scope_lexicals,
            code.scopes
                .get(code.variable_scope as usize)
                .into_iter()
                .flat_map(|scope| scope.iter())
                .filter_map(|slot| {
                    let binding = &code.bindings[*slot as usize];
                    binding.lexical.then(|| binding.name.clone())
                })
                .collect(),
        );

        let mut iterators = Vec::new();
        let outcome = self.interpret(
            &code,
            &mut iterators,
            0,
            None,
            Some(code.generator_entry as usize),
            None,
        );
        let state = match outcome {
            Ok(InterpreterExit::Suspend { pc }) => {
                debug_assert_eq!(pc, code.generator_entry as usize);
                let stack = self.stack.split_off(frame_base);
                Ok(GeneratorState::Suspended {
                    code,
                    pc,
                    stack,
                    bindings: std::mem::take(&mut self.bindings),
                    cells: std::mem::take(&mut self.cells).into_iter().collect(),
                    this: std::mem::replace(&mut self.this, Value::Undefined),
                    args: std::mem::take(&mut self.arguments),
                    completion: std::mem::replace(&mut self.completion, Value::Undefined),
                    completion_empty: std::mem::replace(&mut self.completion_empty, true),
                    active_scopes: std::mem::take(&mut self.active_scopes),
                    iterators,
                    handlers: Vec::new(),
                    pending_completions: Vec::new(),
                    completion_saves: Vec::new(),
                    async_delegate: None,
                    dynamic_bindings: std::mem::take(&mut self.dynamic_eval_bindings)
                        .into_iter()
                        .map(|(name, binding)| (name, binding.cell, binding.shadowed_cells))
                        .collect(),
                    home: std::mem::take(&mut self.home_object),
                    callee: std::mem::replace(&mut self.callee, Value::Undefined),
                })
            }
            Ok(InterpreterExit::Return(_)) | Ok(InterpreterExit::Yield { .. }) => {
                unreachable!("generator entry contains only instantiation bytecode")
            }
            Ok(InterpreterExit::Await { .. }) => {
                unreachable!("generator entry cannot contain top-level await")
            }
            Err(error) => Err(error),
        };

        self.bindings = bindings;
        self.binding_metadata = binding_metadata;
        self.cells = cells;
        self.dynamic_eval_bindings = dynamic_eval_bindings;
        self.dynamic_eval_outer_bindings = dynamic_eval_outer_bindings;
        self.eval_dynamic_slots = eval_dynamic_slots;
        self.script_global_slots = script_global_slots;
        self.this = this;
        self.arguments = arguments;
        self.callee = frame_callee;
        self.completion = completion;
        self.completion_empty = completion_empty;
        self.active_scopes = active_scopes;
        self.active_scope_slots = active_scope_slots;
        self.strict = strict;
        self.home_object = home_object;
        self.variable_scope = variable_scope;
        self.variable_scope_lexicals = variable_scope_lexicals;
        self.stack.truncate(base - 1);
        state
    }

    fn generator_next(
        &mut self,
        receiver: &Value,
        value: Option<Value>,
        async_target: Option<ObjectId>,
    ) -> Result<Value, RuntimeError> {
        self.generator_resume(receiver, value, async_target, None)
    }

    /// Resume a generator either normally after `yield`, or with an abrupt
    /// completion supplied by `.return()` / `.throw()`.  The latter must pass
    /// through saved catch and finally records instead of closing the frame
    /// at the builtin boundary.
    fn generator_resume(
        &mut self,
        receiver: &Value,
        value: Option<Value>,
        async_target: Option<ObjectId>,
        abrupt: Option<Completion>,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(generator) = receiver else {
            return Err(RuntimeError::TypeError(
                "Generator next requires a generator".into(),
            ));
        };
        let state = self.heap.take_generator_state(*generator)?;
        let (
            code,
            pc,
            resume_value,
            frame_stack,
            frame_bindings,
            frame_cells,
            frame_this,
            frame_args,
            frame_completion,
            frame_completion_empty,
            frame_scopes,
            frame_iterators,
            frame_handlers,
            frame_pending_completions,
            frame_completion_saves,
            frame_home,
            frame_callee,
            frame_variable_scope,
            frame_variable_scope_lexicals,
            frame_dynamic_bindings,
        ) = match state {
            GeneratorState::Done => {
                self.heap
                    .set_generator_state(*generator, GeneratorState::Done)?;
                if let Some(Completion::Throw(error)) = abrupt {
                    return Err(error);
                }
                if let Some(Completion::Return(value)) = abrupt {
                    return self.iterator_result(value, true);
                }
                return self.iterator_result(Value::Undefined, true);
            }
            GeneratorState::Start {
                code,
                captures,
                callee,
                receiver,
                args,
                home,
            } => {
                let mut bindings = vec![None; code.bindings.len()];
                let variable_scope = code.variable_scope;
                let variable_scope_lexicals = code
                    .scopes
                    .get(variable_scope as usize)
                    .into_iter()
                    .flat_map(|scope| scope.iter())
                    .filter_map(|slot| {
                        let binding = &code.bindings[*slot as usize];
                        binding.lexical.then(|| binding.name.clone())
                    })
                    .collect();
                if let Some(slot) = code.self_slot {
                    bindings[slot as usize] = Some(callee.clone());
                }
                (
                    code,
                    0,
                    None,
                    Vec::new(),
                    bindings,
                    captures.into_iter().enumerate().collect(),
                    receiver,
                    args,
                    Value::Undefined,
                    true,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    home,
                    callee,
                    variable_scope,
                    variable_scope_lexicals,
                    HashMap::new(),
                )
            }
            GeneratorState::Suspended {
                code,
                pc,
                stack,
                bindings,
                cells,
                this,
                args,
                completion,
                completion_empty,
                active_scopes,
                iterators,
                handlers,
                pending_completions,
                completion_saves,
                async_delegate: _,
                dynamic_bindings,
                home,
                callee,
            } => {
                let variable_scope = code.variable_scope;
                let variable_scope_lexicals = code
                    .scopes
                    .get(variable_scope as usize)
                    .into_iter()
                    .flat_map(|scope| scope.iter())
                    .filter_map(|slot| {
                        let binding = &code.bindings[*slot as usize];
                        binding.lexical.then(|| binding.name.clone())
                    })
                    .collect();
                (
                    code,
                    pc,
                    value,
                    stack,
                    bindings,
                    cells.into_iter().collect(),
                    this,
                    args,
                    completion,
                    completion_empty,
                    active_scopes,
                    iterators,
                    handlers,
                    pending_completions,
                    completion_saves,
                    home,
                    callee,
                    variable_scope,
                    variable_scope_lexicals,
                    dynamic_bindings
                        .into_iter()
                        .map(|(name, cell, shadowed_cells)| {
                            (
                                name,
                                DynamicEvalBinding {
                                    cell,
                                    shadowed_cells,
                                },
                            )
                        })
                        .collect(),
                )
            }
        };

        let base = self.stack.len();
        let remaining_instructions = self.remaining_instructions;
        self.stack.extend(self.bindings.iter().flatten().cloned());
        self.stack
            .extend(self.cells.values().copied().map(Value::Object));
        self.stack.push(self.completion.clone());
        self.stack.push(self.this.clone());
        self.stack.extend(self.arguments.iter().cloned());
        let frame_base = self.stack.len();
        self.stack.extend(frame_stack.iter().cloned());

        let bindings = std::mem::replace(&mut self.bindings, frame_bindings);
        let binding_metadata = std::mem::replace(&mut self.binding_metadata, code.bindings.clone());
        let cells = std::mem::replace(&mut self.cells, frame_cells);
        let dynamic_eval_outer_bindings = std::mem::take(&mut self.dynamic_eval_outer_bindings);
        let dynamic_eval_bindings =
            std::mem::replace(&mut self.dynamic_eval_bindings, frame_dynamic_bindings);
        let eval_dynamic_slots = std::mem::take(&mut self.eval_dynamic_slots);
        let this = std::mem::replace(&mut self.this, frame_this);
        let arguments = std::mem::replace(&mut self.arguments, frame_args);
        let completion = std::mem::replace(&mut self.completion, frame_completion);
        let completion_empty =
            std::mem::replace(&mut self.completion_empty, frame_completion_empty);
        let active_scopes = std::mem::replace(&mut self.active_scopes, frame_scopes);
        let active_scope_slots = std::mem::replace(
            &mut self.active_scope_slots,
            self.active_scopes
                .iter()
                .map(|scope| code.scopes[*scope as usize].clone())
                .collect(),
        );
        let strict = std::mem::replace(&mut self.strict, code.strict);
        let home_object = std::mem::replace(&mut self.home_object, frame_home);
        let callee = std::mem::replace(&mut self.callee, frame_callee);
        let variable_scope = std::mem::replace(&mut self.variable_scope, frame_variable_scope);
        let variable_scope_lexicals = std::mem::replace(
            &mut self.variable_scope_lexicals,
            frame_variable_scope_lexicals,
        );
        let pending_completions = std::mem::replace(
            &mut self.pending_completions,
            frame_pending_completions
                .into_iter()
                .map(Completion::from_generator_pending)
                .collect(),
        );
        let completion_saves =
            std::mem::replace(&mut self.completion_saves, frame_completion_saves);
        let mut iterators = frame_iterators;
        let mut handlers = frame_handlers;
        let outcome = if let Some(completion) = abrupt {
            // `GeneratorState` keeps handler offsets relative to its saved
            // operand stack. `resolve_completion` runs before `interpret`,
            // so inflate them here, then return them to their stored form
            // before the resumed interpreter takes ownership.
            for handler in &mut handlers {
                handler.stack_depth += frame_base;
            }
            let action = self.resolve_completion(&code, &mut handlers, &mut iterators, completion);
            for handler in &mut handlers {
                handler.stack_depth -= frame_base;
            }
            match action {
                Ok(CompletionAction::Continue) => self.interpret(
                    &code,
                    &mut iterators,
                    pc,
                    None,
                    None,
                    Some((handlers, frame_base)),
                ),
                Ok(CompletionAction::Jump(target)) => self.interpret(
                    &code,
                    &mut iterators,
                    target,
                    None,
                    None,
                    Some((handlers, frame_base)),
                ),
                Ok(CompletionAction::Return(value)) => Ok(InterpreterExit::Return(value)),
                Ok(CompletionAction::TailRecur(_)) => Err(RuntimeError::TypeError(
                    "generator cannot tail recur across an abrupt resume".into(),
                )),
                Ok(CompletionAction::Throw(error)) | Err(error) => Err(error),
            }
        } else {
            self.interpret(
                &code,
                &mut iterators,
                pc,
                resume_value,
                None,
                Some((handlers, frame_base)),
            )
        };
        let mut suspended_async = None;

        let (next_state, result) = match outcome {
            Ok(InterpreterExit::Return(value)) => {
                // An injected generator return skips the compiler's normal
                // IteratorFinish/IteratorClose instructions. Keep the
                // completion and every active record rooted while close
                // callbacks can allocate, then discard the frame as done.
                let close_base = self.stack.len();
                self.stack.push(value.clone());
                self.stack.extend(iterators.iter().cloned());
                self.close_iterators_to(&mut iterators, 0);
                self.stack.truncate(close_base);
                self.stack.truncate(frame_base);
                (GeneratorState::Done, Ok((value, true)))
            }
            Ok(InterpreterExit::Yield {
                value,
                pc,
                iterators,
                handlers,
            }) => {
                let stack = self.stack.split_off(frame_base);
                let async_delegate = code
                    .async_yield_delegates
                    .iter()
                    .find(|(resume, _)| *resume as usize == pc)
                    .and_then(|(_, exit_pc)| {
                        stack.last().cloned().map(|record| AsyncGeneratorDelegate {
                            record,
                            exit_pc: *exit_pc as usize,
                        })
                    });
                let state = GeneratorState::Suspended {
                    code,
                    pc,
                    stack,
                    bindings: std::mem::take(&mut self.bindings),
                    cells: std::mem::take(&mut self.cells).into_iter().collect(),
                    this: std::mem::replace(&mut self.this, Value::Undefined),
                    args: std::mem::take(&mut self.arguments),
                    completion: std::mem::replace(&mut self.completion, Value::Undefined),
                    completion_empty: std::mem::replace(&mut self.completion_empty, true),
                    active_scopes: std::mem::take(&mut self.active_scopes),
                    iterators,
                    handlers,
                    pending_completions: std::mem::take(&mut self.pending_completions)
                        .into_iter()
                        .map(|completion| {
                            completion
                                .into_generator_pending()
                                .expect("only catchable completions can survive a generator yield")
                        })
                        .collect(),
                    completion_saves: std::mem::take(&mut self.completion_saves),
                    async_delegate,
                    dynamic_bindings: std::mem::take(&mut self.dynamic_eval_bindings)
                        .into_iter()
                        .map(|(name, binding)| (name, binding.cell, binding.shadowed_cells))
                        .collect(),
                    home: std::mem::take(&mut self.home_object),
                    callee: std::mem::replace(&mut self.callee, Value::Undefined),
                };
                (state, Ok((value, false)))
            }
            Err(error) => {
                // The same applies when a resumed generator completes
                // abruptly without a remaining handler. In particular, a
                // return()/throw() request must close a destructuring
                // iterator that was live at the preceding yield.
                let close_base = self.stack.len();
                if let RuntimeError::Thrown(value) = &error {
                    self.stack.push(value.clone());
                }
                self.stack.extend(iterators.iter().cloned());
                self.close_iterators_to(&mut iterators, 0);
                self.stack.truncate(close_base);
                self.stack.truncate(frame_base);
                (GeneratorState::Done, Err(error))
            }
            Ok(InterpreterExit::Suspend { .. }) => {
                unreachable!("ordinary generator execution has no suspend boundary")
            }
            Ok(InterpreterExit::Await {
                promise,
                pc,
                handlers,
            }) => {
                if let Some(target) = async_target {
                    let stack = self.stack.split_off(frame_base);
                    let mut execution = self.suspend_module_execution();
                    let parent_stack = std::mem::replace(&mut execution.stack, stack);
                    self.stack = parent_stack;
                    let templates = execution.templates.clone();
                    self.templates = templates;
                    suspended_async = Some((
                        AsyncContinuation {
                            generator: Some(*generator),
                            target,
                            code: code.clone(),
                            pc,
                            execution,
                            iterators,
                            handlers,
                            call_depth: self.call_depth,
                        },
                        promise,
                    ));
                    (GeneratorState::Done, Ok((Value::Undefined, true)))
                } else {
                    self.stack.truncate(frame_base);
                    (
                        GeneratorState::Done,
                        Err(RuntimeError::TypeError(
                            "await requires an async generator function".into(),
                        )),
                    )
                }
            }
        };
        self.heap.set_generator_state(*generator, next_state)?;
        self.bindings = bindings;
        self.binding_metadata = binding_metadata;
        self.cells = cells;
        self.dynamic_eval_bindings = dynamic_eval_bindings;
        self.dynamic_eval_outer_bindings = dynamic_eval_outer_bindings;
        self.eval_dynamic_slots = eval_dynamic_slots;
        self.this = this;
        self.arguments = arguments;
        self.completion = completion;
        self.completion_empty = completion_empty;
        self.pending_completions = pending_completions;
        self.completion_saves = completion_saves;
        self.active_scopes = active_scopes;
        self.active_scope_slots = active_scope_slots;
        self.strict = strict;
        self.home_object = home_object;
        self.callee = callee;
        self.variable_scope = variable_scope;
        self.variable_scope_lexicals = variable_scope_lexicals;
        self.remaining_instructions = remaining_instructions;
        self.stack.truncate(base);
        if let Some((state, promise)) = suspended_async {
            self.suspend_async_await(state, promise)?;
            return Ok(Value::Undefined);
        }
        let (value, done) = result?;
        self.iterator_result(value, done)
    }

    fn generator_return(&mut self, receiver: &Value, value: Value) -> Result<Value, RuntimeError> {
        self.generator_resume(receiver, None, None, Some(Completion::Return(value)))
    }

    /// Returns the explicit delegation record installed when the compiler's
    /// async `yield*` loop reached a public yield boundary. It is frame
    /// metadata, so forwarding never depends on recognizing bytecode layout.
    fn yield_star_delegate(state: &GeneratorState) -> Option<(Value, usize)> {
        let GeneratorState::Suspended {
            async_delegate: Some(delegate),
            ..
        } = state
        else {
            return None;
        };
        Some((delegate.record.clone(), delegate.exit_pc))
    }

    /// Starts forwarding an abrupt outer request into a suspended async
    /// `yield*` delegate. `None` means this is an ordinary generator yield
    /// and the caller must retain its standard return/throw behaviour.
    fn async_generator_delegate_request(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        kind: AsyncGeneratorDelegateKind,
        value: Value,
    ) -> Result<Option<Value>, RuntimeError> {
        let state = self.heap.take_generator_state(generator)?;
        let Some((record, _)) = Self::yield_star_delegate(&state) else {
            self.heap.set_generator_state(generator, state)?;
            return Ok(None);
        };
        self.heap.set_generator_state(generator, state)?;
        let Value::Object(record) = record else {
            unreachable!("yield* keeps an iterator record on its stack")
        };
        let iterator = self.get_property(&Value::Object(record), &"iterator".into())?;
        let name = match kind {
            AsyncGeneratorDelegateKind::Return => "return",
            AsyncGeneratorDelegateKind::Throw => "throw",
        };
        let method = self.get_method(&iterator, &name.into())?;
        if method == Value::Undefined {
            if matches!(kind, AsyncGeneratorDelegateKind::Throw) {
                self.close_async_generator(generator)?;
                return Err(RuntimeError::TypeError(
                    "yield* iterator does not provide a throw method".into(),
                ));
            }
            // GetMethod already observed the delegate's `return` property.
            // The abrupt completion still crosses the outer generator's
            // finally records.  `generator_resume` owns both that cleanup
            // and the completed-start special case.
            return self
                .generator_resume(
                    &Value::Object(generator),
                    None,
                    Some(target),
                    Some(Completion::Return(value)),
                )
                .map(Some);
        }
        let result = self.call_native(method, iterator, vec![value], false)?;
        let promise = self
            .promise_resolve(result)?
            .object_id()
            .expect("Promise.resolve returns a Promise");
        match self
            .promises
            .get(&promise)
            .expect("Promise.resolve registers its Promise")
            .status
            .clone_for_await()
        {
            PromiseAwaitStatus::Pending => self
                .promises
                .get_mut(&promise)
                .expect("checked pending Promise exists")
                .reactions
                .push(PromiseReaction::AsyncGeneratorDelegate {
                    generator,
                    target,
                    kind,
                }),
            PromiseAwaitStatus::Fulfilled(value) => {
                self.finish_async_generator_delegate(generator, target, kind, value, true)?
            }
            PromiseAwaitStatus::Rejected(value) => {
                self.finish_async_generator_delegate(generator, target, kind, value, false)?
            }
        }
        Ok(Some(Value::Undefined))
    }

    fn set_async_generator_status(
        &mut self,
        generator: ObjectId,
        status: AsyncGeneratorStatus,
    ) -> Result<(), RuntimeError> {
        let mut control = self
            .heap
            .async_generator_control(generator)?
            .ok_or_else(|| RuntimeError::TypeError("Async generator receiver required".into()))?;
        control.status = status;
        self.heap.set_async_generator_control(generator, control)?;
        Ok(())
    }

    /// Settles only the queue head. Completion moves the state back to a
    /// resumable boundary before the next request is considered, so a second
    /// `.next()` can never observe the `Done` placeholder used while an async
    /// continuation owns the frame.
    pub(super) fn complete_async_generator_request(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        status: PromiseStatus,
    ) -> Result<(), RuntimeError> {
        let mut control = self
            .heap
            .async_generator_control(generator)?
            .ok_or_else(|| RuntimeError::TypeError("Async generator receiver required".into()))?;
        let request = control
            .requests
            .pop_front()
            .ok_or(RuntimeError::Unsupported("missing async generator request"))?;
        if request.target != target {
            return Err(RuntimeError::Unsupported(
                "async generator request completed out of order",
            ));
        }
        if control
            .requests
            .iter()
            .any(|queued| queued.id <= request.id)
        {
            return Err(RuntimeError::Unsupported(
                "async generator queue identifiers are not FIFO",
            ));
        }
        control.status = if self.heap.generator_state_is_done(generator)? {
            AsyncGeneratorStatus::Completed
        } else {
            AsyncGeneratorStatus::SuspendedYield
        };
        self.heap.set_async_generator_control(generator, control)?;
        self.settle_promise(target, status)
    }

    /// Implements the single `AsyncGeneratorResumeNext` scheduler. Only the
    /// queue head may enter the interpreter; an `Awaiting` generator keeps
    /// every later request as heap-owned data until its Promise job completes.
    pub(super) fn resume_async_generator_next(
        &mut self,
        generator: ObjectId,
    ) -> Result<(), RuntimeError> {
        loop {
            let control = self
                .heap
                .async_generator_control(generator)?
                .ok_or_else(|| {
                    RuntimeError::TypeError("Async generator receiver required".into())
                })?;
            if control.requests.is_empty()
                || matches!(
                    control.status,
                    AsyncGeneratorStatus::Awaiting | AsyncGeneratorStatus::Executing
                )
            {
                return Ok(());
            }
            let request = control
                .requests
                .front()
                .cloned()
                .expect("checked async generator queue is non-empty");
            if control.status == AsyncGeneratorStatus::Completed {
                let completion = match request.completion {
                    AsyncGeneratorCompletion::Next(_) => {
                        PromiseStatus::Fulfilled(self.iterator_result(Value::Undefined, true)?)
                    }
                    AsyncGeneratorCompletion::Return(value) => {
                        PromiseStatus::Fulfilled(self.iterator_result(value, true)?)
                    }
                    AsyncGeneratorCompletion::Throw(value) => PromiseStatus::Rejected(value),
                };
                self.complete_async_generator_request(generator, request.target, completion)?;
                continue;
            }

            self.set_async_generator_status(generator, AsyncGeneratorStatus::Executing)?;
            let receiver = Value::Object(generator);
            let result = match request.completion {
                AsyncGeneratorCompletion::Next(value) => {
                    self.generator_next(&receiver, Some(value), Some(request.target))
                }
                AsyncGeneratorCompletion::Return(value) => {
                    match self.async_generator_delegate_request(
                        generator,
                        request.target,
                        AsyncGeneratorDelegateKind::Return,
                        value.clone(),
                    )? {
                        Some(result) => Ok(result),
                        None => self.generator_resume(
                            &receiver,
                            None,
                            Some(request.target),
                            Some(Completion::Return(value)),
                        ),
                    }
                }
                AsyncGeneratorCompletion::Throw(value) => {
                    if let Some(result) = self.async_generator_delegate_request(
                        generator,
                        request.target,
                        AsyncGeneratorDelegateKind::Throw,
                        value.clone(),
                    )? {
                        Ok(result)
                    } else {
                        self.generator_resume(
                            &receiver,
                            None,
                            Some(request.target),
                            Some(Completion::Throw(RuntimeError::Thrown(value))),
                        )
                    }
                }
            };
            match result {
                // `generator_next` uses this private sentinel after moving
                // the body frame into an async continuation. Delegate awaits
                // use it too; in both cases the queue head remains pending.
                Ok(Value::Undefined)
                    if matches!(
                        self.promises
                            .get(&request.target)
                            .expect("queued async-generator target exists")
                            .status,
                        PromiseStatus::Pending
                    ) =>
                {
                    return self
                        .set_async_generator_status(generator, AsyncGeneratorStatus::Awaiting);
                }
                Ok(Value::Undefined) => return Ok(()),
                Ok(result) => {
                    return self.await_async_generator_yield(generator, request.target, result);
                }
                Err(error) => {
                    let value = self.error_value(error)?;
                    self.complete_async_generator_request(
                        generator,
                        request.target,
                        PromiseStatus::Rejected(value),
                    )?;
                }
            }
        }
    }

    /// Async generator methods append one request and return its capability.
    /// The scheduler is the only path that may enter the generator frame.
    fn async_generator_request(
        &mut self,
        receiver: &Value,
        value: Value,
        kind: NativeFunction,
    ) -> Result<Value, RuntimeError> {
        let generator = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("AsyncGenerator request requires an async generator".into())
        })?;
        let base = self.stack.len();
        self.stack.extend([receiver.clone(), value.clone()]);
        let result = (|| {
            let target = self.new_promise()?;
            self.stack.push(Value::Object(target));
            let mut control = self
                .heap
                .async_generator_control(generator)?
                .ok_or_else(|| {
                    RuntimeError::TypeError(
                        "AsyncGenerator request requires an async generator".into(),
                    )
                })?;
            let id = control.next_request_id;
            control.next_request_id =
                control
                    .next_request_id
                    .checked_add(1)
                    .ok_or(RuntimeError::RangeError(
                        "async generator request identifiers exhausted".into(),
                    ))?;
            let completion = match kind {
                NativeFunction::AsyncGeneratorNext => AsyncGeneratorCompletion::Next(value),
                NativeFunction::AsyncGeneratorReturn => AsyncGeneratorCompletion::Return(value),
                NativeFunction::AsyncGeneratorThrow => AsyncGeneratorCompletion::Throw(value),
                _ => unreachable!("only async generator request kinds reach this helper"),
            };
            control.requests.push_back(AsyncGeneratorRequest {
                id,
                completion,
                target,
            });
            self.heap.set_async_generator_control(generator, control)?;
            self.resume_async_generator_next(generator)?;
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }

    /// Implements the Await in AsyncGeneratorYield. The generator is already
    /// suspended at this point, so only its outstanding request capability
    /// waits; fulfillment writes the awaited value into its IteratorResult.
    pub(super) fn await_async_generator_yield(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        result: Value,
    ) -> Result<(), RuntimeError> {
        let result = result
            .object_id()
            .expect("generator resumes always produce an IteratorResult object");
        let base = self.stack.len();
        self.stack.extend([
            Value::Object(generator),
            Value::Object(target),
            Value::Object(result),
        ]);
        let outcome = (|| {
            self.set_async_generator_status(generator, AsyncGeneratorStatus::Awaiting)?;
            let value = self.get_property(&Value::Object(result), &"value".into())?;
            self.stack.push(value.clone());
            let awaited = self.promise_resolve(value)?;
            let awaited = awaited
                .object_id()
                .expect("Promise.resolve always returns a Promise");
            let status = self
                .promises
                .get(&awaited)
                .expect("Promise.resolve registers its Promise")
                .status
                .clone_for_await();
            match status {
                PromiseAwaitStatus::Pending => self
                    .promises
                    .get_mut(&awaited)
                    .expect("checked pending Promise exists")
                    .reactions
                    .push(PromiseReaction::AsyncGeneratorYield {
                        generator,
                        target,
                        result,
                    }),
                PromiseAwaitStatus::Fulfilled(value) => {
                    // Await always crosses a Promise job boundary, including
                    // for a value that Promise.resolve fulfilled immediately.
                    self.promise_jobs
                        .push_back(PromiseJob::AsyncGeneratorYield {
                            generator,
                            target,
                            result,
                            value,
                            fulfilled: true,
                        });
                }
                PromiseAwaitStatus::Rejected(value) => {
                    self.promise_jobs
                        .push_back(PromiseJob::AsyncGeneratorYield {
                            generator,
                            target,
                            result,
                            value,
                            fulfilled: false,
                        });
                }
            }
            Ok(())
        })();
        self.stack.truncate(base);
        outcome
    }

    fn finish_async_generator_yield(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        result: ObjectId,
        value: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        if !fulfilled {
            self.close_async_generator(generator)?;
            self.complete_async_generator_request(
                generator,
                target,
                PromiseStatus::Rejected(value),
            )?;
            return self.resume_async_generator_next(generator);
        }
        self.set_property(&Value::Object(result), &"value".into(), &value)?;
        self.complete_async_generator_request(
            generator,
            target,
            PromiseStatus::Fulfilled(Value::Object(result)),
        )?;
        self.resume_async_generator_next(generator)
    }

    fn finish_async_generator_delegate(
        &mut self,
        generator: ObjectId,
        target: ObjectId,
        kind: AsyncGeneratorDelegateKind,
        result: Value,
        fulfilled: bool,
    ) -> Result<(), RuntimeError> {
        if !fulfilled {
            self.close_async_generator(generator)?;
            self.complete_async_generator_request(
                generator,
                target,
                PromiseStatus::Rejected(result),
            )?;
            return self.resume_async_generator_next(generator);
        }
        if !matches!(result, Value::Object(_)) {
            self.close_async_generator(generator)?;
            let error = self.error_object(
                "TypeError",
                "yield* delegate method must return an object".into(),
            )?;
            self.complete_async_generator_request(
                generator,
                target,
                PromiseStatus::Rejected(error),
            )?;
            return self.resume_async_generator_next(generator);
        }
        let done = self.get_property(&result, &"done".into())?;
        let value = self.get_property(&result, &"value".into())?;
        if !self.to_boolean(&done)? {
            let iterator_result = self.iterator_result(value, false)?;
            return self.await_async_generator_yield(generator, target, iterator_result);
        }
        match kind {
            AsyncGeneratorDelegateKind::Return => {
                // A completed delegate supplies the final value of the
                // delegated expression, but the original outer return still
                // propagates through any enclosing finally blocks.
                let mut state = self.heap.take_generator_state(generator)?;
                let Some((_, exit)) = Self::yield_star_delegate(&state) else {
                    return Err(RuntimeError::Unsupported(
                        "lost async yield* delegation state",
                    ));
                };
                let GeneratorState::Suspended {
                    pc,
                    stack,
                    async_delegate,
                    ..
                } = &mut state
                else {
                    unreachable!("yield* delegation is always suspended")
                };
                *pc = exit;
                *async_delegate = None;
                *stack
                    .last_mut()
                    .expect("yield* delegation keeps its iterator record") = value.clone();
                self.heap.set_generator_state(generator, state)?;
                let result = self.generator_resume(
                    &Value::Object(generator),
                    None,
                    Some(target),
                    Some(Completion::Return(value)),
                );
                match result {
                    Ok(Value::Undefined) => Ok(()),
                    Ok(result) => self.await_async_generator_yield(generator, target, result),
                    Err(error) => {
                        let error = self.error_value(error)?;
                        self.complete_async_generator_request(
                            generator,
                            target,
                            PromiseStatus::Rejected(error),
                        )?;
                        self.resume_async_generator_next(generator)
                    }
                }
            }
            AsyncGeneratorDelegateKind::Throw => {
                let mut state = self.heap.take_generator_state(generator)?;
                let Some((_, exit)) = Self::yield_star_delegate(&state) else {
                    return Err(RuntimeError::Unsupported(
                        "lost async yield* delegation state",
                    ));
                };
                let GeneratorState::Suspended {
                    pc,
                    stack,
                    async_delegate,
                    ..
                } = &mut state
                else {
                    unreachable!("yield* delegation is always suspended")
                };
                *pc = exit;
                *async_delegate = None;
                *stack
                    .last_mut()
                    .expect("yield* delegation keeps its iterator record") = value;
                self.heap.set_generator_state(generator, state)?;
                let result = self.generator_next(&Value::Object(generator), None, Some(target));
                match result {
                    Ok(Value::Undefined) => Ok(()),
                    Ok(result) => self.await_async_generator_yield(generator, target, result),
                    Err(error) => {
                        let error = self.error_value(error)?;
                        self.complete_async_generator_request(
                            generator,
                            target,
                            PromiseStatus::Rejected(error),
                        )?;
                        self.resume_async_generator_next(generator)
                    }
                }
            }
        }
    }

    fn close_async_generator(&mut self, generator: ObjectId) -> Result<(), RuntimeError> {
        let state = self.heap.take_generator_state(generator)?;
        let iterators = match state {
            GeneratorState::Suspended { iterators, .. } => iterators,
            GeneratorState::Start { .. } | GeneratorState::Done => Vec::new(),
        };
        self.heap
            .set_generator_state(generator, GeneratorState::Done)?;
        let base = self.stack.len();
        self.stack.extend(iterators.iter().cloned());
        let result = iterators
            .iter()
            .rev()
            .try_for_each(|record| self.iterator_close(record));
        self.stack.truncate(base);
        result
    }

    fn promise_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.promise_prototype {
            return Ok(prototype);
        }
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let function_prototype = self.function_prototype()?;
        self.install_native(
            prototype,
            function_prototype,
            "then",
            2,
            NativeFunction::PromiseThen,
        )?;
        self.install_native(
            prototype,
            function_prototype,
            "catch",
            1,
            NativeFunction::PromiseCatch,
        )?;
        self.install_native(
            prototype,
            function_prototype,
            "finally",
            1,
            NativeFunction::PromiseFinally,
        )?;
        self.promise_prototype = Some(prototype);
        Ok(prototype)
    }

    /// Map and Set have distinct ordinary prototypes.  This shared bootstrap
    /// keeps constructor/new-target inheritance correct before collection
    /// entries and iterators are introduced.
    fn collection_prototype(&mut self, map: bool) -> Result<ObjectId, RuntimeError> {
        let cached = if map {
            self.map_prototype
        } else {
            self.set_prototype
        };
        if let Some(prototype) = cached {
            return Ok(prototype);
        }
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        self.stack.push(Value::Object(prototype));
        let result = self.define_data(
            prototype,
            JsSymbol::well_known("toStringTag"),
            Value::String(if map { "Map" } else { "Set" }.into()),
            false,
            false,
            true,
        );
        self.stack.pop();
        result?;
        if map {
            self.map_prototype = Some(prototype);
        } else {
            self.set_prototype = Some(prototype);
        }
        Ok(prototype)
    }

    fn collection_constructor(
        &mut self,
        map: bool,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                if map {
                    "Map constructor must be called with new"
                } else {
                    "Set constructor must be called with new"
                }
                .into(),
            ));
        }
        let default = self.collection_prototype(map)?;
        let prototype = self.constructor_prototype(default)?;
        let collection = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(collection));
        let result = self.define_data(collection, "size", Value::Number(0.0), false, false, true);
        self.stack.pop();
        result?;
        Ok(Value::Object(collection))
    }

    pub(super) fn new_promise(&mut self) -> Result<ObjectId, RuntimeError> {
        let prototype = self.promise_prototype()?;
        let promise = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.promises.insert(
            promise,
            PromiseRecord {
                status: PromiseStatus::Pending,
                reactions: Vec::new(),
            },
        );
        Ok(promise)
    }

    /// Create one of the resolving functions belonging to a Promise
    /// capability. Their target is private native-function state rather than
    /// a JavaScript-visible property, which keeps `resolve.call(...)` and
    /// `reject.call(...)` correct.
    fn promise_resolving_function(
        &mut self,
        promise: ObjectId,
        fulfill: bool,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.function_prototype()?;
        let function = NativeFunction::PromiseResolvingFunction { promise, fulfill };
        let id = self.with_roots(|heap| heap.alloc_native_function(function, "", prototype))?;
        self.stack.push(Value::Object(id));
        let result = (|| {
            self.define_data(id, "name", Value::String("".into()), false, false, true)?;
            self.define_data(id, "length", Value::Number(1.0), false, false, true)?;
            Ok(Value::Object(id))
        })();
        self.stack.pop();
        result
    }

    fn promise_constructor(
        &mut self,
        executor: Value,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Promise constructor must be called with new".into(),
            ));
        }
        if !self.is_callable(&executor)? {
            return Err(RuntimeError::TypeError(
                "Promise resolver is not a function".into(),
            ));
        }
        let promise = self.new_promise()?;
        // The capability must survive allocations for its resolving functions
        // and for executor invocation.
        self.stack.push(Value::Object(promise));
        let result = (|| {
            let resolve = self.promise_resolving_function(promise, true)?;
            let reject = self.promise_resolving_function(promise, false)?;
            match self.call_native(executor, Value::Undefined, vec![resolve, reject], false) {
                Ok(_) => {}
                Err(RuntimeError::Thrown(value)) => {
                    self.settle_promise(promise, PromiseStatus::Rejected(value))?;
                }
                Err(error) => {
                    let value = self.error_value(error)?;
                    self.settle_promise(promise, PromiseStatus::Rejected(value))?;
                }
            }
            Ok(Value::Object(promise))
        })();
        self.stack.pop();
        result
    }

    fn promise_with_resolvers(&mut self) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let promise = self.new_promise()?;
        self.stack.push(Value::Object(promise));
        let result = (|| {
            let resolve = self.promise_resolving_function(promise, true)?;
            let reject = self.promise_resolving_function(promise, false)?;
            self.stack.extend([resolve.clone(), reject.clone()]);
            let object_prototype = self.object_prototype;
            let capability = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
            self.stack.push(Value::Object(capability));
            self.define_data(
                capability,
                "promise",
                Value::Object(promise),
                true,
                true,
                true,
            )?;
            self.define_data(capability, "resolve", resolve, true, true, true)?;
            self.define_data(capability, "reject", reject, true, true, true)?;
            Ok(Value::Object(capability))
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn settle_promise(
        &mut self,
        promise: ObjectId,
        status: PromiseStatus,
    ) -> Result<(), RuntimeError> {
        let record = self
            .promises
            .get_mut(&promise)
            .ok_or(RuntimeError::TypeError("invalid Promise receiver".into()))?;
        if !matches!(record.status, PromiseStatus::Pending) {
            return Ok(());
        }
        let fulfilled = matches!(status, PromiseStatus::Fulfilled(_));
        let value = match &status {
            PromiseStatus::Fulfilled(value) | PromiseStatus::Rejected(value) => value.clone(),
            PromiseStatus::Pending => unreachable!("Promise settlement is final"),
        };
        let reactions = std::mem::take(&mut record.reactions);
        record.status = status;
        self.promise_jobs
            .extend(reactions.into_iter().map(|reaction| match reaction {
                PromiseReaction::Then(reaction) => PromiseJob::Reaction {
                    target: reaction.target,
                    handler: if fulfilled {
                        reaction.on_fulfilled
                    } else {
                        reaction.on_rejected
                    },
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::ModuleAwait { continuation } => PromiseJob::ModuleAwait {
                    continuation,
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::AsyncAwait { continuation } => PromiseJob::AsyncAwait {
                    continuation,
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::AsyncGeneratorYield {
                    generator,
                    target,
                    result,
                } => PromiseJob::AsyncGeneratorYield {
                    generator,
                    target,
                    result,
                    value: value.clone(),
                    fulfilled,
                },
                PromiseReaction::AsyncGeneratorDelegate {
                    generator,
                    target,
                    kind,
                } => PromiseJob::AsyncGeneratorDelegate {
                    generator,
                    target,
                    kind,
                    value: value.clone(),
                    fulfilled,
                },
            }));
        Ok(())
    }

    /// Resolve a capability from a value returned by user code. Promise
    /// reactions adopt another BlueJS Promise instead of fulfilling with the
    /// Promise object itself, which is essential for `then`, async functions
    /// and top-level await.
    pub(super) fn resolve_promise(
        &mut self,
        promise: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        if value.object_id() == Some(promise) {
            let error = self.error_object("TypeError", "Promise resolved with itself".into())?;
            return self.settle_promise(promise, PromiseStatus::Rejected(error));
        }
        let resolution = self.promise_resolve(value)?;
        let Value::Object(resolved) = resolution else {
            unreachable!("Promise.resolve always returns a promise")
        };
        let status = match self.promises.get(&resolved) {
            Some(PromiseRecord {
                status: PromiseStatus::Fulfilled(value),
                ..
            }) => Some((true, value.clone())),
            Some(PromiseRecord {
                status: PromiseStatus::Rejected(value),
                ..
            }) => Some((false, value.clone())),
            Some(PromiseRecord {
                status: PromiseStatus::Pending,
                ..
            }) => None,
            None => unreachable!("Promise.resolve returns a registered promise"),
        };
        if let Some((fulfilled, value)) = status {
            return self.settle_promise(
                promise,
                if fulfilled {
                    PromiseStatus::Fulfilled(value)
                } else {
                    PromiseStatus::Rejected(value)
                },
            );
        }
        self.promises
            .get_mut(&resolved)
            .expect("checked pending promise exists")
            .reactions
            .push(PromiseReaction::Then(PromiseThenReaction {
                target: promise,
                on_fulfilled: Value::Undefined,
                on_rejected: Value::Undefined,
            }));
        Ok(())
    }

    fn promise_then(&mut self, receiver: &Value, args: &[Value]) -> Result<Value, RuntimeError> {
        let promise = receiver
            .object_id()
            .filter(|id| self.promises.contains_key(id))
            .ok_or(RuntimeError::TypeError(
                "Promise.prototype.then receiver".into(),
            ))?;
        let target = self.new_promise()?;
        let reaction = PromiseThenReaction {
            target,
            on_fulfilled: native::argument(args, 0).clone(),
            on_rejected: native::argument(args, 1).clone(),
        };
        let status = {
            let record = self.promises.get_mut(&promise).unwrap();
            match &record.status {
                PromiseStatus::Pending => {
                    record.reactions.push(PromiseReaction::Then(reaction));
                    return Ok(Value::Object(target));
                }
                PromiseStatus::Fulfilled(value) => (true, value.clone()),
                PromiseStatus::Rejected(value) => (false, value.clone()),
            }
        };
        self.promise_jobs.push_back(PromiseJob::Reaction {
            target,
            handler: if status.0 {
                reaction.on_fulfilled
            } else {
                reaction.on_rejected
            },
            value: status.1,
            fulfilled: status.0,
        });
        Ok(Value::Object(target))
    }

    fn promise_catch(
        &mut self,
        receiver: &Value,
        reason_handler: &Value,
    ) -> Result<Value, RuntimeError> {
        self.promise_then(receiver, &[Value::Undefined, reason_handler.clone()])
    }

    fn promise_finally(
        &mut self,
        receiver: &Value,
        handler: &Value,
    ) -> Result<Value, RuntimeError> {
        // A non-callable handler already passes the original completion
        // through in `promise_then`. Callable handlers run on both paths;
        // reaction adoption is handled by the shared job queue.
        self.promise_then(receiver, &[handler.clone(), handler.clone()])
    }

    /// Implements the settled-value portion of Await. A pending promise needs
    /// a saved interpreter continuation, which remains a separate boundary.
    pub(super) fn await_value(&self, value: Value) -> Result<Value, RuntimeError> {
        let Some(promise) = value.object_id() else {
            return Ok(value);
        };
        let Some(record) = self.promises.get(&promise) else {
            return Ok(value);
        };
        match &record.status {
            PromiseStatus::Pending => Err(RuntimeError::Unsupported("pending await continuation")),
            PromiseStatus::Fulfilled(value) => Ok(value.clone()),
            PromiseStatus::Rejected(value) => Err(RuntimeError::Thrown(value.clone())),
        }
    }

    pub(super) fn promise_resolve(&mut self, value: Value) -> Result<Value, RuntimeError> {
        if value
            .object_id()
            .is_some_and(|promise| self.promises.contains_key(&promise))
        {
            return Ok(value);
        }
        let promise = self.new_promise()?;
        let then = match &value {
            Value::Object(_) => self.get_property(&value, &"then".into()),
            _ => Ok(Value::Undefined),
        };
        match then {
            Ok(then) if self.is_callable(&then)? => {
                self.promise_jobs.push_back(PromiseJob::Thenable {
                    target: promise,
                    thenable: value,
                    then,
                });
            }
            Ok(_) => self.settle_promise(promise, PromiseStatus::Fulfilled(value))?,
            Err(error) => {
                let error = self.error_value(error)?;
                self.settle_promise(promise, PromiseStatus::Rejected(error))?;
            }
        }
        Ok(Value::Object(promise))
    }

    fn promise_reject(&mut self, value: Value) -> Result<Value, RuntimeError> {
        let promise = self.new_promise()?;
        self.settle_promise(promise, PromiseStatus::Rejected(value))?;
        Ok(Value::Object(promise))
    }

    fn promise_all_handler(
        &mut self,
        target: ObjectId,
        index: Option<u32>,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.function_prototype()?;
        let function = match index {
            Some(index) => NativeFunction::PromiseAllResolve { target, index },
            None => NativeFunction::PromiseAllReject { target },
        };
        let id = self.with_roots(|heap| heap.alloc_native_function(function, "", prototype))?;
        self.stack.push(Value::Object(id));
        let result = (|| {
            self.define_data(id, "name", Value::String("".into()), false, false, true)?;
            self.define_data(id, "length", Value::Number(1.0), false, false, true)?;
            Ok(Value::Object(id))
        })();
        self.stack.pop();
        result
    }

    fn promise_all_settled(
        &mut self,
        target: ObjectId,
        index: u32,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let complete = {
            let Some(state) = self.promise_all.get_mut(&target) else {
                return Ok(());
            };
            let slot = state
                .values
                .get_mut(index as usize)
                .expect("Promise.all reaction index was allocated");
            if slot.is_some() {
                return Ok(());
            }
            *slot = Some(value);
            state.remaining -= 1;
            (state.remaining == 0).then(|| {
                state
                    .values
                    .iter()
                    .cloned()
                    .map(|value| value.expect("completed Promise.all has every value"))
                    .collect::<Vec<_>>()
            })
        };
        let Some(values) = complete else {
            return Ok(());
        };
        self.promise_all.remove(&target);
        let base = self.stack.len();
        self.stack.extend(values.iter().cloned());
        let values = self.array_from(values);
        self.stack.truncate(base);
        let values = values?;
        self.settle_promise(target, PromiseStatus::Fulfilled(values))
    }

    fn promise_all_reject(&mut self, target: ObjectId, value: Value) -> Result<(), RuntimeError> {
        if self.promise_all.remove(&target).is_some() {
            self.settle_promise(target, PromiseStatus::Rejected(value))?;
        }
        Ok(())
    }

    fn promise_all(&mut self, values: &Value) -> Result<Value, RuntimeError> {
        let values = self.array_like_values(values)?;
        let promise = self.new_promise()?;
        if values.is_empty() {
            let values = self.array_from(Vec::new())?;
            self.settle_promise(promise, PromiseStatus::Fulfilled(values))?;
            return Ok(Value::Object(promise));
        }
        self.promise_all.insert(
            promise,
            PromiseAllState {
                values: vec![None; values.len()],
                remaining: values.len(),
            },
        );
        for (index, value) in values.into_iter().enumerate() {
            let input = self.promise_resolve(value)?;
            let Value::Object(input) = input else {
                unreachable!("Promise.resolve always returns a promise")
            };
            let fulfilled = self.promise_all_handler(promise, Some(index as u32))?;
            let rejected = self.promise_all_handler(promise, None)?;
            // Promise reactions require a target capability even though the
            // aggregate handlers ignore their own continuation. The dummy
            // Promise remains an ordinary resolved Promise after execution.
            let continuation = self.new_promise()?;
            let reaction = PromiseThenReaction {
                target: continuation,
                on_fulfilled: fulfilled,
                on_rejected: rejected,
            };
            let status = match &self
                .promises
                .get(&input)
                .expect("Promise.resolve registered its result")
                .status
            {
                PromiseStatus::Pending => None,
                PromiseStatus::Fulfilled(value) => Some((true, value.clone())),
                PromiseStatus::Rejected(value) => Some((false, value.clone())),
            };
            if let Some((fulfilled, value)) = status {
                self.promise_jobs.push_back(PromiseJob::Reaction {
                    target: continuation,
                    handler: if fulfilled {
                        reaction.on_fulfilled
                    } else {
                        reaction.on_rejected
                    },
                    value,
                    fulfilled,
                });
            } else {
                self.promises
                    .get_mut(&input)
                    .expect("checked pending promise exists")
                    .reactions
                    .push(PromiseReaction::Then(reaction));
            }
        }
        Ok(Value::Object(promise))
    }

    pub fn run_promise_jobs(&mut self) -> Result<(), RuntimeError> {
        while !self.promise_jobs.is_empty() {
            // Each queued Promise reaction has its own execution context and
            // therefore its own instruction budget. A module continuation
            // deliberately leaves the ambient interpreter with zero fuel
            // while suspended; it must not starve the next job turn.
            self.remaining_instructions = self.config.instruction_budget;
            self.run_next_promise_job()?;
        }
        Ok(())
    }

    /// Execute exactly one Promise job.  Async functions and top-level await
    /// resume from a queued continuation, rather than draining later turns
    /// in the same checkpoint, so callers that need an await boundary can
    /// advance the queue one observable turn at a time.
    pub(super) fn run_next_promise_job(&mut self) -> Result<bool, RuntimeError> {
        let Some(job) = self.promise_jobs.pop_front() else {
            return Ok(false);
        };
        match job {
            PromiseJob::Reaction {
                target,
                handler,
                value,
                fulfilled,
            } => {
                if !self.is_callable(&handler)? {
                    self.settle_promise(
                        target,
                        if fulfilled {
                            PromiseStatus::Fulfilled(value)
                        } else {
                            PromiseStatus::Rejected(value)
                        },
                    )?;
                    return Ok(true);
                }
                let result = self.call_native(handler, Value::Undefined, vec![value], false);
                match result {
                    Ok(value) => self.resolve_promise(target, value)?,
                    Err(error) => {
                        let error = self.error_value(error)?;
                        self.settle_promise(target, PromiseStatus::Rejected(error))?;
                    }
                }
            }
            PromiseJob::Thenable {
                target,
                thenable,
                then,
            } => {
                let result = (|| {
                    let resolve = self.promise_resolving_function(target, true)?;
                    let reject = self.promise_resolving_function(target, false)?;
                    self.call_native(then, thenable, vec![resolve, reject], false)
                })();
                if let Err(error) = result {
                    let error = self.error_value(error)?;
                    self.settle_promise(target, PromiseStatus::Rejected(error))?;
                }
            }
            PromiseJob::DynamicImport {
                target,
                referrer,
                specifier,
            } => {
                let result = self.dynamic_import_job(&referrer, &specifier);
                match result {
                    Ok(DynamicImportResult::Fulfilled(namespace)) => {
                        self.settle_promise(target, PromiseStatus::Fulfilled(namespace))?
                    }
                    Ok(DynamicImportResult::Waiting(module)) => {
                        self.module_import_waiters
                            .entry(module)
                            .or_default()
                            .push(target);
                    }
                    // Dynamic import delegates loading and linking to the
                    // host. A host module-resolution failure rejects the
                    // capability with its host error rather than leaking
                    // the static-module SyntaxError classification.
                    Err(RuntimeError::ModuleResolution(message)) => {
                        let error = self.error_object("TypeError", message)?;
                        self.settle_promise(target, PromiseStatus::Rejected(error))?;
                    }
                    Err(error) => {
                        let error = self.error_value(error)?;
                        self.settle_promise(target, PromiseStatus::Rejected(error))?;
                    }
                }
            }
            PromiseJob::ModuleAwait {
                continuation,
                value,
                fulfilled,
            } => self.resume_module_await(continuation, value, fulfilled)?,
            PromiseJob::AsyncAwait {
                continuation,
                value,
                fulfilled,
            } => self.resume_async_await(continuation, value, fulfilled)?,
            PromiseJob::AsyncGeneratorYield {
                generator,
                target,
                result,
                value,
                fulfilled,
            } => self.finish_async_generator_yield(generator, target, result, value, fulfilled)?,
            PromiseJob::AsyncGeneratorDelegate {
                generator,
                target,
                kind,
                value,
                fulfilled,
            } => self.finish_async_generator_delegate(generator, target, kind, value, fulfilled)?,
        }
        Ok(true)
    }

    pub fn install_test262_done(&mut self) -> Result<(), RuntimeError> {
        let global = self.global("globalThis")?.object_id().unwrap();
        let prototype = self.function_prototype()?;
        self.install_native(global, prototype, "$DONE", 1, NativeFunction::Test262Done)
    }

    pub fn take_test262_done(&mut self) -> Option<Result<(), Value>> {
        self.test262_done.take()
    }

    pub(super) fn is_callable(&self, value: &Value) -> Result<bool, RuntimeError> {
        Ok(if let Value::Object(id) = value {
            if let Some((callable, _)) = self.heap.proxy_capabilities(*id)? {
                callable
            } else {
                self.heap.native_function(*id)?.is_some()
                    || self.heap.closure(*id)?.is_some()
                    || self.heap.bound_function(*id)?.is_some()
            }
        } else {
            false
        })
    }

    pub(super) fn coerce_primitive(
        &mut self,
        value: &Value,
        hint: &str,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(_) = value else {
            return Ok(value.clone());
        };
        self.string_intrinsics()?;
        let method = self.get_method(value, &JsSymbol::well_known("toPrimitive").into())?;
        if !matches!(method, Value::Undefined) {
            let result = self.call_native(
                method,
                value.clone(),
                vec![Value::String(hint.into())],
                false,
            )?;
            return if matches!(result, Value::Object(_)) {
                Err(RuntimeError::TypeError(
                    "ToPrimitive returned an object".into(),
                ))
            } else {
                Ok(result)
            };
        }
        let names = if hint == "string" {
            ["toString", "valueOf"]
        } else {
            ["valueOf", "toString"]
        };
        for name in names {
            let method = self.get_property(value, &name.into())?;
            if self.is_callable(&method)? {
                let result = self.call_native(method, value.clone(), Vec::new(), false)?;
                if !matches!(result, Value::Object(_)) {
                    return Ok(result);
                }
            }
        }
        Err(RuntimeError::TypeError(
            "cannot convert object to primitive".into(),
        ))
    }

    pub(super) fn coerce_string(&mut self, value: &Value) -> Result<JsString, RuntimeError> {
        primitive::string(&self.coerce_primitive(value, "string")?)
    }

    pub(super) fn coerce_number(&mut self, value: &Value) -> Result<f64, RuntimeError> {
        primitive::number(&self.coerce_primitive(value, "number")?)
    }

    pub(super) fn coerce_numeric(
        &mut self,
        value: &Value,
    ) -> Result<primitive::Numeric, RuntimeError> {
        primitive::numeric(&self.coerce_primitive(value, "number")?)
    }

    pub(super) fn coerce_length(&mut self, value: &Value) -> Result<f64, RuntimeError> {
        native::length(&Value::Number(self.coerce_number(value)?))
    }

    /// ECMA-262 §19.2.5 parseInt.  The scan is deliberately prefix based:
    /// unlike Number(), trailing non-digits are ignored and an incomplete
    /// exponent is irrelevant because exponent syntax is not part of
    /// StringIntegerLiteral.
    fn parse_int(&mut self, value: &Value, radix: &Value) -> Result<Value, RuntimeError> {
        let string = self.coerce_string(value)?;
        let Ok(string) = string.to_utf8() else {
            return Ok(Value::Number(f64::NAN));
        };
        let mut input = string.trim_start_matches(primitive::whitespace);
        let negative = input.starts_with('-');
        if matches!(input.as_bytes().first(), Some(b'+' | b'-')) {
            input = &input[1..];
        }
        let requested = if matches!(radix, Value::Undefined) {
            0
        } else {
            let number = self.coerce_number(radix)?;
            primitive::to_uint32(number) as i32
        };
        if requested != 0 && !(2..=36).contains(&requested) {
            return Ok(Value::Number(f64::NAN));
        }
        let mut radix = requested;
        if (radix == 0 || radix == 16) && (input.starts_with("0x") || input.starts_with("0X")) {
            input = &input[2..];
            radix = 16;
        }
        if radix == 0 {
            radix = 10;
        }
        let mut digits = 0usize;
        let mut number = 0.0;
        for byte in input.bytes() {
            let digit = match byte {
                b'0'..=b'9' => u32::from(byte - b'0'),
                b'a'..=b'z' => u32::from(byte - b'a') + 10,
                b'A'..=b'Z' => u32::from(byte - b'A') + 10,
                _ => break,
            };
            if digit >= radix as u32 {
                break;
            }
            digits += 1;
            number = number * f64::from(radix) + f64::from(digit);
        }
        if digits == 0 {
            Ok(Value::Number(f64::NAN))
        } else {
            Ok(Value::Number(if negative { -number } else { number }))
        }
    }

    /// ECMA-262 §19.2.4 parseFloat.  It recognizes only the longest valid
    /// decimal/Infinity prefix after StringTrim; hexadecimal and binary text
    /// therefore stop after their leading decimal zero.
    fn parse_float(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let string = self.coerce_string(value)?;
        let Ok(input) = string.to_utf8() else {
            return Ok(Value::Number(f64::NAN));
        };
        let input = input.trim_start_matches(primitive::whitespace);
        let sign_end = usize::from(matches!(input.as_bytes().first(), Some(b'+' | b'-')));
        let negative = input.starts_with('-');
        if input[sign_end..].starts_with("Infinity") {
            return Ok(Value::Number(if negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }));
        }
        let bytes = input.as_bytes();
        let mut index = sign_end;
        let mut digits = 0usize;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
            digits += 1;
        }
        if bytes.get(index) == Some(&b'.') {
            index += 1;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return Ok(Value::Number(f64::NAN));
        }
        if matches!(bytes.get(index), Some(b'e' | b'E')) {
            let exponent = index;
            index += 1;
            if matches!(bytes.get(index), Some(b'+' | b'-')) {
                index += 1;
            }
            let exponent_digits = index;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
            if index == exponent_digits {
                index = exponent;
            }
        }
        Ok(Value::Number(input[..index].parse().unwrap_or(f64::NAN)))
    }

    pub(super) fn array_length_value(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        // ArraySetLength performs ToUint32 followed by a separate ToNumber.
        let length = native::uint32(&Value::Number(self.coerce_number(value)?))?;
        if f64::from(length) != self.coerce_number(value)? {
            return Err(RuntimeError::RangeError("invalid array length".into()));
        }
        Ok(Value::Number(f64::from(length)))
    }

    pub(super) fn coerce_property_key(
        &mut self,
        value: &Value,
    ) -> Result<PropertyName, RuntimeError> {
        let value = self.coerce_primitive(value, "string")?;
        Ok(match value {
            Value::Symbol(symbol) => symbol.into(),
            value => primitive::string(&value)?.into(),
        })
    }

    pub(super) fn string_constructor_argument(
        &mut self,
        value: &Value,
        construct: bool,
    ) -> Result<JsString, RuntimeError> {
        if !construct {
            if let Value::Symbol(symbol) = value {
                return Ok(symbol.descriptive_string());
            }
        }
        self.coerce_string(value)
    }

    pub(super) fn get_method(
        &mut self,
        value: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        let method = self.get_property(value, key)?;
        if matches!(method, Value::Undefined | Value::Null) {
            return Ok(Value::Undefined);
        }
        if !self.is_callable(&method)? {
            return Err(RuntimeError::TypeError("property is not callable".into()));
        }
        Ok(method)
    }

    pub(super) fn is_regexp(&mut self, value: &Value) -> Result<bool, RuntimeError> {
        if !matches!(value, Value::Object(_)) {
            return Ok(false);
        }
        let matcher = self.get_property(value, &JsSymbol::well_known("match").into())?;
        if !matches!(matcher, Value::Undefined) {
            return self.to_boolean(&matcher);
        }
        Ok(if let Value::Object(id) = value {
            self.heap.regexp(*id)?.is_some()
        } else {
            false
        })
    }

    pub(super) fn dispatch_string_method(
        &mut self,
        method: StringMethod,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        use StringMethod::*;
        if matches!(method, ToString | ValueOf) {
            return native::string_method(
                method,
                &self.unbox_string(receiver)?,
                &[],
                self.config.max_string_bytes,
            );
        }
        let string = self.string_receiver(receiver)?;
        let mut converted = Vec::new();
        let first = native::argument(args, 0);
        let second = native::argument(args, 1);
        match method {
            At | CharAt | CharCodeAt | CodePointAt | Repeat => {
                converted.push(Value::Number(self.coerce_number(first)?));
            }
            Slice | Substring | Substr => {
                converted.push(Value::Number(self.coerce_number(first)?));
                converted.push(if matches!(second, Value::Undefined) {
                    Value::Undefined
                } else {
                    Value::Number(self.coerce_number(second)?)
                });
            }
            IndexOf | LastIndexOf | Includes | StartsWith | EndsWith => {
                if matches!(method, Includes | StartsWith | EndsWith) && self.is_regexp(first)? {
                    return Err(RuntimeError::TypeError(
                        "String search argument must not be a RegExp".into(),
                    ));
                }
                converted.push(Value::String(self.coerce_string(first)?));
                converted.push(if matches!(second, Value::Undefined) {
                    Value::Undefined
                } else {
                    Value::Number(self.coerce_number(second)?)
                });
            }
            Concat => {
                for value in args {
                    converted.push(Value::String(self.coerce_string(value)?));
                }
            }
            PadStart | PadEnd => {
                let target = self.coerce_length(first)?;
                if target <= string.len() as f64 {
                    return Ok(Value::String(string));
                }
                converted.push(Value::Number(target));
                converted.push(if matches!(second, Value::Undefined) {
                    Value::Undefined
                } else {
                    Value::String(self.coerce_string(second)?)
                });
            }
            Normalize if !matches!(first, Value::Undefined) => {
                converted.push(Value::String(self.coerce_string(first)?));
            }
            Html { attribute, .. } if !attribute.is_empty() => {
                converted.push(Value::String(self.coerce_string(first)?));
            }
            _ => {}
        }
        native::string_method(
            method,
            &Value::String(string),
            &converted,
            self.config.max_string_bytes,
        )
    }

    pub(super) fn define_data(
        &mut self,
        owner: ObjectId,
        key: impl Into<PropertyName>,
        value: Value,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Result<(), RuntimeError> {
        let key = key.into();
        let result = self.with_roots(|heap| {
            heap.define_own_property(
                owner,
                key,
                PropertyDescriptor::data(value, writable, enumerable, configurable),
            )
        })?;
        result
            .then_some(())
            .ok_or_else(|| RuntimeError::TypeError("cannot define property".into()))
    }

    pub(super) fn array_from(&mut self, values: Vec<Value>) -> Result<Value, RuntimeError> {
        let prototype = self.array_prototype;
        let base = self.stack.len();
        self.stack.extend(values.iter().cloned());
        let result = (|| {
            let array =
                self.with_roots(|heap| heap.alloc_array(values.len() as u32, Some(prototype)))?;
            self.stack.push(Value::Object(array));
            for (index, value) in values.into_iter().enumerate() {
                self.with_roots(|heap| heap.set(array, index.to_string(), value))?;
            }
            Ok(Value::Object(array))
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn iterator_result(
        &mut self,
        value: Value,
        done: bool,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.object_prototype;
        self.stack.push(value.clone());
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        self.with_roots(|heap| heap.set(result, "value", value))?;
        self.with_roots(|heap| heap.set(result, "done", Value::Bool(done)))?;
        self.stack.pop();
        self.stack.pop();
        Ok(Value::Object(result))
    }

    pub(super) fn global(&mut self, name: &str) -> Result<Value, RuntimeError> {
        self.string_intrinsics()?;
        if name == "String" {
            let id = self
                .string_intrinsics
                .expect("String intrinsics initialized")
                .0;
            if self.globals.insert(name.into(), id).is_none() {
                if let Some(&global) = self.globals.get("globalThis") {
                    self.define_data(global, name, Value::Object(id), true, false, true)?;
                }
            }
            return Ok(Value::Object(id));
        }
        if matches!(
            name,
            "Error"
                | "TypeError"
                | "RangeError"
                | "SyntaxError"
                | "ReferenceError"
                | "EvalError"
                | "URIError"
        ) {
            return self.error_global(name);
        }
        if name == "Intl" {
            return self.intl_global();
        }
        if name == "RegExp" {
            return self.regexp_global();
        }
        if name == "Math" {
            return self.math_global();
        }
        if name == "JSON" {
            return self.json_global();
        }
        if let Some(&id) = self.globals.get(name) {
            return Ok(Value::Object(id));
        }
        let constructor = self.string_intrinsics.unwrap().0;
        let prototype = self.heap.prototype(constructor)?.unwrap();
        let native = match name {
            "Function" => NativeFunction::Function,
            "Symbol" => NativeFunction::Symbol,
            "Array" => NativeFunction::Array,
            "ArrayBuffer" => NativeFunction::ArrayBuffer,
            "DataView" => NativeFunction::DataView,
            "Int8Array" => NativeFunction::TypedArray(TypedArrayKind::Int8),
            "Uint8Array" => NativeFunction::TypedArray(TypedArrayKind::Uint8),
            "Uint8ClampedArray" => NativeFunction::TypedArray(TypedArrayKind::Uint8Clamped),
            "Int16Array" => NativeFunction::TypedArray(TypedArrayKind::Int16),
            "Uint16Array" => NativeFunction::TypedArray(TypedArrayKind::Uint16),
            "Int32Array" => NativeFunction::TypedArray(TypedArrayKind::Int32),
            "Uint32Array" => NativeFunction::TypedArray(TypedArrayKind::Uint32),
            "Float32Array" => NativeFunction::TypedArray(TypedArrayKind::Float32),
            "Float64Array" => NativeFunction::TypedArray(TypedArrayKind::Float64),
            "Proxy" => NativeFunction::Proxy,
            "Map" => NativeFunction::Map,
            "Set" => NativeFunction::Set,
            "Promise" => NativeFunction::Promise,
            "eval" => NativeFunction::Eval,
            "Object" => NativeFunction::Object,
            "Number" => NativeFunction::PrimitiveConstructor(false),
            "Boolean" => NativeFunction::PrimitiveConstructor(true),
            "BigInt" => NativeFunction::BigInt,
            "isNaN" => NativeFunction::IsNaN,
            "isFinite" => NativeFunction::IsFinite,
            "parseInt" => NativeFunction::ParseInt,
            "parseFloat" => NativeFunction::ParseFloat,
            // The remaining compiler-recognized globals are namespace objects.
            _ => NativeFunction::Empty,
        };
        let object_prototype = self.object_prototype;
        let native_prototype = if matches!(native, NativeFunction::TypedArray(_)) {
            self.typed_array_intrinsics()?.0
        } else {
            prototype
        };
        let id = if matches!(name, "Reflect" | "globalThis") {
            self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?
        } else {
            self.with_roots(|heap| heap.alloc_native_function(native, name, native_prototype))?
        };
        let root = self.heap.root(id)?;
        let result = (|| {
            if !matches!(name, "Reflect" | "globalThis") {
                self.define_data(id, "name", Value::String(name.into()), false, false, true)?;
                self.define_data(
                    id,
                    "length",
                    Value::Number(if name == "Symbol" { 0.0 } else { 1.0 }),
                    false,
                    false,
                    true,
                )?;
            }
            if name == "Symbol" {
                let symbol_prototype =
                    self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(symbol_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    symbol_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                self.install_native(
                    symbol_prototype,
                    prototype,
                    "toString",
                    0,
                    NativeFunction::SymbolToString,
                )?;
                self.install_native(
                    symbol_prototype,
                    prototype,
                    "valueOf",
                    0,
                    NativeFunction::SymbolValueOf,
                )?;
                self.install_symbol_native(
                    symbol_prototype,
                    prototype,
                    "toPrimitive",
                    1,
                    NativeFunction::SymbolValueOf,
                )?;
                let to_primitive = self
                    .heap
                    .get(symbol_prototype, JsSymbol::well_known("toPrimitive"))?;
                self.define_data(
                    symbol_prototype,
                    JsSymbol::well_known("toPrimitive"),
                    to_primitive,
                    false,
                    false,
                    true,
                )?;
                self.define_data(
                    symbol_prototype,
                    JsSymbol::well_known("toStringTag"),
                    Value::String("Symbol".into()),
                    false,
                    false,
                    true,
                )?;
                for &name in crate::property::WELL_KNOWN {
                    self.define_data(
                        id,
                        name,
                        Value::Symbol(JsSymbol::well_known(name)),
                        false,
                        false,
                        false,
                    )?;
                }
            } else if name == "Promise" {
                let promise_prototype = self.promise_prototype()?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(promise_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    promise_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                self.install_native(id, prototype, "resolve", 1, NativeFunction::PromiseResolve)?;
                self.install_native(id, prototype, "reject", 1, NativeFunction::PromiseReject)?;
                self.install_native(id, prototype, "all", 1, NativeFunction::PromiseAll)?;
                self.install_native(
                    id,
                    prototype,
                    "withResolvers",
                    0,
                    NativeFunction::PromiseWithResolvers,
                )?;
            } else if name == "Array" {
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(self.array_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    self.array_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                self.install_native(id, prototype, "isArray", 1, NativeFunction::ArrayIsArray)?;
                self.install_native(id, prototype, "from", 1, NativeFunction::ArrayFrom)?;
            } else if name == "ArrayBuffer" {
                let buffer_prototype =
                    self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(buffer_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    buffer_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                self.define_data(
                    buffer_prototype,
                    JsSymbol::well_known("toStringTag"),
                    Value::String("ArrayBuffer".into()),
                    false,
                    false,
                    true,
                )?;
                self.install_native_getter(
                    buffer_prototype,
                    prototype,
                    "byteLength",
                    NativeFunction::ArrayBufferByteLength,
                )?;
                self.install_native(
                    buffer_prototype,
                    prototype,
                    "slice",
                    2,
                    NativeFunction::ArrayBufferSlice,
                )?;
                self.install_native(
                    id,
                    prototype,
                    "isView",
                    1,
                    NativeFunction::ArrayBufferIsView,
                )?;
                self.install_getter(
                    id,
                    prototype,
                    JsSymbol::well_known("species").into(),
                    "get [Symbol.species]",
                    NativeFunction::ArrayBufferSpecies,
                )?;
            } else if name == "DataView" {
                let view_prototype =
                    self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(view_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    view_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                self.define_data(
                    view_prototype,
                    JsSymbol::well_known("toStringTag"),
                    Value::String("DataView".into()),
                    false,
                    false,
                    true,
                )?;
                for (name, native) in [
                    ("buffer", NativeFunction::DataViewBuffer),
                    ("byteLength", NativeFunction::DataViewByteLength),
                    ("byteOffset", NativeFunction::DataViewByteOffset),
                ] {
                    self.install_native_getter(view_prototype, prototype, name, native)?;
                }
                for (name, width, signed, floating) in [
                    ("getUint8", 1, false, false),
                    ("getInt8", 1, true, false),
                    ("getUint16", 2, false, false),
                    ("getInt16", 2, true, false),
                    ("getUint32", 4, false, false),
                    ("getInt32", 4, true, false),
                    ("getFloat32", 4, false, true),
                    ("getFloat64", 8, false, true),
                ] {
                    self.install_native(
                        view_prototype,
                        prototype,
                        name,
                        1,
                        NativeFunction::DataViewGet {
                            width,
                            signed,
                            floating,
                        },
                    )?;
                    let set_name = name.replacen("get", "set", 1);
                    self.install_native(
                        view_prototype,
                        prototype,
                        &set_name,
                        2,
                        NativeFunction::DataViewSet {
                            width,
                            signed,
                            floating,
                        },
                    )?;
                }
            } else if let Some(kind) = typed_array_kind(name) {
                let typed_array_prototype = self.typed_array_intrinsics()?.1;
                let typed_prototype =
                    self.with_roots(|heap| heap.alloc_object(Some(typed_array_prototype)))?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(typed_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    typed_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                self.define_data(
                    typed_prototype,
                    JsSymbol::well_known("toStringTag"),
                    Value::String(kind.name().into()),
                    false,
                    false,
                    true,
                )?;
                self.define_data(
                    id,
                    "BYTES_PER_ELEMENT",
                    Value::Number(kind.byte_width() as f64),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    typed_prototype,
                    "BYTES_PER_ELEMENT",
                    Value::Number(kind.byte_width() as f64),
                    false,
                    false,
                    false,
                )?;
            } else if name == "Proxy" {
                self.install_native(
                    id,
                    prototype,
                    "revocable",
                    2,
                    NativeFunction::ProxyRevocable,
                )?;
            } else if matches!(name, "Map" | "Set") {
                let collection_prototype = self.collection_prototype(name == "Map")?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(collection_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    collection_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
            } else if name == "Function" {
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(prototype),
                    false,
                    false,
                    false,
                )?;
            } else if matches!(name, "Number" | "Boolean" | "BigInt") {
                let boolean = name == "Boolean";
                let bigint = name == "BigInt";
                let value = if boolean {
                    Value::Bool(false)
                } else if bigint {
                    Value::BigInt(0.into())
                } else {
                    Value::Number(0.0)
                };
                let boxed_prototype =
                    self.with_roots(|heap| heap.alloc_boxed_primitive(value, object_prototype))?;
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(boxed_prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    boxed_prototype,
                    "constructor",
                    Value::Object(id),
                    true,
                    false,
                    true,
                )?;
                if bigint {
                    self.install_native(
                        boxed_prototype,
                        prototype,
                        "toString",
                        0,
                        NativeFunction::BigIntToString,
                    )?;
                    self.install_native(
                        boxed_prototype,
                        prototype,
                        "valueOf",
                        0,
                        NativeFunction::BigIntValueOf,
                    )?;
                } else {
                    self.install_native(
                        boxed_prototype,
                        prototype,
                        "toString",
                        0,
                        NativeFunction::PrimitiveMethod {
                            boolean,
                            string: true,
                        },
                    )?;
                    self.install_native(
                        boxed_prototype,
                        prototype,
                        "valueOf",
                        0,
                        NativeFunction::PrimitiveMethod {
                            boolean,
                            string: false,
                        },
                    )?;
                }
                if name == "Number" {
                    for (property, value) in [
                        ("EPSILON", f64::EPSILON),
                        ("MAX_SAFE_INTEGER", 9_007_199_254_740_991.0),
                        ("MAX_VALUE", f64::MAX),
                        ("MIN_SAFE_INTEGER", -9_007_199_254_740_991.0),
                        ("MIN_VALUE", f64::from_bits(1)),
                        ("NaN", f64::NAN),
                        ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
                        ("POSITIVE_INFINITY", f64::INFINITY),
                    ] {
                        self.define_data(id, property, Value::Number(value), false, false, false)?;
                    }
                }
            } else if name == "globalThis" {
                self.define_data(id, "String", Value::Object(constructor), true, false, true)?;
                self.define_data(id, "globalThis", Value::Object(id), true, false, true)?;
            } else if name == "Reflect" {
                self.install_native(id, prototype, "apply", 3, NativeFunction::ReflectApply)?;
                self.install_native(
                    id,
                    prototype,
                    "ownKeys",
                    1,
                    NativeFunction::ObjectMethod(ObjectMethod::OwnKeys),
                )?;
                self.install_native(
                    id,
                    prototype,
                    "construct",
                    2,
                    NativeFunction::ReflectConstruct,
                )?;
                for (name, length, method) in [
                    ("get", 2, ObjectMethod::ReflectGet),
                    (
                        "getOwnPropertyDescriptor",
                        2,
                        ObjectMethod::ReflectGetOwnPropertyDescriptor,
                    ),
                    ("getPrototypeOf", 1, ObjectMethod::ReflectGetPrototypeOf),
                    ("defineProperty", 3, ObjectMethod::ReflectDefineProperty),
                    ("set", 3, ObjectMethod::ReflectSet),
                    ("deleteProperty", 2, ObjectMethod::ReflectDeleteProperty),
                    (
                        "preventExtensions",
                        1,
                        ObjectMethod::ReflectPreventExtensions,
                    ),
                    ("setPrototypeOf", 2, ObjectMethod::ReflectSetPrototypeOf),
                    ("isExtensible", 1, ObjectMethod::ReflectIsExtensible),
                    ("has", 2, ObjectMethod::ReflectHas),
                ] {
                    self.install_native(
                        id,
                        prototype,
                        name,
                        length,
                        NativeFunction::ObjectMethod(method),
                    )?;
                }
                self.define_data(
                    id,
                    JsSymbol::well_known("toStringTag"),
                    Value::String("Reflect".into()),
                    false,
                    false,
                    true,
                )?;
            } else {
                self.define_data(
                    id,
                    "prototype",
                    Value::Object(self.object_prototype),
                    false,
                    false,
                    false,
                )?;
                if name == "Object" {
                    self.define_data(
                        self.object_prototype,
                        "constructor",
                        Value::Object(id),
                        true,
                        false,
                        true,
                    )?;
                }
                use ObjectMethod::*;
                for (name, length, method) in [
                    ("getOwnPropertyDescriptor", 2, GetOwnPropertyDescriptor),
                    ("defineProperty", 3, DefineProperty),
                    ("defineProperties", 2, DefineProperties),
                    ("keys", 1, Keys),
                    ("getOwnPropertyNames", 1, GetOwnPropertyNames),
                    ("getOwnPropertySymbols", 1, GetOwnPropertySymbols),
                    ("getPrototypeOf", 1, GetPrototypeOf),
                    ("setPrototypeOf", 2, SetPrototypeOf),
                    ("create", 2, Create),
                    ("isExtensible", 1, IsExtensible),
                    ("preventExtensions", 1, PreventExtensions),
                    ("seal", 1, Seal),
                    ("freeze", 1, Freeze),
                    ("isSealed", 1, IsSealed),
                    ("isFrozen", 1, IsFrozen),
                ] {
                    self.install_native(
                        id,
                        prototype,
                        name,
                        length,
                        NativeFunction::ObjectMethod(method),
                    )?;
                }
            }
            Ok(Value::Object(id))
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.globals.insert(name.into(), id);
            if name == "globalThis" {
                let globals = self.globals.clone();
                for (global_name, value) in globals {
                    if global_name != "globalThis" {
                        self.define_data(id, global_name, Value::Object(value), true, false, true)?;
                    }
                }
            } else if let Some(&global) = self.globals.get("globalThis") {
                self.define_data(global, name, Value::Object(id), true, false, true)?;
            }
        }
        result
    }

    pub(super) fn native_call(
        &mut self,
        function: NativeFunction,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let first = native::argument(&args, 0);
        match function {
            NativeFunction::Promise => self.promise_constructor(first.clone(), construct),
            NativeFunction::PromiseResolvingFunction { promise, fulfill } => {
                if fulfill {
                    self.resolve_promise(promise, first.clone())?;
                } else {
                    self.settle_promise(promise, PromiseStatus::Rejected(first.clone()))?;
                }
                Ok(Value::Undefined)
            }
            NativeFunction::AbstractModuleSource => Err(RuntimeError::TypeError(
                "AbstractModuleSource is an abstract constructor".into(),
            )),
            NativeFunction::AbstractModuleSourceToStringTag => Ok(Value::Undefined),
            NativeFunction::Function => self.function_constructor(&args),
            NativeFunction::AsyncFunction => self.async_function_constructor(&args),
            NativeFunction::Error(name) => self.error_constructor(name, &args, construct),
            NativeFunction::ErrorToString => self.error_to_string(&receiver),
            NativeFunction::Test262(name) => self.test262_call(name, &args),
            NativeFunction::Test262RealmEval(realm) => self.test262_realm_eval(realm, &args),
            NativeFunction::Test262Done => {
                self.test262_done = Some(if matches!(first, Value::Undefined) {
                    Ok(())
                } else {
                    Err(first.clone())
                });
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseThen => self.promise_then(&receiver, &args),
            NativeFunction::PromiseCatch => self.promise_catch(&receiver, first),
            NativeFunction::PromiseFinally => self.promise_finally(&receiver, first),
            NativeFunction::PromiseResolve => self.promise_resolve(first.clone()),
            NativeFunction::PromiseReject => self.promise_reject(first.clone()),
            NativeFunction::PromiseAll => self.promise_all(first),
            NativeFunction::PromiseAllResolve { target, index } => {
                self.promise_all_settled(target, index, first.clone())?;
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseAllReject { target } => {
                self.promise_all_reject(target, first.clone())?;
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseWithResolvers => self.promise_with_resolvers(),
            NativeFunction::ToLocaleLowerCase
            | NativeFunction::ToLocaleUpperCase
            | NativeFunction::LocaleCompare => {
                let string = self.string_receiver(&receiver)?;
                if function == NativeFunction::LocaleCompare {
                    let other = self.coerce_string(first)?;
                    let collator = self
                        .resolve_collator(native::argument(&args, 1), native::argument(&args, 2))?;
                    Ok(collator.compare(&string, &other))
                } else {
                    let locales = self.canonical_locales(first)?;
                    let locale = locales
                        .first()
                        .cloned()
                        .unwrap_or(icu_locale_core::locale!("en-US"));
                    crate::intl::case_map(
                        &string,
                        &locale,
                        function == NativeFunction::ToLocaleUpperCase,
                        self.config.max_string_bytes,
                    )
                    .map(Value::String)
                }
            }
            NativeFunction::Collator => self.create_collator(&args, construct),
            NativeFunction::Locale => self.create_locale(&args, construct),
            NativeFunction::CanonicalLocales => {
                let locales = self.canonical_locales(first)?;
                self.array_from(
                    locales
                        .into_iter()
                        .map(|l| Value::String(l.to_string().into()))
                        .collect(),
                )
            }
            NativeFunction::SupportedLocales => self.supported_locales(&args),
            NativeFunction::CollatorCompareGetter => self.collator_compare_getter(&receiver),
            NativeFunction::CollatorCompare => {
                let collator = self.collator_data(&receiver)?;
                let left = self.coerce_string(first)?;
                let right = self.coerce_string(native::argument(&args, 1))?;
                Ok(collator.compare(&left, &right))
            }
            NativeFunction::CollatorResolvedOptions => self.collator_resolved_options(&receiver),
            NativeFunction::LocaleToString => self.locale_to_string(&receiver),
            NativeFunction::LocaleMaximize => self.locale_transform(&receiver, true),
            NativeFunction::LocaleMinimize => self.locale_transform(&receiver, false),
            NativeFunction::LocaleGetter(name) => self.locale_getter(&receiver, name),
            NativeFunction::LocaleInfo(name) => self.locale_info(&receiver, name),
            NativeFunction::Array => {
                if args.len() == 1 {
                    if let Value::Number(length) = first {
                        let Value::Number(length) =
                            self.array_length_value(&Value::Number(*length))?
                        else {
                            unreachable!()
                        };
                        let prototype = self.array_prototype;
                        return Ok(Value::Object(self.with_roots(|heap| {
                            heap.alloc_array(length as u32, Some(prototype))
                        })?));
                    }
                }
                self.array_from(args)
            }
            NativeFunction::ArrayBuffer => self.array_buffer_constructor(&args, construct),
            NativeFunction::ArrayBufferByteLength => Ok(Value::Number(
                self.heap
                    .array_buffer_byte_length(self.array_buffer_receiver(&receiver)?)?
                    as f64,
            )),
            NativeFunction::ArrayBufferSlice => self.array_buffer_slice(&receiver, &args),
            NativeFunction::ArrayBufferIsView => {
                Ok(Value::Bool(first.object_id().is_some_and(|object| {
                    self.heap.is_data_view(object).unwrap_or(false)
                        || self.heap.is_typed_array(object).unwrap_or(false)
                })))
            }
            NativeFunction::ArrayBufferSpecies => Ok(receiver),
            NativeFunction::DataView => self.data_view_constructor(&args, construct),
            NativeFunction::DataViewBuffer => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError("DataView method requires a DataView receiver".into())
                })?;
                let buffer = self
                    .heap
                    .data_view_buffer(object)
                    .map_err(|error| match error {
                        HeapError::InvalidInternalSlot(_) | HeapError::InvalidObject(_) => {
                            RuntimeError::TypeError(
                                "DataView method requires a DataView receiver".into(),
                            )
                        }
                        error => error.into(),
                    })?;
                Ok(Value::Object(buffer))
            }
            NativeFunction::DataViewByteLength => {
                let (_, _, length) = self.data_view_receiver(&receiver)?;
                Ok(Value::Number(length as f64))
            }
            NativeFunction::DataViewByteOffset => {
                let (_, offset, _) = self.data_view_receiver(&receiver)?;
                Ok(Value::Number(offset as f64))
            }
            NativeFunction::DataViewGet {
                width,
                signed,
                floating,
            } => self.data_view_get(&receiver, &args, width, signed, floating),
            NativeFunction::DataViewSet {
                width,
                signed,
                floating,
            } => self.data_view_set(&receiver, &args, width, signed, floating),
            NativeFunction::TypedArray(kind) => {
                self.typed_array_constructor(&args, construct, kind)
            }
            NativeFunction::TypedArrayIntrinsic => Err(RuntimeError::TypeError(
                "%TypedArray% is not directly constructible".into(),
            )),
            NativeFunction::TypedArrayBuffer => {
                let (buffer, _, _, _) = self.typed_array_receiver(&receiver)?;
                Ok(Value::Object(buffer))
            }
            NativeFunction::TypedArrayByteLength => {
                let (buffer, _, length, kind) = self.typed_array_receiver(&receiver)?;
                let byte_length = if self.heap.array_buffer_is_detached(buffer)? {
                    0
                } else {
                    length * kind.byte_width()
                };
                Ok(Value::Number(byte_length as f64))
            }
            NativeFunction::TypedArrayByteOffset => {
                let (buffer, offset, _, _) = self.typed_array_receiver(&receiver)?;
                let byte_offset = if self.heap.array_buffer_is_detached(buffer)? {
                    0
                } else {
                    offset
                };
                Ok(Value::Number(byte_offset as f64))
            }
            NativeFunction::TypedArrayLength => {
                let (buffer, _, length, _) = self.typed_array_receiver(&receiver)?;
                let element_length = if self.heap.array_buffer_is_detached(buffer)? {
                    0
                } else {
                    length
                };
                Ok(Value::Number(element_length as f64))
            }
            NativeFunction::TypedArraySet => self.typed_array_set(&receiver, &args),
            NativeFunction::TypedArraySubarray => self.typed_array_subarray(&receiver, &args),
            NativeFunction::Proxy => self.proxy_constructor(&args, construct),
            NativeFunction::ProxyRevocable => self.proxy_revocable(&args),
            NativeFunction::ProxyRevoker(proxy) => {
                self.with_roots(|heap| heap.revoke_proxy(proxy))?;
                Ok(Value::Undefined)
            }
            NativeFunction::Map => self.collection_constructor(true, construct),
            NativeFunction::Set => self.collection_constructor(false, construct),
            NativeFunction::ArrayIsArray => Ok(Value::Bool(
                first
                    .object_id()
                    .is_some_and(|id| self.heap.is_array(id).unwrap_or(false)),
            )),
            NativeFunction::ArrayFrom => self.array_from_method(&args),
            NativeFunction::ArrayForEach => {
                self.array_for_each(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayIncludes => {
                self.array_includes(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayReduce => self.array_reduce(&receiver, &args),
            NativeFunction::ArrayPush => {
                let object = self.coerce_object(&receiver)?;
                let array = Value::Object(object);
                self.stack.push(array.clone());
                let result = (|| {
                    for value in &args {
                        self.array_push(&array, value, 0)?;
                    }
                    self.heap.get(object, "length").map_err(Into::into)
                })();
                self.stack.pop();
                result
            }
            NativeFunction::ArrayIndexOf => {
                self.array_index_of(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::Eval => self.indirect_eval(first),
            NativeFunction::IsNaN => Ok(Value::Bool(self.coerce_number(first)?.is_nan())),
            NativeFunction::IsFinite => Ok(Value::Bool(self.coerce_number(first)?.is_finite())),
            NativeFunction::ParseInt => self.parse_int(first, native::argument(&args, 1)),
            NativeFunction::ParseFloat => self.parse_float(first),
            NativeFunction::JsonParse => self.json_parse(first),
            NativeFunction::JsonStringify => self.json_stringify(first),
            NativeFunction::Math(method) => self.math_method(method, &args),
            NativeFunction::Bind => self.bind_function(receiver, &args),
            NativeFunction::HasInstance => self
                .has_instance(first.clone(), receiver, true)
                .map(Value::Bool),
            NativeFunction::RegExpEscape => self.regexp_escape(first),
            NativeFunction::ArrayIterator => {
                let object = self.coerce_object(&receiver)?;
                let prototype = self.array_iterator_prototype()?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_array_iterator(object, prototype)
                })?))
            }
            NativeFunction::ArrayIteratorNext => {
                let Value::Object(id) = receiver else {
                    return Err(RuntimeError::TypeError(
                        "Array iterator next requires an iterator".into(),
                    ));
                };
                let Some((object, index, done)) = self.heap.array_iterator(id)? else {
                    return Err(RuntimeError::TypeError(
                        "Array iterator next requires an iterator".into(),
                    ));
                };
                if done {
                    return self.iterator_result(Value::Undefined, true);
                }
                let length = self.get_property(&Value::Object(object), &"length".into())?;
                let length = self.coerce_length(&length)?;
                let done = index as f64 >= length;
                self.heap.advance_array_iterator(id, done);
                let value = if done {
                    Value::Undefined
                } else {
                    self.get_property(&Value::Object(object), &index.to_string().into())?
                };
                self.iterator_result(value, done)
            }
            NativeFunction::GeneratorNext => {
                self.generator_next(&receiver, Some(first.clone()), None)
            }
            NativeFunction::GeneratorReturn => self.generator_return(&receiver, first.clone()),
            NativeFunction::AsyncGeneratorNext
            | NativeFunction::AsyncGeneratorReturn
            | NativeFunction::AsyncGeneratorThrow => {
                self.async_generator_request(&receiver, first.clone(), function)
            }
            NativeFunction::Apply => {
                if !self.is_callable(&receiver)? {
                    return Err(RuntimeError::TypeError("apply requires a callable".into()));
                }
                let list = native::argument(&args, 1);
                let values = if matches!(list, Value::Null | Value::Undefined) {
                    Vec::new()
                } else {
                    self.array_like_values(list)?
                };
                self.call_native(receiver, first.clone(), values, false)
            }
            NativeFunction::ReflectApply => {
                if !self.is_callable(first)? {
                    return Err(RuntimeError::TypeError(
                        "Reflect.apply requires a callable target".into(),
                    ));
                }
                let values = self.array_like_values(native::argument(&args, 2))?;
                self.call_native(
                    first.clone(),
                    native::argument(&args, 1).clone(),
                    values,
                    false,
                )
            }
            NativeFunction::ReflectConstruct => {
                let new_target = if args.len() > 2 {
                    args[2].clone()
                } else {
                    first.clone()
                };
                if !self.is_constructor(first)? || !self.is_constructor(&new_target)? {
                    return Err(RuntimeError::TypeError(
                        "Reflect.construct requires constructors".into(),
                    ));
                }
                let values = self.array_like_values(native::argument(&args, 1))?;
                self.call_with_target(first.clone(), Value::Undefined, values, true, new_target)
            }
            NativeFunction::FunctionToString => {
                if !self.is_callable(&receiver)? {
                    return Err(RuntimeError::TypeError(
                        "Function.toString requires a callable".into(),
                    ));
                }
                let name = self
                    .heap
                    .function_initial_name(receiver.object_id().unwrap())?;
                let mut result = JsString::from("function ");
                result.push_str(&name);
                result.push_str(&"() { [native code] }".into());
                Ok(Value::String(result))
            }
            NativeFunction::PrimitiveConstructor(boolean) => {
                let value = if boolean {
                    Value::Bool(self.to_boolean(first)?)
                } else {
                    Value::Number(if args.is_empty() {
                        0.0
                    } else {
                        self.coerce_number(first)?
                    })
                };
                if !construct {
                    return Ok(value);
                }
                let constructor = self.global(if boolean { "Boolean" } else { "Number" })?;
                let default = self
                    .get_property(&constructor, &"prototype".into())?
                    .object_id()
                    .unwrap();
                let prototype = self.constructor_prototype(default)?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_boxed_primitive(value, prototype)
                })?))
            }
            NativeFunction::BigInt => {
                if construct {
                    return Err(RuntimeError::TypeError(
                        "BigInt is not a constructor".into(),
                    ));
                }
                let value = self.coerce_primitive(first, "number")?;
                let Value::BigInt(value) = value else {
                    return Err(RuntimeError::TypeError(
                        "BigInt conversion is not implemented for this value".into(),
                    ));
                };
                Ok(Value::BigInt(value))
            }
            NativeFunction::PrimitiveMethod { boolean, string } => {
                let value = if let Value::Object(id) = receiver {
                    self.heap.boxed_primitive(id)?.unwrap_or(Value::Undefined)
                } else {
                    receiver
                };
                if !matches!(
                    (&value, boolean),
                    (Value::Bool(_), true) | (Value::Number(_), false)
                ) {
                    return Err(RuntimeError::TypeError(
                        "incompatible boxed primitive receiver".into(),
                    ));
                }
                if string {
                    Ok(Value::String(primitive::string(&value)?))
                } else {
                    Ok(value)
                }
            }
            NativeFunction::SymbolToString | NativeFunction::SymbolValueOf => {
                let value = if let Value::Object(id) = receiver {
                    self.heap.boxed_primitive(id)?.unwrap_or(Value::Undefined)
                } else {
                    receiver
                };
                let Value::Symbol(symbol) = value else {
                    return Err(RuntimeError::TypeError(
                        "Symbol method requires a Symbol".into(),
                    ));
                };
                if function == NativeFunction::SymbolToString {
                    Ok(Value::String(symbol.descriptive_string()))
                } else {
                    Ok(Value::Symbol(symbol))
                }
            }
            NativeFunction::BigIntToString | NativeFunction::BigIntValueOf => {
                let value = if let Value::Object(id) = receiver {
                    self.heap.boxed_primitive(id)?.unwrap_or(Value::Undefined)
                } else {
                    receiver
                };
                let Value::BigInt(value) = value else {
                    return Err(RuntimeError::TypeError(
                        "BigInt method requires a BigInt".into(),
                    ));
                };
                if function == NativeFunction::BigIntToString {
                    Ok(Value::String(value.to_string().into()))
                } else {
                    Ok(Value::BigInt(value))
                }
            }
            NativeFunction::RegExp => {
                if !construct
                    && *native::argument(&args, 1) == Value::Undefined
                    && self.is_regexp(first)?
                {
                    let constructor = self.get_property(first, &"constructor".into())?;
                    if constructor == self.regexp_global()? {
                        return Ok(first.clone());
                    }
                }
                self.regexp_create(first, native::argument(&args, 1))
            }
            NativeFunction::RegExpMethod(method) => self.regexp_method(method, &receiver, &args),
            NativeFunction::RegExpGetter(name) => self.regexp_getter(name, &receiver),
            NativeFunction::RegExpIteratorNext => self.regexp_iterator_next(&receiver),
            NativeFunction::ThrowTypeError => Err(RuntimeError::TypeError(
                "restricted function property".into(),
            )),
            NativeFunction::Empty => Ok(Value::Undefined),
            NativeFunction::ObjectValueOf => self.coerce_object(&receiver).map(Value::Object),
            NativeFunction::ObjectToString => {
                let tag = match &receiver {
                    Value::Undefined => "Undefined",
                    Value::Null => "Null",
                    Value::String(_) => "String",
                    Value::Symbol(_) => "Symbol",
                    Value::Number(_) => "Number",
                    Value::BigInt(_) => "BigInt",
                    Value::Bool(_) => "Boolean",
                    Value::Object(id) => {
                        if self.heap.boxed_string(*id)?.is_some() {
                            "String"
                        } else if self.heap.is_array(*id)? {
                            "Array"
                        } else if self.heap.is_arguments(*id)? {
                            "Arguments"
                        } else if self.is_callable(&receiver)? {
                            "Function"
                        } else if self.heap.regexp(*id)?.is_some() {
                            "RegExp"
                        } else if let Some(value) = self.heap.boxed_primitive(*id)? {
                            match value {
                                Value::Number(_) => "Number",
                                Value::Bool(_) => "Boolean",
                                Value::BigInt(_) => "BigInt",
                                Value::Symbol(_) => "Symbol",
                                _ => "Object",
                            }
                        } else {
                            "Object"
                        }
                    }
                };
                let custom = if matches!(receiver, Value::Undefined | Value::Null) {
                    Value::Undefined
                } else {
                    self.get_property(&receiver, &JsSymbol::well_known("toStringTag").into())?
                };
                let mut result = JsString::from("[object ");
                result.push_str(&if let Value::String(custom) = custom {
                    custom
                } else {
                    tag.into()
                });
                result.push_str(&"]".into());
                Ok(Value::String(result))
            }
            NativeFunction::ArrayToString => {
                let object = Value::Object(self.coerce_object(&receiver)?);
                self.stack.push(object.clone());
                let join = self.get_property(&object, &"join".into())?;
                if self.is_callable(&join)? {
                    self.call_native(join, object, vec![], false)
                } else {
                    self.native_call(NativeFunction::ObjectToString, object, vec![], false)
                }
            }
            NativeFunction::ArrayConcat => self.array_concat(&receiver, &args),
            NativeFunction::ArrayJoin => self.array_join(&receiver, first),
            NativeFunction::Symbol => Ok(Value::Symbol(JsSymbol::new(
                if matches!(first, Value::Undefined) {
                    None
                } else {
                    Some(self.coerce_string(first)?)
                },
            ))),
            NativeFunction::Object => {
                if matches!(first, Value::Undefined | Value::Null) {
                    let proto = self.object_prototype;
                    return Ok(Value::Object(
                        self.with_roots(|heap| heap.alloc_object(Some(proto)))?,
                    ));
                }
                self.coerce_object(first).map(Value::Object)
            }
            NativeFunction::ObjectMethod(method) => self.object_method(method, &receiver, &args),
            NativeFunction::StringIterator => {
                let string = self.string_receiver(&receiver)?;
                let prototype = self.string_iterator_prototype()?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_string_iterator(string, prototype)
                })?))
            }
            NativeFunction::IteratorNext => {
                let Value::Object(id) = receiver else {
                    return Err(RuntimeError::TypeError(
                        "iterator next requires an iterator".into(),
                    ));
                };
                let Some(value) = self.heap.string_iterator_next(id)? else {
                    return Err(RuntimeError::TypeError(
                        "iterator next requires a String iterator".into(),
                    ));
                };
                let done = value.is_none();
                self.iterator_result(value.map_or(Value::Undefined, Value::String), done)
            }
            NativeFunction::IteratorSelf | NativeFunction::AsyncIteratorSelf => Ok(receiver),
            NativeFunction::Pattern(method) => self.string_pattern(method, &receiver, &args),
            NativeFunction::String => {
                let string = if args.is_empty() {
                    JsString::default()
                } else {
                    self.string_constructor_argument(native::argument(&args, 0), construct)?
                };
                self.check_string(&Value::String(string.clone()))?;
                if construct {
                    let (_, prototype) = self.string_intrinsics()?;
                    let prototype = self.constructor_prototype(prototype)?;
                    Ok(Value::Object(self.with_roots(|heap| {
                        heap.alloc_string(string, Some(prototype))
                    })?))
                } else {
                    Ok(Value::String(string))
                }
            }
            NativeFunction::FromCharCode | NativeFunction::FromCodePoint => {
                let mut result = JsString::default();
                for arg in &args {
                    let number = Value::Number(self.coerce_number(arg)?);
                    let Value::String(part) = native::from_codes(
                        &[number],
                        function == NativeFunction::FromCodePoint,
                        self.config.max_string_bytes,
                    )?
                    else {
                        unreachable!()
                    };
                    native::append(&mut result, &part, self.config.max_string_bytes)?;
                }
                Ok(Value::String(result))
            }
            NativeFunction::Raw => self.string_raw(&args),
            NativeFunction::Split => self.string_split(&receiver, &args),
            NativeFunction::Replace | NativeFunction::ReplaceAll => {
                self.string_replace(&receiver, &args, function == NativeFunction::ReplaceAll)
            }
            NativeFunction::StringMethod(method) => {
                self.dispatch_string_method(method, &receiver, &args)
            }
            NativeFunction::Call => self.call_native(
                receiver,
                first.clone(),
                args.iter().skip(1).cloned().collect(),
                false,
            ),
        }
    }

    fn array_join(&mut self, receiver: &Value, separator: &Value) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let length = self.get_property(&Value::Object(object), &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        let separator = if *separator == Value::Undefined {
            ",".into()
        } else {
            self.coerce_string(separator)?
        };
        if self.joining.contains(&object) {
            return Ok(Value::String(JsString::default()));
        }
        self.joining.push(object);
        let result = (|| {
            let mut result = JsString::default();
            for index in 0..length {
                self.charge_step()?;
                if index > 0 {
                    native::append(&mut result, &separator, self.config.max_string_bytes)?;
                }
                let value = self.get_property(&Value::Object(object), &index.to_string().into())?;
                if !matches!(value, Value::Null | Value::Undefined) {
                    let value = self.coerce_string(&value)?;
                    native::append(&mut result, &value, self.config.max_string_bytes)?;
                }
            }
            Ok(Value::String(result))
        })();
        self.joining.pop();
        result
    }

    fn array_concat(&mut self, receiver: &Value, args: &[Value]) -> Result<Value, RuntimeError> {
        let mut values = Vec::new();
        for value in std::iter::once(receiver).chain(args) {
            if let Some(object) = value
                .object_id()
                .filter(|id| self.heap.is_array(*id).unwrap_or(false))
            {
                let length = self.get_property(&Value::Object(object), &"length".into())?;
                let length = self.coerce_length(&length)? as u64;
                for index in 0..length {
                    self.charge_step()?;
                    values.push(
                        self.get_property(&Value::Object(object), &index.to_string().into())?,
                    );
                }
            } else {
                values.push(value.clone());
            }
        }
        self.array_from(values)
    }

    fn array_for_each(
        &mut self,
        receiver: &Value,
        callback: &Value,
        this_arg: &Value,
    ) -> Result<Value, RuntimeError> {
        if !self.is_callable(callback)? {
            return Err(RuntimeError::TypeError(
                "Array.prototype.forEach callback must be callable".into(),
            ));
        }
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let length = self.get_property(&Value::Object(object), &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        if let Some(indices) = self.array_own_indices(object, length)? {
            for index in indices {
                self.charge_step()?;
                let value = self.get_property(&Value::Object(object), &index.to_string().into())?;
                self.call_native(
                    callback.clone(),
                    this_arg.clone(),
                    vec![value, Value::Number(index as f64), Value::Object(object)],
                    false,
                )?;
            }
        } else {
            for index in 0..length {
                self.charge_step()?;
                let key: PropertyName = index.to_string().into();
                if self.has_property(object, &key)? {
                    let value = self.get_property(&Value::Object(object), &key)?;
                    self.call_native(
                        callback.clone(),
                        this_arg.clone(),
                        vec![value, Value::Number(index as f64), Value::Object(object)],
                        false,
                    )?;
                }
            }
        }
        self.stack.pop();
        Ok(Value::Undefined)
    }

    fn array_reduce(&mut self, receiver: &Value, args: &[Value]) -> Result<Value, RuntimeError> {
        let callback = native::argument(args, 0);
        if !self.is_callable(callback)? {
            return Err(RuntimeError::TypeError(
                "Array.prototype.reduce callback must be callable".into(),
            ));
        }
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as u64;
            let mut index = 0;
            let mut accumulator = if args.len() > 1 {
                args[1].clone()
            } else {
                loop {
                    if index >= length {
                        return Err(RuntimeError::TypeError(
                            "reduce of empty array with no initial value".into(),
                        ));
                    }
                    let key: PropertyName = index.to_string().into();
                    if self.has_property(object, &key)? {
                        let value = self.get_property(&Value::Object(object), &key)?;
                        index += 1;
                        break value;
                    }
                    index += 1;
                }
            };
            while index < length {
                self.charge_step()?;
                let key: PropertyName = index.to_string().into();
                if self.has_property(object, &key)? {
                    let value = self.get_property(&Value::Object(object), &key)?;
                    accumulator = self.call_native(
                        callback.clone(),
                        Value::Undefined,
                        vec![
                            accumulator,
                            value,
                            Value::Number(index as f64),
                            Value::Object(object),
                        ],
                        false,
                    )?;
                }
                index += 1;
            }
            Ok(accumulator)
        })();
        self.stack.pop();
        result
    }

    fn array_own_indices(
        &mut self,
        object: ObjectId,
        length: u64,
    ) -> Result<Option<Vec<u32>>, RuntimeError> {
        // Scanning ordinary arrays preserves properties added by callbacks.  This
        // shortcut is only for the large sparse arrays that would otherwise turn
        // a bounded operation into millions of empty property lookups.
        if length < 65_536 || !self.heap.is_array(object)? || self.heap.proxy(object)?.is_some() {
            return Ok(None);
        }
        let mut prototype = self.heap.prototype(object)?;
        while let Some(id) = prototype {
            // The optimized path does not invoke [[HasProperty]]. A Proxy
            // prototype can observe that operation, so retain the normal path.
            if self.heap.proxy(id)?.is_some() {
                return Ok(None);
            }
            if self
                .heap
                .own_property_keys(id)?
                .iter()
                .any(|key| array_index_below_length(key, length).is_some())
            {
                return Ok(None);
            }
            prototype = self.heap.prototype(id)?;
        }
        let mut indices = Vec::new();
        for key in self.heap.own_property_keys(object)? {
            let Some(index) = array_index_below_length(&key, length) else {
                continue;
            };
            // Accessors can add or remove later indexed properties while the
            // method scans. Keep the ordinary path for that observable case.
            if self
                .heap
                .get_own_property_descriptor(object, &key)?
                .is_some_and(|descriptor| descriptor.accessor())
            {
                return Ok(None);
            }
            indices.push(index);
        }
        indices.sort_unstable();
        Ok(Some(indices))
    }

    fn array_start_index(&mut self, from_index: &Value, length: i64) -> Result<i64, RuntimeError> {
        let from_index = if *from_index == Value::Undefined {
            0
        } else {
            self.coerce_number(from_index)? as i64
        };
        Ok(if from_index < 0 {
            (length + from_index).max(0)
        } else {
            from_index.min(length)
        })
    }

    fn array_includes(
        &mut self,
        receiver: &Value,
        search: &Value,
        from_index: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as i64;
            let start = self.array_start_index(from_index, length)?;
            if let Some(indices) = self.array_own_indices(object, length as u64)? {
                let mut index = start as u64;
                for present in indices
                    .into_iter()
                    .filter(|present| i64::from(*present) >= start)
                {
                    // With no inherited indexed properties, the first omitted
                    // element is an observable `undefined` for includes.
                    if *search == Value::Undefined && index < u64::from(present) {
                        return Ok(Value::Bool(true));
                    }
                    self.charge_step()?;
                    let value = self.get_property(
                        &Value::Object(object),
                        &u64::from(present).to_string().into(),
                    )?;
                    if same_value_zero(&value, search) {
                        return Ok(Value::Bool(true));
                    }
                    index = u64::from(present) + 1;
                }
                return Ok(Value::Bool(
                    *search == Value::Undefined && index < length as u64,
                ));
            }
            for index in start..length {
                self.charge_step()?;
                let value =
                    self.get_property(&Value::Object(object), &(index as u64).to_string().into())?;
                if same_value_zero(&value, search) {
                    return Ok(Value::Bool(true));
                }
            }
            Ok(Value::Bool(false))
        })();
        self.stack.pop();
        result
    }

    fn array_index_of(
        &mut self,
        receiver: &Value,
        search: &Value,
        from_index: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.get_property(&Value::Object(object), &"length".into())?;
            let length = self.coerce_length(&length)? as i64;
            let start = self.array_start_index(from_index, length)?;
            if let Some(indices) = self.array_own_indices(object, length as u64)? {
                for present in indices {
                    if i64::from(present) < start {
                        continue;
                    }
                    self.charge_step()?;
                    let key: PropertyName = present.to_string().into();
                    if self.get_property(&Value::Object(object), &key)? == *search {
                        return Ok(Value::Number(present as f64));
                    }
                }
                return Ok(Value::Number(-1.0));
            }
            let mut index = start;
            while index < length {
                self.charge_step()?;
                let key: PropertyName = index.to_string().into();
                if self.has_property(object, &key)?
                    && self.get_property(&Value::Object(object), &key)? == *search
                {
                    return Ok(Value::Number(index as f64));
                }
                index += 1;
            }
            Ok(Value::Number(-1.0))
        })();
        self.stack.pop();
        result
    }

    fn math_global(&mut self) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.globals.get("Math") {
            return Ok(Value::Object(id));
        }
        let function_prototype = self.function_prototype()?;
        let object_prototype = self.object_prototype;
        let math = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(math)?;
        let result = (|| {
            for (name, value) in [
                ("E", std::f64::consts::E),
                ("LN10", std::f64::consts::LN_10),
                ("LN2", std::f64::consts::LN_2),
                ("LOG10E", std::f64::consts::LOG10_E),
                ("LOG2E", std::f64::consts::LOG2_E),
                ("PI", std::f64::consts::PI),
                ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
                ("SQRT2", std::f64::consts::SQRT_2),
            ] {
                self.define_data(math, name, Value::Number(value), false, false, false)?;
            }
            for (name, length, method) in [
                ("abs", 1, MathMethod::Abs),
                ("acos", 1, MathMethod::Acos),
                ("acosh", 1, MathMethod::Acosh),
                ("asin", 1, MathMethod::Asin),
                ("asinh", 1, MathMethod::Asinh),
                ("atan", 1, MathMethod::Atan),
                ("atanh", 1, MathMethod::Atanh),
                ("atan2", 2, MathMethod::Atan2),
                ("ceil", 1, MathMethod::Ceil),
                ("cbrt", 1, MathMethod::Cbrt),
                ("clz32", 1, MathMethod::Clz32),
                ("cos", 1, MathMethod::Cos),
                ("cosh", 1, MathMethod::Cosh),
                ("exp", 1, MathMethod::Exp),
                ("expm1", 1, MathMethod::Expm1),
                ("floor", 1, MathMethod::Floor),
                ("fround", 1, MathMethod::Fround),
                ("hypot", 2, MathMethod::Hypot),
                ("imul", 2, MathMethod::Imul),
                ("log", 1, MathMethod::Log),
                ("log1p", 1, MathMethod::Log1p),
                ("log2", 1, MathMethod::Log2),
                ("log10", 1, MathMethod::Log10),
                ("max", 2, MathMethod::Max),
                ("min", 2, MathMethod::Min),
                ("pow", 2, MathMethod::Pow),
                ("random", 0, MathMethod::Random),
                ("round", 1, MathMethod::Round),
                ("sign", 1, MathMethod::Sign),
                ("sin", 1, MathMethod::Sin),
                ("sinh", 1, MathMethod::Sinh),
                ("sqrt", 1, MathMethod::Sqrt),
                ("tan", 1, MathMethod::Tan),
                ("tanh", 1, MathMethod::Tanh),
                ("trunc", 1, MathMethod::Trunc),
            ] {
                self.install_native(
                    math,
                    function_prototype,
                    name,
                    length,
                    NativeFunction::Math(method),
                )?;
            }
            self.define_data(
                math,
                JsSymbol::well_known("toStringTag"),
                Value::String("Math".into()),
                false,
                false,
                true,
            )?;
            Ok(Value::Object(math))
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.globals.insert("Math".into(), math);
        }
        result
    }

    fn math_method(&mut self, method: MathMethod, args: &[Value]) -> Result<Value, RuntimeError> {
        let first = native::argument(args, 0);
        let second = native::argument(args, 1);
        let number = |value: &Value, vm: &mut Self| vm.coerce_number(value);
        let result = match method {
            MathMethod::Max | MathMethod::Min => {
                let mut result = if method == MathMethod::Max {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                };
                for value in args {
                    let value = number(value, self)?;
                    if value.is_nan() {
                        return Ok(Value::Number(f64::NAN));
                    }
                    if value == 0.0 && result == 0.0 {
                        if (method == MathMethod::Max && value.is_sign_positive())
                            || (method == MathMethod::Min && value.is_sign_negative())
                        {
                            result = value;
                        }
                    } else if (method == MathMethod::Max && value > result)
                        || (method == MathMethod::Min && value < result)
                    {
                        result = value;
                    }
                }
                result
            }
            MathMethod::Hypot => {
                let mut values = Vec::with_capacity(args.len());
                for value in args {
                    let value = number(value, self)?.abs();
                    if value.is_infinite() {
                        return Ok(Value::Number(f64::INFINITY));
                    }
                    values.push(value);
                }
                if values.iter().any(|value| value.is_nan()) {
                    f64::NAN
                } else {
                    let scale = values.iter().copied().fold(0.0_f64, f64::max);
                    if scale == 0.0 {
                        0.0
                    } else {
                        scale
                            * values
                                .iter()
                                .map(|value| (value / scale).powi(2))
                                .sum::<f64>()
                                .sqrt()
                    }
                }
            }
            MathMethod::Imul => {
                let left = primitive::to_uint32(number(first, self)?);
                let right = primitive::to_uint32(number(second, self)?);
                (left as i32).wrapping_mul(right as i32) as f64
            }
            MathMethod::Clz32 => primitive::to_uint32(number(first, self)?).leading_zeros() as f64,
            MathMethod::Atan2 => number(first, self)?.atan2(number(second, self)?),
            MathMethod::Pow => number(first, self)?.powf(number(second, self)?),
            MathMethod::Random => {
                let elapsed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                (elapsed.as_nanos() % 1_000_000_000) as f64 / 1_000_000_000.0
            }
            MathMethod::Round => {
                let value = number(first, self)?;
                if value.is_nan() || !value.is_finite() || value == 0.0 {
                    value
                } else if (-0.5..0.5).contains(&value) {
                    if value.is_sign_negative() {
                        -0.0
                    } else {
                        0.0
                    }
                } else {
                    (value + 0.5).floor()
                }
            }
            MathMethod::Sign => {
                let value = number(first, self)?;
                if value.is_nan() || value == 0.0 {
                    value
                } else {
                    value.signum()
                }
            }
            MathMethod::Abs => {
                let value = number(first, self)?;
                value.abs()
            }
            MathMethod::Acos => number(first, self)?.acos(),
            MathMethod::Acosh => number(first, self)?.acosh(),
            MathMethod::Asin => number(first, self)?.asin(),
            MathMethod::Asinh => number(first, self)?.asinh(),
            MathMethod::Atan => number(first, self)?.atan(),
            MathMethod::Atanh => number(first, self)?.atanh(),
            MathMethod::Ceil => number(first, self)?.ceil(),
            MathMethod::Cbrt => number(first, self)?.cbrt(),
            MathMethod::Cos => number(first, self)?.cos(),
            MathMethod::Cosh => number(first, self)?.cosh(),
            MathMethod::Exp => number(first, self)?.exp(),
            MathMethod::Expm1 => number(first, self)?.exp_m1(),
            MathMethod::Floor => number(first, self)?.floor(),
            MathMethod::Fround => (number(first, self)? as f32) as f64,
            MathMethod::Log => number(first, self)?.ln(),
            MathMethod::Log1p => number(first, self)?.ln_1p(),
            MathMethod::Log2 => number(first, self)?.log2(),
            MathMethod::Log10 => number(first, self)?.log10(),
            MathMethod::Sin => number(first, self)?.sin(),
            MathMethod::Sinh => number(first, self)?.sinh(),
            MathMethod::Sqrt => number(first, self)?.sqrt(),
            MathMethod::Tan => number(first, self)?.tan(),
            MathMethod::Tanh => number(first, self)?.tanh(),
            MathMethod::Trunc => number(first, self)?.trunc(),
        };
        Ok(Value::Number(result))
    }

    /// ECMA-262 Function constructor. Dynamic function source is compiled in
    /// the realm's global environment rather than inheriting the native
    /// caller's active lexical bindings.
    fn function_constructor(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        self.dynamic_function_constructor(args, false)
    }

    /// Shared constructor path for `%Function%` and `%AsyncFunction%`. Dynamic
    /// functions compile against the realm global environment; the async form
    /// then takes the same Promise/continuation path as a source async
    /// function. Generators have a separate constructor family and are not
    /// conflated with this operation.
    fn async_function_constructor(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        self.dynamic_function_constructor(args, true)
    }

    fn dynamic_function_constructor(
        &mut self,
        args: &[Value],
        async_function: bool,
    ) -> Result<Value, RuntimeError> {
        let mut source = String::from(if async_function {
            "async function anonymous("
        } else {
            "function anonymous("
        });
        for (index, argument) in args.iter().enumerate() {
            if index != 0 {
                source.push(',');
            }
            if index + 1 == args.len() {
                break;
            }
            source.push_str(&self.coerce_string(argument)?.to_utf8().map_err(|_| {
                RuntimeError::SyntaxError(
                    "Function parameter contains an unpaired surrogate".into(),
                )
            })?);
        }
        source.push_str(") {\n");
        if let Some(body) = args.last() {
            source.push_str(&self.coerce_string(body)?.to_utf8().map_err(|_| {
                RuntimeError::SyntaxError("Function body contains an unpaired surrogate".into())
            })?);
        }
        source.push_str("\n}");

        let program =
            crate::parse(&source).map_err(|error| RuntimeError::SyntaxError(error.message))?;
        let code = crate::compile(&program)
            .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        let child = code
            .functions
            .first()
            .cloned()
            .expect("Function wrapper compiles one function declaration");

        debug_assert_eq!(child.async_function, async_function);
        let function_prototype = if async_function {
            self.async_function_prototype()?
        } else {
            self.function_prototype()?
        };
        // Compiling the wrapper declaration produces a single capture for
        // its declaration name. It is an implementation detail of using the
        // ordinary compiler, not a capture of the Function caller.
        let stack_base = self.stack.len();
        let function: Result<ObjectId, RuntimeError> = (|| {
            let mut captures = Vec::with_capacity(child.captures.len());
            for _ in &child.captures {
                let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                self.stack.push(Value::Object(cell));
                captures.push(cell);
            }
            let function = self.with_roots(|heap| {
                heap.alloc_closure(
                    child.clone(),
                    captures.clone(),
                    Value::Undefined,
                    function_prototype,
                )
            })?;
            self.stack.push(Value::Object(function));
            for (&slot, &cell) in child.captures.iter().zip(&captures) {
                let value = if code.bindings[slot as usize].name == "anonymous" {
                    Value::Object(function)
                } else {
                    Value::Undefined
                };
                self.with_roots(|heap| heap.set(cell, "value", value))?;
            }
            Ok(function)
        })();
        self.stack.truncate(stack_base);
        let function = function?;
        self.stack.push(Value::Object(function));
        let result: Result<(), RuntimeError> = (|| {
            self.define_data(
                function,
                "name",
                Value::String("anonymous".into()),
                false,
                false,
                true,
            )?;
            self.define_data(
                function,
                "length",
                Value::Number(child.function_length as f64),
                false,
                false,
                true,
            )?;
            if child.constructible {
                let object_prototype = self.object_prototype;
                let prototype =
                    self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                self.define_data(
                    function,
                    "prototype",
                    Value::Object(prototype),
                    true,
                    false,
                    false,
                )?;
                self.define_data(
                    prototype,
                    "constructor",
                    Value::Object(function),
                    true,
                    false,
                    true,
                )?;
            }
            Ok(())
        })();
        self.stack.truncate(stack_base);
        result?;
        Ok(Value::Object(function))
    }

    pub(super) fn coerce_object(&mut self, value: &Value) -> Result<ObjectId, RuntimeError> {
        match value {
            Value::Object(id) => Ok(*id),
            Value::String(s) => {
                let (_, prototype) = self.string_intrinsics()?;
                Ok(self.with_roots(|heap| heap.alloc_string(s.clone(), Some(prototype)))?)
            }
            Value::Null | Value::Undefined => Err(RuntimeError::TypeError(
                "cannot convert null or undefined to Object".into(),
            )),
            _ => {
                let constructor = self.global(match value {
                    Value::Symbol(_) => "Symbol",
                    Value::Bool(_) => "Boolean",
                    Value::BigInt(_) => "BigInt",
                    _ => "Number",
                })?;
                let prototype = self
                    .get_property(&constructor, &"prototype".into())?
                    .object_id()
                    .unwrap();
                Ok(self.with_roots(|heap| heap.alloc_boxed_primitive(value.clone(), prototype))?)
            }
        }
    }

    fn object_method(
        &mut self,
        method: ObjectMethod,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        use ObjectMethod::*;
        let first = native::argument(args, 0);
        if method == IsExtensible && !matches!(first, Value::Object(_)) {
            return Ok(Value::Bool(false));
        }
        if matches!(method, IsSealed | IsFrozen) && !matches!(first, Value::Object(_)) {
            return Ok(Value::Bool(true));
        }
        if matches!(method, PreventExtensions | Seal | Freeze) && !matches!(first, Value::Object(_))
        {
            return Ok(first.clone());
        }
        if matches!(
            method,
            DefineProperty
                | DefineProperties
                | OwnKeys
                | ReflectGet
                | ReflectGetOwnPropertyDescriptor
                | ReflectDefineProperty
                | ReflectSet
                | ReflectDeleteProperty
                | ReflectPreventExtensions
                | ReflectGetPrototypeOf
                | ReflectSetPrototypeOf
                | ReflectIsExtensible
                | ReflectHas
        ) && !matches!(first, Value::Object(_))
        {
            return Err(RuntimeError::TypeError(
                "operation requires an object".into(),
            ));
        }
        let object = if matches!(method, PropertyIsEnumerable | HasOwnProperty) {
            self.coerce_object(receiver)?
        } else if method == Create {
            let prototype = match first {
                Value::Null => None,
                Value::Object(id) => Some(*id),
                _ => {
                    return Err(RuntimeError::TypeError(
                        "Object.create prototype must be object or null".into(),
                    ))
                }
            };
            self.with_roots(|heap| heap.alloc_object(prototype))?
        } else {
            self.coerce_object(first)?
        };
        self.stack.push(Value::Object(object));
        match method {
            DefineProperties => {
                let properties = self.coerce_object(native::argument(args, 1))?;
                self.stack.push(Value::Object(properties));
                let mut descriptors = Vec::new();
                for key in self.object_own_property_keys(properties)? {
                    if self
                        .object_get_own_property(properties, &key)?
                        .is_none_or(|descriptor| descriptor.enumerable != Some(true))
                    {
                        continue;
                    }
                    let descriptor_object = self.get_property(&Value::Object(properties), &key)?;
                    self.stack.push(descriptor_object.clone());
                    descriptors.push((key, self.read_descriptor(&descriptor_object)?));
                }
                for (key, descriptor) in descriptors {
                    if !self.object_define_own_property(object, key, descriptor)? {
                        return Err(RuntimeError::TypeError("cannot redefine property".into()));
                    }
                }
                Ok(Value::Object(object))
            }
            ReflectGet => {
                let key = self.coerce_property_key(native::argument(args, 1))?;
                let receiver = args.get(2).cloned().unwrap_or(Value::Object(object));
                self.get_object_property(object, &receiver, &key)
            }
            GetOwnPropertyDescriptor
            | ReflectGetOwnPropertyDescriptor
            | DefineProperty
            | ReflectDefineProperty => {
                let key = self.coerce_property_key(native::argument(args, 1))?;
                if matches!(method, DefineProperty | ReflectDefineProperty) {
                    let mut descriptor = self.read_descriptor(native::argument(args, 2))?;
                    if key == "length" && self.heap.is_array(object)? {
                        if let Some(value) = &descriptor.value {
                            descriptor.value = Some(self.array_length_value(value)?);
                        }
                    }
                    let defined = self.object_define_own_property(object, key, descriptor)?;
                    if method == ReflectDefineProperty {
                        return Ok(Value::Bool(defined));
                    }
                    if !defined {
                        return Err(RuntimeError::TypeError("cannot redefine property".into()));
                    }
                    return Ok(Value::Object(object));
                }
                let Some(descriptor) = self.object_get_own_property(object, &key)? else {
                    return Ok(Value::Undefined);
                };
                let prototype = self.object_prototype;
                let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(result));
                for (name, value) in [
                    ("value", descriptor.value),
                    ("writable", descriptor.writable.map(Value::Bool)),
                    ("get", descriptor.get),
                    ("set", descriptor.set),
                    ("enumerable", descriptor.enumerable.map(Value::Bool)),
                    ("configurable", descriptor.configurable.map(Value::Bool)),
                ] {
                    if let Some(value) = value {
                        self.with_roots(|heap| heap.set(result, name, value))?;
                    }
                }
                Ok(Value::Object(result))
            }
            PropertyIsEnumerable => {
                let key = self.coerce_property_key(native::argument(args, 0))?;
                Ok(Value::Bool(
                    self.heap
                        .get_own_property_descriptor(object, key)?
                        .is_some_and(|descriptor| descriptor.enumerable == Some(true)),
                ))
            }
            HasOwnProperty => {
                let key = self.coerce_property_key(native::argument(args, 0))?;
                Ok(Value::Bool(
                    self.heap
                        .get_own_property_descriptor(object, key)?
                        .is_some(),
                ))
            }
            Keys | GetOwnPropertyNames | GetOwnPropertySymbols | OwnKeys => {
                let keys = self.object_own_property_keys(object)?;
                let mut values = Vec::new();
                for key in keys {
                    if method == Keys
                        && (!matches!(key, PropertyName::String(_))
                            || self
                                .object_get_own_property(object, &key)?
                                .unwrap()
                                .enumerable
                                != Some(true))
                    {
                        continue;
                    }
                    if method == GetOwnPropertyNames && !matches!(key, PropertyName::String(_))
                        || method == GetOwnPropertySymbols
                            && !matches!(key, PropertyName::Symbol(_))
                    {
                        continue;
                    }
                    values.push(key.value());
                }
                self.array_from(values)
            }
            GetPrototypeOf | ReflectGetPrototypeOf => Ok(self
                .object_get_prototype(object)?
                .map_or(Value::Null, Value::Object)),
            SetPrototypeOf | ReflectSetPrototypeOf => {
                let prototype = match native::argument(args, 1) {
                    Value::Null => None,
                    Value::Object(id) => Some(*id),
                    _ => {
                        return Err(RuntimeError::TypeError(
                            "prototype must be object or null".into(),
                        ))
                    }
                };
                let changed = self.object_set_prototype(object, prototype)?;
                if method == ReflectSetPrototypeOf {
                    return Ok(Value::Bool(changed));
                }
                if !changed {
                    return Err(RuntimeError::TypeError(
                        "cannot set object prototype".into(),
                    ));
                }
                Ok(first.clone())
            }
            Create => {
                let properties = native::argument(args, 1);
                if *properties != Value::Undefined {
                    let properties = self.coerce_object(properties)?;
                    self.stack.push(Value::Object(properties));
                    let mut descriptors = Vec::new();
                    for key in self.object_own_property_keys(properties)? {
                        if self
                            .object_get_own_property(properties, &key)?
                            .is_some_and(|d| d.enumerable == Some(true))
                        {
                            let value = self.get_property(&Value::Object(properties), &key)?;
                            self.stack.push(value.clone());
                            descriptors.push((key, self.read_descriptor(&value)?));
                        }
                    }
                    for (key, descriptor) in descriptors {
                        if !self.object_define_own_property(object, key, descriptor)? {
                            return Err(RuntimeError::TypeError("cannot define property".into()));
                        }
                    }
                }
                Ok(Value::Object(object))
            }
            IsExtensible | ReflectIsExtensible => {
                Ok(Value::Bool(self.object_is_extensible(object)?))
            }
            PreventExtensions => {
                if !self.object_prevent_extensions(object)? {
                    return Err(RuntimeError::TypeError(
                        "cannot prevent object extensions".into(),
                    ));
                }
                Ok(Value::Object(object))
            }
            ReflectPreventExtensions => Ok(Value::Bool(self.object_prevent_extensions(object)?)),
            ReflectSet => {
                let key = self.coerce_property_key(native::argument(args, 1))?;
                let value = native::argument(args, 2).clone();
                let receiver = args.get(3).cloned().unwrap_or(Value::Object(object));
                Ok(Value::Bool(self.ordinary_set_with_receiver(
                    object, &receiver, &key, &value,
                )?))
            }
            ReflectDeleteProperty => {
                let key = self.coerce_property_key(native::argument(args, 1))?;
                Ok(Value::Bool(self.object_delete(object, &key)?))
            }
            ReflectHas => {
                let key = self.coerce_property_key(native::argument(args, 1))?;
                Ok(Value::Bool(self.has_property(object, &key)?))
            }
            Seal | Freeze => {
                let keys = self.object_own_property_keys(object)?;
                for key in keys {
                    let Some(current) = self.object_get_own_property(object, &key)? else {
                        continue;
                    };
                    let descriptor = PropertyDescriptor {
                        configurable: Some(false),
                        writable: (method == Freeze && current.value.is_some()).then_some(false),
                        ..Default::default()
                    };
                    if !self.object_define_own_property(object, key, descriptor)? {
                        return Err(RuntimeError::TypeError(
                            "cannot make object non-extensible".into(),
                        ));
                    }
                }
                if !self.object_prevent_extensions(object)? {
                    return Err(RuntimeError::TypeError(
                        "cannot make object non-extensible".into(),
                    ));
                }
                Ok(first.clone())
            }
            IsSealed | IsFrozen => {
                if self.object_is_extensible(object)? {
                    return Ok(Value::Bool(false));
                }
                for key in self.object_own_property_keys(object)? {
                    let Some(descriptor) = self.object_get_own_property(object, &key)? else {
                        continue;
                    };
                    if descriptor.configurable != Some(false)
                        || (method == IsFrozen
                            && descriptor.value.is_some()
                            && descriptor.writable != Some(false))
                    {
                        return Ok(Value::Bool(false));
                    }
                }
                Ok(Value::Bool(true))
            }
        }
    }

    fn read_descriptor(&mut self, value: &Value) -> Result<PropertyDescriptor, RuntimeError> {
        let Value::Object(object) = value else {
            return Err(RuntimeError::TypeError(
                "descriptor must be an object".into(),
            ));
        };
        let base = self.stack.len();
        let result = (|| {
            let mut descriptor = PropertyDescriptor::default();
            for name in [
                "enumerable",
                "configurable",
                "value",
                "writable",
                "get",
                "set",
            ] {
                if !self.has_property(*object, &name.into())? {
                    continue;
                }
                let property = self.get_property(value, &name.into())?;
                // Later descriptor getters may allocate and collect earlier values.
                self.stack.push(property.clone());
                match name {
                    "enumerable" => descriptor.enumerable = Some(self.to_boolean(&property)?),
                    "configurable" => descriptor.configurable = Some(self.to_boolean(&property)?),
                    "writable" => descriptor.writable = Some(self.to_boolean(&property)?),
                    "value" => descriptor.value = Some(property),
                    _ => {
                        if property != Value::Undefined && !self.is_callable(&property)? {
                            return Err(RuntimeError::TypeError(
                                "accessor must be callable or undefined".into(),
                            ));
                        }
                        if name == "get" {
                            descriptor.get = Some(property);
                        } else {
                            descriptor.set = Some(property);
                        }
                    }
                }
            }
            if descriptor.accessor()
                && (descriptor.value.is_some() || descriptor.writable.is_some())
            {
                return Err(RuntimeError::TypeError(
                    "invalid mixed property descriptor".into(),
                ));
            }
            Ok(descriptor)
        })();
        self.stack.truncate(base);
        result
    }

    fn string_iterator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.iterator_prototype {
            return Ok(prototype);
        }
        let constructor = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(constructor)?.unwrap();
        let iterator_base = self.base_iterator_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(iterator_base)))?;
        let root = self.heap.root(prototype)?;
        let result = (|| {
            self.install_native(
                prototype,
                function_prototype,
                "next",
                0,
                NativeFunction::IteratorNext,
            )?;
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("String Iterator".into()),
                false,
                false,
                true,
            )?;
            Ok(prototype)
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
        } else {
            self.iterator_prototype = Some(prototype);
        }
        result
    }

    pub(super) fn install_symbol_native(
        &mut self,
        owner: ObjectId,
        prototype: ObjectId,
        symbol: &str,
        length: u32,
        native: NativeFunction,
    ) -> Result<(), RuntimeError> {
        let name = format!("[Symbol.{symbol}]");
        let id = self.with_roots(|heap| heap.alloc_native_function(native, &name, prototype))?;
        self.stack.push(Value::Object(id));
        self.define_data(
            id,
            "name",
            Value::String(format!("[Symbol.{symbol}]").into()),
            false,
            false,
            true,
        )?;
        self.define_data(
            id,
            "length",
            Value::Number(length as f64),
            false,
            false,
            true,
        )?;
        // Function.prototype @@hasInstance is the immutable Symbol method.
        let mutable = native != NativeFunction::HasInstance;
        self.define_data(
            owner,
            JsSymbol::well_known(symbol),
            Value::Object(id),
            mutable,
            false,
            mutable,
        )?;
        self.stack.pop();
        Ok(())
    }

    fn string_pattern(
        &mut self,
        method: PatternMethod,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        if matches!(receiver, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "String method requires a non-null receiver".into(),
            ));
        }
        let pattern = native::argument(args, 0);
        let symbol = match method {
            PatternMethod::Match => "match",
            PatternMethod::MatchAll => "matchAll",
            PatternMethod::Search => "search",
        };
        if !matches!(pattern, Value::Null | Value::Undefined) {
            if method == PatternMethod::MatchAll {
                self.require_global_pattern(pattern)?;
            }
            let method = self.get_method(pattern, &JsSymbol::well_known(symbol).into())?;
            if method != Value::Undefined {
                return self.call_native(method, pattern.clone(), vec![receiver.clone()], false);
            }
        }
        let string = self.coerce_string(receiver)?;
        let regexp = self.regexp_create(
            pattern,
            &if method == PatternMethod::MatchAll {
                Value::String("g".into())
            } else {
                Value::Undefined
            },
        )?;
        self.stack.push(regexp.clone());
        let function = self.get_property(&regexp, &JsSymbol::well_known(symbol).into())?;
        self.call_native(function, regexp, vec![Value::String(string)], false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn array_concat_keeps_non_array_values_as_elements() {
        let mut vm = Vm::default();
        vm.remaining_instructions = vm.config.instruction_budget;
        let receiver = vm.array_from(vec![Value::Number(1.0)]).unwrap();
        let result = vm.array_concat(&receiver, &[Value::Number(2.0)]).unwrap();
        let result = result.object_id().unwrap();
        assert_eq!(vm.heap.get(result, "0"), Ok(Value::Number(1.0)));
        assert_eq!(vm.heap.get(result, "1"), Ok(Value::Number(2.0)));
    }

    #[test]
    fn math_extrema_replace_the_running_result_for_later_arguments() {
        let mut vm = Vm::default();
        assert_eq!(
            vm.math_method(MathMethod::Max, &[Value::Number(1.0), Value::Number(2.0)]),
            Ok(Value::Number(2.0))
        );
        assert_eq!(
            vm.math_method(MathMethod::Min, &[Value::Number(2.0), Value::Number(1.0)]),
            Ok(Value::Number(1.0))
        );
    }

    #[test]
    fn generator_prototype_releases_its_temporary_root_after_an_allocation_failure() {
        let prerequisites_ready = |max_heap_bytes| {
            let config = VmConfig {
                heap: HeapConfig {
                    major_threshold_bytes: max_heap_bytes,
                    max_heap_bytes,
                    ..HeapConfig::default()
                },
                ..VmConfig::default()
            };
            let Ok(mut vm) = Vm::new(config) else {
                return false;
            };
            let _ = vm.generator_prototype();
            vm.string_intrinsics.is_some() && vm.iterator_base.is_some()
        };
        let mut lower = 4_096;
        let mut upper = 4 * 1024 * 1024;
        assert!(
            prerequisites_ready(upper),
            "the bounded search must initialize the prerequisite prototypes"
        );
        while lower + 1 < upper {
            let middle = lower + (upper - lower) / 2;
            if prerequisites_ready(middle) {
                upper = middle;
            } else {
                lower = middle;
            }
        }
        let mut found = false;
        for max_heap_bytes in upper..=upper + 4_096 {
            let config = VmConfig {
                heap: HeapConfig {
                    major_threshold_bytes: max_heap_bytes,
                    max_heap_bytes,
                    ..HeapConfig::default()
                },
                ..VmConfig::default()
            };
            let Ok(mut vm) = Vm::new(config) else {
                continue;
            };
            let result = vm.generator_prototype();
            let candidate = vm.string_intrinsics.is_some()
                && vm.iterator_base.is_some()
                && matches!(
                    result,
                    Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))
                );
            if candidate {
                found = true;
            }
            if found {
                break;
            }
        }
        assert!(
            found,
            "a bounded heap must exercise generator prototype cleanup"
        );
    }
}
