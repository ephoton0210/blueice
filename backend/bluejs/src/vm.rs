// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded operand-stack interpreter for compiler-owned bytecode.
//! Allocating instructions root all VM-held objects around heap
//! safepoints. The collector itself additionally protects store inputs.

use crate::native::{self, NativeFunction};
use crate::primitive;
use crate::{Bytecode, Heap, HeapConfig, HeapError, JsString, JsSymbol, ObjectId, Opcode, PropertyDescriptor, PropertyName, RootId, Value};
use std::collections::HashMap;
mod builtins;
mod functions;
mod regexp;
use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy)]
pub struct VmConfig {
    pub heap: HeapConfig,
    /// Per-execute dispatch limit, including branches/scopes and native
    /// raw/split/replace loop steps; not a wall-clock or whole-native-work limit.
    pub instruction_budget: u64,
    /// Maximum UTF-16 payload bytes in any one runtime string (not total
    /// RSS): two bytes per code unit, including lone surrogates.
    pub max_string_bytes: usize,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self { heap: HeapConfig::default(), instruction_budget: 1_000_000, max_string_bytes: 1024 * 1024 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeError {
    ReferenceError(String),
    TypeError(String),
    RangeError(String),
    SyntaxError(String),
    Thrown(Value),
    Heap(HeapError),
    InstructionLimit,
    StringLimit { limit: usize },
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReferenceError(name) => write!(f, "ReferenceError: {name} is not defined"),
            Self::TypeError(message) => write!(f, "TypeError: {message}"),
            Self::RangeError(message) => write!(f, "RangeError: {message}"),
            Self::SyntaxError(message) => write!(f, "SyntaxError: {message}"),
            Self::Thrown(value) => write!(f, "uncaught JavaScript value: {value:?}"),
            Self::Heap(error) => error.fmt(f),
            Self::InstructionLimit => f.write_str("BlueJS instruction budget exhausted"),
            Self::StringLimit { limit } => write!(f, "BlueJS string exceeds {limit} bytes"),
        }
    }
}
impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Heap(error) => Some(error),
            _ => None,
        }
    }
}
impl From<HeapError> for RuntimeError {
    fn from(error: HeapError) -> Self {
        match error {
            HeapError::InvalidArrayLength => Self::RangeError("invalid array length".into()),
            _ => Self::Heap(error),
        }
    }
}

/// An isolated execution context. Each execute has fresh bindings; this
/// is not yet a persistent REPL/global environment or browser host.
pub struct Vm {
    config: VmConfig,
    heap: Heap,
    object_prototype: ObjectId,
    array_prototype: ObjectId,
    string_intrinsics: Option<(ObjectId, ObjectId)>,
    result_root: Option<RootId>,
    stack: Vec<Value>,
    // None is a lexical binding's uninitialized state, never JS undefined.
    bindings: Vec<Option<Value>>,
    completion: Value,
    remaining_instructions: u64,
    cells: HashMap<usize, ObjectId>,
    this: Value,
    arguments: Vec<Value>,
    strict: bool,
    call_depth: usize,
    globals: HashMap<String, ObjectId>,
    iterator_prototype: Option<ObjectId>,
    regexp_iterator_prototype: Option<ObjectId>,
    templates: HashMap<u64, ObjectId>,
    new_target: Value,
    iterator_base: Option<ObjectId>,
    array_iterator_prototype: Option<ObjectId>,
    joining: Vec<ObjectId>,
}

impl Default for Vm {
    fn default() -> Self {
        Self::new(VmConfig::default()).expect("default VM configuration is valid")
    }
}

impl Vm {
    /// Creates an isolated VM and its rooted object/array prototypes.
    /// The heap budget must accommodate both prototype records.
    pub fn new(config: VmConfig) -> Result<Self, HeapError> {
        let mut heap = Heap::new(config.heap)?;
        let object_prototype = heap.alloc_object(None)?;
        // Permanent root, released with the heap. Builtin properties and
        // callable methods are installed lazily by string_intrinsics.
        heap.root(object_prototype)?;
        let array_prototype = heap.alloc_array(0, Some(object_prototype))?;
        heap.root(array_prototype)?;
        Ok(Self {
            config,
            heap,
            object_prototype,
            array_prototype,
            string_intrinsics: None,
            result_root: None,
            stack: Vec::new(),
            bindings: Vec::new(),
            completion: Value::Undefined,
            remaining_instructions: 0,
            cells: HashMap::new(),
            this: Value::Undefined,
            arguments: Vec::new(),
            strict: false,
            call_depth: 0,
            globals: HashMap::new(),
            iterator_prototype: None,
            regexp_iterator_prototype: None,
            templates: HashMap::new(),
            new_target: Value::Undefined,
            iterator_base: None,
            array_iterator_prototype: None,
            joining: Vec::new(),
        })
    }

    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    /// Executes only bytecode produced by [`crate::compile`]. A returned
    /// object and its reachable graph stay alive until the next execute
    /// (including a failing execute), or until this VM is dropped.
    /// Both success and error paths release all temporary runtime roots.
    pub fn execute(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        if let Some(root) = self.result_root.take() {
            self.heap.unroot(root)?;
        }
        self.heap.collect_major();
        self.bindings.resize(code.bindings.len(), None);
        self.remaining_instructions = self.config.instruction_budget;
        self.strict = code.strict;
        let result = self.run(code).and_then(|value| {
            if let Value::Object(id) = value {
                self.result_root = Some(self.heap.root(id)?);
            }
            Ok(value)
        });
        if let Err(RuntimeError::Thrown(Value::Object(id))) = &result {
            self.result_root = Some(self.heap.root(*id)?);
        }
        self.stack.clear();
        self.bindings.clear();
        self.cells.clear();
        self.completion = Value::Undefined;
        self.heap.collect_major();
        result
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("compiler balances the operand stack")
    }

