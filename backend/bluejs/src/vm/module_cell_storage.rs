// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Linked module cell tables keep their own capacity through frame transfers.

use super::*;

pub(super) struct PreparedModuleCells {
    cells: HashMap<usize, ObjectId>,
    charge: execution_payload::ExecutionPayloadCharge,
}
impl PreparedModuleCells {
    pub(super) fn install(self, record: &mut LinkedModule) {
        record.cells = self.cells;
        record.cells_charge = self.charge;
    }
}
impl LinkedModule {
    pub(super) fn refresh_cells_charge(&mut self) {
        self.cells_charge.resize(if self.cells_charge.enabled() {
            host_scheduling::map_slots(&self.cells)
        } else {
            0
        });
    }
}

impl Vm {
    pub(super) fn prepare_linked_module_cells(
        &mut self,
        count: usize,
    ) -> Result<PreparedModuleCells, RuntimeError> {
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: count.saturating_mul(std::mem::size_of::<(usize, ObjectId)>()),
            ..Default::default()
        })?;
        let mut cells = HashMap::new();
        cells.try_reserve(count).map_err(|_| {
            self.host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
        })?;
        let bytes = host_scheduling::map_slots(&cells);
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: bytes,
            ..Default::default()
        })?;
        let prepared = PreparedModuleCells {
            cells,
            charge: self.charge_module_cells(bytes),
        };
        if let Err(error) = self.synchronize_scheduling_usage() {
            drop(prepared);
            self.reconcile_scheduling_ownership();
            return Err(error);
        }
        Ok(prepared)
    }

    /// Stage the graph's independent copy while active cells still root all
    /// entries. Refusal becomes an ordinary engine exit before frame capture.
    pub(super) fn stage_module_exit_cells(
        &mut self,
        outcome: Result<InterpreterExit, RuntimeError>,
    ) -> (
        Result<InterpreterExit, RuntimeError>,
        Option<PreparedModuleCells>,
    ) {
        if !matches!(
            &outcome,
            Ok(InterpreterExit::Await { .. } | InterpreterExit::Suspend { .. })
        ) {
            return (outcome, None);
        }
        match self.prepare_linked_module_cells(self.cells.len()) {
            Ok(mut prepared) => {
                prepared
                    .cells
                    .extend(self.cells.iter().map(|(slot, cell)| (*slot, *cell)));
                (outcome, Some(prepared))
            }
            Err(error) => (Err(error), None),
        }
    }

    pub(super) fn attach_linked_module_cells(&mut self) {
        let mut graph = self.module_graph.take();
        let mut parked = self.evaluating_linked.take();
        for record in graph
            .iter_mut()
            .flat_map(|graph| graph.linked.values_mut())
            .chain(parked.iter_mut().flat_map(|linked| linked.values_mut()))
        {
            if !record.cells_charge.enabled() {
                record.cells_charge =
                    self.charge_module_cells(host_scheduling::map_slots(&record.cells));
            }
        }
        self.module_graph = graph;
        self.evaluating_linked = parked;
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/module_cell_ownership.rs"]
mod ownership_tests;
