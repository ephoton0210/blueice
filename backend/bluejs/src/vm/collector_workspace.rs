// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bind collector, persistent-root and foreign-pin storage to the native account.

use super::*;

impl Vm {
    pub(super) fn bind_native_budget(&mut self) -> Result<(), RuntimeError> {
        if let Some(scope) = &self.host_scheduling {
            let budget = scope.heap_native_budget();
            self.heap.bind_native_budget(budget)?;
        }
        self.attach_retained_debugger_storage()
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/collector_workspace.rs"]
mod ownership_tests;
