// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Realm-owned template identities with staged cache publication.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;
use crate::bytecode::TemplateSite;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/template_storage.rs"]
mod storage_tests;

pub(super) struct PreparedTemplateCache {
    replacement: Option<HashMap<TemplateSiteId, ObjectId>>,
    charge: Option<SharedSchedulingCharge>,
    previous: (usize, usize),
}

impl PreparedTemplateCache {
    fn admit_reported(&mut self, operand_bytes: usize) -> Result<(), RuntimeError> {
        let bytes = self
            .replacement
            .as_ref()
            .map_or(0, host_scheduling::map_slots)
            .saturating_add(operand_bytes);
        if let Some(charge) = &mut self.charge {
            charge.resize(bytes)?;
        }
        Ok(())
    }

    fn publish(mut self, vm: &mut Vm, site: TemplateSiteId, object: ObjectId) {
        // Template construction runs no page code and GC keeps realm template
        // identities strongly owned, so neither operation changes this table.
        assert_eq!((vm.templates.len(), vm.templates.capacity()), self.previous);
        if let Some(mut table) = self.replacement.take() {
            table.extend(vm.templates.drain());
            vm.templates = table;
        }
        debug_assert!(vm.templates.len() < vm.templates.capacity());
        vm.templates.insert(site, object);
        // Adopt fields, refund staging and reconcile without intervening work.
        drop(self);
        vm.reconcile_scheduling_ownership();
    }
}

impl Vm {
    pub(super) fn prepare_template_cache_insert(
        &mut self,
    ) -> Result<PreparedTemplateCache, RuntimeError> {
        let result = self.prepare_template_cache_storage();
        if result.is_err() {
            self.reconcile_scheduling_ownership();
        }
        result
    }

    fn prepare_template_cache_storage(&mut self) -> Result<PreparedTemplateCache, RuntimeError> {
        self.synchronize_scheduling_usage()?;
        let required = self.templates.len().saturating_add(1);
        let replace = required > self.templates.capacity();
        let stack_required = self.stack.len().saturating_add(2);
        let replace_stack = stack_required > self.stack.capacity();
        let operand_bytes = if replace_stack {
            stack_required.saturating_mul(std::mem::size_of::<Value>())
        } else {
            0
        };
        let minimum = operand_bytes.saturating_add(if replace {
            required.saturating_mul(std::mem::size_of::<(TemplateSiteId, ObjectId)>())
        } else {
            0
        });
        let charge = self
            .host_scheduling
            .as_ref()
            .map(|scope| scope.shared_charge(minimum))
            .transpose()?;
        let mut prepared = PreparedTemplateCache {
            replacement: None,
            charge,
            previous: (self.templates.len(), self.templates.capacity()),
        };
        let fail = || {
            self.host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
        };
        if replace {
            let mut table = HashMap::new();
            table.try_reserve(required).map_err(|_| fail())?;
            prepared.replacement = Some(table);
        }
        // Check reported table capacity before allocating the operand buffer.
        prepared.admit_reported(operand_bytes)?;
        if replace_stack {
            let mut stack = Vec::new();
            stack
                .try_reserve_exact(stack_required)
                .map_err(|_| fail())?;
            prepared.admit_reported(host_scheduling::slots(&stack))?;
            stack.append(&mut self.stack);
            self.stack = stack;
            let table_bytes = prepared
                .replacement
                .as_ref()
                .map_or(0, host_scheduling::map_slots);
            if let Some(charge) = &mut prepared.charge {
                charge.release_to(table_bytes);
            }
            self.reconcile_scheduling_ownership();
        }
        Ok(prepared)
    }

    pub(super) fn template_object(&mut self, site: &TemplateSite) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        let result = self.template_object_inner(site);
        self.stack.truncate(base);
        self.reconcile_scheduling_ownership();
        result
    }

    fn template_object_inner(&mut self, site: &TemplateSite) -> Result<Value, RuntimeError> {
        self.check_scheduling_failure()?;
        if let Some(&object) = self.templates.get(&site.id) {
            return Ok(Value::Object(object));
        }
        let prepared = self.prepare_template_cache_insert()?;
        for string in site.raw.iter().chain(site.cooked.iter().flatten()) {
            self.check_string(&Value::String(string.clone()))?;
        }
        // Build final heap arrays directly. No temporary value vectors or
        // property-key enumeration buffers need to cross allocation safepoints.
        let raw = self.template_array(site.raw.iter().cloned().map(Some))?;
        let cooked = self.template_array(site.cooked.iter().cloned())?;
        self.define_data(cooked, "raw", Value::Object(raw), false, false, false)?;
        for array in [raw, cooked] {
            self.heap.prevent_extensions(array)?;
        }
        prepared.publish(self, site.id.clone(), cooked);
        Ok(Value::Object(cooked))
    }

    fn template_array(
        &mut self,
        values: impl ExactSizeIterator<Item = Option<JsString>>,
    ) -> Result<ObjectId, RuntimeError> {
        let length = values.len();
        let prototype = self.array_create_prototype()?;
        let array = self.with_roots(|heap| heap.alloc_array(length as u32, Some(prototype)))?;
        self.stack.push(Value::Object(array));
        for (index, value) in values.enumerate() {
            self.define_data(
                array,
                index.to_string(),
                value.map_or(Value::Undefined, Value::String),
                false,
                true,
                false,
            )?;
        }
        self.define_data(
            array,
            "length",
            Value::Number(length as f64),
            false,
            false,
            false,
        )?;
        Ok(array)
    }
}
