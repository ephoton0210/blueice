// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Retained error-report inputs and the original accounts across callbacks.

use super::error_input_storage::utf16_from_utf8;
use super::host_scheduling::SharedSchedulingCharge;
use super::native_value_storage::NativeValue;
use super::*;
use crate::diagnostics::DiagnosticSourceLocation;
use crate::heap::DragEventState;

pub(super) struct ConsoleOwners {
    pub owner: Option<SharedSchedulingCharge>,
    pub caller: Option<SharedSchedulingCharge>,
    sources: [Option<SharedSchedulingCharge>; 2],
    pub context: crate::vm::web_event_storage::EventContext,
}

impl ConsoleOwners {
    pub(super) fn new(
        owner: Option<SharedSchedulingCharge>,
        caller: Option<SharedSchedulingCharge>,
        context: crate::vm::web_event_storage::EventContext,
    ) -> Self {
        Self {
            owner,
            caller,
            sources: [None, None],
            context,
        }
    }

    pub(super) fn with_sources(
        mut self,
        first: Option<SharedSchedulingCharge>,
        second: Option<SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        self.sources = [first, second];
        self.validate()?;
        Ok(self)
    }

    pub(super) fn validate(&self) -> Result<(), RuntimeError> {
        self.context.validate()?;
        for owner in self
            .owner
            .iter()
            .chain(self.caller.iter())
            .chain(self.sources.iter().flatten())
        {
            owner.reserve_peer(0)?;
        }
        Ok(())
    }
}

pub(super) struct ReportOrigin {
    pub value: Option<DiagnosticSourceLocation>,
    // Destroy the filename before returning its independent native credit.
    storage: Option<SharedSchedulingCharge>,
}

impl ReportOrigin {
    pub(super) fn adopt(
        value: Option<DiagnosticSourceLocation>,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let storage = owner
            .map(|owner| owner.reserve_peer(Vm::diagnostic_origin_bytes(&value)))
            .transpose()?;
        Ok(Self { value, storage })
    }

    pub(super) fn from_parts(
        value: Option<DiagnosticSourceLocation>,
        storage: Option<SharedSchedulingCharge>,
    ) -> Self {
        Self { value, storage }
    }

    pub(super) fn copied(
        value: Option<&DiagnosticSourceLocation>,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        let mut storage = owner
            .map(|owner| owner.reserve_peer(value.map_or(0, |source| source.filename.len())))
            .transpose()?;
        let value = value.cloned();
        if let Some(storage) = &mut storage {
            storage.resize(Vm::diagnostic_origin_bytes(&value))?;
        }
        Ok(Self { value, storage })
    }

    pub(super) fn owner_view(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        self.storage
            .as_ref()
            .map(|owner| owner.reserve_peer(0))
            .transpose()
    }

    pub(super) fn retain_for_owner(
        &mut self,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<(), RuntimeError> {
        self.owner_view()?;
        if let Some(owner) = owner {
            if self
                .storage
                .as_ref()
                .is_none_or(|storage| !storage.same_owner(owner))
            {
                self.storage = Some(owner.reserve_peer(Vm::diagnostic_origin_bytes(&self.value))?);
            }
        }
        Ok(())
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        Option<DiagnosticSourceLocation>,
        Option<SharedSchedulingCharge>,
    ) {
        (self.value, self.storage)
    }

    pub(super) fn restore(self, destination: &mut Option<DiagnosticSourceLocation>) {
        *destination = self.value;
        drop(self.storage);
    }
}

pub(super) struct PreparedReportEvent {
    state: DragEventState,
    storage: Option<SharedSchedulingCharge>,
}

impl PreparedReportEvent {
    pub(super) fn new(
        vm: &Vm,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<Self, RuntimeError> {
        const ATTRIBUTES: usize = 11;
        let mut storage = owner
            .map(|owner| owner.reserve_peer(ATTRIBUTES * std::mem::size_of::<(&str, Value)>()))
            .transpose()?;
        let mut state = DragEventState::new(6);
        state
            .attributes
            .try_reserve_exact(ATTRIBUTES)
            .map_err(|_| {
                owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
            })?;
        if let Some(storage) = &mut storage {
            storage.resize(host_scheduling::slots(&state.attributes))?;
        }
        let (name, name_storage) = utf16_from_utf8("error", owner)?.into_retained_parts();
        let Value::String(name) = name else {
            unreachable!()
        };
        vm.initialize_web_event_state(&mut state, name);
        state.cancelable = true;
        let mut result = Self { state, storage };
        result.absorb(name_storage);
        Ok(result)
    }

    fn absorb(&mut self, storage: Option<SharedSchedulingCharge>) {
        match (&mut self.storage, storage) {
            (Some(owner), Some(storage)) => owner
                .absorb(storage)
                .expect("prepared input has the event account"),
            (None, None) => {}
            _ => unreachable!("all prepared event inputs use one account"),
        }
    }

    pub(super) fn set(&mut self, name: &'static str, value: NativeValue) {
        assert!(self.state.attributes.len() < self.state.attributes.capacity());
        let (value, storage) = value.into_retained_parts();
        self.state.set(name, value);
        self.absorb(storage);
    }

    pub(super) fn number(&mut self, name: &'static str, value: u32) {
        assert!(self.state.attributes.len() < self.state.attributes.capacity());
        self.state.set(name, Value::Number(f64::from(value)));
    }

    pub(super) fn into_heap(
        mut self,
        vm: &mut Vm,
        prototype: ObjectId,
    ) -> Result<ObjectId, RuntimeError> {
        vm.with_roots(|heap| {
            if let Some(storage) = &mut self.storage {
                storage.release_to(0);
            }
            heap.alloc_drag_event(self.state, prototype)
        })
    }
}
