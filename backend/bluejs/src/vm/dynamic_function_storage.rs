// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owned constructor text, parsed wrappers and generated function code.

use super::compilation_input_storage::{fail, parsing_charge, CompilationInput};
use super::host_scheduling::{owned_code_bytes, SharedSchedulingCharge};
use super::*;

pub(super) struct NativeText {
    text: String,
    charge: Option<SharedSchedulingCharge>,
}
struct CoercedText {
    value: JsString,
    _charge: Option<SharedSchedulingCharge>,
}
impl std::ops::Deref for NativeText {
    type Target = str;
    fn deref(&self) -> &str {
        &self.text
    }
}
impl AsRef<str> for NativeText {
    fn as_ref(&self) -> &str {
        &self.text
    }
}
impl NativeText {
    pub(super) fn empty() -> Self {
        Self {
            text: String::new(),
            charge: None,
        }
    }
    fn capacity(count: usize, scope: Option<&SchedulingScope>) -> Result<Self, RuntimeError> {
        let mut charge = scope.map(|scope| scope.shared_charge(count)).transpose()?;
        let mut text = String::new();
        text.try_reserve_exact(count).map_err(|_| fail(scope))?;
        if let Some(charge) = &mut charge {
            charge.resize(text.capacity())?;
        }
        Ok(Self { text, charge })
    }
    fn ensure(
        &mut self,
        extra: usize,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        if let Some(scope) = scope {
            scope.shared_charge(0)?;
        }
        match (&self.charge, scope) {
            (Some(owner), Some(scope)) if !owner.belongs_to(scope) => return Err(scope.fail()),
            (Some(owner), None) => return Err(owner.fail()),
            (None, Some(scope)) => self.charge = Some(scope.shared_charge(self.text.capacity())?),
            _ => {}
        }
        let required = self
            .text
            .len()
            .checked_add(extra)
            .ok_or_else(|| fail(scope))?;
        if required > self.text.capacity() {
            let count = required.max(self.text.capacity().saturating_mul(2)).max(4);
            let mut replacement = Self::capacity(count, scope)?;
            replacement.text.push_str(&self.text);
            std::mem::swap(self, &mut replacement);
        }
        Ok(())
    }
    pub(super) fn append(
        &mut self,
        text: &str,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        self.ensure(text.len(), scope)?;
        self.text.push_str(text);
        Ok(())
    }
    pub(super) fn append_encoded(
        &mut self,
        value: JsString,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        let bytes = value
            .owned_heap_payload_bytes()
            .ok_or_else(|| fail(scope))?;
        let input = CoercedText {
            value,
            _charge: scope.map(|scope| scope.shared_charge(bytes)).transpose()?,
        };
        let length =
            crate::source_encoding::encoded_bytes(&input.value).ok_or_else(|| fail(scope))?;
        self.ensure(length, scope)?;
        crate::source_encoding::append_encoded(&input.value, &mut self.text);
        Ok(())
    }
    pub(super) fn parts(
        parts: &[&str],
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let count = parts
            .iter()
            .try_fold(0usize, |n, text| n.checked_add(text.len()))
            .ok_or_else(|| fail(scope))?;
        let mut text = Self::capacity(count, scope)?;
        for part in parts {
            text.text.push_str(part);
        }
        Ok(text)
    }
    pub(super) fn filtered(
        source: &str,
        filter: impl FnOnce(&str, &mut String),
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let mut text = Self::capacity(source.len(), scope)?;
        let capacity = text.text.capacity();
        filter(source, &mut text.text);
        debug_assert_eq!(
            capacity,
            text.text.capacity(),
            "the filter only removes source characters"
        );
        Ok(text)
    }
}

pub(super) struct NativeCaptures {
    pub(super) values: Vec<ObjectId>,
    _charge: Option<SharedSchedulingCharge>,
}
impl NativeCaptures {
    pub(super) fn new(count: usize, scope: Option<&SchedulingScope>) -> Result<Self, RuntimeError> {
        let bytes = count
            .checked_mul(std::mem::size_of::<ObjectId>())
            .ok_or_else(|| fail(scope))?;
        let mut charge = scope.map(|scope| scope.shared_charge(bytes)).transpose()?;
        let mut values = Vec::new();
        values.try_reserve_exact(count).map_err(|_| fail(scope))?;
        if let Some(charge) = &mut charge {
            charge.resize(
                values
                    .capacity()
                    .saturating_mul(std::mem::size_of::<ObjectId>()),
            )?;
        }
        Ok(Self {
            values,
            _charge: charge,
        })
    }
}

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
