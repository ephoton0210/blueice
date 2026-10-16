// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bind heap collector storage to the document native account.

use super::*;

impl Vm {
    pub(super) fn bind_collector_budget(&mut self) -> Result<(), RuntimeError> {
        if let Some(scope) = &self.host_scheduling {
            self.heap.bind_collector_budget(scope.collector_budget())?;
        }
        Ok(())
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/collector_workspace.rs"]
mod ownership_tests;
