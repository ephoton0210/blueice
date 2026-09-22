// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! BlueJS's fixed-width operand-stack bytecode.
//!
//! An [`Instruction`] is exactly one `u64`: eight opcode bits, eight reserved
//! tiering/inline-cache-hint bits, and a 48-bit operand.  Keeping that shape
//! stable lets a later optimizing tier attach metadata to an instruction site
//! without changing the compiler's byte stream or replacing the interpreter.

use crate::value::Value;

const OPCODE_MASK: u64 = 0xff;
const TIERING_SHIFT: u64 = 8;
const OPERAND_SHIFT: u64 = 16;
const OPERAND_MASK: u64 = (1_u64 << 48) - 1;

/// One stack-machine operation. Values are explicit instead of relying on a
/// Rust enum's layout so every encoded instruction has one fixed width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    Nop,
    LoadConstant,
    LoadUndefined,
    LoadNull,
    LoadTrue,
    LoadFalse,
    LoadThis,
    LoadBinding,
    StoreBinding,
    DeclareVar,
    DeclareLet,
    DeclareConst,
    EnterScope,
    LeaveScope,
    Pop,
    SetCompletion,
    Dup,
    Dup2,
    MakeArray,
    ArrayPush,
    ArrayHole,
    ArraySpread,
    MakeObject,
    ObjectSet,
    ObjectSpread,
    MakeFunction,
    GetProperty,
    SetProperty,
    UnaryNeg,
    UnaryPos,
    UnaryNot,
    Typeof,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
    LessThan,
    GreaterThan,
    LessThanOrEqual,
    GreaterThanOrEqual,
    Instanceof,
    In,
    Jump,
    JumpIfFalse,
    JumpIfTrue,
    JumpIfNullish,
    JumpIfNotNullish,
    Call,
    CallWithThis,
    Construct,
    Return,
    Throw,
    TryBegin,
    TryEnd,
    BindPattern,
    IteratorStartOf,
    IteratorStartIn,
    IteratorNext,
    Halt,
    Rotate3,
    DeclarePatternVar,
    DeclarePatternLet,
    DeclarePatternConst,
    SetPropertyKeepOld,
    JumpIfTruePop,
    /// Invokes a function with an argument array assembled by the compiler
    /// when any syntactic argument uses `...spread`.
    CallSpread,
    CallWithThisSpread,
    ConstructSpread,
}

impl Opcode {
    pub const fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Nop,
            1 => Self::LoadConstant,
            2 => Self::LoadUndefined,
            3 => Self::LoadNull,
            4 => Self::LoadTrue,
            5 => Self::LoadFalse,
            6 => Self::LoadThis,
            7 => Self::LoadBinding,
            8 => Self::StoreBinding,
            9 => Self::DeclareVar,
            10 => Self::DeclareLet,
            11 => Self::DeclareConst,
            12 => Self::EnterScope,
            13 => Self::LeaveScope,
            14 => Self::Pop,
            15 => Self::SetCompletion,
            16 => Self::Dup,
            17 => Self::Dup2,
            18 => Self::MakeArray,
            19 => Self::ArrayPush,
            20 => Self::ArrayHole,
            21 => Self::ArraySpread,
            22 => Self::MakeObject,
            23 => Self::ObjectSet,
            24 => Self::ObjectSpread,
            25 => Self::MakeFunction,
            26 => Self::GetProperty,
            27 => Self::SetProperty,
            28 => Self::UnaryNeg,
            29 => Self::UnaryPos,
            30 => Self::UnaryNot,
            31 => Self::Typeof,
            32 => Self::Add,
            33 => Self::Subtract,
            34 => Self::Multiply,
            35 => Self::Divide,
            36 => Self::Remainder,
            37 => Self::Equal,
            38 => Self::NotEqual,
            39 => Self::StrictEqual,
            40 => Self::StrictNotEqual,
            41 => Self::LessThan,
            42 => Self::GreaterThan,
            43 => Self::LessThanOrEqual,
            44 => Self::GreaterThanOrEqual,
            45 => Self::Instanceof,
            46 => Self::In,
            47 => Self::Jump,
            48 => Self::JumpIfFalse,
            49 => Self::JumpIfTrue,
            50 => Self::JumpIfNullish,
            51 => Self::JumpIfNotNullish,
            52 => Self::Call,
            53 => Self::CallWithThis,
            54 => Self::Construct,
            55 => Self::Return,
            56 => Self::Throw,
            57 => Self::TryBegin,
            58 => Self::TryEnd,
            59 => Self::BindPattern,
            60 => Self::IteratorStartOf,
            61 => Self::IteratorStartIn,
            62 => Self::IteratorNext,
            63 => Self::Halt,
            64 => Self::Rotate3,
            65 => Self::DeclarePatternVar,
            66 => Self::DeclarePatternLet,
            67 => Self::DeclarePatternConst,
            68 => Self::SetPropertyKeepOld,
            69 => Self::JumpIfTruePop,
            70 => Self::CallSpread,
            71 => Self::CallWithThisSpread,
            72 => Self::ConstructSpread,
            _ => return None,
        })
    }
}