    fn check_string(&self, value: &Value) -> Result<(), RuntimeError> {
        if matches!(value, Value::String(s) if s.byte_len() > self.config.max_string_bytes) {
            Err(RuntimeError::StringLimit { limit: self.config.max_string_bytes })
        } else {
            Ok(())
        }
    }

    fn with_roots<T>(&mut self, operation: impl FnOnce(&mut Heap) -> Result<T, HeapError>) -> Result<T, RuntimeError> {
        let mut roots = Vec::new();
        let registration = (|| {
            for value in self.stack.iter().chain(self.bindings.iter().flatten()).chain(std::iter::once(&self.completion)) {
                if let Value::Object(id) = value {
                    // Rooting cannot GC, but can exhaust the root-ID
                    // counter. Partial registrations must be released too.
                    roots.push(self.heap.root(*id)?);
                }
            }
            for id in self.cells.values() {
                roots.push(self.heap.root(*id)?);
            }
            Ok(())
        })();
        let result = registration.and_then(|()| operation(&mut self.heap));
        for root in roots {
            self.heap.unroot(root).expect("temporary root belongs to this safepoint");
        }
        result.map_err(RuntimeError::from)
    }

    fn run(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        let mut iterators = Vec::new();
        let result = self.interpret(code, &mut iterators);
        if result.is_err() {
            if let Err(RuntimeError::Thrown(value)) = &result {
                self.stack.push(value.clone());
            }
            self.stack.extend(iterators.iter().cloned());
            for record in iterators.into_iter().rev() {
                // IteratorClose preserves an existing throw even when return
                // throws too. Resource exhaustion retains the original limit.
                let _ = self.iterator_close(&record);
            }
        }
        result
    }

