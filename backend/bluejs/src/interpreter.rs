// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The BlueJS fixed-width operand-stack interpreter.
//!
//! It only dispatches [`crate::Instruction`] words. Parsing and AST lowering
//! are complete before a [`Vm`] sees a program, which keeps later profiling or
//! optimizing tiers additive instead of replacing a tree-walking evaluator.

use crate::bytecode::*;
use crate::compiler::{CompileError, compile};
use crate::heap::{
    EnvironmentId, Heap, HeapError, HeapLimits, HeapRoots, HostObjectKind, ObjectAccess, ObjectId,
    ObjectKind,
};
use crate::parser::{ParseError, parse};
use crate::value::Value;
use crate::{ScriptCapability, ScriptGatekeeperHook};
use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant};

const NATIVE_CONSOLE_LOG: u32 = 1;
const NATIVE_CONSOLE_WARN: u32 = 2;
const NATIVE_CONSOLE_ERROR: u32 = 3;
const NATIVE_ARRAY_PUSH: u32 = 10;
const NATIVE_ARRAY_POP: u32 = 11;
const NATIVE_ARRAY_INDEX_OF: u32 = 12;
const NATIVE_ARRAY_INCLUDES: u32 = 13;
const NATIVE_ARRAY_JOIN: u32 = 14;
const NATIVE_ARRAY_FOR_EACH: u32 = 15;
const NATIVE_ARRAY_MAP: u32 = 16;
const NATIVE_ARRAY_FILTER: u32 = 17;
const NATIVE_ARRAY_SLICE: u32 = 18;
const NATIVE_STRING_SLICE: u32 = 20;
const NATIVE_STRING_SPLIT: u32 = 21;
const NATIVE_STRING_INDEX_OF: u32 = 22;
const NATIVE_STRING_INCLUDES: u32 = 23;
const NATIVE_STRING_TRIM: u32 = 24;
const NATIVE_STRING_UPPER: u32 = 25;
const NATIVE_STRING_LOWER: u32 = 26;
const NATIVE_DOCUMENT_GET_ELEMENT_BY_ID: u32 = 40;
const NATIVE_DOCUMENT_CREATE_ELEMENT: u32 = 41;
const NATIVE_DOCUMENT_CREATE_TEXT_NODE: u32 = 42;
const NATIVE_NODE_APPEND_CHILD: u32 = 43;
const NATIVE_DOCUMENT_QUERY_SELECTOR: u32 = 44;
const NATIVE_DOCUMENT_QUERY_SELECTOR_ALL: u32 = 45;
const NATIVE_NODE_INSERT_BEFORE: u32 = 100;
const NATIVE_NODE_REMOVE_CHILD: u32 = 101;
const NATIVE_NODE_REMOVE: u32 = 102;
const NATIVE_NODE_GET_ATTRIBUTE: u32 = 103;
const NATIVE_NODE_SET_ATTRIBUTE: u32 = 104;
const NATIVE_NODE_REMOVE_ATTRIBUTE: u32 = 105;
const NATIVE_CLASS_LIST_ADD: u32 = 110;
const NATIVE_CLASS_LIST_REMOVE: u32 = 111;
const NATIVE_CLASS_LIST_TOGGLE: u32 = 112;
const NATIVE_CLASS_LIST_CONTAINS: u32 = 113;
const NATIVE_NODE_ADD_EVENT_LISTENER: u32 = 120;
const NATIVE_NODE_REMOVE_EVENT_LISTENER: u32 = 121;
const NATIVE_EVENT_PREVENT_DEFAULT: u32 = 122;
const NATIVE_SET_TIMEOUT: u32 = 130;
const NATIVE_CLEAR_TIMEOUT: u32 = 131;
const NATIVE_OBJECT_KEYS: u32 = 50;
const NATIVE_OBJECT_VALUES: u32 = 51;
const NATIVE_OBJECT_ENTRIES: u32 = 52;
const NATIVE_MATH_FLOOR: u32 = 60;
const NATIVE_MATH_CEIL: u32 = 61;
const NATIVE_MATH_ROUND: u32 = 62;
const NATIVE_MATH_ABS: u32 = 63;
const NATIVE_MATH_MAX: u32 = 64;
const NATIVE_MATH_MIN: u32 = 65;
const NATIVE_MATH_RANDOM: u32 = 66;
const NATIVE_ERROR: u32 = 70;
const NATIVE_TYPE_ERROR: u32 = 71;
const NATIVE_RANGE_ERROR: u32 = 72;

/// The bounded instruction budget is a core-side safety mechanism for a
/// runaway script. Process isolation remains the primary crash boundary; the
/// budget gives the embedding host a deterministic abort point as well.
pub const DEFAULT_INSTRUCTION_BUDGET: usize = 1_000_000;

/// The narrow host boundary the VM uses for DOM objects. BlueJS values never
/// contain a `Page`/`Document`; a production implementation of this trait is
/// the long-lived `blueice_ipc::script` client.
pub trait DomHost: fmt::Debug {
    fn get_element_by_id(&mut self, id: &str) -> Result<Option<u64>, String>;
    fn query_selector(&mut self, selector: &str) -> Result<Option<u64>, String>;
    fn query_selector_all(&mut self, selector: &str) -> Result<Vec<u64>, String>;
    fn create_element(&mut self, tag_name: &str) -> Result<u64, String>;
    fn create_text_node(&mut self, data: &str) -> Result<u64, String>;
    fn append_child(&mut self, parent: u64, child: u64) -> Result<(), String>;
    fn insert_before(
        &mut self,
        parent: u64,
        child: u64,
        reference: Option<u64>,
    ) -> Result<(), String>;
    fn remove_child(&mut self, parent: u64, child: u64) -> Result<(), String>;
    fn remove(&mut self, node: u64) -> Result<(), String>;
    fn get_text_content(&mut self, node: u64) -> Result<String, String>;
    fn set_text_content(&mut self, node: u64, value: &str) -> Result<(), String>;
    fn get_attribute(&mut self, node: u64, name: &str) -> Result<Option<String>, String>;
    fn set_attribute(&mut self, node: u64, name: &str, value: &str) -> Result<(), String>;
    fn remove_attribute(&mut self, node: u64, name: &str) -> Result<(), String>;
    fn inner_html(&mut self, node: u64) -> Result<String, String>;
    fn get_style_property(&mut self, node: u64, property: &str) -> Result<String, String>;
    fn set_style_property(&mut self, node: u64, property: &str, value: &str) -> Result<(), String>;
    fn class_list(
        &mut self,
        node: u64,
        operation: ClassListOperation,
        class_name: &str,
    ) -> Result<bool, String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassListOperation {
    Add,
    Remove,
    Toggle,
    Contains,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VmError {
    Parse(ParseError),
    Compile(CompileError),
    Runtime(String),
    Thrown(Value),
    InstructionBudgetExceeded,
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "BlueJS parse error: {}", error.message),
            Self::Compile(error) => write!(f, "BlueJS compile error: {}", error.message),
            Self::Runtime(message) => write!(f, "BlueJS runtime error: {message}"),
            Self::Thrown(value) => write!(f, "uncaught BlueJS value: {value:?}"),
            Self::InstructionBudgetExceeded => f.write_str("BlueJS instruction budget exceeded"),
        }
    }
}

impl std::error::Error for VmError {}

impl From<HeapError> for VmError {
    fn from(error: HeapError) -> Self {
        Self::Runtime(error.to_string())
    }
}

#[derive(Debug)]
struct Frame {
    module: usize,
    function: usize,
    ip: usize,
    stack: Vec<Value>,
    environment: EnvironmentId,
    this: Value,
    handlers: Vec<usize>,
    constructor_this: Option<Value>,
}

#[derive(Debug, Clone)]
struct Timer {
    due: Instant,
    callback: Value,
    args: Vec<Value>,
}

trait RuntimeGatekeeper {
    fn before_host_call(
        &mut self,
        capability: ScriptCapability,
        callee: &str,
    ) -> Result<(), String>;
}

struct GatekeeperAdapter<H>(H);

impl<H> RuntimeGatekeeper for GatekeeperAdapter<H>
where
    H: ScriptGatekeeperHook,
    H::Error: fmt::Display,
{
    fn before_host_call(
        &mut self,
        capability: ScriptCapability,
        callee: &str,
    ) -> Result<(), String> {
        self.0
            .before_host_call(capability, callee)
            .map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, Copy)]
enum BindMode {
    Assign,
    Var,
    Let,
    Const,
}

/// A stateful BlueJS realm. A realm owns its heap and global environment, so
/// separate tabs/processes never share object handles or globals.
pub struct Vm {
    heap: Heap,
    global: EnvironmentId,
    modules: Vec<BytecodeModule>,
    frames: Vec<Frame>,
    completion: Value,
    last_finished: Option<Value>,
    output: Vec<String>,
    temporary_roots: Vec<Value>,
    instruction_budget: usize,
    executed_instructions: usize,
    random_state: u64,
    dom: Option<Box<dyn DomHost>>,
    gatekeeper: Option<Box<dyn RuntimeGatekeeper>>,
    event_listeners: HashMap<(u64, String), Vec<Value>>,
    active_event_default_prevented: Option<bool>,
    timers: HashMap<u64, Timer>,
    next_timer_id: u64,
}

impl fmt::Debug for Vm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vm")
            .field("heap", &self.heap)
            .field("global", &self.global)
            .field("frames", &self.frames)
            .field("module_count", &self.modules.len())
            .field("completion", &self.completion)
            .field("output", &self.output)
            .field("instruction_budget", &self.instruction_budget)
            .field("executed_instructions", &self.executed_instructions)
            .field("has_dom_host", &self.dom.is_some())
            .finish()
    }
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

impl Vm {
    pub fn new() -> Self {
        Self::with_heap_limits(HeapLimits::default())
            .expect("default BlueJS heap can construct its global environment")
    }

    pub fn with_heap_limits(limits: HeapLimits) -> Result<Self, VmError> {
        let mut heap = Heap::new(limits);
        let global = heap.allocate_environment(None)?;
        let mut vm = Self {
            heap,
            global,
            modules: Vec::new(),
            frames: Vec::new(),
            completion: Value::Undefined,
            last_finished: None,
            output: Vec::new(),
            temporary_roots: Vec::new(),
            instruction_budget: DEFAULT_INSTRUCTION_BUDGET,
            executed_instructions: 0,
            // A per-realm PRNG state keeps Math.random independent across
            // tabs without adding another ambient process-global dependency.
            random_state: 0x7d4a_7c15_9e37_79b9,
            dom: None,
            gatekeeper: None,
            event_listeners: HashMap::new(),
            active_event_default_prevented: None,
            timers: HashMap::new(),
            next_timer_id: 1,
        };
        vm.install_intrinsics()?;
        Ok(vm)
    }

    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    pub fn global_environment(&self) -> EnvironmentId {
        self.global
    }

    pub fn set_instruction_budget(&mut self, budget: usize) {
        self.instruction_budget = budget;
    }

    /// Installs the Phase 7 execution-time safety hook. Preflight analysis is
    /// useful but cannot prove data-dependent calls; this guard runs directly
    /// before BlueJS crosses a DOM host boundary.
    pub fn set_gatekeeper_hook<H>(&mut self, hook: H)
    where
        H: ScriptGatekeeperHook + 'static,
        H::Error: fmt::Display,
    {
        self.gatekeeper = Some(Box::new(GatekeeperAdapter(hook)));
    }

