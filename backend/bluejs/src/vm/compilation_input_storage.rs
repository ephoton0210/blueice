// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared retained AST inputs for runtime module and Function compilation.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;
use std::sync::Arc;

type SourceVisitor<'a> = &'a mut dyn FnMut(&Arc<str>) -> Option<bool>;
pub(super) trait CompilationAst {
    fn without_sources(&self) -> Option<usize>;
    fn with_sources(&self, visitor: SourceVisitor<'_>) -> Option<usize>;
}
macro_rules! compilation_ast {
    ($($ty:ty),*) => {$(impl CompilationAst for $ty {
        fn without_sources(&self) -> Option<usize> { self.payload_without_source_text() }
        fn with_sources(&self, visitor: SourceVisitor<'_>) -> Option<usize> { self.payload_with_source_admission(visitor) }
    })*};
}
compilation_ast!(crate::Module, crate::Program);

pub(super) struct CompilationInput<T> {
    pub(super) ast: T,
    sources: super::compiled_source_storage::SourceOwners,
    bytes: usize,
    charge: Option<SharedSchedulingCharge>,
}
impl<T> std::ops::Deref for CompilationInput<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.ast
    }
}
pub(super) fn fail(scope: Option<&SchedulingScope>) -> RuntimeError {
    scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
}
pub(super) fn parsing_charge(
    source: &str,
    scope: Option<&SchedulingScope>,
) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
    let minimum = crate::parser::module_parse_input_bytes(source).ok_or_else(|| fail(scope))?;
    scope.map(|scope| scope.shared_charge(minimum)).transpose()
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

impl<T: CompilationAst> CompilationInput<T> {
    pub(super) fn from_parsed(
        ast: T,
        charge: Option<SharedSchedulingCharge>,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let mut input = Self {
            ast,
            sources: super::compiled_source_storage::SourceOwners::default(),
            bytes: 0,
            charge,
        };
        input.reprice(scope)?;
        Ok(input)
    }
    #[cfg(any(test, coverage))]
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }
    pub(super) fn reserve_extra(&mut self, bytes: usize) -> Result<(), RuntimeError> {
        if let Some(charge) = &mut self.charge {
            let next = charge
                .bytes()
                .checked_add(bytes)
                .ok_or_else(|| charge.fail())?;
            charge.resize(next)?;
        }
        Ok(())
    }
    pub(super) fn reprice(&mut self, scope: Option<&SchedulingScope>) -> Result<(), RuntimeError> {
        let mut accounted = self.ast.without_sources().ok_or_else(|| fail(scope))?;
        if let Some(charge) = &mut self.charge {
            if charge.bytes() < accounted {
                charge.resize(accounted)?;
            }
        }
        let mut sources = SourceScratch::default();
        let mut failure = None;
        let bytes = self
            .ast
            .with_sources(&mut |source| {
                let identity = source.as_ptr() as usize;
                if sources.ids.contains(&identity) {
                    return Some(false);
                }
                let result: Result<bool, RuntimeError> = (|| {
                    let next = accounted
                        .checked_add(source.len())
                        .ok_or_else(|| fail(scope))?;
                    if let Some(charge) = &mut self.charge {
                        if charge.bytes() < next {
                            charge.resize(next)?;
                        }
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
        if let Some(charge) = &mut self.charge {
            if bytes <= charge.bytes() {
                charge.release_to(bytes);
            } else {
                charge.resize(bytes)?;
            }
        }
        self.bytes = bytes;
        Ok(())
    }
    pub(super) fn attach_input(
        &mut self,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        match (&self.charge, scope) {
            (Some(owner), Some(scope)) if !owner.belongs_to(scope) => return Err(scope.fail()),
            (Some(owner), None) => return Err(owner.fail()),
            (None, Some(scope)) => self.charge = Some(scope.shared_charge(self.bytes)?),
            _ => {}
        }
        if let Some(scope) = scope {
            scope.shared_charge(0)?;
        }
        Ok(())
    }
    pub(super) fn prepare(&mut self, scope: Option<&SchedulingScope>) -> Result<(), RuntimeError> {
        self.attach_input(scope)?;
        let mut failure = None;
        self.ast
            .with_sources(
                &mut |source| match self.sources.insert(source, &mut self.charge, scope) {
                    Ok(unique) => Some(unique),
                    Err(error) => {
                        failure = Some(error);
                        None
                    }
                },
            )
            .ok_or_else(|| failure.unwrap_or_else(|| fail(scope)))?;
        Ok(())
    }
    pub(super) fn install(
        &self,
        code: &mut Bytecode,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        self.sources.install(code, scope)
    }
}
