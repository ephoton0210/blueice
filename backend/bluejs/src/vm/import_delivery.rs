// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Import delivery prepares native snapshots and roots before consuming waiters.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

struct ImportDeliveryEntry {
    promise: ObjectId,
    deferred: bool,
    namespace: Option<ObjectId>,
    rejection: Option<(Value, ThrowOrigin)>,
    value_charge: Option<SharedSchedulingCharge>,
    origin_charge: Option<SharedSchedulingCharge>,
}
impl ImportDeliveryEntry {
    fn payload_bytes(&self) -> usize {
        self.rejection.as_ref().map_or(0, |(value, origin)| {
            host_scheduling::value_payload_bytes(value).saturating_add(origin.payload_bytes())
        })
    }
}
struct PreparedImportDelivery {
    entries: Vec<ImportDeliveryEntry>,
    references: Vec<ObjectId>,
    roots: Vec<RootId>,
    charge: Option<SharedSchedulingCharge>,
}
impl PreparedImportDelivery {
    fn slots(&self) -> usize {
        host_scheduling::slots(&self.entries)
            .saturating_add(host_scheduling::slots(&self.references))
            .saturating_add(host_scheduling::slots(&self.roots))
    }
}
fn ready_dependency(waiter: &DeferredImportWaiter, module: &str, rejected: bool) -> bool {
    if rejected {
        waiter.pending.contains(module)
    } else {
        waiter.pending.is_empty() || waiter.pending.len() == 1 && waiter.pending.contains(module)
    }
}
impl Vm {
    fn prepare_import_delivery(
        &mut self,
        module: &str,
        namespace: Option<ObjectId>,
        error: Option<&ModuleEvaluationError>,
    ) -> Result<PreparedImportDelivery, RuntimeError> {
        self.synchronize_scheduling_usage()?;
        let count = self
            .deferred_import_waiters
            .iter()
            .filter(|waiter| ready_dependency(waiter, module, error.is_some()))
            .count()
            .saturating_add(self.module_import_waiters.get(module).map_or(0, Vec::len));
        let mut batch = PreparedImportDelivery {
            entries: Vec::new(),
            references: Vec::new(),
            roots: Vec::new(),
            charge: None,
        };
        if count == 0 {
            return Ok(batch);
        }
        let ref_count = count.saturating_mul(error.map_or(2, |error| {
            1 + usize::from(error.value.object_id().is_some())
        }));
        let payload = error
            .map_or(0, |error| {
                host_scheduling::value_payload_bytes(&error.value)
                    .saturating_add(error.origin.payload_bytes())
            })
            .saturating_mul(count);
        let reference_slots = ref_count.saturating_mul(std::mem::size_of::<ObjectId>());
        let root_slots = ref_count.saturating_mul(std::mem::size_of::<RootId>());
        let scope = self.host_scheduling.as_ref();
        let fail = || scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail);
        batch.charge = scope
            .map(|scope| {
                scope.shared_charge(
                    count
                        .saturating_mul(std::mem::size_of::<ImportDeliveryEntry>())
                        .saturating_add(reference_slots)
                        .saturating_add(root_slots)
                        .saturating_add(payload),
                )
            })
            .transpose()?;
        batch.entries.try_reserve_exact(count).map_err(|_| fail())?;
        if let Some(charge) = &mut batch.charge {
            charge.resize(
                host_scheduling::slots(&batch.entries)
                    .saturating_add(reference_slots)
                    .saturating_add(root_slots)
                    .saturating_add(payload),
            )?;
        }
        batch
            .references
            .try_reserve_exact(ref_count)
            .map_err(|_| fail())?;
        if let Some(charge) = &mut batch.charge {
            charge.resize(
                host_scheduling::slots(&batch.entries)
                    .saturating_add(host_scheduling::slots(&batch.references))
                    .saturating_add(root_slots)
                    .saturating_add(payload),
            )?;
        }
        batch
            .roots
            .try_reserve_exact(ref_count)
            .map_err(|_| fail())?;
        let slots = batch.slots();
        if let Some(charge) = &mut batch.charge {
            charge.resize(slots.saturating_add(payload))?;
        }
        let entry = |promise, deferred, namespace| ImportDeliveryEntry {
            promise,
            deferred,
            namespace: error.is_none().then_some(namespace),
            rejection: error.map(|error| (error.value.clone(), error.origin.clone())),
            value_charge: None,
            origin_charge: None,
        };
        for waiter in &self.deferred_import_waiters {
            if ready_dependency(waiter, module, error.is_some()) {
                batch
                    .entries
                    .push(entry(waiter.promise, true, waiter.namespace));
            }
        }
        if let Some(waiters) = self.module_import_waiters.get(module) {
            for promise in waiters {
                let namespace = if error.is_some() {
                    *promise
                } else {
                    namespace.expect("eager fulfillment has its prepared namespace")
                };
                batch.entries.push(entry(*promise, false, namespace));
            }
        }
        assert_eq!(batch.entries.len(), count);
        let actual = batch.entries.iter().fold(slots, |bytes, entry| {
            bytes.saturating_add(entry.payload_bytes())
        });
        if let Some(charge) = &mut batch.charge {
            charge.resize(actual)?;
            for entry in &mut batch.entries {
                if let Some((value, origin)) = &entry.rejection {
                    entry.value_charge =
                        Some(charge.split_off(host_scheduling::value_payload_bytes(value)));
                    entry.origin_charge = Some(charge.split_off(origin.payload_bytes()));
                }
            }
        }
        for entry in &batch.entries {
            batch.references.push(entry.promise);
            if let Some(namespace) = entry.namespace {
                batch.references.push(namespace);
            }
            if let Some(reference) = entry
                .rejection
                .as_ref()
                .and_then(|(value, _)| value.object_id())
            {
                batch.references.push(reference);
            }
        }
        assert_eq!(batch.references.len(), ref_count);
        let roots = self.heap.prepare_root_batch(&batch.references)?;
        self.heap.commit_root_batch(roots, &mut batch.roots);
        Ok(batch)
    }

    fn consume_import_delivery(
        &mut self,
        module: &str,
        entry: &ImportDeliveryEntry,
        rejected: bool,
    ) -> bool {
        if entry.deferred {
            let Some(index) = self.deferred_import_waiters.iter().position(|waiter| {
                waiter.promise == entry.promise && ready_dependency(waiter, module, rejected)
            }) else {
                return false;
            };
            self.deferred_import_waiters.remove(index);
        } else {
            let Some(waiters) = self.module_import_waiters.get_mut(module) else {
                return false;
            };
            let Some(index) = waiters.iter().position(|promise| *promise == entry.promise) else {
                return false;
            };
            waiters.remove(index);
            if waiters.is_empty() {
                self.module_import_waiters.remove(module);
            }
        }
        self.reclaim_import_waiter_storage();
        true
    }

    fn dispatch_import_delivery(
        &mut self,
        module: &str,
        mut batch: PreparedImportDelivery,
        rejected: bool,
    ) -> Result<(), RuntimeError> {
        // Readiness preparation commits dependency progress, while ready
        // entries retain their marker until their individual attempt begins.
        if !rejected {
            for waiter in &mut self.deferred_import_waiters {
                if waiter.pending.len() > 1 {
                    waiter.pending.remove(module);
                }
            }
        }
        self.reclaim_import_waiter_storage();
        let result = (|| {
            for entry in &mut batch.entries {
                if !self.consume_import_delivery(module, entry, rejected) {
                    continue;
                }
                if let Some((value, origin)) = entry.rejection.take() {
                    let input = entry.value_charge.take();
                    self.with_leased_rejection_origin(
                        Some(origin),
                        entry.origin_charge.take(),
                        |vm| {
                            vm.settle_promise_with_input_charge(
                                entry.promise,
                                PromiseStatus::Rejected(value),
                                input,
                            )
                        },
                    )?;
                } else {
                    self.resolve_promise(
                        entry.promise,
                        Value::Object(entry.namespace.expect("prepared namespace")),
                    )?;
                }
                self.check_scheduling_failure()?;
            }
            Ok(())
        })();
        for root in batch.roots.drain(..) {
            self.release_root(root);
        }
        drop(batch);
        self.reconcile_scheduling_ownership();
        result
    }

    pub(super) fn settle_dynamic_import_waiters(
        &mut self,
        module: &str,
        linked: &mut HashMap<String, LinkedModule>,
    ) -> Result<(), RuntimeError> {
        let mut namespace_roots = ModuleRoots::default();
        let result = (|| {
            let namespace = if self
                .module_import_waiters
                .get(module)
                .is_some_and(|waiters| !waiters.is_empty())
            {
                let modules = self.module_registry.clone();
                Some(self.module_namespace(
                    module,
                    false,
                    &modules,
                    linked,
                    &mut namespace_roots,
                )?)
            } else {
                None
            };
            let batch = self.prepare_import_delivery(module, namespace, None)?;
            self.dispatch_import_delivery(module, batch, false)
        })();
        self.release_module_roots_from(&mut namespace_roots, 0);
        self.reconcile_scheduling_ownership();
        result
    }
    pub(super) fn reject_dynamic_import_waiters(
        &mut self,
        module: &str,
        error: ModuleEvaluationError,
    ) -> Result<(), RuntimeError> {
        let batch = self.prepare_import_delivery(module, None, Some(&error))?;
        self.dispatch_import_delivery(module, batch, true)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/import_delivery.rs"]
mod ownership_tests;