    fn interpret(&mut self, code: &Bytecode, iterators: &mut Vec<Value>) -> Result<Value, RuntimeError> {
        let mut pc = 0;
        loop {
            self.charge_step()?;
            let instruction = code.instruction(pc).expect("compiler emits valid instruction boundaries");
            let operand = instruction.operand.unwrap_or(0) as usize;
            pc += instruction.opcode.width();
            match instruction.opcode {
                Opcode::DefineData | Opcode::DefineAccessor => {
                    let value = self.pop();
                    let (receiver, key) = self.property_reference()?;
                    let object = receiver.object_id().unwrap();
                    if instruction.opcode == Opcode::DefineData {
                        self.define_data(object, key, value.clone(), true, true, true)?;
                    } else {
                        let descriptor = PropertyDescriptor {
                            get: (operand == 0).then(|| value.clone()),
                            set: (operand != 0).then(|| value.clone()),
                            enumerable: Some(true),
                            configurable: Some(true),
                            ..Default::default()
                        };
                        self.with_roots(|heap| heap.define_own_property(object, key, descriptor))?;
                    }
                    self.stack.push(value);
                }
                Opcode::DeleteProperty => {
                    let (receiver, key) = self.property_reference()?;
                    let deleted = match receiver {
                        Value::Object(id) => self.heap.delete(id, key)?,
                        Value::String(s) => !matches!(&key, PropertyName::String(key) if s.own_property(key).is_some()),
                        _ => true,
                    };
                    if !deleted && self.strict {
                        return Err(RuntimeError::TypeError("cannot delete non-configurable property".into()));
                    }
                    self.stack.push(Value::Bool(deleted));
                }
                Opcode::Throw => return Err(RuntimeError::Thrown(self.pop())),
                Opcode::ArrayPush => {
                    let base = self.stack.len() - 2;
                    self.array_push(&self.stack[base].clone(), &self.stack[base + 1].clone(), operand)?;
                    self.stack.truncate(base + 1);
                }
                Opcode::CallSpread => {
                    let base = self.stack.len() - 3;
                    let args = self.array_like_values(&self.stack[base + 2].clone())?;
                    let result = self.call_native(self.stack[base].clone(), self.stack[base + 1].clone(), args, operand != 0)?;
                    self.stack.truncate(base);
                    self.stack.push(result);
                }
                Opcode::RegExpLiteral => {
                    let base = self.stack.len() - 2;
                    let regexp = self.regexp_create(&self.stack[base].clone(), &self.stack[base + 1].clone())?;
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
                    self.stack.push(iterator);
                }
                Opcode::IteratorStep => {
                    let record = self.stack.last().unwrap().clone();
                    iterators.retain(|active| active != &record);
                    let result = self.iterator_step(&record)?;
                    self.pop();
                    if let Some(value) = result {
                        iterators.push(record);
                        self.stack.push(value);
                    } else {
                        pc = operand;
                    }
                }
                Opcode::IteratorClose => {
                    let record = self.stack.last().unwrap().clone();
                    iterators.retain(|active| active != &record);
                    self.iterator_close(&record)?;
                    self.pop();
                }
                Opcode::Closure => {
                    let child = code.functions[operand].clone();
                    let (_, prototype) = self.string_intrinsics()?;
                    let constructor = self.heap.get(prototype, "constructor")?.object_id().unwrap();
                    let function_prototype = self.heap.prototype(constructor)?.unwrap();
                    let mut captures = Vec::new();
                    for &slot in &child.captures {
                        captures.push(self.capture(slot as usize)?);
                    }
                    let this = if child.arrow { self.this.clone() } else { Value::Undefined };
                    let id = self.with_roots(|heap| heap.alloc_closure(child.clone(), captures, this, function_prototype))?;
                    self.stack.push(Value::Object(id));
                    self.define_data(id, "name", Value::String(child.function_name.clone().into()), false, false, true)?;
                    self.define_data(id, "length", Value::Number(child.function_length as f64), false, false, true)?;
                    if child.constructible {
                        let object_prototype = self.object_prototype;
                        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                        self.define_data(id, "prototype", Value::Object(prototype), true, false, false)?;
                        self.define_data(prototype, "constructor", Value::Object(id), true, false, true)?;
                    }
                }
                Opcode::This => self.stack.push(self.this.clone()),
                Opcode::Argument => self.stack.push(native::argument(&self.arguments, operand).clone()),
                Opcode::RestArguments => {
                    let array = self.array_from(self.arguments.iter().skip(operand).cloned().collect())?;
                    self.stack.push(array);
                }
                Opcode::Return => return Ok(self.pop()),
                Opcode::Global => {
                    let Value::String(name) = &code.constants[operand] else { unreachable!() };
                    let value = self.global(&name.to_utf8().unwrap())?;
                    self.stack.push(value);
                }
                Opcode::ToPropertyKey => {
                    let value = self.stack.last().unwrap().clone();
                    let key = self.coerce_property_key(&value)?;
                    self.pop();
                    self.stack.push(key.value());
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
                    let value = self.binding_value(operand)?.ok_or_else(|| RuntimeError::ReferenceError(code.bindings[operand].name.clone()))?;
                    self.stack.push(value);
                }
                Opcode::InitializeBinding => {
                    let value = self.pop();
                    self.store_binding(operand, value)?;
                }
                Opcode::StoreBinding => {
                    // ECMA-262 §9.1.1.1.5: TDZ takes precedence over the
                    // immutable-binding assignment error, including const.
                    if self.binding_value(operand)?.is_none() {
                        return Err(RuntimeError::ReferenceError(code.bindings[operand].name.clone()));
                    }
                    if !code.bindings[operand].mutable {
                        return Err(RuntimeError::TypeError(format!("assignment to constant {}", code.bindings[operand].name)));
                    }
                    self.store_binding(operand, self.stack.last().expect("store has a value").clone())?;
                }
                Opcode::UnboundName => {
                    let Value::String(name) = &code.constants[operand] else { unreachable!("compiler emits a name") };
                    return Err(RuntimeError::ReferenceError(name.to_utf8().expect("compiler emits a UTF-8 identifier")));
                }
                Opcode::EnterScope | Opcode::LeaveScope => {
                    for slot in &code.scopes[operand] {
                        self.cells.remove(&(*slot as usize));
                        self.bindings[*slot as usize] =
                            if instruction.opcode == Opcode::EnterScope && !code.bindings[*slot as usize].lexical { Some(Value::Undefined) } else { None };
                    }
                }
                Opcode::Pop => {
                    self.pop();
                }
                Opcode::Dup => self.stack.push(self.stack.last().expect("dup has a value").clone()),
                Opcode::Dup2 => {
                    let index = self.stack.len() - 2;
                    self.stack.push(self.stack[index].clone());
                    self.stack.push(self.stack[index + 1].clone());
                }
                Opcode::Add => self.binary(Self::add)?,
                Opcode::Subtract => self.numeric(|a, b| a - b)?,
                Opcode::Multiply => self.numeric(|a, b| a * b)?,
                Opcode::Divide => self.numeric(|a, b| a / b)?,
                Opcode::Remainder => self.numeric(|a, b| a % b)?,
                Opcode::StrictEqual => self.binary(|_, a, b| Ok(Value::Bool(a == b)))?,
                Opcode::StrictNotEqual => self.binary(|_, a, b| Ok(Value::Bool(a != b)))?,
                Opcode::Instanceof => self.binary(|vm, value, target| vm.has_instance(value, target, false).map(Value::Bool))?,
                Opcode::Less => self.relational(|order| order == Ordering::Less)?,
                Opcode::Greater => self.relational(|order| order == Ordering::Greater)?,
                Opcode::LessEqual => self.relational(|order| order != Ordering::Greater)?,
                Opcode::GreaterEqual => self.relational(|order| order != Ordering::Less)?,
                Opcode::Negate | Opcode::ToNumber | Opcode::ToString | Opcode::Not | Opcode::Typeof => {
                    let arg = self.stack.last().unwrap().clone();
                    let value = match instruction.opcode {
                        Opcode::Negate => Value::Number(-self.coerce_number(&arg)?),
                        Opcode::ToNumber => Value::Number(self.coerce_number(&arg)?),
                        Opcode::ToString => Value::String(self.coerce_string(&arg)?),
                        Opcode::Not => Value::Bool(!primitive::truthy(&arg)),
                        _ => {
                            let callable = self.is_callable(&arg)?;
                            Value::String(if callable { "function" } else { primitive::type_name(&arg) }.into())
                        }
                    };
                    self.check_string(&value)?;
                    self.pop();
                    self.stack.push(value);
                }
                Opcode::Jump => pc = operand,
                Opcode::JumpIfFalse | Opcode::JumpIfTrue | Opcode::JumpIfNotNullish => {
                    let arg = self.pop();
                    let take = match instruction.opcode {
                        Opcode::JumpIfFalse => !primitive::truthy(&arg),
                        Opcode::JumpIfTrue => primitive::truthy(&arg),
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
                    let id = self.with_roots(|heap| heap.alloc_array(operand as u32, Some(prototype)))?;
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
                Opcode::Call | Opcode::Construct => {
                    // Leave every call input on the stack until dispatch
                    // completes, so native allocations see all GC roots.
                    let base = self.stack.len() - operand - 2;
                    let result =
                        self.call_native(self.stack[base].clone(), self.stack[base + 1].clone(), self.stack[base + 2..].to_vec(), instruction.opcode == Opcode::Construct)?;
                    self.check_string(&result)?;
                    self.stack.truncate(base);
                    self.stack.push(result);
                }
                Opcode::SetProperty => {
                    let value = self.pop();
                    let (object, key) = self.property_reference()?;
                    self.set_property(&object, &key, &value)?;
                    self.stack.push(value);
                }
                Opcode::UpdateProperty => {
                    let (object, key) = self.property_reference()?;
                    self.stack.push(object.clone());
                    let old = self.get_property(&object, &key)?;
                    let old = self.coerce_number(&old)?;
                    let new = if operand & 1 == 0 { old + 1.0 } else { old - 1.0 };
                    self.set_property(&object, &key, &Value::Number(new))?;
                    self.stack.pop();
                    self.stack.push(Value::Number(if operand & 2 == 0 { old } else { new }));
                }
                Opcode::SetLiteralPrototype => {
                    let value = self.pop();
                    let Value::Object(object) = self.pop() else { unreachable!("literal receiver is an object") };
                    match value {
                        Value::Object(prototype) => self.heap.set_prototype(object, Some(prototype))?,
                        Value::Null => self.heap.set_prototype(object, None)?,
                        _ => {} // Literal __proto__ with a primitive value has no effect.
                    }
                }
                Opcode::SetCompletion => self.completion = self.pop(),
                Opcode::ClearCompletion => self.completion = Value::Undefined,
                Opcode::Halt => return Ok(self.completion.clone()),
            }
        }
    }

    fn charge_step(&mut self) -> Result<(), RuntimeError> {
        if self.remaining_instructions == 0 {
            return Err(RuntimeError::InstructionLimit);
        }
        self.remaining_instructions -= 1;
        Ok(())
    }

    fn get_property(&mut self, receiver: &Value, key: &PropertyName) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        let result = self.get_property_value(receiver, key);
        self.stack.truncate(base);
        result
    }

    fn get_property_value(&mut self, receiver: &Value, key: &PropertyName) -> Result<Value, RuntimeError> {
        match receiver {
            Value::Object(id) => {
                if self.string_intrinsics.is_none() && (key == "toString" || key == "valueOf" || key == "join" || *key == PropertyName::from(JsSymbol::well_known("iterator"))) {
                    self.string_intrinsics()?;
                }
                self.get_from_prototype(*id, receiver, key)
            }
            Value::String(string) => {
                if let PropertyName::String(key) = key {
                    if let Some(value) = string.own_property(key) {
                        return Ok(value);
                    }
                }
                let (_, prototype) = self.string_intrinsics()?;
                self.get_from_prototype(prototype, receiver, key)
            }
            Value::Null | Value::Undefined => Err(RuntimeError::TypeError("cannot access a property of null or undefined".into())),
            _ => {
                let constructor = self.global(match receiver {
                    Value::Symbol(_) => "Symbol",
                    Value::Bool(_) => "Boolean",
                    _ => "Number",
                })?;
                let prototype = self.get_property(&constructor, &"prototype".into())?.object_id().unwrap();
                self.get_from_prototype(prototype, receiver, key)
            }
        }
    }

    fn set_property(&mut self, receiver: &Value, key: &PropertyName, value: &Value) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        self.stack.extend([receiver.clone(), value.clone()]);
        let result = self.set_property_value(receiver, key, value);
        self.stack.truncate(base);
        result
    }

