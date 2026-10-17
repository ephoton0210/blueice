// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Eval inputs, compiler snapshots and root code keep independent native owners.

use super::compilation_input_storage::{parsing_charge, CompilationInput};
use super::host_scheduling::{owned_code_bytes, SharedSchedulingCharge};
use super::*;

pub(super) type ParsedEvalInput = CompilationInput<crate::Program>;

pub(super) struct PreparedEvalCode {
    code: Bytecode,
    _charge: Option<SharedSchedulingCharge>,
}
impl std::ops::Deref for PreparedEvalCode {
    type Target = Bytecode;
    fn deref(&self) -> &Bytecode {
        &self.code
    }
}
impl ParsedEvalInput {
    pub(super) fn parse_eval(
        source: &str,
        strict: bool,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let charge = parsing_charge(source, scope)?;
        let ast = crate::parse_eval(source, strict).map_err(|error| {
            error
                .resource
                .unwrap_or(RuntimeError::SyntaxError(error.message))
        })?;
        Self::from_parsed(ast, charge, scope)
    }
    pub(super) fn parse_script(
        source: &str,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let charge = parsing_charge(source, scope)?;
        let ast = crate::parser::parse_encoded(source).map_err(|error| {
            error
                .resource
                .unwrap_or(RuntimeError::SyntaxError(error.message))
        })?;
        Self::from_parsed(ast, charge, scope)
    }
    pub(super) fn compile_with(
        mut self,
        scope: Option<&SchedulingScope>,
        compile: impl FnOnce(&crate::Program) -> Result<Bytecode, crate::CompileError>,
    ) -> Result<PreparedEvalCode, RuntimeError> {
        self.prepare(scope)?;
        let mut code =
            compile(&self.ast).map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        self.install(&mut code, scope)?;
        let charge = scope
            .map(|scope| scope.shared_charge(owned_code_bytes(&code)))
            .transpose()?;
        drop(self);
        Ok(PreparedEvalCode {
            code,
            _charge: charge,
        })
    }
}

/// These returned snapshots remain alive through compilation and capture
/// preparation. Temporary tree builders are a separate producer boundary.
pub(super) struct EvalCompileEnvironment {
    pub(super) visible: Vec<(String, Binding, u32)>,
    pub(super) dynamic_captures: HashMap<u32, ObjectId>,
    pub(super) variables: Vec<String>,
    pub(super) lexical: Vec<String>,
    _charge: Option<SharedSchedulingCharge>,
}
fn names_bytes(names: &Vec<String>) -> usize {
    names
        .capacity()
        .saturating_mul(std::mem::size_of::<String>())
        .saturating_add(
            names
                .iter()
                .map(String::capacity)
                .fold(0usize, usize::saturating_add),
        )
}
impl EvalCompileEnvironment {
    pub(super) fn adopt(
        visible: Vec<(String, Binding, u32)>,
        dynamic_captures: HashMap<u32, ObjectId>,
        variables: Vec<String>,
        lexical: Vec<String>,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let bytes = visible
            .capacity()
            .saturating_mul(std::mem::size_of::<(String, Binding, u32)>())
            .saturating_add(visible.iter().fold(0usize, |bytes, (name, binding, _)| {
                bytes
                    .saturating_add(name.capacity())
                    .saturating_add(binding.name.capacity())
            }))
            .saturating_add(
                dynamic_captures
                    .capacity()
                    .saturating_mul(std::mem::size_of::<(u32, ObjectId)>()),
            )
            .saturating_add(names_bytes(&variables))
            .saturating_add(names_bytes(&lexical));
        let charge = scope.map(|scope| scope.shared_charge(bytes)).transpose()?;
        Ok(Self {
            visible,
            dynamic_captures,
            variables,
            lexical,
            _charge: charge,
        })
    }
}
