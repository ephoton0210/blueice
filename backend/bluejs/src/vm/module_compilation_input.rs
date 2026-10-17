// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parsed module inputs retain native ownership through root-code preparation.

use super::compilation_input_storage::{parsing_charge, CompilationInput};
use super::module_code_storage::PreparedModuleCode;
use super::*;

pub(super) type ParsedModuleInput = CompilationInput<crate::Module>;
impl ParsedModuleInput {
    pub(super) fn parse(
        source: &str,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let charge = parsing_charge(source, scope)?;
        let module = crate::parse_module(source).map_err(|error| {
            error
                .resource
                .unwrap_or(RuntimeError::SyntaxError(error.message))
        })?;
        Self::from_parsed(module, charge, scope)
    }
    pub(super) fn compile(
        mut self,
        scope: Option<&SchedulingScope>,
    ) -> Result<PreparedModuleCode, RuntimeError> {
        self.prepare(scope)?;
        let mut code = crate::compile_module_with_limit(&self.ast, u32::MAX)
            .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        self.install(&mut code, scope)?;
        let mut code = PreparedModuleCode::input(code);
        code.attach(scope)?;
        drop(self);
        Ok(code)
    }
}