    fn set_property_value(&mut self, receiver: &Value, key: &PropertyName, value: &Value) -> Result<(), RuntimeError> {
        // ToObject provides the lookup chain; accessor calls retain the
        // original primitive receiver. Creating a data property still fails.
        let object = self.coerce_object(receiver)?;
        self.stack.push(Value::Object(object));
        let mut current = Some(object);
        while let Some(id) = current {
            if let Some(desc) = self.heap.get_own_property_descriptor(id, key)? {
                if desc.accessor() {
                    let setter = desc.set.unwrap_or(Value::Undefined);
                    if self.is_callable(&setter)? {
                        self.call_native(setter, receiver.clone(), vec![value.clone()], false)?;
                        return Ok(());
                    }
                    return if self.strict { Err(RuntimeError::TypeError("property has no setter".into())) } else { Ok(()) };
                }
                if desc.writable == Some(false) {
                    return if self.strict { Err(RuntimeError::TypeError("property is read-only".into())) } else { Ok(()) };
                }
                break;
            }
            current = self.heap.prototype(id)?;
        }
        if !matches!(receiver, Value::Object(_)) {
            return if self.strict { Err(RuntimeError::TypeError("cannot assign to primitive property".into())) } else { Ok(()) };
        }
        let stored = if key == "length" && self.heap.is_array(object)? { self.array_length_value(value)? } else { value.clone() };
        match self.with_roots(|heap| heap.set(object, key, stored)) {
            Err(RuntimeError::Heap(HeapError::ReadOnlyProperty)) => {
                if self.strict {
                    Err(RuntimeError::TypeError("property cannot be assigned".into()))
                } else {
                    Ok(())
                }
            }
            result => result,
        }
    }

