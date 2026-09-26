// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Side-effect-free, bounded copies of paused debugger values. This walks
//! stored own data descriptors directly, never ECMAScript property lookup.

use super::*;
use crate::vm::{VmDebuggerValuePreview, VM_DEBUGGER_MAX_VALUE_PAYLOAD_BYTES};

const MAX_DEPTH: u32 = 4;
const MAX_CONTAINER_LENGTH: usize = 32;
const MAX_NODES: usize = 256;

#[derive(Clone, Copy)]
struct PreviewPrototypes {
    object: ObjectId,
    array: ObjectId,
}

struct PreviewBudget {
    nodes: usize,
    payload_bytes: usize,
}

impl PreviewBudget {
    fn node(&mut self, depth: u32) -> Result<(), &'static str> {
        if depth > MAX_DEPTH || self.nodes == MAX_NODES {
            return Err("debugger value exceeds the tree depth or node budget");
        }
        self.nodes += 1;
        Ok(())
    }

    fn bytes(&mut self, count: usize) -> Result<(), &'static str> {
        if count > VM_DEBUGGER_MAX_VALUE_PAYLOAD_BYTES - self.payload_bytes {
            return Err("debugger value exceeds the payload byte budget");
        }
        self.payload_bytes += count;
        Ok(())
    }
}

impl Heap {
    pub(crate) fn debugger_value_preview(
        &self,
        value: &Value,
        object_prototype: ObjectId,
        array_prototype: ObjectId,
    ) -> Result<VmDebuggerValuePreview, &'static str> {
        let mut budget = PreviewBudget {
            nodes: 0,
            payload_bytes: 0,
        };
        let mut ancestors = HashSet::new();
        let prototypes = PreviewPrototypes {
            object: object_prototype,
            array: array_prototype,
        };
        self.preview_value(value, 0, prototypes, &mut budget, &mut ancestors)
    }

    fn preview_value(
        &self,
        value: &Value,
        depth: u32,
        prototypes: PreviewPrototypes,
        budget: &mut PreviewBudget,
        ancestors: &mut HashSet<ObjectId>,
    ) -> Result<VmDebuggerValuePreview, &'static str> {
        budget.node(depth)?;
        match value {
            Value::Undefined => Ok(VmDebuggerValuePreview::Undefined),
            Value::Null => Ok(VmDebuggerValuePreview::Null),
            Value::Bool(value) => Ok(VmDebuggerValuePreview::Bool(*value)),
            Value::Number(value) => Ok(VmDebuggerValuePreview::NumberBits(value.to_bits())),
            Value::BigInt(value) => {
                let remaining = VM_DEBUGGER_MAX_VALUE_PAYLOAD_BYTES - budget.payload_bytes;
                // A positive sign byte can add one byte at this boundary;
                // the exact post-check below rejects it without an output.
                if value.bits() > (remaining as u64) * 8 {
                    return Err("debugger value exceeds the payload byte budget");
                }
                let bytes = value.to_signed_bytes_le();
                budget.bytes(bytes.len())?;
                Ok(VmDebuggerValuePreview::BigIntBytes(bytes))
            }
            Value::String(value) => {
                budget.bytes(value.byte_len())?;
                Ok(VmDebuggerValuePreview::StringUnits(
                    value.as_code_units().to_vec(),
                ))
            }
            Value::Symbol(_) => Err("debugger value is not plain data"),
            Value::Object(id) => self.preview_object(*id, depth, prototypes, budget, ancestors),
        }
    }

    fn preview_object(
        &self,
        id: ObjectId,
        depth: u32,
        prototypes: PreviewPrototypes,
        budget: &mut PreviewBudget,
        ancestors: &mut HashSet<ObjectId>,
    ) -> Result<VmDebuggerValuePreview, &'static str> {
        let obj = self
            .object(id)
            .map_err(|_| "debugger value object is not in the paused heap")?;
        if !ancestors.insert(id) {
            return Err("debugger value contains a cycle");
        }
        if obj.is_html_dda || obj.private.is_some() {
            return Err("debugger value is not plain data");
        }
        let result = match &obj.kind {
            ObjectKind::Ordinary
                if obj.prototype.is_none() || obj.prototype == Some(prototypes.object) =>
            {
                self.preview_record(obj, depth, prototypes, budget, ancestors)
            }
            ObjectKind::Array { length } if obj.prototype == Some(prototypes.array) => {
                self.preview_array(obj, *length, depth, prototypes, budget, ancestors)
            }
            _ => Err("debugger value is not plain data"),
        };
        ancestors.remove(&id);
        result
    }

    fn preview_record(
        &self,
        obj: &Object,
        depth: u32,
        prototypes: PreviewPrototypes,
        budget: &mut PreviewBudget,
        ancestors: &mut HashSet<ObjectId>,
    ) -> Result<VmDebuggerValuePreview, &'static str> {
        if obj.order.len() > MAX_CONTAINER_LENGTH || obj.properties.len() != obj.order.len() {
            return Err("debugger record exceeds its entry limit or stored shape");
        }
        // The ordinary kind was validated by preview_object. Sorting its
        // stored keys cannot fail or run a user trap.
        let keys = object_storage::ordered_stored_property_keys(&obj.order, Vec::new(), Vec::new());
        let mut entries = Vec::with_capacity(keys.len());
        for key in keys {
            let PropertyName::String(name) = &key else {
                return Err("debugger record has a symbol key");
            };
            let value = self.preview_stored_data(obj, &key)?;
            budget.bytes(name.byte_len())?;
            entries.push((
                name.clone(),
                self.preview_value(value, depth + 1, prototypes, budget, ancestors)?,
            ));
        }
        Ok(VmDebuggerValuePreview::Record(entries))
    }

    fn preview_array(
        &self,
        obj: &Object,
        length: u32,
        depth: u32,
        prototypes: PreviewPrototypes,
        budget: &mut PreviewBudget,
        ancestors: &mut HashSet<ObjectId>,
    ) -> Result<VmDebuggerValuePreview, &'static str> {
        let length = length as usize;
        if length > MAX_CONTAINER_LENGTH || obj.order.len() > length {
            return Err("debugger array exceeds its element limit");
        }
        if obj.properties.len() != obj.order.len()
            || obj
                .order
                .iter()
                .any(|key| key.index().is_none_or(|index| index >= length))
        {
            return Err("debugger array has non-index own data");
        }
        let mut elements = Vec::with_capacity(length);
        for index in 0..length {
            let key: PropertyName = index.to_string().into();
            if obj.properties.contains_key(&key) {
                let value = self.preview_stored_data(obj, &key)?;
                elements.push(Some(self.preview_value(
                    value,
                    depth + 1,
                    prototypes,
                    budget,
                    ancestors,
                )?));
            } else {
                budget.node(depth + 1)?;
                elements.push(None);
            }
        }
        Ok(VmDebuggerValuePreview::Array(elements))
    }

    fn preview_stored_data<'a>(
        &self,
        obj: &'a Object,
        key: &PropertyName,
    ) -> Result<&'a Value, &'static str> {
        if obj
            .attributes
            .get(key)
            .is_some_and(PropertyDescriptor::accessor)
        {
            return Err("debugger value has an accessor");
        }
        obj.properties
            .get(key)
            .ok_or("debugger value has no stored own data")
    }
}
