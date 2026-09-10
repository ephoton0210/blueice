// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compiler-owned bytecode: one byte per opcode plus a fixed u32 operand
//! where needed. The table defines decoding, width and tiering metadata
//! together; VM dispatch is an exhaustive match on its generated enum.
//! Jumps use byte offsets. No public deserializer accepts arbitrary code:
//! serialization/versioning and hostile-bytecode validation are future work.

use crate::Value;

/// Reserved opcode metadata for later inline-cache tiers. No cache is
/// allocated or consulted by the first interpreter.
pub const MAY_USE_INLINE_CACHE: u8 = 1;

macro_rules! opcodes {
    ($($name:ident: $width:literal, $flags:expr;)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        pub enum Opcode { $($name,)* }

        impl Opcode {
            pub fn width(self) -> usize { match self { $(Self::$name => $width,)* } }
            pub fn flags(self) -> u8 { match self { $(Self::$name => $flags,)* } }
            fn decode(byte: u8) -> Option<Self> {
                match byte { $(n if n == Self::$name as u8 => Some(Self::$name),)* _ => None }
            }
        }
    };
}

opcodes! {
    Constant: 5, 0;
    GetBinding: 5, 0;
    InitializeBinding: 5, 0;
    StoreBinding: 5, 0;
    UnboundName: 5, 0;
    EnterScope: 5, 0;
    LeaveScope: 5, 0;
    Pop: 1, 0;
    Dup: 1, 0;
    Dup2: 1, 0;
    Add: 1, 0;
    Subtract: 1, 0;
    Multiply: 1, 0;
    Divide: 1, 0;
    Remainder: 1, 0;
    StrictEqual: 1, 0;
    StrictNotEqual: 1, 0;
    Equal: 1, 0;
    NotEqual: 1, 0;
    Less: 1, 0;
    Greater: 1, 0;
    LessEqual: 1, 0;
    GreaterEqual: 1, 0;
    Negate: 1, 0;
    ToNumber: 1, 0;
    ToString: 1, 0;
    Not: 1, 0;
    Typeof: 1, 0;
    Jump: 5, 0;
    JumpIfFalse: 5, 0;
    JumpIfTrue: 5, 0;
    JumpIfNotNullish: 5, 0;
    NewObject: 1, 0;
    GetProperty: 1, MAY_USE_INLINE_CACHE;
    SetProperty: 1, MAY_USE_INLINE_CACHE;
    SetDestructureProperty: 1, MAY_USE_INLINE_CACHE;
    UpdateProperty: 5, MAY_USE_INLINE_CACHE;
    SetLiteralPrototype: 1, 0;
    SetCompletion: 1, 0;
    ClearCompletion: 1, 0;
    Halt: 1, 0;
    NewArray: 5, 0;
    GlobalString: 1, 0;
    GetMethod: 1, MAY_USE_INLINE_CACHE;
    Call: 5, 0;
    Construct: 5, 0;
    Closure: 5, 0;
    This: 1, 0;
    Argument: 5, 0;
    RestArguments: 5, 0;
    Return: 1, 0;
    Global: 5, 0;
    ToPropertyKey: 1, 0;
    GetIterator: 1, 0;
    ForInKeys: 1, 0;
    IteratorStep: 5, 0;
    IteratorElision: 1, 0;
    IteratorClose: 1, 0;
    IteratorFinish: 1, 0;
    IteratorRest: 1, 0;
    RequireObject: 1, 0;
    DestructureProperty: 1, 0;
    ObjectRest: 1, 0;
    CopyDataProperties: 1, 0;
    RegExpLiteral: 1, 0;
    TemplateObject: 5, 0;
    ArrayPush: 5, 0;
    CallSpread: 5, 0;
    Throw: 1, 0;
    PushHandler: 5, 0;
    PopHandler: 1, 0;
    ResumeCompletion: 5, 0;
    SaveCompletion: 1, 0;
    AbruptJump: 5, 0;
    DefineData: 1, 0;
    DefineAccessor: 5, 0;
    DeleteProperty: 1, 0;
    Instanceof: 1, 0;
    In: 1, 0;
    TypeofName: 5, 0;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    pub offset: usize,
    pub opcode: Opcode,
    pub operand: Option<u32>,
}

#[derive(Clone)]
pub(crate) struct Binding {
    pub name: String,
    pub mutable: bool,
    pub lexical: bool,
}

#[derive(Clone)]
pub(crate) struct TemplateSite {
    pub id: u64,
    pub raw: Vec<crate::JsString>,
    pub cooked: Vec<Option<crate::JsString>>,
}

/// Static destinations for one `try` statement. Runtime handler frames add
/// the operand-stack, lexical-scope and iterator depths needed to restore the
/// execution state on an abrupt completion.
#[derive(Clone)]
pub(crate) struct Handler {
    pub try_start: u32,
    pub try_end: u32,
    pub catch: Option<u32>,
    pub catch_end: Option<u32>,
    pub finally: Option<u32>,
}

/// One control-transfer continuation. `cleanup` runs after every enclosing
/// finalizer; `target` is the eventual destination used to decide which
/// enclosing handlers the transfer actually leaves.
#[derive(Clone)]
pub(crate) struct AbruptJump {
    pub cleanup: u32,
    pub target: u32,
}

/// Read-only compiled code plus its constant pool and binding/scope
/// metadata. Can execute repeatedly in the same or independent VMs;
/// runtime object handles are never stored in its constant pool.
#[derive(Clone)]
pub struct Bytecode {
    pub(crate) code: Vec<u8>,
    pub(crate) constants: Vec<Value>,
    pub(crate) bindings: Vec<Binding>,
    pub(crate) scopes: Vec<Vec<u32>>,
    pub(crate) functions: Vec<std::rc::Rc<Bytecode>>,
    pub(crate) captures: Vec<u32>,
    pub(crate) function_name: String,
    pub(crate) function_length: u32,
    pub(crate) arrow: bool,
    pub(crate) constructible: bool,
    pub(crate) strict: bool,
    pub(crate) templates: Vec<TemplateSite>,
    pub(crate) handlers: Vec<Handler>,
    pub(crate) abrupt_jumps: Vec<AbruptJump>,
}

impl Bytecode {
    pub(crate) fn empty() -> Self {
        Self {
            code: Vec::new(),
            constants: Vec::new(),
            bindings: Vec::new(),
            scopes: Vec::new(),
            functions: Vec::new(),
            captures: Vec::new(),
            function_name: String::new(),
            function_length: 0,
            arrow: false,
            constructible: false,
            strict: false,
            templates: Vec::new(),
            handlers: Vec::new(),
            abrupt_jumps: Vec::new(),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.code
    }

    pub fn constants(&self) -> &[Value] {
        &self.constants
    }

    pub fn instructions(&self) -> impl Iterator<Item = Instruction> + '_ {
        let mut offset = 0;
        std::iter::from_fn(move || {
            let instruction = self.instruction(offset)?;
            offset += instruction.opcode.width();
            Some(instruction)
        })
    }

    pub(crate) fn instruction(&self, offset: usize) -> Option<Instruction> {
        let opcode = Opcode::decode(*self.code.get(offset)?)?;
        let operand = if opcode.width() == 5 { Some(u32::from_le_bytes(self.code.get(offset + 1..offset + 5)?.try_into().ok()?)) } else { None };
        Some(Instruction { offset, opcode, operand })
    }
}