    fn property_reference(&mut self) -> Result<(Value, PropertyName), RuntimeError> {
        let key = self.pop();
        let key = self.coerce_property_key(&key)?;
        match self.pop() {
            Value::Null | Value::Undefined => Err(RuntimeError::TypeError("cannot access a property of null or undefined".into())),
            receiver => Ok((receiver, key)),
        }
    }

    fn string_intrinsics(&mut self) -> Result<(ObjectId, ObjectId), RuntimeError> {
        if let Some(intrinsics) = self.string_intrinsics {
            return Ok(intrinsics);
        }
        let object_prototype = self.object_prototype;
        let function_prototype = self.with_roots(|heap| heap.alloc_native_function(NativeFunction::Empty, "", object_prototype))?;
        let constructor = self.with_roots(|heap| heap.alloc_native_function(NativeFunction::String, "String", function_prototype))?;
        let root = self.heap.root(constructor)?;
        let result = (|| {
            let prototype = self.with_roots(|heap| heap.alloc_string(JsString::default(), Some(object_prototype)))?;
            self.define_data(constructor, "prototype", Value::Object(prototype), false, false, false)?;
            self.define_data(prototype, "constructor", Value::Object(constructor), true, false, true)?;
            self.define_data(constructor, "name", Value::String("String".into()), false, false, true)?;
            self.define_data(constructor, "length", Value::Number(1.0), false, false, true)?;
            self.define_data(function_prototype, "length", Value::Number(0.0), false, false, true)?;
            self.define_data(function_prototype, "name", Value::String(JsString::default()), false, false, true)?;
            self.install_native(function_prototype, function_prototype, "call", 1, NativeFunction::Call)?;
            self.install_native(function_prototype, function_prototype, "apply", 2, NativeFunction::Apply)?;
            self.install_native(function_prototype, function_prototype, "bind", 1, NativeFunction::Bind)?;
            self.install_symbol_native(function_prototype, function_prototype, "hasInstance", 1, NativeFunction::HasInstance)?;
            self.install_native(function_prototype, function_prototype, "toString", 0, NativeFunction::FunctionToString)?;
            self.install_native(constructor, function_prototype, "fromCharCode", 1, NativeFunction::FromCharCode)?;
            self.install_native(constructor, function_prototype, "fromCodePoint", 1, NativeFunction::FromCodePoint)?;
            self.install_native(constructor, function_prototype, "raw", 1, NativeFunction::Raw)?;
            self.install_native(prototype, function_prototype, "split", 2, NativeFunction::Split)?;
            self.install_native(prototype, function_prototype, "replace", 2, NativeFunction::Replace)?;
            self.install_native(prototype, function_prototype, "replaceAll", 2, NativeFunction::ReplaceAll)?;
            for &(name, length, method) in native::STRING_METHODS {
                self.install_native(prototype, function_prototype, name, length, NativeFunction::StringMethod(method))?;
            }
            for (alias, original) in [("trimLeft", "trimStart"), ("trimRight", "trimEnd")] {
                let function = self.heap.get(prototype, original)?;
                self.define_data(prototype, alias, function, true, false, true)?;
            }
            self.install_symbol_native(prototype, function_prototype, "iterator", 0, NativeFunction::StringIterator)?;
            for (name, method) in [("match", native::PatternMethod::Match), ("matchAll", native::PatternMethod::MatchAll), ("search", native::PatternMethod::Search)] {
                self.install_native(prototype, function_prototype, name, 1, NativeFunction::Pattern(method))?;
            }
            self.install_native(object_prototype, function_prototype, "toString", 0, NativeFunction::ObjectToString)?;
            self.install_native(object_prototype, function_prototype, "valueOf", 0, NativeFunction::ObjectValueOf)?;
            self.install_native(self.array_prototype, function_prototype, "toString", 0, NativeFunction::ArrayToString)?;
            self.install_native(self.array_prototype, function_prototype, "join", 1, NativeFunction::ArrayJoin)?;
            self.install_symbol_native(self.array_prototype, function_prototype, "iterator", 0, NativeFunction::ArrayIterator)?;
            Ok((constructor, prototype))
        })();
        match result {
            Ok(intrinsics) => {
                self.string_intrinsics = Some(intrinsics);
                Ok(intrinsics)
            }
            Err(error) => {
                // The only pre-existing owners modified by bootstrap are
                // these two empty prototypes. Roll back published edges too.
                for (owner, key) in [
                    (object_prototype, PropertyName::from("toString")),
                    (object_prototype, "valueOf".into()),
                    (self.array_prototype, "toString".into()),
                    (self.array_prototype, "join".into()),
                    (self.array_prototype, JsSymbol::well_known("iterator").into()),
                ] {
                    self.heap.delete(owner, key)?;
                }
                self.heap.unroot(root)?;
                Err(error)
            }
        }
    }

