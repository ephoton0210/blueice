// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bind collector, persistent-root and foreign-pin storage to the native account.

use super::*;

impl Vm {
    pub(super) fn bind_native_budget(&mut self) -> Result<(), RuntimeError> {
        if let Some(scope) = &self.host_scheduling {
            let budget = scope.heap_native_budget();
            self.heap.bind_native_budget(budget.clone())?;
            if let Some(frame) = &mut self.debugger_continuation {
                frame.code.attach(&budget, &mut self.heap)?;
            }
            if let Some(frame) = &mut self.debugger_nested_continuation {
                frame.code.attach(&budget, &mut self.heap)?;
            }
            if let Some(frame) = &mut self.debugger_nested_intermediate_continuation {
                frame.code.attach(&budget, &mut self.heap)?;
            }
        }
        Ok(())
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/collector_workspace.rs"]
mod ownership_tests;
