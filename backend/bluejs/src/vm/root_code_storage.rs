// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Borrowed outer buffers and saved debugger code have distinct native owners.

use super::host_scheduling::{cloned_code_bytes, owned_code_bytes, SharedSchedulingCharge};
use super::*;

pub(super) struct BorrowedRootCode<'a> {
    _code: &'a Bytecode,
    _charge: Option<SharedSchedulingCharge>,
}
impl<'a> BorrowedRootCode<'a> {
    pub(super) fn new(
        code: &'a Bytecode,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let charge = scope
            .map(|scope| scope.shared_charge(owned_code_bytes(code)))
            .transpose()?;
        if let Some(scope) = scope {
            code.attach_runtime_sources(&scope.heap_native_budget())?;
        }
        Ok(Self {
            _code: code,
            _charge: charge,
        })
    }
}

pub(super) struct SavedRootCode {
    bytecode: Bytecode,
    charge: Option<SharedSchedulingCharge>,
}
impl std::ops::Deref for SavedRootCode {
    type Target = Bytecode;
    fn deref(&self) -> &Bytecode {
        &self.bytecode
    }
}
impl SavedRootCode {
    #[cfg(any(test, coverage))]
    pub(super) fn set_generation_for_test(&mut self, generation: Option<u64>) {
        self.bytecode.debugger_program_generation = generation;
    }
    pub(super) fn copy(code: &Bytecode, vm: &mut Vm) -> Result<Self, RuntimeError> {
        let budget = vm
            .host_scheduling
            .as_ref()
            .map(SchedulingScope::heap_native_budget);
        let minimum = std::mem::size_of::<Self>().saturating_add(cloned_code_bytes(code));
        let charge = budget
            .as_ref()
            .map(|budget| budget.reserve(minimum))
            .transpose()?;
        let mut saved = Self {
            bytecode: code.clone(),
            charge,
        };
        let bytes = saved.retained_bytes();
        if let Some(charge) = &mut saved.charge {
            charge.resize(bytes)?;
        }
        if let Some(budget) = &budget {
            vm.heap.prepare_code_snapshot(&mut saved.bytecode, budget)?;
        }
        Ok(saved)
    }
    pub(super) fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>().saturating_add(owned_code_bytes(&self.bytecode))
    }
    pub(super) fn attach(
        &mut self,
        budget: &HeapNativeBudget,
        heap: &mut Heap,
    ) -> Result<(), RuntimeError> {
        budget.reserve(0)?;
        if let Some(owner) = &self.charge {
            if !budget.owns(owner) {
                return Err(budget.fail());
            }
            return self.bytecode.attach_runtime_sources(budget);
        }
        self.charge = Some(budget.reserve(self.retained_bytes())?);
        heap.prepare_code_snapshot(&mut self.bytecode, budget)?;
        Ok(())
    }
}
