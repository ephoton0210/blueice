// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// The largest index an iterator-consuming built-in may reach, 2^53 - 1.
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

impl Vm {
    /// Steps `record` to completion, calling `visit(vm, value, index)` for
    /// each value, and returns how many values there were. Reaching index
    /// 2^53 - 1 is a TypeError carrying `too_large`. Every abrupt completion
    /// (from a step, from `visit`, or the index limit) closes the iterator
    /// while preserving that original completion.
    fn iterate_and_close(
        &mut self,
        record: &Value,
        too_large: &'static str,
        visit: &mut dyn FnMut(&mut Vm, Value, u64) -> Result<(), RuntimeError>,
    ) -> Result<u64, RuntimeError> {
        let outcome = self.iterate_values_from(record, 0, too_large, visit);
        if let Err(error) = &outcome {
            // The thrown value is only reachable from `outcome`, and
            // return() runs user code that can allocate and collect.
            let error_base = self.stack.len();
            if let RuntimeError::Thrown(value) = error {
                self.stack.push(value.clone());
            }
            // An IteratorStep abrupt completion has already marked the
            // record done, which makes IteratorClose a no-op in that case;
            // otherwise the original error wins over a close failure.
            let _ = self.iterator_close(record);
            self.stack.truncate(error_base);
        }
        outcome
    }

    /// The stepping loop of [`Self::iterate_and_close`], starting at `first`.
    fn iterate_values_from(
        &mut self,
        record: &Value,
        first: u64,
        too_large: &'static str,
        visit: &mut dyn FnMut(&mut Vm, Value, u64) -> Result<(), RuntimeError>,
    ) -> Result<u64, RuntimeError> {
        let mut index = first;
        loop {
            if index >= MAX_SAFE_INTEGER {
                return Err(RuntimeError::TypeError(too_large.into()));
            }
            let Some(value) = self.iterator_step(record, true)? else {
                return Ok(index);
            };
            visit(self, value, index)?;
            index += 1;
        }
    }

    /// Object.groupBy consumes its source as an iterator, rather than using
    /// array-like indexing. The callback result is converted to a property
    /// key before a group is created, and every callback/key/append abrupt
    /// completion closes the still-live iterator while preserving that
    /// original completion.
    pub(in super::super) fn object_group_by_method(
        &mut self,
        items: &Value,
        callback: &Value,
    ) -> Result<Value, RuntimeError> {
        // Only a handle to a collected object can make this fail, and
        // JavaScript cannot hold one, so such a handle is simply not callable.
        if !self.is_callable(callback).unwrap_or(false) {
            return Err(RuntimeError::TypeError(
                "Object.groupBy callback must be callable".into(),
            ));
        }
        let base = self.stack.len();
        self.stack.extend([items.clone(), callback.clone()]);
        let result = (|| {
            let record = self.get_iterator(items)?;
            self.stack.push(record.clone());
            let groups = self.with_roots(|heap| heap.alloc_object(None))?;
            self.stack.push(Value::Object(groups));
            self.iterate_and_close(
                &record,
                "Object.groupBy iterator is too large",
                &mut |vm, value, index| vm.object_group_by_step(callback, groups, value, index),
            )?;
            Ok(Value::Object(groups))
        })();
        self.stack.truncate(base);
        result
    }

