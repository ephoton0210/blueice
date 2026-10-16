// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admit scope records and alias cells before changing lexical state.

use super::*;

pub(super) struct PreparedScopeSlots {
    slots: Vec<Vec<u32>>,
    _charge: execution_payload::ExecutionPayloadCharge,
}

impl PreparedScopeSlots {
    pub(super) fn take(&mut self) -> Vec<Vec<u32>> {
        std::mem::take(&mut self.slots)
    }
}

impl Vm {
    fn scope_alias_cell(&self, slot: usize) -> Option<ObjectId> {
        if let Some(name) = self.eval_dynamic_slots.get(&slot) {
            Some(
                self.dynamic_eval_bindings
                    .get(name)
                    .expect("prepared eval alias retains its binding")
                    .cell,
            )
        } else {
            self.script_global_slots.get(&slot).map(|name| {
                self.global_bindings
                    .get(name)
                    .expect("prepared script alias retains its binding")
                    .cell
            })
        }
    }

    pub(super) fn enter_scope(&mut self, code: &Bytecode, scope: u32) -> Result<(), RuntimeError> {
        let result = self.enter_scope_admitted(code, scope);
        self.reconcile_scheduling_ownership();
        result
    }

    fn enter_scope_admitted(&mut self, code: &Bytecode, scope: u32) -> Result<(), RuntimeError> {
        let slots = &code.scopes[scope as usize];
        let mut removed = 0usize;
        let mut added = 0usize;
        for &slot in slots {
            let slot = slot as usize;
            if self.scope_alias_cell(slot).is_some() {
                added += usize::from(!self.cells.contains_key(&slot));
            } else if !(code.module && scope == 0) {
                removed += usize::from(self.cells.contains_key(&slot));
            }
        }
        let additional = added.saturating_sub(removed);
        let scope_growth = SchedulingCapacity::for_vec_push(&self.active_scopes);
        let slot_growth = SchedulingCapacity::for_vec_push(&self.active_scope_slots);
        let records = HostSchedulingUsage {
            payload_bytes: slots.len().saturating_mul(std::mem::size_of::<u32>()),
            ..Default::default()
        }
        .with_growth(scope_growth)
        .with_growth(slot_growth);
        // Refuse even the minimum table growth before allocating a replacement.
        // Its reported capacity is admitted jointly with both record vectors.
        let minimum_map_growth = self
            .cells
            .len()
            .saturating_add(additional)
            .saturating_sub(self.cells.capacity())
            .saturating_mul(std::mem::size_of::<(usize, ObjectId)>());
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: records.payload_bytes.saturating_add(minimum_map_growth),
            ..Default::default()
        })?;
        let cells = SchedulingMapCapacity::for_additional(
            &self.cells,
            additional,
            self.host_scheduling.as_ref(),
        )?;
        self.admit_scheduling_work(records.with_map_growth(&cells))?;
        scope_growth.reserve_vec(&mut self.active_scopes);
        slot_growth.reserve_vec(&mut self.active_scope_slots);
        let slots = slots.clone();
        let slot_charge = self.charge_execution_control(host_scheduling::slots(&slots));
        self.admit_scheduling_work(HostSchedulingUsage::default().with_map_growth(&cells))?;
        cells.commit(&mut self.cells);
        // Remove obsolete cells first, so new aliases can use their slots.
        for &slot in &slots {
            let slot = slot as usize;
            if self.scope_alias_cell(slot).is_none()
                && !(code.module && scope == 0 && self.cells.contains_key(&slot))
            {
                self.cells.remove(&slot);
                self.bindings[slot] = if code.bindings[slot].lexical {
                    None
                } else {
                    Some(Value::Undefined)
                };
            }
        }
        for &slot in &slots {
            if let Some(cell) = self.scope_alias_cell(slot as usize) {
                self.cells.insert(slot as usize, cell);
            }
        }
        self.active_scopes.push(scope);
        self.active_scope_slots.push(slots);
        drop(slot_charge);
        self.synchronize_scheduling_usage()
    }

    pub(super) fn prepare_generator_scope_slots(
        &mut self,
        generator: ObjectId,
    ) -> Result<PreparedScopeSlots, RuntimeError> {
        let bytes = match self.heap.generator_scope_layout(generator)? {
            Some((code, scopes)) => scopes.iter().fold(
                scopes.len().saturating_mul(std::mem::size_of::<Vec<u32>>()),
                |bytes, scope| {
                    bytes.saturating_add(
                        code.scopes[*scope as usize]
                            .len()
                            .saturating_mul(std::mem::size_of::<u32>()),
                    )
                },
            ),
            None => 0,
        };
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: bytes,
            ..Default::default()
        })?;
        let slots = match self.heap.generator_scope_layout(generator)? {
            Some((code, scopes)) => {
                let mut slots = Vec::with_capacity(scopes.len());
                slots.extend(
                    scopes
                        .iter()
                        .map(|scope| code.scopes[*scope as usize].clone()),
                );
                slots
            }
            None => Vec::new(),
        };
        let actual = slots
            .iter()
            .fold(host_scheduling::slots(&slots), |bytes, slots| {
                bytes.saturating_add(host_scheduling::slots(slots))
            });
        let prepared = PreparedScopeSlots {
            slots,
            _charge: self.charge_execution_control(actual),
        };
        if let Err(error) = self.synchronize_scheduling_usage() {
            drop(prepared);
            self.reconcile_scheduling_ownership();
            return Err(error);
        }
        Ok(prepared)
    }
}
