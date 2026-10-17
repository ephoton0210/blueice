// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Namespace identity caches own admitted backing and transactional roots.

use super::host_scheduling::SharedSchedulingCharge;
use super::promise_queue_storage::PreparedMap;
use super::retained_control_map::RetainedControlMap;
use super::*;

pub(super) type NamespaceMap = RetainedControlMap<String, ObjectId>;
pub(super) type NamespaceRoots = RetainedControlMap<String, RootId>;
pub(super) type DeferredNames = RetainedControlMap<ObjectId, String>;

pub(super) struct PreparedNamespaceEntry {
    name: String,
    root_name: String,
    deferred_name: Option<String>,
    table: Option<PreparedMap<String, ObjectId>>,
    roots: Option<PreparedMap<String, RootId>>,
    reverse: Option<PreparedMap<ObjectId, String>>,
    charge: Option<SharedSchedulingCharge>,
    deferred: bool,
    epoch: u64,
}
impl PreparedNamespaceEntry {
    pub(super) fn prepare(vm: &Vm, module: &str, deferred: bool) -> Result<Self, RuntimeError> {
        let scope = vm.host_scheduling.as_ref();
        let fail = || scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail);
        vm.next_namespace_epoch.checked_add(1).ok_or_else(fail)?;
        let copies = 2 + usize::from(deferred);
        let minimum = module.len().checked_mul(copies).ok_or_else(fail)?;
        let mut prepared = Self {
            name: String::new(),
            root_name: String::new(),
            deferred_name: None,
            table: None,
            roots: None,
            reverse: None,
            charge: scope
                .map(|scope| scope.shared_charge(minimum))
                .transpose()?,
            deferred,
            epoch: vm.next_namespace_epoch,
        };
        prepared.name = module_referrers::copy_referrer(module, scope)?;
        prepared.refresh_names(module.len(), copies - 1)?;
        prepared.root_name = module_referrers::copy_referrer(module, scope)?;
        prepared.refresh_names(module.len(), copies - 2)?;
        if deferred {
            prepared.deferred_name = Some(module_referrers::copy_referrer(module, scope)?);
            prepared.refresh_names(module.len(), 0)?;
        }
        let (table, roots) = if deferred {
            (
                &vm.module_deferred_namespace_cache,
                &vm.module_deferred_namespace_roots,
            )
        } else {
            (&vm.module_namespace_cache, &vm.module_namespace_roots)
        };
        prepared.table = Some(PreparedMap::for_insert(table, scope)?);
        prepared.roots = Some(PreparedMap::for_insert(roots, scope)?);
        if deferred {
            prepared.reverse = Some(PreparedMap::for_insert(&vm.deferred_namespaces, scope)?);
        }
        Ok(prepared)
    }
    fn refresh_names(&mut self, size: usize, pending: usize) -> Result<(), RuntimeError> {
        if let Some(charge) = &mut self.charge {
            charge.resize(
                self.name
                    .capacity()
                    .saturating_add(self.root_name.capacity())
                    .saturating_add(self.deferred_name.as_ref().map_or(0, String::capacity))
                    .saturating_add(size.saturating_mul(pending)),
            )?;
        }
        Ok(())
    }
    pub(super) fn publish(
        mut self,
        vm: &mut Vm,
        module: &str,
        linked: &mut HashMap<String, LinkedModule>,
        namespace: ObjectId,
        root: RootId,
    ) {
        assert_eq!(vm.next_namespace_epoch, self.epoch);
        let (table, roots) = if self.deferred {
            (
                &mut vm.module_deferred_namespace_cache,
                &mut vm.module_deferred_namespace_roots,
            )
        } else {
            (
                &mut vm.module_namespace_cache,
                &mut vm.module_namespace_roots,
            )
        };
        self.table.take().unwrap().commit(table);
        self.roots.take().unwrap().commit(roots);
        table.insert(std::mem::take(&mut self.name), namespace);
        roots.insert(std::mem::take(&mut self.root_name), root);
        if let Some(reverse) = self.reverse.take() {
            reverse.commit(&mut vm.deferred_namespaces);
            vm.deferred_namespaces
                .insert(namespace, self.deferred_name.take().unwrap());
        }
        let record = linked
            .get_mut(module)
            .expect("namespace retains its linked record");
        if self.deferred {
            record.deferred_namespace = Some(namespace);
        } else {
            record.namespace = Some(namespace);
        }
        record.namespace_epochs[usize::from(self.deferred)] = self.epoch;
        vm.next_namespace_epoch += 1;
        // The synchronous ownership handoff performs no allocation/callback.
        drop(self);
        vm.reconcile_scheduling_ownership();
    }
}