    fn install_native(&mut self, owner: ObjectId, prototype: ObjectId, name: &str, length: u32, function: NativeFunction) -> Result<(), RuntimeError> {
        let id = self.with_roots(|heap| heap.alloc_native_function(function, name, prototype))?;
        self.stack.push(Value::Object(id));
        let result = (|| {
            self.define_data(id, "name", Value::String(name.into()), false, false, true)?;
            self.define_data(id, "length", Value::Number(f64::from(length)), false, false, true)?;
            self.define_data(owner, name, Value::Object(id), true, false, true)
        })();
        self.stack.pop();
        result?;
        Ok(())
    }

    fn call_native(&mut self, callee: Value, receiver: Value, args: Vec<Value>, construct: bool) -> Result<Value, RuntimeError> {
        let target = if construct { callee.clone() } else { Value::Undefined };
        self.call_with_target(callee, receiver, args, construct, target)
    }

    fn call_with_target(&mut self, callee: Value, receiver: Value, args: Vec<Value>, construct: bool, target: Value) -> Result<Value, RuntimeError> {
        if self.call_depth >= 32 {
            return Err(RuntimeError::RangeError("maximum call depth exceeded".into()));
        }
        self.charge_step()?;
        let base = self.stack.len();
        self.stack.extend([callee.clone(), receiver.clone()]);
        self.stack.extend(args.iter().cloned());
        self.stack.push(target.clone());
        let previous_target = std::mem::replace(&mut self.new_target, target);
        self.call_depth += 1;
        let result = self.dispatch_call(callee, receiver, args, construct).and_then(|value| {
            self.check_string(&value)?;
            Ok(value)
        });
        self.new_target = previous_target;
        self.call_depth -= 1;
        self.stack.truncate(base);
        result
    }

