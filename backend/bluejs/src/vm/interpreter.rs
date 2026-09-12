// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(super) fn interpret(
        &mut self,
        code: &Bytecode,
        iterators: &mut Vec<Value>,
        start_pc: usize,
        resume_value: Option<Value>,
        suspend_at: Option<usize>,
        restored_handlers: Option<(Vec<HandlerFrame>, usize)>,
    ) -> Result<InterpreterExit, RuntimeError> {
        let stack_base = self.stack.len();
        let pending_base = self.pending_completions.len();
        let save_base = self.completion_saves.len();
        if let Some(value) = resume_value {
            self.stack.push(value);
        }
        let mut pc = start_pc;
        let (mut handlers, handler_stack_base) =
            restored_handlers.unwrap_or_else(|| (Vec::new(), stack_base));
        // Suspended frames store stack offsets relative to their saved stack,
        // while an active interpreter may have an ambient caller frame below
        // it.  Convert at the boundary so catch/finally cleanup always uses
        // the current absolute operand-stack offsets.
        for handler in &mut handlers {
            handler.stack_depth += handler_stack_base;
        }
        let mut suspended_await = None;
        loop {
            if suspend_at == Some(pc) {
                return Ok(InterpreterExit::Suspend { pc });
            }
            self.charge_step()?;
            let instruction = code
                .instruction(pc)
                .expect("compiler emits valid instruction boundaries");
            let operand = instruction.operand.unwrap_or(0) as usize;
            pc += instruction.opcode.width();
            let outcome: Result<Option<Completion>, RuntimeError> = (|| {
                match instruction.opcode {
                    Opcode::DefineData
                    | Opcode::DefineAccessor
                    | Opcode::DefineMethod
                    | Opcode::DefineClassAccessor => {
                        let value = self.pop();
                        let (receiver, key) = self.property_reference()?;
                        let object = receiver.object_id().unwrap();
                        if matches!(
                            instruction.opcode,
                            Opcode::DefineMethod
                                | Opcode::DefineAccessor
                                | Opcode::DefineClassAccessor
                        ) {
                            if let Value::Object(function) = value {
                                self.with_roots(|heap| heap.set_closure_home(function, object))?;
                            }
                        }
                        if instruction.opcode == Opcode::DefineData {
                            self.define_data(object, key, value.clone(), true, true, true)?;
                        } else {
                            let descriptor = if instruction.opcode == Opcode::DefineMethod {
                                PropertyDescriptor::data(value.clone(), true, operand != 0, true)
                            } else {
                                PropertyDescriptor {
                                    get: (operand == 0).then(|| value.clone()),
                                    set: (operand != 0).then(|| value.clone()),
                                    enumerable: Some(instruction.opcode == Opcode::DefineAccessor),
                                    configurable: Some(true),
                                    ..Default::default()
                                }
                            };
                            if !self.with_roots(|heap| {
                                heap.define_own_property(object, key, descriptor)
                            })? {
                                return Err(RuntimeError::TypeError(
                                    "cannot define class property".into(),
                                ));
                            }
                        }
                        self.stack.push(value);
                    }
                    Opcode::DefinePrivateField => {
                        let (receiver, name) = self.property_reference()?;
                        let Value::Object(owner) = receiver else {
                            unreachable!("private class owner is an object")
                        };
                        let PropertyName::String(name) = name else {
                            unreachable!("compiler emits string private names")
                        };
                        self.with_roots(|heap| {
                            if operand != 0 {
                                heap.add_private_brand(owner, owner)?;
                            }
                            heap.define_private_field(owner, name)
                        })?;
                    }
                    Opcode::DefinePrivateMethod | Opcode::DefinePrivateAccessor => {
                        let value = self.pop();
                        let (receiver, name) = self.property_reference()?;
                        let Value::Object(owner) = receiver else {
                            unreachable!("private class owner is an object")
                        };
                        let PropertyName::String(name) = name else {
                            unreachable!("compiler emits string private names")
                        };
                        if let Value::Object(function) = value {
                            self.with_roots(|heap| heap.set_closure_home(function, owner))?;
                        }
                        if instruction.opcode == Opcode::DefinePrivateMethod {
                            self.with_roots(|heap| {
                                if operand != 0 {
                                    heap.add_private_brand(owner, owner)?;
                                }
                                heap.define_private_method(owner, name, value)
                            })?;
                        } else {
                            self.with_roots(|heap| {
                                if operand & 2 != 0 {
                                    heap.add_private_brand(owner, owner)?;
                                }
                                heap.define_private_accessor(owner, name, value, operand & 1 != 0)
                            })?;
                        }
                    }
                    Opcode::DeleteProperty => {
                        let (receiver, key) = self.property_reference()?;
                        let deleted = match receiver {
                            Value::Object(id) => self.object_delete(id, &key)?,
                            Value::String(s) => {
                                !matches!(&key, PropertyName::String(key) if s.own_property(key).is_some())
                            }
                            _ => true,
                        };
                        if !deleted && self.strict {
                            return Err(RuntimeError::TypeError(
                                "cannot delete non-configurable property".into(),
                            ));
                        }
                        self.stack.push(Value::Bool(deleted));
                    }
                    Opcode::Throw => {
                        return Ok(Some(Completion::Throw(RuntimeError::Thrown(self.pop()))))
                    }
                    Opcode::InvalidAssignmentTarget => {
                        // Annex B CallExpression targets evaluate the call,
                        // then throw before an assignment RHS or update
                        // coercion can be observed.
                        self.pop();
                        return Err(RuntimeError::ReferenceError(
                            "invalid assignment target".into(),
                        ));
                    }
                    Opcode::ArrayPush => {
                        let base = self.stack.len() - 2;
                        self.array_push(
                            &self.stack[base].clone(),
                            &self.stack[base + 1].clone(),
                            operand,
                        )?;
                        self.stack.truncate(base + 1);
                    }
                    Opcode::CallSpread | Opcode::DirectEvalSpread => {
                        let base = self.stack.len() - 3;
                        let args = self.array_like_values(&self.stack[base + 2].clone())?;
                        let callee = self.stack[base].clone();
                        let receiver = self.stack[base + 1].clone();
                        let result = if instruction.opcode == Opcode::DirectEvalSpread
                            && self.is_intrinsic_eval(&callee)?
                        {
                            self.direct_eval(native::argument(&args, 0))?
                        } else {
                            self.call_native(callee, receiver, args, operand != 0)?
                        };
                        self.stack.truncate(base);
                        self.stack.push(result);
                    }
                    Opcode::CallClassStaticBlock => {
                        let base = self.stack.len() - 2;
                        let Value::Object(target) = self.stack[base].clone() else {
                            unreachable!("class constructors are objects")
                        };
                        if let Value::Object(function) = self.stack[base + 1] {
                            self.with_roots(|heap| heap.set_closure_home(function, target))?;
                        }
                        self.call_native(
                            self.stack[base + 1].clone(),
                            self.stack[base].clone(),
                            Vec::new(),
                            false,
                        )?;
                        self.stack.truncate(base + 1);
                    }
                    Opcode::DefineClassStaticField => {
                        let base = self.stack.len() - 4;
                        let target = self.stack[base + 1].clone();
                        let key = self.coerce_property_key(&self.stack[base + 2].clone())?;
                        let initializer = self.stack[base + 3].clone();
                        if let (Value::Object(target), Value::Object(function)) =
                            (&target, &initializer)
                        {
                            self.with_roots(|heap| heap.set_closure_home(*function, *target))?;
                        }
                        let value =
                            self.call_native(initializer, target.clone(), Vec::new(), false)?;
                        let Value::Object(target) = target else {
                            unreachable!("class fields target the constructor")
                        };
                        if !self.with_roots(|heap| {
                            heap.define_own_property(
                                target,
                                key,
                                PropertyDescriptor::data(value, true, true, true),
                            )
                        })? {
                            return Err(RuntimeError::TypeError(
                                "cannot define class field".into(),
                            ));
                        }
                        self.stack.truncate(base + 1);
                    }
                    Opcode::DefinePrivateStaticField => {
                        let base = self.stack.len() - 4;
                        let target = self.stack[base + 1].clone();
                        let name = match self.stack[base + 2].clone() {
                            Value::String(name) => name,
                            _ => unreachable!("compiler emits a private-name string"),
                        };
                        let initializer = self.stack[base + 3].clone();
                        if let (Value::Object(target), Value::Object(function)) =
                            (&target, &initializer)
                        {
                            self.with_roots(|heap| heap.set_closure_home(*function, *target))?;
                        }
                        let value =
                            self.call_native(initializer, target.clone(), Vec::new(), false)?;
                        let Value::Object(target) = target else {
                            unreachable!("class fields target the constructor")
                        };
                        self.with_roots(|heap| heap.set_private_slot(target, target, name, value))?;
                        self.stack.truncate(base + 1);
                    }
                    Opcode::SetClassHome => self.set_class_home()?,
                    Opcode::SetClassHeritage => self.set_class_heritage()?,
                    Opcode::InitializePrivateBrand => {
                        let owner = self
                            .binding_value(operand)?
                            .and_then(|value| value.object_id())
                            .ok_or_else(|| {
                                RuntimeError::TypeError(
                                    "private elements are not available in this function".into(),
                                )
                            })?;
                        let receiver = self.this.object_id().ok_or_else(|| {
                            RuntimeError::TypeError(
                                "private fields require an object receiver".into(),
                            )
                        })?;
                        self.with_roots(|heap| heap.add_private_brand(receiver, owner))?;
                    }
                    Opcode::PrivateGet | Opcode::PrivateGetMethod => {
                        let (receiver, owner, name) = self.private_reference(operand)?;
                        let value = self.private_get(&receiver, owner, &name)?;
                        self.stack.push(value);
                        if instruction.opcode == Opcode::PrivateGetMethod {
                            self.stack.push(receiver);
                        }
                    }
                    Opcode::PrivateSet => {
                        let value = self.pop();
                        let (receiver, owner, name) = self.private_reference(operand)?;
                        self.private_set(&receiver, owner, name, value.clone())?;
                        self.stack.push(value);
                    }
                    Opcode::PrivateIn => {
                        let (receiver, owner, _name) = self.private_reference(operand)?;
                        let object = receiver.object_id().ok_or_else(|| {
                            RuntimeError::TypeError("private brand checks require an object".into())
                        })?;
                        self.stack
                            .push(Value::Bool(self.heap.has_private_brand(object, owner)?));
                    }
                    Opcode::SuperGet | Opcode::SuperGetMethod => {
                        let key_value = self.pop();
                        let key = self.coerce_property_key(&key_value)?;
                        let value = self.super_get(&key)?;
                        self.check_string(&value)?;
                        self.stack.push(value);
                        if instruction.opcode == Opcode::SuperGetMethod {
                            self.stack.push(self.this.clone());
                        }
                    }
                    Opcode::SuperSet => {
                        let value = self.pop();
                        let key_value = self.pop();
                        let key = self.coerce_property_key(&key_value)?;
                        self.super_set(&key, &value)?;
                        self.stack.push(value);
                    }
                    Opcode::SuperUpdate => {
                        let key_value = self.pop();
                        let key = self.coerce_property_key(&key_value)?;
                        let old_value = self.super_get(&key)?;
                        let old = self.coerce_number(&old_value)?;
                        let new = if operand & 1 == 0 {
                            old + 1.0
                        } else {
                            old - 1.0
                        };
                        self.super_set(&key, &Value::Number(new))?;
                        self.stack
                            .push(Value::Number(if operand & 2 == 0 { old } else { new }));
                    }
                    Opcode::SuperCall | Opcode::SuperCallSpread | Opcode::SuperCallForward => {
                        let args = if instruction.opcode == Opcode::SuperCall {
                            let base = self.stack.len() - operand;
                            let args = self.stack[base..].to_vec();
                            self.stack.truncate(base);
                            args
                        } else if instruction.opcode == Opcode::SuperCallSpread {
                            let arguments = self.pop();
                            self.array_like_values(&arguments)?
                        } else {
                            self.arguments.clone()
                        };
                        let value = self.super_call(args)?;
                        self.stack.push(value);
                    }
                    Opcode::EnterClassFieldInitializer => self.class_field_initializer_depth += 1,
                    Opcode::LeaveClassFieldInitializer => {
                        self.class_field_initializer_depth = self
                            .class_field_initializer_depth
                            .checked_sub(1)
                            .expect("compiler balances class field initializers");
                    }
                    Opcode::RegExpLiteral => {
                        let base = self.stack.len() - 2;
                        let regexp = self.regexp_create(
                            &self.stack[base].clone(),
                            &self.stack[base + 1].clone(),
                        )?;
                        self.stack.truncate(base);
                        self.stack.push(regexp);
                    }
                    Opcode::TemplateObject => {
                        let object = self.template_object(&code.templates[operand])?;
                        self.stack.push(object);
                    }
                    Opcode::GetIterator => {
                        let value = self.stack.last().unwrap().clone();
                        let iterator = self.get_iterator(&value)?;
                        self.pop();
                        self.stack.push(iterator.clone());
                        iterators.push(iterator);
                    }
                    Opcode::IteratorNext => {
                        let (record, argument) = if operand == 0 {
                            (self.stack.last().unwrap().clone(), None)
                        } else {
                            let base = self.stack.len() - 2;
                            let record = self.stack[base].clone();
                            let argument = self.stack[base + 1].clone();
                            self.stack.truncate(base + 1);
                            (record, Some(argument))
                        };
                        let result = self.iterator_next(&record, argument)?;
                        self.stack.push(result);
                    }
                    Opcode::IteratorStepValue => {
                        // Synchronous yield* needs the completed iterator
                        // result's value as its own expression result.
                        let base = self.stack.len() - 2;
                        let record = self.stack[base].clone();
                        let result = self.stack[base + 1].clone();
                        iterators.retain(|active| active != &record);
                        let Value::Object(record_id) = record else {
                            unreachable!("compiler only emits iterator records")
                        };
                        if !matches!(result, Value::Object(_)) {
                            return Err(RuntimeError::TypeError(
                                "iterator result must be an object".into(),
                            ));
                        }
                        let done = self.get_property(&result, &"done".into())?;
                        let value = self.get_property(&result, &"value".into())?;
                        self.stack.truncate(base);
                        if self.to_boolean(&done)? {
                            self.with_roots(|heap| heap.set(record_id, "done", Value::Bool(true)))?;
                            self.stack.push(value);
                            pc = operand;
                        } else {
                            iterators.push(Value::Object(record_id));
                            self.stack.push(Value::Object(record_id));
                            self.stack.push(value);
                        }
                    }
                    Opcode::GetAsyncIterator => {
                        let value = self.stack.last().unwrap().clone();
                        let iterator = self.get_async_iterator(&value)?;
                        self.pop();
                        self.stack.push(iterator.clone());
                        iterators.push(iterator);
                    }
                    Opcode::ForInKeys => {
                        let source = self.stack.last().expect("for-in has a source").clone();
                        let keys = self.for_in_keys(&source)?;
                        self.pop();
                        self.stack.push(keys);
                    }
                    Opcode::IteratorStep => {
                        let record = self.stack.last().unwrap().clone();
                        iterators.retain(|active| active != &record);
                        let result = self.iterator_step(&record, true)?;
                        self.pop();
                        if let Some(value) = result {
                            iterators.push(record);
                            self.stack.push(value);
                        } else {
                            pc = operand;
                        }
                    }
                    Opcode::AsyncIteratorNext => {
                        let (record, argument) = if operand == 0 {
                            (self.stack.last().unwrap().clone(), None)
                        } else {
                            let base = self.stack.len() - 2;
                            let record = self.stack[base].clone();
                            let argument = self.stack[base + 1].clone();
                            self.stack.truncate(base + 1);
                            (record, Some(argument))
                        };
                        let promise = self.async_iterator_next(&record, argument)?;
                        self.stack.push(promise);
                    }
                    Opcode::AsyncIteratorStep => {
                        let base = self.stack.len() - 2;
                        let record = self.stack[base].clone();
                        let result = self.stack[base + 1].clone();
                        iterators.retain(|active| active != &record);
                        let value = self.async_iterator_step(&record, &result)?;
                        self.stack.truncate(base);
                        if let Some(value) = value {
                            iterators.push(record);
                            self.stack.push(value);
                        } else {
                            pc = operand;
                        }
                    }
                    Opcode::AsyncIteratorStepValue => {
                        // yield* needs the completed iterator result's value
                        // as its own expression result, unlike for-await.
                        let base = self.stack.len() - 2;
                        let record = self.stack[base].clone();
                        let result = self.stack[base + 1].clone();
                        iterators.retain(|active| active != &record);
                        let Value::Object(record_id) = record else {
                            unreachable!("compiler only emits iterator records")
                        };
                        if !matches!(result, Value::Object(_)) {
                            return Err(RuntimeError::TypeError(
                                "async iterator result must be an object".into(),
                            ));
                        }
                        let done = self.get_property(&result, &"done".into())?;
                        let value = self.get_property(&result, &"value".into())?;
                        self.stack.truncate(base);
                        if self.to_boolean(&done)? {
                            self.with_roots(|heap| heap.set(record_id, "done", Value::Bool(true)))?;
                            self.stack.push(value);
                            pc = operand;
                        } else {
                            iterators.push(Value::Object(record_id));
                            self.stack.push(Value::Object(record_id));
                            self.stack.push(value);
                        }
                    }
                    Opcode::IteratorStepReference => {
                        let base = self.stack.len() - 3;
                        let record = self.stack[base].clone();
                        iterators.retain(|active| active != &record);
                        let result = self.iterator_step(&record, true)?;
                        self.stack.remove(base);
                        if let Some(value) = result {
                            iterators.push(record);
                            self.stack.push(value);
                        } else {
                            pc = operand;
                        }
                    }
                    Opcode::IteratorElision => {
                        let record = self.stack.last().unwrap().clone();
                        iterators.retain(|active| active != &record);
                        if self.iterator_step(&record, false)?.is_some() {
                            iterators.push(record);
                        }
                    }
                    Opcode::IteratorClose => {
                        let record = self.stack.last().unwrap().clone();
                        iterators.retain(|active| active != &record);
                        self.iterator_close(&record)?;
                        self.pop();
                    }
                    Opcode::IteratorFinish => {
                        let record = self.stack.last().unwrap().clone();
                        let Value::Object(id) = record else {
                            unreachable!("compiler only emits iterator records")
                        };
                        let done =
                            matches!(self.heap.get_own(id, "done")?, Some(Value::Bool(true)));
                        iterators.retain(|candidate| candidate != &record);
                        if !done {
                            self.iterator_close(&record)?;
                        }
                        self.pop();
                    }
                    Opcode::IteratorRest => {
                        let record = self.stack.last().unwrap().clone();
                        let base = self.stack.len() - 1;
                        iterators.retain(|candidate| candidate != &record);
                        iterators.push(record.clone());
                        // Keep accumulated values in a rooted managed array while
                        // subsequent next/done/value callbacks can trigger GC.
                        let array = self.array_from(Vec::new())?;
                        self.stack.push(array.clone());
                        while let Some(value) = self.iterator_step(&record, true)? {
                            self.charge_step()?;
                            self.array_push(&array, &value, 0)?;
                        }
                        iterators.retain(|candidate| candidate != &record);
                        self.stack.truncate(base);
                        self.stack.push(array);
                    }
                    Opcode::IteratorRestReference => {
                        let base = self.stack.len() - 3;
                        let record = self.stack[base].clone();
                        iterators.retain(|active| active != &record);
                        iterators.push(record.clone());
                        let array = self.array_from(Vec::new())?;
                        self.stack.push(array.clone());
                        while let Some(value) = self.iterator_step(&record, true)? {
                            self.charge_step()?;
                            self.array_push(&array, &value, 0)?;
                        }
                        iterators.retain(|active| active != &record);
                        self.stack.remove(base);
                    }
                    Opcode::RequireObject => {
                        let value = self.stack.last().unwrap().clone();
                        self.coerce_object(&value)?;
                    }
                    Opcode::DestructureProperty => {
                        let base = self.stack.len() - 3;
                        let source = self.stack[base].clone();
                        let excluded = self.stack[base + 1].clone();
                        let key = self.coerce_property_key(&self.stack[base + 2].clone())?;
                        let value = self.get_property(&source, &key)?;
                        self.array_push(&excluded, &key.value(), 0)?;
                        self.stack.truncate(base);
                        self.stack.extend([source, excluded, value]);
                    }
                    Opcode::DestructurePropertyReference => {
                        // [..., source, excluded, source-key, source-key,
                        // target-object, raw-target-key] becomes [...,
                        // source, excluded, target-object, raw-target-key,
                        // value].
                        // The duplicate source key keeps target evaluation
                        // ahead of GetV without changing excluded-key order.
                        let base = self.stack.len() - 6;
                        let source = self.stack[base].clone();
                        let excluded = self.stack[base + 1].clone();
                        let source_key = self.coerce_property_key(&self.stack[base + 2].clone())?;
                        let object = self.stack[base + 4].clone();
                        let raw_target_key = self.stack[base + 5].clone();
                        let value = self.get_property(&source, &source_key)?;
                        self.array_push(&excluded, &source_key.value(), 0)?;
                        self.stack.truncate(base);
                        self.stack
                            .extend([source, excluded, object, raw_target_key, value]);
                    }
                    Opcode::ObjectRest => {
                        let base = self.stack.len() - 2;
                        let source = self.stack[base].clone();
                        let excluded = self.stack[base + 1].clone();
                        let rest = self.destructure_object_rest(&source, &excluded)?;
                        self.stack.truncate(base);
                        self.stack.push(rest);
                    }
                    Opcode::CopyDataProperties => {
                        let base = self.stack.len() - 2;
                        let Value::Object(target) = self.stack[base].clone() else {
                            unreachable!("compiler creates an object literal target")
                        };
                        let source = self.stack[base + 1].clone();
                        self.copy_data_properties(target, &source, &[])?;
                        self.pop();
                    }
                    Opcode::Closure => {
                        let child = code.functions[operand].clone();
                        let function_prototype = if child.async_function {
                            self.async_function_prototype()?
                        } else {
                            self.function_prototype()?
                        };
                        let mut captures = Vec::new();
                        for &slot in &child.captures {
                            captures.push(self.capture(slot as usize)?);
                        }
                        let this = if child.arrow {
                            if self.this == Value::Undefined
                                && self.call_depth == 0
                                && !self.top_level_module
                            {
                                self.global("globalThis")?
                            } else {
                                self.this.clone()
                            }
                        } else {
                            Value::Undefined
                        };
                        let id = self.with_roots(|heap| {
                            heap.alloc_closure(child.clone(), captures, this, function_prototype)
                        })?;
                        if let Some(module) = &self.active_module_name {
                            self.module_closure_referrers.insert(id, module.clone());
                        }
                        self.stack.push(Value::Object(id));
                        // Arrow functions inherit their containing function's
                        // [[HomeObject]] together with lexical `this`.  Keeping
                        // the new closure on the operand stack first makes it a
                        // GC root while installing metadata may allocate.
                        if child.arrow {
                            if let Some(home) = self.home_object {
                                self.with_roots(|heap| heap.set_closure_home(id, home))?;
                            }
                            // A derived constructor's arrow may invoke `super()`.
                            // Store its resolved superclass on the arrow closure;
                            // the call frame then treats that closure as the
                            // lexical derived-constructor context.
                            if let Some(constructor) = self.class_constructor {
                                if let Some(base) = self.heap.class_base(constructor)? {
                                    self.with_roots(|heap| heap.set_class_base(id, base))?;
                                }
                            }
                        }
                        self.define_data(
                            id,
                            "name",
                            Value::String(child.function_name.clone().into()),
                            false,
                            false,
                            true,
                        )?;
                        self.define_data(
                            id,
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
                            self.install_legacy_function_properties(id)?;
                        }
                        if child.generator {
                            // Generator function objects are not constructors,
                            // but each owns the prototype used for iterators it
                            // creates. The own object inherits the shared
                            // generator/async-generator prototype and remains
                            // replaceable by user code.
                            let base_prototype = if child.async_function {
                                self.async_generator_prototype()?
                            } else {
                                self.generator_prototype()?
                            };
                            let prototype =
                                self.with_roots(|heap| heap.alloc_object(Some(base_prototype)))?;
                            self.define_data(
                                id,
                                "prototype",
                                Value::Object(prototype),
                                true,
                                false,
                                false,
                            )?;
                        } else if child.constructible {
                            let object_prototype = self.object_prototype;
                            let prototype =
                                self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                            self.define_data(
                                id,
                                "prototype",
                                Value::Object(prototype),
                                !child.class_constructor,
                                false,
                                false,
                            )?;
                            self.define_data(
                                prototype,
                                "constructor",
                                Value::Object(id),
                                true,
                                false,
                                true,
                            )?;
                        }
                    }
                    Opcode::This => {
                        if self.this == Value::Undefined
                            && self.class_constructor.is_some_and(|constructor| {
                                self.heap.class_base(constructor).ok().flatten().is_some()
                            })
                        {
                            return Err(RuntimeError::ReferenceError(
                                "this is uninitialized before super()".into(),
                            ));
                        }
                        if self.this == Value::Undefined
                            && self.call_depth == 0
                            && !self.top_level_module
                        {
                            self.this = self.global("globalThis")?;
                        }
                        self.stack.push(self.this.clone());
                    }
                    Opcode::NewTarget => self.stack.push(self.new_target.clone()),
                    Opcode::Argument => self
                        .stack
                        .push(native::argument(&self.arguments, operand).clone()),
                    Opcode::RestArguments => {
                        let array = self
                            .array_from(self.arguments.iter().skip(operand).cloned().collect())?;
                        self.stack.push(array);
                    }
                    Opcode::ArgumentsObject => self.create_arguments_object(code)?,
                    Opcode::Return => return Ok(Some(Completion::Return(self.pop()))),
                    Opcode::TailRecur => {
                        let base = self.stack.len() - operand;
                        let args = self.stack[base..].to_vec();
                        self.stack.truncate(base);
                        return Ok(Some(Completion::TailRecur(args)));
                    }
                    Opcode::Yield => {
                        if !code.generator {
                            return Err(RuntimeError::TypeError(
                                "yield is not supported in this execution context".into(),
                            ));
                        }
                        return Ok(Some(Completion::Yield(self.pop())));
                    }
                    Opcode::Await => {
                        // Await always applies PromiseResolve before it
                        // observes settlement, so plain thenables use the
                        // same queued resolution path as Promise values.
                        let awaited_value = self.pop();
                        let awaited = self.promise_resolve(awaited_value)?;
                        if code.async_function || (self.top_level_module && self.call_depth == 0) {
                            suspended_await = Some(
                                awaited
                                    .object_id()
                                    .expect("PromiseResolve returns a Promise object"),
                            );
                            return Ok(None);
                        }
                        // Await resumes only after the next queued turn. A
                        // fulfilled Promise still crosses that boundary; a
                        // pending one may need several pre-existing jobs to
                        // settle, but we never collapse later turns into the
                        // current continuation.
                        loop {
                            let pending = awaited
                                .object_id()
                                .and_then(|promise| self.promises.get(&promise))
                                .is_some_and(|record| {
                                    matches!(record.status, PromiseStatus::Pending)
                                });
                            let advanced = self.run_next_job_while_module_suspended()?;
                            if !pending || !advanced {
                                break;
                            }
                        }
                        let value = self.await_value(awaited)?;
                        self.stack.push(value);
                    }
                    Opcode::DynamicImport => {
                        let specifier = self.pop();
                        let promise = self.dynamic_import(specifier)?;
                        self.stack.push(promise);
                    }
                    Opcode::ImportMeta => {
                        let meta = self.import_meta()?;
                        self.stack.push(meta);
                    }
                    Opcode::EnterWith => {
                        let object = self.pop();
                        let object = self.coerce_object(&object)?;
                        self.with_objects.push(Value::Object(object));
                    }
                    Opcode::LeaveWith => {
                        self.with_objects
                            .pop()
                            .expect("compiler balances with scopes");
                    }
                    Opcode::WithGet => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        let name = name.to_utf8().expect("compiler emits a UTF-8 identifier");
                        let fallback = code
                            .bindings
                            .iter()
                            // Captures precede function-local bindings in
                            // bytecode. An object-environment miss therefore
                            // resolves the innermost matching slot.
                            .rposition(|binding| binding.name == name)
                            .map(|slot| self.eval_aware_binding_value(slot, &name))
                            .transpose()?;
                        let value = self.with_get(&name, fallback)?;
                        self.stack.push(value);
                    }
                    Opcode::WithSet => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        self.with_set(
                            &name.to_utf8().expect("compiler emits a UTF-8 identifier"),
                            self.stack.last().expect("assignment has a value").clone(),
                        )?;
                    }
                    Opcode::ResolveWithReference => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        let name = name.to_utf8().expect("compiler emits a UTF-8 identifier");
                        let mut object_reference = None;
                        for object in self.with_objects.clone().into_iter().rev() {
                            if self.with_has_binding(&object, &name)? {
                                object_reference = Some(object);
                                break;
                            }
                        }
                        if let Some(object) = object_reference {
                            // The pair is a property reference. Keeping it on
                            // the operand stack makes it survive calls, handler
                            // unwinding and generator suspension just like an
                            // ordinary member-reference pair.
                            self.stack.push(object);
                            self.stack.push(Value::String(name.into()));
                        } else if let Some(slot) = code
                            .bindings
                            .iter()
                            .rposition(|binding| binding.name == name)
                        {
                            // `Null` tags an internal binding reference; the
                            // slot is safe because it is compiler-owned and is
                            // consumed only by StoreWithReference.
                            self.stack.push(Value::Number(slot as f64));
                            self.stack.push(Value::Null);
                        } else {
                            // `Undefined` tags an unresolvable reference and
                            // preserves its source name for sloppy PutValue.
                            self.stack.push(Value::Undefined);
                            self.stack.push(Value::String(name.into()));
                        }
                    }
                    Opcode::LoadWithReference => {
                        let marker = self.pop();
                        let target = self.pop();
                        let value = match (&target, &marker) {
                            (Value::Object(object), Value::String(name)) => {
                                self.get_property(&Value::Object(*object), &name.clone().into())?
                            }
                            (Value::Number(slot), Value::Null)
                                if slot.is_finite()
                                    && *slot >= 0.0
                                    && slot.fract() == 0.0
                                    && (*slot as usize) < code.bindings.len() =>
                            {
                                let slot = *slot as usize;
                                self.eval_aware_binding_value(slot, &code.bindings[slot].name)?
                                    .ok_or_else(|| {
                                        RuntimeError::ReferenceError(
                                            code.bindings[slot].name.clone(),
                                        )
                                    })?
                            }
                            (Value::Undefined, Value::String(name)) => {
                                return Err(RuntimeError::ReferenceError(
                                    name.to_utf8().expect("compiler emits a UTF-8 identifier"),
                                ));
                            }
                            _ => unreachable!("compiler emits a valid with reference"),
                        };
                        // Preserve the original Reference for PutValue after
                        // the RHS has run. `get_property` may invoke a getter,
                        // so stack-resident values are the GC roots here.
                        self.stack.push(target);
                        self.stack.push(marker);
                        self.stack.push(value);
                    }
                    Opcode::StoreWithReference => {
                        let value = self.pop();
                        let marker = self.pop();
                        let target = self.pop();
                        match (target, marker) {
                            (Value::Object(object), Value::String(name)) => {
                                self.set_property(&Value::Object(object), &name.into(), &value)?;
                            }
                            (Value::Number(slot), Value::Null)
                                if slot.is_finite()
                                    && slot >= 0.0
                                    && slot.fract() == 0.0
                                    && (slot as usize) < code.bindings.len() =>
                            {
                                let slot = slot as usize;
                                let name = &code.bindings[slot].name;
                                if !self.store_dynamic_eval_shadowing_binding(
                                    slot,
                                    name,
                                    value.clone(),
                                )? {
                                    if self.binding_value(slot)?.is_none() {
                                        return Err(RuntimeError::ReferenceError(name.clone()));
                                    }
                                    if binding_allows_assignment(&code.bindings[slot], code.strict)?
                                    {
                                        self.store_binding(slot, value.clone())?;
                                    }
                                }
                            }
                            (Value::Undefined, Value::String(name)) => {
                                let name =
                                    name.to_utf8().expect("compiler emits a UTF-8 identifier");
                                if !self.set_dynamic_eval_binding(&name, value.clone())?
                                    && !self.set_global_binding(&name, value.clone())?
                                {
                                    let global = self.global("globalThis")?;
                                    self.set_property(&global, &name.into(), &value)?;
                                }
                            }
                            _ => unreachable!("compiler emits a valid with reference"),
                        }
                        self.stack.push(value);
                    }
                    Opcode::Global => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!()
                        };
                        let value = self.global(&name.to_utf8().unwrap())?;
                        self.stack.push(value);
                    }
                    Opcode::ToPropertyKey => {
                        let value = self.stack.last().unwrap().clone();
                        let key = self.coerce_property_key(&value)?;
                        self.pop();
                        self.stack.push(key.value());
                    }
                    Opcode::PreparePropertyReference => {
                        let base = self.stack.len() - 2;
                        if matches!(self.stack[base], Value::Null | Value::Undefined) {
                            return Err(RuntimeError::TypeError(
                                "cannot access a property of null or undefined".into(),
                            ));
                        }
                        let key = self.coerce_property_key(&self.stack[base + 1].clone())?;
                        self.stack[base + 1] = key.value();
                    }
                    Opcode::Constant => {
                        self.check_string(&code.constants[operand])?;
                        self.stack.push(code.constants[operand].clone());
                    }
                    Opcode::GlobalString => {
                        let (constructor, _) = self.string_intrinsics()?;
                        self.stack.push(Value::Object(constructor));
                    }
                    Opcode::GetBinding => {
                        let value = self
                            .eval_aware_binding_value(operand, &code.bindings[operand].name)?
                            .ok_or_else(|| {
                                RuntimeError::ReferenceError(code.bindings[operand].name.clone())
                            })?;
                        self.stack.push(value);
                    }
                    Opcode::ResolveBindingReference => {
                        let slot = operand;
                        let dynamic = self.cells.get(&slot).and_then(|cell| {
                            self.dynamic_eval_shadowing_cell(&code.bindings[slot].name, *cell)
                        });
                        // `Number(slot), Null` is a static binding reference;
                        // replacing Null with a cell records a dynamic eval
                        // binding that was already visible at resolution.
                        self.stack.push(Value::Number(slot as f64));
                        self.stack
                            .push(dynamic.map(Value::Object).unwrap_or(Value::Null));
                    }
                    Opcode::LoadBindingReference => {
                        let marker = self.pop();
                        let target = self.pop();
                        let Value::Number(slot) = target else {
                            panic!("compiler emits a binding reference: target={target:?}, marker={marker:?}")
                        };
                        let slot = slot as usize;
                        let value = match marker {
                            Value::Null => self.binding_value(slot)?,
                            Value::Object(cell) => self.heap.get_own(cell, "value")?,
                            _ => unreachable!("compiler emits a binding reference marker"),
                        }
                        .ok_or_else(|| {
                            RuntimeError::ReferenceError(code.bindings[slot].name.clone())
                        })?;
                        self.stack.push(Value::Number(slot as f64));
                        self.stack.push(marker);
                        self.stack.push(value);
                    }
                    Opcode::InitializeBinding => {
                        let value = self.pop();
                        self.store_binding(operand, value)?;
                    }
                    Opcode::StoreBinding => {
                        // ECMA-262 §9.1.1.1.5: TDZ takes precedence over the
                        // immutable-binding assignment error, including const.
                        let name = &code.bindings[operand].name;
                        let value = self.stack.last().expect("store has a value").clone();
                        if !self.store_dynamic_eval_shadowing_binding(operand, name, value)? {
                            if self.binding_value(operand)?.is_none() {
                                return Err(RuntimeError::ReferenceError(name.clone()));
                            }
                            if binding_allows_assignment(&code.bindings[operand], code.strict)? {
                                self.store_binding(
                                    operand,
                                    self.stack.last().expect("store has a value").clone(),
                                )?;
                            }
                        }
                    }
                    Opcode::StoreBindingReference => {
                        let (target, marker, value, result) = if operand == 0 {
                            let value = self.pop();
                            let marker = self.pop();
                            let target = self.pop();
                            (target, marker, value.clone(), value)
                        } else {
                            // A postfix update leaves its original value
                            // below the reference's new value.
                            let base = self.stack.len() - 4;
                            let target = self.stack[base].clone();
                            let marker = self.stack[base + 1].clone();
                            let result = self.stack[base + 2].clone();
                            let value = self.stack[base + 3].clone();
                            self.stack.truncate(base);
                            (target, marker, value, result)
                        };
                        let Value::Number(slot) = target else {
                            unreachable!("compiler emits a binding reference")
                        };
                        let slot = slot as usize;
                        match marker {
                            Value::Null => {
                                if self.binding_value(slot)?.is_none() {
                                    return Err(RuntimeError::ReferenceError(
                                        code.bindings[slot].name.clone(),
                                    ));
                                }
                                if binding_allows_assignment(&code.bindings[slot], code.strict)? {
                                    self.store_binding(slot, value.clone())?;
                                }
                            }
                            Value::Object(cell) => self.store_global_cell(cell, value.clone())?,
                            _ => unreachable!("compiler emits a binding reference marker"),
                        }
                        if operand != 0 {
                            // The compiler removes the new value first and
                            // leaves the old value as the postfix result.
                            self.stack.push(result);
                            self.stack.push(value);
                        } else {
                            self.stack.push(result);
                        }
                    }
                    Opcode::UnboundName | Opcode::TypeofName => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        let name = name.to_utf8().expect("compiler emits a UTF-8 identifier");
                        let value = self.lookup_global_name(&name)?;
                        if instruction.opcode == Opcode::TypeofName {
                            let value = value.unwrap_or(Value::Undefined);
                            self.stack
                                .push(Value::String(self.typeof_value(&value)?.into()));
                        } else {
                            self.stack
                                .push(value.ok_or(RuntimeError::ReferenceError(name))?);
                        }
                    }
                    Opcode::SetUnboundName => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        let name = name.to_utf8().expect("compiler emits a UTF-8 identifier");
                        let value = self.stack.last().expect("assignment has a value").clone();
                        if !self.set_dynamic_eval_binding(&name, value.clone())?
                            && !self.set_global_binding(&name, value.clone())?
                        {
                            let global = self.global("globalThis")?;
                            let global_id = global.object_id().expect("globalThis is an object");
                            let key: PropertyName = name.as_str().into();
                            if code.strict && !self.has_property(global_id, &key)? {
                                return Err(RuntimeError::ReferenceError(name));
                            }
                            self.set_property(&global, &key, &value)?;
                        }
                    }
                    Opcode::DeleteUnboundName => {
                        let Value::String(name) = &code.constants[operand] else {
                            unreachable!("compiler emits a name")
                        };
                        let name = name.to_utf8().expect("compiler emits a UTF-8 identifier");
                        let deleted = self.delete_dynamic_eval_binding(&name)?;
                        self.stack.push(Value::Bool(deleted));
                    }
                    Opcode::DeleteDynamicBinding => {
                        let name = &code.bindings[operand].name;
                        let deleted = self.delete_dynamic_eval_binding(name)?;
                        self.stack.push(Value::Bool(deleted));
                    }
                    Opcode::EnterScope => {
                        for slot in &code.scopes[operand] {
                            if let Some(name) = self.eval_dynamic_slots.get(&(*slot as usize)) {
                                let cell = self
                                    .dynamic_eval_bindings
                                    .get(name)
                                    .expect("prepared dynamic eval binding survives execution")
                                    .cell;
                                self.cells.insert(*slot as usize, cell);
                                continue;
                            }
                            if let Some(name) = self.script_global_slots.get(&(*slot as usize)) {
                                let cell = self
                                    .global_bindings
                                    .get(name)
                                    .expect("prepared global binding survives script execution")
                                    .cell;
                                self.cells.insert(*slot as usize, cell);
                                continue;
                            }
                            if code.module
                                && operand == 0
                                && self.cells.contains_key(&(*slot as usize))
                            {
                                // ModuleDeclarationInstantiation seeded this
                                // slot (or linked it to an exporter cell).
                                continue;
                            }
                            self.cells.remove(&(*slot as usize));
                            self.bindings[*slot as usize] =
                                if !code.bindings[*slot as usize].lexical {
                                    Some(Value::Undefined)
                                } else {
                                    None
                                };
                        }
                        self.active_scopes.push(operand as u32);
                        self.active_scope_slots.push(code.scopes[operand].clone());
                    }
                    Opcode::CloneScope => self.clone_scope(code, operand as u32)?,
                    Opcode::LeaveScope => self.leave_scope(code, operand as u32),
                    Opcode::Pop => {
                        self.pop();
                    }
                    Opcode::Dup => self
                        .stack
                        .push(self.stack.last().expect("dup has a value").clone()),
                    Opcode::Dup2 => {
                        let index = self.stack.len() - 2;
                        self.stack.push(self.stack[index].clone());
                        self.stack.push(self.stack[index + 1].clone());
                    }
                    Opcode::Swap => {
                        let index = self.stack.len() - 2;
                        self.stack.swap(index, index + 1);
                    }
                    Opcode::Add => self.binary(Self::add)?,
                    Opcode::Subtract => self.numeric(|a, b| a - b)?,
                    Opcode::Multiply => self.numeric(|a, b| a * b)?,
                    Opcode::Exponentiate => self.exponentiate()?,
                    Opcode::Divide => self.numeric(|a, b| a / b)?,
                    Opcode::Remainder => self.numeric(|a, b| a % b)?,
                    Opcode::ShiftLeft | Opcode::ShiftRight | Opcode::UnsignedShiftRight => {
                        self.shift(instruction.opcode)?
                    }
                    Opcode::BitAnd | Opcode::BitXor | Opcode::BitOr => {
                        self.bitwise(instruction.opcode)?
                    }
                    Opcode::StrictEqual => self.binary(|_, a, b| Ok(Value::Bool(a == b)))?,
                    Opcode::StrictNotEqual => self.binary(|_, a, b| Ok(Value::Bool(a != b)))?,
                    Opcode::Equal => {
                        self.binary(|vm, a, b| vm.loose_equal(a, b).map(Value::Bool))?
                    }
                    Opcode::NotEqual => self
                        .binary(|vm, a, b| vm.loose_equal(a, b).map(|equal| Value::Bool(!equal)))?,
                    Opcode::Instanceof => self.binary(|vm, value, target| {
                        vm.has_instance(value, target, false).map(Value::Bool)
                    })?,
                    Opcode::In => self
                        .binary(|vm, key, object| vm.property_in(&key, &object).map(Value::Bool))?,
                    Opcode::Less => self.relational(|order| order == Ordering::Less)?,
                    Opcode::Greater => self.relational(|order| order == Ordering::Greater)?,
                    Opcode::LessEqual => self.relational(|order| order != Ordering::Greater)?,
                    Opcode::GreaterEqual => self.relational(|order| order != Ordering::Less)?,
                    Opcode::Negate
                    | Opcode::BitNot
                    | Opcode::ToNumber
                    | Opcode::ToString
                    | Opcode::Not
                    | Opcode::Typeof => {
                        let arg = self.stack.last().unwrap().clone();
                        let value = match instruction.opcode {
                            Opcode::Negate => self.negate(&arg)?,
                            Opcode::BitNot => self.bit_not(&arg)?,
                            Opcode::ToNumber => Value::Number(self.coerce_number(&arg)?),
                            Opcode::ToString => Value::String(self.coerce_string(&arg)?),
                            Opcode::Not => Value::Bool(!self.to_boolean(&arg)?),
                            _ => Value::String(self.typeof_value(&arg)?.into()),
                        };
                        self.check_string(&value)?;
                        self.pop();
                        self.stack.push(value);
                    }
                    Opcode::Jump => pc = operand,
                    Opcode::JumpIfFalse | Opcode::JumpIfTrue | Opcode::JumpIfNotNullish => {
                        let arg = self.pop();
                        let take = match instruction.opcode {
                            Opcode::JumpIfFalse => !self.to_boolean(&arg)?,
                            Opcode::JumpIfTrue => self.to_boolean(&arg)?,
                            _ => !matches!(arg, Value::Null | Value::Undefined),
                        };
                        if take {
                            pc = operand;
                        }
                    }
                    Opcode::NewObject => {
                        let prototype = self.object_prototype;
                        let id = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                        self.stack.push(Value::Object(id));
                    }
                    Opcode::NewArray => {
                        let prototype = self.array_prototype;
                        let id = self
                            .with_roots(|heap| heap.alloc_array(operand as u32, Some(prototype)))?;
                        self.stack.push(Value::Object(id));
                    }
                    Opcode::GetProperty | Opcode::GetMethod => {
                        let (receiver, key) = self.property_reference()?;
                        let value = self.get_property(&receiver, &key)?;
                        self.check_string(&value)?;
                        self.stack.push(value);
                        if instruction.opcode == Opcode::GetMethod {
                            self.stack.push(receiver);
                        }
                    }
                    Opcode::Call | Opcode::DirectEval | Opcode::Construct => {
                        // Leave every call input on the stack until dispatch
                        // completes, so native allocations see all GC roots.
                        let base = self.stack.len() - operand - 2;
                        let callee = self.stack[base].clone();
                        let receiver = self.stack[base + 1].clone();
                        let args = self.stack[base + 2..].to_vec();
                        let result = if instruction.opcode == Opcode::DirectEval
                            && self.is_intrinsic_eval(&callee)?
                        {
                            self.direct_eval(native::argument(&args, 0))?
                        } else {
                            self.call_native(
                                callee,
                                receiver,
                                args,
                                instruction.opcode == Opcode::Construct,
                            )?
                        };
                        self.check_string(&result)?;
                        self.stack.truncate(base);
                        self.stack.push(result);
                    }
                    Opcode::DiscardReference => {
                        let value = self.pop();
                        let reference_values = operand;
                        let reference_start = self
                            .stack
                            .len()
                            .checked_sub(reference_values)
                            .expect("compiler retains a complete reference");
                        self.stack.truncate(reference_start);
                        self.stack.push(value);
                    }
                    Opcode::SetProperty => {
                        let value = self.pop();
                        let (object, key) = self.property_reference()?;
                        self.set_property(&object, &key, &value)?;
                        self.stack.push(value);
                    }
                    Opcode::SetDestructureProperty => {
                        // A destructuring leaf has already produced its value;
                        // evaluating a member target appends its object/key after
                        // that value. Preserve the value for the caller to pop.
                        let base = self.stack.len() - 3;
                        let value = self.stack[base].clone();
                        let object = self.stack[base + 1].clone();
                        let key = self.coerce_property_key(&self.stack[base + 2].clone())?;
                        self.set_property(&object, &key, &value)?;
                        self.stack.truncate(base);
                        self.stack.push(value);
                    }
                    Opcode::SetDestructurePropertyReference => {
                        // IteratorStepReference removed the iterator record,
                        // leaving object, raw key and element value in order.
                        let base = self.stack.len() - 3;
                        let object = self.stack[base].clone();
                        let key = self.coerce_property_key(&self.stack[base + 1].clone())?;
                        let value = self.stack[base + 2].clone();
                        self.set_property(&object, &key, &value)?;
                        self.stack.truncate(base);
                        self.stack.push(value);
                    }
                    Opcode::UpdateProperty => {
                        let (object, key) = self.property_reference()?;
                        self.stack.push(object.clone());
                        let old = self.get_property(&object, &key)?;
                        let old = self.coerce_number(&old)?;
                        let new = if operand & 1 == 0 {
                            old + 1.0
                        } else {
                            old - 1.0
                        };
                        self.set_property(&object, &key, &Value::Number(new))?;
                        self.stack.pop();
                        self.stack
                            .push(Value::Number(if operand & 2 == 0 { old } else { new }));
                    }
                    Opcode::SetLiteralPrototype => {
                        let value = self.pop();
                        let Value::Object(object) = self.pop() else {
                            unreachable!("literal receiver is an object")
                        };
                        match value {
                            Value::Object(prototype) => {
                                self.heap.set_prototype(object, Some(prototype))?
                            }
                            Value::Null => self.heap.set_prototype(object, None)?,
                            _ => {} // Literal __proto__ with a primitive value has no effect.
                        }
                    }
                    Opcode::SetCompletion => {
                        self.completion = self.pop();
                        self.completion_empty = false;
                    }
                    Opcode::ClearCompletion => {
                        self.completion = Value::Undefined;
                        self.completion_empty = true;
                    }
                    Opcode::PushHandler => {
                        debug_assert!(operand < code.handlers.len());
                        handlers.push(HandlerFrame {
                            metadata: operand,
                            stack_depth: self.stack.len(),
                            scope_depth: self.active_scopes.len(),
                            iterator_depth: iterators.len(),
                            with_depth: self.with_objects.len(),
                            state: HandlerState::Try,
                            pending: None,
                        });
                    }
                    Opcode::PopHandler => {
                        handlers
                            .pop()
                            .expect("compiler pops its active try handler");
                    }
                    Opcode::SaveCompletion => self
                        .completion_saves
                        .push((self.completion.clone(), self.completion_empty)),
                    Opcode::ResumeCompletion => return Ok(Some(Completion::Resume(operand))),
                    Opcode::AbruptJump => {
                        let jump = &code.abrupt_jumps[operand];
                        return Ok(Some(Completion::Jump {
                            cleanup: jump.cleanup as usize,
                            target: jump.target as usize,
                        }));
                    }
                    Opcode::Halt => return Ok(Some(Completion::Halt(self.completion.clone()))),
                }
                Ok(None)
            })();
            let completion = match outcome {
                Ok(completion) => completion,
                Err(error) if error.is_catchable() => Some(Completion::Throw(error)),
                Err(error) => return Err(error),
            };
            if let Some(promise) = suspended_await.take() {
                for handler in &mut handlers {
                    handler.stack_depth -= handler_stack_base;
                }
                return Ok(InterpreterExit::Await {
                    promise,
                    pc,
                    handlers: std::mem::take(&mut handlers),
                });
            }
            if let Some(completion) = completion {
                if let Completion::Yield(value) = completion {
                    for handler in &mut handlers {
                        handler.stack_depth -= handler_stack_base;
                    }
                    return Ok(InterpreterExit::Yield {
                        value,
                        pc,
                        iterators: std::mem::take(iterators),
                        handlers: std::mem::take(&mut handlers),
                    });
                }
                match self.resolve_completion(code, &mut handlers, iterators, completion)? {
                    CompletionAction::Continue => {}
                    CompletionAction::Jump(target) => pc = target,
                    CompletionAction::Return(value) => return Ok(InterpreterExit::Return(value)),
                    CompletionAction::TailRecur(args) => {
                        self.stack.truncate(stack_base);
                        self.unwind_scopes(code, 0);
                        self.unwind_with(0);
                        self.close_iterators_to(iterators, 0);
                        handlers.clear();
                        self.pending_completions.truncate(pending_base);
                        self.completion_saves.truncate(save_base);
                        self.completion = Value::Undefined;
                        self.completion_empty = true;
                        self.this = Value::Undefined;
                        self.arguments = args;
                        pc = 0;
                    }
                    CompletionAction::Throw(error) => return Err(error),
                }
            }
        }
    }
}
