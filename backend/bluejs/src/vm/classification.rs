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

    /// Words a call's failure by what the source called, when the failure is that the callee
    /// itself is not a function (`a.b.c is not a function`). A callback a builtin rejects
    /// raises the same generic message from inside the builtin; it is left alone.
    pub(super) fn word_uncallable_callee(
        &self,
        error: RuntimeError,
        callee: Option<ObjectId>,
        code: &Bytecode,
        offset: usize,
    ) -> RuntimeError {
        let RuntimeError::TypeError(message) = &error else {
            return error;
        };
        if message != "value is not callable" {
            return error;
        }
        let callee_is_callable = match callee {
            None => false,
            Some(id) => self.is_callable(&Value::Object(id)).unwrap_or(true),
        };
        if callee_is_callable {
            return error;
        }
        let at = offset as u32;
        match code
            .call_names
            .binary_search_by_key(&at, |(call_offset, _)| *call_offset)
        {
            Ok(index) => RuntimeError::TypeError(format!(
                "{} is not a function",
                code.call_names[index].1
            )),
            Err(_) => error,
        }
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
