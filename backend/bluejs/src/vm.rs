// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded operand-stack interpreter for compiler-owned bytecode.
//! Allocating instructions root all VM-held objects around heap
//! safepoints. The collector itself additionally protects store inputs.

use crate::primitive;
use crate::{Bytecode, Heap, HeapConfig, HeapError, ObjectId, Opcode, RootId, Value};
use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy)]
pub struct VmConfig {
    pub heap: HeapConfig,
    /// Per-execute limit, including branches and scope instructions.
    pub instruction_budget: u64,
    /// Maximum UTF-8 bytes in any one runtime string (not total RSS).
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
    result_root: Option<RootId>,
    stack: Vec<Value>,
    // None is a lexical binding's uninitialized state, never JS undefined.
    bindings: Vec<Option<Value>>,
    completion: Value,
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
        Ok(Self { config, heap, object_prototype, array_prototype, result_root: None, stack: Vec::new(), bindings: Vec::new(), completion: Value::Undefined })
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
        if matches!(value, Value::String(s) if s.len() > self.config.max_string_bytes) {
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
        let mut remaining = self.config.instruction_budget;
        loop {
            if remaining == 0 {
                return Err(RuntimeError::InstructionLimit);
            }
            remaining -= 1;
            let instruction = code.instruction(pc).expect("compiler emits valid instruction boundaries");
            let operand = instruction.operand.unwrap_or(0) as usize;
            pc += instruction.opcode.width();
            match instruction.opcode {
                Opcode::Constant => {
                    self.check_string(&code.constants[operand])?;
                    self.stack.push(code.constants[operand].clone());
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
                    return Err(RuntimeError::ReferenceError(name.clone()));
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
                        _ => Value::String(primitive::type_name(&arg).into()),
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
                Opcode::GetProperty => {
                    let (object, key) = self.property_reference()?;
                    self.stack.push(self.heap.get(object, &key)?);
                }
                Opcode::SetProperty => {
                    let value = self.pop();
                    let (object, key) = self.property_reference()?;
                    self.set_property(object, &key, &value)?;
                    self.stack.push(value);
                }
                Opcode::UpdateProperty => {
                    let (object, key) = self.property_reference()?;
                    let old = primitive::number(&self.heap.get(object, &key)?)?;
                    let new = if operand & 1 == 0 { old + 1.0 } else { old - 1.0 };
                    self.set_property(object, &key, &Value::Number(new))?;
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

    fn set_property(&mut self, object: ObjectId, key: &str, value: &Value) -> Result<(), RuntimeError> {
        let stored = if key == "length" && self.heap.is_array(object)? { Value::Number(primitive::number(value)?) } else { value.clone() };
        self.with_roots(|heap| heap.set(object, key, stored))
    }

    fn property_reference(&mut self) -> Result<(ObjectId, String), RuntimeError> {
        let Value::String(key) = self.pop() else { unreachable!("compiler converts property keys to strings") };
        match self.pop() {
            Value::Object(id) => Ok((id, key)),
            Value::Null | Value::Undefined => Err(RuntimeError::TypeError("cannot access a property of null or undefined".into())),
            _ => Err(RuntimeError::Unsupported("primitive property access/boxing")),
        }
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
            if a.len().checked_add(b.len()).is_none_or(|len| len > self.config.max_string_bytes) {
                return Err(RuntimeError::StringLimit { limit: self.config.max_string_bytes });
            }
            a.push_str(&b);
            Ok(Value::String(a))
        } else {
            Ok(Value::Number(primitive::number(&left)? + primitive::number(&right)?))
        }
    }
}
