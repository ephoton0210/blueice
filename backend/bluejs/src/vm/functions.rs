// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::heap::BoundFunction;

impl Vm {
    pub(super) fn bind_function(&mut self, target: Value, args: &[Value]) -> Result<Value, RuntimeError> {
        if !self.is_callable(&target)? {
            return Err(RuntimeError::TypeError("bind requires a callable".into()));
        }
        let id = target.object_id().unwrap();
        let prototype = self.heap.prototype(id)?;
        let bound =
            BoundFunction { target: id, this: native::argument(args, 0).clone(), args: args.iter().skip(1).cloned().collect(), constructible: self.is_constructor(&target)? };
        let count = bound.args.len();
        let function = self.with_roots(|heap| heap.alloc_bound_function(bound, prototype))?;
        self.stack.push(Value::Object(function));
        let mut length = 0.0;
        if self.heap.get_own_property_descriptor(id, "length")?.is_some() {
            if let Value::Number(number) = self.get_property(&target, &"length".into())? {
                // ECMAScript's max(0, …) returns +0 for a -0 target length.
                // `f64::max` may retain the receiver's -0 sign for equal
                // operands, so make that observable zero explicitly.
                let candidate = number.trunc() - count as f64;
                length = if candidate.is_nan() || candidate <= 0.0 { 0.0 } else { candidate };
            }
        }
        self.define_data(function, "length", Value::Number(length), false, false, true)?;
        let target_name = self.get_property(&target, &"name".into())?;
        let mut name = JsString::from("bound ");
        if let Value::String(target_name) = target_name {
            native::append(&mut name, &target_name, self.config.max_string_bytes)?;
        }
        self.check_string(&Value::String(name.clone()))?;
        self.define_data(function, "name", Value::String(name), false, false, true)?;
        Ok(Value::Object(function))
    }

    // `ordinary` selects the entry algorithm: direct @@hasInstance calls
    // start with OrdinaryHasInstance; the operator starts with hook lookup.
    pub(super) fn has_instance(&mut self, value: Value, mut target: Value, mut ordinary: bool) -> Result<bool, RuntimeError> {
        loop {
            self.charge_step()?;
            if !ordinary {
                if !matches!(target, Value::Object(_)) {
                    return Err(RuntimeError::TypeError("instanceof target must be an object".into()));
                }
                let method = self.get_method(&target, &JsSymbol::well_known("hasInstance").into())?;
                if let Value::Object(id) = method {
                    // The intrinsic can be tail-dispatched here. Custom hooks
                    // still use normal call rooting, error and depth handling.
                    if self.heap.native_function(id)? != Some(NativeFunction::HasInstance) {
                        return self.call_native(method, target, vec![value], false).map(|v| primitive::truthy(&v));
                    }
                } else if !self.is_callable(&target)? {
                    return Err(RuntimeError::TypeError("instanceof target must be callable".into()));
                }
            }
            if !self.is_callable(&target)? {
                return Ok(false);
            }
            if let Some(bound) = self.heap.bound_function(target.object_id().unwrap())? {
                target = Value::Object(bound.target);
                ordinary = false;
                continue;
            }
            let Value::Object(mut object) = value else { return Ok(false) };
            let prototype = self.get_property(&target, &"prototype".into())?;
            let Value::Object(prototype) = prototype else { return Err(RuntimeError::TypeError("instanceof prototype must be an object".into())) };
            loop {
                self.charge_step()?;
                let Some(parent) = self.heap.prototype(object)? else { return Ok(false) };
                if parent == prototype {
                    return Ok(true);
                }
                object = parent;
            }
        }
    }
}