    /// Installs one core-backed DOM binding realm. Replacing it is allowed for
    /// a newly navigated tab, but the old proxy objects remain opaque values
    /// and cannot dereference a `Page` directly.
    pub fn set_dom_host(&mut self, host: Box<dyn DomHost>) -> Result<(), VmError> {
        self.dom = Some(host);
        let document = self.allocate_object(ObjectKind::HostObject {
            kind: HostObjectKind::Document,
            id: 0,
        })?;
        self.heap.define_binding(
            self.global,
            "document".to_string(),
            Value::Object(document),
            false,
        )?;
        let window = self.allocate_object(ObjectKind::Ordinary)?;
        self.heap.define_binding(
            self.global,
            "window".to_string(),
            Value::Object(window),
            false,
        )?;
        self.heap
            .set_property(window, "document".to_string(), Value::Object(document))?;
        self.heap.set_property(
            window,
            "console".to_string(),
            self.heap.get_binding(self.global, "console")?,
        )?;
        for (name, id) in [
            ("setTimeout", NATIVE_SET_TIMEOUT),
            ("clearTimeout", NATIVE_CLEAR_TIMEOUT),
        ] {
            let function = self.allocate_object(ObjectKind::NativeFunction {
                id,
                name: name.to_string(),
                arity: if id == NATIVE_SET_TIMEOUT { 2 } else { 1 },
            })?;
            let value = Value::Object(function);
            self.heap
                .define_binding(self.global, name.to_string(), value.clone(), false)?;
            self.heap.set_property(window, name.to_string(), value)?;
        }
        Ok(())
    }

    /// Runs listeners registered for a core-dispatched DOM event. The caller
    /// is core's event/default-action boundary, so the returned bit controls
    /// whether a link navigation or form-like default action may proceed.
    pub fn dispatch_event(&mut self, node: u64, event_type: &str) -> Result<bool, VmError> {
        if !self.frames.is_empty() {
            return Err(VmError::Runtime(
                "cannot dispatch a DOM event while BlueJS is already executing".to_string(),
            ));
        }
        let callbacks = self
            .event_listeners
            .get(&(node, event_type.to_string()))
            .cloned()
            .unwrap_or_default();
        if callbacks.is_empty() {
            return Ok(false);
        }

        let checkpoint = self.temporary_roots.len();
        self.temporary_roots.extend(callbacks.iter().cloned());
        let result = (|| {
            let target = self.node_value(node)?;
            self.temporary_roots.push(target.clone());
            let event = self.allocate_object(ObjectKind::Ordinary)?;
            let event = Value::Object(event);
            self.temporary_roots.push(event.clone());
            let event_object = self.expect_object(event.clone())?;
            self.heap.set_property(
                event_object,
                "type".to_string(),
                Value::String(event_type.to_string()),
            )?;
            self.heap
                .set_property(event_object, "target".to_string(), target.clone())?;
            self.heap
                .set_property(event_object, "currentTarget".to_string(), target.clone())?;
            self.heap.set_property(
                event_object,
                "defaultPrevented".to_string(),
                Value::Bool(false),
            )?;
            let prevent_default = self.allocate_bound_native(
                NATIVE_EVENT_PREVENT_DEFAULT,
                "preventDefault".to_string(),
                0,
                event.clone(),
            )?;
            self.heap.set_property(
                event_object,
                "preventDefault".to_string(),
                Value::Object(prevent_default),
            )?;

            let previous = self.active_event_default_prevented.replace(false);
            let callback_result = callbacks.into_iter().try_for_each(|callback| {
                self.invoke_callback_with_this(callback, target.clone(), vec![event.clone()])
                    .map(|_| ())
            });
            let prevented = self.active_event_default_prevented.unwrap_or(false);
            self.active_event_default_prevented = previous;
            callback_result.map(|_| prevented)
        })();
        self.temporary_roots.truncate(checkpoint);
        result
    }

    pub fn has_event_listeners(&self, node: u64, event_type: &str) -> bool {
        self.event_listeners
            .get(&(node, event_type.to_string()))
            .is_some_and(|listeners| !listeners.is_empty())
    }

    /// Runs all one-shot timers whose deadline has elapsed. This is called by
    /// the core-owned event-loop tick; timer callbacks therefore keep the
    /// same process isolation and DOM-request completion barrier as events.
    pub fn run_due_timers(&mut self) -> Result<bool, VmError> {
        if !self.frames.is_empty() {
            return Err(VmError::Runtime(
                "cannot run timers while BlueJS is already executing".to_string(),
            ));
        }
        let now = Instant::now();
        let mut due: Vec<u64> = self
            .timers
            .iter()
            .filter_map(|(id, timer)| (timer.due <= now).then_some(*id))
            .collect();
        due.sort_unstable();
        let mut ran = false;
        for id in due {
            let Some(timer) = self.timers.remove(&id) else {
                continue;
            };
            ran = true;
            self.invoke_callback_with_this(timer.callback, Value::Undefined, timer.args)?;
        }
        Ok(ran)
    }

    pub fn output(&self) -> &[String] {
        &self.output
    }

    pub fn take_output(&mut self) -> Vec<String> {
        std::mem::take(&mut self.output)
    }

    /// Node-like printable representation used by the standalone shell. It is
    /// intentionally realm-aware (arrays render their elements) rather than a
    /// `Display` impl on [`Value`], whose object handles have no meaning
    /// without this VM's heap.
    pub fn format_value(&self, value: &Value) -> String {
        self.display_value(value)
    }

    pub fn evaluate(&mut self, source: &str) -> Result<Value, VmError> {
        let program = parse(source).map_err(VmError::Parse)?;
        let module = compile(&program).map_err(VmError::Compile)?;
        self.run(module)
    }

    pub fn run(&mut self, module: BytecodeModule) -> Result<Value, VmError> {
        if !self.frames.is_empty() {
            return Err(VmError::Runtime(
                "cannot replace a module while BlueJS is executing".to_string(),
            ));
        }
        let module_id = self.modules.len();
        self.modules.push(module);
        self.completion = Value::Undefined;
        self.last_finished = None;
        self.executed_instructions = 0;
        self.push_frame(module_id, 0, self.global, Value::Undefined, Vec::new(), None)?;
        self.run_until_depth(0)
    }

    fn install_intrinsics(&mut self) -> Result<(), VmError> {
        self.heap.define_binding(
            self.global,
            "undefined".to_string(),
            Value::Undefined,
            false,
        )?;
        let console = self.allocate_object(ObjectKind::Ordinary)?;
        self.heap.define_binding(
            self.global,
            "console".to_string(),
            Value::Object(console),
            false,
        )?;
        for (name, id) in [
            ("log", NATIVE_CONSOLE_LOG),
            ("warn", NATIVE_CONSOLE_WARN),
            ("error", NATIVE_CONSOLE_ERROR),
        ] {
            let function = self.allocate_object(ObjectKind::NativeFunction {
                id,
                name: name.to_string(),
                arity: 0,
            })?;
            self.heap
                .set_property(console, name.to_string(), Value::Object(function))?;
        }

        let object = self.allocate_object(ObjectKind::Ordinary)?;
        self.heap.define_binding(
            self.global,
            "Object".to_string(),
            Value::Object(object),
            false,
        )?;
        for (name, id) in [
            ("keys", NATIVE_OBJECT_KEYS),
            ("values", NATIVE_OBJECT_VALUES),
            ("entries", NATIVE_OBJECT_ENTRIES),
        ] {
            self.install_native_property(object, name, id, 1)?;
        }

        let math = self.allocate_object(ObjectKind::Ordinary)?;
        self.heap
            .define_binding(self.global, "Math".to_string(), Value::Object(math), false)?;
        for (name, id, arity) in [
            ("floor", NATIVE_MATH_FLOOR, 1),
            ("ceil", NATIVE_MATH_CEIL, 1),
            ("round", NATIVE_MATH_ROUND, 1),
            ("abs", NATIVE_MATH_ABS, 1),
            ("max", NATIVE_MATH_MAX, 0),
            ("min", NATIVE_MATH_MIN, 0),
            ("random", NATIVE_MATH_RANDOM, 0),
        ] {
            self.install_native_property(math, name, id, arity)?;
        }

        for (name, id) in [
            ("Error", NATIVE_ERROR),
            ("TypeError", NATIVE_TYPE_ERROR),
            ("RangeError", NATIVE_RANGE_ERROR),
        ] {
            let error = self.allocate_object(ObjectKind::NativeFunction {
                id,
                name: name.to_string(),
                arity: 1,
            })?;
            self.heap
                .define_binding(self.global, name.to_string(), Value::Object(error), false)?;
        }
        Ok(())
    }

    fn install_native_property(
        &mut self,
        object: ObjectId,
        name: &str,
        id: u32,
        arity: usize,
    ) -> Result<(), VmError> {
        let function = self.allocate_object(ObjectKind::NativeFunction {
            id,
            name: name.to_string(),
            arity,
        })?;
        self.heap
            .set_property(object, name.to_string(), Value::Object(function))?;
        Ok(())
    }

    fn run_until_depth(&mut self, depth: usize) -> Result<Value, VmError> {
        while self.frames.len() > depth {
            if self.executed_instructions >= self.instruction_budget {
                return Err(VmError::InstructionBudgetExceeded);
            }
            self.executed_instructions += 1;
            let instruction = self.next_instruction()?;
            self.execute_instruction(instruction)?;
        }
        Ok(self.last_finished.take().unwrap_or(Value::Undefined))
    }

    fn next_instruction(&mut self) -> Result<Instruction, VmError> {
        let frame = self
            .frames
            .last_mut()
            .ok_or_else(|| VmError::Runtime("no active BlueJS frame".to_string()))?;
        let instruction = self
            .modules
            .get(frame.module)
            .ok_or_else(|| VmError::Runtime("no active BlueJS module".to_string()))?
            .functions
            .get(frame.function)
            .and_then(|function| function.code.get(frame.ip))
            .copied()
            .ok_or_else(|| VmError::Runtime("instruction pointer escaped bytecode".to_string()))?;
        frame.ip += 1;
        Ok(instruction)
    }

