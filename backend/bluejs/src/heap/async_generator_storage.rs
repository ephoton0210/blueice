// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Async control buffers and setter roots retain admitted native backing.

use super::*;
use crate::vm::HeapNativeCharge;

impl AsyncGeneratorRequest {
    pub(super) fn unowned_value_bytes(&self) -> usize {
        if self.value_storage.is_some() {
            0
        } else {
            self.completion.value().payload_bytes()
        }
    }

    pub(super) fn value_can_join(&self, budget: &HeapNativeBudget) -> bool {
        self.value_storage
            .as_ref()
            .is_none_or(|storage| budget.owns(storage))
    }

    fn retain_value(&mut self, budget: &HeapNativeBudget) -> Result<(), HeapError> {
        if !self.value_can_join(budget) {
            return Err(HeapError::AsyncGeneratorStorageLimit);
        }
        let bytes = HeapNativeBudget::value_payload_bytes(self.completion.value());
        if let Some(storage) = &mut self.value_storage {
            storage
                .resize(bytes)
                .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?;
        } else {
            self.value_storage = Some(
                budget
                    .reserve(bytes)
                    .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?,
            );
        }
        Ok(())
    }

    pub(crate) fn try_snapshot(&self, hint: Option<&HeapNativeBudget>) -> Result<Self, HeapError> {
        let bytes = HeapNativeBudget::value_payload_bytes(self.completion.value());
        let mut storage = if let Some(storage) = &self.value_storage {
            if hint.is_some_and(|budget| !budget.owns(storage)) {
                return Err(HeapError::AsyncGeneratorStorageLimit);
            }
            Some(
                storage
                    .reserve_peer(bytes)
                    .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?,
            )
        } else {
            self.scheduling
                .owner()
                .as_ref()
                .or(hint)
                .map(|budget| {
                    budget
                        .reserve(bytes)
                        .map_err(|_| HeapError::AsyncGeneratorStorageLimit)
                })
                .transpose()?
        };
        let completion = self.completion.clone();
        if let Some(storage) = &mut storage {
            storage
                .resize(HeapNativeBudget::value_payload_bytes(completion.value()))
                .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?;
        }
        Ok(Self {
            id: self.id,
            completion,
            target: self.target,
            origin: self.origin.clone(),
            scheduling: self.scheduling.clone(),
            value_storage: storage,
        })
    }
}

