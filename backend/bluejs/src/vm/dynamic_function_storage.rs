// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owned constructor text, parsed wrappers and generated function code.

use super::compilation_input_storage::{fail, parsing_charge, CompilationInput};
use super::host_scheduling::owned_code_bytes;
use super::*;

pub(super) use super::compilation_text_storage::{NativeCaptures, NativeText};

pub(super) type ParsedFunctionInput = CompilationInput<crate::Program>;
impl ParsedFunctionInput {
    pub(super) fn parse_wrapper(
        source: &str,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let charge = parsing_charge(source, scope)?;
        let ast = crate::parser::parse_encoded(source).map_err(|error| {
            error
                .resource
                .unwrap_or(RuntimeError::SyntaxError(error.message))
        })?;
        let input = Self::from_parsed(ast, charge, scope)?;
        match input.ast.body.as_slice() {
            [crate::ast::Stmt::FunctionDecl(_)] => Ok(input),
            _ => Err(RuntimeError::SyntaxError(
                "Function source must be exactly one function".into(),
            )),
        }
    }
    pub(super) fn configure(
        &mut self,
        binding: &str,
        original: Option<&str>,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        self.attach_input(scope)?;
        let extra = binding
            .len()
            .checked_add(original.map_or(0, str::len))
            .ok_or_else(|| fail(scope))?;
        self.reserve_extra(extra)?;
        let mut name = String::new();
        name.try_reserve_exact(binding.len())
            .map_err(|_| fail(scope))?;
        name.push_str(binding);
        let [crate::ast::Stmt::FunctionDecl(function)] = self.ast.body.as_mut_slice() else {
            unreachable!("the wrapper shape was checked")
        };
        function.name = Some(name);
        if let Some(original) = original {
            function.source_text = crate::ast::SourceText::whole(original);
        }
        self.reprice(scope)
    }
    pub(super) fn compile_function(
        mut self,
        binding: &str,
        scope: Option<&SchedulingScope>,
    ) -> Result<SharedBytecode, RuntimeError> {
        self.prepare(scope)?;
        let mut code = crate::compile(&self.ast)
            .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        self.install(&mut code, scope)?;
        // The temporary root buffers coexist with the AST and child owners.
        // Destroy that root before refunding its separate native lease.
        let root_charge = scope
            .map(|scope| scope.shared_charge(owned_code_bytes(&code)))
            .transpose()?;
        let child = code
            .functions
            .first()
            .expect("the wrapper compiles one function")
            .clone();
        for slot in &child.captures {
            assert_eq!(code.bindings[*slot as usize].name, binding);
        }
        drop(code);
        drop(root_charge);
        drop(self);
        Ok(child)
    }
}
