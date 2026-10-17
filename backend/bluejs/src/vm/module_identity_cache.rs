// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Source/import-meta identities own admitted native backing and key buffers.

use super::host_scheduling::SharedSchedulingCharge;
use super::promise_queue_storage::PreparedMap;
use super::retained_control_map::RetainedControlMap;
use super::*;

pub(super) type IdentityMap = RetainedControlMap<String, ObjectId>;
pub(super) type IdentityRoots = RetainedControlMap<String, RootId>;

#[derive(Clone, Copy)]
pub(super) enum IdentityKind {
    Source,
    Meta,
}
fn maps(vm: &Vm, kind: IdentityKind) -> (&IdentityMap, &IdentityRoots) {
    match kind {
        IdentityKind::Source => (&vm.module_source_cache, &vm.module_source_roots),
        IdentityKind::Meta => (&vm.module_import_meta, &vm.module_import_meta_roots),
    }
}
fn maps_mut(vm: &mut Vm, kind: IdentityKind) -> (&mut IdentityMap, &mut IdentityRoots) {
    match kind {
        IdentityKind::Source => (&mut vm.module_source_cache, &mut vm.module_source_roots),
        IdentityKind::Meta => (&mut vm.module_import_meta, &mut vm.module_import_meta_roots),
    }
}

pub(super) struct PreparedIdentity {
    name: String,
    root_name: String,
    table: Option<PreparedMap<String, ObjectId>>,
    roots: Option<PreparedMap<String, RootId>>,
    charge: Option<SharedSchedulingCharge>,
    owner_prototype: ObjectId,
    kind: IdentityKind,
}
impl PreparedIdentity {
    pub(super) fn prepare(vm: &Vm, name: &str, kind: IdentityKind) -> Result<Self, RuntimeError> {
        let scope = vm.host_scheduling.as_ref();
        let minimum = name
            .len()
            .checked_mul(2)
            .ok_or_else(|| scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail))?;
        let mut prepared = Self {
            name: String::new(),
            root_name: String::new(),
            table: None,
            roots: None,
            charge: scope
                .map(|scope| scope.shared_charge(minimum))
                .transpose()?,
            owner_prototype: vm.object_prototype,
            kind,
        };
        prepared.name = module_referrers::copy_referrer(name, scope)?;
        if let Some(charge) = &mut prepared.charge {
            charge.resize(prepared.name.capacity().saturating_add(name.len()))?;
        }
        prepared.root_name = module_referrers::copy_referrer(name, scope)?;
        if let Some(charge) = &mut prepared.charge {
            charge.resize(
                prepared
                    .name
                    .capacity()
                    .saturating_add(prepared.root_name.capacity()),
            )?;
        }
        let (table, roots) = maps(vm, kind);
        prepared.table = Some(PreparedMap::for_insert(table, scope)?);
        prepared.roots = Some(PreparedMap::for_insert(roots, scope)?);
        Ok(prepared)
    }
    pub(super) fn publish(mut self, vm: &mut Vm, object: ObjectId, root: RootId) {
        assert_eq!(
            self.owner_prototype, vm.object_prototype,
            "cache preparation stays with its VM"
        );
        match (&self.charge, &vm.host_scheduling) {
            (Some(owner), Some(scope)) => assert!(owner.belongs_to(scope)),
            (None, None) => {}
            _ => panic!("cache preparation keeps its document owner"),
        }
        let (table, roots) = maps_mut(vm, self.kind);
        self.table.take().unwrap().commit(table);
        self.roots.take().unwrap().commit(roots);
        table.insert(std::mem::take(&mut self.name), object);
        roots.insert(std::mem::take(&mut self.root_name), root);
        drop(self);
        vm.reconcile_scheduling_ownership();
    }
}

impl Vm {
    pub(super) fn module_identity_cache_bytes(&self) -> usize {
        let mut bytes = self
            .module_source_cache
            .retained_bytes()
            .saturating_add(self.module_source_roots.retained_bytes())
            .saturating_add(self.module_import_meta.retained_bytes())
            .saturating_add(self.module_import_meta_roots.retained_bytes());
        for name in self
            .module_source_cache
            .keys()
            .chain(self.module_source_roots.keys())
            .chain(self.module_import_meta.keys())
            .chain(self.module_import_meta_roots.keys())
        {
            bytes = bytes.saturating_add(name.capacity());
        }
        bytes
    }
    pub(super) fn reclaim_module_identity_cache_storage(&mut self) {
        self.reconcile_scheduling_ownership();
        if self.preserve_promise_table_capacity {
            return;
        }
        let mut scope = self.host_scheduling.as_mut();
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
        reclaim!(module_source_cache);
        reclaim!(module_source_roots);
        reclaim!(module_import_meta);
        reclaim!(module_import_meta_roots);
    }
}
