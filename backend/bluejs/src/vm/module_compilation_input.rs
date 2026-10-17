// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parsed module inputs retain native ownership through root-code preparation.

use super::host_scheduling::SharedSchedulingCharge;
use super::module_code_storage::PreparedModuleCode;
use super::*;

pub(super) struct ParsedModuleInput {
    module: crate::Module,
    bytes: usize,
    charge: Option<SharedSchedulingCharge>,
}
impl std::ops::Deref for ParsedModuleInput {
    type Target = crate::Module;
    fn deref(&self) -> &Self::Target {
        &self.module
    }
}
fn fail(scope: Option<&SchedulingScope>) -> RuntimeError {
    scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
}

#[derive(Default)]
struct SourceScratch {
    ids: HashSet<usize>,
    charge: Option<SharedSchedulingCharge>,
}
impl SourceScratch {
    fn insert_new(
        &mut self,
        identity: usize,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        debug_assert!(!self.ids.contains(&identity));
        if self.ids.len() == self.ids.capacity() {
            let count = self.ids.len().checked_add(1).ok_or_else(|| fail(scope))?;
            let minimum = count
                .checked_mul(std::mem::size_of::<usize>())
                .ok_or_else(|| fail(scope))?;
            let mut charge = scope
                .map(|scope| scope.shared_charge(minimum))
                .transpose()?;
            let mut replacement = HashSet::new();
            replacement.try_reserve(count).map_err(|_| fail(scope))?;
            if let Some(charge) = &mut charge {
                charge.resize(
                    replacement
                        .capacity()
                        .saturating_mul(std::mem::size_of::<usize>()),
                )?;
            }
            replacement.extend(self.ids.drain());
            self.ids = replacement;
            self.charge = charge;
        }
        self.ids.insert(identity);
        Ok(())
    }
}
impl ParsedModuleInput {
    pub(super) fn parse(
        source: &str,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let minimum = crate::parser::module_parse_input_bytes(source).ok_or_else(|| fail(scope))?;
        let mut charge = scope
            .map(|scope| scope.shared_charge(minimum))
            .transpose()?;
        let module = crate::parse_module(source).map_err(|error| {
            error
                .resource
                .unwrap_or(RuntimeError::SyntaxError(error.message))
        })?;
        let mut accounted = module
            .payload_without_source_text()
            .ok_or_else(|| fail(scope))?;
        if let Some(charge) = &mut charge {
            charge.resize(accounted)?;
        }
        let mut sources = SourceScratch::default();
        let mut failure = None;
        let bytes = module
            .payload_with_source_admission(&mut |identity, length| {
                if sources.ids.contains(&identity) {
                    return Some(false);
                }
                let result: Result<bool, RuntimeError> = (|| {
                    let next = accounted.checked_add(length).ok_or_else(|| fail(scope))?;
                    if let Some(charge) = &mut charge {
                        charge.resize(next)?;
                    }
                    sources.insert_new(identity, scope)?;
                    accounted = next;
                    Ok(true)
                })();
                match result {
                    Ok(unique) => Some(unique),
                    Err(error) => {
                        failure = Some(error);
                        None
                    }
                }
            })
            .ok_or_else(|| failure.unwrap_or_else(|| fail(scope)))?;
        debug_assert_eq!(bytes, accounted);
        drop(sources);
        Ok(Self {
            module,
            bytes,
            charge,
        })
    }
    #[cfg(any(test, coverage))]
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }
    pub(super) fn compile(
        mut self,
        scope: Option<&SchedulingScope>,
    ) -> Result<PreparedModuleCode, RuntimeError> {
        match (&self.charge, scope) {
            (Some(owner), Some(scope)) if !owner.belongs_to(scope) => return Err(scope.fail()),
            (Some(owner), None) => return Err(owner.fail()),
            (None, Some(scope)) => self.charge = Some(scope.shared_charge(self.bytes)?),
            _ => {}
        }
        if let Some(scope) = scope {
            scope.shared_charge(0)?;
        }
        let code = crate::compile_module_with_limit(&self.module, u32::MAX)
            .map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        let mut code = PreparedModuleCode::input(code);
        code.attach(scope)?;
        // Both owners coexist until root buffers are admitted. Drop the AST
        // before any registry copy or publication; shared program data is a
        // separate ownership boundary from these root buffers.
        drop(self);
        Ok(code)
    }
}
