// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ES2023 change-array-by-copy methods: `Array.prototype.toReversed`,
//! `toSorted`, `toSpliced` and `with`, plus `Array.prototype[@@unscopables]`.
//!
//! Each method reads its receiver through `Get` at the ordinary property
//! boundary (holes read as `undefined`, never `HasProperty`) and builds the
//! result with `ArrayCreate` on the current Realm's `%Array.prototype%`. They
//! never consult `@@species`, unlike `slice`/`splice`. Temporary values use
//! operand-stack or scoped object-ID roots across guest operations. `toSorted`
//! owns its native snapshot and moves each payload lease into result creation.
use super::super::{native_value_storage, realm_call_storage};
use super::*;

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const MAX_ARRAY_LENGTH: f64 = 4_294_967_295.0;

/// Names `Array.prototype[@@unscopables]` hides from `with` statements, in
/// the order the specification creates them.
pub(in super::super) const ARRAY_UNSCOPABLES: [&str; 16] = [
    "at",
    "copyWithin",
    "entries",
    "fill",
    "find",
    "findIndex",
    "findLast",
    "findLastIndex",
    "flat",
    "flatMap",
    "includes",
    "keys",
    "toReversed",
    "toSorted",
    "toSpliced",
    "values",
];

impl Vm {
    /// `ArrayCreate(length)` with the current Realm's `%Array.prototype%`.
    pub(in super::super) fn array_create_exact(
        &mut self,
        length: f64,
    ) -> Result<ObjectId, RuntimeError> {
        if length > MAX_ARRAY_LENGTH {
            return Err(RuntimeError::RangeError("invalid Array length".into()));
        }
        let prototype = self.array_create_prototype()?;
        self.with_roots(|heap| heap.alloc_array(length as u32, Some(prototype)))
    }

    fn array_length_of(&mut self, object: ObjectId) -> Result<f64, RuntimeError> {
        let length = self.get_property(&Value::Object(object), &"length".into())?;
        self.coerce_length(&length)
    }

