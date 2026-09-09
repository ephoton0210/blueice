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
    UpdateProperty: 5, MAY_USE_INLINE_CACHE;
    SetLiteralPrototype: 1, 0;
    SetCompletion: 1, 0;
    ClearCompletion: 1, 0;
    Halt: 1, 0;
    NewArray: 5, 0;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    pub offset: usize,
    pub opcode: Opcode,
    pub operand: Option<u32>,
}

pub(crate) struct Binding {
    pub name: String,
    pub mutable: bool,
    pub lexical: bool,
}

/// Read-only compiled code plus its constant pool and binding/scope
/// metadata. Can execute repeatedly in the same or independent VMs;
/// runtime object handles are never stored in its constant pool.
pub struct Bytecode {
    pub(crate) code: Vec<u8>,
    pub(crate) constants: Vec<Value>,
    pub(crate) bindings: Vec<Binding>,
    pub(crate) scopes: Vec<Vec<u32>>,
}

impl Bytecode {
    pub(crate) fn empty() -> Self {
        Self { code: Vec::new(), constants: Vec::new(), bindings: Vec::new(), scopes: Vec::new() }
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