    fn dispatch_call(&mut self, mut callee: Value, mut receiver: Value, mut args: Vec<Value>, construct: bool) -> Result<Value, RuntimeError> {
        // Bound wrappers have no execution contexts of their own. Walk them
        // with fuel rather than consuming Rust stack or the JS frame limit.
        let mut prefixes = Vec::new();
        while let Value::Object(id) = callee {
            let Some(bound) = self.heap.bound_function(id)?.cloned() else { break };
            self.charge_step()?;
            if construct && !bound.constructible {
                return Err(RuntimeError::TypeError("bound target is not a constructor".into()));
            }
            if construct && self.new_target == callee {
                self.new_target = Value::Object(bound.target);
            }
            callee = Value::Object(bound.target);
            receiver = bound.this;
            prefixes.push(bound.args);
        }
        if !prefixes.is_empty() {
            // Concatenate once, starting at the innermost wrapper, to avoid
            // repeatedly copying the accumulated arguments of long chains.
            args = prefixes.into_iter().rev().flatten().chain(args).collect();
        }
        if let Value::Object(id) = callee {
            if let Some((code, captures, lexical_this)) = self.heap.closure(id)? {
                let receiver = if code.arrow { lexical_this } else { receiver };
                return self.call_closure(code, captures, receiver, args, construct);
            }
        }
        let function = if let Value::Object(id) = callee { self.heap.native_function(id)? } else { None };
        let Some(function) = function else { return Err(RuntimeError::TypeError("value is not callable".into())) };
        if construct && !matches!(function, NativeFunction::String | NativeFunction::Object | NativeFunction::RegExp | NativeFunction::PrimitiveConstructor(_)) {
            return Err(RuntimeError::TypeError("value is not a constructor".into()));
        }
        self.native_call(function, receiver, args, construct)
    }

