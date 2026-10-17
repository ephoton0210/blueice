// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Immutable outgoing inspections keep data and native credit in one owner.

use super::*;

#[cfg(any(test, coverage))]
#[path = "../../../tests/fixtures/debugger_projection_storage.rs"]
mod projection_tests;

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
impl<T> VmDebuggerInspection<T> {
    /// Admit a complete native projection while the original data stays live.
    /// The converter must obtain new buffers from `output` and retain any
    /// existing allocations transferred into its returned value there as well.
    /// No mutable access to the original or unowned result is exposed.
    pub fn try_map<U>(
        &self,
        buffer_bytes: usize,
        convert: impl FnOnce(&T, &mut VmDebuggerOutput<U>) -> Result<U, RuntimeError>,
    ) -> Result<VmDebuggerInspection<U>, RuntimeError> {
        let mut output = VmDebuggerOutput {
            storage: InspectionStorage::peer(self.inner._storage.as_ref(), buffer_bytes)?,
        };
        let value = convert(&self.inner.value, &mut output)?;
        output.storage.finish(value)
    }
}

/// Pre-admitted storage for one immutable native debugger projection.
///
/// This stage is supplied only to `VmDebuggerInspection::try_map`. Its complete
/// buffer plan must cover all new and transferred allocations in the result.
/// Explicit copies made outside this stage remain the embedding host's owners.
pub struct VmDebuggerOutput<T> {
    storage: InspectionStorage<T>,
}
impl<T> VmDebuggerOutput<T> {
    /// Allocate admitted slots, reconciling reported excess before copying.
    pub fn vector<U>(&mut self, count: usize) -> Result<Vec<U>, RuntimeError> {
        self.storage.vector(count)
    }
    /// Copy plain slots without running a user-defined clone implementation.
    pub fn copy_slice<U: Copy>(&mut self, data: &[U]) -> Result<Vec<U>, RuntimeError> {
        let mut output = self.vector(data.len())?;
        output.extend_from_slice(data);
        Ok(output)
    }
    /// Allocate an admitted payload header after its contents are prepared.
    pub fn boxed<U>(&mut self, value: U) -> Result<Box<U>, RuntimeError> {
        self.storage.consume(std::mem::size_of::<U>())?;
        Ok(Box::new(value))
    }
    /// Attach previously allocated data moved into this projection. Include
    /// actual backing capacities and boxed payloads in the complete plan.
    /// Use this only for raw allocations without an existing native lease.
    pub fn retain_bytes(&mut self, bytes: usize) -> Result<(), RuntimeError> {
        self.storage.consume(bytes)
    }
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
    remaining: usize,
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
            remaining: buffers,
            charge,
        })
    }
    fn peer(owner: Option<&HeapNativeCharge>, buffers: usize) -> Result<Self, RuntimeError> {
        let bytes = std::mem::size_of::<InspectionData<T>>()
            .checked_add(buffers)
            .ok_or_else(|| owner.map_or(RuntimeError::SchedulingLimit, HeapNativeCharge::fail))?;
        let charge = owner.map(|owner| owner.reserve_peer(bytes)).transpose()?;
        Ok(Self {
            output: std::marker::PhantomData,
            bytes,
            remaining: buffers,
            charge,
        })
    }
    fn consume(&mut self, bytes: usize) -> Result<(), RuntimeError> {
        self.remaining = self
            .remaining
            .checked_sub(bytes)
            .ok_or_else(|| self.fail())?;
        Ok(())
    }
    fn fail(&self) -> RuntimeError {
        self.charge
            .as_ref()
            .map_or(RuntimeError::SchedulingLimit, HeapNativeCharge::fail)
    }
    /// All requested slots were admitted in the complete output plan. Reconcile
    /// allocator-reported excess before copying into this candidate buffer.
    pub(crate) fn vector<U>(&mut self, count: usize) -> Result<Vec<U>, RuntimeError> {
        let requested = count
            .checked_mul(std::mem::size_of::<U>())
            .ok_or_else(|| self.fail())?;
        self.consume(requested)?;
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
    pub(crate) fn finish(self, value: T) -> Result<VmDebuggerInspection<T>, RuntimeError> {
        if self.remaining != 0 {
            let error = self.fail();
            drop(value);
            return Err(error);
        }
        if let Some(owner) = &self.charge {
            owner.reserve_peer(0)?;
        }
        Ok(VmDebuggerInspection {
            inner: Rc::new(InspectionData {
                value,
                _storage: self.charge,
            }),
        })
    }
}
