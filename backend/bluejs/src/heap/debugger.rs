// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Two-pass, side-effect-free debugger copies. Validation uses borrowed stored
//! data and fixed stack scratch; complete output admission precedes allocation.

use super::*;
use crate::vm::{
    InspectionStorage, RuntimeError, VmDebuggerInspection, VmDebuggerValuePreview,
    VM_DEBUGGER_MAX_VALUE_PAYLOAD_BYTES,
};

const MAX_DEPTH: usize = 4;
const MAX_CONTAINER_LENGTH: usize = 32;
const MAX_NODES: usize = 256;
fn unsupported(message: &'static str) -> RuntimeError {
    RuntimeError::Unsupported(message)
}

#[derive(Clone, Copy)]
struct PreviewPrototypes {
    object: ObjectId,
    array: ObjectId,
}

#[derive(Default)]
struct PreviewBudget {
    nodes: usize,
    payload_bytes: usize,
    buffer_bytes: usize,
}
struct PreviewPass<'a> {
    budget: PreviewBudget,
    storage: Option<&'a mut InspectionStorage<VmDebuggerValuePreview>>,
}
impl PreviewPass<'_> {
    fn node(&mut self, depth: usize) -> Result<(), RuntimeError> {
        if depth > MAX_DEPTH || self.budget.nodes == MAX_NODES {
            return Err(unsupported(
                "debugger value exceeds the tree depth or node budget",
            ));
        }
        self.budget.nodes += 1;
        Ok(())
    }
    fn bytes(&mut self, count: usize) -> Result<(), RuntimeError> {
        if count > VM_DEBUGGER_MAX_VALUE_PAYLOAD_BYTES - self.budget.payload_bytes {
            return Err(unsupported(
                "debugger value exceeds the payload byte budget",
            ));
        }
        self.budget.payload_bytes += count;
        Ok(())
    }
    fn vector<T>(&mut self, count: usize) -> Result<Vec<T>, RuntimeError> {
        let bytes = count
            .checked_mul(size_of::<T>())
            .and_then(|n| n.checked_add(self.budget.buffer_bytes))
            .ok_or_else(|| unsupported("debugger native output size overflow"))?;
        self.budget.buffer_bytes = bytes;
        self.storage
            .as_mut()
            .map_or_else(|| Ok(Vec::new()), |storage| storage.vector(count))
    }
    fn copy<T: Copy>(&mut self, data: &[T]) -> Result<Vec<T>, RuntimeError> {
        let mut output = self.vector(data.len())?;
        if self.storage.is_some() {
            output.extend_from_slice(data);
        }
        Ok(output)
    }
}

impl Heap {
    pub(crate) fn debugger_owned_value_preview(
        &self,
        value: &Value,
        object_prototype: ObjectId,
        array_prototype: ObjectId,
    ) -> Result<VmDebuggerInspection<VmDebuggerValuePreview>, RuntimeError> {
        let budget = self.roots.budget();
        if let Some(budget) = budget {
            budget.reserve(0)?;
        }
        let prototypes = PreviewPrototypes {
            object: object_prototype,
            array: array_prototype,
        };
        let mut plan = PreviewPass {
            budget: PreviewBudget::default(),
            storage: None,
        };
        self.preview_value(value, 0, prototypes, &mut plan, &mut [None; MAX_DEPTH + 1])?;
        let mut storage =
            InspectionStorage::<VmDebuggerValuePreview>::new(budget, plan.budget.buffer_bytes)?;
        let mut build = PreviewPass {
            budget: PreviewBudget::default(),
            storage: Some(&mut storage),
        };
        let output =
            self.preview_value(value, 0, prototypes, &mut build, &mut [None; MAX_DEPTH + 1])?;
        debug_assert_eq!(build.budget.buffer_bytes, plan.budget.buffer_bytes);
        storage.finish(output)
    }