    fn unbox_string(&self, value: &Value) -> Result<Value, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(string) = self.heap.boxed_string(*id)? {
                return Ok(Value::String(string.clone()));
            }
        }
        Ok(value.clone())
    }

    fn string_receiver(&mut self, value: &Value) -> Result<JsString, RuntimeError> {
        if matches!(value, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError("String method requires a non-null receiver".into()));
        }
        self.coerce_string(value)
    }

    fn string_raw(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let template = Value::Object(self.coerce_object(native::argument(args, 0))?);
        self.stack.push(template.clone());
        let raw = self.get_property(&template, &"raw".into())?;
        self.stack.push(raw.clone());
        let raw = Value::Object(self.coerce_object(&raw)?);
        self.stack.push(raw.clone());
        let length = self.get_property(&raw, &"length".into())?;
        let count = self.coerce_length(&length)? as u64;
        let mut result = JsString::default();
        for index in 0..count {
            self.charge_step()?; // A huge array-like length with empty entries must still terminate.
            let literal = self.get_property(&raw, &index.to_string().into())?;
            native::append(&mut result, &self.coerce_string(&literal)?, self.config.max_string_bytes)?;
            if index + 1 < count {
                if let Some(substitution) = args.get(index as usize + 1) {
                    native::append(&mut result, &self.coerce_string(substitution)?, self.config.max_string_bytes)?;
                }
            }
        }
        Ok(Value::String(result))
    }

    fn string_split(&mut self, receiver: &Value, args: &[Value]) -> Result<Value, RuntimeError> {
        if matches!(receiver, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError("String method requires a non-null receiver".into()));
        }
        let separator = native::argument(args, 0);
        if !matches!(separator, Value::Null | Value::Undefined) {
            let method = self.get_method(separator, &JsSymbol::well_known("split").into())?;
            if method != Value::Undefined {
                return self.call_native(method, separator.clone(), vec![receiver.clone(), native::argument(args, 1).clone()], false);
            }
        }
        let string = self.string_receiver(receiver)?;
        let separator = native::argument(args, 0);
        let limit = native::argument(args, 1);
        let limit = if matches!(limit, Value::Undefined) { u32::MAX } else { native::uint32(&Value::Number(self.coerce_number(limit)?))? };
        // Even a zero limit converts the separator first (§22.1.3.23).
        let search = self.coerce_string(separator)?;
        let prototype = self.array_prototype;
        let array = self.with_roots(|heap| heap.alloc_array(0, Some(prototype)))?;
        self.stack.push(Value::Object(array)); // Root the growing result through every store.
        let mut count = 0u32;
        let units = string.as_code_units();
        let mut start = 0;
        while count < limit {
            self.charge_step()?;
            let end = if matches!(separator, Value::Undefined) {
                units.len()
            } else if search.is_empty() {
                if start == units.len() {
                    break;
                }
                start + 1
            } else if search.len() <= units.len() {
                (start..=units.len() - search.len()).find(|&index| units[index..].starts_with(search.as_code_units())).unwrap_or(units.len())
            } else {
                units.len()
            };
            let value = Value::String(JsString::from_code_units(units[start..end].to_vec()));
            self.with_roots(|heap| heap.set(array, count.to_string(), value))?;
            count += 1;
            if end == units.len() {
                break;
            }
            start = end + search.len();
        }
        Ok(Value::Object(array))
    }

    fn string_replace(&mut self, receiver: &Value, args: &[Value], all: bool) -> Result<Value, RuntimeError> {
        if matches!(receiver, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError("String method requires a non-null receiver".into()));
        }
        let search = native::argument(args, 0);
        if !matches!(search, Value::Null | Value::Undefined) {
            if all {
                self.require_global_pattern(search)?;
            }
            let method = self.get_method(search, &JsSymbol::well_known("replace").into())?;
            if method != Value::Undefined {
                return self.call_native(method, search.clone(), vec![receiver.clone(), native::argument(args, 1).clone()], false);
            }
        }
        let string = self.string_receiver(receiver)?;
        let search = self.coerce_string(native::argument(args, 0))?;
        let replace = native::argument(args, 1);
        let callable = self.is_callable(replace)?;
        let template = if callable { JsString::default() } else { self.coerce_string(replace)? };
        let mut result = JsString::default();
        let mut end = 0;
        let mut next = 0;
        if search.len() <= string.len() {
            while let Some(position) = (next..=string.len() - search.len()).find(|&index| string.as_code_units()[index..].starts_with(search.as_code_units())) {
                self.charge_step()?;
                let replacement = if callable {
                    let value = self.call_native(
                        replace.clone(),
                        Value::Undefined,
                        vec![Value::String(search.clone()), Value::Number(position as f64), Value::String(string.clone())],
                        false,
                    )?;
                    self.coerce_string(&value)?
                } else {
                    native::substitution(&string, &search, position, &template, self.config.max_string_bytes)?
                };
                native::append(&mut result, &JsString::from_code_units(string.as_code_units()[end..position].to_vec()), self.config.max_string_bytes)?;
                native::append(&mut result, &replacement, self.config.max_string_bytes)?;
                end = position + search.len();
                if !all {
                    break;
                }
                next = position + search.len().max(1);
            }
        }
        native::append(&mut result, &JsString::from_code_units(string.as_code_units()[end..].to_vec()), self.config.max_string_bytes)?;
        Ok(Value::String(result))
    }

    fn binary(&mut self, operation: impl FnOnce(&mut Self, Value, Value) -> Result<Value, RuntimeError>) -> Result<(), RuntimeError> {
        let base = self.stack.len() - 2;
        let value = operation(self, self.stack[base].clone(), self.stack[base + 1].clone())?;
        self.stack.truncate(base);
        self.stack.push(value);
        Ok(())
    }

    fn numeric(&mut self, operation: fn(f64, f64) -> f64) -> Result<(), RuntimeError> {
        self.binary(|vm, a, b| Ok(Value::Number(operation(vm.coerce_number(&a)?, vm.coerce_number(&b)?))))
    }

    fn relational(&mut self, accept: fn(Ordering) -> bool) -> Result<(), RuntimeError> {
        self.binary(|vm, a, b| {
            let a = vm.coerce_primitive(&a, "number")?;
            let b = vm.coerce_primitive(&b, "number")?;
            Ok(Value::Bool(primitive::compare(&a, &b)?.is_some_and(accept)))
        })
    }

    fn add(&mut self, left: Value, right: Value) -> Result<Value, RuntimeError> {
        let left = self.coerce_primitive(&left, "default")?;
        let right = self.coerce_primitive(&right, "default")?;
        if matches!(left, Value::String(_)) || matches!(right, Value::String(_)) {
            let mut a = primitive::string(&left)?;
            let b = primitive::string(&right)?;
            if a.byte_len().checked_add(b.byte_len()).is_none_or(|len| len > self.config.max_string_bytes) {
                return Err(RuntimeError::StringLimit { limit: self.config.max_string_bytes });
            }
            a.push_str(&b);
            Ok(Value::String(a))
        } else {
            Ok(Value::Number(primitive::number(&left)? + primitive::number(&right)?))
        }
    }
}
