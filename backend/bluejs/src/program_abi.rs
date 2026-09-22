// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Versioned structured-program hand-off owned by BlueJS.
//!
//! Embedders construct the existing public AST directly and wrap it in this
//! type before requesting bytecode.  It deliberately accepts no source text:
//! a language front end therefore cannot turn an emitted JavaScript string
//! into an implicit second parser pass at this boundary.

use crate::{compile, compile_module, Bytecode, CompileError, Module, Program};

/// The first BlueJS-owned structured program ABI.
pub const BLUEJS_PROGRAM_ABI_V1: &str = "bluejs-program-v1";

/// A version-tagged script or module AST accepted by the BlueJS compiler.
#[derive(Debug, Clone, PartialEq)]
pub enum BlueJsProgramV1 {
    Script(Program),
    Module(Module),
}

impl BlueJsProgramV1 {
    /// The ABI string checked by a bridge or host before compilation.
    pub const ABI: &'static str = BLUEJS_PROGRAM_ABI_V1;

    /// Compiles this already-structured program to BlueJS bytecode.
    pub fn compile(&self) -> Result<Bytecode, CompileError> {
        match self {
            Self::Script(program) => compile(program),
            Self::Module(module) => compile_module(module),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Expr, Stmt, Value, Vm};

    #[test]
    fn compiles_a_structured_script_without_a_source_parser() {
        let program = BlueJsProgramV1::Script(Program {
            body: vec![Stmt::Expr(Expr::Number(42.0))],
        });
        let code = program.compile().unwrap();
        assert_eq!(Vm::default().execute(&code).unwrap(), Value::Number(42.0));
    }
}
