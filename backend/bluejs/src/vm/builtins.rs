// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

mod arrays;
mod binary_data;
mod execution;
mod generators;
mod globals;
mod math;
mod native_dispatch;
mod object;
mod promises;
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

/// Annex B permits HTML-style single-line comments while parsing the formal
/// parameter text supplied to the dynamic Function constructors.  The parser
/// intentionally keeps the main grammar strict, so normalize only this
/// legacy, Script-goal input before compiling the generated wrapper.  Strings,
/// templates, and ordinary comments retain their source verbatim.
fn strip_dynamic_function_html_comments(source: &str) -> String {
    #[derive(Clone, Copy)]
    enum Mode {
        Code,
        SingleQuoted,
        DoubleQuoted,
        Template,
        LineComment,
        BlockComment,
        HtmlComment,
    }

    let source = source.chars().collect::<Vec<_>>();
    let mut result = String::new();
    let mut mode = Mode::Code;
    // Parameter text follows the opening parenthesis in the generated source,
    // so it is not initially at a line start. `-->` becomes an Annex B HTML
    // close comment only after a line terminator in that text.
    let mut line_start = false;
    let mut escaped = false;
    let mut index = 0;
    while index < source.len() {
        let character = source[index];
        let next = source.get(index + 1).copied();
        let follows = |text: &[char]| source[index..].starts_with(text);
        match mode {
            Mode::Code if follows(&['<', '!', '-', '-']) => {
                mode = Mode::HtmlComment;
                index += 4;
                continue;
            }
            Mode::Code if line_start && follows(&['-', '-', '>']) => {
                mode = Mode::HtmlComment;
                index += 3;
                continue;
            }
            Mode::Code if character == '/' && next == Some('/') => {
                result.push(character);
                result.push('/');
                mode = Mode::LineComment;
                index += 2;
                continue;
            }
            Mode::Code if character == '/' && next == Some('*') => {
                result.push(character);
                result.push('*');
                mode = Mode::BlockComment;
                index += 2;
                continue;
            }
            Mode::Code if character == '\'' => mode = Mode::SingleQuoted,
            Mode::Code if character == '"' => mode = Mode::DoubleQuoted,
            Mode::Code if character == '`' => mode = Mode::Template,
            Mode::SingleQuoted | Mode::DoubleQuoted | Mode::Template if escaped => {
                escaped = false;
            }
            Mode::SingleQuoted | Mode::DoubleQuoted | Mode::Template if character == '\\' => {
                escaped = true;
            }
            Mode::SingleQuoted if character == '\'' => mode = Mode::Code,
            Mode::DoubleQuoted if character == '"' => mode = Mode::Code,
            Mode::Template if character == '`' => mode = Mode::Code,
            Mode::LineComment if matches!(character, '\n' | '\r') => mode = Mode::Code,
            Mode::BlockComment if character == '*' && next == Some('/') => {
                result.push(character);
                result.push('/');
                mode = Mode::Code;
                index += 2;
                continue;
            }
            Mode::HtmlComment if matches!(character, '\n' | '\r') => {
                mode = Mode::Code;
                line_start = true;
                result.push(character);
                index += 1;
                continue;
            }
            Mode::HtmlComment => {
                index += 1;
                continue;
            }
            _ => {}
        }
        result.push(character);
        line_start = if matches!(character, '\n' | '\r') {
            true
        } else if matches!(mode, Mode::Code) && character.is_whitespace() {
            line_start
        } else {
            false
        };
        index += 1;
    }
    result
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

    /// Annex B legacy own properties on a non-strict ordinary, constructible
    /// function hide the restricted accessors inherited from
    /// `%Function.prototype%`. Until call-chain tracking exists, `caller`
    /// remains `undefined`, which preserves the standard compatibility
    /// fallback instead of falsely advertising an active caller extension.
    pub(super) fn install_legacy_function_properties(
        &mut self,
        function: ObjectId,
    ) -> Result<(), RuntimeError> {
        for (name, value) in [("arguments", Value::Null), ("caller", Value::Undefined)] {
            self.define_data(function, name, value, false, false, false)?;
        }
        Ok(())
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
            self.install_native(
                prototype,
                function_prototype,
                "throw",
                1,
                NativeFunction::GeneratorThrow,
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
}

impl Vm {
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
            if iterator == Value::Undefined {
                let object = self.coerce_object(&source)?;
                self.stack.push(Value::Object(object));
                let values = self.array_like_values(&Value::Object(object));
                self.stack.pop();
                let mut values = values?;
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
                return self.array_from(values);
            }

            // Array.from maps one iterator value at a time. Collecting the
            // iterator first makes an infinite source consume its resource
            // budget before an abrupt mapper can close it, which is both
            // observably wrong and turns finite conformance checks into
            // timeouts.
            let record = self.get_iterator_from_method(&source, iterator)?;
            self.stack.push(record.clone());
            let array = self.array_from(Vec::new())?;
            self.stack.push(array.clone());
            let outcome = (|| {
                let mut index = 0usize;
                while let Some(value) = self.iterator_step(&record, true)? {
                    let value = if mapper == Value::Undefined {
                        value
                    } else {
                        self.call_native(
                            mapper.clone(),
                            this_arg.clone(),
                            vec![value, Value::Number(index as f64)],
                            false,
                        )?
                    };
                    self.array_push(&array, &value, 0)?;
                    index = index.checked_add(1).ok_or(RuntimeError::RangeError(
                        "Array.from result length is too large".into(),
                    ))?;
                }
                Ok(array)
            })();
            if outcome.is_err() {
                // IteratorClose retains an existing abrupt completion. The
                // original mapper/iterator error must win over a return()
                // failure, so close only for its required side effect here.
                let _ = self.iterator_close(&record);
            }
            outcome
        })();
        self.stack.truncate(base);
        result
    }
}