impl std::ops::Deref for AsyncGeneratorOrigin {
    type Target = crate::diagnostics::ThrowOrigin;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl AsyncGeneratorOrigin {
    pub(crate) fn allocation_bytes() -> usize {
        size_of::<Self>() + 2 * size_of::<usize>()
    }

    pub(crate) fn prepared(
        payload_bytes: usize,
        budget: Option<&HeapNativeBudget>,
        produce: impl FnOnce() -> crate::diagnostics::ThrowOrigin,
    ) -> Result<Rc<Self>, HeapError> {
        let mut storage = budget
            .map(|budget| {
                budget
                    .reserve(Self::allocation_bytes().saturating_add(payload_bytes))
                    .map_err(|_| HeapError::AsyncGeneratorStorageLimit)
            })
            .transpose()?;
        let value = produce();
        if let Some(storage) = &mut storage {
            storage
                .resize(Self::allocation_bytes().saturating_add(value.payload_bytes()))
                .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?;
        }
        Ok(Rc::new(Self {
            value,
            storage: std::cell::RefCell::new(storage),
        }))
    }

    #[cfg(any(test, coverage))]
    pub(crate) fn unowned(value: crate::diagnostics::ThrowOrigin) -> Rc<Self> {
        Self::prepared(value.payload_bytes(), None, || value)
            .expect("standalone provenance needs no account admission")
    }

    pub(super) fn unowned_bytes(&self) -> usize {
        if self.storage.borrow().is_none() {
            Self::allocation_bytes().saturating_add(self.value.payload_bytes())
        } else {
            0
        }
    }

    pub(super) fn can_join(&self, budget: &HeapNativeBudget) -> bool {
        self.storage
            .borrow()
            .as_ref()
            .is_none_or(|storage| budget.owns(storage))
    }

    fn retain(&self, budget: &HeapNativeBudget) -> Result<(), HeapError> {
        let mut retained = self.storage.borrow_mut();
        let bytes = Self::allocation_bytes().saturating_add(self.value.payload_bytes());
        if let Some(storage) = retained.as_mut() {
            if !budget.owns(storage) {
                return Err(HeapError::AsyncGeneratorStorageLimit);
            }
            storage
                .resize(bytes)
                .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?;
        } else {
            *retained = Some(
                budget
                    .reserve(bytes)
                    .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?,
            );
        }
        Ok(())
    }

    pub(crate) fn copied_value(
        &self,
    ) -> Result<(crate::diagnostics::ThrowOrigin, Option<HeapNativeCharge>), HeapError> {
        let mut storage = self
            .storage
            .borrow()
            .as_ref()
            .map(|storage| {
                storage
                    .reserve_peer(self.value.payload_bytes())
                    .map_err(|_| HeapError::AsyncGeneratorStorageLimit)
            })
            .transpose()?;
        let value = self.value.clone();
        if let Some(storage) = &mut storage {
            storage
                .resize(value.payload_bytes())
                .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?;
        }
        Ok((value, storage))
    }
}

impl AsyncGeneratorControl {
    pub(crate) fn backing_bytes(&self) -> usize {
        self.requests
            .capacity()
            .saturating_mul(size_of::<AsyncGeneratorRequest>())
    }
    pub(super) fn retain_backing(
        &mut self,
        budget: Option<&HeapNativeBudget>,
    ) -> Result<(), HeapError> {
        let Some(budget) = budget else {
            return Ok(());
        };
        let bytes = self.backing_bytes();
        if let Some(storage) = &mut self.storage {
            if !budget.owns(storage) {
                return Err(HeapError::AsyncGeneratorStorageLimit);
            }
            storage
                .resize(bytes)
                .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?;
        } else {
            self.storage = Some(
                budget
                    .reserve(bytes)
                    .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?,
            );
        }
        self.owner = Some(budget.clone());
        for request in &mut self.requests {
            request.origin.retain(budget)?;
            request.retain_value(budget)?;
        }
        Ok(())
    }
    pub(super) fn try_snapshot(&self, hint: Option<&HeapNativeBudget>) -> Result<Self, HeapError> {
        let budget = self.owner.as_ref().or(hint);
        let mut output = Self {
            status: self.status,
            requests: VecDeque::new(),
            next_request_id: self.next_request_id,
            owner: budget.cloned(),
            storage: budget
                .map(|budget| {
                    budget
                        .reserve(
                            self.requests
                                .len()
                                .saturating_mul(size_of::<AsyncGeneratorRequest>()),
                        )
                        .map_err(|_| HeapError::AsyncGeneratorStorageLimit)
                })
                .transpose()?,
        };
        output
            .requests
            .try_reserve_exact(self.requests.len())
            .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?;
        let bytes = output.backing_bytes();
        if let Some(storage) = &mut output.storage {
            storage
                .resize(bytes)
                .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?;
        }
        for request in &self.requests {
            output.requests.push_back(request.try_snapshot(budget)?);
        }
        Ok(output)
    }
    pub(crate) fn reserve_request_slot(&mut self) -> Result<(), HeapError> {
        let budget = self.owner.clone();
        if self.requests.len() < self.requests.capacity() {
            return self.retain_backing(budget.as_ref());
        }
        let count = self.requests.capacity().saturating_mul(2).max(4);
        let mut storage = budget
            .as_ref()
            .map(|budget| {
                budget
                    .reserve(count.saturating_mul(size_of::<AsyncGeneratorRequest>()))
                    .map_err(|_| HeapError::AsyncGeneratorStorageLimit)
            })
            .transpose()?;
        let mut replacement = VecDeque::new();
        replacement
            .try_reserve_exact(count)
            .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?;
        if let Some(storage) = &mut storage {
            storage
                .resize(
                    replacement
                        .capacity()
                        .saturating_mul(size_of::<AsyncGeneratorRequest>()),
                )
                .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?;
        }
        replacement.append(&mut self.requests);
        self.requests = replacement;
        self.storage = storage;
        Ok(())
    }
    pub(crate) fn push_request(&mut self, request: AsyncGeneratorRequest) -> Result<(), HeapError> {
        self.reserve_request_slot()?;
        self.requests.push_back(request);
        Ok(())
    }
    pub(super) fn release_empty(&mut self) {
        if self.requests.is_empty() {
            self.requests = VecDeque::new();
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
        }
    }
}

pub(crate) struct AsyncGeneratorRoots {
    data: Vec<ObjectId>,
    new_count: usize,
    storage: Option<HeapNativeCharge>,
}
impl std::ops::Deref for AsyncGeneratorRoots {
    type Target = [ObjectId];
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl AsyncGeneratorRoots {
    #[cfg(any(test, coverage))]
    pub(crate) fn backing_bytes(&self) -> usize {
        self.data.capacity().saturating_mul(size_of::<ObjectId>())
    }
    pub(super) fn prepare(
        object: ObjectId,
        input: &AsyncGeneratorControl,
        old: Option<&AsyncGeneratorControl>,
        budget: Option<&HeapNativeBudget>,
    ) -> Result<Self, HeapError> {
        let mut new_count = 0usize;
        input.visit_references(&mut |_| new_count += 1);
        let mut count = 1usize.saturating_add(new_count);
        if let Some(old) = old {
            old.visit_references(&mut |_| count += 1);
        }
        let mut roots = Self {
            data: Vec::new(),
            new_count,
            storage: budget
                .map(|budget| {
                    budget
                        .reserve(count.saturating_mul(size_of::<ObjectId>()))
                        .map_err(|_| HeapError::AsyncGeneratorStorageLimit)
                })
                .transpose()?,
        };
        roots
            .data
            .try_reserve_exact(count)
            .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?;
        if let Some(storage) = &mut roots.storage {
            storage
                .resize(roots.data.capacity().saturating_mul(size_of::<ObjectId>()))
                .map_err(|_| HeapError::AsyncGeneratorStorageLimit)?;
        }
        roots.data.push(object);
        input.visit_references(&mut |id| roots.data.push(id));
        if let Some(old) = old {
            old.visit_references(&mut |id| roots.data.push(id));
        }
        Ok(roots)
    }
    pub(super) fn new_references(&self) -> &[ObjectId] {
        &self.data[1..1 + self.new_count]
    }
}

impl Heap {
    pub(crate) fn prepare_async_generator_roots(
        &self,
        object: ObjectId,
        input: &AsyncGeneratorControl,
    ) -> Result<AsyncGeneratorRoots, HeapError> {
        let old = self.async_generator_control_ref(object)?;
        let budget = old
            .and_then(|control| control.owner.clone())
            .or_else(|| self.collector.budget().cloned())
            .or_else(|| input.owner.clone());
        AsyncGeneratorRoots::prepare(object, input, old, budget.as_ref())
    }
    pub(crate) fn retain_async_generator_backing(
        &mut self,
        budget: &HeapNativeBudget,
        only: Option<ObjectId>,
    ) -> Result<(), HeapError> {
        if !self.async_generator_counts_can_join_for(budget, only) {
            return Err(HeapError::InvalidConfig);
        }
        for (id, object) in self.objects.iter_mut() {
            if only.is_some_and(|only| only != *id) {
                continue;
            }
            if let ObjectKind::Generator {
                async_control: Some(control),
                async_control_bytes,
                ..
            } = &mut object.kind
            {
                let result = control.retain_backing(Some(budget));
                let bytes = control.managed_bytes();
                object.bytes = object.bytes - *async_control_bytes + bytes;
                self.managed_bytes
                    .set(self.managed_bytes.get() - *async_control_bytes + bytes);
                *async_control_bytes = bytes;
                result?;
            }
        }
        Ok(())
    }
    pub(crate) fn set_async_generator_status(
        &mut self,
        object: ObjectId,
        status: AsyncGeneratorStatus,
    ) -> Result<(), HeapError> {
        let (control, _, _) = self.async_generator_slots_mut(object)?;
        control
            .as_mut()
            .ok_or(HeapError::InvalidInternalSlot(object))?
            .status = status;
        Ok(())
    }
    pub(crate) fn pop_async_generator_request(
        &mut self,
        object: ObjectId,
        status: AsyncGeneratorStatus,
    ) -> Result<(), HeapError> {
        let (old, new) = {
            let (slot, bytes, object_bytes) = self.async_generator_slots_mut(object)?;
            let control = slot
                .as_mut()
                .ok_or(HeapError::InvalidInternalSlot(object))?;
            let request = control
                .requests
                .pop_front()
                .ok_or(HeapError::InvalidInternalSlot(object))?;
            request.scheduling.retire();
            drop(request);
            control.status = status;
            control.release_empty();
            let new = control.managed_bytes();
            let old = *bytes;
            *bytes = new;
            *object_bytes = *object_bytes - old + new;
            (old, new)
        };
        self.managed_bytes.set(self.managed_bytes.get() - old + new);
        Ok(())
    }
}