    /// One element of Object.groupBy: calls the callback, converts its result
    /// to a property key and appends the element to that key's group.
    fn object_group_by_step(
        &mut self,
        callback: &Value,
        groups: ObjectId,
        value: Value,
        index: u64,
    ) -> Result<(), RuntimeError> {
        let item_base = self.stack.len();
        self.stack.push(value.clone());
        let key_value = self.call_native(
            callback.clone(),
            Value::Undefined,
            vec![value, Value::Number(index as f64)],
            false,
        )?;
        self.stack.push(key_value.clone());
        let key = self.coerce_property_key(&key_value)?;
        // `groups` is an ordinary object, so this cannot run a Proxy trap.
        let existing = self
            .object_get_own_property(groups, &key)
            .expect("an ordinary object's own property lookup cannot fail");
        let group = if let Some(descriptor) = existing {
            descriptor
                .value
                .expect("Object.groupBy groups are data properties")
        } else {
            let group = self.array_from(Vec::new())?;
            self.stack.push(group.clone());
            let defined = self.object_define_own_property(
                groups,
                key,
                PropertyDescriptor::data(group.clone(), true, true, true),
            )?;
            self.stack.pop();
            assert!(defined, "a group can always be added to the fresh result");
            group
        };
        self.stack.push(group.clone());
        let value = self.stack[item_base].clone();
        self.array_push(&group, &value, 0)?;
        self.stack.truncate(item_base);
        Ok(())
    }

    /// `Map.groupBy` (GroupBy with zero key coercion): like `Object.groupBy`
    /// it consumes an iterator and closes it on any abrupt completion, but
    /// keys keep their identity (only `-0` becomes `+0`) and the groups land
    /// in a fresh `%Map%` in first-seen order. Grouping straight into that
    /// Map is equivalent to the spec's separate group list: the Map is not
    /// observable until it is returned, and its SameValueZero lookup is the
    /// spec's SameValue on the zero-normalized keys.
    pub(in super::super) fn map_group_by_method(
        &mut self,
        items: &Value,
        callback: &Value,
    ) -> Result<Value, RuntimeError> {
        if matches!(items, Value::Undefined | Value::Null) {
            return Err(RuntimeError::TypeError(
                "Map.groupBy items must not be null or undefined".into(),
            ));
        }
        if !self.is_callable(callback).unwrap_or(false) {
            return Err(RuntimeError::TypeError(
                "Map.groupBy callback must be callable".into(),
            ));
        }
        let base = self.stack.len();
        self.stack.extend([items.clone(), callback.clone()]);
        let result = (|| {
            let record = self.get_iterator(items)?;
            self.stack.push(record.clone());
            // `Map.groupBy` is installed only after %Map.prototype% exists.
            let prototype = self
                .collection_prototype(true)
                .expect("%Map.prototype% exists before Map.groupBy does");
            let groups = self.with_roots(|heap| heap.alloc_map(Some(prototype)))?;
            self.stack.push(Value::Object(groups));
            self.iterate_and_close(
                &record,
                "Map.groupBy iterator is too large",
                &mut |vm, value, index| vm.map_group_by_step(callback, groups, value, index),
            )?;
            Ok(Value::Object(groups))
        })();
        self.stack.truncate(base);
        result
    }

    /// One element of Map.groupBy: calls the callback and appends the element
    /// to the group of the key it returned.
    fn map_group_by_step(
        &mut self,
        callback: &Value,
        groups: ObjectId,
        value: Value,
        index: u64,
    ) -> Result<(), RuntimeError> {
        let item_base = self.stack.len();
        self.stack.push(value.clone());
        let key = self.call_native(
            callback.clone(),
            Value::Undefined,
            vec![value.clone(), Value::Number(index as f64)],
            false,
        )?;
        self.stack.push(key.clone());
        let existing = self
            .heap
            .map_get(groups, &key)
            .expect("`groups` is a live Map");
        let group = match existing {
            Some(group) => group,
            None => {
                let group = self.array_from(Vec::new())?;
                self.stack.push(group.clone());
                self.with_roots(|heap| heap.map_set(groups, key, group.clone()))?;
                group
            }
        };
        self.stack.push(group.clone());
        self.array_push(&group, &value, 0)?;
        self.stack.truncate(item_base);
        Ok(())
    }