    /// Compiler-owned binding cells are ordinary stored data. Borrow their
    /// value directly so inspection never clones a potentially large binding.
    pub(crate) fn debugger_cell_value_preview(
        &self,
        cell: ObjectId,
        object_prototype: ObjectId,
        array_prototype: ObjectId,
    ) -> Result<VmDebuggerInspection<VmDebuggerValuePreview>, RuntimeError> {
        let object = self
            .object(cell)
            .expect("paused execution retains its compiler-owned binding cell");
        let value = object
            .properties
            .iter()
            .find_map(|(key, value)| match key {
                PropertyName::String(name) if name == "value" => Some(value),
                _ => None,
            })
            .ok_or_else(|| unsupported("debugger value binding is uninitialized"))?;
        self.debugger_owned_value_preview(value, object_prototype, array_prototype)
    }

    #[cfg(test)]
    pub(crate) fn debugger_value_preview(
        &self,
        value: &Value,
        object_prototype: ObjectId,
        array_prototype: ObjectId,
    ) -> Result<VmDebuggerValuePreview, &'static str> {
        self.debugger_owned_value_preview(value, object_prototype, array_prototype)
            .map(|value| value.as_ref().clone())
            .map_err(|error| match error {
                RuntimeError::Unsupported(message) => message,
                _ => "debugger native storage admission failed",
            })
    }

    fn preview_value(
        &self,
        value: &Value,
        depth: usize,
        prototypes: PreviewPrototypes,
        pass: &mut PreviewPass<'_>,
        ancestors: &mut [Option<ObjectId>; MAX_DEPTH + 1],
    ) -> Result<VmDebuggerValuePreview, RuntimeError> {
        pass.node(depth)?;
        match value {
            Value::Undefined => Ok(VmDebuggerValuePreview::Undefined),
            Value::Null => Ok(VmDebuggerValuePreview::Null),
            Value::Bool(value) => Ok(VmDebuggerValuePreview::Bool(*value)),
            Value::Number(value) => Ok(VmDebuggerValuePreview::NumberBits(value.to_bits())),
            Value::BigInt(value) => {
                let bits = value.bits();
                let negative = value.sign() == num_bigint::Sign::Minus;
                let sign_byte = bits != 0
                    && bits.is_multiple_of(8)
                    && !(negative && value.trailing_zeros() == Some(bits - 1));
                let count = usize::try_from(bits.div_ceil(8))
                    .ok()
                    .and_then(|n| n.checked_add(usize::from(sign_byte)))
                    .ok_or_else(|| unsupported("debugger value exceeds the payload byte budget"))?
                    .max(1);
                pass.bytes(count)?;
                let mut bytes = pass.vector(count)?;
                if pass.storage.is_some() {
                    bytes.resize(count, 0u8);
                    for (index, digit) in value.iter_u32_digits().enumerate() {
                        for (offset, byte) in digit.to_le_bytes().into_iter().enumerate() {
                            if let Some(output) = bytes.get_mut(index * 4 + offset) {
                                *output = byte;
                            }
                        }
                    }
                    if negative {
                        let mut carry = true;
                        for byte in &mut bytes {
                            let (next, overflow) = (!*byte).overflowing_add(u8::from(carry));
                            *byte = next;
                            carry = overflow;
                        }
                    }
                }
                Ok(VmDebuggerValuePreview::BigIntBytes(bytes))
            }
            Value::String(value) => {
                pass.bytes(value.byte_len())?;
                Ok(VmDebuggerValuePreview::StringUnits(
                    pass.copy(value.as_code_units())?,
                ))
            }
            Value::Symbol(_) => Err(unsupported("debugger value is not plain data")),
            Value::Object(id) => self.preview_object(*id, depth, prototypes, pass, ancestors),
        }
    }

    fn preview_object(
        &self,
        id: ObjectId,
        depth: usize,
        prototypes: PreviewPrototypes,
        pass: &mut PreviewPass<'_>,
        ancestors: &mut [Option<ObjectId>; MAX_DEPTH + 1],
    ) -> Result<VmDebuggerValuePreview, RuntimeError> {
        let object = self
            .object(id)
            .map_err(|_| unsupported("debugger value object is not in the paused heap"))?;
        if ancestors[..depth].contains(&Some(id)) {
            return Err(unsupported("debugger value contains a cycle"));
        }
        if object.is_html_dda || object.private.is_some() {
            return Err(unsupported("debugger value is not plain data"));
        }
        ancestors[depth] = Some(id);
        let result = match &object.kind {
            ObjectKind::Ordinary
                if object.prototype.is_none() || object.prototype == Some(prototypes.object) =>
            {
                self.preview_record(object, depth, prototypes, pass, ancestors)
            }
            ObjectKind::Array { length } if object.prototype == Some(prototypes.array) => {
                self.preview_array(object, *length, depth, prototypes, pass, ancestors)
            }
            _ => Err(unsupported("debugger value is not plain data")),
        };
        ancestors[depth] = None;
        result
    }

    fn preview_record(
        &self,
        object: &Object,
        depth: usize,
        prototypes: PreviewPrototypes,
        pass: &mut PreviewPass<'_>,
        ancestors: &mut [Option<ObjectId>; MAX_DEPTH + 1],
    ) -> Result<VmDebuggerValuePreview, RuntimeError> {
        if object.order.len() > MAX_CONTAINER_LENGTH
            || object.properties.len() != object.order.len()
        {
            return Err(unsupported(
                "debugger record exceeds its entry limit or stored shape",
            ));
        }
        // Stable insertion of numeric indices before stored string keys, with
        // no cloned keys or allocation of sorting scratch.
        let mut keys = [None; MAX_CONTAINER_LENGTH];
        for (index, key) in object.order.iter().enumerate() {
            if matches!(key, PropertyName::Symbol(_)) {
                return Err(unsupported("debugger record has a symbol key"));
            }
            let mut at = index;
            if let Some(target) = array_index(key) {
                while at > 0
                    && array_index(keys[at - 1].expect("preceding key is present"))
                        .is_none_or(|prior| prior > target)
                {
                    keys[at] = keys[at - 1];
                    at -= 1;
                }
            }
            keys[at] = Some(key);
        }
        let mut entries = pass.vector(object.order.len())?;
        for key in keys.into_iter().take(object.order.len()).flatten() {
            let PropertyName::String(name) = key else {
                unreachable!("symbol keys refused above");
            };
            let value = self.preview_stored_data(object, key)?;
            pass.bytes(name.byte_len())?;
            let name = JsString::from_code_units(pass.copy(name.as_code_units())?);
            let value = self.preview_value(value, depth + 1, prototypes, pass, ancestors)?;
            if pass.storage.is_some() {
                entries.push((name, value));
            }
        }
        Ok(VmDebuggerValuePreview::Record(entries))
    }

    fn preview_array(
        &self,
        object: &Object,
        length: u32,
        depth: usize,
        prototypes: PreviewPrototypes,
        pass: &mut PreviewPass<'_>,
        ancestors: &mut [Option<ObjectId>; MAX_DEPTH + 1],
    ) -> Result<VmDebuggerValuePreview, RuntimeError> {
        let length = length as usize;
        if length > MAX_CONTAINER_LENGTH || object.order.len() > length {
            return Err(unsupported("debugger array exceeds its element limit"));
        }
        if object.properties.len() != object.order.len()
            || object
                .order
                .iter()
                .any(|key| key.index().is_none_or(|index| index >= length))
        {
            return Err(unsupported("debugger array has non-index own data"));
        }
        let mut elements = pass.vector(length)?;
        for index in 0..length {
            let key = object.order.iter().find(|key| key.index() == Some(index));
            let value = if let Some(key) = key {
                Some(self.preview_value(
                    self.preview_stored_data(object, key)?,
                    depth + 1,
                    prototypes,
                    pass,
                    ancestors,
                )?)
            } else {
                pass.node(depth + 1)?;
                None
            };
            if pass.storage.is_some() {
                elements.push(value);
            }
        }
        Ok(VmDebuggerValuePreview::Array(elements))
    }

    fn preview_stored_data<'a>(
        &self,
        object: &'a Object,
        key: &PropertyName,
    ) -> Result<&'a Value, RuntimeError> {
        if object
            .attributes
            .get(key)
            .is_some_and(PropertyDescriptor::accessor)
        {
            return Err(unsupported("debugger value has an accessor"));
        }
        object
            .properties
            .get(key)
            .ok_or_else(|| unsupported("debugger value has no stored own data"))
    }
}
