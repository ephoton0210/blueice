// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Membrane roots are conditional edges at complete, inactive agent safepoints.

use super::realm_collection::{MembranePin, RealmCollection};
use super::*;

impl Vm {
    pub(super) fn visit_membrane_pins(
        &self,
        mut visit: impl FnMut(MembranePin) -> Result<(), RuntimeError>,
    ) -> Result<(), RuntimeError> {
        for (&wrapper, value) in &self.test262_foreign_values {
            let Some(realm) = self.test262_realms.get(&value.realm) else {
                continue;
            };
            visit(MembranePin::new(
                wrapper,
                wrapper,
                value._wrapper_root,
                realm.vm.heap(),
            ))?;
            visit(MembranePin::new(
                wrapper,
                value.target,
                value._target_root,
                wrapper.heap,
            ))?;
        }
        for (&wrapper, value) in &self.test262_reverse_values {
            visit(MembranePin::new(
                wrapper,
                wrapper,
                value._wrapper_root,
                value.home_heap,
            ))?;
            visit(MembranePin::new(
                wrapper,
                value.target,
                value._target_root,
                wrapper.heap,
            ))?;
        }
        for realm in self.test262_realms.values() {
            for (&target, value) in &realm.imported_values {
                let source = value
                    .value
                    .object_id()
                    .expect("transport identity belongs to an object");
                visit(MembranePin::new(
                    target,
                    target,
                    value._target_root,
                    source.heap,
                ))?;
                visit(MembranePin::new(
                    target,
                    source,
                    value._source_root,
                    target.heap,
                ))?;
            }
        }
        for (&(buffer, _), mirror) in &self.test262_foreign_buffer_mirrors {
            if let Some(root) = mirror._buffer_root {
                visit(MembranePin::new(buffer, buffer, root, mirror.target.heap))?;
            }
        }
        Ok(())
    }

    /// Same-heap metadata edges apply to ordinary allocating collections too.
    /// Neither end is a root: a dead facade/mirror pair cannot retain itself.
    pub(super) fn visit_membrane_owned_edges(&self, mut visit: impl FnMut(ObjectId, ObjectId)) {
        for (&wrapper, value) in &self.test262_foreign_values {
            if let Some(prototype) = value.prototype_override {
                visit(wrapper, prototype);
            }
        }
        for (&wrapper, value) in &self.test262_reverse_values {
            if let Some(prototype) = value.prototype_override {
                visit(wrapper, prototype);
            }
        }
        for (&(buffer, _), mirror) in &self.test262_foreign_buffer_mirrors {
            if let Some(facade) = mirror.facade {
                visit(buffer, facade);
                visit(facade, buffer);
            }
        }
    }

    pub(super) fn prune_membrane_owners(&mut self, plan: &RealmCollection) {
        self.test262_foreign_values
            .retain(|wrapper, _| plan.object_is_live(*wrapper));
        self.test262_reverse_values.retain(|wrapper, value| {
            // A locally entered child cannot release an absent parent's root.
            // Keep the complete pair until its owner's agent can inspect it.
            plan.index_of(value.home_heap).is_none() || plan.object_is_live(*wrapper)
        });
        self.test262_foreign_buffer_mirrors
            .retain(|(buffer, _), mirror| {
                plan.index_of(mirror.target.heap).is_none()
                    || (plan.object_is_live(*buffer) && plan.object_is_live(mirror.target))
            });
        self.test262_realms
            .retain(|_, realm| plan.is_active(realm.vm.heap()));
        for realm in self.test262_realms.values_mut() {
            realm
                .wrappers
                .retain(|_, wrapper| plan.object_is_live(*wrapper));
            realm
                .imported_values
                .retain(|target, _| plan.object_is_live(*target));
            realm
                .imported_sources
                .retain(|_, target| realm.imported_values.contains_key(target));
            realm
                .shadow_realm_reexports
                .retain(|_, instance| plan.object_is_live(*instance));
            release_empty(&mut realm.wrappers);
            release_empty(&mut realm.imported_values);
            release_empty(&mut realm.imported_sources);
            release_empty(&mut realm.shadow_realm_reexports);
        }
        release_empty(&mut self.test262_foreign_values);
        release_empty(&mut self.test262_reverse_values);
        release_empty(&mut self.test262_foreign_buffer_mirrors);
        release_empty(&mut self.test262_realms);
    }
}

fn release_empty<K, V>(table: &mut HashMap<K, V>) {
    if table.is_empty() {
        *table = HashMap::new();
    }
}
