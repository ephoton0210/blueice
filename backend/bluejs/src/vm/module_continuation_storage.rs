// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module suspensions retain their immutable code and admitted identifiers.

use super::*;

impl Vm {
    pub(super) fn prepare_module_frame_name(
        &mut self,
        name: &str,
    ) -> Result<(String, execution_payload::ExecutionPayloadCharge), RuntimeError> {
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: name.len(),
            ..Default::default()
        })?;
        let name = module_referrers::copy_referrer(name, self.host_scheduling.as_ref())?;
        let charge = self.charge_execution_control(name.capacity());
        if let Err(error) = self.synchronize_scheduling_usage() {
            drop(charge);
            self.reconcile_scheduling_ownership();
            return Err(error);
        }
        Ok((name, charge))
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/module_continuation_ownership.rs"]
mod ownership_tests;
