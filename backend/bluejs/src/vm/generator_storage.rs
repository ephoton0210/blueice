// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Suspended buffers retain native ownership until the heap adopts them.

use super::*;

pub(super) struct OwnedGeneratorState {
    state: GeneratorState,
    charge: execution_payload::ExecutionPayloadCharge,
}

impl OwnedGeneratorState {
    pub(super) fn admit(self, vm: &mut Vm) -> Result<Self, RuntimeError> {
        // Terminal cleanup must still close a frame after a resource abort.
        if !matches!(self.state, GeneratorState::Done | GeneratorState::Running) {
            if let Err(error) = vm.synchronize_scheduling_usage() {
                drop(self);
                vm.reconcile_scheduling_ownership();
                return Err(error);
            }
        }
        Ok(self)
    }

    pub(super) fn store(self, vm: &mut Vm, generator: ObjectId) -> Result<(), RuntimeError> {
        let owned = match self.admit(vm) {
            Ok(owned) => owned,
            Err(error) => {
                let _ = vm.heap.set_generator_state(generator, GeneratorState::Done);
                return Err(error);
            }
        };
        let Self { state, charge } = owned;
        vm.with_roots(|heap| {
            let result = heap.set_generator_state(generator, state);
            // Release inside the heap operation, before the VM reconciles
            // collection effects and queues native cleanup work.
            drop(charge);
            result
        })
    }
}

impl Vm {
    pub(super) fn own_generator_state(&self, state: GeneratorState) -> OwnedGeneratorState {
        let charge = self.charge_execution_control(state.owned_buffer_bytes());
        OwnedGeneratorState { state, charge }
    }
}
