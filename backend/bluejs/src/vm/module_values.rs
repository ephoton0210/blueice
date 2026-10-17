// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Derived module values share storage; conversion copies retain scoped leases.

use super::host_scheduling::{value_payload_bytes, SharedSchedulingCharge};
use super::*;

struct SyntheticValueOwner {
    value: Value,
    bytes: usize,
    charge: RefCell<Option<SharedSchedulingCharge>>,
}

#[derive(Clone)]
pub(crate) struct SyntheticModuleValue(Rc<SyntheticValueOwner>);

fn allocation_error(charge: &Option<SharedSchedulingCharge>) -> RuntimeError {
    charge
        .as_ref()
        .map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail)
}

fn resize(charge: &mut Option<SharedSchedulingCharge>, bytes: usize) -> Result<(), RuntimeError> {
    if let Some(charge) = charge {
        charge.resize(bytes)?;
    }
    Ok(())
}

fn encode_text(
    source: &str,
    charge: &mut Option<SharedSchedulingCharge>,
    header: usize,
) -> Result<JsString, RuntimeError> {
    let length = source.encode_utf16().count();
    resize(charge, header.saturating_add(length.saturating_mul(2)))?;
    let mut units = Vec::new();
    units
        .try_reserve_exact(length)
        .map_err(|_| allocation_error(charge))?;
    resize(
        charge,
        header.saturating_add(units.capacity().saturating_mul(2)),
    )?;
    units.extend(source.encode_utf16());
    Ok(JsString::from_code_units(units))
}

pub(super) struct PreparedSyntheticValue {
    charge: Option<SharedSchedulingCharge>,
}

impl PreparedSyntheticValue {
    pub(super) fn adopt_copy(
        mut self,
        mut copy: ModuleValueCopy,
    ) -> Result<SyntheticModuleValue, RuntimeError> {
        if let Some(charge) = copy.charge.take() {
            if let Some(owner) = &mut self.charge {
                owner.absorb(charge)?;
            } else {
                self.charge = Some(charge);
            }
        }
        self.finish(copy.value)
    }

    pub(super) fn text(mut self, source: &str) -> Result<SyntheticModuleValue, RuntimeError> {
        let text = encode_text(
            source,
            &mut self.charge,
            SyntheticModuleValue::header_bytes(),
        )?;
        self.finish(Value::String(text))
    }

    pub(super) fn finish(mut self, value: Value) -> Result<SyntheticModuleValue, RuntimeError> {
        let bytes =
            SyntheticModuleValue::header_bytes().saturating_add(value_payload_bytes(&value));
        resize(&mut self.charge, bytes)?;
        Ok(SyntheticModuleValue(Rc::new(SyntheticValueOwner {
            value,
            bytes,
            charge: RefCell::new(self.charge),
        })))
    }
}

impl SyntheticModuleValue {
    pub(crate) fn value(&self) -> &Value {
        &self.0.value
    }

    pub(super) fn header_bytes() -> usize {
        std::mem::size_of::<SyntheticValueOwner>()
    }

    pub(super) fn retained_bytes(&self) -> usize {
        self.0.bytes
    }

    pub(super) fn attach(&self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        let mut charge = self.0.charge.borrow_mut();
        if let Some(charge) = charge.as_ref() {
            if !charge.belongs_to(scope) {
                scope.fail();
                return Err(RuntimeError::TypeError(
                    "synthetic module value already has a document owner".into(),
                ));
            }
        } else {
            *charge = Some(scope.shared_charge(self.retained_bytes())?);
        }
        Ok(())
    }
}

/// A primitive copy awaiting heap adoption or transfer to an export owner.
pub(super) struct ModuleValueCopy {
    value: Value,
    charge: Option<SharedSchedulingCharge>,
}

impl ModuleValueCopy {
    pub(super) fn into_retained(self) -> (Value, Option<SharedSchedulingCharge>) {
        (self.value, self.charge)
    }
    pub(super) fn copy(
        value: &Value,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        if let Value::String(text) = value {
            let copy = ModuleKeyCopy::from_units(text.as_code_units(), scope)?;
            return Ok(Self {
                value: Value::String(copy.text),
                charge: copy.charge,
            });
        }
        let mut charge = scope
            .map(|scope| scope.shared_charge(value_payload_bytes(value)))
            .transpose()?;
        let value = value.clone();
        resize(&mut charge, value_payload_bytes(&value))?;
        Ok(Self { value, charge })
    }

