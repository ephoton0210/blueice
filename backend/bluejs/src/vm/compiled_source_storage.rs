// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Transfer parsed source leases into independently shared runtime code.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;
use std::sync::Arc;

struct SourceOwner {
    // Release the actual source before refunding its lease.
    _text: Arc<str>,
    bytes: usize,
    charge: RefCell<Option<SharedSchedulingCharge>>,
}
impl SourceOwner {
    fn attach(&self, budget: &HeapNativeBudget) -> Result<(), RuntimeError> {
        let mut charge = self.charge.borrow_mut();
        if let Some(owner) = charge.as_ref() {
            if !budget.owns(owner) {
                return Err(budget.fail());
            }
        } else {
            *charge = Some(budget.reserve(self.bytes)?);
        }
        Ok(())
    }
}

/// Inline metadata: only runtime source/child-code owners bind a document,
/// preserving cross-VM reuse of ordinary host-compiled code.
#[derive(Clone, Default)]
pub(crate) struct RuntimeCodeSources {
    source: Option<Rc<SourceOwner>>,
    present: bool,
    document: RefCell<Option<HeapNativeBudget>>,
}

impl Bytecode {
    #[cfg(any(test, coverage))]
    pub(crate) fn runtime_source_bytes(&self) -> usize {
        self.runtime_sources
            .source
            .as_ref()
            .map_or(0, |source| source.bytes)
    }

    pub(crate) fn attach_runtime_sources(
        &self,
        budget: &HeapNativeBudget,
    ) -> Result<(), RuntimeError> {
        if !self.runtime_sources.present {
            return Ok(());
        }
        // A cached identity never permits new work after terminal failure.
        budget.reserve(0)?;
        if let Some(owner) = self.runtime_sources.document.borrow().as_ref() {
            return if owner.same_document(budget) {
                Ok(())
            } else {
                Err(budget.fail())
            };
        }
        if let Some(source) = &self.runtime_sources.source {
            source.attach(budget)?;
        }
        for child in &self.functions {
            child.attach_runtime_sources(budget)?;
        }
        *self.runtime_sources.document.borrow_mut() = Some(budget.clone());
        Ok(())
    }
}

/// Compilation-only lookup; its coexisting backing is charged separately from
/// the AST and the per-source owners that survive it.
#[derive(Default)]
pub(super) struct SourceOwners {
    sources: HashMap<usize, Rc<SourceOwner>>,
    charge: Option<SharedSchedulingCharge>,
}
fn fail(scope: Option<&SchedulingScope>) -> RuntimeError {
    scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
}
impl SourceOwners {
    pub(super) fn insert(
        &mut self,
        source: &Arc<str>,
        ast_charge: &mut Option<SharedSchedulingCharge>,
        scope: Option<&SchedulingScope>,
    ) -> Result<bool, RuntimeError> {
        let identity = source.as_ptr() as usize;
        if self.sources.contains_key(&identity) {
            return Ok(false);
        }
        let header = std::mem::size_of::<SourceOwner>();
        let bytes = header
            .checked_add(source.len())
            .ok_or_else(|| fail(scope))?;
        let mut charge = scope.map(|scope| scope.shared_charge(header)).transpose()?;
        if self.sources.len() == self.sources.capacity() {
            let count = self
                .sources
                .len()
                .checked_add(1)
                .ok_or_else(|| fail(scope))?;
            let slot = std::mem::size_of::<(usize, Rc<SourceOwner>)>();
            let minimum = count.checked_mul(slot).ok_or_else(|| fail(scope))?;
            let mut candidate_charge = scope
                .map(|scope| scope.shared_charge(minimum))
                .transpose()?;
            let mut candidate = HashMap::new();
            candidate.try_reserve(count).map_err(|_| fail(scope))?;
            if let Some(charge) = &mut candidate_charge {
                charge.resize(candidate.capacity().saturating_mul(slot))?;
            }
            candidate.extend(self.sources.drain());
            self.sources = candidate;
            self.charge = candidate_charge;
        }
        if let (Some(ast), Some(owner)) = (ast_charge.as_mut(), charge.as_mut()) {
            owner.absorb(ast.split_off(source.len()))?;
        }
        self.sources.insert(
            identity,
            Rc::new(SourceOwner {
                _text: source.clone(),
                bytes,
                charge: RefCell::new(charge),
            }),
        );
        Ok(true)
    }

    pub(super) fn install(
        &self,
        code: &mut Bytecode,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        if let Some(source) = code.source_text.allocation() {
            code.runtime_sources.source = Some(
                self.sources
                    .get(&(source.as_ptr() as usize))
                    .ok_or_else(|| fail(scope))?
                    .clone(),
            );
        }
        let mut present = code.runtime_sources.source.is_some();
        // Fresh compiler output is a tree of unique shared children. Install before
        // publication, without allocating a traversal stack or copying code.
        for child in &mut code.functions {
            self.install(child.get_mut().ok_or_else(|| fail(scope))?, scope)?;
            child.enable_runtime(scope)?;
            present |= child.runtime_sources.present;
        }
        code.runtime_sources.present = present || !code.functions.is_empty();
        Ok(())
    }
}