    pub(in super::super) fn object_from_entries_method(
        &mut self,
        source: &Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(source.clone());
        let result = (|| {
            let record = self.get_iterator(source)?;
            self.stack.push(record.clone());
            let prototype = self.object_prototype;
            let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
            self.stack.push(Value::Object(object));
            self.iterate_and_close(
                &record,
                "Object.fromEntries iterator is too large",
                &mut |vm, entry, _| vm.object_from_entries_step(object, entry),
            )?;
            Ok(Value::Object(object))
        })();
        self.stack.truncate(base);
        result
    }

    /// One entry of Object.fromEntries: reads the entry's key and value and
    /// defines the property.
    fn object_from_entries_step(
        &mut self,
        object: ObjectId,
        entry: Value,
    ) -> Result<(), RuntimeError> {
        let Value::Object(entry) = entry else {
            return Err(RuntimeError::TypeError(
                "Object.fromEntries entry must be an object".into(),
            ));
        };
        let entry_base = self.stack.len();
        self.stack.push(Value::Object(entry));
        let key_value = self.get_property(&Value::Object(entry), &"0".into())?;
        self.stack.push(key_value.clone());
        let value = self.get_property(&Value::Object(entry), &"1".into())?;
        self.stack.push(value.clone());
        let key = self.coerce_property_key(&key_value)?;
        let defined = self.object_define_own_property(
            object,
            key,
            PropertyDescriptor::data(value, true, true, true),
        )?;
        self.stack.truncate(entry_base);
        assert!(
            defined,
            "a property can always be added to the fresh result"
        );
        Ok(())
    }

