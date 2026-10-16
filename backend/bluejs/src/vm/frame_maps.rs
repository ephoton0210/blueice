// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admit frame tables before entry effects or consuming saved state.

use super::*;

pub(super) struct PreparedFrameMaps {
    cells: HashMap<usize, ObjectId>,
    dynamic: HashMap<String, DynamicEvalBinding>,
    charge: execution_payload::ExecutionPayloadCharge,
}

impl PreparedFrameMaps {
    fn refresh(&mut self) {
        let bytes = host_scheduling::map_slots(&self.cells)
            .saturating_add(host_scheduling::named_cells_bytes(&self.dynamic));
        self.charge
            .resize(if self.charge.enabled() { bytes } else { 0 });
    }

    pub(super) fn adopt_captures(&mut self, captures: Vec<ObjectId>) {
        debug_assert!(self.cells.capacity() >= captures.len());
        self.cells.extend(captures.into_iter().enumerate());
        self.refresh();
    }

    #[cfg(any(test, coverage))]
    pub(super) fn adopt_cells(&mut self, cells: Vec<(usize, ObjectId)>) {
        debug_assert!(self.cells.capacity() >= cells.len());
        self.cells.extend(cells);
        self.refresh();
    }

    #[cfg(any(test, coverage))]
    pub(super) fn adopt_dynamic(&mut self, bindings: Vec<(String, ObjectId, Vec<ObjectId>)>) {
        debug_assert!(self.dynamic.capacity() >= bindings.len());
        self.dynamic
            .extend(bindings.into_iter().map(|(name, cell, shadowed_cells)| {
                (
                    name,
                    DynamicEvalBinding {
                        cell,
                        shadowed_cells,
                    },
                )
            }));
        self.refresh();
    }

    pub(super) fn adopt_saved_maps(
        &mut self,
        cells: HashMap<usize, ObjectId>,
        dynamic: HashMap<String, DynamicEvalBinding>,
    ) {
        self.cells = cells;
        self.dynamic = dynamic;
        self.refresh();
    }

    // Detached parts stay leased until all tables enter active VM ownership.
    pub(super) fn take_cells(&mut self) -> HashMap<usize, ObjectId> {
        std::mem::take(&mut self.cells)
    }

    pub(super) fn take_dynamic(&mut self) -> HashMap<String, DynamicEvalBinding> {
        std::mem::take(&mut self.dynamic)
    }
}

impl Vm {
    pub(super) fn prepare_frame_maps(
        &mut self,
        cells: usize,
        dynamic: usize,
        adopted_payload: usize,
    ) -> Result<PreparedFrameMaps, RuntimeError> {
        let minimum = cells
            .saturating_mul(std::mem::size_of::<(usize, ObjectId)>())
            .saturating_add(
                dynamic.saturating_mul(std::mem::size_of::<(String, DynamicEvalBinding)>()),
            )
            .saturating_add(adopted_payload);
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: minimum,
            ..Default::default()
        })?;
        let mut cell_table = HashMap::new();
        let mut dynamic_table = HashMap::new();
        if cell_table.try_reserve(cells).is_err() || dynamic_table.try_reserve(dynamic).is_err() {
            return Err(self
                .host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail));
        }
        let actual = host_scheduling::map_slots(&cell_table)
            .saturating_add(host_scheduling::map_slots(&dynamic_table));
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: actual.saturating_add(adopted_payload),
            ..Default::default()
        })?;
        let prepared = PreparedFrameMaps {
            cells: cell_table,
            dynamic: dynamic_table,
            charge: self.charge_execution_control(actual),
        };
        // Only allocated tables belong to this owner so far. Incoming names
        // and shadowed-cell buffers move from managed ownership immediately
        // after the joint generator preparation succeeds.
        self.synchronize_scheduling_usage()?;
        Ok(prepared)
    }

    pub(super) fn prepare_generator_frame_maps(
        &mut self,
        generator: ObjectId,
    ) -> Result<PreparedFrameMaps, RuntimeError> {
        let (cells, dynamic, payload) = self.heap.generator_map_layout(generator)?;
        self.prepare_frame_maps(cells, dynamic, payload)
    }
}
