// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module JSON borrows UTF-8, leases its AST and needs no reviver source map.

use super::*;
use crate::vm::module_values::{ModuleKeyCopy, ModuleValueCopy};

impl Vm {
    pub(in super::super) fn json_module_value(
        &mut self,
        source: &str,
    ) -> Result<ModuleValueCopy, RuntimeError> {
        let parsed =
            JsonSourceParser::for_module(source, self.host_scheduling.as_ref())?.parse()?;
        // Parsing has validated the entire input before heap construction.
        let base = self.stack.len();
        let required = base.saturating_add(parsed.depth);
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: required
                .saturating_sub(self.stack.capacity())
                .saturating_mul(std::mem::size_of::<Value>()),
            ..Default::default()
        })?;
        if self
            .stack
            .try_reserve_exact(required.saturating_sub(base))
            .is_err()
        {
            return Err(self
                .host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail));
        }
        self.synchronize_scheduling_usage()?;
        let result = self.json_module_node(&parsed);
        self.stack.truncate(base);
        result
    }

    fn json_module_node(&mut self, node: &JsonNode) -> Result<ModuleValueCopy, RuntimeError> {
        match node {
            JsonNode::Primitive { value, .. } => {
                self.check_string(value)?;
                ModuleValueCopy::copy(value, self.host_scheduling.as_ref())
            }
            JsonNode::Array(values) => {
                let length = u32::try_from(values.len())
                    .map_err(|_| RuntimeError::Heap(HeapError::InvalidArrayLength))?;
                let prototype = self.array_prototype;
                let array = self.with_roots(|heap| heap.alloc_array(length, Some(prototype)))?;
                let base = self.stack.len();
                self.stack.push(Value::Object(array));
                let result = (|| {
                    for (index, node) in values.iter().enumerate() {
                        self.charge_step()?;
                        let value = self.json_module_node(node)?;
                        if let Some(object) = value.value().object_id() {
                            self.stack.push(Value::Object(object));
                        }
                        let key =
                            ModuleKeyCopy::index(index as u32, self.host_scheduling.as_ref())?;
                        self.with_roots(|heap| {
                            heap.set(array, key.into_managed(), value.into_managed())
                        })?;
                        self.stack.truncate(base + 1);
                    }
                    ModuleValueCopy::copy(&Value::Object(array), self.host_scheduling.as_ref())
                })();
                self.stack.truncate(base);
                result
            }
            JsonNode::Object(values) => {
                let prototype = self.object_prototype;
                let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                let base = self.stack.len();
                self.stack.push(Value::Object(object));
                let result = (|| {
                    for (key, node) in values {
                        self.charge_step()?;
                        let value = self.json_module_node(node)?;
                        if let Some(object) = value.value().object_id() {
                            self.stack.push(Value::Object(object));
                        }
                        let key = ModuleKeyCopy::from_units(
                            key.as_code_units(),
                            self.host_scheduling.as_ref(),
                        )?;
                        self.with_roots(|heap| {
                            heap.set(object, key.into_managed(), value.into_managed())
                        })?;
                        self.stack.truncate(base + 1);
                    }
                    ModuleValueCopy::copy(&Value::Object(object), self.host_scheduling.as_ref())
                })();
                self.stack.truncate(base);
                result
            }
        }
    }
}