    pub(super) fn value(&self) -> &Value {
        &self.value
    }

    /// Called inside the heap operation, immediately before managed adoption.
    pub(super) fn into_managed(self) -> Value {
        drop(self.charge);
        self.value
    }
}

pub(super) struct ModuleKeyCopy {
    text: JsString,
    charge: Option<SharedSchedulingCharge>,
}

impl ModuleKeyCopy {
    pub(super) fn from_units(
        source: &[u16],
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let mut charge = scope
            .map(|scope| scope.shared_charge(source.len().saturating_mul(2)))
            .transpose()?;
        let mut units = Vec::new();
        units
            .try_reserve_exact(source.len())
            .map_err(|_| allocation_error(&charge))?;
        resize(&mut charge, units.capacity().saturating_mul(2))?;
        units.extend_from_slice(source);
        Ok(Self {
            text: JsString::from_code_units(units),
            charge,
        })
    }

    pub(super) fn index(
        mut index: u32,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let mut digits = [0u16; 10];
        let mut start = digits.len();
        loop {
            start -= 1;
            digits[start] = u16::from(b'0') + (index % 10) as u16;
            index /= 10;
            if index == 0 {
                break;
            }
        }
        Self::from_units(&digits[start..], scope)
    }

    pub(super) fn into_managed(self) -> JsString {
        drop(self.charge);
        self.text
    }
}

pub(super) struct ModuleBytesCopy {
    data: Vec<u8>,
    charge: Option<SharedSchedulingCharge>,
}

impl ModuleBytesCopy {
    pub(super) fn copy(
        source: &[u8],
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let mut charge = scope
            .map(|scope| scope.shared_charge(source.len()))
            .transpose()?;
        let mut data = Vec::new();
        data.try_reserve_exact(source.len())
            .map_err(|_| allocation_error(&charge))?;
        resize(&mut charge, data.capacity())?;
        data.extend_from_slice(source);
        Ok(Self { data, charge })
    }

    pub(super) fn len(&self) -> usize {
        self.data.len()
    }

    /// Called inside the heap operation, immediately before managed adoption.
    pub(super) fn into_managed(self) -> Vec<u8> {
        drop(self.charge);
        self.data
    }
}

impl Vm {
    pub(super) fn prepare_module_buffer_root(&mut self) -> Result<(), RuntimeError> {
        let required = self.stack.len().saturating_add(1);
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: required
                .saturating_sub(self.stack.capacity())
                .saturating_mul(std::mem::size_of::<Value>()),
            ..Default::default()
        })?;
        if self
            .stack
            .try_reserve_exact(required.saturating_sub(self.stack.len()))
            .is_err()
        {
            return Err(self
                .host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail));
        }
        self.synchronize_scheduling_usage()
    }

    pub(super) fn prepare_synthetic_value(
        &mut self,
        payload_hint: usize,
    ) -> Result<PreparedSyntheticValue, RuntimeError> {
        self.reconcile_scheduling_ownership();
        let bytes = SyntheticModuleValue::header_bytes().saturating_add(payload_hint);
        Ok(PreparedSyntheticValue {
            charge: self
                .host_scheduling
                .as_ref()
                .map(|scope| scope.shared_charge(bytes))
                .transpose()?,
        })
    }

    pub(super) fn attach_synthetic_values(
        modules: &HashMap<String, Bytecode>,
        scope: &SchedulingScope,
    ) -> Result<(), RuntimeError> {
        for value in modules
            .values()
            .filter_map(|code| code.synthetic_default_export.as_ref())
        {
            value.attach(scope)?;
        }
        Ok(())
    }

    pub(super) fn install_synthetic_export(
        &mut self,
        cell: ObjectId,
        export: &SyntheticModuleValue,
    ) -> Result<(), RuntimeError> {
        self.reconcile_scheduling_ownership();
        if let Some(scope) = &self.host_scheduling {
            export.attach(scope)?;
        }
        let value = ModuleValueCopy::copy(export.value(), self.host_scheduling.as_ref())?;
        self.with_roots(|heap| heap.set(cell, "value", value.into_managed()))
    }
}