    fn execute_instruction(&mut self, instruction: Instruction) -> Result<(), VmError> {
        let operand = instruction.operand() as usize;
        match instruction.opcode() {
            Opcode::Nop => {}
            Opcode::LoadConstant => self.push(self.constant_value(operand)?)?,
            Opcode::LoadUndefined => self.push(Value::Undefined)?,
            Opcode::LoadNull => self.push(Value::Null)?,
            Opcode::LoadTrue => self.push(Value::Bool(true))?,
            Opcode::LoadFalse => self.push(Value::Bool(false))?,
            Opcode::LoadThis => self.push(self.current_frame()?.this.clone())?,
            Opcode::LoadBinding => {
                let name = self.constant_string(operand)?;
                let environment = self.current_frame()?.environment;
                let value = self.heap.get_binding(environment, name)?;
                self.push(value)?;
            }
            Opcode::StoreBinding => {
                let name = self.constant_string(operand)?.to_string();
                let value = self.pop()?;
                let environment = self.current_frame()?.environment;
                self.heap.assign_binding(environment, &name, value)?;
            }
            Opcode::DeclareVar | Opcode::DeclareLet | Opcode::DeclareConst => {
                let name = self.constant_string(operand)?.to_string();
                let value = self.pop()?;
                let environment = self.current_frame()?.environment;
                self.heap.define_binding(
                    environment,
                    name,
                    value,
                    instruction.opcode() != Opcode::DeclareConst,
                )?;
            }
            Opcode::EnterScope => {
                let parent = self.current_frame()?.environment;
                let child = self.allocate_environment(Some(parent))?;
                self.current_frame_mut()?.environment = child;
            }
            Opcode::LeaveScope => {
                let child = self.current_frame()?.environment;
                let parent = self.heap.environment_parent(child)?.ok_or_else(|| {
                    VmError::Runtime("attempted to leave the global scope".to_string())
                })?;
                self.current_frame_mut()?.environment = parent;
            }
            Opcode::Pop => {
                self.pop()?;
            }
            Opcode::SetCompletion => {
                self.completion = self.pop()?;
            }
            Opcode::Dup => {
                self.push(self.peek()?.clone())?;
            }
            Opcode::Dup2 => {
                let frame = self.current_frame()?;
                let second = frame
                    .stack
                    .get(frame.stack.len().saturating_sub(2))
                    .cloned()
                    .ok_or_else(|| VmError::Runtime("Dup2 needs two stack values".to_string()))?;
                let first =
                    frame.stack.last().cloned().ok_or_else(|| {
                        VmError::Runtime("Dup2 needs two stack values".to_string())
                    })?;
                self.push(second)?;
                self.push(first)?;
            }
            Opcode::Rotate3 => {
                let third = self.pop()?;
                let second = self.pop()?;
                let first = self.pop()?;
                self.push(third)?;
                self.push(first)?;
                self.push(second)?;
            }
            Opcode::MakeArray => {
                let object = self.allocate_object(ObjectKind::Array {
                    elements: Vec::new(),
                })?;
                self.push(Value::Object(object))?;
            }
            Opcode::ArrayPush => {
                let value = self.pop()?;
                let array = self.expect_object(self.peek()?.clone())?;
                self.heap.array_push(array, value)?;
            }
            Opcode::ArrayHole => {
                let array = self.expect_object(self.peek()?.clone())?;
                self.heap.array_push(array, Value::Undefined)?;
            }
            Opcode::ArraySpread => {
                let source = self.pop()?;
                let values = self.iterable_values(source)?;
                let array = self.expect_object(self.peek()?.clone())?;
                for value in values {
                    self.heap.array_push(array, value)?;
                }
            }
            Opcode::MakeObject => {
                let object = self.allocate_object(ObjectKind::Ordinary)?;
                self.push(Value::Object(object))?;
            }
            Opcode::ObjectSet => {
                let value = self.pop()?;
                let key_value = self.pop()?;
                let key = self.property_key(key_value);
                let object = self.expect_object(self.peek()?.clone())?;
                self.heap.set_property(object, key, value)?;
            }
            Opcode::ObjectSpread => {
                let source = self.pop()?;
                let target = self.expect_object(self.peek()?.clone())?;
                let source = self.expect_object(source)?;
                for key in self.heap.own_property_keys(source)? {
                    let value = self.heap.get_property(source, &key)?;
                    self.heap.set_property(target, key, value)?;
                }
            }
            Opcode::MakeFunction => {
                let module = self.current_frame()?.module;
                let function = self.bytecode_function(module, operand)?;
                let environment = self.current_frame()?.environment;
                let object = self.allocate_object(ObjectKind::Function {
                    module: module as u32,
                    code: operand as u32,
                    environment,
                    arity: function.parameters.len(),
                })?;
                self.push(Value::Object(object))?;
            }
            Opcode::GetProperty => {
                let key_value = self.pop()?;
                let key = self.property_key(key_value);
                let object = self.pop()?;
                let value = self.get_property(object, &key)?;
                self.push(value)?;
            }
            Opcode::SetProperty => {
                let value = self.pop()?;
                let key_value = self.pop()?;
                let key = self.property_key(key_value);
                let object_value = self.pop()?;
                let object = self.expect_object(object_value)?;
                if matches!(self.heap.object_kind(object)?, ObjectKind::Array { .. })
                    && array_index(&key).is_some()
                {
                    let index = array_index(&key).expect("checked array index above");
                    self.heap.array_set(object, index, value.clone())?;
                } else if !self.set_host_property(object, &key, &value)? {
                    self.heap.set_property(object, key, value.clone())?;
                }
                self.push(value)?;
            }
            Opcode::SetPropertyKeepOld => {
                let value = self.pop()?;
                let old = self.pop()?;
                let key_value = self.pop()?;
                let key = self.property_key(key_value);
                let object_value = self.pop()?;
                let object = self.expect_object(object_value)?;
                if matches!(self.heap.object_kind(object)?, ObjectKind::Array { .. })
                    && array_index(&key).is_some()
                {
                    let index = array_index(&key).expect("checked array index above");
                    self.heap.array_set(object, index, value)?;
                } else if !self.set_host_property(object, &key, &value)? {
                    self.heap.set_property(object, key, value)?;
                }
                self.push(old)?;
            }
            Opcode::UnaryNeg | Opcode::UnaryPos | Opcode::UnaryNot | Opcode::Typeof => {
                self.execute_unary(instruction.opcode())?
            }
            Opcode::Add
            | Opcode::Subtract
            | Opcode::Multiply
            | Opcode::Divide
            | Opcode::Remainder
            | Opcode::Equal
            | Opcode::NotEqual
            | Opcode::StrictEqual
            | Opcode::StrictNotEqual
            | Opcode::LessThan
            | Opcode::GreaterThan
            | Opcode::LessThanOrEqual
            | Opcode::GreaterThanOrEqual
            | Opcode::Instanceof
            | Opcode::In => self.execute_binary(instruction.opcode())?,
            Opcode::Jump => self.jump_to(operand)?,
            Opcode::JumpIfFalse => {
                if !self.peek()?.is_truthy() {
                    self.jump_to(operand)?;
                }
            }
            Opcode::JumpIfTrue => {
                if self.peek()?.is_truthy() {
                    self.jump_to(operand)?;
                }
            }
            Opcode::JumpIfTruePop => {
                if self.pop()?.is_truthy() {
                    self.jump_to(operand)?;
                }
            }
            Opcode::JumpIfNullish => {
                if self.peek()?.is_nullish() {
                    self.jump_to(operand)?;
                }
            }
            Opcode::JumpIfNotNullish => {
                if !self.peek()?.is_nullish() {
                    self.jump_to(operand)?;
                }
            }
            Opcode::Call => {
                let args = self.pop_arguments(operand)?;
                let callee = self.pop()?;
                self.call(callee, Value::Undefined, args, false)?;
            }
            Opcode::CallWithThis => {
                let args = self.pop_arguments(operand)?;
                let callee = self.pop()?;
                let this = self.pop()?;
                self.call(callee, this, args, false)?;
            }
            Opcode::Construct => {
                let args = self.pop_arguments(operand)?;
                let callee = self.pop()?;
                self.call(callee, Value::Undefined, args, true)?;
            }
            Opcode::CallSpread => {
                let argument_array = self.pop()?;
                let args = self.iterable_values(argument_array)?;
                let callee = self.pop()?;
                self.call(callee, Value::Undefined, args, false)?;
            }
            Opcode::CallWithThisSpread => {
                let argument_array = self.pop()?;
                let args = self.iterable_values(argument_array)?;
                let callee = self.pop()?;
                let this = self.pop()?;
                self.call(callee, this, args, false)?;
            }
            Opcode::ConstructSpread => {
                let argument_array = self.pop()?;
                let args = self.iterable_values(argument_array)?;
                let callee = self.pop()?;
                self.call(callee, Value::Undefined, args, true)?;
            }
            Opcode::Return => {
                let value = self.pop()?;
                self.finish_frame(value);
            }
            Opcode::Throw => {
                let value = self.pop()?;
                self.throw(value)?;
            }
            Opcode::TryBegin => self.current_frame_mut()?.handlers.push(operand),
            Opcode::TryEnd => {
                self.current_frame_mut()?
                    .handlers
                    .pop()
                    .ok_or_else(|| VmError::Runtime("TryEnd without TryBegin".to_string()))?;
            }
            Opcode::BindPattern
            | Opcode::DeclarePatternVar
            | Opcode::DeclarePatternLet
            | Opcode::DeclarePatternConst => {
                let value = self.pop()?;
                let pattern = self.pattern(operand)?;
                let mode = match instruction.opcode() {
                    Opcode::BindPattern => BindMode::Assign,
                    Opcode::DeclarePatternVar => BindMode::Var,
                    Opcode::DeclarePatternLet => BindMode::Let,
                    Opcode::DeclarePatternConst => BindMode::Const,
                    _ => unreachable!(),
                };
                let environment = self.current_frame()?.environment;
                self.bind_pattern(&pattern, value, mode, environment)?;
            }
            Opcode::IteratorStartOf | Opcode::IteratorStartIn => {
                let source = self.pop()?;
                let values = if instruction.opcode() == Opcode::IteratorStartOf {
                    self.iterable_values(source)?
                } else {
                    let object = self.expect_object(source)?;
                    self.heap
                        .own_property_keys(object)?
                        .into_iter()
                        .map(Value::String)
                        .collect()
                };
                let iterator = self.allocate_object(ObjectKind::Iterator { values, next: 0 })?;
                self.push(Value::Object(iterator))?;
            }
            Opcode::IteratorNext => {
                let iterator = self.expect_object(self.peek()?.clone())?;
                if let Some(value) = self.heap.iterator_next(iterator)? {
                    self.push(value)?;
                } else {
                    self.pop()?;
                    self.jump_to(operand)?;
                }
            }
            Opcode::Halt => self.finish_frame(self.completion.clone()),
        }
        Ok(())
    }

    fn execute_unary(&mut self, opcode: Opcode) -> Result<(), VmError> {
        let value = self.pop()?;
        let result = match opcode {
            Opcode::UnaryNeg => Value::Number(-self.to_number(&value)),
            Opcode::UnaryPos => Value::Number(self.to_number(&value)),
            Opcode::UnaryNot => Value::Bool(!value.is_truthy()),
            Opcode::Typeof => Value::String(self.type_of(&value)),
            _ => unreachable!(),
        };
        self.push(result)
    }

    fn execute_binary(&mut self, opcode: Opcode) -> Result<(), VmError> {
        let right = self.pop()?;
        let left = self.pop()?;
        let result = match opcode {
            Opcode::Add => {
                if matches!(left, Value::String(_)) || matches!(right, Value::String(_)) {
                    Value::String(format!(
                        "{}{}",
                        self.display_value(&left),
                        self.display_value(&right)
                    ))
                } else {
                    Value::Number(self.to_number(&left) + self.to_number(&right))
                }
            }
            Opcode::Subtract => Value::Number(self.to_number(&left) - self.to_number(&right)),
            Opcode::Multiply => Value::Number(self.to_number(&left) * self.to_number(&right)),
            Opcode::Divide => Value::Number(self.to_number(&left) / self.to_number(&right)),
            Opcode::Remainder => Value::Number(self.to_number(&left) % self.to_number(&right)),
            Opcode::Equal => Value::Bool(self.loose_equal(&left, &right)),
            Opcode::NotEqual => Value::Bool(!self.loose_equal(&left, &right)),
            Opcode::StrictEqual => Value::Bool(self.strict_equal(&left, &right)),
            Opcode::StrictNotEqual => Value::Bool(!self.strict_equal(&left, &right)),
            Opcode::LessThan => Value::Bool(self.to_number(&left) < self.to_number(&right)),
            Opcode::GreaterThan => Value::Bool(self.to_number(&left) > self.to_number(&right)),
            Opcode::LessThanOrEqual => Value::Bool(self.to_number(&left) <= self.to_number(&right)),
            Opcode::GreaterThanOrEqual => {
                Value::Bool(self.to_number(&left) >= self.to_number(&right))
            }
            Opcode::Instanceof => Value::Bool(false), // prototypes are outside the Phase 2 MVP object cut.
            Opcode::In => {
                let object = self.expect_object(right)?;
                let key = self.property_key(left);
                Value::Bool(self.heap.own_property_keys(object)?.contains(&key))
            }
            _ => unreachable!(),
        };
        self.push(result)
    }