    pub(in super::super) fn array_to_reversed(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        let result = (|| {
            let length = self.array_length_of(object)?;
            let target = self.array_create_exact(length)?;
            self.stack.push(Value::Object(target));
            let mark = self.stack.len();
            for index in 0..length as u64 {
                self.charge_step()?;
                let from: PropertyName = (length as u64 - index - 1).to_string().into();
                let value = self.get_property(&Value::Object(object), &from)?;
                self.stack.push(value.clone());
                self.array_create_data_property_or_throw(target, index.to_string().into(), value)?;
                self.stack.truncate(mark);
            }
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn array_to_sorted(
        &mut self,
        receiver: &Value,
        compare: &Value,
    ) -> Result<Value, RuntimeError> {
        let owner = self.argument_list_owner()?;
        let inputs = realm_call_storage::RealmCallRoots::new(3, owner.as_ref())?;
        [receiver, compare]
            .into_iter()
            .filter_map(Value::object_id)
            .try_for_each(|id| inputs.push(&self.heap, id))?;
        let result = self.with_realm_call_roots(inputs.clone(), |vm| {
            if *compare != Value::Undefined && !vm.is_callable(compare)? {
                return Err(RuntimeError::TypeError(
                    "Array toSorted comparator must be callable".into(),
                ));
            }
            let object = vm.coerce_object(receiver)?;
            if receiver.object_id().is_none() {
                inputs.push(&vm.heap, object)?;
            }
            let length = vm.get_property(&Value::Object(object), &"length".into())?;
            let length = native_value_storage::NativeValue::adopt(length, owner.as_ref())?;
            let count = length.with_value(vm, |vm, value| vm.coerce_length(value))?;
            drop(length);
            let target = vm.array_create_exact(count)?;
            inputs.push(&vm.heap, target)?;
            let collected = realm_call_storage::RealmCallRoots::new(0, owner.as_ref())?;
            vm.with_realm_call_roots(collected.clone(), |vm| {
                let mut values = vm.array_sort_input()?;
                let mut undefined = 0usize;
                for index in 0..count as usize {
                    vm.charge_step()?;
                    let key = Self::array_sort_key(index, owner.as_ref())?;
                    let value = vm.get_property(&Value::Object(object), &key.name)?;
                    if let Some(owner) = &owner {
                        owner.reserve_peer(0)?;
                    }
                    if value == Value::Undefined {
                        undefined += 1;
                    } else {
                        if let Some(object) = value.object_id() {
                            collected.push_admitted(&vm.heap, object)?;
                        }
                        vm.push_array_sort_input(&mut values, value)?;
                    }
                }
                let values = vm.array_sort_values(values, compare)?;
                collected.truncate(0);
                let mut remaining = 0usize;
                for object in values.iter().rev().filter_map(Value::object_id) {
                    collected.push(&vm.heap, object)?;
                    remaining += 1;
                }
                let mut values = values.into_owned_iter()?;
                let mut index = 0usize;
                while let Some(value) = values.next_owned()? {
                    let object_value = value.value().object_id().is_some();
                    vm.array_create_data_property_or_throw_owned(
                        target,
                        index.to_string().into(),
                        value,
                    )?;
                    if let Some(owner) = &owner {
                        owner.reserve_peer(0)?;
                    }
                    if object_value {
                        remaining -= 1;
                        collected.truncate(remaining);
                    }
                    index += 1;
                }
                drop(values);
                for _ in 0..undefined {
                    let value =
                        native_value_storage::NativeValue::adopt(Value::Undefined, owner.as_ref())?;
                    vm.array_create_data_property_or_throw_owned(
                        target,
                        index.to_string().into(),
                        value,
                    )?;
                    if let Some(owner) = &owner {
                        owner.reserve_peer(0)?;
                    }
                    index += 1;
                }
                Ok(Value::Object(target))
            })
        });
        if let Some(owner) = owner {
            owner.reserve_peer(0)?;
        }
        result
    }

    pub(in super::super) fn array_to_spliced(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        self.stack.extend(args.iter().cloned());
        let result = (|| {
            let length = self.array_length_of(object)?;
            let start = self.array_start_index(native::argument(args, 0), length as i64)?;
            let items = args.get(2..).unwrap_or_default();
            let skip = match args.len() {
                0 => 0,
                1 => length as i64 - start,
                _ => (self.coerce_length(native::argument(args, 1))? as i64)
                    .clamp(0, length as i64 - start),
            };
            let new_length = length + items.len() as f64 - skip as f64;
            if new_length > MAX_SAFE_INTEGER {
                return Err(RuntimeError::TypeError("invalid Array length".into()));
            }
            let target = self.array_create_exact(new_length)?;
            self.stack.push(Value::Object(target));
            let mark = self.stack.len();
            let mut next = 0u64;
            for index in 0..start as u64 {
                self.charge_step()?;
                let value = self.get_property(&Value::Object(object), &index.to_string().into())?;
                self.stack.push(value.clone());
                self.array_create_data_property_or_throw(target, next.to_string().into(), value)?;
                self.stack.truncate(mark);
                next += 1;
            }
            for item in items {
                self.array_create_data_property_or_throw(
                    target,
                    next.to_string().into(),
                    item.clone(),
                )?;
                next += 1;
            }
            let mut from = (start + skip) as u64;
            while (next as f64) < new_length {
                self.charge_step()?;
                let value = self.get_property(&Value::Object(object), &from.to_string().into())?;
                self.stack.push(value.clone());
                self.array_create_data_property_or_throw(target, next.to_string().into(), value)?;
                self.stack.truncate(mark);
                next += 1;
                from += 1;
            }
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn array_with(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let object = self.coerce_object(receiver)?;
        let base = self.stack.len();
        self.stack.push(Value::Object(object));
        self.stack.extend(args.iter().cloned());
        let result = (|| {
            let length = self.array_length_of(object)?;
            let relative = self.coerce_number(native::argument(args, 0))?;
            let relative = if relative.is_nan() {
                0.0
            } else {
                relative.trunc()
            };
            let actual = if relative >= 0.0 {
                relative
            } else {
                length + relative
            };
            if actual >= length || actual < 0.0 {
                return Err(RuntimeError::RangeError(
                    "Array.prototype.with index is out of range".into(),
                ));
            }
            let actual = actual as u64;
            let target = self.array_create_exact(length)?;
            self.stack.push(Value::Object(target));
            let mark = self.stack.len();
            for index in 0..length as u64 {
                self.charge_step()?;
                let value = if index == actual {
                    native::argument(args, 1).clone()
                } else {
                    self.get_property(&Value::Object(object), &index.to_string().into())?
                };
                self.stack.push(value.clone());
                self.array_create_data_property_or_throw(target, index.to_string().into(), value)?;
                self.stack.truncate(mark);
            }
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }

    /// Install `Array.prototype[@@unscopables]`: a null-prototype object whose
    /// [[Writable]] is false and [[Configurable]] is true, holding `true` for
    /// every name in [`ARRAY_UNSCOPABLES`].
    pub(in super::super) fn install_array_unscopables(&mut self) -> Result<(), RuntimeError> {
        let record = self.with_roots(|heap| heap.alloc_object(None))?;
        let base = self.stack.len();
        self.stack.push(Value::Object(record));
        let result = (|| {
            for name in ARRAY_UNSCOPABLES {
                self.define_data(record, name, Value::Bool(true), true, true, true)?;
            }
            self.define_data(
                self.array_prototype,
                JsSymbol::well_known("unscopables"),
                Value::Object(record),
                false,
                false,
                true,
            )
        })();
        self.stack.truncate(base);
        result
    }
}
