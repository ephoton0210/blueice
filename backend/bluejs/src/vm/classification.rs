// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Value classification owns no callback, coercion or intrinsic initialization.
//! All object ingress stays fallible. The returned bits can be read without
//! touching the heap again; they confer no permission to dereference an object.

use super::*;

impl Vm {
    pub(super) fn value_capabilities(
        &self,
        object: ObjectId,
    ) -> Result<crate::heap::ObjectCapabilities, RuntimeError> {
        let mut bits = self.heap.object_capabilities(object)?;
        if let Some((callable, constructible)) = self
            .test262_foreign_reference(object)
            .map(|(_, _, callable, constructible)| (callable, constructible))
            .or_else(|| {
                self.test262_reverse_reference(object)
                    .map(|(_, _, callable, constructible)| (callable, constructible))
            })
        {
            bits.callable = callable;
            bits.constructible = constructible;
        }
        Ok(bits)
    }

    pub(super) fn is_callable(&self, value: &Value) -> Result<bool, RuntimeError> {
        match value.object_id() {
            Some(object) => Ok(self.value_capabilities(object)?.callable),
            None => Ok(false),
        }
    }

    pub(super) fn is_constructor(&self, value: &Value) -> Result<bool, RuntimeError> {
        match value.object_id() {
            Some(object) => Ok(self.value_capabilities(object)?.constructible),
            None => Ok(false),
        }
    }

    /// ECMAScript ToBoolean, including the host-defined Annex B IsHTMLDDA.
    pub(super) fn to_boolean(&self, value: &Value) -> Result<bool, RuntimeError> {
        match value.object_id() {
            Some(object) => Ok(!self.value_capabilities(object)?.html_dda),
            None => Ok(primitive::truthy(value)),
        }
    }

    pub(super) fn typeof_value(&self, value: &Value) -> Result<&'static str, RuntimeError> {
        let Some(object) = value.object_id() else {
            return Ok(primitive::type_name(value));
        };
        let bits = self.value_capabilities(object)?;
        Ok(if bits.html_dda {
            "undefined"
        } else if bits.callable {
            "function"
        } else {
            "object"
        })
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/common_boundary_contracts.rs"]
mod boundary_contracts;