    fn call(
        &mut self,
        callee: Value,
        this: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<(), VmError> {
        let object = self.expect_object(callee)?;
        match self.heap.object_kind(object)?.clone() {
            ObjectKind::Function {
                module,
                code,
                environment,
                ..
            } => {
                let constructor_this = if construct {
                    Some(Value::Object(self.allocate_object(ObjectKind::Ordinary)?))
                } else {
                    None
                };
                let this = constructor_this.clone().unwrap_or(this);
                self.push_frame(
                    module as usize,
                    code as usize,
                    environment,
                    this,
                    args,
                    constructor_this,
                )?;
            }
            ObjectKind::NativeFunction { id, .. } => {
                let value = self.call_native(id, None, args)?;
                self.push(value)?;
            }
            ObjectKind::BoundNativeFunction { id, receiver, .. } => {
                let value = self.call_native(id, Some(receiver), args)?;
                self.push(value)?;
            }
            _ => {
                return Err(VmError::Runtime(
                    "attempted to call a non-function value".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn call_native(
        &mut self,
        id: u32,
        receiver: Option<Value>,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let root_count = self.temporary_roots.len();
        if let Some(receiver) = &receiver {
            self.temporary_roots.push(receiver.clone());
        }
        self.temporary_roots.extend(args.iter().cloned());
        let result = self.call_native_inner(id, receiver, args);
        self.temporary_roots.truncate(root_count);
        result
    }

    fn call_native_inner(
        &mut self,
        id: u32,
        receiver: Option<Value>,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        if let Some((capability, callee)) = host_call_capability(id) {
            self.authorize_host_call(capability, callee)?;
        }
        match id {
            NATIVE_CONSOLE_LOG | NATIVE_CONSOLE_WARN | NATIVE_CONSOLE_ERROR => {
                self.output.push(
                    args.iter()
                        .map(|value| self.display_value(value))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                Ok(Value::Undefined)
            }
            NATIVE_ARRAY_PUSH => {
                let array = self.expect_object(receiver.ok_or_else(|| {
                    VmError::Runtime("array method needs a receiver".to_string())
                })?)?;
                for value in args {
                    self.heap.array_push(array, value)?;
                }
                Ok(Value::Number(self.heap.array_elements(array)?.len() as f64))
            }
            NATIVE_ARRAY_POP => {
                let array = self.expect_object(receiver.ok_or_else(|| {
                    VmError::Runtime("array method needs a receiver".to_string())
                })?)?;
                Ok(self.heap.array_pop(array)?)
            }
            NATIVE_ARRAY_INDEX_OF => {
                let array = self.expect_object(receiver.ok_or_else(|| {
                    VmError::Runtime("array method needs a receiver".to_string())
                })?)?;
                let needle = args.first().cloned().unwrap_or(Value::Undefined);
                let index = self
                    .heap
                    .array_elements(array)?
                    .iter()
                    .position(|value| self.strict_equal(value, &needle))
                    .map_or(-1.0, |index| index as f64);
                Ok(Value::Number(index))
            }
            NATIVE_ARRAY_INCLUDES => {
                let array = self.expect_object(receiver.ok_or_else(|| {
                    VmError::Runtime("array method needs a receiver".to_string())
                })?)?;
                let needle = args.first().cloned().unwrap_or(Value::Undefined);
                Ok(Value::Bool(
                    self.heap
                        .array_elements(array)?
                        .iter()
                        .any(|value| self.strict_equal(value, &needle)),
                ))
            }
            NATIVE_ARRAY_JOIN => {
                let array = self.expect_object(receiver.ok_or_else(|| {
                    VmError::Runtime("array method needs a receiver".to_string())
                })?)?;
                let separator = args
                    .first()
                    .map_or_else(|| ",".to_string(), |value| self.display_value(value));
                Ok(Value::String(
                    self.heap
                        .array_elements(array)?
                        .iter()
                        .map(|value| self.display_value(value))
                        .collect::<Vec<_>>()
                        .join(&separator),
                ))
            }
            NATIVE_ARRAY_FOR_EACH | NATIVE_ARRAY_MAP | NATIVE_ARRAY_FILTER => {
                self.array_callback_method(id, receiver, args)
            }
            NATIVE_ARRAY_SLICE => self.array_slice(receiver, args),
            NATIVE_STRING_SLICE => self.string_slice(receiver, args),
            NATIVE_STRING_SPLIT => self.string_split(receiver, args),
            NATIVE_STRING_INDEX_OF => self.string_index_of(receiver, args),
            NATIVE_STRING_INCLUDES => {
                let text = self.expect_string_receiver(receiver)?;
                let needle = args
                    .first()
                    .map_or_else(String::new, |value| self.display_value(value));
                Ok(Value::Bool(text.contains(&needle)))
            }
            NATIVE_STRING_TRIM => Ok(Value::String(
                self.expect_string_receiver(receiver)?.trim().to_string(),
            )),
            NATIVE_STRING_UPPER => Ok(Value::String(
                self.expect_string_receiver(receiver)?.to_uppercase(),
            )),
            NATIVE_STRING_LOWER => Ok(Value::String(
                self.expect_string_receiver(receiver)?.to_lowercase(),
            )),
            NATIVE_DOCUMENT_GET_ELEMENT_BY_ID => {
                let id = args
                    .first()
                    .map_or_else(String::new, |value| self.display_value(value));
                let node = self
                    .dom_mut()?
                    .get_element_by_id(&id)
                    .map_err(VmError::Runtime)?;
                node.map_or(Ok(Value::Null), |node| self.node_value(node))
            }
            NATIVE_DOCUMENT_QUERY_SELECTOR => {
                let selector = args
                    .first()
                    .map_or_else(String::new, |value| self.display_value(value));
                let node = self
                    .dom_mut()?
                    .query_selector(&selector)
                    .map_err(VmError::Runtime)?;
                node.map_or(Ok(Value::Null), |node| self.node_value(node))
            }
            NATIVE_DOCUMENT_QUERY_SELECTOR_ALL => {
                let selector = args
                    .first()
                    .map_or_else(String::new, |value| self.display_value(value));
                let nodes = self
                    .dom_mut()?
                    .query_selector_all(&selector)
                    .map_err(VmError::Runtime)?;
                let mut values = Vec::with_capacity(nodes.len());
                for node in nodes {
                    values.push(self.node_value(node)?);
                }
                let checkpoint = self.temporary_roots.len();
                self.temporary_roots.extend(values.iter().cloned());
                let result = self.allocate_object(ObjectKind::Array { elements: values });
                self.temporary_roots.truncate(checkpoint);
                Ok(Value::Object(result?))
            }
            NATIVE_DOCUMENT_CREATE_ELEMENT => {
                let tag = args
                    .first()
                    .map_or_else(String::new, |value| self.display_value(value));
                let node = self
                    .dom_mut()?
                    .create_element(&tag)
                    .map_err(VmError::Runtime)?;
                self.node_value(node)
            }
            NATIVE_DOCUMENT_CREATE_TEXT_NODE => {
                let data = args
                    .first()
                    .map_or_else(String::new, |value| self.display_value(value));
                let node = self
                    .dom_mut()?
                    .create_text_node(&data)
                    .map_err(VmError::Runtime)?;
                self.node_value(node)
            }
            NATIVE_NODE_APPEND_CHILD => {
                let parent = self.host_node_id(receiver.ok_or_else(|| {
                    VmError::Runtime("appendChild needs a node receiver".to_string())
                })?)?;
                let child = self.host_node_id(args.first().cloned().unwrap_or(Value::Undefined))?;
                self.dom_mut()?
                    .append_child(parent, child)
                    .map_err(VmError::Runtime)?;
                self.node_value(child)
            }
            NATIVE_NODE_INSERT_BEFORE => {
                let parent = self.host_node_id(receiver.ok_or_else(|| {
                    VmError::Runtime("insertBefore needs a node receiver".to_string())
                })?)?;
                let child = self.host_node_id(args.first().cloned().unwrap_or(Value::Undefined))?;
                let reference = match args.get(1).cloned().unwrap_or(Value::Null) {
                    Value::Null | Value::Undefined => None,
                    value => Some(self.host_node_id(value)?),
                };
                self.dom_mut()?
                    .insert_before(parent, child, reference)
                    .map_err(VmError::Runtime)?;
                self.node_value(child)
            }
            NATIVE_NODE_REMOVE_CHILD => {
                let parent = self.host_node_id(receiver.ok_or_else(|| {
                    VmError::Runtime("removeChild needs a node receiver".to_string())
                })?)?;
                let child = self.host_node_id(args.first().cloned().unwrap_or(Value::Undefined))?;
                self.dom_mut()?
                    .remove_child(parent, child)
                    .map_err(VmError::Runtime)?;
                self.node_value(child)
            }
            NATIVE_NODE_REMOVE => {
                let node = self.host_node_id(receiver.ok_or_else(|| {
                    VmError::Runtime("remove needs a node receiver".to_string())
                })?)?;
                self.dom_mut()?.remove(node).map_err(VmError::Runtime)?;
                Ok(Value::Undefined)
            }
            NATIVE_NODE_GET_ATTRIBUTE => {
                let node = self.host_node_id(receiver.ok_or_else(|| {
                    VmError::Runtime("getAttribute needs a node receiver".to_string())
                })?)?;
                let name = args
                    .first()
                    .map_or_else(String::new, |value| self.display_value(value));
                let value = self
                    .dom_mut()?
                    .get_attribute(node, &name)
                    .map_err(VmError::Runtime)?;
                Ok(value.map_or(Value::Null, Value::String))
            }
            NATIVE_NODE_SET_ATTRIBUTE => {
                let node = self.host_node_id(receiver.ok_or_else(|| {
                    VmError::Runtime("setAttribute needs a node receiver".to_string())
                })?)?;
                let name = args
                    .first()
                    .map_or_else(String::new, |value| self.display_value(value));
                let value = args.get(1).map_or_else(
                    || "undefined".to_string(),
                    |value| self.display_value(value),
                );
                self.dom_mut()?
                    .set_attribute(node, &name, &value)
                    .map_err(VmError::Runtime)?;
                Ok(Value::Undefined)
            }
            NATIVE_NODE_REMOVE_ATTRIBUTE => {
                let node = self.host_node_id(receiver.ok_or_else(|| {
                    VmError::Runtime("removeAttribute needs a node receiver".to_string())
                })?)?;
                let name = args
                    .first()
                    .map_or_else(String::new, |value| self.display_value(value));
                self.dom_mut()?
                    .remove_attribute(node, &name)
                    .map_err(VmError::Runtime)?;
                Ok(Value::Undefined)
            }
            NATIVE_CLASS_LIST_ADD
            | NATIVE_CLASS_LIST_REMOVE
            | NATIVE_CLASS_LIST_TOGGLE
            | NATIVE_CLASS_LIST_CONTAINS => {
                let node = self.host_id(
                    receiver.ok_or_else(|| {
                        VmError::Runtime("classList method needs a receiver".to_string())
                    })?,
                    HostObjectKind::ClassList,
                )?;
                let class_name = args
                    .first()
                    .map_or_else(String::new, |value| self.display_value(value));
                let operation = match id {
                    NATIVE_CLASS_LIST_ADD => ClassListOperation::Add,
                    NATIVE_CLASS_LIST_REMOVE => ClassListOperation::Remove,
                    NATIVE_CLASS_LIST_TOGGLE => ClassListOperation::Toggle,
                    NATIVE_CLASS_LIST_CONTAINS => ClassListOperation::Contains,
                    _ => unreachable!(),
                };
                Ok(Value::Bool(
                    self.dom_mut()?
                        .class_list(node, operation, &class_name)
                    .map_err(VmError::Runtime)?,
                ))
            }
            NATIVE_NODE_ADD_EVENT_LISTENER | NATIVE_NODE_REMOVE_EVENT_LISTENER => {
                let node = self.host_node_id(receiver.ok_or_else(|| {
                    VmError::Runtime("event listener method needs a node receiver".to_string())
                })?)?;
                let event_type = args
                    .first()
                    .map_or_else(String::new, |value| self.display_value(value));
                let callback = args.get(1).cloned().ok_or_else(|| {
                    VmError::Runtime("addEventListener requires a callback".to_string())
                })?;
                if !self.is_callable(&callback)? {
                    return Err(VmError::Runtime(
                        "event listener callback is not callable".to_string(),
                    ));
                }
                let listeners = self.event_listeners.entry((node, event_type)).or_default();
                if id == NATIVE_NODE_ADD_EVENT_LISTENER {
                    if !listeners.iter().any(|registered| registered == &callback) {
                        listeners.push(callback);
                    }
                } else {
                    listeners.retain(|registered| registered != &callback);
                }
                Ok(Value::Undefined)
            }
            NATIVE_EVENT_PREVENT_DEFAULT => {
                let Some(default_prevented) = self.active_event_default_prevented.as_mut() else {
                    return Err(VmError::Runtime(
                        "preventDefault called outside DOM event dispatch".to_string(),
                    ));
                };
                *default_prevented = true;
                if let Some(Value::Object(event)) = receiver {
                    self.heap.set_property(
                        event,
                        "defaultPrevented".to_string(),
                        Value::Bool(true),
                    )?;
                }
                Ok(Value::Undefined)
            }
            NATIVE_SET_TIMEOUT => {
                let callback = args.first().cloned().ok_or_else(|| {
                    VmError::Runtime("setTimeout requires a callback".to_string())
                })?;
                if !self.is_callable(&callback)? {
                    return Err(VmError::Runtime(
                        "setTimeout callback is not callable".to_string(),
                    ));
                }
                let delay = args
                    .get(1)
                    .map(|value| self.to_number(value))
                    .unwrap_or(0.0);
                let delay = if delay.is_finite() && delay > 0.0 {
                    delay.min(u64::MAX as f64) as u64
                } else {
                    0
                };
                let timer_id = self.next_timer_id;
                self.next_timer_id = self.next_timer_id.saturating_add(1).max(1);
                self.timers.insert(
                    timer_id,
                    Timer {
                        due: Instant::now() + Duration::from_millis(delay),
                        callback,
                        args: args.into_iter().skip(2).collect(),
                    },
                );
                Ok(Value::Number(timer_id as f64))
            }
            NATIVE_CLEAR_TIMEOUT => {
                let timer_id = args
                    .first()
                    .map(|value| self.to_number(value))
                    .filter(|id| id.is_finite() && *id >= 0.0)
                    .unwrap_or(0.0) as u64;
                self.timers.remove(&timer_id);
                Ok(Value::Undefined)
            }
            NATIVE_OBJECT_KEYS | NATIVE_OBJECT_VALUES | NATIVE_OBJECT_ENTRIES => {
                self.object_entries_method(id, args)
            }
            NATIVE_MATH_FLOOR => Ok(Value::Number(self.first_number(&args).floor())),
            NATIVE_MATH_CEIL => Ok(Value::Number(self.first_number(&args).ceil())),
            NATIVE_MATH_ROUND => Ok(Value::Number((self.first_number(&args) + 0.5).floor())),
            NATIVE_MATH_ABS => Ok(Value::Number(self.first_number(&args).abs())),
            NATIVE_MATH_MAX => Ok(Value::Number(
                args.iter()
                    .map(|value| self.to_number(value))
                    .fold(f64::NEG_INFINITY, f64::max),
            )),
            NATIVE_MATH_MIN => Ok(Value::Number(
                args.iter()
                    .map(|value| self.to_number(value))
                    .fold(f64::INFINITY, f64::min),
            )),
            NATIVE_MATH_RANDOM => Ok(Value::Number(self.random_unit_interval())),
            NATIVE_ERROR | NATIVE_TYPE_ERROR | NATIVE_RANGE_ERROR => {
                let error = self.allocate_object(ObjectKind::Ordinary)?;
                let message = args
                    .first()
                    .map_or_else(String::new, |value| self.display_value(value));
                let name = match id {
                    NATIVE_ERROR => "Error",
                    NATIVE_TYPE_ERROR => "TypeError",
                    NATIVE_RANGE_ERROR => "RangeError",
                    _ => unreachable!(),
                };
                self.heap.set_property(
                    error,
                    "name".to_string(),
                    Value::String(name.to_string()),
                )?;
                self.heap
                    .set_property(error, "message".to_string(), Value::String(message))?;
                Ok(Value::Object(error))
            }
            _ => Err(VmError::Runtime(format!("unknown native function {id}"))),
        }
    }

    fn first_number(&self, args: &[Value]) -> f64 {
        args.first().map_or(f64::NAN, |value| self.to_number(value))
    }

    fn random_unit_interval(&mut self) -> f64 {
        let mut state = self.random_state;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.random_state = state;
        (state >> 11) as f64 / (1_u64 << 53) as f64
    }

    fn array_callback_method(
        &mut self,
        id: u32,
        receiver: Option<Value>,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let array = self.expect_object(
            receiver
                .ok_or_else(|| VmError::Runtime("array method needs a receiver".to_string()))?,
        )?;
        let callback = args.first().cloned().ok_or_else(|| {
            VmError::Runtime("array callback method needs a callback".to_string())
        })?;
        let values = self.heap.array_elements(array)?.to_vec();
        let receiver = Value::Object(array);
        let mut mapped = Vec::with_capacity(values.len());

        for (index, value) in values.into_iter().enumerate() {
            let checkpoint = self.temporary_roots.len();
            self.temporary_roots.extend(mapped.iter().cloned());
            let callback_value = self.invoke_callback(
                callback.clone(),
                vec![value.clone(), Value::Number(index as f64), receiver.clone()],
            )?;
            self.temporary_roots.truncate(checkpoint);
            match id {
                NATIVE_ARRAY_FOR_EACH => {}
                NATIVE_ARRAY_MAP => mapped.push(callback_value),
                NATIVE_ARRAY_FILTER if callback_value.is_truthy() => mapped.push(value),
                NATIVE_ARRAY_FILTER => {}
                _ => unreachable!("only array callback native ids reach this method"),
            }
        }

        if id == NATIVE_ARRAY_FOR_EACH {
            Ok(Value::Undefined)
        } else {
            let checkpoint = self.temporary_roots.len();
            self.temporary_roots.extend(mapped.iter().cloned());
            let result = self.allocate_object(ObjectKind::Array { elements: mapped });
            self.temporary_roots.truncate(checkpoint);
            Ok(Value::Object(result?))
        }
    }

    fn array_slice(&mut self, receiver: Option<Value>, args: Vec<Value>) -> Result<Value, VmError> {
        let array = self.expect_object(
            receiver
                .ok_or_else(|| VmError::Runtime("array method needs a receiver".to_string()))?,
        )?;
        let values = self.heap.array_elements(array)?.to_vec();
        let start = self.normalized_index(args.first(), values.len(), 0);
        let end = self.normalized_index(args.get(1), values.len(), values.len());
        let elements = values[start..end.max(start)].to_vec();
        let checkpoint = self.temporary_roots.len();
        self.temporary_roots.extend(elements.iter().cloned());
        let result = self.allocate_object(ObjectKind::Array { elements });
        self.temporary_roots.truncate(checkpoint);
        Ok(Value::Object(result?))
    }

    fn object_entries_method(&mut self, id: u32, args: Vec<Value>) -> Result<Value, VmError> {
        let object = self.expect_object(args.first().cloned().unwrap_or(Value::Undefined))?;
        let mut properties = if matches!(self.heap.object_kind(object)?, ObjectKind::Array { .. }) {
            self.heap
                .array_elements(object)?
                .iter()
                .enumerate()
                .map(|(index, value)| (index.to_string(), value.clone()))
                .collect::<Vec<_>>()
        } else {
            self.heap
                .own_property_keys(object)?
                .into_iter()
                .map(|key| {
                    let value = self.heap.get_property(object, &key)?;
                    Ok((key, value))
                })
                .collect::<Result<Vec<_>, HeapError>>()?
        };
        properties.sort_by(|left, right| left.0.cmp(&right.0));

        let mut result = Vec::with_capacity(properties.len());
        for (key, value) in properties {
            let output = match id {
                NATIVE_OBJECT_KEYS => Value::String(key),
                NATIVE_OBJECT_VALUES => value,
                NATIVE_OBJECT_ENTRIES => {
                    let checkpoint = self.temporary_roots.len();
                    self.temporary_roots.extend(result.iter().cloned());
                    self.temporary_roots.push(Value::String(key.clone()));
                    self.temporary_roots.push(value.clone());
                    let pair = self.allocate_object(ObjectKind::Array {
                        elements: vec![Value::String(key), value],
                    });
                    self.temporary_roots.truncate(checkpoint);
                    Value::Object(pair?)
                }
                _ => unreachable!("only Object.* native ids reach this method"),
            };
            result.push(output);
        }
        let checkpoint = self.temporary_roots.len();
        self.temporary_roots.extend(result.iter().cloned());
        let result = self.allocate_object(ObjectKind::Array { elements: result });
        self.temporary_roots.truncate(checkpoint);
        Ok(Value::Object(result?))
    }

    fn invoke_callback(&mut self, callback: Value, args: Vec<Value>) -> Result<Value, VmError> {
        self.invoke_callback_with_this(callback, Value::Undefined, args)
    }

    fn invoke_callback_with_this(
        &mut self,
        callback: Value,
        this: Value,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let object = self.expect_object(callback.clone())?;
        match self.heap.object_kind(object)?.clone() {
            ObjectKind::NativeFunction { id, .. } => return self.call_native(id, None, args),
            ObjectKind::BoundNativeFunction { id, receiver, .. } => {
                return self.call_native(id, Some(receiver), args);
            }
            ObjectKind::Function { .. } => {}
            _ => {
                return Err(VmError::Runtime(
                    "event listener callback is not callable".to_string(),
                ));
            }
        }
        let depth = self.frames.len();
        self.call(callback, this, args, false)?;
        if depth == 0 {
            return self.run_until_depth(0);
        }
        if self.frames.len() == depth {
            return self.pop();
        }
        let value = self.run_until_depth(depth)?;
        let stack_value = self.pop()?;
        debug_assert_eq!(value, stack_value);
        Ok(value)
    }

    fn normalized_index(&self, value: Option<&Value>, length: usize, default: usize) -> usize {
        let number = value.map_or(default as f64, |value| self.to_number(value));
        if number.is_nan() {
            return 0;
        }
        if number.is_infinite() {
            return if number.is_sign_negative() { 0 } else { length };
        }
        let index = number.trunc() as isize;
        if index < 0 {
            length.saturating_sub(index.unsigned_abs())
        } else {
            (index as usize).min(length)
        }
    }

    fn string_slice(
        &mut self,
        receiver: Option<Value>,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let text = self.expect_string_receiver(receiver)?;
        let chars = text.chars().collect::<Vec<_>>();
        let start = args
            .first()
            .map_or(0, |value| self.to_number(value) as isize)
            .max(0) as usize;
        let end = args
            .get(1)
            .map_or(chars.len() as isize, |value| self.to_number(value) as isize)
            .max(0) as usize;
        Ok(Value::String(
            chars[start.min(chars.len())..end.min(chars.len()).max(start.min(chars.len()))]
                .iter()
                .collect(),
        ))
    }

    fn string_split(
        &mut self,
        receiver: Option<Value>,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let text = self.expect_string_receiver(receiver)?;
        let separator = args
            .first()
            .map_or_else(String::new, |value| self.display_value(value));
        let parts = if separator.is_empty() {
            text.chars()
                .map(|character| Value::String(character.to_string()))
                .collect()
        } else {
            text.split(&separator)
                .map(|part| Value::String(part.to_string()))
                .collect()
        };
        let object = self.allocate_object(ObjectKind::Array { elements: parts })?;
        Ok(Value::Object(object))
    }

    fn string_index_of(
        &mut self,
        receiver: Option<Value>,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let text = self.expect_string_receiver(receiver)?;
        let needle = args
            .first()
            .map_or_else(String::new, |value| self.display_value(value));
        Ok(Value::Number(
            text.find(&needle).map_or(-1.0, |index| index as f64),
        ))
    }

    fn expect_string_receiver(&self, receiver: Option<Value>) -> Result<String, VmError> {
        match receiver.unwrap_or(Value::Undefined) {
            Value::String(value) => Ok(value),
            _ => Err(VmError::Runtime(
                "string method needs a string receiver".to_string(),
            )),
        }
    }

    fn push_frame(
        &mut self,
        module: usize,
        function: usize,
        parent: EnvironmentId,
        this: Value,
        args: Vec<Value>,
        constructor_this: Option<Value>,
    ) -> Result<(), VmError> {
        let metadata = self.bytecode_function(module, function)?;
        // Classic top-level scripts share the realm's global environment.
        // Nested functions still get their own child scope; without this
        // distinction a declaration from the first `<script>` disappears when
        // its temporary top-level frame returns before the second runs.
        let environment = if function == 0 && parent == self.global {
            parent
        } else {
            self.allocate_environment(Some(parent))?
        };
        // Root the environment before binding parameters: a default expression
        // may allocate enough to collect, and this frame is its live owner.
        self.frames.push(Frame {
            module,
            function,
            ip: 0,
            stack: Vec::new(),
            environment,
            this,
            handlers: Vec::new(),
            constructor_this,
        });
        for (index, parameter) in metadata.parameters.iter().enumerate() {
            let value = if parameter.rest {
                let rest = self.allocate_object(ObjectKind::Array {
                    elements: args[index..].to_vec(),
                })?;
                Value::Object(rest)
            } else {
                args.get(index).cloned().unwrap_or(Value::Undefined)
            };
            let value = if parameter.rest {
                value
            } else {
                self.apply_default(value, parameter.default_function, environment)?
            };
            self.bind_pattern(&parameter.pattern, value, BindMode::Let, environment)?;
        }
        Ok(())
    }

    fn bind_pattern(
        &mut self,
        pattern: &BindingPattern,
        value: Value,
        mode: BindMode,
        environment: EnvironmentId,
    ) -> Result<(), VmError> {
        match pattern {
            BindingPattern::Identifier(name) => self.bind_name(environment, name, value, mode)?,
            BindingPattern::Array(elements) => {
                let values = match value {
                    Value::Object(id) => self
                        .heap
                        .array_elements(id)
                        .map(|values| values.to_vec())
                        .unwrap_or_default(),
                    Value::String(text) => text
                        .chars()
                        .map(|character| Value::String(character.to_string()))
                        .collect(),
                    _ => Vec::new(),
                };
                let mut next = 0;
                for element in elements {
                    let Some(element) = element else {
                        next += 1;
                        continue;
                    };
                    let element_value = if element.rest {
                        let rest = self.allocate_object(ObjectKind::Array {
                            elements: values[next..].to_vec(),
                        })?;
                        next = values.len();
                        Value::Object(rest)
                    } else {
                        let value = values.get(next).cloned().unwrap_or(Value::Undefined);
                        next += 1;
                        self.apply_default(value, element.default_function, environment)?
                    };
                    self.bind_pattern(&element.pattern, element_value, mode, environment)?;
                }
            }
            BindingPattern::Object(properties) => {
                let object = self.expect_object(value)?;
                let mut consumed = Vec::new();
                for property in properties {
                    match property {
                        ObjectBindingProperty::KeyValue {
                            key,
                            value,
                            default_function,
                        } => {
                            let key = self.binding_key(key, environment)?;
                            let property_value = self.heap.get_property(object, &key)?;
                            let property_value =
                                self.apply_default(property_value, *default_function, environment)?;
                            consumed.push(key);
                            self.bind_pattern(value, property_value, mode, environment)?;
                        }
                        ObjectBindingProperty::Rest(pattern) => {
                            let rest = self.allocate_object(ObjectKind::Ordinary)?;
                            for key in self.heap.own_property_keys(object)? {
                                if !consumed.contains(&key) {
                                    self.heap.set_property(
                                        rest,
                                        key.clone(),
                                        self.heap.get_property(object, &key)?,
                                    )?;
                                }
                            }
                            self.bind_pattern(pattern, Value::Object(rest), mode, environment)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn bind_name(
        &mut self,
        environment: EnvironmentId,
        name: &str,
        value: Value,
        mode: BindMode,
    ) -> Result<(), VmError> {
        match mode {
            BindMode::Assign => self.heap.assign_binding(environment, name, value)?,
            BindMode::Var | BindMode::Let => {
                self.heap
                    .define_binding(environment, name.to_string(), value, true)?
            }
            BindMode::Const => {
                self.heap
                    .define_binding(environment, name.to_string(), value, false)?
            }
        }
        Ok(())
    }

    fn binding_key(
        &mut self,
        key: &BindingKey,
        environment: EnvironmentId,
    ) -> Result<String, VmError> {
        match key {
            BindingKey::Static(key) => Ok(key.clone()),
            BindingKey::ComputedFunction(function) => {
                let value = self.run_helper(*function as usize, environment)?;
                Ok(self.property_key(value))
            }
        }
    }

    fn apply_default(
        &mut self,
        value: Value,
        default: Option<u32>,
        environment: EnvironmentId,
    ) -> Result<Value, VmError> {
        if value.is_nullish() {
            if let Some(function) = default {
                return self.run_helper(function as usize, environment);
            }
        }
        Ok(value)
    }

    fn run_helper(&mut self, function: usize, parent: EnvironmentId) -> Result<Value, VmError> {
        let depth = self.frames.len();
        let module = self.current_frame()?.module;
        self.push_frame(module, function, parent, Value::Undefined, Vec::new(), None)?;
        let value = self.run_until_depth(depth)?;
        let stack_value = self.pop()?;
        debug_assert_eq!(value, stack_value);
        Ok(value)
    }

    fn get_property(&mut self, object: Value, key: &str) -> Result<Value, VmError> {
        if let Value::String(text) = &object {
            if key == "length" {
                return Ok(Value::Number(text.chars().count() as f64));
            }
            if let Some(id) = string_method(key) {
                return self.bound_native(id, key, object, 0);
            }
            return Ok(Value::Undefined);
        }
        let id = self.expect_object(object.clone())?;
        if let ObjectKind::HostObject { kind, id: host_id } = self.heap.object_kind(id)?.clone() {
            return self.get_host_property(kind, host_id, key, object);
        }
        if self.heap.has_own_property(id, key)? {
            return Ok(self.heap.get_property(id, key)?);
        }
        if matches!(self.heap.object_kind(id)?, ObjectKind::Array { .. }) {
            if key == "length" {
                return Ok(Value::Number(self.heap.array_elements(id)?.len() as f64));
            }
            if let Some(index) = array_index(key) {
                return Ok(self
                    .heap
                    .array_elements(id)?
                    .get(index)
                    .cloned()
                    .unwrap_or(Value::Undefined));
            }
            if let Some(native) = array_method(key) {
                return self.bound_native(native, key, object, 0);
            }
        }
        Ok(Value::Undefined)
    }

    fn bound_native(
        &mut self,
        id: u32,
        name: &str,
        receiver: Value,
        arity: usize,
    ) -> Result<Value, VmError> {
        let object = self.allocate_bound_native(id, name.to_string(), arity, receiver)?;
        Ok(Value::Object(object))
    }

    fn get_host_property(
        &mut self,
        kind: HostObjectKind,
        id: u64,
        key: &str,
        receiver: Value,
    ) -> Result<Value, VmError> {
        if matches!(
            (kind, key),
            (
                HostObjectKind::Node,
                "textContent" | "innerHTML" | "id" | "className" | "value" | "checked"
            ) | (HostObjectKind::Style, _)
        ) {
            self.authorize_host_call(ScriptCapability::DomRead, "node.property.get")?;
        }
        match (kind, key) {
            (HostObjectKind::Document, "getElementById") => {
                self.bound_native(NATIVE_DOCUMENT_GET_ELEMENT_BY_ID, key, receiver, 1)
            }
            (HostObjectKind::Document, "createElement") => {
                self.bound_native(NATIVE_DOCUMENT_CREATE_ELEMENT, key, receiver, 1)
            }
            (HostObjectKind::Document, "createTextNode") => {
                self.bound_native(NATIVE_DOCUMENT_CREATE_TEXT_NODE, key, receiver, 1)
            }
            (HostObjectKind::Document, "querySelector") => {
                self.bound_native(NATIVE_DOCUMENT_QUERY_SELECTOR, key, receiver, 1)
            }
            (HostObjectKind::Document, "querySelectorAll") => {
                self.bound_native(NATIVE_DOCUMENT_QUERY_SELECTOR_ALL, key, receiver, 1)
            }
            (HostObjectKind::Node, "appendChild") => {
                self.bound_native(NATIVE_NODE_APPEND_CHILD, key, receiver, 1)
            }
            (HostObjectKind::Node, "insertBefore") => {
                self.bound_native(NATIVE_NODE_INSERT_BEFORE, key, receiver, 2)
            }
            (HostObjectKind::Node, "removeChild") => {
                self.bound_native(NATIVE_NODE_REMOVE_CHILD, key, receiver, 1)
            }
            (HostObjectKind::Node, "remove") => {
                self.bound_native(NATIVE_NODE_REMOVE, key, receiver, 0)
            }
            (HostObjectKind::Node, "getAttribute") => {
                self.bound_native(NATIVE_NODE_GET_ATTRIBUTE, key, receiver, 1)
            }
            (HostObjectKind::Node, "setAttribute") => {
                self.bound_native(NATIVE_NODE_SET_ATTRIBUTE, key, receiver, 2)
            }
            (HostObjectKind::Node, "removeAttribute") => {
                self.bound_native(NATIVE_NODE_REMOVE_ATTRIBUTE, key, receiver, 1)
            }
            (HostObjectKind::Node, "addEventListener") => {
                self.bound_native(NATIVE_NODE_ADD_EVENT_LISTENER, key, receiver, 2)
            }
            (HostObjectKind::Node, "removeEventListener") => {
                self.bound_native(NATIVE_NODE_REMOVE_EVENT_LISTENER, key, receiver, 2)
            }
            (HostObjectKind::Node, "textContent") => Ok(Value::String(
                self.dom_mut()?
                    .get_text_content(id)
                    .map_err(VmError::Runtime)?,
            )),
            (HostObjectKind::Node, "innerHTML") => Ok(Value::String(
                self.dom_mut()?.inner_html(id).map_err(VmError::Runtime)?,
            )),
            (HostObjectKind::Node, "id" | "className" | "value") => Ok(Value::String(
                self.dom_mut()?
                    .get_attribute(
                        id,
                        match key {
                            "className" => "class",
                            property => property,
                        },
                    )
                    .map_err(VmError::Runtime)?
                    .unwrap_or_default(),
            )),
            (HostObjectKind::Node, "checked") => Ok(Value::Bool(
                self.dom_mut()?
                    .get_attribute(id, "checked")
                    .map_err(VmError::Runtime)?
                    .is_some(),
            )),
            (HostObjectKind::Node, "classList") => self.host_value(HostObjectKind::ClassList, id),
            (HostObjectKind::Node, "style") => self.host_value(HostObjectKind::Style, id),
            (HostObjectKind::ClassList, "add") => {
                self.bound_native(NATIVE_CLASS_LIST_ADD, key, receiver, 1)
            }
            (HostObjectKind::ClassList, "remove") => {
                self.bound_native(NATIVE_CLASS_LIST_REMOVE, key, receiver, 1)
            }
            (HostObjectKind::ClassList, "toggle") => {
                self.bound_native(NATIVE_CLASS_LIST_TOGGLE, key, receiver, 1)
            }
            (HostObjectKind::ClassList, "contains") => {
                self.bound_native(NATIVE_CLASS_LIST_CONTAINS, key, receiver, 1)
            }
            (HostObjectKind::Style, property) => Ok(Value::String(
                self.dom_mut()?
                    .get_style_property(id, property)
                    .map_err(VmError::Runtime)?,
            )),
            _ => Ok(Value::Undefined),
        }
    }

    fn set_host_property(
        &mut self,
        object: ObjectId,
        key: &str,
        value: &Value,
    ) -> Result<bool, VmError> {
        let ObjectKind::HostObject { kind, id } = self.heap.object_kind(object)?.clone() else {
            return Ok(false);
        };
        if matches!(
            (kind, key),
            (
                HostObjectKind::Node,
                "textContent" | "id" | "className" | "value" | "checked"
            ) | (HostObjectKind::Style, _)
        ) {
            self.authorize_host_call(ScriptCapability::DomWrite, "node.property.set")?;
        }
        match (kind, key) {
            (HostObjectKind::Node, "textContent") => {
                let value = self.display_value(value);
                self.dom_mut()?
                    .set_text_content(id, &value)
                    .map_err(VmError::Runtime)?;
                Ok(true)
            }
            (HostObjectKind::Node, "id" | "className" | "value") => {
                let name = if key == "className" { "class" } else { key };
                let value = self.display_value(value);
                self.dom_mut()?
                    .set_attribute(id, name, &value)
                    .map_err(VmError::Runtime)?;
                Ok(true)
            }
            (HostObjectKind::Node, "checked") => {
                if value.is_truthy() {
                    self.dom_mut()?
                        .set_attribute(id, "checked", "")
                        .map_err(VmError::Runtime)?;
                } else {
                    self.dom_mut()?
                        .remove_attribute(id, "checked")
                        .map_err(VmError::Runtime)?;
                }
                Ok(true)
            }
            (HostObjectKind::Style, property) => {
                let value = self.display_value(value);
                self.dom_mut()?
                    .set_style_property(id, property, &value)
                    .map_err(VmError::Runtime)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn dom_mut(&mut self) -> Result<&mut (dyn DomHost + '_), VmError> {
        match self.dom.as_mut() {
            Some(host) => Ok(host.as_mut()),
            None => Err(VmError::Runtime(
                "DOM binding is unavailable in this BlueJS realm".to_string(),
            )),
        }
    }

    fn authorize_host_call(
        &mut self,
        capability: ScriptCapability,
        callee: &str,
    ) -> Result<(), VmError> {
        match self.gatekeeper.as_mut() {
            Some(gatekeeper) => gatekeeper
                .before_host_call(capability, callee)
                .map_err(VmError::Runtime),
            None => Ok(()),
        }
    }

    fn node_value(&mut self, id: u64) -> Result<Value, VmError> {
        let object = self.allocate_object(ObjectKind::HostObject {
            kind: HostObjectKind::Node,
            id,
        })?;
        Ok(Value::Object(object))
    }

    fn host_value(&mut self, kind: HostObjectKind, id: u64) -> Result<Value, VmError> {
        let object = self.allocate_object(ObjectKind::HostObject { kind, id })?;
        Ok(Value::Object(object))
    }

    fn host_node_id(&self, value: Value) -> Result<u64, VmError> {
        self.host_id(value, HostObjectKind::Node)
    }

    fn host_id(&self, value: Value, expected_kind: HostObjectKind) -> Result<u64, VmError> {
        let object = self.expect_object(value)?;
        match self.heap.object_kind(object)? {
            ObjectKind::HostObject { kind, id } if *kind == expected_kind => Ok(*id),
            _ => Err(VmError::Runtime(
                "operation requires the expected DOM host object".to_string(),
            )),
        }
    }

    fn iterable_values(&self, value: Value) -> Result<Vec<Value>, VmError> {
        match value {
            Value::Object(id) => Ok(self.heap.array_elements(id)?.to_vec()),
            Value::String(value) => Ok(value
                .chars()
                .map(|character| Value::String(character.to_string()))
                .collect()),
            _ => Err(VmError::Runtime(
                "value is not iterable in BlueJS MVP".to_string(),
            )),
        }
    }

    fn finish_frame(&mut self, value: Value) {
        let frame = self
            .frames
            .pop()
            .expect("a running BlueJS frame must exist");
        let value = match frame.constructor_this {
            Some(this) if !matches!(value, Value::Object(_)) => this,
            _ => value,
        };
        self.last_finished = Some(value.clone());
        if let Some(caller) = self.frames.last_mut() {
            caller.stack.push(value);
        }
    }

    fn throw(&mut self, value: Value) -> Result<(), VmError> {
        loop {
            let Some(frame) = self.frames.last_mut() else {
                return Err(VmError::Thrown(value));
            };
            if let Some(handler) = frame.handlers.pop() {
                frame.stack.clear();
                frame.stack.push(value);
                frame.ip = handler;
                return Ok(());
            }
            self.frames.pop();
        }
    }

    fn pop_arguments(&mut self, count: usize) -> Result<Vec<Value>, VmError> {
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.pop()?);
        }
        values.reverse();
        Ok(values)
    }

    fn constant_value(&self, index: usize) -> Result<Value, VmError> {
        match self.active_module()?.constants.get(index) {
            Some(Constant::Value(value)) => Ok(value.clone()),
            Some(Constant::String(value)) => Ok(Value::String(value.clone())),
            None => Err(VmError::Runtime(
                "invalid bytecode constant index".to_string(),
            )),
        }
    }

    fn constant_string(&self, index: usize) -> Result<&str, VmError> {
        match self.active_module()?.constants.get(index) {
            Some(Constant::String(value)) => Ok(value),
            _ => Err(VmError::Runtime(
                "bytecode operation expected a string constant".to_string(),
            )),
        }
    }

    fn pattern(&self, index: usize) -> Result<BindingPattern, VmError> {
        self.active_module()?
            .patterns
            .get(index)
            .cloned()
            .ok_or_else(|| VmError::Runtime("invalid binding pattern index".to_string()))
    }

    fn bytecode_function(
        &self,
        module: usize,
        index: usize,
    ) -> Result<BytecodeFunction, VmError> {
        self.modules
            .get(module)
            .and_then(|module| module.functions.get(index))
            .cloned()
            .ok_or_else(|| VmError::Runtime("invalid bytecode function index".to_string()))
    }

    fn current_frame(&self) -> Result<&Frame, VmError> {
        self.frames
            .last()
            .ok_or_else(|| VmError::Runtime("no active BlueJS frame".to_string()))
    }

    fn active_module(&self) -> Result<&BytecodeModule, VmError> {
        let module = self.current_frame()?.module;
        self.modules
            .get(module)
            .ok_or_else(|| VmError::Runtime("no active BlueJS module".to_string()))
    }

    fn current_frame_mut(&mut self) -> Result<&mut Frame, VmError> {
        self.frames
            .last_mut()
            .ok_or_else(|| VmError::Runtime("no active BlueJS frame".to_string()))
    }

    fn push(&mut self, value: Value) -> Result<(), VmError> {
        self.current_frame_mut()?.stack.push(value);
        Ok(())
    }

    fn pop(&mut self) -> Result<Value, VmError> {
        self.current_frame_mut()?
            .stack
            .pop()
            .ok_or_else(|| VmError::Runtime("operand stack underflow".to_string()))
    }

    fn peek(&self) -> Result<&Value, VmError> {
        self.current_frame()?
            .stack
            .last()
            .ok_or_else(|| VmError::Runtime("operand stack underflow".to_string()))
    }

    fn jump_to(&mut self, position: usize) -> Result<(), VmError> {
        self.current_frame_mut()?.ip = position;
        Ok(())
    }

    fn expect_object(&self, value: Value) -> Result<ObjectId, VmError> {
        match value {
            Value::Object(id) => Ok(id),
            value => Err(VmError::Runtime(format!(
                "operation requires an object, received {value:?}"
            ))),
        }
    }

    fn is_callable(&self, value: &Value) -> Result<bool, VmError> {
        let Value::Object(object) = value else {
            return Ok(false);
        };
        Ok(matches!(
            self.heap.object_kind(*object)?,
            ObjectKind::Function { .. }
                | ObjectKind::NativeFunction { .. }
                | ObjectKind::BoundNativeFunction { .. }
        ))
    }

    fn property_key(&self, value: Value) -> String {
        self.display_value(&value)
    }

    fn type_of(&self, value: &Value) -> String {
        match value {
            Value::Object(id)
                if matches!(
                    self.heap.object_kind(*id),
                    Ok(ObjectKind::Function { .. }
                        | ObjectKind::NativeFunction { .. }
                        | ObjectKind::BoundNativeFunction { .. })
                ) =>
            {
                "function".to_string()
            }
            _ => value.type_of().to_string(),
        }
    }

    fn to_number(&self, value: &Value) -> f64 {
        match value {
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            Value::Bool(value) => f64::from(*value),
            Value::Number(value) => *value,
            Value::String(value) => value.trim().parse().unwrap_or(f64::NAN),
            Value::Object(_) => f64::NAN,
        }
    }

    fn display_value(&self, value: &Value) -> String {
        match value {
            Value::Undefined => "undefined".to_string(),
            Value::Null => "null".to_string(),
            Value::Bool(value) => value.to_string(),
            Value::Number(value) if value.is_nan() => "NaN".to_string(),
            Value::Number(value) if value.fract() == 0.0 => format!("{value:.0}"),
            Value::Number(value) => value.to_string(),
            Value::String(value) => value.clone(),
            Value::Object(id) => match self.heap.object_kind(*id) {
                Ok(ObjectKind::Array { elements }) => elements
                    .iter()
                    .map(|value| self.display_value(value))
                    .collect::<Vec<_>>()
                    .join(","),
                _ => "[object Object]".to_string(),
            },
        }
    }

    fn strict_equal(&self, left: &Value, right: &Value) -> bool {
        match (left, right) {
            (Value::Number(left), Value::Number(right)) => {
                !left.is_nan() && !right.is_nan() && left == right
            }
            _ => left == right,
        }
    }

    fn loose_equal(&self, left: &Value, right: &Value) -> bool {
        if self.strict_equal(left, right) {
            return true;
        }
        match (left, right) {
            (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null) => true,
            (Value::Number(_), other) | (other, Value::Number(_)) => {
                !matches!(other, Value::Object(_)) && self.to_number(left) == self.to_number(right)
            }
            _ => false,
        }
    }

    fn roots(&self) -> (Vec<Value>, Vec<EnvironmentId>) {
        let mut values = Vec::new();
        let mut environments = vec![self.global];
        values.extend(self.temporary_roots.iter().cloned());
        for listeners in self.event_listeners.values() {
            values.extend(listeners.iter().cloned());
        }
        for timer in self.timers.values() {
            values.push(timer.callback.clone());
            values.extend(timer.args.iter().cloned());
        }
        for frame in &self.frames {
            values.extend(frame.stack.iter().cloned());
            values.push(frame.this.clone());
            if let Some(this) = &frame.constructor_this {
                values.push(this.clone());
            }
            environments.push(frame.environment);
        }
        (values, environments)
    }

    fn allocate_object(&mut self, kind: ObjectKind) -> Result<ObjectId, VmError> {
        match self.heap.allocate_object(kind.clone()) {
            Ok(id) => Ok(id),
            Err(HeapError::NurseryFull) => {
                let (values, environments) = self.roots();
                self.heap.collect(HeapRoots {
                    values: &values,
                    environments: &environments,
                });
                self.heap.allocate_object(kind).map_err(VmError::from)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn allocate_bound_native(
        &mut self,
        id: u32,
        name: String,
        arity: usize,
        receiver: Value,
    ) -> Result<ObjectId, VmError> {
        match self
            .heap
            .allocate_bound_native_function(id, name.clone(), arity, receiver.clone())
        {
            Ok(object) => Ok(object),
            Err(HeapError::NurseryFull) => {
                let (values, environments) = self.roots();
                self.heap.collect(HeapRoots {
                    values: &values,
                    environments: &environments,
                });
                self.heap
                    .allocate_bound_native_function(id, name, arity, receiver)
                    .map_err(VmError::from)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn allocate_environment(
        &mut self,
        parent: Option<EnvironmentId>,
    ) -> Result<EnvironmentId, VmError> {
        match self.heap.allocate_environment(parent) {
            Ok(environment) => Ok(environment),
            Err(HeapError::NurseryFull) => {
                let (values, environments) = self.roots();
                self.heap.collect(HeapRoots {
                    values: &values,
                    environments: &environments,
                });
                self.heap
                    .allocate_environment(parent)
                    .map_err(VmError::from)
            }
            Err(error) => Err(error.into()),
        }
    }
}

fn host_call_capability(id: u32) -> Option<(ScriptCapability, &'static str)> {
    Some(match id {
        NATIVE_DOCUMENT_GET_ELEMENT_BY_ID => (ScriptCapability::DomRead, "document.getElementById"),
        NATIVE_DOCUMENT_QUERY_SELECTOR => (ScriptCapability::DomRead, "document.querySelector"),
        NATIVE_DOCUMENT_QUERY_SELECTOR_ALL => {
            (ScriptCapability::DomRead, "document.querySelectorAll")
        }
        NATIVE_DOCUMENT_CREATE_ELEMENT => (ScriptCapability::DomWrite, "document.createElement"),
        NATIVE_DOCUMENT_CREATE_TEXT_NODE => (ScriptCapability::DomWrite, "document.createTextNode"),
        NATIVE_NODE_APPEND_CHILD => (ScriptCapability::DomWrite, "node.appendChild"),
        NATIVE_NODE_INSERT_BEFORE => (ScriptCapability::DomWrite, "node.insertBefore"),
        NATIVE_NODE_REMOVE_CHILD => (ScriptCapability::DomWrite, "node.removeChild"),
        NATIVE_NODE_REMOVE => (ScriptCapability::DomWrite, "node.remove"),
        NATIVE_NODE_GET_ATTRIBUTE => (ScriptCapability::DomRead, "node.getAttribute"),
        NATIVE_NODE_SET_ATTRIBUTE => (ScriptCapability::DomWrite, "node.setAttribute"),
        NATIVE_NODE_REMOVE_ATTRIBUTE => (ScriptCapability::DomWrite, "node.removeAttribute"),
        NATIVE_CLASS_LIST_CONTAINS => (ScriptCapability::DomRead, "node.classList.contains"),
        NATIVE_CLASS_LIST_ADD => (ScriptCapability::DomWrite, "node.classList.add"),
        NATIVE_CLASS_LIST_REMOVE => (ScriptCapability::DomWrite, "node.classList.remove"),
        NATIVE_CLASS_LIST_TOGGLE => (ScriptCapability::DomWrite, "node.classList.toggle"),
        NATIVE_NODE_ADD_EVENT_LISTENER => (ScriptCapability::DomWrite, "node.addEventListener"),
        NATIVE_NODE_REMOVE_EVENT_LISTENER => {
            (ScriptCapability::DomWrite, "node.removeEventListener")
        }
        _ => return None,
    })
}

fn array_method(name: &str) -> Option<u32> {
    Some(match name {
        "push" => NATIVE_ARRAY_PUSH,
        "pop" => NATIVE_ARRAY_POP,
        "indexOf" => NATIVE_ARRAY_INDEX_OF,
        "includes" => NATIVE_ARRAY_INCLUDES,
        "join" => NATIVE_ARRAY_JOIN,
        "forEach" => NATIVE_ARRAY_FOR_EACH,
        "map" => NATIVE_ARRAY_MAP,
        "filter" => NATIVE_ARRAY_FILTER,
        "slice" => NATIVE_ARRAY_SLICE,
        _ => return None,
    })
}

fn array_index(key: &str) -> Option<usize> {
    let index = key.parse::<usize>().ok()?;
    (index.to_string() == key).then_some(index)
}

fn string_method(name: &str) -> Option<u32> {
    Some(match name {
        "slice" => NATIVE_STRING_SLICE,
        "split" => NATIVE_STRING_SPLIT,
        "indexOf" => NATIVE_STRING_INDEX_OF,
        "includes" => NATIVE_STRING_INCLUDES,
        "trim" => NATIVE_STRING_TRIM,
        "toUpperCase" => NATIVE_STRING_UPPER,
        "toLowerCase" => NATIVE_STRING_LOWER,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_bytecode_for_arithmetic_control_flow_and_completion() {
        let mut vm = Vm::new();

        let value = vm
            .evaluate("let total = 0; for (let i = 0; i < 4; i++) { total += i; } total;")
            .unwrap();

        assert_eq!(value, Value::Number(6.0));
    }

    #[test]
    fn closures_capture_their_parent_environment_and_call_through_bytecode() {
        let mut vm = Vm::new();

        let value = vm.evaluate("function makeAdder(base) { return value => base + value; } let addTwo = makeAdder(2); addTwo(5);").unwrap();

        assert_eq!(value, Value::Number(7.0));
    }

    #[test]
    fn functions_from_an_earlier_script_keep_their_own_compiled_module() {
        let mut vm = Vm::new();

        vm.evaluate("function fromFirstScript() { return 'first-module'; }")
            .unwrap();
        vm.evaluate("console.log(fromFirstScript());").unwrap();

        assert_eq!(vm.take_output(), vec!["first-module"]);
    }

    #[test]
    fn arrays_methods_templates_and_destructuring_execute_without_ast_evaluation() {
        let mut vm = Vm::new();

        let value = vm.evaluate("let values = [1, 2]; values.push(3); const [first, , third] = values; `${first}:${third}:${values.join('-')}`;").unwrap();

        assert_eq!(value, Value::String("1:3:1-2-3".to_string()));
    }

    #[test]
    fn array_indexing_callbacks_and_slice_are_bytecode_callable() {
        let mut vm = Vm::new();

        let value = vm
            .evaluate("let values = [1, 2, 3]; values[1] = 4; let sum = 0; values.forEach(value => sum += value); let mapped = values.map(value => value * 2); let filtered = mapped.filter(value => value > 4); `${values.length}:${values[1]}:${sum}:${filtered.slice(-2).join(',')}`;")
            .unwrap();

        assert_eq!(value, Value::String("3:4:8:8,6".to_string()));
    }

    #[test]
    fn object_math_and_error_intrinsics_share_the_managed_value_model() {
        let mut vm = Vm::new();

        let value = vm
            .evaluate("let object = { beta: 2, alpha: 1 }; let message = new Error('bad').name + ':' + new Error('bad').message; let errorKinds = new TypeError('type').name + ',' + new RangeError('range').name; `${Object.keys(object).join(',')}:${Object.values(object).join(',')}:${Object.entries(object).map(entry => entry.join('=')).join(',')}:${Math.floor(2.9)}:${Math.round(-1.5)}:${Math.max(2, 7, 3)}:${Math.min(2, 7, 3)}:${message}:${errorKinds}`;")
            .unwrap();

        assert_eq!(
            value,
            Value::String(
                "alpha,beta:1,2:alpha=1,beta=2:2:-1:7:2:Error:bad:TypeError,RangeError".to_string()
            )
        );
        let random = vm.evaluate("Math.random();").unwrap();
        assert!(matches!(random, Value::Number(value) if (0.0..1.0).contains(&value)));
    }

    #[test]
    fn spread_arguments_use_dedicated_call_bytecode_for_plain_member_and_new_calls() {
        let mut vm = Vm::new();

        let value = vm
            .evaluate("function collect(...items) { return items.join('-'); } const values = [2, 7]; const error = new Error(...['boom']); `${collect(1, ...values)}:${Math.max(...values)}:${error.message}`;")
            .unwrap();

        assert_eq!(value, Value::String("1-2-7:7:boom".to_string()));
    }

    #[derive(Debug)]
    struct NoopDom;

    impl DomHost for NoopDom {
        fn get_element_by_id(&mut self, _: &str) -> Result<Option<u64>, String> {
            Ok(Some(1))
        }
        fn query_selector(&mut self, _: &str) -> Result<Option<u64>, String> {
            Ok(None)
        }
        fn query_selector_all(&mut self, _: &str) -> Result<Vec<u64>, String> {
            Ok(Vec::new())
        }
        fn create_element(&mut self, _: &str) -> Result<u64, String> {
            Ok(1)
        }
        fn create_text_node(&mut self, _: &str) -> Result<u64, String> {
            Ok(1)
        }
        fn append_child(&mut self, _: u64, _: u64) -> Result<(), String> {
            Ok(())
        }
        fn insert_before(&mut self, _: u64, _: u64, _: Option<u64>) -> Result<(), String> {
            Ok(())
        }
        fn remove_child(&mut self, _: u64, _: u64) -> Result<(), String> {
            Ok(())
        }
        fn remove(&mut self, _: u64) -> Result<(), String> {
            Ok(())
        }
        fn get_text_content(&mut self, _: u64) -> Result<String, String> {
            Ok(String::new())
        }
        fn set_text_content(&mut self, _: u64, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn get_attribute(&mut self, _: u64, _: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        fn set_attribute(&mut self, _: u64, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn remove_attribute(&mut self, _: u64, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn inner_html(&mut self, _: u64) -> Result<String, String> {
            Ok(String::new())
        }
        fn get_style_property(&mut self, _: u64, _: &str) -> Result<String, String> {
            Ok(String::new())
        }
        fn set_style_property(&mut self, _: u64, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn class_list(&mut self, _: u64, _: ClassListOperation, _: &str) -> Result<bool, String> {
            Ok(false)
        }
    }

    struct RejectWrites;

    impl ScriptGatekeeperHook for RejectWrites {
        type Error = &'static str;

        fn before_host_call(
            &mut self,
            capability: ScriptCapability,
            _: &str,
        ) -> Result<(), Self::Error> {
            if capability == ScriptCapability::DomWrite {
                Err("DOM writes require approval")
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn gatekeeper_hook_runs_immediately_before_a_dom_host_call() {
        let mut vm = Vm::new();
        vm.set_dom_host(Box::new(NoopDom)).unwrap();
        vm.set_gatekeeper_hook(RejectWrites);

        assert!(matches!(
            vm.evaluate("document.createElement('p');"),
            Err(VmError::Runtime(message)) if message == "DOM writes require approval"
        ));
    }

    #[test]
    fn event_listeners_run_outside_the_original_script_turn_and_can_prevent_default() {
        let mut vm = Vm::new();
        vm.set_dom_host(Box::new(NoopDom)).unwrap();

        vm.evaluate(
            "let button = document.getElementById('button'); button.addEventListener('click', event => { if (event.type === 'click') event.preventDefault(); console.log('listener-fired', event.defaultPrevented); });",
        )
        .unwrap();

        assert!(vm.has_event_listeners(1, "click"));
        assert!(vm.dispatch_event(1, "click").unwrap());
        assert_eq!(vm.take_output(), vec!["listener-fired true"]);
    }

    #[test]
    fn remove_event_listener_and_due_timers_have_their_own_turns() {
        let mut vm = Vm::new();
        vm.set_dom_host(Box::new(NoopDom)).unwrap();

        vm.evaluate(
            "let node = document.getElementById('node'); let calls = 0; let listener = () => calls += 1; node.addEventListener('click', listener); node.removeEventListener('click', listener); setTimeout(() => console.log('timer-fired'), 0);",
        )
        .unwrap();

        assert!(!vm.has_event_listeners(1, "click"));
        assert!(!vm.dispatch_event(1, "click").unwrap());
        assert!(vm.run_due_timers().unwrap());
        assert_eq!(vm.take_output(), vec!["timer-fired"]);
        assert!(!vm.run_due_timers().unwrap(), "a timeout is one-shot");
    }

    #[test]
    fn thrown_values_reach_bytecode_catch_and_finally() {
        let mut vm = Vm::new();

        let value = vm.evaluate("let result = 0; try { throw 4; } catch (error) { result = error + 1; } finally { result += 2; } result;").unwrap();

        assert_eq!(value, Value::Number(7.0));
    }

    #[test]
    fn console_is_a_host_binding_with_captured_output_not_process_global_io() {
        let mut vm = Vm::new();

        let value = vm.evaluate("console.log('value', 3); 1;").unwrap();

        assert_eq!(value, Value::Number(1.0));
        assert_eq!(vm.take_output(), vec!["value 3"]);
    }

    #[test]
    fn low_nursery_limit_collects_from_active_frame_roots() {
        let mut vm = Vm::with_heap_limits(HeapLimits {
            nursery_entries: 5,
            tenured_entries: 8,
        })
        .unwrap();

        let value = vm.evaluate("let items = []; for (let i = 0; i < 12; i++) { items.push({ value: i }); } items.length;").unwrap();

        assert_eq!(value, Value::Number(12.0));
        assert!(vm.heap().stats().nursery_collections > 0);
    }

    #[test]
    fn instruction_budget_stops_a_runaway_loop() {
        let mut vm = Vm::new();
        vm.set_instruction_budget(100);

        assert_eq!(
            vm.evaluate("while (true) {}"),
            Err(VmError::InstructionBudgetExceeded)
        );
    }

    #[test]
    fn short_circuit_do_while_and_switch_keep_the_operand_stack_balanced() {
        let mut vm = Vm::new();

        let value = vm
            .evaluate("let calls = 0; let keep = false && (calls = 1); let i = 0; do { i++; } while (i < 2); switch (i) { case 1: calls = 10; break; case 2: calls = calls + 2; break; default: calls = 99; } calls;")
            .unwrap();

        assert_eq!(value, Value::Number(2.0));
    }

    #[test]
    fn parameter_and_destructuring_defaults_run_as_compiled_helper_functions() {
        let mut vm = Vm::new();

        let value = vm
            .evaluate("function read({ value = 4 } = {}) { return value; } read();")
            .unwrap();

        assert_eq!(value, Value::Number(4.0));
    }
}