/// A packed fixed-width instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Instruction(u64);

impl Instruction {
    pub const fn new(opcode: Opcode, operand: u64) -> Self {
        assert!(
            operand <= OPERAND_MASK,
            "BlueJS bytecode operand exceeds 48 bits"
        );
        Self((opcode as u64) | (operand << OPERAND_SHIFT))
    }

    pub const fn opcode(self) -> Opcode {
        match Opcode::from_u8((self.0 & OPCODE_MASK) as u8) {
            Some(opcode) => opcode,
            None => panic!("invalid BlueJS opcode"),
        }
    }

    pub const fn operand(self) -> u64 {
        self.0 >> OPERAND_SHIFT
    }

    /// Reserved for a future interpreter/JIT tier to annotate a bytecode site
    /// (the role of SpiderMonkey's `JOF_IC`). The MVP interpreter leaves it at
    /// zero and never gives it semantic meaning.
    pub const fn tiering_hint(self) -> u8 {
        ((self.0 >> TIERING_SHIFT) & OPCODE_MASK) as u8
    }

    pub const fn with_tiering_hint(self, hint: u8) -> Self {
        Self((self.0 & !(OPCODE_MASK << TIERING_SHIFT)) | ((hint as u64) << TIERING_SHIFT))
    }
}

/// Constants are module data, not instructions, so their values do not affect
/// the byte stream's fixed width.
#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Value(Value),
    String(String),
}

/// A compiler-lowered binding pattern. It intentionally is not an AST type:
/// the VM consumes this compact runtime plan and never walks the parser AST.
#[derive(Debug, Clone, PartialEq)]
pub enum BindingPattern {
    Identifier(String),
    Array(Vec<Option<ArrayBindingElement>>),
    Object(Vec<ObjectBindingProperty>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayBindingElement {
    pub pattern: BindingPattern,
    pub default_function: Option<u32>,
    pub rest: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BindingKey {
    Static(String),
    ComputedFunction(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObjectBindingProperty {
    KeyValue {
        key: BindingKey,
        value: BindingPattern,
        default_function: Option<u32>,
    },
    Rest(BindingPattern),
}

/// Parameter lowering metadata. A default expression is compiled into a
/// zero-argument helper function, so parameter binding is still bytecode-only
/// at execution time.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledParameter {
    pub pattern: BindingPattern,
    pub default_function: Option<u32>,
    pub rest: bool,
}

/// The code and binding metadata for one function (function 0 is the script's
/// top-level function).
#[derive(Debug, Clone, PartialEq)]
pub struct BytecodeFunction {
    pub name: Option<String>,
    pub parameters: Vec<CompiledParameter>,
    pub code: Vec<Instruction>,
    pub is_arrow: bool,
}

/// A complete compiled script.
#[derive(Debug, Clone, PartialEq)]
pub struct BytecodeModule {
    pub constants: Vec<Constant>,
    pub functions: Vec<BytecodeFunction>,
    pub patterns: Vec<BindingPattern>,
}

impl BytecodeModule {
    pub fn entry(&self) -> &BytecodeFunction {
        &self.functions[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn instruction_is_a_fixed_eight_byte_word_with_reserved_tiering_bits() {
        let instruction = Instruction::new(Opcode::LoadConstant, 123).with_tiering_hint(9);

        assert_eq!(size_of::<Instruction>(), 8);
        assert_eq!(instruction.opcode(), Opcode::LoadConstant);
        assert_eq!(instruction.operand(), 123);
        assert_eq!(instruction.tiering_hint(), 9);
    }

    #[test]
    fn every_emitted_opcode_round_trips_through_its_fixed_encoding() {
        for byte in 0..=72 {
            let opcode = Opcode::from_u8(byte).unwrap();
            assert_eq!(Instruction::new(opcode, 0).opcode(), opcode);
        }
        assert!(Opcode::from_u8(73).is_none());
    }
}
