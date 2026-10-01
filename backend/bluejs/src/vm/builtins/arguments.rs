// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// The number an internal list's `length` holds; every array's does.
pub(super) fn number_or_zero(value: &Value) -> f64 {
    if let Value::Number(number) = value {
        *number
    } else {
        0.0
    }
}

impl Vm {
    /// Roots an object this module has just allocated while it goes on to
    /// build the rest of an intrinsic; released again by [`Self::release_root`].
    pub(in super::super) fn root_new_object(&mut self, object: ObjectId) -> crate::heap::RootId {
        self.heap
            .root(object)
            .expect("a freshly allocated object can be rooted")
    }

    /// Releases a root registered by [`Self::root_new_object`] (or by another
    /// caller that registered exactly one root for it).
    pub(in super::super) fn release_root(&mut self, root: crate::heap::RootId) {
        self.heap
            .unroot(root)
            .expect("the root is still registered");
    }

    /// Materializes the per-invocation arguments binding after the function
    /// environment has entered. Mapped indices point at the same heap cells
    /// as simple sloppy parameter bindings; every other index remains an
    /// ordinary data property copied from the argument list.
    pub(in super::super) fn create_arguments_object(
        &mut self,
        code: &Bytecode,
    ) -> Result<(), RuntimeError> {
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
            let array_prototype = self
                .get_property(&array, &"prototype".into())
                .expect("Array.prototype is a plain data property");
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

    pub(in super::super) fn throw_type_error(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(function) = self.throw_type_error {
            return Ok(function);
        }
        let constructor = self.string_intrinsics()?.0;
        let prototype = self
            .heap
            .prototype(constructor)
            .expect("the String constructor is a live object")
            .expect("String constructor has Function.prototype");
        let function = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::ThrowTypeError, "", prototype)
        })?;
        let root = self.root_new_object(function);
        let result = (|| {
            // %ThrowTypeError% is frozen: `length` then `name`, both
            // non-configurable, and the function is not extensible.
            self.define_data(function, "length", Value::Number(0.0), false, false, false)?;
            self.define_data(
                function,
                "name",
                Value::String("".into()),
                false,
                false,
                false,
            )?;
            self.heap
                .prevent_extensions(function)
                .expect("%ThrowTypeError% was just allocated");
            Ok(function)
        })();
        match result {
            Ok(function) => {
                self.throw_type_error = Some(function);
                Ok(function)
            }
            Err(error) => {
                self.release_root(root);
                Err(error)
            }
        }
    }

    /// Annex B legacy own properties on a non-strict ordinary, constructible
    /// function hide the restricted accessors inherited from
    /// `%Function.prototype%`. They are own accessors sharing one getter pair
    /// (no setter, so assignment is ignored or, in strict code, throws): see
    /// `legacy_function_caller` and `legacy_function_arguments`.
    pub(in super::super) fn install_legacy_function_properties(
        &mut self,
        function: ObjectId,
    ) -> Result<(), RuntimeError> {
        let (caller, arguments) = self.legacy_function_getters()?;
        for (name, getter) in [("arguments", arguments), ("caller", caller)] {
            let descriptor = PropertyDescriptor {
                get: Some(Value::Object(getter)),
                set: Some(Value::Undefined),
                enumerable: Some(false),
                configurable: Some(false),
                ..PropertyDescriptor::default()
            };
            let defined =
                self.with_roots(|heap| heap.define_own_property(function, name, descriptor))?;
            assert!(defined, "a new function accepts its legacy properties");
        }
        Ok(())
    }

    fn legacy_function_getters(&mut self) -> Result<(ObjectId, ObjectId), RuntimeError> {
        if let Some(getters) = self.legacy_function_getters {
            return Ok(getters);
        }
        let constructor = self.string_intrinsics()?.0;
        let prototype = self
            .heap
            .prototype(constructor)
            .expect("the String constructor is a live object")
            .expect("String constructor has Function.prototype");
        let mut created = Vec::new();
        let result = (|| {
            for native in [
                NativeFunction::LegacyFunctionCaller,
                NativeFunction::LegacyFunctionArguments,
            ] {
                let function =
                    self.with_roots(|heap| heap.alloc_native_function(native, "", prototype))?;
                created.push((function, self.root_new_object(function)));
                self.define_data(
                    function,
                    "name",
                    Value::String("".into()),
                    false,
                    false,
                    true,
                )?;
                self.define_data(function, "length", Value::Number(0.0), false, false, true)?;
            }
            Ok((created[0].0, created[1].0))
        })();
        match result {
            Ok(getters) => {
                self.legacy_function_getters = Some(getters);
                Ok(getters)
            }
            Err(error) => {
                for (_, root) in created {
                    self.release_root(root);
                }
                Err(error)
            }
        }
    }

    /// `f.caller`: the sloppy function that is currently running `f`, or
    /// `null` when `f` is not running, was called from outside any function,
    /// or was called by a function the legacy reflection proposal censors
    /// (strict, generator, async, class constructor).
    pub(in super::super) fn legacy_function_caller(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(function) = receiver else {
            return Ok(Value::Null);
        };
        let Some(caller) = self
            .call_stack
            .iter()
            .rposition(|running| running == function)
            .and_then(|position| position.checked_sub(1))
            .and_then(|position| self.call_stack.get(position).copied())
        else {
            return Ok(Value::Null);
        };
        // Only closures are pushed on the call stack.
        let (code, ..) = self
            .heap
            .closure(caller)
            .expect("a function on the call stack is a live object")
            .expect("a function on the call stack is a closure");
        if code.strict || code.generator || code.async_function || code.class_constructor {
            return Ok(Value::Null);
        }
        Ok(Value::Object(caller))
    }

    /// `f.arguments`: a copy of the running call's arguments while `f`'s own
    /// body reads it, `null` otherwise. Outer activations of `f` are not
    /// reachable from here, so they also report `null`.
    pub(in super::super) fn legacy_function_arguments(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(function) = receiver else {
            return Ok(Value::Null);
        };
        if self.call_stack.last() != Some(function) || self.callee != *receiver {
            return Ok(Value::Null);
        }
        let base = self.stack.len();
        self.stack.extend(self.arguments.iter().cloned());
        let result = (|| {
            let object_prototype = self.object_prototype;
            let object =
                self.with_roots(|heap| heap.alloc_arguments(HashMap::new(), object_prototype))?;
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
            let array_prototype = Value::Object(self.array_prototype);
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
            self.define_data(object, "callee", receiver.clone(), true, false, true)?;
            Ok(Value::Object(object))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn direct_eval(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let Value::String(source) = value else {
            return Ok(value.clone());
        };
        let source = crate::source_encoding::encode(source);
        let program = crate::parse_eval(&source, self.strict)
            .map_err(|error| RuntimeError::SyntaxError(error.message))?;
        let visible = self.eval_visible_bindings();
        // Only the derived constructor itself, or an arrow function or eval
        // code inside it, has the constructor binding `super()` needs.
        let derived_constructor = visible
            .iter()
            .any(|(name, _, _)| name == crate::compiler::DERIVED_CONSTRUCTOR_BINDING);
        if crate::ast::contains_super_call_outside_class(&program)
            && (self.class_field_initializer || !derived_constructor)
        {
            return Err(RuntimeError::SyntaxError(
                "super() is not valid in this eval context".into(),
            ));
        }
        // A field initializer has no `arguments` binding, so eval code inside
        // it (or inside an arrow function it created) may not refer to one.
        if self.class_field_initializer && crate::ast::statements_contain_arguments(&program.body) {
            return Err(RuntimeError::SyntaxError(
                "arguments is not valid in a class field initializer".into(),
            ));
        }
        if crate::ast::contains_super_property_outside_class(&program) && self.home_object.is_none()
        {
            return Err(RuntimeError::SyntaxError(
                "super property is not valid in this eval context".into(),
            ));
        }
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
            crate::compiler::EvalContext {
                strict: self.strict,
                new_target_allowed: self.new_target_allowed,
                class_field_initializer: self.class_field_initializer,
                with_scopes: crate::compiler::EvalWithScopes {
                    depth: self.with_objects.len(),
                    inherited: self.inherited_with_depth,
                },
            },
            self.config.eval_compile_limits,
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
            self.this = self
                .global("globalThis")
                .expect("the global object exists once code is running");
        }
        let global_var_environment = !code.strict && global_execution;
        self.execute_eval(&code, captures, global_var_environment)
    }

    /// Indirect eval starts from the realm global environment. It never
    /// captures caller bindings, even when the caller itself is strict.
    pub(in super::super) fn indirect_eval(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let Value::String(source) = value else {
            return Ok(value.clone());
        };
        let source = crate::source_encoding::encode(source);
        let program = crate::parser::parse_encoded(&source)
            .map_err(|error| RuntimeError::SyntaxError(error.message))?;
        let code = crate::compiler::compile_eval(
            &program,
            &[],
            &[],
            &[],
            crate::compiler::EvalContext::default(),
            self.config.eval_compile_limits,
        )
        .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        let global_this = self
            .global("globalThis")
            .expect("the global object exists once code is running");
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

    pub(in super::super) fn is_intrinsic_eval(&self, value: &Value) -> Result<bool, RuntimeError> {
        let Some(object) = value.object_id() else {
            return Ok(false);
        };
        Ok(self
            .heap
            .native_function(object)
            .expect("an object handed to a call is live")
            == Some(NativeFunction::Eval))
    }

    pub(in super::super) fn array_push(
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
            let length = number_or_zero(
                &self
                    .heap
                    .get(id, "length")
                    .expect("an array being appended to is a live object"),
            );
            if kind == 0 {
                self.with_roots(|heap| heap.set(id, (length as u32).to_string(), value.clone()))?;
            }
            // Even an extensible array rejects a length greater than 2^32-1.
            self.with_roots(|heap| heap.set(id, "length", Value::Number(length + 1.0)))?;
        }
        Ok(())
    }

    pub(in super::super) fn array_iterator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.array_iterator_prototype {
            return Ok(prototype);
        }
        let constructor = self.string_intrinsics()?.0;
        let function_prototype = self
            .heap
            .prototype(constructor)
            .expect("the String constructor is a live object")
            .unwrap();
        let base = self.base_iterator_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(base)))?;
        let root = self.root_new_object(prototype);
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
            self.release_root(root);
        } else {
            self.array_iterator_prototype = Some(prototype);
        }
        result
    }

    /// `%GeneratorFunction.prototype%` is the prototype of generator
    /// function objects, distinct from the `.prototype` object owned by an
    /// individual generator function for its iterator instances.  Keeping
    /// the `@@toStringTag` here gives both a generator function and a Proxy
    /// around it the standard `GeneratorFunction` observation, while later
    /// mutation through `function.constructor.prototype` remains visible.
    pub(in super::super) fn generator_function_prototype(
        &mut self,
    ) -> Result<ObjectId, RuntimeError> {
        if self.generator_function_prototype.is_none() {
            self.generator_intrinsics()?;
        }
        Ok(self
            .generator_function_prototype
            .expect("generator intrinsics install both prototypes"))
    }

    /// Creates `%GeneratorFunction.prototype%` and `%GeneratorPrototype%`
    /// together and links them: each is the other's `constructor` /
    /// `prototype` (both non-writable, non-enumerable, configurable), which
    /// is how `Object.getPrototypeOf(function* () {}).prototype` reaches the
    /// prototype shared by every generator object. On failure neither is
    /// installed and both temporary roots are released.
    fn generator_intrinsics(&mut self) -> Result<(), RuntimeError> {
        let (function_side, function_root) = self.build_generator_function_prototype()?;
        let (generator_side, generator_root) = match self.build_generator_prototype() {
            Ok(built) => built,
            Err(error) => {
                self.release_root(function_root);
                return Err(error);
            }
        };
        let base = self.stack.len();
        self.stack.push(Value::Object(function_side));
        self.stack.push(Value::Object(generator_side));
        let linked = (|| {
            self.define_data(
                function_side,
                "prototype",
                Value::Object(generator_side),
                false,
                false,
                true,
            )?;
            self.define_data(
                generator_side,
                "constructor",
                Value::Object(function_side),
                false,
                false,
                true,
            )
        })();
        self.stack.truncate(base);
        match linked {
            Ok(()) => {
                self.generator_function_prototype = Some(function_side);
                self.generator_prototype = Some(generator_side);
                Ok(())
            }
            Err(error) => {
                self.release_root(function_root);
                self.release_root(generator_root);
                Err(error)
            }
        }
    }

    fn build_generator_function_prototype(
        &mut self,
    ) -> Result<(ObjectId, crate::heap::RootId), RuntimeError> {
        // %GeneratorFunction% inherits from the %Function% constructor, not
        // from %Function.prototype%.
        let function_constructor = self
            .global("Function")?
            .object_id()
            .expect("Function is callable");
        // `global` built the String intrinsics before answering.
        let function_prototype = self
            .function_prototype()
            .expect("the String intrinsics exist");
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(function_prototype)))?;
        let root = self.root_new_object(prototype);
        let base = self.stack.len();
        self.stack.push(Value::Object(prototype));
        let result = (|| {
            let constructor = self.with_roots(|heap| {
                heap.alloc_native_function(
                    NativeFunction::GeneratorFunction,
                    "GeneratorFunction",
                    function_constructor,
                )
            })?;
            self.stack.push(Value::Object(constructor));
            self.define_data(
                constructor,
                "name",
                Value::String("GeneratorFunction".into()),
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
                Value::String("GeneratorFunction".into()),
                false,
                false,
                true,
            )
        })();
        self.stack.truncate(base);
        match result {
            Ok(()) => Ok((prototype, root)),
            Err(error) => {
                self.release_root(root);
                Err(error)
            }
        }
    }

    pub(in super::super) fn generator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if self.generator_prototype.is_none() {
            self.generator_intrinsics()?;
        }
        Ok(self
            .generator_prototype
            .expect("generator intrinsics install both prototypes"))
    }

    fn build_generator_prototype(
        &mut self,
    ) -> Result<(ObjectId, crate::heap::RootId), RuntimeError> {
        // The function prototype was built just before, by
        // `build_generator_function_prototype`.
        let constructor = self
            .string_intrinsics()
            .expect("the String intrinsics exist")
            .0;
        let function_prototype = self
            .heap
            .prototype(constructor)
            .expect("the String constructor is a live object")
            .unwrap();
        let base = self.base_iterator_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(base)))?;
        let root = self.root_new_object(prototype);
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
            Ok(())
        })();
        match result {
            Ok(()) => Ok((prototype, root)),
            Err(error) => {
                self.release_root(root);
                Err(error)
            }
        }
    }

    /// `%AsyncIteratorPrototype%` has no global binding. It is the common
    /// parent of async-generator iterator prototypes and supplies the
    /// `@@asyncIterator` identity method used by `for await` later on.
    pub(in super::super) fn async_iterator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.async_iterator_base {
            return Ok(prototype);
        }
        let object_prototype = self.object_prototype;
        let function_prototype = self.function_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.root_new_object(prototype);
        let result = self
            .install_symbol_native(
                prototype,
                function_prototype,
                "asyncIterator",
                0,
                NativeFunction::AsyncIteratorSelf,
            )
            .and_then(|()| {
                self.install_symbol_native(
                    prototype,
                    function_prototype,
                    "asyncDispose",
                    0,
                    NativeFunction::AsyncIteratorDispose,
                )
            });
        if let Err(error) = result {
            self.release_root(root);
            Err(error)
        } else {
            self.async_iterator_base = Some(prototype);
            Ok(prototype)
        }
    }

    /// The shared `%AsyncGeneratorPrototype%`. Individual async-generator
    /// functions receive their own `.prototype` object above this base, just
    /// like ordinary generator functions do.
    pub(in super::super) fn async_generator_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.async_generator_prototype {
            return Ok(prototype);
        }
        let base = self.async_iterator_prototype()?;
        let function_prototype = self
            .function_prototype()
            .expect("the String intrinsics exist");
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(base)))?;
        let root = self.root_new_object(prototype);
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
            self.release_root(root);
            Err(error)
        } else {
            self.async_generator_prototype = Some(prototype);
            Ok(prototype)
        }
    }

    /// `%AsyncGeneratorFunction.prototype%`, the prototype of async generator
    /// function objects (which is neither `%AsyncFunction.prototype%` nor
    /// `%GeneratorFunction.prototype%`). It is built together with
    /// `%AsyncGeneratorFunction%` and linked to `%AsyncGeneratorPrototype%`
    /// exactly as the synchronous pair is: each is the other's
    /// `prototype` / `constructor`, both non-writable and configurable.
    pub(in super::super) fn async_generator_function_prototype(
        &mut self,
    ) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.async_generator_function_prototype {
            return Ok(prototype);
        }
        let generator_side = self.async_generator_prototype()?;
        let function_prototype = self
            .function_prototype()
            .expect("the String intrinsics exist");
        let function_constructor = self
            .global("Function")?
            .object_id()
            .expect("Function is callable");
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(function_prototype)))?;
        let root = self.root_new_object(prototype);
        let base = self.stack.len();
        self.stack.push(Value::Object(prototype));
        let result: Result<(), RuntimeError> = (|| {
            let constructor = self.with_roots(|heap| {
                heap.alloc_native_function(
                    NativeFunction::AsyncGeneratorFunction,
                    "AsyncGeneratorFunction",
                    function_constructor,
                )
            })?;
            self.stack.push(Value::Object(constructor));
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
                "name",
                Value::String("AsyncGeneratorFunction".into()),
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
                "prototype",
                Value::Object(generator_side),
                false,
                false,
                true,
            )?;
            self.define_data(
                generator_side,
                "constructor",
                Value::Object(prototype),
                false,
                false,
                true,
            )?;
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("AsyncGeneratorFunction".into()),
                false,
                false,
                true,
            )
        })();
        self.stack.truncate(base);
        match result {
            Ok(()) => {
                self.async_generator_function_prototype = Some(prototype);
                Ok(prototype)
            }
            Err(error) => {
                self.release_root(root);
                Err(error)
            }
        }
    }

    /// Lazily creates `%AsyncFunction%` and `%AsyncFunction.prototype%`.
    ///
    /// Async function objects inherit from the latter, which in turn inherits
    /// from `%Function.prototype%`. `%AsyncFunction%` itself inherits from
    /// `%Function%`, is reachable through `AsyncFunction.prototype.constructor`,
    /// and intentionally has no global binding.
    pub(in super::super) fn async_function_prototype(&mut self) -> Result<ObjectId, RuntimeError> {
        if let Some(prototype) = self.async_function_prototype {
            return Ok(prototype);
        }
        let function_constructor = self
            .global("Function")?
            .object_id()
            .expect("Function is callable");
        // `global` built the String intrinsics before answering.
        let function_prototype = self
            .function_prototype()
            .expect("the String intrinsics exist");
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(function_prototype)))?;
        let root = self.root_new_object(prototype);
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
                self.release_root(root);
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::math::fault_injection::heap_limit_failures;
    use super::*;

    #[test]
    fn a_list_length_is_read_as_a_number_and_anything_else_as_zero() {
        assert_eq!(number_or_zero(&Value::Number(3.0)), 3.0);
        assert_eq!(number_or_zero(&Value::Undefined), 0.0);
    }

    /// Builds one lazily created intrinsic (twice: the second call finds it
    /// cached) on VMs that run out of heap at each allocation in turn, so the
    /// partly built intrinsic is released and the next attempt starts afresh.
    /// The first sweep fails inside the String intrinsics every builder starts
    /// from; the second starts after them and covers the builder itself.
    fn intrinsic_survives_every_allocation_failure(
        build: fn(&mut Vm) -> Result<ObjectId, RuntimeError>,
    ) {
        let mut operation = |vm: &mut Vm| {
            let first = build(vm)?;
            assert_eq!(build(vm), Ok(first));
            Ok(())
        };
        assert!(heap_limit_failures(&|_| Ok(()), 512, &mut operation) > 0);
        let strings = |vm: &mut Vm| vm.function_prototype().map(|_| ());
        heap_limit_failures(&strings, usize::MAX, &mut operation);
    }

    #[test]
    fn lazily_built_intrinsics_survive_running_out_of_heap() {
        intrinsic_survives_every_allocation_failure(Vm::throw_type_error);
        intrinsic_survives_every_allocation_failure(Vm::array_iterator_prototype);
        intrinsic_survives_every_allocation_failure(Vm::generator_function_prototype);
        intrinsic_survives_every_allocation_failure(Vm::generator_prototype);
        intrinsic_survives_every_allocation_failure(Vm::async_iterator_prototype);
        intrinsic_survives_every_allocation_failure(Vm::async_generator_prototype);
        intrinsic_survives_every_allocation_failure(Vm::async_generator_function_prototype);
        intrinsic_survives_every_allocation_failure(Vm::async_function_prototype);
        intrinsic_survives_every_allocation_failure(|vm| {
            vm.legacy_function_getters().map(|(caller, _)| caller)
        });
    }
}
