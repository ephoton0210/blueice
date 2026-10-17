// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Immutable outgoing inspections keep data and native credit in one owner.

use super::*;

struct InspectionData<T> {
    value: T,
    _storage: Option<HeapNativeCharge>,
}

/// An immutable debugger result with document-owned native storage.
///
/// Cloning shares the same data and credit without allocating another copy.
/// Results can outlive their VM; the last clone drops data before returning its
/// native credit. Explicit copies made by an embedding host belong to that host.
pub struct VmDebuggerInspection<T> {
    inner: Rc<InspectionData<T>>,
}
impl<T> Clone for VmDebuggerInspection<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
impl<T> std::ops::Deref for VmDebuggerInspection<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.inner.value
    }
}
impl<T> AsRef<T> for VmDebuggerInspection<T> {
    fn as_ref(&self) -> &T {
        &self.inner.value
    }
}
impl<T: std::fmt::Debug> std::fmt::Debug for VmDebuggerInspection<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.value.fmt(f)
    }
}
impl<T: PartialEq> PartialEq for VmDebuggerInspection<T> {
    fn eq(&self, other: &Self) -> bool {
        self.inner.value == other.inner.value
    }
}
impl<T: Eq> Eq for VmDebuggerInspection<T> {}

pub(crate) struct InspectionStorage<T> {
    output: std::marker::PhantomData<fn() -> T>,
    bytes: usize,
    charge: Option<HeapNativeCharge>,
}
impl<T> InspectionStorage<T> {
    pub(crate) fn new(
        budget: Option<&HeapNativeBudget>,
        buffers: usize,
    ) -> Result<Self, RuntimeError> {
        let bytes = std::mem::size_of::<InspectionData<T>>()
            .checked_add(buffers)
            .ok_or_else(|| budget.map_or(RuntimeError::SchedulingLimit, HeapNativeBudget::fail))?;
        let charge = budget.map(|budget| budget.reserve(bytes)).transpose()?;
        Ok(Self {
            output: std::marker::PhantomData,
            bytes,
            charge,
        })
    }
    fn fail(&self) -> RuntimeError {
        self.charge
            .as_ref()
            .map_or(RuntimeError::SchedulingLimit, HeapNativeCharge::fail)
    }
    /// All requested slots were admitted in the complete output plan. Reconcile
    /// allocator-reported excess before copying into this candidate buffer.
    pub(crate) fn vector<U>(&mut self, count: usize) -> Result<Vec<U>, RuntimeError> {
        let mut vector = Vec::new();
        vector.try_reserve_exact(count).map_err(|_| self.fail())?;
        let excess = vector
            .capacity()
            .checked_sub(count)
            .and_then(|n| n.checked_mul(std::mem::size_of::<U>()))
            .ok_or_else(|| self.fail())?;
        let bytes = self.bytes.checked_add(excess).ok_or_else(|| self.fail())?;
        if let Some(charge) = &mut self.charge {
            charge.resize(bytes)?;
        }
        self.bytes = bytes;
        Ok(vector)
    }
    pub(crate) fn finish(self, value: T) -> VmDebuggerInspection<T> {
        VmDebuggerInspection {
            inner: Rc::new(InspectionData {
                value,
                _storage: self.charge,
            }),
        }
    }
}
