// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pending module errors retain reporting ownership across native dispatch.

use super::*;

use super::host_scheduling::SharedSchedulingCharge;

struct PendingModuleReport {
    name: String,
    error: ModuleEvaluationError,
    context: Rc<realm_call_storage::RealmCallRoots>,
}
struct PendingModuleReports {
    entries: Vec<PendingModuleReport>,
    roots: ModuleRoots,
    // Drop snapshots before refunding their native storage.
    _charge: Option<SharedSchedulingCharge>,
}
impl PendingModuleReports {
    fn entry_bytes(&self) -> usize {
        self.entries
            .iter()
            .fold(host_scheduling::slots(&self.entries), |bytes, entry| {
                bytes.saturating_add(entry.name.capacity())
            })
    }

    #[cfg(any(test, coverage))]
    fn retained_bytes(&self) -> usize {
        self.entries
            .iter()
            .fold(self.entry_bytes(), |bytes, entry| {
                bytes.saturating_add(entry.context.retained_bytes())
            })
    }

    fn validate_entry(&self, index: usize) -> Result<(), RuntimeError> {
        if let Some(owner) = &self._charge {
            owner.reserve_peer(0)?;
        }
        self.entries[index].error.owner_view()?;
        Ok(())
    }
}

impl Vm {
    fn pending_module_report_owner(&self) -> Result<Option<SharedSchedulingCharge>, RuntimeError> {
        let graph = self
            .module_graph
            .as_ref()
            .expect("counted live module graph");
        let mut owner = graph.linked.owner_view()?;
        if let (Some(scope), Some(original)) = (&self.host_scheduling, &owner) {
            if !original.belongs_to(scope) {
                return Err(scope.fail());
            }
        }
        for error in graph
            .linked
            .values()
            .filter(|record| record.host_error_pending && record.evaluated)
            .filter_map(|record| record.error.as_ref())
        {
            let original = error.owner_view()?;
            if let (Some(scope), Some(original)) = (&self.host_scheduling, &original) {
                if !original.belongs_to(scope) {
                    return Err(scope.fail());
                }
            }
            if owner.is_none() {
                owner = original;
            }
        }
        let caller = self.argument_list_owner()?;
        Ok(owner.or(caller))
    }

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
        let owner = self.pending_module_report_owner()?;
        let owned_scope = owner.as_ref().map(SharedSchedulingCharge::scheduling_scope);
        self.synchronize_scheduling_usage()?;
        let scope = owned_scope.as_ref();
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
            if error.owner_view()?.is_none() {
                if let Some(scope) = scope {
                    error.attach(scope)?;
                }
            }
            let context_owner = error.owner_view()?;
            let context = realm_call_storage::RealmCallRoots::new(0, context_owner.as_ref())?;
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
                context,
            });
        }
        let mut batch = PendingModuleReports {
            entries,
            roots: ModuleRoots::default(),
            _charge: charge,
        };
        let actual = batch.entry_bytes();
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
            if let Err(error) = batch.validate_entry(index) {
                self.restore_unattempted_module_reports(&batch.entries[index..]);
                result = Err(error);
                break;
            }
            let entry = &batch.entries[index];
            let reported =
                self.with_realm_call_roots(entry.context.clone(), |vm| report(vm, &entry.error));
            if let Err(error) = batch.validate_entry(index).and(reported) {
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

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/module_report_lifetime.rs"]
mod lifetime_tests;