impl Vm {
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
            if let Some((_, _, callable, _)) = self.test262_foreign_reference(*id) {
                callable
            } else if let Some((callable, _)) = self.heap.proxy_capabilities(*id)? {
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

    fn uri_coding_error<T>(&mut self, error: native::UriCodingError) -> Result<T, RuntimeError> {
        match error {
            native::UriCodingError::Malformed => Err(RuntimeError::Thrown(
                self.error_object("URIError", "malformed URI".into())?,
            )),
            native::UriCodingError::StringLimit { limit } => {
                Err(RuntimeError::StringLimit { limit })
            }
        }
    }

    fn encode_uri(&mut self, value: &Value, component: bool) -> Result<Value, RuntimeError> {
        let string = self.coerce_string(value)?;
        match native::encode_uri(&string, component, self.config.max_string_bytes) {
            Ok(result) => Ok(Value::String(result)),
            Err(error) => self.uri_coding_error(error),
        }
    }

    fn decode_uri(&mut self, value: &Value, component: bool) -> Result<Value, RuntimeError> {
        let string = self.coerce_string(value)?;
        match native::decode_uri(&string, component, self.config.max_string_bytes) {
            Ok(result) => Ok(Value::String(result)),
            Err(error) => self.uri_coding_error(error),
        }
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
}

impl Vm {
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
        let mut parameters = String::new();
        for (index, argument) in args.iter().take(args.len().saturating_sub(1)).enumerate() {
            if index != 0 {
                parameters.push(',');
            }
            parameters.push_str(&self.coerce_string(argument)?.to_utf8().map_err(|_| {
                RuntimeError::SyntaxError(
                    "Function parameter contains an unpaired surrogate".into(),
                )
            })?);
        }
        let mut source = String::from(if async_function {
            "async function anonymous("
        } else {
            "function anonymous("
        });
        source.push_str(&strip_dynamic_function_html_comments(&parameters));
        // Dynamic parameter text is parsed as its own grammar production.
        // Preserve that boundary in the generated wrapper: a trailing
        // single-line comment belongs to the parameters, not to the closing
        // parenthesis that follows them.
        source.push_str("\n) {\n");
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
        } else if self.new_target != Value::Undefined {
            let default = self.function_prototype()?;
            self.constructor_prototype(default)?
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
            if child.constructible
                && !child.strict
                && !child.arrow
                && !child.generator
                && !child.async_function
            {
                self.install_legacy_function_properties(function)?;
            }
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