impl Vm {
    pub(super) fn namespace_cache_bytes(&self) -> usize {
        let mut bytes = self
            .module_namespace_cache
            .retained_bytes()
            .saturating_add(self.module_namespace_roots.retained_bytes())
            .saturating_add(self.module_deferred_namespace_cache.retained_bytes())
            .saturating_add(self.module_deferred_namespace_roots.retained_bytes())
            .saturating_add(self.deferred_namespaces.retained_bytes());
        for name in self
            .module_namespace_cache
            .keys()
            .chain(self.module_namespace_roots.keys())
            .chain(self.module_deferred_namespace_cache.keys())
            .chain(self.module_deferred_namespace_roots.keys())
            .chain(self.deferred_namespaces.values())
        {
            bytes = bytes.saturating_add(name.capacity());
        }
        bytes
    }
    /// A failed operation must not allocate a cleanup snapshot or replacement.
    pub(super) fn rollback_namespace_cache_from(
        &mut self,
        epoch: u64,
        linked: &mut HashMap<String, LinkedModule>,
    ) {
        assert!(epoch != 0, "zero is an unpublished namespace mark");
        for (name, record) in linked.iter_mut() {
            for deferred in [false, true] {
                let index = usize::from(deferred);
                if record.namespace_epochs[index] < epoch {
                    continue;
                }
                let (table, roots) = if deferred {
                    (
                        &mut self.module_deferred_namespace_cache,
                        &mut self.module_deferred_namespace_roots,
                    )
                } else {
                    (
                        &mut self.module_namespace_cache,
                        &mut self.module_namespace_roots,
                    )
                };
                let namespace = table
                    .remove(name)
                    .expect("publication mark owns its cache entry");
                let root = roots.remove(name).expect("namespace cache owns its root");
                if deferred {
                    self.deferred_namespaces.remove(&namespace);
                    record.deferred_namespace = None;
                } else {
                    record.namespace = None;
                }
                record.namespace_epochs[index] = 0;
                self.heap
                    .unroot(root)
                    .expect("namespace transaction owns its cache root");
            }
        }
        if self.module_namespace_cache.is_empty() {
            self.module_namespace_cache.release_empty();
        }
        if self.module_namespace_roots.is_empty() {
            self.module_namespace_roots.release_empty();
        }
        if self.module_deferred_namespace_cache.is_empty() {
            self.module_deferred_namespace_cache.release_empty();
        }
        if self.module_deferred_namespace_roots.is_empty() {
            self.module_deferred_namespace_roots.release_empty();
        }
        if self.deferred_namespaces.is_empty() {
            self.deferred_namespaces.release_empty();
        }
        self.reconcile_scheduling_ownership();
    }
    pub(super) fn reclaim_namespace_cache_storage(&mut self) {
        self.reconcile_scheduling_ownership();
        if self.preserve_promise_table_capacity {
            return;
        }
        let mut scope = self.host_scheduling.as_mut();
        // Reclaim sequentially, refunding the adopted reduction before the
        // candidate lease drops or the following layer asks for headroom.
        macro_rules! reclaim {
            ($field:ident) => {{
                let before = self.$field.retained_bytes();
                let candidate = self.$field.reclaim(scope.as_deref());
                if let Some(scope) = scope.as_deref_mut() {
                    scope.release_owned_payload(before - self.$field.retained_bytes());
                }
                drop(candidate);
            }};
        }
        reclaim!(module_namespace_cache);
        reclaim!(module_namespace_roots);
        reclaim!(module_deferred_namespace_cache);
        reclaim!(module_deferred_namespace_roots);
        reclaim!(deferred_namespaces);
    }
}