    /// `Array.from(items, mapfn, thisArg)` with `this` as the constructor `C`
    /// (`Array.from` step 1). The iterator method is read before `C` is
    /// constructed, elements are defined with `CreateDataPropertyOrThrow`, and
    /// `length` is set at the end. A non-constructor `this` builds a plain
    /// Array, exactly as `Array.of` does.
    pub(in super::super) fn array_from_method(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let source = native::argument(args, 0).clone();
        if matches!(source, Value::Null | Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "Array.from requires an object".into(),
            ));
        }
        let mapper = native::argument(args, 1).clone();
        if mapper != Value::Undefined && !self.is_callable(&mapper).unwrap_or(false) {
            return Err(RuntimeError::TypeError(
                "Array.from mapper must be callable".into(),
            ));
        }
        let this_arg = native::argument(args, 2).clone();
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.push(source.clone());
        if mapper != Value::Undefined {
            self.stack.push(mapper.clone());
            self.stack.push(this_arg.clone());
        }
        let result = (|| {
            let iterator = self.get_method(&source, &JsSymbol::well_known("iterator").into())?;
            // A getter may have produced the method just now, so nothing but
            // this local holds it while `C` is constructed below.
            self.stack.push(iterator.clone());
            let constructor = self.is_constructor(receiver).unwrap_or(false);
            if iterator == Value::Undefined {
                // Each element is read, mapped and stored before the next
                // one is read (spec order), so a mapper result is reachable
                // from the rooted result array before any later mapper call
                // can allocate and collect.
                let object = self.coerce_object(&source)?;
                let object_value = Value::Object(object);
                self.stack.push(object_value.clone());
                let length = self.get_property(&object_value, &"length".into())?;
                let length = self.coerce_length(&length)?;
                let target = self.array_from_target(receiver, constructor, Some(length))?;
                self.stack.push(Value::Object(target));
                let mark = self.stack.len();
                for index in 0..length as u64 {
                    self.charge_step()?;
                    let value = self.get_property(&object_value, &index.to_string().into())?;
                    let value = if mapper == Value::Undefined {
                        value
                    } else {
                        self.stack.push(value.clone());
                        self.call_native(
                            mapper.clone(),
                            this_arg.clone(),
                            vec![value, Value::Number(index as f64)],
                            false,
                        )?
                    };
                    self.stack.push(value.clone());
                    self.array_create_data_property_or_throw(
                        target,
                        index.to_string().into(),
                        value,
                    )?;
                    self.stack.truncate(mark);
                }
                self.array_set_or_throw(target, "length".into(), &Value::Number(length))?;
                return Ok(Value::Object(target));
            }

            // Array.from maps one iterator value at a time. Collecting the
            // iterator first makes an infinite source consume its resource
            // budget before an abrupt mapper can close it, which is both
            // observably wrong and turns finite conformance checks into
            // timeouts.
            let target = self.array_from_target(receiver, constructor, None)?;
            self.stack.push(Value::Object(target));
            let record = self.get_iterator_from_method(&source, iterator)?;
            self.stack.push(record.clone());
            let count = self.iterate_and_close(
                &record,
                "Array.from result length is too large",
                &mut |vm, value, index| {
                    let mark = vm.stack.len();
                    let value = if mapper == Value::Undefined {
                        value
                    } else {
                        vm.stack.push(value.clone());
                        vm.call_native(
                            mapper.clone(),
                            this_arg.clone(),
                            vec![value, Value::Number(index as f64)],
                            false,
                        )?
                    };
                    vm.stack.push(value.clone());
                    vm.array_create_data_property_or_throw(
                        target,
                        index.to_string().into(),
                        value,
                    )?;
                    vm.stack.truncate(mark);
                    Ok(())
                },
            )?;
            self.array_set_or_throw(target, "length".into(), &Value::Number(count as f64))?;
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }

    /// The result object of `Array.from`: `Construct(C)` (or `Construct(C,
    /// «len»)` for an array-like) when `this` is a constructor, otherwise a
    /// plain Array of the current Realm.
    pub(in super::super) fn array_from_target(
        &mut self,
        receiver: &Value,
        constructor: bool,
        length: Option<f64>,
    ) -> Result<ObjectId, RuntimeError> {
        if !constructor {
            return self.array_create_exact(length.unwrap_or(0.0));
        }
        let args = length.map(Value::Number).into_iter().collect();
        Ok(self
            .call_with_target(
                receiver.clone(),
                Value::Undefined,
                args,
                true,
                receiver.clone(),
            )?
            .object_id()
            .expect("Construct returns an object"))
    }

    pub(in super::super) fn array_of_method(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.extend(args.iter().cloned());
        let result = (|| {
            let array = if self.is_constructor(receiver).unwrap_or(false) {
                self.call_with_target(
                    receiver.clone(),
                    Value::Undefined,
                    vec![Value::Number(args.len() as f64)],
                    true,
                    receiver.clone(),
                )?
                .object_id()
                .expect("Construct returns an object")
            } else {
                let prototype = self.array_prototype;
                self.with_roots(|heap| heap.alloc_array(0, Some(prototype)))?
            };
            self.stack.push(Value::Object(array));
            for (index, value) in args.iter().cloned().enumerate() {
                self.array_create_data_property_or_throw(array, index.to_string().into(), value)?;
            }
            self.array_set_or_throw(array, "length".into(), &Value::Number(args.len() as f64))?;
            Ok(Value::Object(array))
        })();
        self.stack.truncate(base);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iteration_refuses_to_pass_the_safe_integer_limit() {
        // Outside `execute` there is no instruction budget yet.
        let mut vm = Vm {
            remaining_instructions: 1_000,
            ..Vm::default()
        };
        let too_large = Err(RuntimeError::TypeError("too large".into()));
        let mut visited = Vec::new();
        for first in [MAX_SAFE_INTEGER - 1, MAX_SAFE_INTEGER] {
            let items = vm.array_from(vec![Value::Number(1.0)]).unwrap();
            let record = vm.get_iterator(&items).unwrap();
            let result = vm.iterate_values_from(&record, first, "too large", &mut |_, _, index| {
                visited.push(index);
                Ok(())
            });
            // The last index below the limit is still visited, and the limit
            // is enforced before the iterator is asked whether it has more.
            assert_eq!(result, too_large);
        }
        assert_eq!(visited, vec![MAX_SAFE_INTEGER - 1]);
    }
}
