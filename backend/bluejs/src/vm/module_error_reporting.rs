// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pending module errors retain reporting ownership across native dispatch.

use super::*;

use super::host_scheduling::SharedSchedulingCharge;

struct PendingModuleReport {
    name: String,
    error: ModuleEvaluationError,
}
struct PendingModuleReports {
    entries: Vec<PendingModuleReport>,
    roots: ModuleRoots,
    // Drop snapshots before refunding their native storage.
    _charge: Option<SharedSchedulingCharge>,
}
impl PendingModuleReports {
    fn retained_bytes(&self) -> usize {
        self.entries
            .iter()
            .fold(host_scheduling::slots(&self.entries), |bytes, entry| {
                bytes.saturating_add(entry.name.capacity())
            })
    }
}

impl Vm {
    fn clear_ready_module_error_flags(&mut self) {
        if let Some(graph) = &mut self.module_graph {
            for record in graph.linked.values_mut() {
                if record.host_error_pending && record.evaluated {
                    record.host_error_pending = false;
                }
            }
        }
    }
    fn prepare_pending_module_reports(
        &mut self,
    ) -> Result<Option<PendingModuleReports>, RuntimeError> {
        if self.host_error_reporting.is_none() {
            return Ok(None);
        }
        let (count, names) = self
            .module_graph
            .iter()
            .flat_map(|graph| graph.linked.iter())
            .filter(|(_, record)| {
                record.host_error_pending && record.evaluated && record.error.is_some()
            })
            .fold((0usize, 0usize), |(count, bytes), (name, _)| {
                (count.saturating_add(1), bytes.saturating_add(name.len()))
            });
        if count == 0 {
            self.clear_ready_module_error_flags();
            return Ok(None);
        }
        self.synchronize_scheduling_usage()?;
        let scope = self.host_scheduling.as_ref();
        let fail = || scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail);
        let minimum = count
            .saturating_mul(std::mem::size_of::<PendingModuleReport>())
            .saturating_add(names);
        let mut charge = scope
            .map(|scope| scope.shared_charge(minimum))
            .transpose()?;
        let mut entries = Vec::<PendingModuleReport>::new();
        entries.try_reserve_exact(count).map_err(|_| fail())?;
        let slots = host_scheduling::slots(&entries);
        if let Some(charge) = &mut charge {
            charge.resize(slots.saturating_add(names))?;
        }
        let mut remaining_names = names;
        let mut copied_names = 0usize;
        for (name, record) in self
            .module_graph
            .as_ref()
            .expect("counted live module graph")
            .linked
            .iter()
        {
            if !record.host_error_pending || !record.evaluated {
                continue;
            }
            let Some(error) = &record.error else {
                continue;
            };
            let mut copied = String::new();
            copied.try_reserve_exact(name.len()).map_err(|_| fail())?;
            remaining_names -= name.len();
            copied_names = copied_names.saturating_add(copied.capacity());
            if let Some(charge) = &mut charge {
                charge.resize(
                    slots
                        .saturating_add(copied_names)
                        .saturating_add(remaining_names),
                )?;
            }
            copied.push_str(name);
            assert!(entries.len() < entries.capacity());
            entries.push(PendingModuleReport {
                name: copied,
                error: error.clone(),
            });
        }
        let mut batch = PendingModuleReports {
            entries,
            roots: ModuleRoots::default(),
            _charge: charge,
        };
        let actual = batch.retained_bytes();
        if let Some(charge) = &mut batch._charge {
            charge.resize(actual)?;
        }
        Self::append_registered_frame_roots(
            &mut batch.roots,
            &mut self.heap,
            scope,
            batch
                .entries
                .iter()
                .filter_map(|entry| entry.error.value.object_id()),
        )?;
        Ok(Some(batch))
    }
    fn restore_unattempted_module_reports(&mut self, entries: &[PendingModuleReport]) {
        let Some(graph) = &mut self.module_graph else {
            return;
        };
        for entry in entries {
            if let Some(record) = graph.linked.get_mut(entry.name.as_str()) {
                if record.evaluated
                    && record
                        .error
                        .as_ref()
                        .is_some_and(|error| Rc::ptr_eq(error, &entry.error))
                {
                    record.host_error_pending = true;
                }
            }
        }
    }
    pub(super) fn dispatch_pending_host_module_errors(
        &mut self,
        mut report: impl FnMut(&mut Self, &ModuleEvaluationError) -> Result<(), RuntimeError>,
    ) -> Result<(), RuntimeError> {
        let prepared = self.prepare_pending_module_reports();
        let mut batch = match prepared {
            Ok(Some(batch)) => batch,
            Ok(None) => return Ok(()),
            Err(error) => {
                self.reconcile_scheduling_ownership();
                return Err(error);
            }
        };
        self.clear_ready_module_error_flags();
        let mut result = Ok(());
        for index in 0..batch.entries.len() {
            if let Err(error) = report(self, &batch.entries[index].error) {
                // The failed report may already have entered user code. Only
                // its unattempted suffix is eligible for later delivery.
                self.restore_unattempted_module_reports(&batch.entries[index + 1..]);
                result = Err(error);
                break;
            }
        }
        self.release_module_roots_from(&mut batch.roots, 0);
        drop(batch);
        self.prune_module_error_origins();
        self.reconcile_scheduling_ownership();
        result
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/module_error_reporting.rs"]
mod ownership_tests;
