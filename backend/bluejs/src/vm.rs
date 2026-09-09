// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded operand-stack interpreter for compiler-owned bytecode.
//! Allocating instructions root all VM-held objects around heap
//! safepoints. The collector itself additionally protects store inputs.

use crate::native::{self, NativeFunction};
use crate::primitive;
use crate::{Bytecode, Heap, HeapConfig, HeapError, JsString, ObjectId, Opcode, RootId, Value};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    ReferenceError(String),
    TypeError(String),
    RangeError(String),
    Unsupported(&'static str),
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
            Self::Unsupported(feature) => write!(f, "BlueJS execution does not yet support {feature}"),
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
        // callable Object.prototype methods are a later slice.
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
        let result = self.run(code).and_then(|value| {
            if let Value::Object(id) = value {
                self.result_root = Some(self.heap.root(id)?);
            }
            Ok(value)
        });
        self.stack.clear();
        self.bindings.clear();
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
            Ok(())
        })();
        let result = registration.and_then(|()| operation(&mut self.heap));
        for root in roots {
            self.heap.unroot(root).expect("temporary root belongs to this safepoint");
        }
        result.map_err(RuntimeError::from)
    }

    fn run(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        let mut pc = 0;
        self.remaining_instructions = self.config.instruction_budget;
        loop {
            self.charge_step()?;
            let instruction = code.instruction(pc).expect("compiler emits valid instruction boundaries");
            let operand = instruction.operand.unwrap_or(0) as usize;
            pc += instruction.opcode.width();
            match instruction.opcode {
                Opcode::Constant => {
                    self.check_string(&code.constants[operand])?;
                    self.stack.push(code.constants[operand].clone());
                }
                Opcode::GlobalString => {
                    let (constructor, _) = self.string_intrinsics()?;
                    self.stack.push(Value::Object(constructor));
                }
                Opcode::GetBinding => {
                    let value = self.bindings[operand].as_ref().ok_or_else(|| RuntimeError::ReferenceError(code.bindings[operand].name.clone()))?;
                    self.stack.push(value.clone());
                }
                Opcode::InitializeBinding => self.bindings[operand] = Some(self.pop()),
                Opcode::StoreBinding => {
                    // ECMA-262 §9.1.1.1.5: TDZ takes precedence over the
                    // immutable-binding assignment error, including const.
                    if self.bindings[operand].is_none() {
                        return Err(RuntimeError::ReferenceError(code.bindings[operand].name.clone()));
                    }
                    if !code.bindings[operand].mutable {
                        return Err(RuntimeError::TypeError(format!("assignment to constant {}", code.bindings[operand].name)));
                    }
                    self.bindings[operand] = Some(self.stack.last().expect("store has a value").clone());
                }
                Opcode::UnboundName => {
                    let Value::String(name) = &code.constants[operand] else { unreachable!("compiler emits a name") };
                    return Err(RuntimeError::ReferenceError(name.to_utf8().expect("compiler emits a UTF-8 identifier")));
                }
                Opcode::EnterScope | Opcode::LeaveScope => {
                    for slot in &code.scopes[operand] {
                        self.bindings[*slot as usize] = if instruction.opcode == Opcode::EnterScope && !code.bindings[*slot as usize].lexical { Some(Value::Undefined) } else { None };
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
                Opcode::Less => self.relational(|order| order == Ordering::Less)?,
                Opcode::Greater => self.relational(|order| order == Ordering::Greater)?,
                Opcode::LessEqual => self.relational(|order| order != Ordering::Greater)?,
                Opcode::GreaterEqual => self.relational(|order| order != Ordering::Less)?,
                Opcode::Negate | Opcode::ToNumber | Opcode::ToString | Opcode::Not | Opcode::Typeof => {
                    let arg = self.pop();
                    let value = match instruction.opcode {
                        Opcode::Negate => Value::Number(-primitive::number(&arg)?),
                        Opcode::ToNumber => Value::Number(primitive::number(&arg)?),
                        Opcode::ToString => Value::String(primitive::string(&arg)?),
                        Opcode::Not => Value::Bool(!primitive::truthy(&arg)),
                        _ => {
                            let callable = if let Value::Object(id) = arg { self.heap.native_function(id)?.is_some() } else { false };
                            Value::String(if callable { "function" } else { primitive::type_name(&arg) }.into())
                        }
                    };
                    self.check_string(&value)?;
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
                    let result = self.call_native(self.stack[base].clone(), self.stack[base + 1].clone(), self.stack[base + 2..].to_vec(), instruction.opcode == Opcode::Construct)?;
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
                    let old = primitive::number(&self.get_property(&object, &key)?)?;
                    let new = if operand & 1 == 0 { old + 1.0 } else { old - 1.0 };
                    self.set_property(&object, &key, &Value::Number(new))?;
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

    fn get_property(&mut self, receiver: &Value, key: &JsString) -> Result<Value, RuntimeError> {
        match receiver {
            Value::Object(id) => Ok(self.heap.get(*id, key)?),
            Value::String(string) => {
                if let Some(value) = string.own_property(key) {
                    return Ok(value);
                }
                let (_, prototype) = self.string_intrinsics()?;
                Ok(self.heap.get(prototype, key)?)
            }
            Value::Null | Value::Undefined => Err(RuntimeError::TypeError("cannot access a property of null or undefined".into())),
            _ => Err(RuntimeError::Unsupported("non-String primitive property access/boxing")),
        }
    }

    fn set_property(&mut self, receiver: &Value, key: &JsString, value: &Value) -> Result<(), RuntimeError> {
        // Current scripts are non-strict: primitive and read-only String
        // writes return the assignment value without changing storage.
        let Value::Object(object) = *receiver else { return Ok(()) };
        let stored = if key == "length" && self.heap.is_array(object)? { Value::Number(primitive::number(value)?) } else { value.clone() };
        self.with_roots(|heap| match heap.set(object, key, stored) {
            Err(HeapError::ReadOnlyProperty) => Ok(()),
            result => result,
        })
    }

    fn property_reference(&mut self) -> Result<(Value, JsString), RuntimeError> {
        let Value::String(key) = self.pop() else { unreachable!("compiler converts property keys to strings") };
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
        let function_prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let constructor = self.with_roots(|heap| heap.alloc_native_function(NativeFunction::String, function_prototype))?;
        let root = self.heap.root(constructor)?;
        let result = (|| {
            let prototype = self.with_roots(|heap| heap.alloc_string(JsString::default(), Some(object_prototype)))?;
            self.with_roots(|heap| heap.set(constructor, "prototype", Value::Object(prototype)))?;
            self.with_roots(|heap| heap.set(prototype, "constructor", Value::Object(constructor)))?;
            self.with_roots(|heap| heap.set(constructor, "name", Value::String("String".into())))?;
            self.with_roots(|heap| heap.set(constructor, "length", Value::Number(1.0)))?;
            self.install_native(function_prototype, function_prototype, "call", 1, NativeFunction::Call)?;
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
                self.with_roots(|heap| heap.set(prototype, alias, function))?;
            }
            Ok((constructor, prototype))
        })();
        match result {
            Ok(intrinsics) => {
                self.string_intrinsics = Some(intrinsics);
                Ok(intrinsics)
            }
            Err(error) => {
                self.heap.unroot(root)?;
                Err(error)
            }
        }
    }

    fn install_native(&mut self, owner: ObjectId, prototype: ObjectId, name: &str, length: u32, function: NativeFunction) -> Result<(), RuntimeError> {
        let id = self.with_roots(|heap| heap.alloc_native_function(function, prototype))?;
        self.with_roots(|heap| heap.set(id, "name", Value::String(name.into())))?;
        self.with_roots(|heap| heap.set(id, "length", Value::Number(f64::from(length))))?;
        self.with_roots(|heap| heap.set(owner, name, Value::Object(id)))?;
        Ok(())
    }

    fn call_native(&mut self, mut callee: Value, mut receiver: Value, mut args: Vec<Value>, construct: bool) -> Result<Value, RuntimeError> {
        loop {
            let function = if let Value::Object(id) = callee { self.heap.native_function(id)? } else { None };
            let Some(function) = function else { return Err(RuntimeError::TypeError("value is not callable".into())) };
            if construct && function != NativeFunction::String {
                return Err(RuntimeError::TypeError("value is not a constructor".into()));
            }
            return match function {
                NativeFunction::Call => {
                    callee = receiver;
                    receiver = native::argument(&args, 0).clone();
                    args = args.into_iter().skip(1).collect();
                    continue;
                }
                NativeFunction::String => {
                    let string = if args.is_empty() { JsString::default() } else { primitive::string(&self.unbox_string(native::argument(&args, 0))?)? };
                    self.check_string(&Value::String(string.clone()))?;
                    if construct {
                        let (_, prototype) = self.string_intrinsics()?;
                        Ok(Value::Object(self.with_roots(|heap| heap.alloc_string(string, Some(prototype)))?))
                    } else {
                        Ok(Value::String(string))
                    }
                }
                NativeFunction::FromCharCode | NativeFunction::FromCodePoint => native::from_codes(&args, function == NativeFunction::FromCodePoint, self.config.max_string_bytes),
                NativeFunction::Raw => self.string_raw(&args),
                NativeFunction::Split => self.string_split(&receiver, &args),
                NativeFunction::Replace | NativeFunction::ReplaceAll => self.string_replace(&receiver, &args, function == NativeFunction::ReplaceAll),
                NativeFunction::StringMethod(method) => native::string_method(method, &self.unbox_string(&receiver)?, &args, self.config.max_string_bytes),
            };
        }
    }

    fn unbox_string(&self, value: &Value) -> Result<Value, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(string) = self.heap.boxed_string(*id)? {
                return Ok(Value::String(string.clone()));
            }
        }
        Ok(value.clone())
    }

    fn string_receiver(&self, value: &Value) -> Result<JsString, RuntimeError> {
        if matches!(value, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError("String method requires a non-null receiver".into()));
        }
        primitive::string(&self.unbox_string(value)?)
    }

    fn string_raw(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let raw = self.get_property(native::argument(args, 0), &"raw".into())?;
        let count = native::length(&self.get_property(&raw, &"length".into())?)? as u64;
        let mut result = JsString::default();
        for index in 0..count {
            self.charge_step()?; // A huge array-like length with empty entries must still terminate.
            let literal = self.get_property(&raw, &index.to_string().into())?;
            native::append(&mut result, &primitive::string(&literal)?, self.config.max_string_bytes)?;
            if index + 1 < count {
                if let Some(substitution) = args.get(index as usize + 1) {
                    native::append(&mut result, &primitive::string(substitution)?, self.config.max_string_bytes)?;
                }
            }
        }
        Ok(Value::String(result))
    }

    fn string_split(&mut self, receiver: &Value, args: &[Value]) -> Result<Value, RuntimeError> {
        let string = self.string_receiver(receiver)?;
        let separator = native::argument(args, 0);
        let limit = native::argument(args, 1);
        let limit = if matches!(limit, Value::Undefined) { u32::MAX } else { native::uint32(limit)? };
        // Even a zero limit converts the separator first (§22.1.3.23).
        let search = primitive::string(separator)?;
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
        let string = self.string_receiver(receiver)?;
        let search = primitive::string(native::argument(args, 0))?;
        let replace = native::argument(args, 1);
        let callable = if let Value::Object(id) = replace { self.heap.native_function(*id)?.is_some() } else { false };
        let template = if callable { JsString::default() } else { primitive::string(replace)? };
        let mut result = JsString::default();
        let mut end = 0;
        let mut next = 0;
        if search.len() <= string.len() {
            while let Some(position) = (next..=string.len() - search.len()).find(|&index| string.as_code_units()[index..].starts_with(search.as_code_units())) {
                self.charge_step()?;
                let replacement = if callable {
                    let value = self.call_native(replace.clone(), Value::Undefined, vec![Value::String(search.clone()), Value::Number(position as f64), Value::String(string.clone())], false)?;
                    primitive::string(&value)?
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

    fn binary(&mut self, operation: impl FnOnce(&Self, Value, Value) -> Result<Value, RuntimeError>) -> Result<(), RuntimeError> {
        let right = self.pop();
        let left = self.pop();
        let value = operation(self, left, right)?;
        self.stack.push(value);
        Ok(())
    }

    fn numeric(&mut self, operation: fn(f64, f64) -> f64) -> Result<(), RuntimeError> {
        self.binary(|_, a, b| Ok(Value::Number(operation(primitive::number(&a)?, primitive::number(&b)?))))
    }

    fn relational(&mut self, accept: fn(Ordering) -> bool) -> Result<(), RuntimeError> {
        self.binary(|_, a, b| Ok(Value::Bool(primitive::compare(&a, &b)?.is_some_and(accept))))
    }

    fn add(&self, left: Value, right: Value) -> Result<Value, RuntimeError> {
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
